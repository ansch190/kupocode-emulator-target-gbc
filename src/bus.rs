//! Speicherbus: Adressraum, I/O-Register und Takten der Peripherie.
//!
//! Jeder Zugriff der CPU ist ein M-Zyklus: Zuerst laufen Timer, PPU, Serielle Schnittstelle und
//! OAM-DMA einen M-Zyklus weiter, dann erfolgt der Zugriff.

use kupocode_emulator_core::debug::{Access, Breakpoints};
use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use crate::cart::Mapper;
use crate::cpu::CpuBus;
use crate::joypad::Joypad;
use crate::model::Model;
use crate::ppu::Ppu;
use crate::serial::Serial;
use crate::timer::Timer;

/// Unterbrechungsbits in `IF`/`IE`.
pub const IRQ_TIMER: u8 = 0x04;
pub const IRQ_SERIAL: u8 = 0x08;
pub const IRQ_JOYPAD: u8 = 0x10;

pub struct Bus {
    pub model: Model,
    pub cart: Option<Box<dyn Mapper>>,
    /// Arbeitsspeicher, 8 Bänke à 4 KB (Bänke 2–7 nur CGB).
    pub wram: Vec<u8>,
    pub wram_bank: u8,
    pub hram: [u8; 127],
    pub ie: u8,
    pub if_: u8,
    pub timer: Timer,
    pub serial: Serial,
    pub joypad: Joypad,
    pub ppu: Ppu,
    /// Register der APU (`FF10`–`FF3F`), bis zur Tonausgabe unverändert gespeichert.
    pub apu_regs: [u8; 0x30],
    /// Letzter Wert von `FF46`.
    pub dma_reg: u8,
    /// Laufende OAM-DMA: nächstes Byte (0–159), `None` = keine.
    pub dma_index: Option<u8>,
    /// Verzögerung bis zum Start der OAM-DMA in M-Zyklen.
    pub dma_delay: u8,
    /// Eingeblendetes Boot-ROM.
    pub boot_rom: Option<Vec<u8>>,
    /// T-Zyklen seit dem Einschalten.
    pub cycles: u64,
    pub breakpoints: Breakpoints,
    /// Ausgelöster Watchpoint (Adresse, Zugriff).
    pub watch_hit: Option<(u32, Access)>,
}

impl Bus {
    pub fn new(model: Model, cart: Option<Box<dyn Mapper>>) -> Self {
        let mut serial = Serial::default();
        serial.cgb = model.is_cgb();
        Self {
            model,
            cart,
            wram: vec![0; 0x8000],
            wram_bank: 1,
            hram: [0; 127],
            ie: 0,
            if_: 0,
            timer: Timer::default(),
            serial,
            joypad: Joypad::default(),
            ppu: Ppu::default(),
            apu_regs: [0; 0x30],
            dma_reg: 0,
            dma_index: None,
            dma_delay: 0,
            boot_rom: None,
            cycles: 0,
            breakpoints: Breakpoints::with_mask(0xFFFF),
            watch_hit: None,
        }
    }

    fn wram_index(&self, addr: u16) -> usize {
        let a = usize::from(addr & 0x0FFF);
        if addr & 0x1000 == 0 {
            a
        } else {
            usize::from(self.wram_bank.max(1)) * 0x1000 + a
        }
    }

    fn boot_mapped(&self, addr: u16) -> Option<u8> {
        let boot = self.boot_rom.as_ref()?;
        let a = usize::from(addr);
        if a < 0x100 || ((0x200..0x900).contains(&a) && boot.len() > 0x100) {
            boot.get(a).copied()
        } else {
            None
        }
    }

