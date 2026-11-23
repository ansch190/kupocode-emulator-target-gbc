//! MBC7: Beschleunigungssensor (Neigung) und serielles EEPROM 93LC56 (128 × 16 Bit).
//!
//! RAM-Freigabe in zwei Schritten (`0000` = `0x0A`, `4000` = `0x40`). Danach liegen die Register
//! bei `A000`–`AFFF` (Adressbits 4–7): `A000`/`A010` löschen und übernehmen den Sensorwert,
//! `A020`–`A050` X und Y (Mitte `0x81D0`), `A060` = 0, `A070` = `0xFF`, `A080` das EEPROM
//! (Bit 7 CS, Bit 6 CLK, Bit 1 DI, Bit 0 DO).
//!
//! Die Neigung kommt aus dem linken Stick (`set_tilt`, −128 … 127 je Achse).

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use super::{Mapper, Memory};

const CENTER: u16 = 0x81D0;
/// Ausschlag bei vollem Stick.
const RANGE: i32 = 0x70;

#[derive(Debug, Clone, Default)]
struct Eeprom {
    data: Vec<u16>,
    cs: bool,
    clk: bool,
    do_: bool,
    /// Empfangene Befehlsbits.
    shift: u32,
    bits: u8,
    /// Laufender Befehl nach 11 Bits (`READ` gibt 16 Bits aus, `WRITE`/`WRAL` empfängt 16).
    read_out: Option<(u16, u8)>,
    write_in: Option<(Option<u8>, u16, u8)>,
    write_enabled: bool,
}

impl Eeprom {
    fn new() -> Self {
        Self {
            data: vec![0xFFFF; 128],
            do_: true,
            ..Self::default()
        }
    }

    fn write(&mut self, v: u8) {
        let cs = v & 0x80 != 0;
        let clk = v & 0x40 != 0;
        let di = v & 0x02 != 0;
        if !cs {
            self.cs = false;
            self.clk = clk;
            self.bits = 0;
            self.shift = 0;
            self.read_out = None;
            self.write_in = None;
            return;
        }
        let rising = clk && !self.clk;
        self.cs = true;
        self.clk = clk;
        if !rising {
            return;
        }
        if let Some((value, left)) = self.read_out {
            self.do_ = value & 0x8000 != 0;
            self.read_out = (left > 1).then_some((value << 1, left - 1));
            return;
        }
        if let Some((addr, value, left)) = self.write_in {
            let value = (value << 1) | u16::from(di);
            if left > 1 {
                self.write_in = Some((addr, value, left - 1));
            } else {
                self.write_in = None;
                if self.write_enabled {
                    match addr {
                        Some(a) => self.data[usize::from(a)] = value,
                        None => self.data.fill(value),
                    }
                }
                self.do_ = true;
            }
            return;
        }
        // Auf das Startbit warten.
        if self.bits == 0 && !di {
            return;
        }
        self.shift = (self.shift << 1) | u32::from(di);
        self.bits += 1;
        if self.bits < 11 {
            return;
        }
        let op = (self.shift >> 8) & 3;
        let addr = (self.shift & 0x7F) as u8;
        let sub = (self.shift >> 6) & 3;
        self.bits = 0;
        self.shift = 0;
        match (op, sub) {
            (2, _) => {
                // READ: erst ein Dummy-Bit 0, dann 16 Datenbits
                self.do_ = false;
                self.read_out = Some((self.data[usize::from(addr)], 16));
            }
            (1, _) => self.write_in = Some((Some(addr), 0, 16)),
            (3, _) => {
                if self.write_enabled {
                    self.data[usize::from(addr)] = 0xFFFF;
                }
            }
            (0, 3) => self.write_enabled = true,
            (0, 0) => self.write_enabled = false,
            (0, 2) => {
                if self.write_enabled {
                    self.data.fill(0xFFFF);
                }
            }
            _ => self.write_in = Some((None, 0, 16)),
        }
    }

    fn read(&self) -> u8 {
        u8::from(self.cs) << 7 | u8::from(self.clk) << 6 | u8::from(self.do_)
    }
}

pub struct Mbc7 {
    mem: Memory,
    enable1: bool,
    enable2: bool,
    rom_bank: u8,
    tilt: (i16, i16),
    /// Übernommene Sensorwerte; `None` nach dem Löschen.
    latched: Option<(u16, u16)>,
    erase_armed: bool,
    eeprom: Eeprom,
}

