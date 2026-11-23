//! Super Game Boy: Befehle über `P1`, Paletten, Attribute, Rahmen und Mehrspieler.
//!
//! Die SNES-Seite wird per HLE nachgebildet; Ton- und Datenbefehle (`SOUND`, `SOU_TRN`,
//! `DATA_SND`, `DATA_TRN`, `JUMP`, `OBJ_TRN`) werden angenommen, aber ignoriert.
//!
//! **Pakete:** Ein Impuls mit `P14 = P15 = 0` beginnt ein Paket; danach je Bit ein Impuls auf
//! `P14` (0) oder `P15` (1), jeweils gefolgt von `P14 = P15 = 1`. 16 Bytes, LSB zuerst, dann ein
//! Stoppbit. Das erste Byte enthält Befehl (Bits 3–7) und Paketanzahl (Bits 0–2).
//!
//! **Übertragungen über das Bild** (`*_TRN`): Das Spiel zeigt 4 KB als 256 Kacheln (20 je Zeile)
//! an; das SGB liest sie aus dem nächsten Bild zurück. Hier geschieht das aus den Graustufen des
//! gerenderten Bildes, also genau wie auf der Hardware nach der Palette `BGP`.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};
use kupocode_emulator_core::video::FrameBuffer;

use crate::gb::rgb555;

/// Ausgabe des SGB.
pub const WIDTH: u32 = 256;
pub const HEIGHT: u32 = 224;
/// Lage des Game-Boy-Bildes im Rahmen.
const OFFSET_X: u32 = 48;
const OFFSET_Y: u32 = 40;

/// Graustufen als Ausgangspaletten (das SNES-BIOS ist nicht nachgebildet).
const DEFAULT_PALETTE: [u16; 4] = [0x7FFF, 0x56B5, 0x294A, 0x0000];

#[derive(Debug, Clone)]
pub struct Sgb {
    // ── Paketempfang ──
    receiving: bool,
    bits: u16,
    packet: [u8; 16],
    packets: Vec<[u8; 16]>,
    last_p1: u8,
    // ── Mehrspieler ──
    /// Anzahl Spieler − 1 (0, 1 oder 3).
    pub players: u8,
    pub player: u8,
    // ── Bild ──
    /// Aktive Paletten 0–3.
    pub palettes: [[u16; 4]; 4],
    /// Systempaletten aus `PAL_TRN` (512 × 4 Farben).
    sys_palettes: Vec<u16>,
    /// Palette je 8×8-Feld (20 × 18).
    pub attr: [u8; 360],
    /// Attributdateien aus `ATTR_TRN` (45 × 90 Byte).
    atf: Vec<u8>,
    /// 0 = aus, 1 = Bild einfrieren, 2 = schwarz, 3 = Farbe 0.
    pub mask: u8,
    /// Rahmenkacheln (256 SNES-Kacheln à 32 Byte, 4 Bit je Pixel).
    border_tiles: Vec<u8>,
    /// Rahmenkarte 32 × 32 Einträge.
    border_map: Vec<u16>,
    /// Rahmenpaletten 4–7 (je 16 Farben).
    border_pal: [[u16; 16]; 4],
    /// Ausstehende Übertragung über das Bild: Befehl und Parameter.
    pending: Option<(u8, u8)>,
    /// Letztes Game-Boy-Bild in Graustufen (für Einfrieren).
    frozen: Vec<u16>,
}

impl Default for Sgb {
    fn default() -> Self {
        Self {
            receiving: false,
            bits: 0,
            packet: [0; 16],
            packets: Vec::new(),
            last_p1: 0x30,
            players: 0,
            player: 0,
            palettes: [DEFAULT_PALETTE; 4],
            sys_palettes: vec![0; 512 * 4],
            attr: [0; 360],
            atf: vec![0; 45 * 90],
            mask: 0,
            border_tiles: vec![0; 256 * 32],
            border_map: vec![0; 32 * 32],
            border_pal: [[0; 16]; 4],
            pending: None,
            frozen: Vec::new(),
        }
    }
}

fn word(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes([b[i], b[i + 1]])
}

