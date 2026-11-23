//! CPU des Game Boy: Sharp SM83 (auch „LR35902“ oder „GBZ80“ genannt).
//!
//! Die CPU ist **zyklusgenau über den Bus**: Jeder M-Zyklus ist genau ein Aufruf von
//! [`CpuBus::read`], [`CpuBus::write`] oder [`CpuBus::tick`] (interner Zyklus ohne Zugriff).
//! Der Bus taktet bei jedem Aufruf die Peripherie um einen M-Zyklus; Zyklenzahlen ergeben sich
//! damit von selbst und werden nie aus Tabellen addiert.
//!
//! Unterbrechungen werden vor jedem Befehl abgefragt. `EI` wirkt erst nach dem folgenden Befehl,
//! `RETI` sofort. Der Ablauf einer Unterbrechung dauert 5 M-Zyklen; wird `IE` beim Sichern des
//! oberen PC-Bytes überschrieben, entscheidet der neue Wert (sonst Sprung nach `0x0000`).

pub mod opcodes;

#[cfg(test)]
mod tests;

/// Null.
pub const FLAG_Z: u8 = 0x80;
/// Subtraktion.
pub const FLAG_N: u8 = 0x40;
/// Halbübertrag.
pub const FLAG_H: u8 = 0x20;
/// Übertrag.
pub const FLAG_C: u8 = 0x10;

/// Speicherbus aus Sicht der CPU. Jeder Aufruf ist ein M-Zyklus (4 T-Zyklen im Normaltakt).
pub trait CpuBus {
    /// Liest ein Byte (ein M-Zyklus).
    fn read(&mut self, addr: u16) -> u8;
    /// Schreibt ein Byte (ein M-Zyklus).
    fn write(&mut self, addr: u16, value: u8);
    /// Interner M-Zyklus ohne Buszugriff.
    fn tick(&mut self);
    /// Anstehende Unterbrechungen: `IE & IF & 0x1F`.
    fn pending(&self) -> u8;
    /// Löscht das Anforderungsbit `bit` (0–4) in `IF`.
    fn acknowledge(&mut self, bit: u8);
    /// `STOP` wurde ausgeführt (Taktumschaltung des CGB oder Stromsparmodus).
    fn stop(&mut self) {}
    /// Die Inkrement-/Dekrement-Einheit hat `addr` auf den Adressbus gelegt (`INC rr`/`DEC rr`,
    /// Grundlage des OAM-Fehlers des DMG).
    fn idu(&mut self, _addr: u16) {}
}

/// Was ein Schritt ausgeführt hat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Ein Befehl (Opcode, Adresse).
    Instruction { opcode: u8, pc: u16 },
    /// Eine Unterbrechung wurde angenommen (Vektor, Rücksprungadresse).
    Interrupt { vector: u16, return_pc: u16 },
    /// Die CPU wartet in `HALT` (ein M-Zyklus verstrichen).
    Halted,
    /// Die CPU hängt nach einem gesperrten Opcode.
    Locked { opcode: u8, pc: u16 },
}

/// Registersatz und interner Zustand des SM83.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cpu {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
    /// Interrupt Master Enable.
    pub ime: bool,
    /// Verzögerung von `EI` in Schritten (2 = gerade ausgeführt).
    pub ei_delay: u8,
    /// Wartet in `HALT`.
    pub halted: bool,
    /// HALT-Fehler: Das nächste Opcode-Byte wird gelesen, ohne PC zu erhöhen.
    pub halt_bug: bool,
    /// Nach einem gesperrten Opcode bleibt die CPU stehen.
    pub locked: bool,
}

