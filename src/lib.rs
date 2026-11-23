//! # kupocode-emulator-gbc
//!
//! Emulator und Debugger für den Game Boy, den Game Boy Color und den Super Game Boy in reinem
//! Rust.
//!
//! Die Maschine wird über die Schnittstellen aus `kupocode-emulator-core` (`Machine`,
//! `DebugTarget`, `SaveState`) ausgeführt; das Crate ist als [`core`] re-exportiert.

pub use kupocode_emulator_core as core;

pub mod cpu;
pub mod disasm;
pub mod hook;
