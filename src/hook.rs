//! Beobachtung der Befehlsausführung von außen (z. B. Laufzeitprüfungen des Compilers).
//!
//! Ein [`CpuHook`] wird vor jedem Befehl aufgerufen. Ohne Hook ([`NoHook`]) kostet er nichts.

/// Zustand der CPU unmittelbar vor einem Befehl.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstrCtx {
    pub pc: u16,
    pub opcode: u8,
    pub af: u16,
    pub bc: u16,
    pub de: u16,
    pub hl: u16,
    pub sp: u16,
    /// T-Zyklen seit dem Einschalten.
    pub cycle: u64,
}

/// Entscheidung eines Hooks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookAction {
    /// Befehl normal ausführen.
    Continue,
    /// Mit einem Fehler-Stopp und dieser Meldung anhalten (der Befehl wird nicht ausgeführt).
    Stop(String),
}

/// Beobachter der Befehlsausführung.
pub trait CpuHook {
    fn before_execute(&mut self, ctx: &InstrCtx) -> HookAction;
}

/// Hook ohne Wirkung.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHook;

impl CpuHook for NoHook {
    #[inline(always)]
    fn before_execute(&mut self, _ctx: &InstrCtx) -> HookAction {
        HookAction::Continue
    }
}
