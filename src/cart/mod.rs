//! Kassette: Kopf (`0x100`–`0x14F`) und Mapper.

mod camera;
mod huc1;
mod huc3;
mod mbc1;
mod mbc2;
mod mbc3;
mod mbc5;
mod mbc6;
mod mbc7;
mod mmm01;
mod rom_only;
pub mod rtc;

#[cfg(test)]
mod tests;

pub use camera::Camera;
pub use huc1::HuC1;
pub use huc3::HuC3;
pub use mbc1::Mbc1;
pub use mbc2::Mbc2;
pub use mbc3::Mbc3;
pub use mbc5::Mbc5;
pub use mbc6::Mbc6;
pub use mbc7::Mbc7;
pub use mmm01::Mmm01;
pub use rom_only::RomOnly;
pub use rtc::Rtc;

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

/// Angaben aus dem Kassettenkopf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CartridgeInfo {
    pub title: String,
    /// `0x143`: 0x80 = CGB-kompatibel, 0xC0 = nur CGB.
    pub cgb_flag: u8,
    /// `0x146`: 0x03 = SGB-Funktionen.
    pub sgb_flag: u8,
    pub cart_type: u8,
    pub rom_banks: usize,
    pub ram_bytes: usize,
    pub header_checksum: u8,
    /// Stimmt die Kopf-Prüfsumme bei `0x14D`?
    pub checksum_ok: bool,
}

impl CartridgeInfo {
    pub fn parse(rom: &[u8]) -> Result<Self, String> {
        if rom.len() < 0x150 {
            return Err(format!("ROM zu kurz ({} Bytes, Kopf fehlt)", rom.len()));
        }
        let title_end = if rom[0x143] & 0x80 != 0 { 0x143 } else { 0x144 };
        let title = rom[0x134..title_end]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '?'
                }
            })
            .collect();
        let rom_banks = match rom[0x148] {
            n @ 0..=8 => 2usize << n,
            n => return Err(format!("unbekannte ROM-Größe ${n:02X}")),
        };
        let ram_bytes = match rom[0x149] {
            0 => 0,
            1 => 0x800,
            2 => 0x2000,
            3 => 0x8000,
            4 => 0x20000,
            5 => 0x10000,
            n => return Err(format!("unbekannte RAM-Größe ${n:02X}")),
        };
        Ok(Self {
            title,
            cgb_flag: rom[0x143],
            sgb_flag: rom[0x146],
            cart_type: rom[0x147],
            rom_banks,
            ram_bytes,
            header_checksum: rom[0x14D],
            checksum_ok: header_checksum(rom) == rom[0x14D],
        })
    }

    /// Hat die Kassette eine Batterie?
    pub fn has_battery(&self) -> bool {
        matches!(
            self.cart_type,
            0x03 | 0x06
                | 0x09
                | 0x0D
                | 0x0F
                | 0x10
                | 0x13
                | 0x1B
                | 0x1E
                | 0x22
                | 0xFC
                | 0xFE
                | 0xFF
        )
    }

    /// Name des Kassettentyps.
    pub fn type_name(&self) -> &'static str {
        match self.cart_type {
            0x00 => "ROM",
            0x01..=0x03 => "MBC1",
            0x05 | 0x06 => "MBC2",
            0x08 | 0x09 => "ROM+RAM",
            0x0B..=0x0D => "MMM01",
            0x0F..=0x13 => "MBC3",
            0x19..=0x1E => "MBC5",
            0x20 => "MBC6",
            0x22 => "MBC7",
            0xFC => "Pocket Camera",
            0xFE => "HuC3",
            0xFF => "HuC1",
            _ => "unbekannt",
        }
    }

    /// Erzeugt den passenden Mapper.
    pub fn into_mapper(self, mut rom: Vec<u8>) -> Result<Box<dyn Mapper>, String> {
        // ROM auf die Größe aus dem Kopf (mindestens 32 KB) auffüllen, damit Bankmasken passen.
        let size = (self.rom_banks * 0x4000)
            .max(rom.len().next_power_of_two())
            .max(0x8000);
        rom.resize(size, 0xFF);
        let battery = self.has_battery();
        let ram = self.ram_bytes;
        Ok(match self.cart_type {
            0x00 | 0x08 | 0x09 => Box::new(RomOnly::new(rom, ram, battery)),
            0x01..=0x03 => Box::new(Mbc1::new(rom, ram, battery)),
            0x05 | 0x06 => Box::new(Mbc2::new(rom, battery)),
            0x0F..=0x13 => Box::new(Mbc3::new(
                rom,
                ram,
                battery,
                matches!(self.cart_type, 0x0F | 0x10),
            )),
            0x19..=0x1E => Box::new(Mbc5::new(rom, ram, battery, self.cart_type >= 0x1C)),
            0x0B..=0x0D => Box::new(Mmm01::new(rom, ram, battery)),
            0x20 => Box::new(Mbc6::new(rom, ram, true)),
            0x22 => Box::new(Mbc7::new(rom)),
            0xFC => Box::new(Camera::new(rom, true)),
            0xFE => Box::new(HuC3::new(rom, ram, true)),
            0xFF => Box::new(HuC1::new(rom, ram, true)),
            t => {
                return Err(format!(
                    "Kassettentyp ${t:02X} ({}) wird noch nicht unterstützt",
                    self.type_name()
                ));
            }
        })
    }
}

