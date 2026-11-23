//! MBC3 (bis 2 MB ROM, 32 KB RAM, optional Echtzeituhr; MBC30 mit 8-Bit-Bank und 64 KB RAM).

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use super::{Mapper, Memory, Rtc};

pub struct Mbc3 {
    mem: Memory,
    ram_enabled: bool,
    rom_bank: u8,
    /// 0–7 = RAM-Bank, 8–C = Uhrregister.
    select: u8,
    rtc: Option<Rtc>,
}

impl Mbc3 {
    pub fn new(rom: Vec<u8>, ram_bytes: usize, battery: bool, timer: bool) -> Self {
        Self {
            mem: Memory::new(rom, ram_bytes, battery),
            ram_enabled: false,
            rom_bank: 1,
            select: 0,
            rtc: timer.then(Rtc::default),
        }
    }

    /// MBC30: mehr als 2 MB ROM oder mehr als 32 KB RAM.
    fn rom_mask(&self) -> u8 {
        if self.mem.rom.len() > 0x20_0000 {
            0xFF
        } else {
            0x7F
        }
    }
}

impl Mapper for Mbc3 {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.mem.rom_read(0, addr),
            0x4000..=0x7FFF => self.mem.rom_read(usize::from(self.rom_bank), addr),
            _ if !self.ram_enabled => 0xFF,
            _ => match (self.select, &self.rtc) {
                (0x08..=0x0C, Some(rtc)) => rtc.read(self.select - 8),
                (0x00..=0x07, _) => self.mem.ram_read(usize::from(self.select), addr),
                _ => 0xFF,
            },
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enabled = v & 0x0F == 0x0A,
            0x2000..=0x3FFF => self.rom_bank = (v & self.rom_mask()).max(1),
            0x4000..=0x5FFF => self.select = v & 0x0F,
            0x6000..=0x7FFF => {
                if let Some(rtc) = &mut self.rtc {
                    rtc.latch(v);
                }
            }
            _ if !self.ram_enabled => {}
            _ => match (self.select, &mut self.rtc) {
                (0x08..=0x0C, Some(rtc)) => rtc.write(self.select - 8, v),
                (0x00..=0x07, _) => self.mem.ram_write(usize::from(self.select), addr, v),
                _ => {}
            },
        }
    }

    fn bank_at(&self, addr: u16) -> usize {
        if addr < 0x4000 {
            0
        } else {
            usize::from(self.rom_bank) % self.mem.rom_banks()
        }
    }

    fn tick(&mut self, cycles: u32) {
        if let Some(rtc) = &mut self.rtc {
            rtc.tick(cycles);
        }
    }

    fn has_battery(&self) -> bool {
        self.mem.battery
    }

    fn battery_ram(&self, now: u64) -> Vec<u8> {
        if !self.mem.battery {
            return Vec::new();
        }
        let mut out = self.mem.ram.clone();
        if let Some(rtc) = &self.rtc {
            out.extend(rtc.footer(now));
        }
        out
    }

    fn load_battery_ram(&mut self, data: &[u8]) -> Option<u64> {
        let n = self.mem.ram.len();
        self.mem.load_battery_ram(data);
        match &mut self.rtc {
            Some(rtc) if data.len() > n => rtc.load_footer(&data[n..]),
            _ => None,
        }
    }

    fn advance_rtc(&mut self, seconds: u64) {
        if let Some(rtc) = &mut self.rtc {
            rtc.advance(seconds);
        }
    }

    fn save_state(&self, w: &mut StateWriter) {
        w.blob(&self.mem.ram);
        w.bool(self.ram_enabled);
        w.u8(self.rom_bank);
        w.u8(self.select);
        if let Some(rtc) = &self.rtc {
            rtc.save_state(w);
        }
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.mem.ram = r.blob()?;
        self.ram_enabled = r.bool()?;
        self.rom_bank = r.u8()?;
        self.select = r.u8()?;
        if let Some(rtc) = &mut self.rtc {
            rtc.load_state(r)?;
        }
        Ok(())
    }
}
