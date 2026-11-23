//! Tests des SM83-Kerns an einem flachen 64-KB-Speicher.

use super::opcodes::{CYCLES, cb_cycles, cycles_taken, is_locked};
use super::*;

/// Flacher Speicher, der M-Zyklen und Unterbrechungen nachbildet.
struct Ram {
    mem: Vec<u8>,
    cycles: u32,
    if_: u8,
}

impl Ram {
    fn new(code: &[u8]) -> Self {
        let mut mem = vec![0; 0x10000];
        mem[0x100..0x100 + code.len()].copy_from_slice(code);
        Self {
            mem,
            cycles: 0,
            if_: 0,
        }
    }
}

impl CpuBus for Ram {
    fn read(&mut self, addr: u16) -> u8 {
        self.cycles += 1;
        self.mem[addr as usize]
    }
    fn write(&mut self, addr: u16, value: u8) {
        self.cycles += 1;
        self.mem[addr as usize] = value;
    }
    fn tick(&mut self) {
        self.cycles += 1;
    }
    fn pending(&self) -> u8 {
        self.mem[0xFFFF] & self.if_ & 0x1F
    }
    fn acknowledge(&mut self, bit: u8) {
        self.if_ &= !(1 << bit);
    }
}

fn cpu() -> Cpu {
    Cpu {
        pc: 0x100,
        sp: 0xD000,
        ..Cpu::new()
    }
}

/// Führt einen Befehl aus und liefert seine M-Zyklen.
fn cycles_of(code: &[u8], prep: impl Fn(&mut Cpu)) -> u32 {
    let mut ram = Ram::new(code);
    let mut c = cpu();
    c.set_hl(0xC000);
    prep(&mut c);
    c.step(&mut ram);
    ram.cycles
}

#[test]
fn zyklen_aller_opcodes_stimmen() {
    for op in 0..=255u8 {
        // STOP liest sein Folgebyte; HALT wartet: beide eigens geprüft.
        if is_locked(op) || op == 0xCB || op == 0x76 || op == 0x10 {
            continue;
        }
        // Bedingungen nicht erfüllt: NZ/NC mit gesetzten Flags, Z/C mit gelöschten.
        let nicht = |c: &mut Cpu| {
            c.f = if op & 0x08 == 0 { FLAG_Z | FLAG_C } else { 0 };
        };
        let ja = |c: &mut Cpu| {
            c.f = if op & 0x08 == 0 { 0 } else { FLAG_Z | FLAG_C };
        };
        assert_eq!(
            cycles_of(&[op, 0, 0], nicht),
            u32::from(CYCLES[op as usize]),
            "Opcode {op:02X}"
        );
        if let Some(t) = cycles_taken(op) {
            assert_eq!(
                cycles_of(&[op, 0, 0], ja),
                u32::from(t),
                "Opcode {op:02X} genommen"
            );
        }
    }
    for op in 0..=255u8 {
        assert_eq!(
            cycles_of(&[0xCB, op], |_| {}),
            u32::from(cb_cycles(op)),
            "CB {op:02X}"
        );
    }
}

#[test]
fn gesperrte_opcodes_haengen() {
    let mut ram = Ram::new(&[0xD3]);
    let mut c = cpu();
    assert_eq!(
        c.step(&mut ram),
        Step::Locked {
            opcode: 0xD3,
            pc: 0x100
        }
    );
    assert!(matches!(c.step(&mut ram), Step::Locked { .. }));
    assert_eq!(c.pc, 0x100);
}

#[test]
fn rechenwerk_und_flags() {
    // LD A,0x3A; ADD A,0xC6 → 0x00, Z H C
    let mut ram = Ram::new(&[0x3E, 0x3A, 0xC6, 0xC6, 0xD6, 0x01, 0x27]);
    let mut c = cpu();
    c.step(&mut ram);
    c.step(&mut ram);
    assert_eq!((c.a, c.f), (0x00, FLAG_Z | FLAG_H | FLAG_C));
    // SUB 1 → 0xFF, N H C
    c.step(&mut ram);
    assert_eq!((c.a, c.f), (0xFF, FLAG_N | FLAG_H | FLAG_C));
    // DAA nach Subtraktion mit C und H: 0xFF - 0x66 = 0x99
    c.step(&mut ram);
    assert_eq!((c.a, c.f), (0x99, FLAG_N | FLAG_C));
}

#[test]
fn daa_nach_addition() {
    // LD A,0x45; ADD A,0x38 → 0x7D; DAA → 0x83
    let mut ram = Ram::new(&[0x3E, 0x45, 0xC6, 0x38, 0x27]);
    let mut c = cpu();
    for _ in 0..3 {
        c.step(&mut ram);
    }
    assert_eq!(c.a, 0x83);
    assert_eq!(c.f & FLAG_C, 0);
}

#[test]
fn add_sp_und_ld_hl_sp_flags_aus_unterem_byte() {
    // LD SP,0x00FF; ADD SP,1 → 0x0100, H C
    let mut ram = Ram::new(&[0x31, 0xFF, 0x00, 0xE8, 0x01, 0xF8, 0xFF]);
    let mut c = cpu();
    c.step(&mut ram);
    c.step(&mut ram);
    assert_eq!((c.sp, c.f), (0x0100, FLAG_H | FLAG_C));
    // LD HL,SP-1 → 0x00FF, keine Flags (0x00 + 0xFF ohne Überträge)
    c.step(&mut ram);
    assert_eq!((c.hl(), c.f), (0x00FF, 0));
}

