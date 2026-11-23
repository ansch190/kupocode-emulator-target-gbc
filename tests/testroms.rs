//! Externe Test-ROMs (Sammlung `c-sp/game-boy-test-roms`) im Ordner aus `KUPO_GB_TESTROMS`.
//!
//! Ohne die Umgebungsvariable wird der Test übersprungen (auch in der CI). Erfolg wird wie in der
//! Anleitung der Sammlung festgestellt: Fibonacci-Register nach `LD B,B` (Mooneye, SameSuite,
//! AGE), serielle Ausgabe (Blargg) oder Vergleich mit dem beiliegenden Bildschirmfoto.
//!
//! Bekannte Abweichungen stehen in [`GB_OFFEN`]; der Test schlägt nur bei neuen Fehlern fehl.
//! `KUPO_GB_OFFEN_AUSGEBEN=1` gibt die aktuelle Liste zum Übernehmen aus.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use common::png;
use kupocode_emulator_gbc::core::Machine;
use kupocode_emulator_gbc::{Gb, Model};

/// Bekannte Abweichungen: `Modell:Pfad` relativ zum Test-ROM-Ordner (Stand Phase 10).
///
/// Offen sind vor allem: Zeitverhalten der PPU um Mode 2/0 und nach dem Einschalten (Mooneye
/// `intr_*`, `lcdon_*`), Registeränderungen mitten in Mode 3 (Mealybug), Feinheiten der APU
/// (SameSuite), Startwerte des HLE-Starts (`boot_div`, `boot_hwio`), OAM-Fehler des DMG.
const GB_OFFEN: &[&str] = &[
    "cgb:mealybug-tearoom-tests/ppu/m3_bgp_change.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_bgp_change_sprites.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_lcdc_bg_en_change.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_lcdc_bg_map_change.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change_variant.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change_scx.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_change.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_win_change.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_lcdc_win_map_change.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_obp0_change.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_scx_high_5_bits.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_scx_low_3_bits.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_scy_change.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_window_timing.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_window_timing_wx_0.gb", // Bild weicht ab
    "cgb:mealybug-tearoom-tests/ppu/m3_wx_4_change_sprites.gb", // Bild weicht ab
    "cgb:mooneye-test-suite/acceptance/ppu/intr_2_0_timing.gb", // Fehlerregister
    "cgb:mooneye-test-suite/acceptance/ppu/intr_2_mode0_timing.gb", // Fehlerregister
    "cgb:mooneye-test-suite/acceptance/ppu/intr_2_mode0_timing_sprites.gb", // Fehlerregister
    "cgb:mooneye-test-suite/acceptance/ppu/intr_2_mode3_timing.gb", // Fehlerregister
    "cgb:mooneye-test-suite/acceptance/ppu/intr_2_oam_ok_timing.gb", // Fehlerregister
    "cgb:mooneye-test-suite/acceptance/ppu/stat_lyc_onoff.gb", // Fehlerregister
    "cgb:mooneye-test-suite/acceptance/timer/rapid_toggle.gb", // Fehlerregister
    "cgb:mooneye-test-suite/misc/bits/unused_hwio-C.gb", // Fehlerregister
    "cgb:mooneye-test-suite/misc/boot_div-cgbABCDE.gb", // Fehlerregister
    "cgb:mooneye-test-suite/misc/boot_hwio-C.gb",      // Fehlerregister
    "cgb:mooneye-test-suite/misc/boot_regs-cgb.gb",    // Fehlerregister
    "cgb:mooneye-test-suite/misc/ppu/vblank_stat_intr-C.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_align.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_align_cpu.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_delay.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_duty.gb",  // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_duty_delay.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_extra_length_clocking-cgb0B.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_freq_change.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_freq_change_timing-A.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_freq_change_timing-cgb0BC.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_freq_change_timing-cgbDE.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_nrx2_glitch.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_nrx2_speed_change.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_restart.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_restart_nrx2_glitch.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_stop_div.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_stop_restart.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_sweep.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_sweep_restart.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_sweep_restart_2.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_volume.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_1/channel_1_volume_div.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_align.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_align_cpu.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_delay.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_duty.gb",  // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_duty_delay.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_extra_length_clocking-cgb0B.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_freq_change.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_nrx2_glitch.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_nrx2_speed_change.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_restart.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_restart_nrx2_glitch.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_stop_div.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_stop_restart.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_volume.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_2/channel_2_volume_div.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_3/channel_3_extra_length_clocking-cgb0.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_3/channel_3_extra_length_clocking-cgbB.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_4/channel_4_align.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_4/channel_4_delay.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_4/channel_4_equivalent_frequencies.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_4/channel_4_extra_length_clocking-cgb0B.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_4/channel_4_freq_change.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_4/channel_4_frequency_alignment.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_4/channel_4_lfsr_7_15.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_4/channel_4_lfsr_restart.gb", // Fehlerregister
    "cgb:same-suite/apu/channel_4/channel_4_lfsr_restart_fast.gb", // Fehlerregister
    "cgb:same-suite/apu/div_trigger_volume_10.gb",     // Fehlerregister
    "cgb:same-suite/apu/div_write_trigger_10.gb",      // Fehlerregister
    "cgb:same-suite/apu/div_write_trigger_volume_10.gb", // Fehlerregister
    "cgb:same-suite/dma/hdma_lcd_off.gb",              // Fehlerregister
    "cgb:same-suite/dma/hdma_mode0.gb",                // Fehlerregister
    "cgb:same-suite/sgb/command_mlt_req.gb",           // Fehlerregister
    "cgb:same-suite/sgb/command_mlt_req_1_incrementing.gb", // Fehlerregister
    "dmg:blargg/dmg_sound/dmg_sound.gb",               // Bild weicht ab
    "dmg:blargg/oam_bug/oam_bug.gb",                   // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_bgp_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_bgp_change_sprites.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_lcdc_bg_en_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_lcdc_bg_map_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_lcdc_obj_en_change_variant.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_lcdc_obj_size_change_scx.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_lcdc_tile_sel_win_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_lcdc_win_en_change_multiple_wx.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_lcdc_win_map_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_obp0_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_scx_high_5_bits.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_scx_low_3_bits.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_scy_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_window_timing.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_window_timing_wx_0.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_wx_4_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_wx_4_change_sprites.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_wx_5_change.gb", // Bild weicht ab
    "dmg:mealybug-tearoom-tests/ppu/m3_wx_6_change.gb", // Bild weicht ab
    "dmg:mooneye-test-suite/acceptance/boot_div-dmgABCmgb.gb", // Fehlerregister
    "dmg:mooneye-test-suite/acceptance/boot_hwio-dmgABCmgb.gb", // Fehlerregister
    "dmg:mooneye-test-suite/acceptance/ppu/intr_1_2_timing-GS.gb", // Fehlerregister
    "dmg:mooneye-test-suite/acceptance/ppu/intr_2_0_timing.gb", // Fehlerregister
    "dmg:mooneye-test-suite/acceptance/ppu/intr_2_mode0_timing.gb", // Fehlerregister
    "dmg:mooneye-test-suite/acceptance/ppu/intr_2_mode0_timing_sprites.gb", // Fehlerregister
    "dmg:mooneye-test-suite/acceptance/ppu/intr_2_mode3_timing.gb", // Fehlerregister
    "dmg:mooneye-test-suite/acceptance/ppu/intr_2_oam_ok_timing.gb", // Fehlerregister
    "dmg:mooneye-test-suite/acceptance/ppu/lcdon_timing-GS.gb", // Fehlerregister
    "dmg:mooneye-test-suite/acceptance/ppu/lcdon_write_timing-GS.gb", // Fehlerregister
    "dmg:mooneye-test-suite/acceptance/ppu/stat_lyc_onoff.gb", // Fehlerregister
    "dmg:mooneye-test-suite/acceptance/timer/rapid_toggle.gb", // Fehlerregister
    "sgb:mooneye-test-suite/acceptance/boot_div-S.gb", // Fehlerregister
    "sgb:mooneye-test-suite/acceptance/boot_div2-S.gb", // Fehlerregister
    "sgb:mooneye-test-suite/acceptance/boot_hwio-S.gb", // Fehlerregister
    "sgb:mooneye-test-suite/acceptance/ppu/intr_1_2_timing-GS.gb", // Fehlerregister
    "sgb:mooneye-test-suite/acceptance/ppu/lcdon_timing-GS.gb", // Fehlerregister
    "sgb:mooneye-test-suite/acceptance/ppu/lcdon_write_timing-GS.gb", // Fehlerregister
];

