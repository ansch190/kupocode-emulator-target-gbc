//! MBC5 (bis 8 MB ROM, 128 KB RAM, optional Rumble-Motor).
//!
//! Die ROM-Bank hat 9 Bit (Bank 0 ist auch in `4000`–`7FFF` wählbar). Bei Rumble-Kassetten
//! steuert Bit 3 der RAM-Bank den Motor und fehlt als Bankbit.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use super::{Mapper, Memory};

pub struct Mbc5 {
    mem: Memory,
    ram_enabled: bool,
    rom_bank: u16,
    ram_bank: u8,
    has_rumble: bool,
}

impl Mbc5 {
    pub fn new(rom: Vec<u8>, ram_bytes: usize, battery: bool, rumble: bool) -> Self {
        Self {
            mem: Memory::new(rom, ram_bytes, battery),
            ram_enabled: false,
            rom_bank: 1,
            ram_bank: 0,
            has_rumble: rumble,
        }
    }

    fn ram_bank_index(&self) -> usize {
        let mask = if self.has_rumble { 0x07 } else { 0x0F };
        usize::from(self.ram_bank & mask)
    }
}

impl Mapper for Mbc5 {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.mem.rom_read(0, addr),
            0x4000..=0x7FFF => self.mem.rom_read(usize::from(self.rom_bank), addr),
            _ if self.ram_enabled => self.mem.ram_read(self.ram_bank_index(), addr),
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enabled = v == 0x0A,
            0x2000..=0x2FFF => self.rom_bank = (self.rom_bank & 0x100) | u16::from(v),
            0x3000..=0x3FFF => self.rom_bank = (self.rom_bank & 0xFF) | (u16::from(v & 1) << 8),
            0x4000..=0x5FFF => self.ram_bank = v & 0x0F,
            0x6000..=0x7FFF => {}
            _ if self.ram_enabled => {
                let bank = self.ram_bank_index();
                self.mem.ram_write(bank, addr, v);
            }
            _ => {}
        }
    }

    fn bank_at(&self, addr: u16) -> usize {
        if addr < 0x4000 {
            0
        } else {
            usize::from(self.rom_bank) % self.mem.rom_banks()
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

    fn rumble(&self) -> bool {
        self.has_rumble && self.ram_bank & 0x08 != 0
    }

    fn save_state(&self, w: &mut StateWriter) {
        w.blob(&self.mem.ram);
        w.bool(self.ram_enabled);
        w.u16(self.rom_bank);
        w.u8(self.ram_bank);
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.mem.ram = r.blob()?;
        self.ram_enabled = r.bool()?;
        self.rom_bank = r.u16()?;
        self.ram_bank = r.u8()?;
        Ok(())
    }
}
