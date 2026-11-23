//! Pocket Camera: Bildsensor M64282FP, 128 KB RAM, Bildverarbeitung in der Kassette.
//!
//! RAM-Bank `0x10` blendet bei `A000`–`A07F` die Kamera-Register ein: `A000` Bit 0 startet die
//! Aufnahme (liest sich 1, solange sie läuft), `A002`/`A003` Belichtungszeit, `A006`–`A035` die
//! 4 × 4-Schwellwertmatrix (je Feld drei Schwellen). Das Ergebnis landet als 16 × 14 Kacheln
//! (2 Bit) ab `0x0100` in RAM-Bank 0.
//!
//! Das Sensorbild (128 × 112 Graustufen) kommt aus [`Mapper::set_camera_image`]; ohne Bild
//! liefert der Sensor ein Testbild (Farbverlauf). Vereinfachung: Belichtung wirkt als linearer
//! Verstärkungsfaktor, Kantenfilter und Verstärkung aus `A001` sind nicht nachgebildet.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

use super::{Mapper, Memory};

pub const SENSOR_W: usize = 128;
pub const SENSOR_H: usize = 112;

pub struct Camera {
    mem: Memory,
    ram_enabled: bool,
    rom_bank: u8,
    ram_bank: u8,
    regs: [u8; 0x36],
    /// Verbleibende Takte der laufenden Aufnahme.
    busy: u32,
    image: Vec<u8>,
}

impl Camera {
    pub fn new(rom: Vec<u8>, battery: bool) -> Self {
        let image = (0..SENSOR_W * SENSOR_H)
            .map(|i| ((i % SENSOR_W) * 2) as u8)
            .collect();
        Self {
            mem: Memory::new(rom, 0x20000, battery),
            ram_enabled: false,
            rom_bank: 1,
            ram_bank: 0,
            regs: [0; 0x36],
            busy: 0,
            image,
        }
    }

    fn exposure(&self) -> u32 {
        u32::from(self.regs[2]) << 8 | u32::from(self.regs[3])
    }

    /// Verarbeitet das Sensorbild in 2-Bit-Kacheln.
    fn capture(&mut self) {
        let gain = (self.exposure().max(1) as f32 / 0x1000 as f32).min(8.0);
        for y in 0..SENSOR_H {
            for x in 0..SENSOR_W {
                let v = (f32::from(self.image[y * SENSOR_W + x]) * gain).min(255.0) as u8;
                let m = 6 + ((y & 3) * 4 + (x & 3)) * 3;
                let t = &self.regs[m..m + 3];
                let color = if v < t[0] {
                    3
                } else if v < t[1] {
                    2
                } else if v < t[2] {
                    1
                } else {
                    0
                };
                let tile = (y / 8) * 16 + x / 8;
                let i = 0x100 + tile * 16 + (y % 8) * 2;
                let bit = 7 - (x % 8);
                let lo = &mut self.mem.ram[i];
                *lo = (*lo & !(1 << bit)) | ((color & 1) << bit);
                let hi = &mut self.mem.ram[i + 1];
                *hi = (*hi & !(1 << bit)) | (((color >> 1) & 1) << bit);
            }
        }
    }
}

impl Mapper for Camera {
    fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.mem.rom_read(0, addr),
            0x4000..=0x7FFF => self.mem.rom_read(usize::from(self.rom_bank), addr),
            _ if self.ram_bank & 0x10 != 0 => {
                if addr & 0x7F == 0 {
                    (self.regs[0] & 0x06) | u8::from(self.busy > 0)
                } else {
                    0x00
                }
            }
            // Während der Aufnahme ist das RAM nicht lesbar.
            _ if self.busy > 0 => 0x00,
            _ => self.mem.ram_read(usize::from(self.ram_bank), addr),
        }
    }

    fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enabled = v & 0x0F == 0x0A,
            0x2000..=0x3FFF => self.rom_bank = (v & 0x3F).max(1),
            0x4000..=0x5FFF => self.ram_bank = v & 0x1F,
            0x6000..=0x7FFF => {}
            _ if self.ram_bank & 0x10 != 0 => {
                let r = usize::from(addr & 0x7F);
                if r < self.regs.len() {
                    self.regs[r] = v;
                    if r == 0 && v & 1 != 0 && self.busy == 0 {
                        // Dauer: Grundzeit plus Belichtung (in 16-Takt-Schritten)
                        self.busy = 32_446 * 4 + self.exposure() * 64;
                    }
                }
            }
            _ if self.ram_enabled && self.busy == 0 => {
                let bank = usize::from(self.ram_bank);
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

    fn tick(&mut self, cycles: u32) {
        if self.busy > 0 {
            self.busy = self.busy.saturating_sub(cycles);
            if self.busy == 0 {
                self.capture();
                self.regs[0] &= !1;
            }
        }
    }

    fn set_camera_image(&mut self, image: &[u8]) {
        if image.len() == SENSOR_W * SENSOR_H {
            self.image = image.to_vec();
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
        w.u8(self.rom_bank);
        w.u8(self.ram_bank);
        w.bytes(&self.regs);
        w.u32(self.busy);
        w.blob(&self.image);
    }

    fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.mem.ram = r.blob()?;
        self.ram_enabled = r.bool()?;
        self.rom_bank = r.u8()?;
        self.ram_bank = r.u8()?;
        r.bytes_into(&mut self.regs)?;
        self.busy = r.u32()?;
        self.image = r.blob()?;
        Ok(())
    }
}