impl Mbc7 {
    pub fn new(rom: Vec<u8>) -> Self {
        Self {
            mem: Memory::new(rom, 0, true),
            enable1: false,
            enable2: false,
            rom_bank: 1,
            tilt: (0, 0),
            latched: None,
            erase_armed: false,
            eeprom: Eeprom::new(),
        }
    }

    fn sensor(&self) -> (u16, u16) {
        let axis = |v: i16| (i32::from(CENTER) - i32::from(v) * RANGE / 128) as u16;
        (axis(self.tilt.0), axis(self.tilt.1))
    }
}

impl Mapper for Mbc7 {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.mem.rom_read(0, addr),
            0x4000..=0x7FFF => self.mem.rom_read(usize::from(self.rom_bank), addr),
            0xA000..=0xAFFF if self.enable1 && self.enable2 => {
                let (x, y) = self.latched.unwrap_or((0x8000, 0x8000));
                match (addr >> 4) & 0x0F {
                    2 => x as u8,
                    3 => (x >> 8) as u8,
                    4 => y as u8,
                    5 => (y >> 8) as u8,
                    6 => 0x00,
                    8 => self.eeprom.read(),
                    _ => 0xFF,
                }
            }
            _ => 0xFF,
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x1FFF => self.enable1 = v == 0x0A,
            0x2000..=0x3FFF => self.rom_bank = (v & 0x7F).max(1),
            0x4000..=0x5FFF => self.enable2 = v == 0x40,
            0xA000..=0xAFFF if self.enable1 && self.enable2 => match (addr >> 4) & 0x0F {
                0 => {
                    if v == 0x55 {
                        self.latched = None;
                        self.erase_armed = true;
                    }
                }
                1 => {
                    if v == 0xAA && self.erase_armed {
                        self.latched = Some(self.sensor());
                        self.erase_armed = false;
                    }
                }
                8 => self.eeprom.write(v),
                _ => {}
            },
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

    fn set_tilt(&mut self, x: i8, y: i8) {
        self.tilt = (i16::from(x), i16::from(y));
    }

    fn has_battery(&self) -> bool {
        true
    }

    fn battery_ram(&self, _now: u64) -> Vec<u8> {
        self.eeprom
            .data
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect()
    }

    fn load_battery_ram(&mut self, data: &[u8]) -> Option<u64> {
        for (w, b) in self.eeprom.data.iter_mut().zip(data.as_chunks::<2>().0) {
            *w = u16::from_le_bytes([b[0], b[1]]);
        }
        None
    }

    fn save_state(&self, w: &mut StateWriter) {
        w.bool(self.enable1);
        w.bool(self.enable2);
        w.u8(self.rom_bank);
        w.option(self.latched.as_ref(), |w, &(x, y)| {
            w.u16(x);
            w.u16(y);
        });
        w.bool(self.erase_armed);
        let e = &self.eeprom;
        w.u16_slice(&e.data);
        for v in [e.cs, e.clk, e.do_, e.write_enabled] {
            w.bool(v);
        }
        w.u32(e.shift);
        w.u8(e.bits);
        w.option(e.read_out.as_ref(), |w, &(v, l)| {
            w.u16(v);
            w.u8(l);
        });
        w.option(e.write_in.as_ref(), |w, &(a, v, l)| {
            w.u8(a.unwrap_or(0xFF));
            w.u16(v);
            w.u8(l);
        });
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.enable1 = r.bool()?;
        self.enable2 = r.bool()?;
        self.rom_bank = r.u8()?;
        self.latched = r.option(|r| Ok((r.u16()?, r.u16()?)))?;
        self.erase_armed = r.bool()?;
        let e = &mut self.eeprom;
        e.data = r.u16_vec()?;
        for v in [&mut e.cs, &mut e.clk, &mut e.do_, &mut e.write_enabled] {
            *v = r.bool()?;
        }
        e.shift = r.u32()?;
        e.bits = r.u8()?;
        e.read_out = r.option(|r| Ok((r.u16()?, r.u8()?)))?;
        e.write_in = r.option(|r| {
            let a = r.u8()?;
            Ok(((a != 0xFF).then_some(a), r.u16()?, r.u8()?))
        })?;
        Ok(())
    }
}