/// Kopf-Prüfsumme über `0x134`–`0x14C`.
pub fn header_checksum(rom: &[u8]) -> u8 {
    rom[0x134..=0x14C]
        .iter()
        .fold(0u8, |x, &b| x.wrapping_sub(b).wrapping_sub(1))
}

/// Mapper einer Kassette: Zugriffe auf `0000`–`7FFF` und `A000`–`BFFF`.
pub trait Mapper: Send {
    fn read(&self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, v: u8);
    /// Aktuelle ROM-Bank bei `addr` (für Debug-Adressen).
    fn bank_at(&self, addr: u16) -> usize {
        usize::from(addr >= 0x4000)
    }
    /// Lässt Uhren der Kassette laufen; `cycles` in Takten des Grundtakts (4,194304 MHz).
    fn tick(&mut self, _cycles: u32) {}
    /// Hat die Kassette eine Batterie (Spielstand bleibt erhalten)?
    fn has_battery(&self) -> bool {
        false
    }
    /// Batteriegepuffertes RAM samt Uhr im üblichen Format (BGB/VBA: 48-Byte-Anhang mit
    /// `now` als Unix-Zeitstempel). Leer ohne Batterie.
    fn battery_ram(&self, _now: u64) -> Vec<u8> {
        Vec::new()
    }
    /// Lädt das Batterie-RAM. Liefert den gespeicherten Zeitstempel der Uhr, falls vorhanden.
    fn load_battery_ram(&mut self, _data: &[u8]) -> Option<u64> {
        None
    }
    /// Stellt die Uhr der Kassette um `seconds` vor (Zeit, in der das Spiel nicht lief).
    fn advance_rtc(&mut self, _seconds: u64) {}
    /// Neigung für den Beschleunigungssensor (MBC7), −128 … 127 je Achse.
    fn set_tilt(&mut self, _x: i8, _y: i8) {}
    /// Sensorbild der Pocket Camera (128 × 112 Graustufen).
    fn set_camera_image(&mut self, _image: &[u8]) {}
    /// Läuft der Rumble-Motor?
    fn rumble(&self) -> bool {
        false
    }
    fn save_state(&self, w: &mut StateWriter);
    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError>;
}

/// Gemeinsame Bausteine: ROM und RAM mit Bankzugriff.
#[derive(Debug, Clone)]
pub struct Memory {
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    pub battery: bool,
}

impl Memory {
    pub fn new(rom: Vec<u8>, ram_bytes: usize, battery: bool) -> Self {
        Self {
            rom,
            ram: vec![0; ram_bytes],
            battery,
        }
    }

    pub fn rom_banks(&self) -> usize {
        self.rom.len() / 0x4000
    }

    /// Liest aus ROM-Bank `bank` (über die Größe gespiegelt).
    pub fn rom_read(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank % self.rom_banks();
        self.rom[bank * 0x4000 + usize::from(addr & 0x3FFF)]
    }

    fn ram_index(&self, bank: usize, addr: u16) -> Option<usize> {
        if self.ram.is_empty() {
            return None;
        }
        let i = bank * 0x2000 + usize::from(addr & 0x1FFF);
        Some(i % self.ram.len())
    }

    pub fn ram_read(&self, bank: usize, addr: u16) -> u8 {
        self.ram_index(bank, addr).map_or(0xFF, |i| self.ram[i])
    }

    pub fn ram_write(&mut self, bank: usize, addr: u16, v: u8) {
        if let Some(i) = self.ram_index(bank, addr) {
            self.ram[i] = v;
        }
    }

    pub fn battery_ram(&self) -> Vec<u8> {
        if self.battery {
            self.ram.clone()
        } else {
            Vec::new()
        }
    }

    pub fn load_battery_ram(&mut self, data: &[u8]) {
        let n = data.len().min(self.ram.len());
        self.ram[..n].copy_from_slice(&data[..n]);
    }
}