#[derive(Debug, Clone)]
enum Check {
    /// Mooneye-Register nach `LD B,B`.
    Fibonacci,
    /// „Passed“ in der seriellen Ausgabe.
    Serial,
    /// Bild nach `LD B,B` (oder nach Ablauf der Zeit) wie die PNG-Datei.
    Screen(PathBuf),
}

#[derive(Debug, Clone)]
struct Case {
    rom: PathBuf,
    model: Model,
    check: Check,
    /// Höchstlaufzeit in Bildern.
    frames: u32,
    /// Für Bildvergleiche ohne `LD B,B`: immer die volle Zeit laufen.
    full_time: bool,
}

fn model_name(m: Model) -> &'static str {
    match m {
        Model::Dmg => "dmg",
        Model::Cgb => "cgb",
        Model::Sgb => "sgb",
        Model::Sgb2 => "sgb2",
    }
}

/// Modelle nach der Namenskonvention von Mooneye (`-dmgABC`, `-GS`, `-C`, `-S`, `-cgb` …).
fn mooneye_models(name: &str) -> Vec<Model> {
    let stem = name.trim_end_matches(".gb").trim_end_matches(".gbc");
    let Some((_, suffix)) = stem.rsplit_once('-') else {
        return vec![Model::Dmg, Model::Cgb];
    };
    let mut out = Vec::new();
    let s = suffix;
    if s.contains("dmgABC") || (s.chars().all(|c| c.is_ascii_uppercase()) && s.contains('G')) {
        out.push(Model::Dmg);
    }
    if (s.contains("cgb") || (s.chars().all(|c| c.is_ascii_uppercase()) && s.contains('C')))
        && (!s.contains("cgb0") || s.contains("cgbABCDE"))
    {
        out.push(Model::Cgb);
    }
    if s == "sgb" || (s.chars().all(|c| c.is_ascii_uppercase()) && s.contains('S')) {
        out.push(Model::Sgb);
    }
    if s == "sgb2" {
        out.push(Model::Sgb2);
    }
    if !s.chars().any(|c| c.is_ascii_lowercase()) || out.is_empty() {
        // unbekannte Endung (kein Modellkürzel) → wie ohne Endung
        if out.is_empty() && !["dmg0", "mgb", "cgb0", "A", "agb", "ags"].contains(&s) {
            return vec![Model::Dmg, Model::Cgb];
        }
    }
    out
}

