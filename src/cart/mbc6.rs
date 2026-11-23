//! MBC6 (Net de Get): zwei 8-KB-Fenster für ROM oder Flash, zwei 4-KB-Fenster für RAM.
//!
//! Register: `0000` RAM frei, `0400`/`0800` RAM-Bank A/B, `0C00` Flash frei, `1000` Flash
//! beschreibbar, `2000`/`2800` Bank und Quelle (ROM/Flash) für Fenster A (`4000`–`5FFF`),
//! `3000`/`3800` für Fenster B (`6000`–`7FFF`).
//!
//! Vereinfachung: Der Flash (1 MB) ist bei gesetzter Schreibfreigabe direkt beschreibbar; das
//! Befehlsprotokoll des Flash-Bausteins ist nicht nachgebildet.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use super::{Mapper, Memory};

const FLASH_SIZE: usize = 0x10_0000;

pub struct Mbc6 {
    mem: Memory,
    flash: Vec<u8>,
    ram_enabled: bool,
    ram_bank: [u8; 2],
    flash_enabled: bool,
    flash_write: bool,
    bank: [u8; 2],
    from_flash: [bool; 2],
}

impl Mbc6 {
    pub fn new(rom: Vec<u8>, ram_bytes: usize, battery: bool) -> Self {
        Self {
            mem: Memory::new(rom, ram_bytes.max(0x8000), battery),
            flash: vec![0xFF; FLASH_SIZE],
            ram_enabled: false,
            ram_bank: [0; 2],
            flash_enabled: false,
            flash_write: false,
            bank: [0; 2],
            from_flash: [false; 2],
        }
    }

    fn window(addr: u16) -> usize {
        usize::from(addr & 0x2000 != 0)
    }

    fn rom8(&self, w: usize, addr: u16) -> u8 {
        let off = usize::from(self.bank[w]) * 0x2000 + usize::from(addr & 0x1FFF);
        if self.from_flash[w] && self.flash_enabled {
            self.flash[off % FLASH_SIZE]
        } else {
            self.mem.rom[off % self.mem.rom.len()]
        }
    }

    fn ram_index(&self, addr: u16) -> usize {
        let w = usize::from(addr & 0x1000 != 0);
        (usize::from(self.ram_bank[w]) * 0x1000 + usize::from(addr & 0x0FFF)) % self.mem.ram.len()
    }
}

impl Mapper for Mbc6 {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.mem.rom[usize::from(addr)],
            0x4000..=0x7FFF => self.rom8(Self::window(addr), addr),
            _ if self.ram_enabled => self.mem.ram[self.ram_index(addr)],
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x03FF => self.ram_enabled = v & 0x0F == 0x0A,
            0x0400..=0x07FF => self.ram_bank[0] = v & 7,
            0x0800..=0x0BFF => self.ram_bank[1] = v & 7,
            0x0C00..=0x0FFF => self.flash_enabled = v & 1 != 0,
            0x1000..=0x1FFF => self.flash_write = v & 1 != 0,
            0x2000..=0x27FF => self.bank[0] = v & 0x7F,
            0x2800..=0x2FFF => self.from_flash[0] = v == 0x08,
            0x3000..=0x37FF => self.bank[1] = v & 0x7F,
            0x3800..=0x3FFF => self.from_flash[1] = v == 0x08,
            0x4000..=0x7FFF => {
                let w = Self::window(addr);
                if self.from_flash[w] && self.flash_enabled && self.flash_write {
                    let off = usize::from(self.bank[w]) * 0x2000 + usize::from(addr & 0x1FFF);
                    self.flash[off % FLASH_SIZE] = v;
                }
            }
            _ if self.ram_enabled => {
                let i = self.ram_index(addr);
                self.mem.ram[i] = v;
            }
            _ => {}
        }
    }

    fn bank_at(&self, addr: u16) -> usize {
        if addr < 0x4000 {
            0
        } else {
            usize::from(self.bank[Self::window(addr)]) / 2
        }
    }

    fn has_battery(&self) -> bool {
        self.mem.battery
    }

    /// Batterie-RAM: RAM, danach der Flash.
    fn battery_ram(&self, _now: u64) -> Vec<u8> {
        if !self.mem.battery {
            return Vec::new();
        }
        let mut out = self.mem.ram.clone();
        out.extend_from_slice(&self.flash);
        out
    }

    fn load_battery_ram(&mut self, data: &[u8]) -> Option<u64> {
        let n = self.mem.ram.len();
        self.mem.load_battery_ram(data);
        if data.len() > n {
            let f = &data[n..];
            let m = f.len().min(FLASH_SIZE);
            self.flash[..m].copy_from_slice(&f[..m]);
        }
        None
    }

    fn save_state(&self, w: &mut StateWriter) {
        w.blob(&self.mem.ram);
        w.blob(&self.flash);
        for v in [
            self.ram_enabled,
            self.flash_enabled,
            self.flash_write,
            self.from_flash[0],
            self.from_flash[1],
        ] {
            w.bool(v);
        }
        for v in [
            self.ram_bank[0],
            self.ram_bank[1],
            self.bank[0],
            self.bank[1],
        ] {
            w.u8(v);
        }
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.mem.ram = r.blob()?;
        self.flash = r.blob()?;
        self.ram_enabled = r.bool()?;
        self.flash_enabled = r.bool()?;
        self.flash_write = r.bool()?;
        self.from_flash = [r.bool()?, r.bool()?];
        self.ram_bank = [r.u8()?, r.u8()?];
        self.bank = [r.u8()?, r.u8()?];
        Ok(())
    }
}
