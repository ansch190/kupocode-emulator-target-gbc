//! HuC1 (Hudson): wie MBC1 mit 6-Bit-ROM-Bank, dazu eine Infrarot-Schnittstelle.
//!
//! `0000`–`1FFF`: `0x0E` schaltet `A000`–`BFFF` auf Infrarot, sonst auf RAM. Infrarot ist ein
//! Stub: Es wird nie Licht empfangen (`0xC0`), Senden hat keine Wirkung.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use super::{Mapper, Memory};

pub struct HuC1 {
    mem: Memory,
    ir_mode: bool,
    rom_bank: u8,
    ram_bank: u8,
}

impl HuC1 {
    pub fn new(rom: Vec<u8>, ram_bytes: usize, battery: bool) -> Self {
        Self {
            mem: Memory::new(rom, ram_bytes, battery),
            ir_mode: false,
            rom_bank: 1,
            ram_bank: 0,
        }
    }
}

impl Mapper for HuC1 {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.mem.rom_read(0, addr),
            0x4000..=0x7FFF => self.mem.rom_read(usize::from(self.rom_bank), addr),
            _ if self.ir_mode => 0xC0,
            _ => self.mem.ram_read(usize::from(self.ram_bank), addr),
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x1FFF => self.ir_mode = v & 0x0F == 0x0E,
            0x2000..=0x3FFF => self.rom_bank = (v & 0x3F).max(1),
            0x4000..=0x5FFF => self.ram_bank = v & 3,
            0x6000..=0x7FFF => {}
            _ if self.ir_mode => {}
            _ => {
                let bank = usize::from(self.ram_bank);
                self.mem.ram_write(bank, addr, v);
            }
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

    fn save_state(&self, w: &mut StateWriter) {
        w.blob(&self.mem.ram);
        w.bool(self.ir_mode);
        w.u8(self.rom_bank);
        w.u8(self.ram_bank);
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.mem.ram = r.blob()?;
        self.ir_mode = r.bool()?;
        self.rom_bank = r.u8()?;
        self.ram_bank = r.u8()?;
        Ok(())
    }
}
