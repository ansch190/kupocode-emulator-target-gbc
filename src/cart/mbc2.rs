//! MBC2 (bis 256 KB ROM, eingebautes RAM 512 × 4 Bit).
//!
//! Im Bereich `0000`–`3FFF` entscheidet Adressbit 8: 0 = RAM freigeben, 1 = ROM-Bank (4 Bit).
//! Das RAM spiegelt sich über `A000`–`BFFF`; die oberen vier Bits lesen sich als 1.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use super::{Mapper, Memory};

pub struct Mbc2 {
    mem: Memory,
    ram_enabled: bool,
    bank: u8,
}

impl Mbc2 {
    pub fn new(rom: Vec<u8>, battery: bool) -> Self {
        Self {
            mem: Memory::new(rom, 512, battery),
            ram_enabled: false,
            bank: 1,
        }
    }
}

impl Mapper for Mbc2 {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.mem.rom_read(0, addr),
            0x4000..=0x7FFF => self.mem.rom_read(usize::from(self.bank), addr),
            _ if self.ram_enabled => 0xF0 | self.mem.ram[usize::from(addr & 0x1FF)],
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x3FFF if addr & 0x100 == 0 => self.ram_enabled = v & 0x0F == 0x0A,
            0x0000..=0x3FFF => self.bank = (v & 0x0F).max(1),
            0xA000..=0xBFFF if self.ram_enabled => {
                self.mem.ram[usize::from(addr & 0x1FF)] = v & 0x0F
            }
            _ => {}
        }
    }

    fn bank_at(&self, addr: u16) -> usize {
        if addr < 0x4000 {
            0
        } else {
            usize::from(self.bank) % self.mem.rom_banks()
        }
    }

    fn has_battery(&self) -> bool {
        self.mem.battery
    }

    fn battery_ram(&self, _now: u64) -> Vec<u8> {
        self.mem.battery_ram()
    }

    fn load_battery_ram(&mut self, data: &[u8]) -> Option<u64> {
        self.mem.load_battery_ram(data);
        None
    }

    fn save_state(&self, w: &mut StateWriter) {
        w.blob(&self.mem.ram);
        w.bool(self.ram_enabled);
        w.u8(self.bank);
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.mem.ram = r.blob()?;
        self.ram_enabled = r.bool()?;
        self.bank = r.u8()?;
        Ok(())
    }
}