#[test]
fn cb_befehle() {
    // LD B,0x81; RLC B → 0x03 C; SWAP B → 0x30; BIT 7,B → Z; SET 7,B; SRA B → 0xD8
    let mut ram = Ram::new(&[
        0x06, 0x81, 0xCB, 0x00, 0xCB, 0x30, 0xCB, 0x78, 0xCB, 0xF8, 0xCB, 0x28,
    ]);
    let mut c = cpu();
    c.step(&mut ram);
    c.step(&mut ram);
    assert_eq!((c.b, c.f), (0x03, FLAG_C));
    c.step(&mut ram);
    assert_eq!((c.b, c.f), (0x30, 0));
    c.step(&mut ram);
    assert_eq!(c.f, FLAG_Z | FLAG_H);
    c.step(&mut ram);
    c.step(&mut ram);
    assert_eq!(c.b, 0xD8);
}

#[test]
fn push_pop_af_maskiert_flags() {
    // LD BC,0x12FF; PUSH BC; POP AF
    let mut ram = Ram::new(&[0x01, 0xFF, 0x12, 0xC5, 0xF1]);
    let mut c = cpu();
    for _ in 0..3 {
        c.step(&mut ram);
    }
    assert_eq!(c.af(), 0x12F0);
}

#[test]
fn ei_wirkt_nach_dem_folgenden_befehl() {
    // EI; NOP; NOP mit anstehendem VBlank
    let mut ram = Ram::new(&[0xFB, 0x00, 0x00]);
    ram.mem[0xFFFF] = 0x01;
    ram.if_ = 0x01;
    let mut c = cpu();
    assert!(matches!(
        c.step(&mut ram),
        Step::Instruction { opcode: 0xFB, .. }
    ));
    assert!(matches!(
        c.step(&mut ram),
        Step::Instruction { opcode: 0x00, .. }
    ));
    let before = ram.cycles;
    assert_eq!(
        c.step(&mut ram),
        Step::Interrupt {
            vector: 0x40,
            return_pc: 0x102
        }
    );
    assert_eq!(ram.cycles - before, 5);
    assert_eq!(ram.if_, 0);
    assert!(!c.ime);
}

#[test]
fn ei_di_nimmt_keine_unterbrechung_an() {
    let mut ram = Ram::new(&[0xFB, 0xF3, 0x00]);
    ram.mem[0xFFFF] = 0x01;
    ram.if_ = 0x01;
    let mut c = cpu();
    for _ in 0..3 {
        assert!(matches!(c.step(&mut ram), Step::Instruction { .. }));
    }
}

#[test]
fn unterbrechung_abgebrochen_wenn_ie_beim_sichern_ueberschrieben() {
    // SP = 0x0000: Das obere PC-Byte (0x01) landet in IE = 0xFFFF.
    // Angefordert ist der Timer (Bit 2); nach dem Überschreiben ist nur VBlank erlaubt.
    let mut ram = Ram::new(&[0x00]);
    let mut c = cpu();
    c.sp = 0x0000;
    c.ime = true;
    ram.mem[0xFFFF] = 0x04;
    ram.if_ = 0x04;
    assert_eq!(
        c.step(&mut ram),
        Step::Interrupt {
            vector: 0x0000,
            return_pc: 0x100
        }
    );
}

#[test]
fn halt_wartet_und_halt_fehler() {
    // HALT ohne IME und ohne Anforderung: wartet, bis IF gesetzt wird, dann weiter.
    let mut ram = Ram::new(&[0x76, 0x04]);
    ram.mem[0xFFFF] = 0x01;
    let mut c = cpu();
    c.step(&mut ram);
    assert_eq!(c.step(&mut ram), Step::Halted);
    ram.if_ = 0x01;
    assert_eq!(c.step(&mut ram), Step::Halted);
    c.step(&mut ram);
    assert_eq!(c.b, 1);

    // HALT-Fehler: IME aus, Anforderung steht an → INC B wird zweimal ausgeführt.
    let mut ram = Ram::new(&[0x76, 0x04]);
    ram.mem[0xFFFF] = 0x01;
    ram.if_ = 0x01;
    let mut c = cpu();
    c.step(&mut ram);
    c.step(&mut ram);
    c.step(&mut ram);
    assert_eq!((c.b, c.pc), (2, 0x102));
}

#[test]
fn call_ret_und_rst() {
    // CALL 0x0200; ... 0x0200: RST 0x08 ... 0x0008: RET; 0x0201: RET
    let mut ram = Ram::new(&[0xCD, 0x00, 0x02]);
    ram.mem[0x200] = 0xCF;
    ram.mem[0x201] = 0xC9;
    ram.mem[0x08] = 0xC9;
    let mut c = cpu();
    c.step(&mut ram);
    assert_eq!((c.pc, c.sp), (0x200, 0xCFFE));
    c.step(&mut ram);
    assert_eq!(c.pc, 0x08);
    c.step(&mut ram);
    c.step(&mut ram);
    assert_eq!((c.pc, c.sp), (0x103, 0xD000));
}

#[test]
fn ei_halt_kehrt_zum_halt_zurueck() {
    // EI; HALT mit anstehender Unterbrechung: Rücksprung auf HALT (0x101).
    let mut ram = Ram::new(&[0xFB, 0x76, 0x00]);
    ram.mem[0xFFFF] = 0x01;
    ram.if_ = 0x01;
    let mut c = cpu();
    c.step(&mut ram);
    c.step(&mut ram);
    assert_eq!(
        c.step(&mut ram),
        Step::Interrupt {
            vector: 0x40,
            return_pc: 0x101
        }
    );
}