fn roms_in(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            roms_in(&p, out);
        } else if matches!(p.extension().and_then(|e| e.to_str()), Some("gb" | "gbc")) {
            out.push(p);
        }
    }
}

fn cases(root: &Path) -> Vec<Case> {
    let mut cases = Vec::new();
    let secs = |s: u32| s * 60;

    // Blargg
    let b = root.join("blargg");
    for (dir, rom, s) in [
        ("cpu_instrs", "cpu_instrs.gb", 55),
        ("instr_timing", "instr_timing.gb", 2),
        ("mem_timing", "mem_timing.gb", 4),
    ] {
        for model in [Model::Dmg, Model::Cgb] {
            cases.push(Case {
                rom: b.join(dir).join(rom),
                model,
                check: Check::Serial,
                frames: secs(s),
                full_time: false,
            });
        }
    }
    for (rom, png, models, s) in [
        (
            "mem_timing-2/mem_timing.gb",
            "mem_timing-2/mem_timing-dmg-cgb.png",
            &[Model::Dmg, Model::Cgb][..],
            4,
        ),
        (
            "halt_bug.gb",
            "halt_bug-dmg-cgb.png",
            &[Model::Dmg, Model::Cgb][..],
            2,
        ),
        (
            "interrupt_time/interrupt_time.gb",
            "interrupt_time/interrupt_time-cgb.png",
            &[Model::Cgb][..],
            2,
        ),
        (
            "oam_bug/oam_bug.gb",
            "oam_bug/oam_bug-dmg.png",
            &[Model::Dmg][..],
            21,
        ),
        (
            "dmg_sound/dmg_sound.gb",
            "dmg_sound/dmg_sound-dmg.png",
            &[Model::Dmg][..],
            36,
        ),
        (
            "cgb_sound/cgb_sound.gb",
            "cgb_sound/cgb_sound-cgb.png",
            &[Model::Cgb][..],
            37,
        ),
    ] {
        for &model in models {
            cases.push(Case {
                rom: b.join(rom),
                model,
                check: Check::Screen(b.join(png)),
                frames: secs(s),
                full_time: true,
            });
        }
    }

    // Mooneye
    let m = root.join("mooneye-test-suite");
    let mut roms = Vec::new();
    for sub in ["acceptance", "emulator-only", "misc"] {
        roms_in(&m.join(sub), &mut roms);
    }
    for rom in roms {
        let name = rom.file_name().unwrap().to_string_lossy().to_string();
        for model in mooneye_models(&name) {
            cases.push(Case {
                rom: rom.clone(),
                model,
                check: Check::Fibonacci,
                frames: secs(120),
                full_time: false,
            });
        }
    }

    // acid2
    for (rom, png, model) in [
        (
            "dmg-acid2/dmg-acid2.gb",
            "dmg-acid2/dmg-acid2-dmg.png",
            Model::Dmg,
        ),
        (
            "dmg-acid2/dmg-acid2.gb",
            "dmg-acid2/dmg-acid2-cgb.png",
            Model::Cgb,
        ),
        (
            "cgb-acid2/cgb-acid2.gbc",
            "cgb-acid2/cgb-acid2.png",
            Model::Cgb,
        ),
    ] {
        cases.push(Case {
            rom: root.join(rom),
            model,
            check: Check::Screen(root.join(png)),
            frames: secs(10),
            full_time: false,
        });
    }

    // Mealybug Tearoom (PPU): DMG und CGB (Revision D als nächste zu CGB-E)
    let mut roms = Vec::new();
    roms_in(&root.join("mealybug-tearoom-tests/ppu"), &mut roms);
    for rom in roms {
        let stem = rom.with_extension("");
        for (model, suffix) in [(Model::Dmg, "_dmg_blob.png"), (Model::Cgb, "_cgb_d.png")] {
            let png = PathBuf::from(format!("{}{}", stem.display(), suffix));
            if png.exists() {
                cases.push(Case {
                    rom: rom.clone(),
                    model,
                    check: Check::Screen(png),
                    frames: secs(10),
                    full_time: false,
                });
            }
        }
    }

    // SameSuite (nur CGB-E)
    let mut roms = Vec::new();
    roms_in(&root.join("same-suite"), &mut roms);
    for rom in roms {
        cases.push(Case {
            rom,
            model: Model::Cgb,
            check: Check::Fibonacci,
            frames: secs(30),
            full_time: false,
        });
    }
    cases
}

