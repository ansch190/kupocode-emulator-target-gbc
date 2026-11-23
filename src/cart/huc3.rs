//! HuC3 (Hudson): RAM, Uhr über ein Befehlsprotokoll, Infrarot (Stub) und Tongeber.
//!
//! `0000`–`1FFF` wählt, was bei `A000`–`BFFF` liegt: `0x0A` RAM, `0x0B` Befehl schreiben,
//! `0x0C` Antwort lesen, `0x0D` Bereitschaft, `0x0E` Infrarot. Ein Befehl ist ein Byte:
//! Bits 4–6 Befehl, Bits 0–3 Wert.
//! - 1: Nibble an der Adresse lesen (danach Adresse + 1)
//! - 3: Nibble an die Adresse schreiben (danach Adresse + 1)
//! - 4/5: unteres/oberes Nibble der Adresse setzen
//! - 6: Sonderbefehl – 0 = Uhrzeit in den Speicher 0–5, 1 = Uhrzeit aus dem Speicher setzen,
//!   2 = Status (Antwort 1), 0xE = Ton
//!
//! Die Uhr zählt Minuten (12 Bit) und Tage (16 Bit) aus emulierten Takten.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use super::rtc::CYCLES_PER_SECOND;
use super::{Mapper, Memory};

pub struct HuC3 {
    mem: Memory,
    mode: u8,
    rom_bank: u8,
    ram_bank: u8,
    /// Speicher des Uhr-Bausteins (256 Nibbles).
    regs: Vec<u8>,
    addr: u8,
    /// Letzte Antwort (Befehl in Bits 4–6, Wert in Bits 0–3).
    response: u8,
    /// Minuten seit Mitternacht und Tage.
    minutes: u16,
    days: u16,
    /// Takte und Sekunden bis zur nächsten Minute.
    sub: u32,
    seconds: u8,
}

impl HuC3 {
    pub fn new(rom: Vec<u8>, ram_bytes: usize, battery: bool) -> Self {
        Self {
            mem: Memory::new(rom, ram_bytes, battery),
            mode: 0,
            rom_bank: 1,
            ram_bank: 0,
            regs: vec![0; 256],
            addr: 0,
            response: 0,
            minutes: 0,
            days: 0,
            sub: 0,
            seconds: 0,
        }
    }

    fn minute(&mut self) {
        self.minutes += 1;
        if self.minutes >= 1440 {
            self.minutes = 0;
            self.days = self.days.wrapping_add(1);
        }
    }

    fn command(&mut self, v: u8) {
        let value = v & 0x0F;
        match (v >> 4) & 7 {
            1 => {
                self.response = 0x10 | self.regs[usize::from(self.addr)];
                self.addr = self.addr.wrapping_add(1);
            }
            3 => {
                self.regs[usize::from(self.addr)] = value;
                self.addr = self.addr.wrapping_add(1);
                self.response = 0x30;
            }
            4 => self.addr = (self.addr & 0xF0) | value,
            5 => self.addr = (self.addr & 0x0F) | value << 4,
            6 => {
                match value {
                    0 => {
                        let (m, d) = (self.minutes, self.days);
                        for i in 0..3 {
                            self.regs[i] = ((m >> (i * 4)) & 0x0F) as u8;
                            self.regs[3 + i] = ((d >> (i * 4)) & 0x0F) as u8;
                        }
                    }
                    1 => {
                        let nib = |i: usize| u16::from(self.regs[i]);
                        self.minutes = (nib(0) | nib(1) << 4 | nib(2) << 8) % 1440;
                        self.days = nib(3) | nib(4) << 4 | nib(5) << 8;
                        self.sub = 0;
                        self.seconds = 0;
                    }
                    _ => {}
                }
                self.response = 0x61;
            }
            _ => {}
        }
    }
}

impl Mapper for HuC3 {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.mem.rom_read(0, addr),
            0x4000..=0x7FFF => self.mem.rom_read(usize::from(self.rom_bank), addr),
            _ => match self.mode {
                0x0C => 0x80 | self.response,
                0x0D => 0x01,
                0x0E => 0xC0,
                0x00 | 0x0A => self.mem.ram_read(usize::from(self.ram_bank), addr),
                _ => 0xFF,
            },
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x1FFF => self.mode = v & 0x0F,
            0x2000..=0x3FFF => self.rom_bank = (v & 0x7F).max(1),
            0x4000..=0x5FFF => self.ram_bank = v & 3,
            0x6000..=0x7FFF => {}
            _ => match self.mode {
                0x0A => {
                    let bank = usize::from(self.ram_bank);
                    self.mem.ram_write(bank, addr, v);
                }
                0x0B => self.command(v),
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
        self.sub += cycles;
        while self.sub >= CYCLES_PER_SECOND {
            self.sub -= CYCLES_PER_SECOND;
            self.seconds += 1;
            if self.seconds == 60 {
                self.seconds = 0;
                self.minute();
            }
        }
    }

    fn advance_rtc(&mut self, seconds: u64) {
        let total = u64::from(self.seconds) + seconds;
        self.seconds = (total % 60) as u8;
        let minutes = u64::from(self.minutes) + total / 60;
        self.minutes = (minutes % 1440) as u16;
        self.days = self.days.wrapping_add((minutes / 1440) as u16);
    }

    fn has_battery(&self) -> bool {
        self.mem.battery
    }

    /// Batterie-RAM: RAM, dann Uhr (Minuten, Tage als u16 LE) und der Zeitstempel (u64 LE).
    fn battery_ram(&self, now: u64) -> Vec<u8> {
        if !self.mem.battery {
            return Vec::new();
        }
        let mut out = self.mem.ram.clone();
        out.extend_from_slice(&self.minutes.to_le_bytes());
        out.extend_from_slice(&self.days.to_le_bytes());
        out.extend_from_slice(&now.to_le_bytes());
        out
    }

    fn load_battery_ram(&mut self, data: &[u8]) -> Option<u64> {
        let n = self.mem.ram.len();
        self.mem.load_battery_ram(data);
        let rest = data.get(n..n + 12)?;
        self.minutes = u16::from_le_bytes([rest[0], rest[1]]) % 1440;
        self.days = u16::from_le_bytes([rest[2], rest[3]]);
        Some(u64::from_le_bytes(rest[4..12].try_into().ok()?))
    }

    fn save_state(&self, w: &mut StateWriter) {
        w.blob(&self.mem.ram);
        for v in [
            self.mode,
            self.rom_bank,
            self.ram_bank,
            self.addr,
            self.response,
            self.seconds,
        ] {
            w.u8(v);
        }
        w.blob(&self.regs);
        w.u16(self.minutes);
        w.u16(self.days);
        w.u32(self.sub);
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.mem.ram = r.blob()?;
        for v in [
            &mut self.mode,
            &mut self.rom_bank,
            &mut self.ram_bank,
            &mut self.addr,
            &mut self.response,
            &mut self.seconds,
        ] {
            *v = r.u8()?;
        }
        self.regs = r.blob()?;
        self.minutes = r.u16()?;
        self.days = r.u16()?;
        self.sub = r.u32()?;
        Ok(())
    }
}
