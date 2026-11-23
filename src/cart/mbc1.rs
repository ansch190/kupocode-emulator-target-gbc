//! MBC1 (bis 2 MB ROM, 32 KB RAM) samt Multicart-Verdrahtung MBC1M.
//!
//! `BANK1` (5 Bit, 0 wird zu 1) und `BANK2` (2 Bit) bilden die ROM-Bank. Im Modus 1 wirkt
//! `BANK2` auch auf `0000`–`3FFF` und wählt die RAM-Bank. Bei MBC1M ist `BANK1` nur 4 Bit breit
//! angeschlossen; erkannt wird sie an einem zweiten Spielkopf in Bank `0x10`.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use super::{Mapper, Memory};

pub struct Mbc1 {
    mem: Memory,
    ram_enabled: bool,
    bank1: u8,
    bank2: u8,
    mode: bool,
    multicart: bool,
}

impl Mbc1 {
    pub fn new(rom: Vec<u8>, ram_bytes: usize, battery: bool) -> Self {
        let multicart = rom.len() == 0x10_0000 && rom[0x104..0x134] == rom[0x4_0104..0x4_0134];
        Self {
            mem: Memory::new(rom, ram_bytes, battery),
            ram_enabled: false,
            bank1: 1,
            bank2: 0,
            mode: false,
            multicart,
        }
    }

    fn shift(&self) -> u32 {
        if self.multicart { 4 } else { 5 }
    }

    fn low_bank(&self) -> usize {
        if self.mode {
            usize::from(self.bank2) << self.shift()
        } else {
            0
        }
    }

    fn high_bank(&self) -> usize {
        let b1 = if self.multicart {
            self.bank1 & 0x0F
        } else {
            self.bank1
        };
        (usize::from(self.bank2) << self.shift()) | usize::from(b1)
    }

    fn ram_bank(&self) -> usize {
        if self.mode {
            usize::from(self.bank2)
        } else {
            0
        }
    }
}

impl Mapper for Mbc1 {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.mem.rom_read(self.low_bank(), addr),
            0x4000..=0x7FFF => self.mem.rom_read(self.high_bank(), addr),
            _ if self.ram_enabled => self.mem.ram_read(self.ram_bank(), addr),
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enabled = v & 0x0F == 0x0A,
            0x2000..=0x3FFF => self.bank1 = (v & 0x1F).max(1),
            0x4000..=0x5FFF => self.bank2 = v & 3,
            0x6000..=0x7FFF => self.mode = v & 1 != 0,
            _ if self.ram_enabled => {
                let bank = self.ram_bank();
                self.mem.ram_write(bank, addr, v);
            }
            _ => {}
        }
    }

    fn bank_at(&self, addr: u16) -> usize {
        if addr < 0x4000 {
            self.low_bank() % self.mem.rom_banks()
        } else {
            self.high_bank() % self.mem.rom_banks()
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
        w.u8(self.bank1);
        w.u8(self.bank2);
        w.bool(self.mode);
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.mem.ram = r.blob()?;
        self.ram_enabled = r.bool()?;
        self.bank1 = r.u8()?;
        self.bank2 = r.u8()?;
        self.mode = r.bool()?;
        Ok(())
    }
}