fn compare(gb: &Gb, png_path: &Path) -> Result<(), String> {
    let data = std::fs::read(png_path).map_err(|e| format!("{}: {e}", png_path.display()))?;
    let img = png::decode(&data)?;
    let fb = gb.framebuffer();
    if (img.width, img.height) != (fb.width(), fb.height()) {
        return Err(format!(
            "Bildgröße {}×{} statt {}×{}",
            fb.width(),
            fb.height(),
            img.width,
            img.height
        ));
    }
    let mut diff = 0;
    let mut first = None;
    for y in 0..img.height {
        for x in 0..img.width {
            let [r, g, b, _] = fb.pixel(x, y);
            if [r, g, b] != img.rgb[(y * img.width + x) as usize] {
                diff += 1;
                first.get_or_insert((x, y));
            }
        }
    }
    match first {
        None => Ok(()),
        Some((x, y)) => Err(format!("{diff} Pixel abweichend, erstes bei ({x}, {y})")),
    }
}

fn run(case: &Case) -> Result<(), String> {
    let rom = std::fs::read(&case.rom).map_err(|e| e.to_string())?;
    let mut gb = Gb::from_rom_with(&rom, Some(case.model), None)?;
    for _ in 0..case.frames {
        let stop = gb.run_frame();
        if stop.is_error() {
            return Err(stop.to_string());
        }
        let done = gb.take_ld_b_b();
        match &case.check {
            Check::Serial => {
                if gb.tty_output().contains("Passed") {
                    return Ok(());
                }
                if gb.tty_output().contains("Failed") {
                    return Err(gb.tty_output().trim().replace('\n', " | "));
                }
            }
            Check::Fibonacci if done => {
                let c = &gb.cpu;
                let regs = [c.b, c.c, c.d, c.e, c.h, c.l];
                return if regs == [3, 5, 8, 13, 21, 34] {
                    Ok(())
                } else {
                    Err(format!("Register BCDEHL = {regs:02X?}"))
                };
            }
            Check::Screen(png) if done && !case.full_time => {
                // Das Bild nach dem Ende ist erst im nächsten fertigen Bild sichtbar.
                gb.run_frame();
                return compare(&gb, png);
            }
            _ => {}
        }
    }
    match &case.check {
        Check::Screen(png) if case.full_time => compare(&gb, png),
        _ => Err(format!("kein Ergebnis nach {} Bildern", case.frames)),
    }
}

