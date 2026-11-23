//! Linkkabel: zwei Geräte tauschen Bytes über die serielle Schnittstelle.

mod common;

use common::rom;
use kupocode_emulator_gbc::GbLink;
use kupocode_emulator_gbc::core::{DebugTarget, Machine, StopReason};

/// Sendet `byte` mit Takt `sc` (0x81 intern, 0x80 extern), wartet, legt den Empfang in B ab.
fn transfer(byte: u8, sc: u8) -> Vec<u8> {
    rom(
        &[
            0x3E, byte, 0xE0, 0x01, // SB = byte
            0x3E, sc, 0xE0, 0x02, // SC
            0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA, // warten, bis Bit 7 frei
            0xF0, 0x01, 0x47, // B = SB
            0x18, 0xFE,
        ],
        &[],
    )
}

#[test]
fn zwei_geraete_tauschen_ein_byte() {
    let mut link = GbLink::new(&transfer(0x42, 0x81), &transfer(0x99, 0x80)).unwrap();
    for _ in 0..3 {
        assert_eq!(link.run_frame(), StopReason::FrameDone);
    }
    assert_eq!(link.gbs[0].cpu.b, 0x99);
    assert_eq!(link.gbs[1].cpu.b, 0x42);
    // Beide Geräte liegen zeitlich höchstens einen Schritt auseinander.
    let diff = link.gbs[0].cycles().abs_diff(link.gbs[1].cycles());
    assert!(diff <= 24, "{diff}");
}

#[test]
fn bild_nebeneinander_und_geraetewahl() {
    let mut link = GbLink::new(&transfer(1, 0x81), &transfer(2, 0x80)).unwrap();
    link.run_frame();
    assert_eq!(
        (link.framebuffer().width(), link.framebuffer().height()),
        (320, 144)
    );
    assert_eq!(link.cpus().len(), 2);
    assert!(link.select_cpu(1));
    assert_eq!(link.selected_cpu(), 1);
}

#[test]
fn save_state_ueber_beide_geraete() {
    let mut link = GbLink::new(&transfer(0x42, 0x81), &transfer(0x99, 0x80)).unwrap();
    link.run(200);
    let state = link.save_state_bytes();
    link.run_frame();
    link.run_frame();
    let b = (link.gbs[0].cpu.b, link.gbs[1].cpu.b, link.cycles());
    link.load_state_bytes(&state).unwrap();
    link.run_frame();
    link.run_frame();
    assert_eq!((link.gbs[0].cpu.b, link.gbs[1].cpu.b, link.cycles()), b);
}
