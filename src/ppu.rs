//! PPU: Zeilen, Modi, `LY`/`LYC`/`STAT`, Pixel-FIFO für Hintergrund, Fenster und Objekte.
//!
//! Eine Zeile dauert 456 Punkte, ein Bild 154 Zeilen (70224 Punkte). Je sichtbare Zeile:
//! - **Mode 2** (Punkte 0–79): OAM-Suche, höchstens 10 Objekte.
//! - **Mode 3**: Der Hintergrund-Fetcher holt Kacheln in 6 Punkten (Kachelnummer, unteres und
//!   oberes Datenbyte) und schiebt sie in den Hintergrund-FIFO; je Punkt verlässt ein Pixel den
//!   FIFO. Die Länge ergibt sich von selbst: 172 Punkte plus `SCX & 7` verworfene Pixel, 6 Punkte
//!   je Fensterstart und 6–11 Punkte je Objekt.
//! - **Mode 0** bis Punkt 455.
//!
//! Bei ausgeschaltetem LCD steht `LY` auf 0; das Bild gilt trotzdem nach 70224 Punkten als
//! fertig, damit `run_frame` weiterläuft. Das erste Bild nach dem Einschalten bleibt weiß.

use std::collections::VecDeque;

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

pub const WIDTH: u32 = 160;
pub const HEIGHT: u32 = 144;
pub const DOTS_PER_LINE: u32 = 456;
pub const DOTS_PER_FRAME: u32 = DOTS_PER_LINE * 154;

/// Unterbrechungsbits.
pub const IRQ_VBLANK: u8 = 0x01;
pub const IRQ_STAT: u8 = 0x02;

/// Verzögerung zu Beginn von Mode 3 (erster, verworfener Abruf).
const START_DELAY: u8 = 6;

/// Ein Objekt aus der OAM-Suche.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sprite {
    pub y: u8,
    pub x: u8,
    pub tile: u8,
    pub attr: u8,
    pub index: u8,
    pub fetched: bool,
}

/// Pixel im Objekt-FIFO.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct ObjPixel {
    /// Farbindex 0–3 (0 = durchsichtig).
    color: u8,
    /// Attribute des Objekts (Palette, Priorität).
    attr: u8,
}

