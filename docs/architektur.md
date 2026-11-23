# Architektur von `kupocode-emulator-gbc`

Orientierung für die Weiterentwicklung: Aufbau, zentrale Typen, Invarianten und Tests.

## Stellung im Kupo Environment

- **Abhängigkeiten:** `kupocode-emulator-core` (Schnittstellen, Debug-Infos); re-exportiert als
  `kupocode_emulator_gbc::core`.
- **Nutzer:** der KupoCode Emulator (`kupocode-emulator-gui`) und `kupocode-compiler-gbc`
  (als dev-dependency: führt kompilierte Programme in Tests aus). Dieses Crate kennt den
  Compiler nicht.

## Modulkarte

```text
src/
  lib.rs          Crate-Wurzel, Re-Exporte (`Gb`, `Model`, `TestResult`)
  main.rs         CLI `kupocode-emulator-gbc` (run, test, disasm)
  gb.rs           `Gb`: Machine, DebugTarget, SaveState (`KGBC`); HLE-Start, Test-Erkennung
  model.rs        `Model` (Dmg, Cgb, Sgb, Sgb2), Wahl nach dem Kassettenkopf
  bus.rs          `Bus`: Adressraum, I/O, OAM-DMA, Boot-ROM, Watchpoints; taktet die Peripherie;
                  `CgbRegs` (KEY1/Doppeltakt, HDMA/GDMA, RP, FF72–FF75)
  timer.rs        `Timer`: Systemzähler/DIV, TIMA mit Flanken- und Ladeverhalten
  apu.rs          `Apu`: vier Kanäle, Frame-Sequencer, Mischung, Hochpass, 44,1 kHz
  serial.rs       `Serial`: SB/SC, interner/externer Takt, Ausgabe für Testprogramme
  link.rs         `GbLink`: zwei `Gb` im Gleichtakt am Linkkabel (Machine, DebugTarget, SaveState)
  joypad.rs       `Joypad`: P1, Tastenzuordnung, vier Spieler (SGB), Unterbrechung
  sgb.rs          `Sgb`: Pakete über P1, Paletten/Attribute, *_TRN aus dem Bild, Rahmen,
                  Maske, Mehrspieler; setzt die Ausgabe 256 × 224 zusammen
  ppu.rs          `Ppu`: Zeilen und Modi, LY/STAT, OAM-Suche, Fetcher und Pixel-FIFOs;
                  `ColorState` (CGB-Paletten, CGB-Modus oder Kompatibilitätsmodus)
  views.rs        Debug-Ansichten: Kacheln, Kachelkarten, OAM, CGB-Paletten
  cart/           `CartridgeInfo` (Kopf, Typen), `trait Mapper`, `Memory` (ROM/RAM mit Banken)
    rom_only.rs mbc1.rs (mit MBC1M) mbc2.rs mbc3.rs (mit MBC30) mbc5.rs (mit Rumble)
    mbc6.rs (Flash) mbc7.rs (Neigung, EEPROM 93LC56) huc1.rs huc3.rs (Uhr) mmm01.rs
    camera.rs     Pocket Camera: Register, Aufnahme, Schwellwertmatrix
    rtc.rs        `Rtc`: Uhr des MBC3, Latch, 48-Byte-Anhang (BGB/VBA)
    tests.rs      Banken, Modi, Uhr, Batterieformat
  cpu/            SM83: `Cpu`, `trait CpuBus` (ein Aufruf = ein M-Zyklus), `Step`
    opcodes.rs    Referenztabellen (Länge, M-Zyklen), nur für Disassembler und Tests
    tests.rs      Zyklen aller Opcodes, Flags, Unterbrechungen, HALT
  disasm.rs       `decode`/`lines` (Schreibweise RGBDS)
  hook.rs         `trait CpuHook`, `InstrCtx`, `NoHook`
```

## Invarianten und Fallstricke

- **Keine Abbilder im Repository:** Spiele, Test-ROMs und Boot-ROMs bleiben außerhalb
  (siehe `.gitignore`). Eigene Testprogramme werden im Test assembliert.