impl Sgb {
    /// Schreiben auf `P1` (Bits 4 und 5).
    pub fn write_p1(&mut self, v: u8) {
        let p = v & 0x30;
        let last = self.last_p1;
        self.last_p1 = p;
        if p == 0x00 {
            self.receiving = true;
            self.bits = 0;
            self.packet = [0; 16];
            return;
        }
        // Mehrspieler: P15 von 0 auf 1 schaltet zum nächsten Spieler.
        if !self.receiving && self.players > 0 && last & 0x20 == 0 && p & 0x20 != 0 {
            self.player = (self.player + 1) & self.players;
        }
        if !self.receiving || last != 0x30 || p == 0x30 {
            return;
        }
        let bit = p == 0x10;
        if self.bits < 128 {
            if bit {
                self.packet[usize::from(self.bits / 8)] |= 1 << (self.bits % 8);
            }
            self.bits += 1;
            return;
        }
        // Stoppbit
        self.receiving = false;
        if bit {
            return;
        }
        self.packets.push(self.packet);
        let count = usize::from((self.packets[0][0] & 7).max(1));
        if self.packets.len() >= count {
            let data: Vec<u8> = self.packets.drain(..).flatten().collect();
            self.command(&data);
        }
    }

    /// Unterer Teil von `P1` beim Lesen ohne gewählte Tastenzeile: Spielerkennung.
    pub fn player_id(&self) -> Option<u8> {
        (self.players > 0).then_some(0x0F - self.player)
    }

    fn set_pair(&mut self, a: usize, b: usize, d: &[u8]) {
        let c0 = word(d, 1);
        for p in &mut self.palettes {
            p[0] = c0;
        }
        for i in 0..3 {
            self.palettes[a][i + 1] = word(d, 3 + i * 2);
            self.palettes[b][i + 1] = word(d, 9 + i * 2);
        }
    }

    fn set_block(&mut self, x1: u8, y1: u8, x2: u8, y2: u8, pal: [Option<u8>; 3]) {
        for y in 0..18u8 {
            for x in 0..20u8 {
                let inside = x > x1 && x < x2 && y > y1 && y < y2;
                let border = (x >= x1 && x <= x2 && y >= y1 && y <= y2) && !inside;
                let slot = if inside {
                    pal[0]
                } else if border {
                    pal[1]
                } else {
                    pal[2]
                };
                if let Some(p) = slot {
                    self.attr[usize::from(y) * 20 + usize::from(x)] = p;
                }
            }
        }
    }

    fn apply_atf(&mut self, n: u8) {
        let base = usize::from(n.min(44)) * 90;
        for i in 0..360 {
            let byte = self.atf[base + i / 4];
            self.attr[i] = (byte >> (6 - (i % 4) * 2)) & 3;
        }
    }