impl Cpu {
    /// CPU im Einschaltzustand (alle Register 0); den Zustand nach dem Boot setzt die Maschine.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn af(&self) -> u16 {
        u16::from_be_bytes([self.a, self.f & 0xF0])
    }
    pub fn bc(&self) -> u16 {
        u16::from_be_bytes([self.b, self.c])
    }
    pub fn de(&self) -> u16 {
        u16::from_be_bytes([self.d, self.e])
    }
    pub fn hl(&self) -> u16 {
        u16::from_be_bytes([self.h, self.l])
    }
    pub fn set_af(&mut self, v: u16) {
        [self.a, self.f] = v.to_be_bytes();
        self.f &= 0xF0;
    }
    pub fn set_bc(&mut self, v: u16) {
        [self.b, self.c] = v.to_be_bytes();
    }
    pub fn set_de(&mut self, v: u16) {
        [self.d, self.e] = v.to_be_bytes();
    }
    pub fn set_hl(&mut self, v: u16) {
        [self.h, self.l] = v.to_be_bytes();
    }

    fn flag(&self, f: u8) -> bool {
        self.f & f != 0
    }

    fn set_flags(&mut self, z: bool, n: bool, h: bool, c: bool) {
        self.f = (u8::from(z) << 7) | (u8::from(n) << 6) | (u8::from(h) << 5) | (u8::from(c) << 4);
    }

    /// Führt einen Befehl aus, nimmt eine Unterbrechung an oder wartet einen M-Zyklus in `HALT`.
    pub fn step<B: CpuBus>(&mut self, bus: &mut B) -> Step {
        if self.locked {
            bus.tick();
            return Step::Locked {
                opcode: 0,
                pc: self.pc,
            };
        }
        if self.ei_delay > 0 {
            self.ei_delay -= 1;
            if self.ei_delay == 0 {
                self.ime = true;
            }
        }
        if self.halted {
            bus.tick();
            if bus.pending() != 0 {
                self.halted = false;
            }
            return Step::Halted;
        }
        if self.ime && bus.pending() != 0 {
            return self.dispatch(bus);
        }

        let pc = self.pc;
        let opcode = bus.read(pc);
        if self.halt_bug {
            self.halt_bug = false;
        } else {
            self.pc = pc.wrapping_add(1);
        }
        self.execute(bus, opcode, pc)
    }

    /// Ablauf einer Unterbrechung: 2 Wartezyklen, PC sichern, Vektor laden (5 M-Zyklen).
    fn dispatch<B: CpuBus>(&mut self, bus: &mut B) -> Step {
        // HALT-Fehler mit `EI` davor: Die Unterbrechung kehrt zum `HALT` selbst zurück.
        let return_pc = if self.halt_bug {
            self.halt_bug = false;
            self.pc.wrapping_sub(1)
        } else {
            self.pc
        };
        self.ime = false;
        bus.tick();
        bus.tick();
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, (return_pc >> 8) as u8);
        // IE kann durch das Sichern des oberen Bytes (SP = 0x0000 → 0xFFFF) überschrieben sein.
        let pending = bus.pending();
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, return_pc as u8);
        let vector = if pending == 0 {
            0x0000
        } else {
            let bit = pending.trailing_zeros() as u8;
            bus.acknowledge(bit);
            0x0040 + u16::from(bit) * 8
        };
        self.pc = vector;
        bus.tick();
        Step::Interrupt { vector, return_pc }
    }

    fn fetch<B: CpuBus>(&mut self, bus: &mut B) -> u8 {
        let v = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn fetch16<B: CpuBus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        u16::from_le_bytes([lo, hi])
    }

    fn push<B: CpuBus>(&mut self, bus: &mut B, v: u16) {
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, (v >> 8) as u8);
        self.sp = self.sp.wrapping_sub(1);
        bus.write(self.sp, v as u8);
    }

    fn pop<B: CpuBus>(&mut self, bus: &mut B) -> u16 {
        let lo = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        let hi = bus.read(self.sp);
        self.sp = self.sp.wrapping_add(1);
        u16::from_le_bytes([lo, hi])
    }

    /// 8-Bit-Register nach Index (0–7 = B C D E H L (HL) A); `(HL)` liest über den Bus.
    fn get_r<B: CpuBus>(&mut self, bus: &mut B, r: u8) -> u8 {
        match r {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            6 => bus.read(self.hl()),
            _ => self.a,
        }
    }

    fn set_r<B: CpuBus>(&mut self, bus: &mut B, r: u8, v: u8) {
        match r {
            0 => self.b = v,
            1 => self.c = v,
            2 => self.d = v,
            3 => self.e = v,
            4 => self.h = v,
            5 => self.l = v,
            6 => bus.write(self.hl(), v),
            _ => self.a = v,
        }
    }

    /// 16-Bit-Registerpaar nach Index (0–3 = BC DE HL SP).
    fn get_rr(&self, p: u8) -> u16 {
        match p {
            0 => self.bc(),
            1 => self.de(),
            2 => self.hl(),
            _ => self.sp,
        }
    }

    fn set_rr(&mut self, p: u8, v: u16) {
        match p {
            0 => self.set_bc(v),
            1 => self.set_de(v),
            2 => self.set_hl(v),
            _ => self.sp = v,
        }
    }

    fn cond(&self, cc: u8) -> bool {
        match cc {
            0 => !self.flag(FLAG_Z),
            1 => self.flag(FLAG_Z),
            2 => !self.flag(FLAG_C),
            _ => self.flag(FLAG_C),
        }
    }

    /// Rechenwerk für `ADD ADC SUB SBC AND XOR OR CP` (Index 0–7).
    fn alu(&mut self, op: u8, v: u8) {
        let a = self.a;
        let carry = u8::from(self.flag(FLAG_C));
        match op {
            0 | 1 => {
                let c = if op == 1 { carry } else { 0 };
                let r = u16::from(a) + u16::from(v) + u16::from(c);
                let h = (a & 0xF) + (v & 0xF) + c > 0xF;
                self.a = r as u8;
                self.set_flags(self.a == 0, false, h, r > 0xFF);
            }
            2 | 3 | 7 => {
                let c = if op == 3 { carry } else { 0 };
                let r = i16::from(a) - i16::from(v) - i16::from(c);
                let h = i16::from(a & 0xF) - i16::from(v & 0xF) - i16::from(c) < 0;
                let res = r as u8;
                self.set_flags(res == 0, true, h, r < 0);
                if op != 7 {
                    self.a = res;
                }
            }
            4 => {
                self.a &= v;
                self.set_flags(self.a == 0, false, true, false);
            }
            5 => {
                self.a ^= v;
                self.set_flags(self.a == 0, false, false, false);
            }
            _ => {
                self.a |= v;
                self.set_flags(self.a == 0, false, false, false);
            }
        }
    }

    /// `SP + e` mit den Flags von `ADD SP,e` und `LD HL,SP+e` (aus dem unteren Byte).
    fn sp_plus(&mut self, e: u8) -> u16 {
        let sp = self.sp;
        let h = (sp & 0xF) + (u16::from(e) & 0xF) > 0xF;
        let c = (sp & 0xFF) + u16::from(e) > 0xFF;
        self.set_flags(false, false, h, c);
        sp.wrapping_add(e as i8 as u16)
    }

    fn execute<B: CpuBus>(&mut self, bus: &mut B, opcode: u8, pc: u16) -> Step {
        let x = opcode >> 6;
        let y = (opcode >> 3) & 7;
        let z = opcode & 7;
        match opcode {
            // ── Block 1: LD r,r' und HALT ──
            0x76 => {
                if self.ime || bus.pending() == 0 {
                    self.halted = true;
                } else {
                    self.halt_bug = true;
                }
            }
            0x40..=0x7F => {
                let v = self.get_r(bus, z);
                self.set_r(bus, y, v);
            }
            // ── Block 2: Rechenwerk mit Register ──
            0x80..=0xBF => {
                let v = self.get_r(bus, z);
                self.alu(y, v);
            }
            0x00 => {}
            0x10 => {
                self.fetch(bus);
                bus.stop();
            }
            0x08 => {
                let addr = self.fetch16(bus);
                bus.write(addr, self.sp as u8);
                bus.write(addr.wrapping_add(1), (self.sp >> 8) as u8);
            }
            0x18 => {
                let e = self.fetch(bus);
                bus.tick();
                self.pc = self.pc.wrapping_add(e as i8 as u16);
            }
            0x20 | 0x28 | 0x30 | 0x38 => {
                let e = self.fetch(bus);
                if self.cond(y - 4) {
                    bus.tick();
                    self.pc = self.pc.wrapping_add(e as i8 as u16);
                }
            }
            _ if x == 0 && z == 1 => {
                let p = y >> 1;
                if y & 1 == 0 {
                    let v = self.fetch16(bus);
                    self.set_rr(p, v);
                } else {
                    let hl = self.hl();
                    let v = self.get_rr(p);
                    let r = u32::from(hl) + u32::from(v);
                    let h = (hl & 0xFFF) + (v & 0xFFF) > 0xFFF;
                    let z = self.flag(FLAG_Z);
                    self.set_flags(z, false, h, r > 0xFFFF);
                    bus.tick();
                    self.set_hl(r as u16);
                }
            }
            _ if x == 0 && z == 2 => {
                let addr = match y >> 1 {
                    0 => self.bc(),
                    1 => self.de(),
                    _ => self.hl(),
                };
                match y {
                    0 | 2 | 4 | 6 => bus.write(addr, self.a),
                    _ => self.a = bus.read(addr),
                }
                match y {
                    4 | 5 => self.set_hl(addr.wrapping_add(1)),
                    6 | 7 => self.set_hl(addr.wrapping_sub(1)),
                    _ => {}
                }
            }
            _ if x == 0 && z == 3 => {
                let p = y >> 1;
                let v = self.get_rr(p);
                bus.idu(v);
                bus.tick();
                let v = if y & 1 == 0 {
                    v.wrapping_add(1)
                } else {
                    v.wrapping_sub(1)
                };
                self.set_rr(p, v);
            }
            _ if x == 0 && (z == 4 || z == 5) => {
                let v = self.get_r(bus, y);
                let c = self.flag(FLAG_C);
                let r = if z == 4 {
                    let r = v.wrapping_add(1);
                    self.set_flags(r == 0, false, v & 0xF == 0xF, c);
                    r
                } else {
                    let r = v.wrapping_sub(1);
                    self.set_flags(r == 0, true, v & 0xF == 0, c);
                    r
                };
                self.set_r(bus, y, r);
            }
            _ if x == 0 && z == 6 => {
                let v = self.fetch(bus);
                self.set_r(bus, y, v);
            }
            _ if x == 0 && z == 7 => self.misc(y),
            // ── Block 3 ──
            0xC0 | 0xC8 | 0xD0 | 0xD8 => {
                bus.tick();
                if self.cond(y) {
                    self.pc = self.pop(bus);
                    bus.tick();
                }
            }
            0xC9 | 0xD9 => {
                self.pc = self.pop(bus);
                bus.tick();
                if opcode == 0xD9 {
                    self.ime = true;
                    self.ei_delay = 0;
                }
            }
            0xC1 | 0xD1 | 0xE1 | 0xF1 => {
                let v = self.pop(bus);
                match y >> 1 {
                    3 => self.set_af(v),
                    p => self.set_rr(p, v),
                }
            }
            0xC5 | 0xD5 | 0xE5 | 0xF5 => {
                let v = match y >> 1 {
                    3 => self.af(),
                    p => self.get_rr(p),
                };
                bus.tick();
                self.push(bus, v);
            }
            0xC2 | 0xCA | 0xD2 | 0xDA => {
                let addr = self.fetch16(bus);
                if self.cond(y) {
                    bus.tick();
                    self.pc = addr;
                }
            }
            0xC3 => {
                let addr = self.fetch16(bus);
                bus.tick();
                self.pc = addr;
            }
            0xE9 => self.pc = self.hl(),
            0xC4 | 0xCC | 0xD4 | 0xDC | 0xCD => {
                let addr = self.fetch16(bus);
                if opcode == 0xCD || self.cond(y) {
                    bus.tick();
                    let ret = self.pc;
                    self.push(bus, ret);
                    self.pc = addr;
                }
            }
            _ if x == 3 && z == 6 => {
                let v = self.fetch(bus);
                self.alu(y, v);
            }
            _ if x == 3 && z == 7 => {
                bus.tick();
                let ret = self.pc;
                self.push(bus, ret);
                self.pc = u16::from(y) * 8;
            }
            0xCB => {
                let op = self.fetch(bus);
                self.cb(bus, op);
            }
            0xE0 => {
                let n = self.fetch(bus);
                bus.write(0xFF00 | u16::from(n), self.a);
            }
            0xF0 => {
                let n = self.fetch(bus);
                self.a = bus.read(0xFF00 | u16::from(n));
            }
            0xE2 => bus.write(0xFF00 | u16::from(self.c), self.a),
            0xF2 => self.a = bus.read(0xFF00 | u16::from(self.c)),
            0xEA => {
                let addr = self.fetch16(bus);
                bus.write(addr, self.a);
            }
            0xFA => {
                let addr = self.fetch16(bus);
                self.a = bus.read(addr);
            }
            0xE8 => {
                let e = self.fetch(bus);
                self.sp = self.sp_plus(e);
                bus.tick();
                bus.tick();
            }
            0xF8 => {
                let e = self.fetch(bus);
                let v = self.sp_plus(e);
                bus.tick();
                self.set_hl(v);
            }
            0xF9 => {
                bus.tick();
                self.sp = self.hl();
            }
            0xF3 => {
                self.ime = false;
                self.ei_delay = 0;
            }
            0xFB => {
                if !self.ime && self.ei_delay == 0 {
                    self.ei_delay = 2;
                }
            }
            // Gesperrte Opcodes: D3 DB DD E3 E4 EB EC ED F4 FC FD
            _ => {
                self.locked = true;
                self.pc = pc;
                return Step::Locked { opcode, pc };
            }
        }
        Step::Instruction { opcode, pc }
    }

    /// `RLCA RRCA RLA RRA DAA CPL SCF CCF` (Index 0–7).
    fn misc(&mut self, y: u8) {
        let a = self.a;
        let c = self.flag(FLAG_C);
        match y {
            0 => {
                self.a = a.rotate_left(1);
                self.set_flags(false, false, false, a & 0x80 != 0);
            }
            1 => {
                self.a = a.rotate_right(1);
                self.set_flags(false, false, false, a & 1 != 0);
            }
            2 => {
                self.a = (a << 1) | u8::from(c);
                self.set_flags(false, false, false, a & 0x80 != 0);
            }
            3 => {
                self.a = (a >> 1) | (u8::from(c) << 7);
                self.set_flags(false, false, false, a & 1 != 0);
            }
            4 => {
                let n = self.flag(FLAG_N);
                let h = self.flag(FLAG_H);
                let mut adj = 0u8;
                let mut carry = c;
                if n {
                    if h {
                        adj |= 0x06;
                    }
                    if c {
                        adj |= 0x60;
                    }
                    self.a = a.wrapping_sub(adj);
                } else {
                    if h || a & 0xF > 9 {
                        adj |= 0x06;
                    }
                    if c || a > 0x99 {
                        adj |= 0x60;
                        carry = true;
                    }
                    self.a = a.wrapping_add(adj);
                }
                self.set_flags(self.a == 0, n, false, carry);
            }
            5 => {
                self.a = !a;
                self.f |= FLAG_N | FLAG_H;
            }
            6 => self.f = (self.f & FLAG_Z) | FLAG_C,
            _ => self.f = (self.f & FLAG_Z) | if c { 0 } else { FLAG_C },
        }
    }

    /// Befehle mit dem Präfix `CB`.
    fn cb<B: CpuBus>(&mut self, bus: &mut B, op: u8) {
        let x = op >> 6;
        let y = (op >> 3) & 7;
        let z = op & 7;
        let v = self.get_r(bus, z);
        let c = self.flag(FLAG_C);
        let r = match x {
            0 => {
                let (r, carry) = match y {
                    0 => (v.rotate_left(1), v & 0x80 != 0),
                    1 => (v.rotate_right(1), v & 1 != 0),
                    2 => ((v << 1) | u8::from(c), v & 0x80 != 0),
                    3 => ((v >> 1) | (u8::from(c) << 7), v & 1 != 0),
                    4 => (v << 1, v & 0x80 != 0),
                    5 => ((v >> 1) | (v & 0x80), v & 1 != 0),
                    6 => (v.rotate_left(4), false),
                    _ => (v >> 1, v & 1 != 0),
                };
                self.set_flags(r == 0, false, false, carry);
                r
            }
            1 => {
                self.f = (self.f & FLAG_C) | FLAG_H | if v & (1 << y) == 0 { FLAG_Z } else { 0 };
                return;
            }
            2 => v & !(1 << y),
            _ => v | (1 << y),
        };
        self.set_r(bus, z, r);
    }
}
