//! APU: zwei Rechteckkanäle (Kanal 1 mit Sweep), Wellenkanal, Rauschkanal.
//!
//! Die Kanäle laufen im Grundtakt (auch bei doppeltem CPU-Takt). Der Frame-Sequencer schaltet
//! bei der fallenden Flanke von `DIV` Bit 4 (bei doppeltem Takt Bit 5) weiter, also mit 512 Hz:
//! Schritte 0/2/4/6 Längenzähler, 2/6 Sweep, 7 Hüllkurven.
//!
//! Ausgabe: Die digitalen Kanalwerte (0–15) gehen über die DACs (−1 … +1), werden nach `NR51`
//! verteilt, mit `NR50` gewichtet, über jedes Abtastintervall gemittelt und mit einem
//! Hochpass (Koppelkondensator) gefiltert. Abtastrate 44,1 kHz.

use kupocode_emulator_core::state::{StateError, StateReader, StateWriter};

/// Abtastrate der Ausgabe.
pub const SAMPLE_RATE: u32 = 44_100;
/// Grundtakt in Hz.
const CLOCK: u64 = 4_194_304;

const DUTY: [u8; 4] = [0b0000_0001, 0b1000_0001, 0b1000_0111, 0b0111_1110];
const NOISE_DIVISORS: [u32; 8] = [8, 16, 32, 48, 64, 80, 96, 112];