    fn command(&mut self, d: &[u8]) {
        let cmd = d[0] >> 3;
        match cmd {
            0x00 => self.set_pair(0, 1, d),
            0x01 => self.set_pair(2, 3, d),
            0x02 => self.set_pair(0, 3, d),
            0x03 => self.set_pair(1, 2, d),
            0x04 => {
                // ATTR_BLK
                for k in 0..usize::from(d[1].min(18)) {
                    let s = &d[2 + k * 6..8 + k * 6];
                    let (ctrl, pals) = (s[0], s[1]);
                    let (inside, border, outside) = (ctrl & 1 != 0, ctrl & 2 != 0, ctrl & 4 != 0);
                    let p_in = pals & 3;
                    let p_border = (pals >> 2) & 3;
                    let p_out = (pals >> 4) & 3;
                    // Nur innen oder nur außen gesetzt: Der Rand übernimmt diese Palette.
                    let border_pal = if border {
                        Some(p_border)
                    } else if inside && !outside {
                        Some(p_in)
                    } else if outside && !inside {
                        Some(p_out)
                    } else {
                        None
                    };
                    self.set_block(
                        s[2] & 31,
                        s[3] & 31,
                        s[4] & 31,
                        s[5] & 31,
                        [inside.then_some(p_in), border_pal, outside.then_some(p_out)],
                    );
                }
            }
            0x05 => {
                // ATTR_LIN
                for &b in d.iter().skip(2).take(usize::from(d[1])) {
                    let line = b & 31;
                    let pal = (b >> 5) & 3;
                    if b & 0x80 != 0 {
                        for x in 0..20 {
                            if let Some(a) = self.attr.get_mut(usize::from(line) * 20 + x) {
                                *a = pal;
                            }
                        }
                    } else if line < 20 {
                        for y in 0..18 {
                            self.attr[y * 20 + usize::from(line)] = pal;
                        }
                    }
                }
            }
            0x06 => {
                // ATTR_DIV
                let (after, before, on) = (d[1] & 3, (d[1] >> 2) & 3, (d[1] >> 4) & 3);
                let horizontal = d[1] & 0x40 != 0;
                let pos = d[2];
                for y in 0..18u8 {
                    for x in 0..20u8 {
                        let c = if horizontal { y } else { x };
                        let pal = match c.cmp(&pos) {
                            std::cmp::Ordering::Less => before,
                            std::cmp::Ordering::Equal => on,
                            std::cmp::Ordering::Greater => after,
                        };
                        self.attr[usize::from(y) * 20 + usize::from(x)] = pal;
                    }
                }
            }
            0x07 => {
                // ATTR_CHR
                let (mut x, mut y) = (usize::from(d[1].min(19)), usize::from(d[2].min(17)));
                let count = usize::from(word(d, 3)).min(360);
                let vertical = d[5] & 1 != 0;
                for i in 0..count {
                    let Some(&byte) = d.get(6 + i / 4) else { break };
                    self.attr[y * 20 + x] = (byte >> (6 - (i % 4) * 2)) & 3;
                    if vertical {
                        y += 1;
                        if y == 18 {
                            y = 0;
                            x = (x + 1) % 20;
                        }
                    } else {
                        x += 1;
                        if x == 20 {
                            x = 0;
                            y = (y + 1) % 18;
                        }
                    }
                }
            }
            0x0A => {
                // PAL_SET
                for p in 0..4 {
                    let n = usize::from(word(d, 1 + p * 2) & 0x1FF);
                    for c in 0..4 {
                        self.palettes[p][c] = self.sys_palettes[n * 4 + c];
                    }
                }
                if d[9] & 0x80 != 0 {
                    self.apply_atf(d[9] & 0x3F);
                }
                if d[9] & 0x40 != 0 {
                    self.mask = 0;
                }
            }
            0x0B | 0x13 | 0x14 | 0x15 => self.pending = Some((cmd, d[1])),
            0x11 => {
                self.players = match d[1] & 3 {
                    1 => 1,
                    3 => 3,
                    _ => 0,
                };
                self.player = 0;
            }
            0x16 => {
                // ATTR_SET
                self.apply_atf(d[1] & 0x3F);
                if d[1] & 0x40 != 0 {
                    self.mask = 0;
                }
            }
            0x17 => self.mask = d[1] & 3,
            // SOUND, SOU_TRN, ATRC_EN, TEST_EN, ICON_EN, DATA_SND, DATA_TRN, JUMP, OBJ_TRN
            _ => {}
        }
    }

