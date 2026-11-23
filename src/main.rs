//! Kommandozeile des Game-Boy-Emulators.
//!
//! ```text
//! kupocode-emulator-gbc run    <rom.gb> [--frames N] [--steps N] [--state DATEI] [--shot BILD.ppm]
//! kupocode-emulator-gbc test   <rom.gb> [--frames N]
//! kupocode-emulator-gbc disasm <rom.gb> [--at ADRESSE] [--count N]
//! ```

use kupocode_emulator_gbc::core::{DebugTarget, Machine, StopReason};
use kupocode_emulator_gbc::{Gb, Model, TestResult};
use std::process::ExitCode;

const HILFE: &str = "Aufruf:
  kupocode-emulator-gbc run    <rom.gb> [--frames N] [--steps N] [--state DATEI] [--shot BILD.ppm]
  kupocode-emulator-gbc test   <rom.gb> [--frames N]
  kupocode-emulator-gbc disasm <rom.gb> [--at ADRESSE] [--count N]

run     führt N Bilder (Standard 60) oder N Befehle aus und zeigt Register und serielle Ausgabe
test    läuft, bis das Programm ein Ergebnis meldet (Blargg: serielle Ausgabe, Mooneye: LD B,B);
        Rückgabewert 0 = bestanden
disasm  disassembliert ab ADRESSE (Standard: PC nach dem Start)

--model dmg|cgb|sgb|sgb2  Modell erzwingen (sonst aus dem Kassettenkopf)
--boot DATEI              echtes Boot-ROM laden (sonst HLE-Start)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("Fehler: {e}");
            ExitCode::from(2)
        }
    }
}

/// Wert einer Option `--name WERT`.
fn option(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn number(text: &str) -> Result<u64, String> {
    let t = text.trim();
    let parsed = if let Some(hex) = t.strip_prefix('$').or_else(|| t.strip_prefix("0x")) {
        u64::from_str_radix(hex, 16)
    } else {
        t.parse()
    };
    parsed.map_err(|_| format!("ungültige Zahl `{text}`"))
}

fn load(args: &[String]) -> Result<Gb, String> {
    let path = args.get(1).ok_or("ROM-Datei fehlt")?;
    let bytes = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let model = match option(args, "--model").as_deref() {
        None => None,
        Some("dmg") => Some(Model::Dmg),
        Some("cgb") => Some(Model::Cgb),
        Some("sgb") => Some(Model::Sgb),
        Some("sgb2") => Some(Model::Sgb2),
        Some(m) => return Err(format!("unbekanntes Modell `{m}`")),
    };
    let boot = match option(args, "--boot") {
        Some(p) => Some(std::fs::read(&p).map_err(|e| format!("{p}: {e}"))?),
        None => None,
    };
    Gb::from_rom_with(&bytes, model, boot)
}

fn run(args: &[String]) -> Result<ExitCode, String> {
    let Some(cmd) = args.first() else {
        println!("{HILFE}");
        return Ok(ExitCode::SUCCESS);
    };
    match cmd.as_str() {
        "run" => {
            let mut gb = load(args)?;
            let stop = match option(args, "--steps") {
                Some(n) => gb.run(number(&n)?),
                None => {
                    let frames = option(args, "--frames").map_or(Ok(60), |f| number(&f))?;
                    let mut stop = StopReason::FrameDone;
                    for _ in 0..frames {
                        stop = gb.run_frame();
                        if stop != StopReason::FrameDone {
                            break;
                        }
                    }
                    stop
                }
            };
            let c = &gb.cpu;
            println!("Modell: {}", gb.model().name());
            println!("Stopp:  {stop}");
            println!(
                "AF={:04X} BC={:04X} DE={:04X} HL={:04X} SP={:04X} PC={:04X}  Takte={}",
                c.af(),
                c.bc(),
                c.de(),
                c.hl(),
                c.sp,
                c.pc,
                gb.cycles()
            );
            if !gb.tty_output().is_empty() {
                println!("{}", gb.tty_output());
            }
            if let Some(path) = option(args, "--shot") {
                std::fs::write(&path, gb.framebuffer().to_ppm())
                    .map_err(|e| format!("{path}: {e}"))?;
            }
            if let Some(path) = option(args, "--state") {
                std::fs::write(&path, gb.save_state_bytes()).map_err(|e| format!("{path}: {e}"))?;
            }
            Ok(if stop.is_error() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            })
        }
        "test" => {
            let mut gb = load(args)?;
            let frames = option(args, "--frames").map_or(Ok(3600), |f| number(&f))?;
            let result = gb.run_test(frames as u32);
            print!("{}", gb.tty_output());
            match result {
                Some(TestResult::Passed) => {
                    println!("\nbestanden");
                    Ok(ExitCode::SUCCESS)
                }
                Some(TestResult::Failed(why)) => {
                    println!("\nfehlgeschlagen: {why}");
                    Ok(ExitCode::FAILURE)
                }
                None => {
                    println!("\nkein Ergebnis nach {frames} Bildern");
                    Ok(ExitCode::FAILURE)
                }
            }
        }
        "disasm" => {
            let gb = load(args)?;
            let at = match option(args, "--at") {
                Some(a) => number(&a)? as u32,
                None => gb.pc(),
            };
            let count = option(args, "--count").map_or(Ok(32), |c| number(&c))? as usize;
            for line in gb.disassemble(at, count, None) {
                let bytes: Vec<String> = line.bytes.iter().map(|b| format!("{b:02X}")).collect();
                println!("{:04X}  {:<9} {}", line.addr, bytes.join(" "), line.text);
            }
            Ok(ExitCode::SUCCESS)
        }
        "-h" | "--help" | "hilfe" => {
            println!("{HILFE}");
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unbekannter Befehl `{other}`\n\n{HILFE}")),
    }
}
