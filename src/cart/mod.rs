//! Kassette: Kopf (`0x100`–`0x14F`) und Mapper.

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

    /// Erzeugt den passenden Mapper.
    pub fn into_mapper(self, rom: Vec<u8>) -> Result<Box<dyn Mapper>, String> {
        match self.cart_type {
            0x00 | 0x08 | 0x09 => Ok(Box::new(RomOnly::new(rom, self.ram_bytes))),
            t => Err(format!("Kassettentyp ${t:02X} wird noch nicht unterstützt")),
        }
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
    /// Batteriegepuffertes RAM (leer ohne Batterie).
    fn battery_ram(&self) -> &[u8] {
        &[]
    }
    fn load_battery_ram(&mut self, _data: &[u8]) {}
    fn save_state(&self, w: &mut StateWriter);
    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError>;
}

/// Kassette ohne Mapper (bis 32 KB ROM, optional 8 KB RAM).
pub struct RomOnly {
    rom: Vec<u8>,
    ram: Vec<u8>,
}

impl RomOnly {
    pub fn new(mut rom: Vec<u8>, ram_bytes: usize) -> Self {
        rom.resize(0x8000, 0xFF);
        Self {
            rom,
            ram: vec![0; ram_bytes.min(0x2000)],
        }
    }
}

impl Mapper for RomOnly {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.rom[usize::from(addr)],
            _ => self
                .ram
                .get(usize::from(addr - 0xA000))
                .copied()
                .unwrap_or(0xFF),
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        if let Some(b) = addr
            .checked_sub(0xA000)
            .and_then(|a| self.ram.get_mut(usize::from(a)))
        {
            *b = v;
        }
    }

    fn save_state(&self, w: &mut StateWriter) {
        w.blob(&self.ram);
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.ram = r.blob()?;
        Ok(())
    }
}
