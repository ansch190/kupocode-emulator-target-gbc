//! Serielle Schnittstelle (`SB`, `SC`).
//!
//! Mit internem Takt schiebt sie ein Bit je fallender Flanke von Bit 8 des Systemzählers
//! (8192 Hz), im schnellen CGB-Modus von Bit 3. Ohne Gegenstelle kommen Einsen herein. Gesendete
//! Bytes landen zusätzlich in [`Serial::output`] (Testausgabe von Blargg).

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

#[derive(Debug, Clone, Default)]
pub struct Serial {
    pub sb: u8,
    pub sc: u8,
    /// Noch zu schiebende Bits der laufenden Übertragung.
    bits_left: u8,
    /// Byte zu Beginn der Übertragung.
    sending: u8,
    /// Alle vollständig gesendeten Bytes.
    pub output: Vec<u8>,
    /// CGB: schneller Takt (`SC` Bit 1) verfügbar.
    pub cgb: bool,
}

impl Serial {
    pub fn read(&self, addr: u16) -> u8 {
        if addr == 0xFF01 {
            self.sb
        } else {
            self.sc | if self.cgb { 0x7C } else { 0x7E }
        }
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        if addr == 0xFF01 {
            self.sb = v;
        } else {
            self.sc = v & if self.cgb { 0x83 } else { 0x81 };
            if v & 0x81 == 0x81 {
                self.bits_left = 8;
                self.sending = self.sb;
            }
        }
    }

    /// Ein M-Zyklus; `old`/`new` sind der Systemzähler davor und danach. Liefert `true` am Ende
    /// einer Übertragung (Unterbrechung).
    pub fn tick(&mut self, old: u16, new: u16) -> bool {
        if self.bits_left == 0 || self.sc & 0x01 == 0 {
            return false;
        }
        let bit = if self.cgb && self.sc & 0x02 != 0 {
            3
        } else {
            8
        };
        if old & (1 << bit) == 0 || new & (1 << bit) != 0 {
            return false;
        }
        self.sb = (self.sb << 1) | 1;
        self.bits_left -= 1;
        if self.bits_left == 0 {
            self.sc &= 0x7F;
            self.output.push(self.sending);
            return true;
        }
        false
    }

    pub fn save_state(&self, w: &mut StateWriter) {
        w.u8(self.sb);
        w.u8(self.sc);
        w.u8(self.bits_left);
        w.u8(self.sending);
        w.blob(&self.output);
    }

    pub fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        self.sb = r.u8()?;
        self.sc = r.u8()?;
        self.bits_left = r.u8()?;
        self.sending = r.u8()?;
        self.output = r.blob()?;
        Ok(())
    }
}
