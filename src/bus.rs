//! Speicherbus: Adressraum, I/O-Register und Takten der Peripherie.
//!
//! Jeder Zugriff der CPU ist ein M-Zyklus: Zuerst laufen Timer, PPU, Serielle Schnittstelle und
//! OAM-DMA einen M-Zyklus weiter, dann erfolgt der Zugriff.

use kupocode_emulator_core::debug::{Access, Breakpoints};
use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use crate::apu::Apu;
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

/// Register des CGB außerhalb der PPU.
#[derive(Debug, Clone, Default)]
pub struct CgbRegs {
    /// Doppelter Takt aktiv (`KEY1` Bit 7).
    pub double_speed: bool,
    /// Umschaltung vorbereitet (`KEY1` Bit 0), ausgeführt mit `STOP`.
    pub key1_armed: bool,
    pub hdma_src: u16,
    pub hdma_dst: u16,
    /// Verbleibende 16-Byte-Blöcke der HBlank-DMA.
    pub hdma_blocks: u8,
    pub hdma_active: bool,
    /// Wert von `HDMA5` nach Ende oder Abbruch.
    pub hdma5: u8,
    /// Infrarot-Schnittstelle `RP` (Bits 0, 6, 7).
    pub rp: u8,
    /// Undokumentierte Register `FF72`–`FF75`.
    pub undoc: [u8; 4],
}

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
    pub apu: Apu,
    /// Letzter Wert von `FF46`.
    pub dma_reg: u8,
    /// Laufende OAM-DMA: nächstes Byte (0–159), `None` = keine.
    pub dma_index: Option<u8>,
    /// Verzögerung bis zum Start der OAM-DMA in M-Zyklen.
    pub dma_delay: u8,
    /// Eingeblendetes Boot-ROM.
    pub boot_rom: Option<Vec<u8>>,
    pub cgb: CgbRegs,
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
        let mut ppu = Ppu::default();
        ppu.color.cgb = model.is_cgb();
        ppu.color.cgb_mode = model.is_cgb();
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
            ppu,
            apu: Apu::new(model.is_cgb()),
            dma_reg: 0,
            dma_index: None,
            dma_delay: 0,
            boot_rom: None,
            cgb: CgbRegs {
                hdma5: 0xFF,
                ..CgbRegs::default()
            },
            cycles: 0,
            breakpoints: Breakpoints::with_mask(0xFFFF),
            watch_hit: None,
        }
    }

    /// CGB-Funktionen aktiv (CGB-Spiel auf CGB)?
    pub fn cgb_mode(&self) -> bool {
        self.ppu.color.cgb_mode
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
            0xFF10..=0xFF3F => self.apu.read(addr),
            0xFF46 => self.dma_reg,
            0xFF40..=0xFF4B => self.ppu.read(addr),
            0xFF4D if self.cgb_mode() => {
                0x7E | u8::from(self.cgb.double_speed) << 7 | u8::from(self.cgb.key1_armed)
            }
            0xFF4F if self.cgb_mode() => 0xFE | self.ppu.vram_bank,
            0xFF55 if self.cgb_mode() => {
                if self.cgb.hdma_active {
                    self.cgb.hdma_blocks.wrapping_sub(1) & 0x7F
                } else {
                    self.cgb.hdma5
                }
            }
            0xFF56 if self.model.is_cgb() => self.cgb.rp | 0x3E,
            0xFF68..=0xFF6C => self.ppu.read(addr),
            0xFF70 if self.cgb_mode() => 0xF8 | self.wram_bank,
            0xFF72 | 0xFF73 if self.model.is_cgb() => self.cgb.undoc[usize::from(addr - 0xFF72)],
            0xFF74 if self.cgb_mode() => self.cgb.undoc[2],
            0xFF75 if self.model.is_cgb() => self.cgb.undoc[3] | 0x8F,
            0xFF76 | 0xFF77 if self.model.is_cgb() => {
                // PCM12/PCM34: digitale Ausgänge der Kanäle
                let o = self.apu.outputs();
                let i = usize::from(addr - 0xFF76) * 2;
                o[i] | o[i + 1] << 4
            }
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
            0xFF04 => {
                // Zurücksetzen von DIV kann eine fallende Flanke für den Frame-Sequencer sein.
                if self.timer.counter & self.apu_div_bit() != 0 {
                    self.apu.frame_step();
                }
                self.timer.write(addr, v);
            }
            0xFF05..=0xFF07 => self.timer.write(addr, v),
            0xFF0F => self.if_ = v & 0x1F,
            0xFF10..=0xFF3F => self.apu.write(addr, v),
            0xFF46 => {
                self.dma_reg = v;
                self.dma_delay = 2;
            }
            0xFF40..=0xFF4B => self.ppu.write(addr, v),
            0xFF4C if self.boot_rom.is_some() && self.model.is_cgb() => {
                // KEY0: Das Boot-ROM schaltet DMG-Spiele in den Kompatibilitätsmodus.
                self.ppu.color.cgb_mode = v & 0x04 == 0;
            }
            0xFF4D if self.cgb_mode() => self.cgb.key1_armed = v & 1 != 0,
            0xFF4F if self.cgb_mode() => self.ppu.vram_bank = v & 1,
            0xFF51 if self.cgb_mode() => {
                self.cgb.hdma_src = (self.cgb.hdma_src & 0x00F0) | u16::from(v) << 8
            }
            0xFF52 if self.cgb_mode() => {
                self.cgb.hdma_src = (self.cgb.hdma_src & 0xFF00) | u16::from(v & 0xF0)
            }
            0xFF53 if self.cgb_mode() => {
                self.cgb.hdma_dst = (self.cgb.hdma_dst & 0x00F0) | u16::from(v & 0x1F) << 8
            }
            0xFF54 if self.cgb_mode() => {
                self.cgb.hdma_dst = (self.cgb.hdma_dst & 0x1F00) | u16::from(v & 0xF0)
            }
            0xFF55 if self.cgb_mode() => self.write_hdma5(v),
            0xFF56 if self.model.is_cgb() => self.cgb.rp = v & 0xC1,
            0xFF68..=0xFF6C => self.ppu.write(addr, v),
            0xFF72 | 0xFF73 if self.model.is_cgb() => {
                self.cgb.undoc[usize::from(addr - 0xFF72)] = v
            }
            0xFF74 if self.cgb_mode() => self.cgb.undoc[2] = v,
            0xFF75 if self.model.is_cgb() => self.cgb.undoc[3] = v & 0x70,
            0xFF50 => {
                if v != 0 {
                    self.boot_rom = None;
                }
            }
            0xFF70 if self.cgb_mode() => self.wram_bank = v & 7,
            0xFF80..=0xFFFE => self.hram[usize::from(addr - 0xFF80)] = v,
            0xFFFF => self.ie = v,
            _ => {}
        }
    }

    /// `HDMA5`: Bit 7 = 0 startet eine allgemeine DMA (sofort, CPU steht) bzw. bricht eine
    /// laufende HBlank-DMA ab; Bit 7 = 1 startet eine HBlank-DMA.
    fn write_hdma5(&mut self, v: u8) {
        let blocks = (v & 0x7F) + 1;
        if self.cgb.hdma_active && v & 0x80 == 0 {
            self.cgb.hdma_active = false;
            self.cgb.hdma5 = 0x80 | (self.cgb.hdma_blocks.wrapping_sub(1) & 0x7F);
            return;
        }
        self.cgb.hdma_blocks = blocks;
        if v & 0x80 != 0 {
            self.cgb.hdma_active = true;
            // Im HBlank oder bei ausgeschaltetem LCD wird sofort ein Block übertragen.
            if !self.ppu.enabled() || self.ppu.mode() == 0 {
                self.hdma_block();
            }
        } else {
            while self.cgb.hdma_blocks > 0 {
                self.copy_block();
            }
            self.cgb.hdma5 = 0xFF;
        }
    }

    /// Überträgt 16 Bytes; die CPU steht dabei 8 M-Zyklen (bei doppeltem Takt 16).
    fn copy_block(&mut self) {
        for i in 0..16u16 {
            let v = self.peek(self.cgb.hdma_src.wrapping_add(i));
            let dst = 0x8000 | (self.cgb.hdma_dst.wrapping_add(i) & 0x1FFF);
            let idx = self.ppu.vram_index(dst);
            self.ppu.vram[idx] = v;
        }
        self.cgb.hdma_src = self.cgb.hdma_src.wrapping_add(16);
        self.cgb.hdma_dst = (self.cgb.hdma_dst + 16) & 0x1FF0;
        self.cgb.hdma_blocks -= 1;
        let stall = if self.cgb.double_speed { 16 } else { 8 };
        for _ in 0..stall {
            self.tick_hardware();
        }
    }

    fn hdma_block(&mut self) {
        self.copy_block();
        if self.cgb.hdma_blocks == 0 {
            self.cgb.hdma_active = false;
            self.cgb.hdma5 = 0xFF;
        }
    }

    /// Bit des Systemzählers, dessen fallende Flanke den Frame-Sequencer weiterschaltet
    /// (`DIV` Bit 4, bei doppeltem Takt Bit 5).
    fn apu_div_bit(&self) -> u16 {
        if self.cgb.double_speed {
            1 << 13
        } else {
            1 << 12
        }
    }

    /// Ein M-Zyklus der Peripherie samt HBlank-DMA.
    fn step_peripherals(&mut self) {
        self.tick_hardware();
        if self.ppu.hblank {
            self.ppu.hblank = false;
            if self.cgb.hdma_active {
                self.hdma_block();
            }
        }
    }

    /// Ein M-Zyklus der Peripherie. Timer und Serielle laufen im CPU-Takt, PPU und Uhr der
    /// Kassette im Grundtakt (bei doppeltem Takt also 2 Punkte je M-Zyklus).
    fn tick_hardware(&mut self) {
        let dots = if self.cgb.double_speed { 2 } else { 4 };
        self.cycles += u64::from(dots);
        let old = self.timer.counter;
        if self.timer.tick() {
            self.if_ |= IRQ_TIMER;
        }
        if self.serial.tick(old, self.timer.counter) {
            self.if_ |= IRQ_SERIAL;
        }
        let bit = self.apu_div_bit();
        if old & bit != 0 && self.timer.counter & bit == 0 {
            self.apu.frame_step();
        }
        self.apu.tick(dots);
        if self.joypad.tick() {
            self.if_ |= IRQ_JOYPAD;
        }
        self.if_ |= self.ppu.tick(dots);
        if let Some(c) = self.cart.as_mut() {
            c.tick(dots);
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

    /// Ist der Zugriff der CPU gesperrt (OAM während DMA oder Mode 2/3, VRAM in Mode 3)?
    fn blocked(&self, addr: u16) -> bool {
        match addr {
            0x8000..=0x9FFF => !self.ppu.vram_accessible(),
            0xFE00..=0xFEFF => self.dma_index.is_some() || !self.ppu.oam_accessible(),
            _ => false,
        }
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
        self.apu.save_state(w);
        w.u8(self.dma_reg);
        w.option(self.dma_index.as_ref(), |w, &i| w.u8(i));
        w.u8(self.dma_delay);
        w.bool(self.boot_rom.is_some());
        let c = &self.cgb;
        w.bool(c.double_speed);
        w.bool(c.key1_armed);
        w.u16(c.hdma_src);
        w.u16(c.hdma_dst);
        w.u8(c.hdma_blocks);
        w.bool(c.hdma_active);
        w.u8(c.hdma5);
        w.u8(c.rp);
        w.bytes(&c.undoc);
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
        self.apu.load_state(r)?;
        self.dma_reg = r.u8()?;
        self.dma_index = r.option(|r| r.u8())?;
        self.dma_delay = r.u8()?;
        if !r.bool()? {
            self.boot_rom = None;
        }
        let c = &mut self.cgb;
        c.double_speed = r.bool()?;
        c.key1_armed = r.bool()?;
        c.hdma_src = r.u16()?;
        c.hdma_dst = r.u16()?;
        c.hdma_blocks = r.u8()?;
        c.hdma_active = r.bool()?;
        c.hdma5 = r.u8()?;
        c.rp = r.u8()?;
        r.bytes_into(&mut c.undoc)?;
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
        if self.blocked(addr) {
            return 0xFF;
        }
        self.peek(addr)
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.step_peripherals();
        self.check_watch(addr, Access::Write);
        if self.blocked(addr) {
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

    /// `STOP`: Auf dem CGB mit vorbereitetem `KEY1` wird der Takt umgeschaltet. Die CPU steht
    /// dabei 2050 M-Zyklen; `DIV` wird zurückgesetzt.
    fn stop(&mut self) {
        if self.cgb_mode() && self.cgb.key1_armed {
            self.cgb.key1_armed = false;
            self.cgb.double_speed = !self.cgb.double_speed;
            self.timer.counter = 0;
            for _ in 0..2050 {
                self.tick_hardware();
            }
        }
    }
}
