//! PPU: Zeitverhalten (Zeilen, Modi, `LY`/`LYC`/`STAT`, Unterbrechungen), VRAM und OAM.
//!
//! Eine Zeile dauert 456 Punkte, ein Bild 154 Zeilen (70224 Punkte). Bei ausgeschaltetem LCD
//! steht `LY` auf 0; das Bild gilt trotzdem nach 70224 Punkten als fertig, damit
//! `run_frame` weiterläuft.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

pub const WIDTH: u32 = 160;
pub const HEIGHT: u32 = 144;
pub const DOTS_PER_LINE: u32 = 456;
pub const DOTS_PER_FRAME: u32 = DOTS_PER_LINE * 154;

/// Unterbrechungsbits.
pub const IRQ_VBLANK: u8 = 0x01;
pub const IRQ_STAT: u8 = 0x02;

#[derive(Debug, Clone)]
pub struct Ppu {
    /// VRAM, zwei Bänke à 8 KB (Bank 1 nur CGB).
    pub vram: Vec<u8>,
    pub vram_bank: u8,
    pub oam: [u8; 160],
    pub lcdc: u8,
    /// Schreibbare Bits 3–6 von `STAT`.
    pub stat: u8,
    pub scy: u8,
    pub scx: u8,
    pub ly: u8,
    pub lyc: u8,
    pub bgp: u8,
    pub obp0: u8,
    pub obp1: u8,
    pub wy: u8,
    pub wx: u8,
    /// Punkt in der aktuellen Zeile.
    pub dot: u32,
    /// Punkte seit dem Ausschalten (für das Bildende bei LCD aus).
    off_dots: u32,
    /// Zustand der STAT-Unterbrechungsleitung (Flanke 0 → 1 löst aus).
    stat_line: bool,
    /// Ein Bild ist fertig.
    frame_done: bool,
    /// Bildpuffer mit Farbindizes bzw. RGB555 (für die spätere Darstellung).
    pub lines: Vec<u16>,
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            vram: vec![0; 0x4000],
            vram_bank: 0,
            oam: [0; 160],
            lcdc: 0,
            stat: 0,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            bgp: 0,
            obp0: 0,
            obp1: 0,
            wy: 0,
            wx: 0,
            dot: 0,
            off_dots: 0,
            stat_line: false,
            frame_done: false,
            lines: vec![0; (WIDTH * HEIGHT) as usize],
        }
    }
}

impl Ppu {
    pub fn enabled(&self) -> bool {
        self.lcdc & 0x80 != 0
    }

    /// Aktueller Modus (0 HBlank, 1 VBlank, 2 OAM-Suche, 3 Ausgabe).
    pub fn mode(&self) -> u8 {
        if !self.enabled() {
            0
        } else if self.ly >= 144 {
            1
        } else if self.dot < 80 {
            2
        } else if self.dot < 252 {
            3
        } else {
            0
        }
    }

    fn stat_signal(&self) -> bool {
        if !self.enabled() {
            return false;
        }
        let mode = self.mode();
        (self.stat & 0x40 != 0 && self.ly == self.lyc)
            || (self.stat & 0x08 != 0 && mode == 0)
            || (self.stat & 0x10 != 0 && mode == 1)
            || (self.stat & 0x20 != 0 && mode == 2)
    }

    /// Lässt die PPU `dots` Punkte laufen. Liefert die angeforderten Unterbrechungen.
    pub fn tick(&mut self, dots: u32) -> u8 {
        let mut irq = 0;
        for _ in 0..dots {
            if !self.enabled() {
                self.off_dots += 1;
                if self.off_dots >= DOTS_PER_FRAME {
                    self.off_dots = 0;
                    self.frame_done = true;
                }
                continue;
            }
            self.dot += 1;
            if self.dot == DOTS_PER_LINE {
                self.dot = 0;
                self.ly += 1;
                if self.ly == 144 {
                    irq |= IRQ_VBLANK;
                    self.frame_done = true;
                } else if self.ly == 154 {
                    self.ly = 0;
                }
            }
            let line = self.stat_signal();
            if line && !self.stat_line {
                irq |= IRQ_STAT;
            }
            self.stat_line = line;
        }
        irq
    }

    /// Ist ein Bild fertig? Setzt die Meldung zurück.
    pub fn take_frame(&mut self) -> bool {
        std::mem::take(&mut self.frame_done)
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF40 => self.lcdc,
            0xFF41 => {
                let coinc = u8::from(self.enabled() && self.ly == self.lyc) << 2;
                0x80 | self.stat | coinc | self.mode()
            }
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0xFF40 => {
                let was_on = self.enabled();
                self.lcdc = v;
                if was_on && !self.enabled() {
                    self.ly = 0;
                    self.dot = 0;
                    self.off_dots = 0;
                    self.stat_line = false;
                }
            }
            0xFF41 => self.stat = v & 0x78,
            0xFF42 => self.scy = v,
            0xFF43 => self.scx = v,
            0xFF45 => self.lyc = v,
            0xFF47 => self.bgp = v,
            0xFF48 => self.obp0 = v,
            0xFF49 => self.obp1 = v,
            0xFF4A => self.wy = v,
            0xFF4B => self.wx = v,
            _ => {}
        }
    }

    pub fn vram_index(&self, addr: u16) -> usize {
        usize::from(self.vram_bank) * 0x2000 + usize::from(addr & 0x1FFF)
    }

    pub fn save_state(&self, w: &mut StateWriter) {
        w.blob(&self.vram);
        w.u8(self.vram_bank);
        w.bytes(&self.oam);
        for v in [
            self.lcdc, self.stat, self.scy, self.scx, self.ly, self.lyc, self.bgp, self.obp0,
            self.obp1, self.wy, self.wx,
        ] {
            w.u8(v);
        }
        w.u32(self.dot);
        w.u32(self.off_dots);
        w.bool(self.stat_line);
        w.u16_slice(&self.lines);
    }

    pub fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.vram = r.blob()?;
        if self.vram.len() != 0x4000 {
            return Err(StateError::Invalid("VRAM-Größe".into()));
        }
        self.vram_bank = r.u8()?;
        r.bytes_into(&mut self.oam)?;
        for v in [
            &mut self.lcdc,
            &mut self.stat,
            &mut self.scy,
            &mut self.scx,
            &mut self.ly,
            &mut self.lyc,
            &mut self.bgp,
            &mut self.obp0,
            &mut self.obp1,
            &mut self.wy,
            &mut self.wx,
        ] {
            *v = r.u8()?;
        }
        self.dot = r.u32()?;
        self.off_dots = r.u32()?;
        self.stat_line = r.bool()?;
        self.lines = r.u16_vec()?;
        Ok(())
    }
}