    /// Liest 4 KB aus den Graustufen des Bildes (256 Kacheln, 20 je Zeile).
    fn screen_data(lines: &[u16]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4096);
        for tile in 0..256usize {
            let (tx, ty) = (tile % 20, tile / 20);
            for row in 0..8 {
                let (mut lo, mut hi) = (0u8, 0u8);
                for px in 0..8 {
                    let i = (ty * 8 + row) * 160 + tx * 8 + px;
                    let shade = lines.get(i).copied().unwrap_or(0) as u8;
                    lo |= (shade & 1) << (7 - px);
                    hi |= ((shade >> 1) & 1) << (7 - px);
                }
                out.push(lo);
                out.push(hi);
            }
        }
        out
    }

    /// Am Ende eines Bildes: ausstehende Übertragung ausführen.
    pub fn end_frame(&mut self, lines: &[u16]) {
        let Some((cmd, param)) = self.pending.take() else {
            if self.mask != 1 {
                self.frozen = lines.to_vec();
            }
            return;
        };
        let data = Self::screen_data(lines);
        match cmd {
            0x0B => {
                for (i, c) in self.sys_palettes.iter_mut().enumerate() {
                    *c = word(&data, i * 2) & 0x7FFF;
                }
            }
            0x13 => {
                let base = if param & 1 != 0 { 128 * 32 } else { 0 };
                self.border_tiles[base..base + 4096].copy_from_slice(&data);
            }
            0x14 => {
                for (i, m) in self.border_map.iter_mut().enumerate() {
                    *m = word(&data, i * 2);
                }
                for p in 0..4 {
                    for c in 0..16 {
                        self.border_pal[p][c] = word(&data, 0x800 + (p * 16 + c) * 2) & 0x7FFF;
                    }
                }
            }
            _ => self.atf.copy_from_slice(&data[..45 * 90]),
        }
        if self.mask != 1 {
            self.frozen = lines.to_vec();
        }
    }

    fn border_pixel(&self, x: u32, y: u32) -> Option<u16> {
        let entry = self.border_map[(y / 8 * 32 + x / 8) as usize];
        let tile = usize::from(entry & 0xFF);
        let pal = usize::from((entry >> 10) & 7).saturating_sub(4).min(3);
        let (mut px, mut py) = (x % 8, y % 8);
        if entry & 0x4000 != 0 {
            px = 7 - px;
        }
        if entry & 0x8000 != 0 {
            py = 7 - py;
        }
        let t = &self.border_tiles[tile * 32..tile * 32 + 32];
        let bit = 7 - px;
        let row = py as usize * 2;
        let c = (t[row] >> bit & 1)
            | (t[row + 1] >> bit & 1) << 1
            | (t[16 + row] >> bit & 1) << 2
            | (t[16 + row + 1] >> bit & 1) << 3;
        (c != 0).then(|| self.border_pal[pal][usize::from(c)])
    }

    /// Setzt das Ausgabebild (256 × 224) aus Rahmen und Game-Boy-Bild zusammen.
    pub fn compose(&self, lines: &[u16], fb: &mut FrameBuffer) {
        if fb.width() != WIDTH || fb.height() != HEIGHT {
            fb.resize(WIDTH, HEIGHT);
        }
        let backdrop = self.palettes[0][0];
        let screen = if self.mask == 1 && !self.frozen.is_empty() {
            &self.frozen[..]
        } else {
            lines
        };
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let inside = (OFFSET_X..OFFSET_X + 160).contains(&x)
                    && (OFFSET_Y..OFFSET_Y + 144).contains(&y);
                let color = if let Some(c) = self.border_pixel(x, y) {
                    c
                } else if inside {
                    let (gx, gy) = (x - OFFSET_X, y - OFFSET_Y);
                    match self.mask {
                        2 => 0,
                        3 => backdrop,
                        _ => {
                            let shade = screen[(gy * 160 + gx) as usize] & 3;
                            let pal = self.attr[(gy / 8 * 20 + gx / 8) as usize];
                            self.palettes[usize::from(pal)][usize::from(shade)]
                        }
                    }
                } else {
                    backdrop
                };
                fb.set_pixel(x, y, rgb555(color));
            }
        }
    }

    pub fn save_state(&self, w: &mut StateWriter) {
        w.bool(self.receiving);
        w.u16(self.bits);
        w.bytes(&self.packet);
        w.blob(&self.packets.concat());
        w.u8(self.last_p1);
        w.u8(self.players);
        w.u8(self.player);
        w.u16_slice(&self.palettes.concat());
        w.u16_slice(&self.sys_palettes);
        w.bytes(&self.attr);
        w.blob(&self.atf);
        w.u8(self.mask);
        w.blob(&self.border_tiles);
        w.u16_slice(&self.border_map);
        w.u16_slice(&self.border_pal.concat());
        w.option(self.pending.as_ref(), |w, &(c, p)| {
            w.u8(c);
            w.u8(p);
        });
        w.u16_slice(&self.frozen);
    }

    pub fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        let bad = |what: &str| StateError::Invalid(format!("SGB: {what}"));
        self.receiving = r.bool()?;
        self.bits = r.u16()?;
        r.bytes_into(&mut self.packet)?;
        let packets = r.blob()?;
        self.packets = packets
            .chunks(16)
            .map(|c| c.try_into().map_err(|_| bad("Pakete")))
            .collect::<Result<_, _>>()?;
        self.last_p1 = r.u8()?;
        self.players = r.u8()?;
        self.player = r.u8()?;
        let pals = r.u16_vec()?;
        if pals.len() != 16 {
            return Err(bad("Paletten"));
        }
        for (i, c) in pals.into_iter().enumerate() {
            self.palettes[i / 4][i % 4] = c;
        }
        self.sys_palettes = r.u16_vec()?;
        r.bytes_into(&mut self.attr)?;
        self.atf = r.blob()?;
        self.mask = r.u8()?;
        self.border_tiles = r.blob()?;
        self.border_map = r.u16_vec()?;
        let bp = r.u16_vec()?;
        if bp.len() != 64 || self.border_tiles.len() != 256 * 32 || self.border_map.len() != 1024 {
            return Err(bad("Rahmen"));
        }
        for (i, c) in bp.into_iter().enumerate() {
            self.border_pal[i / 16][i % 16] = c;
        }
        self.pending = r.option(|r| Ok((r.u8()?, r.u8()?)))?;
        self.frozen = r.u16_vec()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sendet Pakete über `P1` wie ein Spiel.
    pub fn send(sgb: &mut Sgb, data: &[u8]) {
        for packet in data.chunks(16) {
            sgb.write_p1(0x00);
            sgb.write_p1(0x30);
            for i in 0..128 {
                let bit = packet.get(i / 8).copied().unwrap_or(0) >> (i % 8) & 1;
                sgb.write_p1(if bit != 0 { 0x10 } else { 0x20 });
                sgb.write_p1(0x30);
            }
            sgb.write_p1(0x20);
            sgb.write_p1(0x30);
        }
    }

    fn packet(cmd: u8, len: u8, args: &[u8]) -> Vec<u8> {
        let mut d = vec![0u8; 16 * usize::from(len)];
        d[0] = cmd << 3 | len;
        d[1..1 + args.len()].copy_from_slice(args);
        d
    }

    #[test]
    fn pal01_setzt_zwei_paletten() {
        let mut s = Sgb::default();
        let args = [
            0x1F, 0x00, 0xE0, 0x03, 0x00, 0x7C, 0x00, 0x00, 0x11, 0x11, 0x22, 0x22, 0x33, 0x33,
        ];
        send(&mut s, &packet(0x00, 1, &args));
        assert_eq!(s.palettes[0], [0x001F, 0x03E0, 0x7C00, 0x0000]);
        assert_eq!(s.palettes[1], [0x001F, 0x1111, 0x2222, 0x3333]);
        assert_eq!(s.palettes[2][0], 0x001F);
    }

    #[test]
    fn attr_blk_innen_und_rand() {
        let mut s = Sgb::default();
        // ein Datensatz: nur innen (→ Rand gleich), Palette 2, Feld (2,2)–(5,5)
        send(&mut s, &packet(0x04, 1, &[1, 0x01, 0x02, 2, 2, 5, 5]));
        assert_eq!(s.attr[3 * 20 + 3], 2);
        assert_eq!(s.attr[2 * 20 + 2], 2);
        assert_eq!(s.attr[6 * 20 + 6], 0);
    }

    #[test]
    fn mehrspieler_wechselt_spieler() {
        let mut s = Sgb::default();
        send(&mut s, &packet(0x11, 1, &[1]));
        assert_eq!(s.player_id(), Some(0x0F));
        // P15 (Bit 5) geht von 0 auf 1: nächster Spieler
        s.write_p1(0x10);
        s.write_p1(0x30);
        assert_eq!(s.player_id(), Some(0x0E));
        s.write_p1(0x10);
        s.write_p1(0x30);
        assert_eq!(s.player_id(), Some(0x0F));
    }

    #[test]
    fn mask_en_und_ausgabegroesse() {
        let mut s = Sgb::default();
        send(&mut s, &packet(0x17, 1, &[2]));
        assert_eq!(s.mask, 2);
        let mut fb = FrameBuffer::new(160, 144);
        s.compose(&vec![0; 160 * 144], &mut fb);
        assert_eq!((fb.width(), fb.height()), (256, 224));
        assert_eq!(fb.pixel(100, 100), [0, 0, 0, 255]);
    }

    #[test]
    fn pct_trn_liest_rahmen_aus_dem_bild() {
        let mut s = Sgb::default();
        send(&mut s, &packet(0x14, 1, &[]));
        // Bild: erste Kachel ganz Graustufe 3 → Bytes 0xFF 0xFF …
        let mut lines = vec![0u16; 160 * 144];
        for y in 0..8 {
            for x in 0..8 {
                lines[y * 160 + x] = 3;
            }
        }
        s.end_frame(&lines);
        assert_eq!(s.border_map[0], 0xFFFF);
        assert_eq!(s.border_map[8], 0x0000);
    }
}
