//! Timer: Systemzähler (`DIV`), `TIMA`, `TMA`, `TAC`.
//!
//! `TIMA` zählt bei der **fallenden Flanke** des Signals „gewähltes Zählerbit UND Timer an“.
//! Daher erhöhen auch Schreibzugriffe auf `DIV` oder `TAC` den Zähler, wenn sie das Signal von
//! 1 auf 0 bringen. Nach einem Überlauf liest sich `TIMA` einen M-Zyklus lang als 0; erst dann
//! wird `TMA` geladen und die Unterbrechung angefordert. Ein Schreiben auf `TIMA` in diesem
//! Zyklus bricht das Laden ab, ein Schreiben im Ladezyklus selbst wird verworfen.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

/// Zählerbit je Taktwahl `TAC & 3` (4096, 262144, 65536, 16384 Hz).
const BITS: [u8; 4] = [9, 3, 5, 7];

#[derive(Debug, Clone, Default)]
pub struct Timer {
    /// Systemzähler in CPU-Takten; `DIV` ist das obere Byte.
    pub counter: u16,
    pub tima: u8,
    pub tma: u8,
    pub tac: u8,
    /// Überlauf erfolgt, `TMA` wird im nächsten M-Zyklus geladen.
    reload_pending: bool,
    /// In diesem M-Zyklus wurde `TMA` geladen.
    reloading: bool,
}

impl Timer {
    fn signal(&self) -> bool {
        self.tac & 4 != 0 && self.counter & (1 << BITS[usize::from(self.tac & 3)]) != 0
    }

    fn increment(&mut self) {
        let (v, overflow) = self.tima.overflowing_add(1);
        self.tima = v;
        if overflow {
            self.reload_pending = true;
        }
    }

    /// Ein M-Zyklus (4 CPU-Takte). Liefert `true`, wenn die Timer-Unterbrechung anzufordern ist.
    pub fn tick(&mut self) -> bool {
        let mut irq = false;
        self.reloading = false;
        if self.reload_pending {
            self.reload_pending = false;
            self.reloading = true;
            self.tima = self.tma;
            irq = true;
        }
        let old = self.signal();
        self.counter = self.counter.wrapping_add(4);
        if old && !self.signal() {
            self.increment();
        }
        irq
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF04 => (self.counter >> 8) as u8,
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            _ => self.tac | 0xF8,
        }
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        match addr {
            0xFF04 => {
                let old = self.signal();
                self.counter = 0;
                if old {
                    self.increment();
                }
            }
            0xFF05 => {
                if self.reload_pending {
                    self.reload_pending = false;
                    self.tima = v;
                } else if !self.reloading {
                    self.tima = v;
                }
            }
            0xFF06 => {
                self.tma = v;
                if self.reloading {
                    self.tima = v;
                }
            }
            _ => {
                let old = self.signal();
                self.tac = v & 7;
                if old && !self.signal() {
                    self.increment();
                }
            }
        }
    }

    pub fn save_state(&self, w: &mut StateWriter) {
        w.u16(self.counter);
        w.u8(self.tima);
        w.u8(self.tma);
        w.u8(self.tac);
        w.bool(self.reload_pending);
        w.bool(self.reloading);
    }

    pub fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.counter = r.u16()?;
        self.tima = r.u8()?;
        self.tma = r.u8()?;
        self.tac = r.u8()?;
        self.reload_pending = r.bool()?;
        self.reloading = r.bool()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ueberlauf_laedt_tma_einen_zyklus_spaeter() {
        let mut t = Timer {
            tac: 0x05, // an, Bit 3 (16 Takte je Zählschritt)
            tima: 0xFF,
            tma: 0x42,
            ..Timer::default()
        };
        // Nach 16 Takten (4 M-Zyklen) läuft TIMA über.
        for _ in 0..4 {
            assert!(!t.tick());
        }
        assert_eq!(t.tima, 0);
        assert!(t.tick());
        assert_eq!(t.tima, 0x42);
    }

    #[test]
    fn schreiben_auf_div_erzeugt_flanke() {
        let mut t = Timer {
            tac: 0x05,
            counter: 0x0008,
            ..Timer::default()
        };
        t.write(0xFF04, 0);
        assert_eq!((t.tima, t.counter), (1, 0));
    }

    #[test]
    fn schreiben_auf_tima_bricht_laden_ab() {
        let mut t = Timer {
            tac: 0x05,
            tima: 0xFF,
            tma: 0x42,
            counter: 0x000C,
            ..Timer::default()
        };
        t.tick();
        t.write(0xFF05, 0x10);
        assert!(!t.tick());
        assert_eq!(t.tima, 0x10);
    }
}