    /// Liest ohne Seiteneffekte (Debugger, Disassembler, DMA).
    pub fn peek(&self, addr: u16) -> u8 {
        if let Some(v) = self.boot_mapped(addr) {
            return v;
        }
        match addr {
            0x0000..=0x7FFF | 0xA000..=0xBFFF => self.cart.as_ref().map_or(0xFF, |c| c.read(addr)),
            0x8000..=0x9FFF => self.ppu.vram[self.ppu.vram_index(addr)],
            0xC000..=0xFDFF => self.wram[self.wram_index(addr)],
            0xFE00..=0xFE9F => self.ppu.oam[usize::from(addr - 0xFE00)],
            0xFEA0..=0xFEFF => 0x00,
            0xFF00 => self.joypad.read(),
            0xFF01 | 0xFF02 => self.serial.read(addr),
            0xFF04..=0xFF07 => self.timer.read(addr),
            0xFF0F => self.if_ | 0xE0,
            0xFF10..=0xFF3F => self.apu_regs[usize::from(addr - 0xFF10)],
            0xFF46 => self.dma_reg,
            0xFF40..=0xFF4B => self.ppu.read(addr),
            0xFF4F if self.model.is_cgb() => 0xFE | self.ppu.vram_bank,
            0xFF70 if self.model.is_cgb() => 0xF8 | self.wram_bank,
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)],
            0xFFFF => self.ie,
            _ => 0xFF,
        }
    }

    /// Schreibt ohne Takt (Debugger, DMA-Ziel, interne Abläufe).
    pub fn poke(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x7FFF | 0xA000..=0xBFFF => {
                if let Some(c) = self.cart.as_mut() {
                    c.write(addr, v);
                }
            }
            0x8000..=0x9FFF => {
                let i = self.ppu.vram_index(addr);
                self.ppu.vram[i] = v;
            }
            0xC000..=0xFDFF => {
                let i = self.wram_index(addr);
                self.wram[i] = v;
            }
            0xFE00..=0xFE9F => self.ppu.oam[usize::from(addr - 0xFE00)] = v,
            0xFF00 => self.joypad.write(v),
            0xFF01 | 0xFF02 => self.serial.write(addr, v),
            0xFF04..=0xFF07 => self.timer.write(addr, v),
            0xFF0F => self.if_ = v & 0x1F,
            0xFF10..=0xFF3F => self.apu_regs[usize::from(addr - 0xFF10)] = v,
            0xFF46 => {
                self.dma_reg = v;
                self.dma_delay = 2;
            }
            0xFF40..=0xFF4B => self.ppu.write(addr, v),
            0xFF4F if self.model.is_cgb() => self.ppu.vram_bank = v & 1,
            0xFF50 => {
                if v != 0 {
                    self.boot_rom = None;
                }
            }
            0xFF70 if self.model.is_cgb() => self.wram_bank = v & 7,
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)] = v,
            0xFFFF => self.ie = v,
            _ => {}
        }
    }

    /// Ein M-Zyklus der Peripherie.
    fn step_peripherals(&mut self) {
        self.cycles += 4;
        let old = self.timer.counter;
        if self.timer.tick() {
            self.if_ |= IRQ_TIMER;
        }
        if self.serial.tick(old, self.timer.counter) {
            self.if_ |= IRQ_SERIAL;
        }
        if self.joypad.tick() {
            self.if_ |= IRQ_JOYPAD;
        }
        self.if_ |= self.ppu.tick(4);
        if let Some(c) = self.cart.as_mut() {
            c.tick(4);
        }
        self.step_dma();
    }

    fn step_dma(&mut self) {
        if let Some(i) = self.dma_index {
            let src = u16::from_be_bytes([self.dma_reg, i]);
            let src = if src >= 0xE000 { src - 0x2000 } else { src };
            self.ppu.oam[usize::from(i)] = self.peek(src);
            self.dma_index = if i == 159 { None } else { Some(i + 1) };
        }
        if self.dma_delay > 0 {
            self.dma_delay -= 1;
            if self.dma_delay == 0 {
                self.dma_index = Some(0);
            }
        }
    }

    fn dma_blocks(&self, addr: u16) -> bool {
        self.dma_index.is_some() && (0xFE00..=0xFEFF).contains(&addr)
    }

    fn check_watch(&mut self, addr: u16, access: Access) {
        if self.breakpoints.has_watch()
            && self.watch_hit.is_none()
            && self
                .breakpoints
                .check_access(u32::from(addr), 1, access)
                .is_some()
        {
            self.watch_hit = Some((u32::from(addr), access));
        }
    }

    pub fn save_state(&self, w: &mut StateWriter) {
        w.blob(&self.wram);
        w.u8(self.wram_bank);
        w.bytes(&self.hram);
        w.u8(self.ie);
        w.u8(self.if_);
        self.timer.save_state(w);
        self.serial.save_state(w);
        self.joypad.save_state(w);
        self.ppu.save_state(w);
        w.bytes(&self.apu_regs);
        w.u8(self.dma_reg);
        w.option(self.dma_index.as_ref(), |w, &i| w.u8(i));
        w.u8(self.dma_delay);
        w.bool(self.boot_rom.is_some());
        w.u64(self.cycles);
        if let Some(c) = &self.cart {
            c.save_state(w);
        }
    }

    /// Lädt den Zustand; ein eingeblendetes Boot-ROM muss bereits geladen sein.
    pub fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.wram = r.blob()?;
        if self.wram.len() != 0x8000 {
            return Err(StateError::Invalid("WRAM-Größe".into()));
        }
        self.wram_bank = r.u8()?;
        r.bytes_into(&mut self.hram)?;
        self.ie = r.u8()?;
        self.if_ = r.u8()?;
        self.timer.load_state(r)?;
        self.serial.load_state(r)?;
        self.joypad.load_state(r)?;
        self.ppu.load_state(r)?;
        r.bytes_into(&mut self.apu_regs)?;
        self.dma_reg = r.u8()?;
        self.dma_index = r.option(|r| r.u8())?;
        self.dma_delay = r.u8()?;
        if !r.bool()? {
            self.boot_rom = None;
        }
        self.cycles = r.u64()?;
        if let Some(c) = &mut self.cart {
            c.load_state(r)?;
        }
        Ok(())
    }
}

impl CpuBus for Bus {
    fn read(&mut self, addr: u16) -> u8 {
        self.step_peripherals();
        self.check_watch(addr, Access::Read);
        if self.dma_blocks(addr) {
            return 0xFF;
        }
        self.peek(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.step_peripherals();
        self.check_watch(addr, Access::Write);
        if self.dma_blocks(addr) {
            return;
        }
        self.poke(addr, value);
    }

    fn tick(&mut self) {
        self.step_peripherals();
    }

    fn pending(&self) -> u8 {
        self.ie & self.if_ & 0x1F
    }

    fn acknowledge(&mut self, bit: u8) {
        self.if_ &= !(1 << bit);
    }
}
