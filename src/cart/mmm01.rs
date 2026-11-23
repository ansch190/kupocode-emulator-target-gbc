//! MMM01: Mapper für Sammelkassetten.
//!
//! Nach dem Einschalten sind die letzten 32 KB des ROMs eingeblendet (das Menü). Das Menü legt
//! über die Register einen Basisbereich und eine Maske fest und sperrt die Konfiguration mit
//! Bit 6 von `0000`–`1FFF`; danach verhält sich die Kassette wie ein MBC1 im gewählten Bereich.
//!
//! Vereinfachung: Basis und Maske werden nur in 16-KB-Bänken geführt (die Hardware verteilt sie
//! auf mehrere Registerfelder); die bekannten Sammelkassetten nutzen nur diese Form.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use super::{Mapper, Memory};

pub struct Mmm01 {
    mem: Memory,
    locked: bool,
    ram_enabled: bool,
    /// Erste ROM-Bank des gewählten Spiels.
    base: usize,
    /// Maske der Spielbänke (Größe − 1).
    mask: usize,
    rom_bank: u8,
    ram_bank: u8,
}

impl Mmm01 {
    pub fn new(rom: Vec<u8>, ram_bytes: usize, battery: bool) -> Self {
        Self {
            mem: Memory::new(rom, ram_bytes, battery),
            locked: false,
            ram_enabled: false,
            base: 0,
            mask: 0x1FF,
            rom_bank: 1,
            ram_bank: 0,
        }
    }

    fn bank(&self, addr: u16) -> usize {
        if !self.locked {
            // Menü: die letzten beiden Bänke
            let n = self.mem.rom_banks();
            return n - 2 + usize::from(addr >= 0x4000);
        }
        let inner = if addr < 0x4000 {
            0
        } else {
            usize::from(self.rom_bank)
        };
        self.base + (inner & self.mask)
    }
}

impl Mapper for Mmm01 {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.mem.rom_read(self.bank(addr), addr),
            _ if self.ram_enabled => self.mem.ram_read(usize::from(self.ram_bank), addr),
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x1FFF => {
                self.ram_enabled = v & 0x0F == 0x0A;
                if !self.locked && v & 0x40 != 0 {
                    self.locked = true;
                }
            }
            0x2000..=0x3FFF if !self.locked => self.base = usize::from(v & 0x7F),
            0x2000..=0x3FFF => self.rom_bank = (v & 0x1F).max(1),
            0x4000..=0x5FFF => self.ram_bank = v & 3,
            0x6000..=0x7FFF if !self.locked => {
                // Bits 2–5: Spielgröße als Maske (0 = 512 Bänke)
                let size = (v >> 2) & 0x0F;
                self.mask = if size == 0 {
                    0x1FF
                } else {
                    (1usize << size.min(9)) - 1
                };
            }
            0x6000..=0x7FFF => {}
            _ if self.ram_enabled => {
                let bank = usize::from(self.ram_bank);
                self.mem.ram_write(bank, addr, v);
            }
            _ => {}
        }
    }

    fn bank_at(&self, addr: u16) -> usize {
        self.bank(addr) % self.mem.rom_banks()
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
        w.bool(self.locked);
        w.bool(self.ram_enabled);
        w.u32(self.base as u32);
        w.u32(self.mask as u32);
        w.u8(self.rom_bank);
        w.u8(self.ram_bank);
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.mem.ram = r.blob()?;
        self.locked = r.bool()?;
        self.ram_enabled = r.bool()?;
        self.base = r.u32()? as usize;
        self.mask = r.u32()? as usize;
        self.rom_bank = r.u8()?;
        self.ram_bank = r.u8()?;
        Ok(())
    }
}