/// Zustand des Hintergrund-Fetchers.
#[derive(Debug, Clone, Copy, Default)]
struct Fetcher {
    /// Punkt im Abruf: 0–5 holen, 6 = schieben.
    step: u8,
    /// Kachelspalte (relativ zum Beginn von Hintergrund bzw. Fenster).
    x: u8,
    tile: u8,
    lo: u8,
    hi: u8,
    window: bool,
}

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
    /// Aktueller Modus 0–3.
    mode: u8,
    /// Punkte seit dem Ausschalten (für das Bildende bei LCD aus).
    off_dots: u32,
    /// Zustand der STAT-Unterbrechungsleitung (Flanke 0 → 1 löst aus).
    stat_line: bool,
    /// Ein Bild ist fertig.
    frame_done: bool,
    /// Das laufende Bild wird nicht angezeigt (erstes Bild nach dem Einschalten).
    blank_frame: bool,
    // ── Mode 3 ──
    sprites: Vec<Sprite>,
    bg_fifo: VecDeque<u8>,
    obj_fifo: [ObjPixel; 8],
    fetcher: Fetcher,
    /// Ausgegebene Pixel der Zeile.
    lx: u8,
    /// Noch zu verwerfende Pixel (`SCX & 7`).
    discard: u8,
    delay: u8,
    /// Laufender Objektabruf: Index in `sprites`, Restpunkte, davon Wartepunkte des Fetchers.
    sprite_fetch: Option<(u8, u8, u8)>,
    // ── Fenster ──
    window_line: u8,
    wy_triggered: bool,
    window_on_line: bool,
    /// Bild als Farben je Pixel (DMG: Graustufe 0–3).
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
            mode: 0,
            off_dots: 0,
            stat_line: false,
            frame_done: false,
            blank_frame: false,
            sprites: Vec::new(),
            bg_fifo: VecDeque::with_capacity(16),
            obj_fifo: [ObjPixel::default(); 8],
            fetcher: Fetcher::default(),
            lx: 0,
            discard: 0,
            delay: 0,
            sprite_fetch: None,
            window_line: 0,
            wy_triggered: false,
            window_on_line: false,
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
        self.mode
    }

    /// Darf die CPU auf das VRAM zugreifen?
    pub fn vram_accessible(&self) -> bool {
        self.mode != 3
    }

    /// Darf die CPU auf die OAM zugreifen?
    pub fn oam_accessible(&self) -> bool {
        self.mode < 2
    }

    fn sprite_height(&self) -> u8 {
        if self.lcdc & 0x04 != 0 { 16 } else { 8 }
    }

    fn stat_signal(&self) -> bool {
        if !self.enabled() {
            return false;
        }
        (self.stat & 0x40 != 0 && self.ly == self.lyc)
            || (self.stat & 0x08 != 0 && self.mode == 0)
            || (self.stat & 0x10 != 0 && self.mode == 1)
            || (self.stat & 0x20 != 0 && self.mode == 2)
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
            irq |= self.dot_step();
            let line = self.stat_signal();
            if line && !self.stat_line {
                irq |= IRQ_STAT;
            }
            self.stat_line = line;
        }
        irq
    }

    /// Ein Punkt bei eingeschaltetem LCD.
    fn dot_step(&mut self) -> u8 {
        let mut irq = 0;
        if self.ly < 144 {
            match self.dot {
                0 => {
                    if self.ly == self.wy {
                        self.wy_triggered = true;
                    }
                    self.mode = 2;
                }
                79 => self.oam_scan(),
                80 => self.start_mode3(),
                _ => {}
            }
            if self.mode == 3 {
                self.mode3_dot();
            }
        }
        self.dot += 1;
        if self.dot == DOTS_PER_LINE {
            self.dot = 0;
            if self.window_on_line {
                self.window_line = self.window_line.wrapping_add(1);
                self.window_on_line = false;
            }
            self.ly += 1;
            if self.ly == 144 {
                self.mode = 1;
                irq |= IRQ_VBLANK;
                self.frame_done = true;
                self.blank_frame = false;
            } else if self.ly == 154 {
                self.ly = 0;
                self.window_line = 0;
                self.wy_triggered = false;
            }
        }
        irq
    }

    fn oam_scan(&mut self) {
        self.sprites.clear();
        let h = self.sprite_height();
        let line = u16::from(self.ly) + 16;
        for i in 0..40u8 {
            let o = usize::from(i) * 4;
            let y = self.oam[o];
            if line >= u16::from(y) && line < u16::from(y) + u16::from(h) {
                self.sprites.push(Sprite {
                    y,
                    x: self.oam[o + 1],
                    tile: self.oam[o + 2],
                    attr: self.oam[o + 3],
                    index: i,
                    fetched: false,
                });
                if self.sprites.len() == 10 {
                    break;
                }
            }
        }
    }

    fn start_mode3(&mut self) {
        self.mode = 3;
        self.bg_fifo.clear();
        self.obj_fifo = [ObjPixel::default(); 8];
        self.fetcher = Fetcher::default();
        self.lx = 0;
        self.discard = self.scx & 7;
        self.delay = START_DELAY;
        self.sprite_fetch = None;
    }

    fn tile_row_addr(&self, tile: u8, row: u8) -> usize {
        let base = if self.lcdc & 0x10 != 0 {
            usize::from(tile) * 16
        } else {
            (0x1000 + i32::from(tile as i8) * 16) as usize
        };
        base + usize::from(row) * 2
    }

    fn fetcher_step(&mut self) {
        let f = self.fetcher;
        match f.step {
            1 => {
                let (map, col, row) = if f.window {
                    let map = if self.lcdc & 0x40 != 0 {
                        0x1C00
                    } else {
                        0x1800
                    };
                    (map, f.x & 31, self.window_line / 8)
                } else {
                    let map = if self.lcdc & 0x08 != 0 {
                        0x1C00
                    } else {
                        0x1800
                    };
                    let y = self.scy.wrapping_add(self.ly);
                    (map, (self.scx / 8).wrapping_add(f.x) & 31, y / 8)
                };
                self.fetcher.tile = self.vram[map + usize::from(row) * 32 + usize::from(col)];
            }
            3 | 5 => {
                let row = if f.window {
                    self.window_line & 7
                } else {
                    self.scy.wrapping_add(self.ly) & 7
                };
                let v = self.vram[self.tile_row_addr(f.tile, row) + usize::from(f.step == 5)];
                if f.step == 3 {
                    self.fetcher.lo = v;
                } else {
                    self.fetcher.hi = v;
                }
            }
            6 => {
                if self.bg_fifo.is_empty() {
                    for bit in (0..8).rev() {
                        let c = ((f.hi >> bit) & 1) << 1 | ((f.lo >> bit) & 1);
                        self.bg_fifo.push_back(c);
                    }
                    self.fetcher.x = f.x.wrapping_add(1);
                    self.fetcher.step = 0;
                }
                return;
            }
            _ => {}
        }
        self.fetcher.step += 1;
    }

    fn fetch_sprite(&mut self, i: usize) {
        let s = self.sprites[i];
        let h = self.sprite_height();
        let mut row = self.ly.wrapping_add(16).wrapping_sub(s.y);
        if s.attr & 0x40 != 0 {
            row = h - 1 - row;
        }
        let tile = if h == 16 { s.tile & 0xFE } else { s.tile };
        let addr = usize::from(tile) * 16 + usize::from(row) * 2;
        let (lo, hi) = (self.vram[addr], self.vram[addr + 1]);
        let skip = (self.lx + 8).saturating_sub(s.x);
        for p in skip..8 {
            let bit = if s.attr & 0x20 != 0 { p } else { 7 - p };
            let color = ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1);
            let slot = &mut self.obj_fifo[usize::from(p - skip)];
            if slot.color == 0 {
                *slot = ObjPixel {
                    color,
                    attr: s.attr,
                };
            }
        }
        self.sprites[i].fetched = true;
    }

    fn mode3_dot(&mut self) {
        if self.delay > 0 {
            self.delay -= 1;
            return;
        }
        if let Some((i, left, wait)) = self.sprite_fetch {
            if wait > 0 {
                self.fetcher_step();
            }
            if left <= 1 {
                self.fetch_sprite(usize::from(i));
                self.sprite_fetch = None;
            } else {
                self.sprite_fetch = Some((i, left - 1, wait.saturating_sub(1)));
            }
            return;
        }
        self.fetcher_step();
        if self.bg_fifo.is_empty() {
            return;
        }
        if self.discard > 0 {
            self.bg_fifo.pop_front();
            self.discard -= 1;
            return;
        }
        // Fensterstart: FIFO leeren, Fetcher auf das Fenster umstellen.
        if !self.fetcher.window
            && self.lcdc & 0x20 != 0
            && self.wy_triggered
            && self.wx <= 166
            && u16::from(self.lx) + 7 >= u16::from(self.wx)
        {
            self.bg_fifo.clear();
            self.fetcher = Fetcher {
                window: true,
                ..Fetcher::default()
            };
            self.window_on_line = true;
            self.fetcher_step();
            return;
        }
        // Objektabruf: Fetcher beendet seinen Abruf (ohne Schieben), dann 6 Punkte für das Objekt.
        if self.lcdc & 0x02 != 0
            && let Some(i) = self
                .sprites
                .iter()
                .position(|s| !s.fetched && s.x <= self.lx + 8)
        {
            let wait = 5 - self.fetcher.step.min(5);
            self.sprite_fetch = Some((i as u8, wait + 6 - 1, wait.saturating_sub(1)));
            return;
        }
        self.output_pixel();
    }

    fn output_pixel(&mut self) {
        let bg = self.bg_fifo.pop_front().unwrap_or(0);
        let obj = self.obj_fifo[0];
        self.obj_fifo.copy_within(1.., 0);
        self.obj_fifo[7] = ObjPixel::default();
        let bg = if self.lcdc & 0x01 != 0 { bg } else { 0 };
        let shade = if obj.color != 0 && self.lcdc & 0x02 != 0 && !(obj.attr & 0x80 != 0 && bg != 0)
        {
            let pal = if obj.attr & 0x10 != 0 {
                self.obp1
            } else {
                self.obp0
            };
            (pal >> (obj.color * 2)) & 3
        } else {
            (self.bgp >> (bg * 2)) & 3
        };
        if !self.blank_frame {
            let i = usize::from(self.ly) * WIDTH as usize + usize::from(self.lx);
            self.lines[i] = u16::from(shade);
        }
        self.lx += 1;
        if self.lx == WIDTH as u8 {
            self.mode = 0;
        }
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
                0x80 | self.stat | coinc | self.mode
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
                    self.mode = 0;
                    self.off_dots = 0;
                    self.stat_line = false;
                    self.lines.fill(0);
                } else if !was_on && self.enabled() {
                    self.ly = 0;
                    self.dot = 0;
                    self.mode = 0;
                    self.window_line = 0;
                    self.wy_triggered = false;
                    self.blank_frame = true;
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
            self.lcdc,
            self.stat,
            self.scy,
            self.scx,
            self.ly,
            self.lyc,
            self.bgp,
            self.obp0,
            self.obp1,
            self.wy,
            self.wx,
            self.mode,
            self.lx,
            self.discard,
            self.delay,
            self.window_line,
        ] {
            w.u8(v);
        }
        w.u32(self.dot);
        w.u32(self.off_dots);
        for v in [
            self.stat_line,
            self.frame_done,
            self.blank_frame,
            self.wy_triggered,
            self.window_on_line,
        ] {
            w.bool(v);
        }
        w.u8(self.sprites.len() as u8);
        for s in &self.sprites {
            for v in [s.y, s.x, s.tile, s.attr, s.index] {
                w.u8(v);
            }
            w.bool(s.fetched);
        }
        w.blob(&self.bg_fifo.iter().copied().collect::<Vec<_>>());
        for p in &self.obj_fifo {
            w.u8(p.color);
            w.u8(p.attr);
        }
        let f = &self.fetcher;
        for v in [f.step, f.x, f.tile, f.lo, f.hi] {
            w.u8(v);
        }
        w.bool(f.window);
        w.option(self.sprite_fetch.as_ref(), |w, &(i, l, wt)| {
            w.u8(i);
            w.u8(l);
            w.u8(wt);
        });
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
            &mut self.mode,
            &mut self.lx,
            &mut self.discard,
            &mut self.delay,
            &mut self.window_line,
        ] {
            *v = r.u8()?;
        }
        self.dot = r.u32()?;
        self.off_dots = r.u32()?;
        for v in [
            &mut self.stat_line,
            &mut self.frame_done,
            &mut self.blank_frame,
            &mut self.wy_triggered,
            &mut self.window_on_line,
        ] {
            *v = r.bool()?;
        }
        let n = r.u8()?;
        self.sprites.clear();
        for _ in 0..n {
            let mut s = Sprite {
                y: r.u8()?,
                x: r.u8()?,
                tile: r.u8()?,
                attr: r.u8()?,
                index: r.u8()?,
                fetched: false,
            };
            s.fetched = r.bool()?;
            self.sprites.push(s);
        }
        self.bg_fifo = r.blob()?.into();
        for p in &mut self.obj_fifo {
            p.color = r.u8()?;
            p.attr = r.u8()?;
        }
        let f = &mut self.fetcher;
        for v in [&mut f.step, &mut f.x, &mut f.tile, &mut f.lo, &mut f.hi] {
            *v = r.u8()?;
        }
        f.window = r.bool()?;
        self.sprite_fetch = r.option(|r| Ok((r.u8()?, r.u8()?, r.u8()?)))?;
        self.lines = r.u16_vec()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Länge von Mode 3 in Zeile 1.
    fn mode3_length(ppu: &mut Ppu) -> u32 {
        while !(ppu.ly == 1 && ppu.mode == 3) {
            ppu.tick(1);
        }
        // Der Punkt, in dem Mode 3 begann, zählt mit.
        let mut n = 1;
        while ppu.mode == 3 {
            ppu.tick(1);
            n += 1;
        }
        n
    }

    fn ppu() -> Ppu {
        Ppu {
            lcdc: 0x93,
            ..Ppu::default()
        }
    }

    #[test]
    fn mode3_dauert_172_punkte() {
        assert_eq!(mode3_length(&mut ppu()), 172);
    }

    #[test]
    fn scx_verlaengert_mode3() {
        let mut p = ppu();
        p.scx = 3;
        assert_eq!(mode3_length(&mut p), 175);
    }

    #[test]
    fn objekt_am_zeilenanfang_kostet_11_punkte() {
        let mut p = ppu();
        p.oam[0] = 16; // Y: Zeilen 0–7
        p.oam[1] = 8; // X: Bildschirmspalte 0
        assert_eq!(mode3_length(&mut p), 183);
    }

    #[test]
    fn fenster_kostet_6_punkte() {
        let mut p = ppu();
        p.lcdc |= 0x20;
        p.wx = 7 + 80;
        assert_eq!(mode3_length(&mut p), 178);
    }

    #[test]
    fn zehn_objekte_je_zeile() {
        let mut p = ppu();
        for i in 0..12 {
            p.oam[i * 4] = 16;
            p.oam[i * 4 + 1] = 8 + i as u8 * 8;
        }
        while p.dot != 80 {
            p.tick(1);
        }
        assert_eq!(p.sprites.len(), 10);
    }
}
