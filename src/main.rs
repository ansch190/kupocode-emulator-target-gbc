//! Kommandozeile des Game-Boy-Emulators.

use std::process::ExitCode;

const HILFE: &str = "kupocode-emulator-gbc – Emulator für Game Boy und Game Boy Color

Aufruf:
  kupocode-emulator-gbc --help   Zeigt diese Hilfe

Ausführen, Testen und Disassemblieren folgen mit der Emulation.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Fehler: {e}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None | Some("--help") => {
            println!("{HILFE}");
            Ok(())
        }
        Some(other) => Err(format!("unbekannter Befehl `{other}`\n\n{HILFE}")),
    }
}
