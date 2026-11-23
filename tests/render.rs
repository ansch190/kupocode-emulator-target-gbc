//! Bildtests: Pixel des Framebuffers nach dem Rendern bekannter VRAM-Inhalte.

mod common;

use common::rom;
use kupocode_emulator_gbc::Gb;
use kupocode_emulator_gbc::core::{DebugTarget, Machine};

const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

/// Maschine im Leerlauf; Kachel 1 ist voll schwarz (Farbe 3), Kachel 2 hat Farbe 1.
fn gb() -> Gb {
    let mut gb = Gb::from_rom(&rom(&[0x18, 0xFE], &[])).unwrap();
    gb.poke(0x8010, &[0xFF; 16]).unwrap();
    gb.poke(0x8020, &[0xFF, 0x00].repeat(8)).unwrap();
    gb
}

fn frame(gb: &mut Gb) {
    gb.run_frame();
    gb.run_frame();
}

#[test]
fn hintergrundkachel() {
    let mut gb = gb();
    gb.poke(0x9800, &[1]).unwrap();
    frame(&mut gb);
    let fb = gb.framebuffer();
    assert_eq!(fb.pixel(0, 0), BLACK);
    assert_eq!(fb.pixel(7, 7), BLACK);
    assert_eq!(fb.pixel(8, 0), WHITE);
    assert_eq!(fb.pixel(0, 8), WHITE);
}

#[test]
fn feines_scrollen() {
    let mut gb = gb();
    gb.poke(0x9800, &[1]).unwrap();
    gb.poke(0xFF43, &[3]).unwrap();
    frame(&mut gb);
    let fb = gb.framebuffer();
    assert_eq!(fb.pixel(4, 0), BLACK);
    assert_eq!(fb.pixel(5, 0), WHITE);
}

#[test]
fn objekt_und_prioritaet() {
    let mut gb = gb();
    // Hintergrund ganz in Farbe 1, Objekt 0 schwarz bei (10, 20), Objekt 1 mit BG-Priorität.
    gb.poke(0x9800, &[2; 32 * 18]).unwrap();
    gb.poke(0xFE00, &[36, 18, 1, 0x00, 36, 60, 1, 0x80])
        .unwrap();
    gb.poke(0xFF40, &[0x93]).unwrap();
    gb.poke(0xFF47, &[0xE4, 0xE4]).unwrap(); // BGP, OBP0: Farbe n → Graustufe n
    frame(&mut gb);
    let fb = gb.framebuffer();
    assert_eq!(fb.pixel(10, 20), BLACK);
    assert_eq!(fb.pixel(17, 27), BLACK);
    assert_eq!(fb.pixel(18, 20), [170, 170, 170, 255]);
    // BG-Farbe 1 ≠ 0 verdeckt das Objekt mit Priorität.
    assert_eq!(fb.pixel(52, 20), [170, 170, 170, 255]);
}

#[test]
fn fenster() {
    let mut gb = gb();
    gb.poke(0x9C00, &[1; 32]).unwrap();
    gb.poke(0xFF4A, &[50, 7 + 100]).unwrap();
    gb.poke(0xFF40, &[0xF1]).unwrap(); // LCD, Fenster an Karte 1, Kacheln $8000, BG an
    frame(&mut gb);
    let fb = gb.framebuffer();
    assert_eq!(fb.pixel(99, 50), WHITE);
    assert_eq!(fb.pixel(100, 50), BLACK);
    assert_eq!(fb.pixel(100, 49), WHITE);
}

#[test]
fn ansichten_fuer_den_debugger() {
    let gb = gb();
    let names: Vec<String> = gb.graphics_views().into_iter().map(|v| v.name).collect();
    assert_eq!(names[0], "Kacheln (Bank 0)");
    assert!(names.iter().any(|n| n == "OAM"));
}
