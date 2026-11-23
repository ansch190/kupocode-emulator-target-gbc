//! Grafikansichten für den Debugger: Kacheln, Hintergrund- und Fensterkarte, OAM.

use kupocode_emulator_core::debug::GraphicsView;
use kupocode_emulator_core::video::FrameBuffer;

use crate::ppu::{ColorState, Ppu};

/// Graustufen für Farbindizes (DMG-Paletten angewendet).
const SHADES: [[u8; 3]; 4] = [[255, 255, 255], [170, 170, 170], [85, 85, 85], [0, 0, 0]];

fn tile_pixel(vram: &[u8], base: usize, x: usize, y: usize) -> u8 {
    let lo = vram[base + y * 2];
    let hi = vram[base + y * 2 + 1];
    let bit = 7 - x;
    ((hi >> bit) & 1) << 1 | ((lo >> bit) & 1)
}

fn shade(palette: u8, color: u8) -> [u8; 3] {
    SHADES[usize::from((palette >> (color * 2)) & 3)]
}

/// Alle 384 Kacheln einer VRAM-Bank als 16 × 24 Kacheln (128 × 192 Pixel).
fn tiles(ppu: &Ppu, bank: usize) -> GraphicsView {
    let mut img = FrameBuffer::new(128, 192);
    for t in 0..384 {
        let base = bank * 0x2000 + t * 16;
        for y in 0..8 {
            for x in 0..8 {
                let c = tile_pixel(&ppu.vram, base, x, y);
                img.set_pixel(
                    ((t % 16) * 8 + x) as u32,
                    ((t / 16) * 8 + y) as u32,
                    shade(ppu.bgp, c),
                );
            }
        }
    }
    GraphicsView {
        name: format!("Kacheln (Bank {bank})"),
        image: img,
        rows: Vec::new(),
    }
}

/// Eine Kachelkarte (32 × 32 Kacheln) mit der aktuellen Kachelauswahl von `LCDC`.
fn tilemap(ppu: &Ppu, name: &str, map: usize) -> GraphicsView {
    let mut img = FrameBuffer::new(256, 256);
    for ty in 0..32 {
        for tx in 0..32 {
            let tile = ppu.vram[map + ty * 32 + tx];
            let base = if ppu.lcdc & 0x10 != 0 {
                usize::from(tile) * 16
            } else {
                (0x1000 + i32::from(tile as i8) * 16) as usize
            };
            for y in 0..8 {
                for x in 0..8 {
                    let c = tile_pixel(&ppu.vram, base, x, y);
                    img.set_pixel((tx * 8 + x) as u32, (ty * 8 + y) as u32, shade(ppu.bgp, c));
                }
            }
        }
    }
    GraphicsView {
        name: format!("{name} (${:04X})", 0x8000 + map),
        image: img,
        rows: vec![format!(
            "SCX={} SCY={} WX={} WY={} LCDC=${:02X}",
            ppu.scx, ppu.scy, ppu.wx, ppu.wy, ppu.lcdc
        )],
    }
}

/// Alle 40 Objekte als Liste und Bild (8 × 5 Objekte à 8 × 16).
fn oam(ppu: &Ppu) -> GraphicsView {
    let mut img = FrameBuffer::new(64, 80);
    let mut rows = Vec::with_capacity(40);
    let tall = ppu.lcdc & 0x04 != 0;
    for i in 0..40 {
        let o = &ppu.oam[i * 4..i * 4 + 4];
        let (y, x, tile, attr) = (o[0], o[1], o[2], o[3]);
        rows.push(format!(
            "{i:2}: X={x:3} Y={y:3} Kachel=${tile:02X} Attr=${attr:02X}"
        ));
        let pal = if attr & 0x10 != 0 { ppu.obp1 } else { ppu.obp0 };
        let height = if tall { 16 } else { 8 };
        let first = if tall { tile & 0xFE } else { tile };
        for py in 0..height {
            for px in 0..8 {
                let base = usize::from(first) * 16 + (py / 8) * 16;
                let c = tile_pixel(&ppu.vram, base, px, py % 8);
                img.set_pixel(
                    ((i % 8) * 8 + px) as u32,
                    ((i / 8) * 16 + py) as u32,
                    shade(pal, c),
                );
            }
        }
    }
    GraphicsView {
        name: "OAM".into(),
        image: img,
        rows,
    }
}

/// CGB-Paletten: 8 Hintergrund- und 8 Objektpaletten à 4 Farben als Felder (16 × 16 Pixel).
fn palettes(ppu: &Ppu) -> GraphicsView {
    let mut img = FrameBuffer::new(64, 256);
    let mut rows = Vec::new();
    for (kind, pal) in [("BG", &ppu.color.bg_pal), ("OBJ", &ppu.color.obj_pal)] {
        for p in 0..8u8 {
            let colors: Vec<u16> = (0..4).map(|c| ColorState::color(pal, p, c)).collect();
            rows.push(format!(
                "{kind}{p}: {}",
                colors
                    .iter()
                    .map(|c| format!("${c:04X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
            let row = if kind == "BG" { p } else { p + 8 };
            for (c, &col) in colors.iter().enumerate() {
                for y in 0..16 {
                    for x in 0..16 {
                        img.set_pixel(
                            c as u32 * 16 + x,
                            u32::from(row) * 16 + y,
                            crate::gb::rgb555(col),
                        );
                    }
                }
            }
        }
    }
    GraphicsView {
        name: "Paletten".into(),
        image: img,
        rows,
    }
}

/// Alle Ansichten; `cgb` ergänzt die zweite VRAM-Bank und die Paletten.
pub fn views(ppu: &Ppu, cgb: bool) -> Vec<GraphicsView> {
    let mut v = vec![tiles(ppu, 0)];
    if cgb {
        v.push(tiles(ppu, 1));
    }
    v.push(tilemap(ppu, "Karte 0", 0x1800));
    v.push(tilemap(ppu, "Karte 1", 0x1C00));
    v.push(oam(ppu));
    if cgb {
        v.push(palettes(ppu));
    }
    v
}
