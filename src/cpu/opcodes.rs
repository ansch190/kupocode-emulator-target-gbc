//! Referenztabellen des SM83: Länge und M-Zyklen je Opcode.
//!
//! Die CPU zählt Zyklen nie aus diesen Tabellen, sie ergeben sich aus den Buszugriffen. Die
//! Tabellen dienen dem Disassembler und den Tests, die die Zugriffe gegen sie prüfen.

/// M-Zyklen ohne Präfix; bei bedingten Befehlen der Fall „nicht genommen“, 0 = gesperrt.
#[rustfmt::skip]
pub const CYCLES: [u8; 256] = [
    1,3,2,2,1,1,2,1,5,2,2,2,1,1,2,1,
    1,3,2,2,1,1,2,1,3,2,2,2,1,1,2,1,
    2,3,2,2,1,1,2,1,2,2,2,2,1,1,2,1,
    2,3,2,2,3,3,3,1,2,2,2,2,1,1,2,1,
    1,1,1,1,1,1,2,1,1,1,1,1,1,1,2,1,
    1,1,1,1,1,1,2,1,1,1,1,1,1,1,2,1,
    1,1,1,1,1,1,2,1,1,1,1,1,1,1,2,1,
    2,2,2,2,2,2,1,2,1,1,1,1,1,1,2,1,
    1,1,1,1,1,1,2,1,1,1,1,1,1,1,2,1,
    1,1,1,1,1,1,2,1,1,1,1,1,1,1,2,1,
    1,1,1,1,1,1,2,1,1,1,1,1,1,1,2,1,
    1,1,1,1,1,1,2,1,1,1,1,1,1,1,2,1,
    2,3,3,4,3,4,2,4,2,4,3,0,3,6,2,4,
    2,3,3,0,3,4,2,4,2,4,3,0,3,0,2,4,
    3,3,2,0,0,4,2,4,4,1,4,0,0,0,2,4,
    3,3,2,1,0,4,2,4,3,2,4,1,0,0,2,4,
];

/// M-Zyklen bedingter Befehle, wenn die Bedingung erfüllt ist.
pub fn cycles_taken(opcode: u8) -> Option<u8> {
    match opcode {
        0x20 | 0x28 | 0x30 | 0x38 => Some(3),
        0xC0 | 0xC8 | 0xD0 | 0xD8 => Some(5),
        0xC2 | 0xCA | 0xD2 | 0xDA => Some(4),
        0xC4 | 0xCC | 0xD4 | 0xDC => Some(6),
        _ => None,
    }
}

/// M-Zyklen eines Befehls mit Präfix `CB` (einschließlich des Präfixes).
pub fn cb_cycles(op: u8) -> u8 {
    match (op & 7 == 6, op >> 6) {
        (false, _) => 2,
        (true, 1) => 3,
        (true, _) => 4,
    }
}

/// Länge eines Befehls in Bytes (gesperrte Opcodes: 1).
pub fn length(opcode: u8) -> u8 {
    match opcode {
        0x01 | 0x11 | 0x21 | 0x31 | 0x08 | 0xC2 | 0xC3 | 0xC4 | 0xCA | 0xCC | 0xCD | 0xD2
        | 0xD4 | 0xDA | 0xDC | 0xEA | 0xFA => 3,
        0x06 | 0x0E | 0x16 | 0x1E | 0x26 | 0x2E | 0x36 | 0x3E | 0x10 | 0x18 | 0x20 | 0x28
        | 0x30 | 0x38 | 0xC6 | 0xCE | 0xD6 | 0xDE | 0xE6 | 0xEE | 0xF6 | 0xFE | 0xE0 | 0xF0
        | 0xE8 | 0xF8 | 0xCB => 2,
        _ => 1,
    }
}

/// Ist der Opcode gesperrt (hängt die CPU auf)?
pub fn is_locked(opcode: u8) -> bool {
    matches!(
        opcode,
        0xD3 | 0xDB | 0xDD | 0xE3 | 0xE4 | 0xEB | 0xEC | 0xED | 0xF4 | 0xFC | 0xFD
    )
}
