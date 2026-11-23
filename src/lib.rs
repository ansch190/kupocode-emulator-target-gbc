//! # kupocode-emulator-gbc
//!
//! Emulator und Debugger für den Game Boy, den Game Boy Color und den Super Game Boy in reinem
//! Rust.
//!
//! Die Maschine wird über die Schnittstellen aus `kupocode-emulator-core` (`Machine`,
//! `DebugTarget`, `SaveState`) ausgeführt; das Crate ist als [`core`] re-exportiert.

pub use kupocode_emulator_core as core;

pub mod apu;
pub mod bus;
pub mod cart;
pub mod cpu;
pub mod disasm;
pub mod gb;
pub mod hook;
pub mod joypad;
pub mod model;
pub mod ppu;
pub mod serial;
pub mod timer;
pub mod views;

pub use gb::{Gb, TestResult};
pub use model::Model;
