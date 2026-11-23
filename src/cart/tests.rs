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

#[test]
fn huc1_infrarot_und_banken() {
    let mut m = HuC1::new(rom(64), 0x8000, true);
    m.write(0x2000, 0x21);
    assert_eq!(m.read(0x4000), 0x21);
    m.write(0xA000, 0x55);
    assert_eq!(m.read(0xA000), 0x55);
    m.write(0x0000, 0x0E);
    assert_eq!(m.read(0xA000), 0xC0);
}

#[test]
fn mmm01_menue_dann_spiel() {
    let mut m = Mmm01::new(rom(64), 0, false);
    assert_eq!(m.read(0x0000), 62); // Menü in den letzten beiden Bänken
    assert_eq!(m.read(0x4000), 63);
    m.write(0x2000, 8); // Spiel ab Bank 8
    m.write(0x6000, 3 << 2); // 8 Bänke
    m.write(0x0000, 0x40); // sperren
    assert_eq!(m.read(0x0000), 8);
    m.write(0x2000, 3);
    assert_eq!(m.read(0x4000), 11);
    m.write(0x2000, 9); // maskiert auf 8 Bänke
    assert_eq!(m.read(0x4000), 9);
}

#[test]
fn mbc6_fenster_und_flash() {
    let mut r = rom(8);
    r[0x2000 * 5] = 0x55; // 8-KB-Bank 5
    let mut m = Mbc6::new(r, 0x8000, true);
    m.write(0x2000, 5);
    assert_eq!(m.read(0x4000), 0x55);
    m.write(0x0C00, 1); // Flash an
    m.write(0x1000, 1); // beschreibbar
    m.write(0x3800, 0x08); // Fenster B aus dem Flash
    m.write(0x3000, 2);
    m.write(0x6000, 0x12);
    assert_eq!(m.read(0x6000), 0x12);
    m.write(0x0000, 0x0A);
    m.write(0x0800, 3);
    m.write(0xB000, 0x77);
    assert_eq!(m.read(0xB000), 0x77);
}

/// Sendet Bits über `A080` (CS gesetzt, Takt steigend).
fn eeprom_bits(m: &mut Mbc7, bits: &[u8]) {
    for &b in bits {
        m.write(0xA080, 0x80 | b << 1);
        m.write(0xA080, 0xC0 | b << 1);
    }
}

#[test]
fn mbc7_eeprom_schreiben_und_lesen() {
    let mut m = Mbc7::new(rom(4));
    m.write(0x0000, 0x0A);
    m.write(0x4000, 0x40);
    // EWEN: 1 00 11xxxxxx
    eeprom_bits(&mut m, &[1, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0]);
    m.write(0xA080, 0x00);
    // WRITE Adresse 5: 1 01 0000101, dann 0x1234
    eeprom_bits(&mut m, &[1, 0, 1, 0, 0, 0, 0, 0, 1, 0, 1]);
    let data: Vec<u8> = (0..16).rev().map(|i| (0x1234u16 >> i & 1) as u8).collect();
    eeprom_bits(&mut m, &data);
    m.write(0xA080, 0x00);
    // READ Adresse 5: 1 10 0000101, dann 16 Takte
    eeprom_bits(&mut m, &[1, 1, 0, 0, 0, 0, 0, 0, 1, 0, 1]);
    assert_eq!(m.read(0xA080) & 1, 0); // Dummy-Bit
    let mut v = 0u16;
    for _ in 0..16 {
        m.write(0xA080, 0x80);
        m.write(0xA080, 0xC0);
        v = v << 1 | u16::from(m.read(0xA080) & 1);
    }
    assert_eq!(v, 0x1234);
    assert_eq!(&m.battery_ram(0)[10..12], &[0x34, 0x12]);
}

#[test]
fn mbc7_neigung() {
    let mut m = Mbc7::new(rom(4));
    m.write(0x0000, 0x0A);
    m.write(0x4000, 0x40);
    m.set_tilt(0, 0);
    m.write(0xA000, 0x55);
    m.write(0xA010, 0xAA);
    assert_eq!(
        u16::from(m.read(0xA020)) | u16::from(m.read(0xA030)) << 8,
        0x81D0
    );
    m.set_tilt(127, 0);
    m.write(0xA000, 0x55);
    m.write(0xA010, 0xAA);
    let x = u16::from(m.read(0xA020)) | u16::from(m.read(0xA030)) << 8;
    assert!(x < 0x81D0 - 0x60, "{x:04X}");
}

#[test]
fn huc3_uhr_ueber_befehle() {
    let mut m = HuC3::new(rom(4), 0x8000, true);
    m.tick(rtc::CYCLES_PER_SECOND * 125); // 2 Minuten 5 Sekunden
    m.write(0x0000, 0x0B);
    m.write(0xA000, 0x60); // Uhrzeit in den Speicher
    m.write(0xA000, 0x40); // Adresse 0
    m.write(0xA000, 0x50);
    m.write(0xA000, 0x10); // lesen
    m.write(0x0000, 0x0C);
    assert_eq!(m.read(0xA000) & 0x0F, 2);
    m.write(0x0000, 0x0D);
    assert_eq!(m.read(0xA000), 0x01);
}

#[test]
fn kamera_nimmt_bild_auf() {
    let mut m = Camera::new(rom(4), true);
    m.set_camera_image(&vec![0u8; 128 * 112]); // schwarz
    m.write(0x0000, 0x0A);
    m.write(0x4000, 0x10);
    m.write(0xA002, 0x10); // Belichtung
    for i in 0..48u16 {
        m.write(0xA006 + i, [0x40, 0x80, 0xC0][usize::from(i % 3)]);
    }
    m.write(0xA000, 0x01);
    assert_eq!(m.read(0xA000) & 1, 1);
    m.tick(4_000_000);
    assert_eq!(m.read(0xA000) & 1, 0);
    m.write(0x4000, 0x00);
    assert_eq!((m.read(0xA100), m.read(0xA101)), (0xFF, 0xFF)); // Farbe 3
}

#[test]
fn neue_kassettentypen_werden_erkannt() {
    for t in [0x0B, 0x20, 0x22, 0xFC, 0xFE, 0xFF] {
        let mut r = rom(8);
        r[0x147] = t;
        r[0x148] = 2;
        r[0x149] = 3;
        let info = CartridgeInfo::parse(&r).unwrap();
        assert!(info.into_mapper(r).is_ok(), "Typ {t:02X}");
    }
}
