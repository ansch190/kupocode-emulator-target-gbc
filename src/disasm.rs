//! Disassembler des SM83 (Schreibweise wie RGBDS).

use crate::cpu::opcodes::{is_locked, length};

const R: [&str; 8] = ["b", "c", "d", "e", "h", "l", "[hl]", "a"];
const RR: [&str; 4] = ["bc", "de", "hl", "sp"];
const RR2: [&str; 4] = ["bc", "de", "hl", "af"];
const CC: [&str; 4] = ["nz", "z", "nc", "c"];
const ALU: [&str; 8] = [
    "add a,", "adc a,", "sub a,", "sbc a,", "and a,", "xor a,", "or a,", "cp a,",
];
const MISC: [&str; 8] = ["rlca", "rrca", "rla", "rra", "daa", "cpl", "scf", "ccf"];
const ROT: [&str; 8] = ["rlc", "rrc", "rl", "rr", "sla", "sra", "swap", "srl"];

/// Entschlüsselt den Befehl an `addr`. `bytes` beginnt an `addr` und enthält mindestens
/// 3 Bytes (fehlende gelten als 0). Liefert Text und Länge.
pub fn decode(bytes: &[u8], addr: u16) -> (String, u8) {
    let b = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let op = b(0);
    let n = b(1);
    let nn = u16::from_le_bytes([b(1), b(2)]);
    let rel = addr.wrapping_add(2).wrapping_add(n as i8 as u16);
    let x = op >> 6;
    let y = ((op >> 3) & 7) as usize;
    let z = (op & 7) as usize;
    let text = match op {
        0x00 => "nop".into(),
        0x10 => "stop".into(),
        0x76 => "halt".into(),
        0x08 => format!("ld [${nn:04X}], sp"),
        0x18 => format!("jr ${rel:04X}"),
        0x20 | 0x28 | 0x30 | 0x38 => format!("jr {}, ${rel:04X}", CC[y - 4]),
        _ if x == 0 && z == 1 && y & 1 == 0 => format!("ld {}, ${nn:04X}", RR[y >> 1]),
        _ if x == 0 && z == 1 => format!("add hl, {}", RR[y >> 1]),
        _ if x == 0 && z == 2 => {
            let m = ["[bc]", "[de]", "[hl+]", "[hl-]"][y >> 1];
            if y & 1 == 0 {
                format!("ld {m}, a")
            } else {
                format!("ld a, {m}")
            }
        }
        _ if x == 0 && z == 3 => {
            format!("{} {}", if y & 1 == 0 { "inc" } else { "dec" }, RR[y >> 1])
        }
        _ if x == 0 && z == 4 => format!("inc {}", R[y]),
        _ if x == 0 && z == 5 => format!("dec {}", R[y]),
        _ if x == 0 && z == 6 => format!("ld {}, ${n:02X}", R[y]),
        _ if x == 0 => MISC[y].into(),
        _ if x == 1 => format!("ld {}, {}", R[y], R[z]),
        _ if x == 2 => format!("{} {}", ALU[y], R[z]),
        0xC0 | 0xC8 | 0xD0 | 0xD8 => format!("ret {}", CC[y]),
        0xC9 => "ret".into(),
        0xD9 => "reti".into(),
        0xC1 | 0xD1 | 0xE1 | 0xF1 => format!("pop {}", RR2[y >> 1]),
        0xC5 | 0xD5 | 0xE5 | 0xF5 => format!("push {}", RR2[y >> 1]),
        0xC2 | 0xCA | 0xD2 | 0xDA => format!("jp {}, ${nn:04X}", CC[y]),
        0xC3 => format!("jp ${nn:04X}"),
        0xE9 => "jp hl".into(),
        0xC4 | 0xCC | 0xD4 | 0xDC => format!("call {}, ${nn:04X}", CC[y]),
        0xCD => format!("call ${nn:04X}"),
        _ if z == 6 => format!("{} ${n:02X}", ALU[y]),
        _ if z == 7 => format!("rst ${:02X}", y * 8),
        0xCB => {
            let (cx, cy, cz) = (n >> 6, ((n >> 3) & 7) as usize, (n & 7) as usize);
            match cx {
                0 => format!("{} {}", ROT[cy], R[cz]),
                1 => format!("bit {cy}, {}", R[cz]),
                2 => format!("res {cy}, {}", R[cz]),
                _ => format!("set {cy}, {}", R[cz]),
            }
        }
        0xE0 => format!("ldh [$FF{n:02X}], a"),
        0xF0 => format!("ldh a, [$FF{n:02X}]"),
        0xE2 => "ldh [c], a".into(),
        0xF2 => "ldh a, [c]".into(),
        0xEA => format!("ld [${nn:04X}], a"),
        0xFA => format!("ld a, [${nn:04X}]"),
        0xE8 => format!("add sp, {}", n as i8),
        0xF8 => format!("ld hl, sp{:+}", n as i8),
        0xF9 => "ld sp, hl".into(),
        0xF3 => "di".into(),
        0xFB => "ei".into(),
        _ => {
            debug_assert!(is_locked(op));
            format!("db ${op:02X}")
        }
    };
    (text, length(op))
}

/// Disassembliert `count` Befehle ab `addr`; `read` liefert Bytes ohne Seiteneffekte.
pub fn lines(
    addr: u16,
    count: usize,
    mut read: impl FnMut(u16) -> u8,
) -> Vec<(u16, Vec<u8>, String)> {
    let mut out = Vec::with_capacity(count);
    let mut a = addr;
    for _ in 0..count {
        let raw = [read(a), read(a.wrapping_add(1)), read(a.wrapping_add(2))];
        let (text, len) = decode(&raw, a);
        out.push((a, raw[..len as usize].to_vec(), text));
        a = a.wrapping_add(u16::from(len));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beispiele() {
        assert_eq!(decode(&[0x3E, 0x12], 0).0, "ld a, $12");
        assert_eq!(decode(&[0x18, 0xFE], 0x150).0, "jr $0150");
        assert_eq!(decode(&[0xCB, 0x7E], 0).0, "bit 7, [hl]");
        assert_eq!(decode(&[0xE0, 0x40], 0).0, "ldh [$FF40], a");
        assert_eq!(decode(&[0xF8, 0xFE], 0).0, "ld hl, sp-2");
        assert_eq!(decode(&[0xCD, 0x34, 0x12], 0), ("call $1234".into(), 3));
        assert_eq!(decode(&[0xD3], 0), ("db $D3".into(), 1));
    }

    #[test]
    fn alle_opcodes_haben_text() {
        for op in 0..=255u8 {
            assert!(!decode(&[op, 0, 0], 0).0.is_empty());
        }
    }
}
