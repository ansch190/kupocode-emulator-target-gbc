//! Tests der CGB-Funktionen: Paletten, Attribute, VRAM-/WRAM-Banken, DMA, Taktumschaltung,
//! Kompatibilitätsmodus und Objektpriorität.

mod common;

use common::{rom, rom_with_flags};
use kupocode_emulator_gbc::core::{DebugTarget, Machine};
use kupocode_emulator_gbc::{Gb, Model};

const RED: u16 = 0x001F;
const GREEN: u16 = 0x03E0;
const BLUE: u16 = 0x7C00;

fn cgb(code: &[u8]) -> Gb {
    Gb::from_rom(&rom_with_flags(code, &[], 0x80, 0)).unwrap()
}

/// Schreibt Farben über `BCPS`/`BCPD` (bzw. `OCPS`/`OCPD`) ab Index 0 mit Auto-Inkrement.
fn palettes(gb: &mut Gb, obj: bool, colors: &[u16]) {
    let (spec, data) = if obj {
        (0xFF6A, 0xFF6B)
    } else {
        (0xFF68, 0xFF69)
    };
    gb.poke(spec, &[0x80]).unwrap();
    for c in colors {
        for b in c.to_le_bytes() {
            gb.poke(data, &[b]).unwrap();
        }
    }
}

fn frame(gb: &mut Gb) {
    gb.run_frame();
    gb.run_frame();
}

fn rgb(c: u16) -> [u8; 4] {
    let [r, g, b] = kupocode_emulator_gbc::gb::rgb555(c);
    [r, g, b, 255]
}

#[test]
fn paletten_und_attribute() {
    let mut gb = cgb(&[0x18, 0xFE]);
    assert_eq!(gb.model(), Model::Cgb);
    gb.poke(0x8010, &[0xFF; 16]).unwrap(); // Kachel 1: Farbe 3
    gb.poke(0x9800, &[1, 1]).unwrap();
    gb.poke(0xFF4F, &[1]).unwrap(); // VRAM-Bank 1: Attribute
    gb.poke(0x9801, &[0x01]).unwrap(); // Palette 1
    gb.poke(0xFF4F, &[0]).unwrap();
    palettes(&mut gb, false, &[BLUE, 0, 0, RED, 0, 0, 0, GREEN]);
    assert_eq!(gb.bus.peek(0xFF68), 0xD0); // Index 16 (8 Farben × 2 Byte), Auto-Inkrement
    frame(&mut gb);
    let fb = gb.framebuffer();
    assert_eq!(fb.pixel(0, 0), rgb(RED));
    assert_eq!(fb.pixel(8, 0), rgb(GREEN));
    assert_eq!(fb.pixel(16, 0), rgb(BLUE));
}

#[test]
fn allgemeine_dma_kopiert_ins_vram() {
    let mut gb = cgb(&[0x18, 0xFE]);
    let data: Vec<u8> = (0..32).collect();
    gb.poke(0xC000, &data).unwrap();
    gb.poke(0xFF51, &[0xC0, 0x00, 0x01, 0x00]).unwrap(); // Quelle $C000, Ziel $8100
    gb.poke(0xFF55, &[0x01]).unwrap(); // 2 Blöcke
    assert_eq!(&gb.bus.ppu.vram[0x100..0x120], &data[..]);
    assert_eq!(gb.bus.peek(0xFF55), 0xFF);
}

#[test]
fn hblank_dma_ueberträgt_je_zeile_einen_block() {
    // HDMA starten (4 Blöcke), dann warten
    let mut gb = cgb(&[0x3E, 0x83, 0xE0, 0x55, 0x18, 0xFE]);
    gb.poke(0xD000, &[0xAA; 64]).unwrap();
    gb.poke(0xFF51, &[0xD0, 0x00, 0x00, 0x00]).unwrap();
    gb.run_frame();
    gb.run_frame();
    assert_eq!(&gb.bus.ppu.vram[..64], &[0xAA; 64][..]);
    assert_eq!(gb.bus.peek(0xFF55), 0xFF);
}

#[test]
fn wram_banken() {
    let mut gb = cgb(&[0x18, 0xFE]);
    gb.poke(0xFF70, &[2]).unwrap();
    gb.poke(0xD000, &[0x22]).unwrap();
    gb.poke(0xFF70, &[0]).unwrap(); // 0 wählt Bank 1
    assert_eq!(gb.bus.peek(0xD000), 0x00);
    gb.poke(0xFF70, &[2]).unwrap();
    assert_eq!(gb.bus.peek(0xD000), 0x22);
}

#[test]
fn key1_und_stop_schalten_den_takt_um() {
    // LD A,1; LDH (KEY1),A; STOP; JR $
    let mut gb = cgb(&[0x3E, 0x01, 0xE0, 0x4D, 0x10, 0x00, 0x18, 0xFE]);
    gb.run_frame();
    assert_eq!(gb.bus.peek(0xFF4D), 0xFE);
    // Ein Bild dauert weiter 70224 Takte des Grundtakts.
    let before = gb.cycles();
    gb.run_frame();
    let diff = gb.cycles() - before;
    assert!((70224..70224 + 16).contains(&diff), "{diff}");
}

#[test]
fn dmg_spiel_auf_cgb_im_kompatibilitaetsmodus() {
    let mut gb = Gb::from_rom_with(&rom(&[0x18, 0xFE], &[]), Some(Model::Cgb), None).unwrap();
    gb.poke(0x8010, &[0xFF; 16]).unwrap();
    gb.poke(0x9800, &[1]).unwrap();
    palettes(&mut gb, false, &[RED; 4]); // gesperrt
    frame(&mut gb);
    let fb = gb.framebuffer();
    assert_eq!(fb.pixel(0, 0), [0, 0, 0, 255]);
    assert_eq!(fb.pixel(8, 0), [255, 255, 255, 255]);
    // CGB-Register wie VBK sind im Kompatibilitätsmodus nicht sichtbar.
    assert_eq!(gb.bus.peek(0xFF4F), 0xFF);
}

#[test]
fn objektprioritaet_nach_oam_index() {
    let mut gb = cgb(&[0x18, 0xFE]);
    gb.poke(0x8010, &[0xFF; 16]).unwrap(); // Kachel 1: Farbe 3
    gb.poke(0x8020, &[0xFF, 0x00].repeat(8)).unwrap(); // Kachel 2: Farbe 1
    // Objekt 0 bei X=12 (Farbe 1), Objekt 1 bei X=8 (Farbe 3): beide decken Spalte 5.
    gb.poke(0xFE00, &[16, 12, 2, 0, 16, 8, 1, 0]).unwrap();
    gb.poke(0xFF40, &[0x93]).unwrap();
    palettes(&mut gb, true, &[0, GREEN, 0, RED]);
    frame(&mut gb);
    assert_eq!(gb.framebuffer().pixel(5, 0), rgb(GREEN));
    // OPRI = 1: Priorität nach X wie beim DMG
    gb.poke(0xFF6C, &[1]).unwrap();
    frame(&mut gb);
    assert_eq!(gb.framebuffer().pixel(5, 0), rgb(RED));
}