/// ODER-Masken beim Lesen von `FF10`–`FF2F`.
const READ_MASK: [u8; 0x20] = [
    0x80, 0x3F, 0x00, 0xFF, 0xBF, 0xFF, 0x3F, 0x00, 0xFF, 0xBF, 0x7F, 0xFF, 0x9F, 0xFF, 0xBF, 0xFF,
    0xFF, 0x00, 0x00, 0xBF, 0x00, 0x00, 0x70, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// Hüllkurve (Kanäle 1, 2, 4).
#[derive(Debug, Clone, Copy, Default)]
struct Envelope {
    initial: u8,
    up: bool,
    period: u8,
    volume: u8,
    timer: u8,
}

impl Envelope {
    fn write(&mut self, v: u8) {
        self.initial = v >> 4;
        self.up = v & 0x08 != 0;
        self.period = v & 7;
    }

    fn trigger(&mut self) {
        self.volume = self.initial;
        self.timer = if self.period == 0 { 8 } else { self.period };
    }

    fn clock(&mut self) {
        if self.period == 0 {
            return;
        }
        self.timer -= 1;
        if self.timer == 0 {
            self.timer = self.period;
            if self.up && self.volume < 15 {
                self.volume += 1;
            } else if !self.up && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }
}

/// Gemeinsamer Teil aller Kanäle.
#[derive(Debug, Clone, Copy, Default)]
struct Common {
    enabled: bool,
    dac: bool,
    length: u16,
    length_enable: bool,
    /// Kanalnummer 0–3 (Länge 64 bzw. 256 beim Wellenkanal).
    max_length: u16,
}

impl Common {
    fn clock_length(&mut self) {
        if self.length_enable && self.length > 0 {
            self.length -= 1;
            if self.length == 0 {
                self.enabled = false;
            }
        }
    }

    /// Schreiben auf `NRx4`: Längenfreigabe samt Zusatztakt und Auslösen.
    /// `extra` = der nächste Schritt des Frame-Sequencers taktet die Länge nicht.
    fn write_control(&mut self, v: u8, extra: bool) -> bool {
        let was = self.length_enable;
        self.length_enable = v & 0x40 != 0;
        if extra && !was && self.length_enable && self.length > 0 {
            self.length -= 1;
            if self.length == 0 && v & 0x80 == 0 {
                self.enabled = false;
            }
        }
        let trigger = v & 0x80 != 0;
        if trigger {
            if self.length == 0 {
                self.length = self.max_length;
                if extra && self.length_enable {
                    self.length -= 1;
                }
            }
            self.enabled = self.dac;
        }
        trigger
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Square {
    c: Common,
    env: Envelope,
    duty: u8,
    duty_pos: u8,
    freq: u16,
    timer: i32,
    // Sweep (nur Kanal 1)
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_timer: u8,
    sweep_enabled: bool,
    sweep_shadow: u16,
    sweep_negated: bool,
}

impl Square {
    fn period(&self) -> i32 {
        (2048 - i32::from(self.freq)) * 4
    }

    fn output(&self) -> u8 {
        if self.c.enabled && DUTY[usize::from(self.duty)] >> self.duty_pos & 1 != 0 {
            self.env.volume
        } else {
            0
        }
    }

    fn tick(&mut self, t: u32) {
        self.timer -= t as i32;
        while self.timer <= 0 {
            self.timer += self.period();
            self.duty_pos = (self.duty_pos + 1) & 7;
        }
    }

    fn sweep_calc(&mut self) -> u16 {
        let delta = self.sweep_shadow >> self.sweep_shift;
        let f = if self.sweep_negate {
            self.sweep_negated = true;
            self.sweep_shadow.wrapping_sub(delta)
        } else {
            self.sweep_shadow + delta
        };
        if f > 2047 {
            self.c.enabled = false;
        }
        f
    }

    fn clock_sweep(&mut self) {
        if self.sweep_timer > 0 {
            self.sweep_timer -= 1;
        }
        if self.sweep_timer == 0 {
            self.sweep_timer = if self.sweep_period == 0 {
                8
            } else {
                self.sweep_period
            };
            if self.sweep_enabled && self.sweep_period != 0 {
                let f = self.sweep_calc();
                if f <= 2047 && self.sweep_shift != 0 {
                    self.sweep_shadow = f;
                    self.freq = f;
                    self.sweep_calc();
                }
            }
        }
    }

    fn trigger(&mut self) {
        self.timer = self.period();
        self.env.trigger();
        self.sweep_shadow = self.freq;
        self.sweep_timer = if self.sweep_period == 0 {
            8
        } else {
            self.sweep_period
        };
        self.sweep_enabled = self.sweep_period != 0 || self.sweep_shift != 0;
        self.sweep_negated = false;
        if self.sweep_shift != 0 {
            self.sweep_calc();
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Wave {
    c: Common,
    volume_code: u8,
    freq: u16,
    timer: i32,
    pos: u8,
    sample: u8,
}

impl Wave {
    fn period(&self) -> i32 {
        (2048 - i32::from(self.freq)) * 2
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Noise {
    c: Common,
    env: Envelope,
    nr43: u8,
    lfsr: u16,
    timer: i32,
}

impl Noise {
    fn period(&self) -> i32 {
        (NOISE_DIVISORS[usize::from(self.nr43 & 7)] << (self.nr43 >> 4)) as i32
    }

    fn output(&self) -> u8 {
        if self.c.enabled && self.lfsr & 1 == 0 {
            self.env.volume
        } else {
            0
        }
    }

    fn tick(&mut self, t: u32) {
        self.timer -= t as i32;
        while self.timer <= 0 {
            self.timer += self.period();
            let x = (self.lfsr ^ (self.lfsr >> 1)) & 1;
            self.lfsr = (self.lfsr >> 1) | (x << 14);
            if self.nr43 & 0x08 != 0 {
                self.lfsr = (self.lfsr & !(1 << 6)) | (x << 6);
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Apu {
    /// Register `FF10`–`FF2F` (wie geschrieben, für das Lesen mit Masken).
    regs: [u8; 0x20],
    pub wave_ram: [u8; 16],
    on: bool,
    ch1: Square,
    ch2: Square,
    ch3: Wave,
    ch4: Noise,
    /// Nächster Schritt des Frame-Sequencers (0–7).
    fs_step: u8,
    /// CGB: beim Lesen des Wave-RAM während der Wiedergabe gilt das aktuelle Byte.
    pub cgb: bool,
    // Ausgabe
    acc: [f32; 2],
    acc_t: u32,
    phase: u64,
    cap: [f32; 2],
    samples: Vec<i16>,
}

impl Default for Apu {
    fn default() -> Self {
        let mut ch3 = Wave::default();
        ch3.c.max_length = 256;
        let mut ch1 = Square::default();
        ch1.c.max_length = 64;
        let ch2 = ch1;
        let mut ch4 = Noise::default();
        ch4.c.max_length = 64;
        Self {
            regs: [0; 0x20],
            wave_ram: [0; 16],
            on: false,
            ch1,
            ch2,
            ch3,
            ch4,
            fs_step: 0,
            cgb: false,
            acc: [0.0; 2],
            acc_t: 0,
            phase: 0,
            cap: [0.0; 2],
            samples: Vec::new(),
        }
    }
}

impl Apu {
    pub fn new(cgb: bool) -> Self {
        Self {
            cgb,
            ..Self::default()
        }
    }

    /// Fallende Flanke von `DIV` Bit 4/5: ein Schritt des Frame-Sequencers.
    pub fn frame_step(&mut self) {
        if !self.on {
            return;
        }
        let step = self.fs_step;
        if step.is_multiple_of(2) {
            self.ch1.c.clock_length();
            self.ch2.c.clock_length();
            self.ch3.c.clock_length();
            self.ch4.c.clock_length();
        }
        if step == 2 || step == 6 {
            self.ch1.clock_sweep();
        }
        if step == 7 {
            self.ch1.env.clock();
            self.ch2.env.clock();
            self.ch4.env.clock();
        }
        self.fs_step = (step + 1) & 7;
    }

    /// Lässt die Kanäle `t` Takte des Grundtakts laufen und erzeugt Abtastwerte.
    pub fn tick(&mut self, t: u32) {
        if self.on {
            self.ch1.tick(t);
            self.ch2.tick(t);
            self.tick_wave(t);
            self.ch4.tick(t);
        }
        let [l, r] = self.mix();
        self.acc[0] += l * t as f32;
        self.acc[1] += r * t as f32;
        self.acc_t += t;
        self.phase += u64::from(t) * u64::from(SAMPLE_RATE);
        if self.phase >= CLOCK {
            self.phase -= CLOCK;
            let n = self.acc_t.max(1) as f32;
            let mut out = [0i16; 2];
            for (i, o) in out.iter_mut().enumerate() {
                let v = self.acc[i] / n;
                // Hochpass wie der Koppelkondensator der Hardware
                let filtered = v - self.cap[i];
                self.cap[i] = v - filtered * 0.996;
                *o = (filtered * 8000.0).clamp(-32768.0, 32767.0) as i16;
            }
            self.samples.extend_from_slice(&out);
            self.acc = [0.0; 2];
            self.acc_t = 0;
        }
    }

    fn tick_wave(&mut self, t: u32) {
        let w = &mut self.ch3;
        if !w.c.enabled {
            return;
        }
        w.timer -= t as i32;
        while w.timer <= 0 {
            w.timer += w.period();
            w.pos = (w.pos + 1) & 31;
            let byte = self.wave_ram[usize::from(w.pos / 2)];
            w.sample = if w.pos & 1 == 0 {
                byte >> 4
            } else {
                byte & 0x0F
            };
        }
    }

    fn wave_output(&self) -> u8 {
        let w = &self.ch3;
        if !w.c.enabled || w.volume_code == 0 {
            return 0;
        }
        w.sample >> (w.volume_code - 1)
    }

    /// Digitale Ausgänge der vier Kanäle (0–15).
    pub fn outputs(&self) -> [u8; 4] {
        [
            self.ch1.output(),
            self.ch2.output(),
            self.wave_output(),
            self.ch4.output(),
        ]
    }

    fn mix(&self) -> [f32; 2] {
        if !self.on {
            return [0.0; 2];
        }
        let dacs = [
            self.ch1.c.dac,
            self.ch2.c.dac,
            self.ch3.c.dac,
            self.ch4.c.dac,
        ];
        let nr51 = self.regs[0x15];
        let nr50 = self.regs[0x14];
        let mut out = [0.0f32; 2];
        for (i, &d) in self.outputs().iter().enumerate() {
            if !dacs[i] {
                continue;
            }
            let analog = f32::from(d) / 7.5 - 1.0;
            if nr51 & (1 << i) != 0 {
                out[1] += analog;
            }
            if nr51 & (0x10 << i) != 0 {
                out[0] += analog;
            }
        }
        let vol_l = f32::from((nr50 >> 4) & 7) + 1.0;
        let vol_r = f32::from(nr50 & 7) + 1.0;
        [out[0] * vol_l / 32.0, out[1] * vol_r / 32.0]
    }

    /// Entnimmt die erzeugten Abtastwerte (Stereo, verschränkt).
    pub fn take_samples(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.samples)
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF30..=0xFF3F => {
                if self.ch3.c.enabled {
                    // Während der Wiedergabe liest man das gerade gespielte Byte.
                    if self.cgb {
                        self.wave_ram[usize::from(self.ch3.pos / 2)]
                    } else {
                        0xFF
                    }
                } else {
                    self.wave_ram[usize::from(addr - 0xFF30)]
                }
            }
            0xFF26 => {
                let status = u8::from(self.ch1.c.enabled)
                    | u8::from(self.ch2.c.enabled) << 1
                    | u8::from(self.ch3.c.enabled) << 2
                    | u8::from(self.ch4.c.enabled) << 3;
                0x70 | u8::from(self.on) << 7 | status
            }
            0xFF10..=0xFF2F => {
                let i = usize::from(addr - 0xFF10);
                self.regs[i] | READ_MASK[i]
            }
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, v: u8) {
        if (0xFF30..=0xFF3F).contains(&addr) {
            if !self.ch3.c.enabled {
                self.wave_ram[usize::from(addr - 0xFF30)] = v;
            } else if self.cgb {
                self.wave_ram[usize::from(self.ch3.pos / 2)] = v;
            }
            return;
        }
        if addr == 0xFF26 {
            let on = v & 0x80 != 0;
            if self.on && !on {
                self.power_off();
            } else if !self.on && on {
                self.fs_step = 0;
                self.ch1.duty_pos = 0;
                self.ch2.duty_pos = 0;
                self.ch3.sample = 0;
            }
            self.on = on;
            return;
        }
        // Ausgeschaltet sind nur die Längenregister schreibbar (DMG).
        if !self.on {
            if self.cgb {
                return;
            }
            match addr {
                0xFF11 => self.ch1.c.length = 64 - u16::from(v & 0x3F),
                0xFF16 => self.ch2.c.length = 64 - u16::from(v & 0x3F),
                0xFF1B => self.ch3.c.length = 256 - u16::from(v),
                0xFF20 => self.ch4.c.length = 64 - u16::from(v & 0x3F),
                _ => {}
            }
            return;
        }
        if let Some(r) = addr.checked_sub(0xFF10).filter(|&i| i < 0x20) {
            self.regs[usize::from(r)] = v;
        }
        // Der nächste Schritt taktet die Länge nicht, wenn er ungerade ist.
        let extra = self.fs_step & 1 == 1;
        match addr {
            0xFF10 => {
                let ch = &mut self.ch1;
                ch.sweep_period = (v >> 4) & 7;
                let negate = v & 0x08 != 0;
                if ch.sweep_negate && !negate && ch.sweep_negated {
                    ch.c.enabled = false;
                }
                ch.sweep_negate = negate;
                ch.sweep_shift = v & 7;
            }
            0xFF11 | 0xFF16 => {
                let ch = if addr == 0xFF11 {
                    &mut self.ch1
                } else {
                    &mut self.ch2
                };
                ch.duty = v >> 6;
                ch.c.length = 64 - u16::from(v & 0x3F);
            }
            0xFF12 | 0xFF17 => {
                let ch = if addr == 0xFF12 {
                    &mut self.ch1
                } else {
                    &mut self.ch2
                };
                ch.env.write(v);
                ch.c.dac = v & 0xF8 != 0;
                if !ch.c.dac {
                    ch.c.enabled = false;
                }
            }
            0xFF13 | 0xFF18 => {
                let ch = if addr == 0xFF13 {
                    &mut self.ch1
                } else {
                    &mut self.ch2
                };
                ch.freq = (ch.freq & 0x700) | u16::from(v);
            }
            0xFF14 | 0xFF19 => {
                let ch = if addr == 0xFF14 {
                    &mut self.ch1
                } else {
                    &mut self.ch2
                };
                ch.freq = (ch.freq & 0xFF) | (u16::from(v & 7) << 8);
                if ch.c.write_control(v, extra) {
                    ch.trigger();
                }
            }
            0xFF1A => {
                self.ch3.c.dac = v & 0x80 != 0;
                if !self.ch3.c.dac {
                    self.ch3.c.enabled = false;
                }
            }
            0xFF1B => self.ch3.c.length = 256 - u16::from(v),
            0xFF1C => self.ch3.volume_code = (v >> 5) & 3,
            0xFF1D => self.ch3.freq = (self.ch3.freq & 0x700) | u16::from(v),
            0xFF1E => {
                let w = &mut self.ch3;
                w.freq = (w.freq & 0xFF) | (u16::from(v & 7) << 8);
                if w.c.write_control(v, extra) {
                    // Erster Abtastwert erst nach einer Periode plus 6 Takten
                    w.timer = w.period() + 6;
                    w.pos = 0;
                }
            }
            0xFF20 => self.ch4.c.length = 64 - u16::from(v & 0x3F),
            0xFF21 => {
                self.ch4.env.write(v);
                self.ch4.c.dac = v & 0xF8 != 0;
                if !self.ch4.c.dac {
                    self.ch4.c.enabled = false;
                }
            }
            0xFF22 => self.ch4.nr43 = v,
            0xFF23 => {
                let n = &mut self.ch4;
                if n.c.write_control(v, extra) {
                    n.env.trigger();
                    n.lfsr = 0x7FFF;
                    n.timer = n.period();
                }
            }
            _ => {}
        }
    }

    fn power_off(&mut self) {
        let lengths = [
            self.ch1.c.length,
            self.ch2.c.length,
            self.ch3.c.length,
            self.ch4.c.length,
        ];
        let wave = self.wave_ram;
        let cgb = self.cgb;
        let samples = std::mem::take(&mut self.samples);
        let (acc, acc_t, phase, cap) = (self.acc, self.acc_t, self.phase, self.cap);
        *self = Apu::default();
        self.wave_ram = wave;
        self.cgb = cgb;
        self.samples = samples;
        (self.acc, self.acc_t, self.phase, self.cap) = (acc, acc_t, phase, cap);
        if !cgb {
            // Der DMG behält die Längenzähler.
            self.ch1.c.length = lengths[0];
            self.ch2.c.length = lengths[1];
            self.ch3.c.length = lengths[2];
            self.ch4.c.length = lengths[3];
        }
    }

    pub fn save_state(&self, w: &mut StateWriter) {
        w.bytes(&self.regs);
        w.bytes(&self.wave_ram);
        w.bool(self.on);
        w.u8(self.fs_step);
        for ch in [&self.ch1, &self.ch2] {
            save_common(w, &ch.c);
            save_env(w, &ch.env);
            for v in [
                ch.duty,
                ch.duty_pos,
                ch.sweep_period,
                ch.sweep_shift,
                ch.sweep_timer,
            ] {
                w.u8(v);
            }
            w.u16(ch.freq);
            w.i32(ch.timer);
            for v in [ch.sweep_negate, ch.sweep_enabled, ch.sweep_negated] {
                w.bool(v);
            }
            w.u16(ch.sweep_shadow);
        }
        let c3 = &self.ch3;
        save_common(w, &c3.c);
        w.u8(c3.volume_code);
        w.u16(c3.freq);
        w.i32(c3.timer);
        w.u8(c3.pos);
        w.u8(c3.sample);
        let c4 = &self.ch4;
        save_common(w, &c4.c);
        save_env(w, &c4.env);
        w.u8(c4.nr43);
        w.u16(c4.lfsr);
        w.i32(c4.timer);
        w.u64(self.phase);
    }

    pub fn load_state(&mut self, r: &mut StateReader<'_>) -> Result<(), StateError> {
        r.bytes_into(&mut self.regs)?;
        r.bytes_into(&mut self.wave_ram)?;
        self.on = r.bool()?;
        self.fs_step = r.u8()?;
        for ch in [&mut self.ch1, &mut self.ch2] {
            load_common(r, &mut ch.c)?;
            load_env(r, &mut ch.env)?;
            for v in [
                &mut ch.duty,
                &mut ch.duty_pos,
                &mut ch.sweep_period,
                &mut ch.sweep_shift,
                &mut ch.sweep_timer,
            ] {
                *v = r.u8()?;
            }
            ch.freq = r.u16()?;
            ch.timer = r.i32()?;
            for v in [
                &mut ch.sweep_negate,
                &mut ch.sweep_enabled,
                &mut ch.sweep_negated,
            ] {
                *v = r.bool()?;
            }
            ch.sweep_shadow = r.u16()?;
        }
        let c3 = &mut self.ch3;
        load_common(r, &mut c3.c)?;
        c3.volume_code = r.u8()?;
        c3.freq = r.u16()?;
        c3.timer = r.i32()?;
        c3.pos = r.u8()?;
        c3.sample = r.u8()?;
        let c4 = &mut self.ch4;
        load_common(r, &mut c4.c)?;
        load_env(r, &mut c4.env)?;
        c4.nr43 = r.u8()?;
        c4.lfsr = r.u16()?;
        c4.timer = r.i32()?;
        self.phase = r.u64()?;
        self.acc = [0.0; 2];
        self.acc_t = 0;
        self.samples.clear();
        Ok(())
    }
}

fn save_common(w: &mut StateWriter, c: &Common) {
    w.bool(c.enabled);
    w.bool(c.dac);
    w.u16(c.length);
    w.bool(c.length_enable);
}

fn load_common(r: &mut StateReader<'_>, c: &mut Common) -> Result<(), StateError> {
    c.enabled = r.bool()?;
    c.dac = r.bool()?;
    c.length = r.u16()?;
    c.length_enable = r.bool()?;
    Ok(())
}

fn save_env(w: &mut StateWriter, e: &Envelope) {
    for v in [e.initial, e.period, e.volume, e.timer] {
        w.u8(v);
    }
    w.bool(e.up);
}

fn load_env(r: &mut StateReader<'_>, e: &mut Envelope) -> Result<(), StateError> {
    e.initial = r.u8()?;
    e.period = r.u8()?;
    e.volume = r.u8()?;
    e.timer = r.u8()?;
    e.up = r.bool()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apu() -> Apu {
        let mut a = Apu::default();
        a.write(0xFF26, 0x80);
        a
    }

    #[test]
    fn ausloesen_und_laengenzaehler() {
        let mut a = apu();
        a.write(0xFF12, 0xF0); // DAC an
        a.write(0xFF11, 0x3E); // Länge 2
        a.write(0xFF14, 0xC0); // auslösen, Länge an
        assert_eq!(a.read(0xFF26) & 1, 1);
        a.frame_step(); // Schritt 0: Länge 1
        a.frame_step(); // Schritt 1
        a.frame_step(); // Schritt 2: Länge 0 → aus
        assert_eq!(a.read(0xFF26) & 1, 0);
    }

    #[test]
    fn dac_aus_schaltet_kanal_ab() {
        let mut a = apu();
        a.write(0xFF17, 0xF0);
        a.write(0xFF19, 0x80);
        assert_eq!(a.read(0xFF26) & 2, 2);
        a.write(0xFF17, 0x00);
        assert_eq!(a.read(0xFF26) & 2, 0);
    }

    #[test]
    fn ausschalten_loescht_register() {
        let mut a = apu();
        a.write(0xFF24, 0x77);
        a.write(0xFF26, 0x00);
        assert_eq!(a.read(0xFF24), 0x00);
        a.write(0xFF24, 0x77); // ignoriert
        assert_eq!(a.read(0xFF24), 0x00);
        assert_eq!(a.read(0xFF26), 0x70);
    }

    #[test]
    fn lesemasken() {
        let a = apu();
        assert_eq!(a.read(0xFF10), 0x80);
        assert_eq!(a.read(0xFF13), 0xFF);
        assert_eq!(a.read(0xFF1A), 0x7F);
    }

    #[test]
    fn sweep_ueberlauf_schaltet_ab() {
        let mut a = apu();
        a.write(0xFF12, 0xF0);
        a.write(0xFF10, 0x11); // Periode 1, Erhöhung, Shift 1
        a.write(0xFF13, 0xFF);
        a.write(0xFF14, 0x87); // Frequenz 2047 → Überlauf schon beim Auslösen
        assert_eq!(a.read(0xFF26) & 1, 0);
    }

    #[test]
    fn rechteck_erzeugt_ton() {
        let mut a = apu();
        a.write(0xFF24, 0x77);
        a.write(0xFF25, 0xFF);
        a.write(0xFF12, 0xF0);
        a.write(0xFF11, 0x80);
        a.write(0xFF13, 0x00);
        a.write(0xFF14, 0x87); // ≈ 1 kHz? (2048-1792)*4 Takte je Schritt
        for _ in 0..100_000 {
            a.tick(4);
        }
        let s = a.take_samples();
        assert!(s.len() > 2000);
        let max = s.iter().map(|&v| i32::from(v).abs()).max().unwrap();
        assert!(max > 1000, "{max}");
    }

    #[test]
    fn rauschen_lfsr() {
        let mut n = Noise::default();
        n.c.enabled = true;
        n.lfsr = 0x7FFF;
        n.nr43 = 0x00;
        n.timer = 1;
        n.tick(8);
        assert_eq!(n.lfsr, 0x3FFF);
    }
}
