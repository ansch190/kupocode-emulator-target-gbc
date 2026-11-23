//! Tests der Mapper.

use super::*;

/// ROM mit `banks` Bänken; jede Bank trägt ihre Nummer im ersten Byte.
fn rom(banks: usize) -> Vec<u8> {
    let mut rom = vec![0u8; banks * 0x4000];
    for b in 0..banks {
        rom[b * 0x4000] = b as u8;
        rom[b * 0x4000 + 1] = (b >> 8) as u8;
    }
    rom
}

#[test]
fn mbc1_banken_und_modus() {
    let mut m = Mbc1::new(rom(128), 0x8000, true);
    assert_eq!(m.read(0x4000), 1);
    m.write(0x2000, 0); // 0 wird zu 1
    assert_eq!(m.read(0x4000), 1);
    m.write(0x2000, 0x21); // nur 5 Bit
    m.write(0x4000, 2);
    assert_eq!(m.read(0x4000), 0x41);
    assert_eq!(m.read(0x0000), 0);
    m.write(0x6000, 1);
    assert_eq!(m.read(0x0000), 0x40);
    // RAM nur nach Freigabe, Bank aus BANK2 im Modus 1
    m.write(0xA000, 0x77);
    assert_eq!(m.read(0xA000), 0xFF);
    m.write(0x0000, 0x0A);
    m.write(0xA000, 0x77);
    m.write(0x4000, 0);
    assert_eq!(m.read(0xA000), 0x00);
    m.write(0x4000, 2);
    assert_eq!(m.read(0xA000), 0x77);
}

#[test]
fn mbc1_multicart() {
    let mut r = rom(64);
    for b in [0usize, 0x10, 0x20, 0x30] {
        r[b * 0x4000 + 0x104..b * 0x4000 + 0x134].fill(0xAB);
    }
    let mut m = Mbc1::new(r, 0, false);
    m.write(0x4000, 1);
    m.write(0x2000, 0x13); // nur 4 Bit von BANK1
    assert_eq!(m.read(0x4000), 0x13);
    m.write(0x6000, 1);
    assert_eq!(m.read(0x0000), 0x10);
}

#[test]
fn mbc2_ram_und_banken() {
    let mut m = Mbc2::new(rom(16), true);
    m.write(0x2100, 5);
    assert_eq!(m.read(0x4000), 5);
    m.write(0x0000, 0x0A);
    m.write(0xA000, 0x3C);
    assert_eq!(m.read(0xA000), 0xFC);
    assert_eq!(m.read(0xA200), 0xFC); // gespiegelt
}

#[test]
fn mbc3_uhr_latch_und_batterieformat() {
    let mut m = Mbc3::new(rom(4), 0x2000, true, true);
    m.write(0x0000, 0x0A);
    m.write(0x4000, 0x08);
    m.write(0xA000, 58); // Sekunden
    m.tick(rtc::CYCLES_PER_SECOND * 3);
    m.write(0x6000, 0);
    m.write(0x6000, 1);
    assert_eq!(m.read(0xA000), 1);
    m.write(0x4000, 0x09);
    assert_eq!(m.read(0xA000), 1); // Minuten

    let save = m.battery_ram(1_700_000_000);
    assert_eq!(save.len(), 0x2000 + 48);
    let mut n = Mbc3::new(rom(4), 0x2000, true, true);
    assert_eq!(n.load_battery_ram(&save), Some(1_700_000_000));
    n.advance_rtc(86_400 + 60);
    n.write(0x0000, 0x0A);
    n.write(0x6000, 0);
    n.write(0x6000, 1);
    n.write(0x4000, 0x0B);
    assert_eq!(n.read(0xA000), 1); // ein Tag später
    n.write(0x4000, 0x09);
    assert_eq!(n.read(0xA000), 2);
}

#[test]
fn rtc_haelt_an_und_ueberlauf() {
    let mut rtc = Rtc::default();
    rtc.write(4, 0x41); // angehalten, Tag-Bit 8
    rtc.write(3, 0xFF);
    rtc.tick(rtc::CYCLES_PER_SECOND * 100);
    assert_eq!(rtc.regs.seconds, 0);
    rtc.write(4, 0x01);
    rtc.write(2, 23);
    rtc.write(1, 59);
    rtc.write(0, 59);
    rtc.tick(rtc::CYCLES_PER_SECOND);
    assert_eq!((rtc.regs.days_low, rtc.regs.days_high), (0, 0x80));
}

#[test]
fn mbc5_neun_bit_bank_und_rumble() {
    let mut m = Mbc5::new(rom(512), 0x2000, false, true);
    m.write(0x2000, 0x00);
    assert_eq!(m.read(0x4000), 0); // Bank 0 wählbar
    m.write(0x2000, 0x05);
    m.write(0x3000, 0x01);
    assert_eq!((m.read(0x4000), m.read(0x4001)), (5, 1));
    m.write(0x4000, 0x08);
    assert!(m.rumble());
}

#[test]
fn kopf_und_typen() {
    let mut r = rom(4);
    r[0x147] = 0x13;
    r[0x148] = 1;
    r[0x149] = 3;
    r[0x14D] = header_checksum(&r);
    let info = CartridgeInfo::parse(&r).unwrap();
    assert!(info.checksum_ok && info.has_battery());
    assert_eq!(
        (info.type_name(), info.rom_banks, info.ram_bytes),
        ("MBC3", 4, 0x8000)
    );
    assert!(info.into_mapper(r).is_ok());
}