- **Kein Nintendo-Code:** Der Start läuft per HLE; ein echtes Boot-ROM kommt nur aus einer vom
  Anwender angegebenen Datei.
- **Deterministisch:** Zeit ist nur der emulierte Takt, auch für die Echtzeituhr der Kassette;
  gleiche Eingabe ergibt gleiche Bilder.
- **Reihenfolge im M-Zyklus:** Der Bus taktet erst die Peripherie (Timer, Seriell, Joypad,
  PPU, DMA) um 4 Takte, dann folgt der Zugriff der CPU.
- **`peek` ohne Seiteneffekte:** Debugger, Disassembler und DMA lesen über `Bus::peek`.
- **Zyklen nur über den Bus:** Die CPU zählt keine Zyklen aus Tabellen; jeder M-Zyklus ist
  ein Aufruf von `read`, `write` oder `tick`. `cpu/tests.rs` prüft das gegen `opcodes.rs`.
- **Länge von Mode 3 nie berechnen:** Sie ergibt sich aus Fetcher, FIFO, verworfenen Pixeln,
  Fensterstart und Objektabrufen. Die Tests in `ppu.rs` sichern 172/175/178/183 Punkte ab.
- **Gesperrte Zugriffe:** Der Bus liefert VRAM in Mode 3 und OAM in Mode 2/3 sowie während der
  DMA als `$FF` und verwirft Schreibzugriffe; `peek`/`poke` des Debuggers sind nicht gesperrt.
- **Takte beim CGB:** Timer und Serielle zählen CPU-Takte, PPU und Kassettenuhr den Grundtakt.
  Bei doppeltem Takt läuft die PPU 2 Punkte je M-Zyklus; `Bus::cycles` zählt den Grundtakt.
- **Frame-Sequencer der APU:** schaltet bei der fallenden Flanke von DIV Bit 4 (doppelter Takt:
  Bit 5) weiter, auch beim Zurücksetzen von DIV. Die Kanäle laufen im Grundtakt.
- **SGB-Übertragungen:** `*_TRN` liest die 4 KB aus den Graustufen des nächsten fertigen Bildes
  (`Sgb::end_frame`), wie die Hardware nach `BGP`. Das SNES-BIOS ist nicht nachgebildet.
- **Linkkabel:** `GbLink` führt immer das zurückliegende Gerät einen Schritt aus und gibt danach
  dessen Taktimpulse (`Serial::pulses`) an die Gegenstelle weiter; deterministisch.
- **CGB-Modus vs. Kompatibilität:** `ppu.color.cgb` = Hardware ist ein CGB, `cgb_mode` = CGB-
  Funktionen an. Register wie VBK, SVBK, KEY1 und HDMA prüfen `cgb_mode`.
- **Uhr der Kassette:** zählt emulierte Takte (`Mapper::tick`); die Zeit zwischen zwei
  Spielsitzungen stellt die GUI mit `advance_rtc` nach. Nur sie liest die Systemzeit.
- **Optimiert auch im Debug-Build:** `Cargo.toml` setzt `opt-level = 2` für Abhängigkeiten
  und Tests, sonst laufen Testprogramme zu langsam.

## Tests

- Unit-Tests in den Modulen; `cpu/tests.rs` mit flachem Testspeicher.
- `tests/system.rs`: ganze Maschine mit Kassetten aus `tests/common` (`rom`, `serial_print`,
  `mooneye_pass`): HLE-Start, Timer, DMA, Boot-ROM, Debugger, Save-States.
- `tests/render.rs`: Pixel des Framebuffers für Hintergrund, Scrollen, Objekte, Fenster.
- `tests/link.rs`: Bytetausch zweier Geräte, Bildaufbau, Save-States im Linkbetrieb.
- `tests/cgb.rs`: Paletten, Attribute, Banken, GDMA/HDMA, Taktumschaltung, Kompatibilität.
