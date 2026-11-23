//! Hilfen für die Tests: kleine Kassetten aus Maschinencode.

#![allow(dead_code)]

use kupocode_emulator_gbc::cart::header_checksum;

/// 32-KB-Kassette ohne Mapper: Einsprung `0x100` springt nach `0x150`, dort steht `code`.
/// `extra` legt weitere Bytes an festen Adressen ab (z. B. Unterbrechungsroutinen).
pub fn rom(code: &[u8], extra: &[(u16, &[u8])]) -> Vec<u8> {
    rom_with_flags(code, extra, 0x00, 0x00)
}

/// Wie [`rom`], mit CGB-Flag (`0x143`) und SGB-Flag (`0x146`).
pub fn rom_with_flags(code: &[u8], extra: &[(u16, &[u8])], cgb: u8, sgb: u8) -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    rom[0x100..0x104].copy_from_slice(&[0x00, 0xC3, 0x50, 0x01]);
    rom[0x134..0x138].copy_from_slice(b"TEST");
    rom[0x143] = cgb;
    rom[0x146] = sgb;
    rom[0x14B] = 0x33;
    rom[0x150..0x150 + code.len()].copy_from_slice(code);
    for (addr, bytes) in extra {
        let a = usize::from(*addr);
        rom[a..a + bytes.len()].copy_from_slice(bytes);
    }
    rom[0x14D] = header_checksum(&rom);
    rom
}

/// Code, der `text` über die serielle Schnittstelle sendet und dann stehen bleibt.
pub fn serial_print(text: &str) -> Vec<u8> {
    let mut code = Vec::new();
    for b in text.bytes() {
        // LD A,b; LDH (SB),A; LD A,$81; LDH (SC),A; warten: LDH A,(SC); BIT 7,A; JR NZ,warten
        code.extend_from_slice(&[0x3E, b, 0xE0, 0x01, 0x3E, 0x81, 0xE0, 0x02]);
        code.extend_from_slice(&[0xF0, 0x02, 0xCB, 0x7F, 0x20, 0xFA]);
    }
    code.extend_from_slice(&[0x18, 0xFE]);
    code
}

/// Code, der die Mooneye-Erfolgsregister setzt und `LD B,B` ausführt.
pub fn mooneye_pass() -> Vec<u8> {
    vec![
        0x06, 3, 0x0E, 5, 0x16, 8, 0x1E, 13, 0x26, 21, 0x2E, 34, 0x40, 0x18, 0xFE,
    ]
}
