//! Tests der ganzen Maschine: Start, Peripherie, Debugger und Save-States.

mod common;

use common::{mooneye_pass, rom, rom_with_flags, serial_print};
use kupocode_emulator_gbc::core::{DebugTarget, Machine, StopReason};
use kupocode_emulator_gbc::{Gb, Model, TestResult};

#[test]
fn hle_start_setzt_register_je_modell() {
    let gb = Gb::from_rom(&rom(&[0x18, 0xFE], &[])).unwrap();
    assert_eq!(gb.model(), Model::Dmg);
    let c = &gb.cpu;
    assert_eq!(
        (c.af(), c.bc(), c.de(), c.hl(), c.sp, c.pc),
        (0x01B0, 0x0013, 0x00D8, 0x014D, 0xFFFE, 0x0100)
    );
    assert_eq!(gb.bus.peek(0xFF40), 0x91);

    let gb = Gb::from_rom(&rom_with_flags(&[0x18, 0xFE], &[], 0x80, 0)).unwrap();
    assert_eq!((gb.model(), gb.cpu.af()), (Model::Cgb, 0x1180));

    let gb = Gb::from_rom(&rom_with_flags(&[0x18, 0xFE], &[], 0, 0x03)).unwrap();
    assert_eq!((gb.model(), gb.cpu.bc()), (Model::Sgb, 0x0014));
}

#[test]
fn serielle_ausgabe_und_blargg_ergebnis() {
    let mut gb = Gb::from_rom(&rom(&serial_print("Passed"), &[])).unwrap();
    assert_eq!(gb.run_test(60), Some(TestResult::Passed));
    assert_eq!(gb.tty_output(), "Passed");
}

#[test]
fn mooneye_ergebnis() {
    let mut gb = Gb::from_rom(&rom(&mooneye_pass(), &[])).unwrap();
    assert_eq!(gb.run_test(10), Some(TestResult::Passed));
}

/// TIMA kurz vor dem Überlauf, Timer-Unterbrechung an, HALT; die Routine setzt B = $55.
fn timer_programm() -> Vec<u8> {
    rom(
        &[
            0x3E, 0x05, 0xE0, 0x07, // TAC = an, 262144 Hz
            0x3E, 0xFE, 0xE0, 0x05, // TIMA = $FE
            0x3E, 0x04, 0xE0, 0xFF, // IE = Timer
            0xAF, 0xE0, 0x0F, // IF = 0
            0xFB, 0x76, // EI; HALT
            0x18, 0xFE,
        ],
        &[(0x50, &[0x06, 0x55, 0xD9])],
    )
}

#[test]
fn timer_unterbrechung_weckt_aus_halt() {
    let mut gb = Gb::from_rom(&timer_programm()).unwrap();
    gb.run_frame();
    assert_eq!(gb.cpu.b, 0x55);
    assert_eq!(gb.call_stack().len(), 0);
}

#[test]
fn oam_dma_kopiert_160_bytes() {
    // LD A,$C0; LDH (DMA),A; LD B,41 (Startverzug der DMA eingerechnet); warten: DEC B; JR NZ,warten; JR $
    let code = [
        0x3E, 0xC0, 0xE0, 0x46, 0x06, 41, 0x05, 0x20, 0xFD, 0x18, 0xFE,
    ];
    let mut gb = Gb::from_rom(&rom(&code, &[])).unwrap();
    let data: Vec<u8> = (0..160).map(|i| i as u8 ^ 0x5A).collect();
    gb.poke(0xC000, &data).unwrap();
    assert_eq!(gb.run(1000), StopReason::Halted);
    assert_eq!(&gb.bus.ppu.oam[..], &data[..]);
}

#[test]
fn save_state_ist_deterministisch() {
    let mut gb = Gb::from_rom(&rom(&serial_print("Hallo Welt"), &[])).unwrap();
    gb.run_frame();
    let state = gb.save_state_bytes();
    for _ in 0..5 {
        gb.run_frame();
    }
    let (cycles, tty, pc) = (gb.cycles(), gb.tty_output().to_string(), gb.cpu.pc);
    gb.load_state_bytes(&state).unwrap();
    for _ in 0..5 {
        gb.run_frame();
    }
    assert_eq!(
        (gb.cycles(), gb.tty_output(), gb.cpu.pc),
        (cycles, tty.as_str(), pc)
    );
}

#[test]
fn bild_endet_auch_bei_ausgeschaltetem_lcd() {
    // LCDC = 0; JR $
    let mut gb = Gb::from_rom(&rom(&[0xAF, 0xE0, 0x40, 0x18, 0xFE], &[])).unwrap();
    assert_eq!(gb.run_frame(), StopReason::FrameDone);
    let before = gb.cycles();
    assert_eq!(gb.run_frame(), StopReason::FrameDone);
    let diff = gb.cycles() - before;
    assert!((70224..70224 + 32).contains(&diff), "{diff}");
}

#[test]
fn echtes_boot_rom_wird_ausgeblendet() {
    // Boot-ROM: LD A,1; LDH ($50),A → danach NOPs der Kassette bis 0x100.
    let mut boot = vec![0u8; 0x100];
    boot[..4].copy_from_slice(&[0x3E, 0x01, 0xE0, 0x50]);
    let mut gb = Gb::from_rom_with(&rom(&mooneye_pass(), &[]), None, Some(boot)).unwrap();
    assert_eq!(gb.cpu.pc, 0);
    assert_eq!(gb.bus.peek(0x0000), 0x3E);
    assert_eq!(gb.run_test(10), Some(TestResult::Passed));
    assert_eq!(gb.bus.peek(0x0000), 0x00);
}

#[test]
fn breakpoint_und_step_over() {
    // CALL $0200; JR $   ($0200: NOP; RET)
    let mut gb = Gb::from_rom(&rom(
        &[0xCD, 0x00, 0x02, 0x18, 0xFE],
        &[(0x200, &[0x00, 0xC9])],
    ))
    .unwrap();
    gb.breakpoints_mut().add(0x150);
    assert_eq!(gb.run_frame(), StopReason::Breakpoint { addr: 0x150 });
    assert_eq!(gb.step_over(), StopReason::StepDone);
    assert_eq!(gb.cpu.pc, 0x153);
    gb.reset();
    gb.breakpoints_mut().clear();
    gb.breakpoints_mut().add(0x200);
    assert_eq!(gb.run_frame(), StopReason::Breakpoint { addr: 0x200 });
    assert_eq!(gb.call_stack().len(), 1);
    assert_eq!(gb.step_out(), StopReason::StepDone);
    assert_eq!(gb.cpu.pc, 0x153);
}

#[test]
fn ton_liefert_44100_abtastwerte_je_sekunde() {
    use kupocode_emulator_gbc::core::audio::AudioBuffer;
    let mut gb = Gb::from_rom(&rom(&[0x18, 0xFE], &[])).unwrap();
    let mut audio = AudioBuffer::new(44_100);
    // Das erste Bild nach dem Start endet schon bei Zeile 144; erst danach messen.
    gb.run_frame();
    gb.drain_audio(&mut audio);
    audio.clear();
    for _ in 0..60 {
        gb.run_frame();
        gb.drain_audio(&mut audio);
    }
    // 60 Bilder à 70224 Takte bei 4,194304 MHz ≈ 1,0046 s
    let expected = 44_100.0 * 60.0 * 70_224.0 / 4_194_304.0;
    assert!(
        (audio.frames() as f64 - expected).abs() < 3.0,
        "{}",
        audio.frames()
    );
}
