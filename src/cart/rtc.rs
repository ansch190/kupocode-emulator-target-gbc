//! Echtzeituhr des MBC3 (auch Grundlage für HuC3).
//!
//! Die Uhr zählt **emulierte** Takte (deterministisch). Die Zeit, in der das Spiel nicht lief,
//! stellt die GUI über [`Rtc::advance`] nach. Register außerhalb ihres Bereichs zählen wie auf
//! der Hardware bis zum Bitüberlauf weiter, ohne in die nächste Einheit zu übertragen.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

/// Takte des Grundtakts je Sekunde.
pub const CYCLES_PER_SECOND: u32 = 4_194_304;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RtcRegs {
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub days_low: u8,
    /// Bit 0: Tag Bit 8, Bit 6: angehalten, Bit 7: Tagesüberlauf.
    pub days_high: u8,
}

impl RtcRegs {
    fn get(&self, i: u8) -> u8 {
        match i {
            0 => self.seconds,
            1 => self.minutes,
            2 => self.hours,
            3 => self.days_low,
            _ => self.days_high,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Rtc {
    pub regs: RtcRegs,
    pub latched: RtcRegs,
    /// Takte seit der letzten vollen Sekunde.
    sub: u32,
    /// Zuletzt auf das Latch-Register geschriebener Wert.
    latch_last: u8,
}

impl Rtc {
    pub fn halted(&self) -> bool {
        self.regs.days_high & 0x40 != 0
    }

    fn second(&mut self) {
        let r = &mut self.regs;
        r.seconds = (r.seconds + 1) & 0x3F;
        if r.seconds != 60 {
            return;
        }
        r.seconds = 0;
        r.minutes = (r.minutes + 1) & 0x3F;
        if r.minutes != 60 {
            return;
        }
        r.minutes = 0;
        r.hours = (r.hours + 1) & 0x1F;
        if r.hours != 24 {
            return;
        }
        r.hours = 0;
        let days = (u16::from(r.days_high & 1) << 8 | u16::from(r.days_low)) + 1;
        r.days_low = days as u8;
        r.days_high = (r.days_high & 0xC0) | ((days >> 8) & 1) as u8;
        if days > 0x1FF {
            r.days_high |= 0x80;
        }
    }

    pub fn tick(&mut self, cycles: u32) {
        if self.halted() {
            return;
        }
        self.sub += cycles;
        while self.sub >= CYCLES_PER_SECOND {
            self.sub -= CYCLES_PER_SECOND;
            self.second();
        }
    }

    /// Stellt die Uhr um `seconds` vor (nur wenn sie läuft).
    pub fn advance(&mut self, seconds: u64) {
        if self.halted() {
            return;
        }
        // Große Sprünge zuerst in ganzen Tagen (Register im gültigen Bereich vorausgesetzt).
        let mut rest = seconds;
        let r = self.regs;
        if r.seconds < 60 && r.minutes < 60 && r.hours < 24 {
            let in_day =
                u64::from(r.hours) * 3600 + u64::from(r.minutes) * 60 + u64::from(r.seconds);
            let total = in_day + rest;
            let days = total / 86_400;
            let left = total % 86_400;
            let old_days = u64::from(r.days_high & 1) << 8 | u64::from(r.days_low);
            let new_days = old_days + days;
            let mut high = (r.days_high & 0xC0) | ((new_days >> 8) & 1) as u8;
            if new_days > 0x1FF {
                high |= 0x80;
            }
            self.regs = RtcRegs {
                seconds: (left % 60) as u8,
                minutes: (left / 60 % 60) as u8,
                hours: (left / 3600) as u8,
                days_low: new_days as u8,
                days_high: high,
            };
            rest = 0;
        }
        for _ in 0..rest {
            self.second();
        }
    }

    pub fn read(&self, reg: u8) -> u8 {
        let v = self.latched.get(reg);
        match reg {
            0 | 1 => v & 0x3F,
            2 => v & 0x1F,
            3 => v,
            _ => v & 0xC1 | 0x3E,
        }
    }

    pub fn write(&mut self, reg: u8, v: u8) {
        let r = &mut self.regs;
        match reg {
            0 => {
                r.seconds = v & 0x3F;
                self.sub = 0;
            }
            1 => r.minutes = v & 0x3F,
            2 => r.hours = v & 0x1F,
            3 => r.days_low = v,
            _ => r.days_high = v & 0xC1,
        }
        self.latched = self.regs;
    }

    /// Schreiben auf `6000`–`7FFF`: Übergang 0 → 1 übernimmt die Zeit ins Latch.
    pub fn latch(&mut self, v: u8) {
        if self.latch_last == 0 && v == 1 {
            self.latched = self.regs;
        }
        self.latch_last = v;
    }

    /// 48-Byte-Anhang (BGB/VBA): fünf aktuelle und fünf gelatchte Register als u32 LE, dann der
    /// Unix-Zeitstempel als u64 LE.
    pub fn footer(&self, now: u64) -> Vec<u8> {
        let mut out = Vec::with_capacity(48);
        for regs in [self.regs, self.latched] {
            for i in 0..5 {
                out.extend_from_slice(&u32::from(regs.get(i)).to_le_bytes());
            }
        }
        out.extend_from_slice(&now.to_le_bytes());
        out
    }

    /// Liest den Anhang; liefert den Zeitstempel. Akzeptiert auch den 44-Byte-Anhang mit u32-Zeit.
    pub fn load_footer(&mut self, data: &[u8]) -> Option<u64> {
        if data.len() < 44 {
            return None;
        }
        let word = |i: usize| data[i * 4];
        let regs = |o: usize| RtcRegs {
            seconds: word(o),
            minutes: word(o + 1),
            hours: word(o + 2),
            days_low: word(o + 3),
            days_high: word(o + 4) & 0xC1,
        };
        self.regs = regs(0);
        self.latched = regs(5);
        Some(if data.len() >= 48 {
            u64::from_le_bytes(data[40..48].try_into().ok()?)
        } else {
            u64::from(u32::from_le_bytes(data[40..44].try_into().ok()?))
        })
    }

    pub fn save_state(&self, w: &mut StateWriter) {
        for regs in [self.regs, self.latched] {
            for i in 0..5 {
                w.u8(regs.get(i));
            }
        }
        w.u32(self.sub);
        w.u8(self.latch_last);
    }

    pub fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        for regs in [&mut self.regs, &mut self.latched] {
            regs.seconds = r.u8()?;
            regs.minutes = r.u8()?;
            regs.hours = r.u8()?;
            regs.days_low = r.u8()?;
            regs.days_high = r.u8()?;
        }
        self.sub = r.u32()?;
        self.latch_last = r.u8()?;
        Ok(())
    }
}
