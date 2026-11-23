//! Kassette ohne Mapper (32 KB ROM, optional 8 KB RAM).

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use super::{Mapper, Memory};

pub struct RomOnly {
    mem: Memory,
}

impl RomOnly {
    pub fn new(rom: Vec<u8>, ram_bytes: usize, battery: bool) -> Self {
        Self {
            mem: Memory::new(rom, ram_bytes.min(0x2000), battery),
        }
    }
}

impl Mapper for RomOnly {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.mem.rom_read(0, addr),
            0x4000..=0x7FFF => self.mem.rom_read(1, addr),
            _ => self.mem.ram_read(0, addr),
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        if addr >= 0xA000 {
            self.mem.ram_write(0, addr, v);
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
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.mem.ram = r.blob()?;
        Ok(())
    }
}