#[test]
fn testsuiten() {
    let Some(root) = std::env::var_os("KUPO_GB_TESTROMS").map(PathBuf::from) else {
        eprintln!("KUPO_GB_TESTROMS nicht gesetzt – Test-ROMs übersprungen");
        return;
    };
    let cases = cases(&root);
    let failures = Mutex::new(Vec::new());
    let next = Mutex::new(0usize);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let i = {
                        let mut n = next.lock().unwrap();
                        let i = *n;
                        *n += 1;
                        i
                    };
                    let Some(case) = cases.get(i) else { break };
                    if let Err(why) = run(case) {
                        let rel = case
                            .rom
                            .strip_prefix(&root)
                            .unwrap_or(&case.rom)
                            .to_string_lossy()
                            .replace('\\', "/");
                        let key = format!("{}:{rel}", model_name(case.model));
                        failures.lock().unwrap().push((key, why));
                    }
                }
            });
        }
    });
    let mut failures = failures.into_inner().unwrap();
    failures.sort();
    let total = cases.len();
    println!(
        "Test-ROMs: {} von {total} bestanden",
        total - failures.len()
    );
    if std::env::var_os("KUPO_GB_OFFEN_AUSGEBEN").is_some() {
        println!("const GB_OFFEN: &[&str] = &[");
        for (k, why) in &failures {
            println!("    {k:?}, // {why}");
        }
        println!("];");
    }
    let new: Vec<_> = failures
        .iter()
        .filter(|(k, _)| !GB_OFFEN.contains(&k.as_str()))
        .collect();
    let fixed: Vec<_> = GB_OFFEN
        .iter()
        .filter(|k| !failures.iter().any(|(f, _)| f == *k))
        .collect();
    if !fixed.is_empty() {
        println!("Inzwischen bestanden (aus GB_OFFEN streichen): {fixed:?}");
    }
    assert!(
        new.is_empty(),
        "neue Fehler:\n{}",
        new.iter()
            .map(|(k, w)| format!("  {k}: {w}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// Rauchtest mit Spielen aus `KUPO_GB_GAMES`: 10 Sekunden ohne Fehler-Stopp.
#[test]
#[ignore]
fn spiele_laufen_an() {
    let Some(dir) = std::env::var_os("KUPO_GB_GAMES").map(PathBuf::from) else {
        eprintln!("KUPO_GB_GAMES nicht gesetzt – übersprungen");
        return;
    };
    let mut roms = Vec::new();
    roms_in(&dir, &mut roms);
    let mut failed = Vec::new();
    for rom in &roms {
        let data = std::fs::read(rom).unwrap();
        let result = Gb::from_rom(&data).and_then(|mut gb| {
            for _ in 0..600 {
                let stop = gb.run_frame();
                if stop.is_error() {
                    return Err(stop.to_string());
                }
            }
            Ok(())
        });
        if let Err(e) = result {
            failed.push(format!("{}: {e}", rom.display()));
        }
    }
    println!(
        "Spiele: {} von {} laufen an",
        roms.len() - failed.len(),
        roms.len()
    );
    assert!(
        failed.is_empty(),
        "{}",
        failed.join(
            "
"
        )
    );
}
