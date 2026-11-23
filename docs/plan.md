# Plan: Game-Boy- und Game-Boy-Color-Emulator

Auszug aus dem Gesamtplan in `kupocode-compiler-target-gbc/docs/plan.md` (Emulator und KupoCode
Emulator). Der Gesamtplan ist maßgeblich; Abweichungen stehen dort unter „Umsetzung“.

**Entscheidungen des Users (2026-11-23):**
- **Zyklengenau:** Der Emulator arbeitet genau auf T-Zyklen. Die CPU greift pro M-Zyklus auf den Bus zu.
  Die PPU arbeitet mit Pixel-FIFO und variabler Länge von Mode 3. Timer, DMA und APU bilden ihre
  Eigenheiten nach. Ziel sind die gängigen Testsuiten und kommerzielle Spiele. Ein JIT ist nicht nötig.
- **Start:** Ohne Boot-ROM setzt ein eigener HLE-Start den Zustand nach dem Boot für DMG, CGB und SGB,
  auf dem CGB samt Palettenwahl für DMG-Spiele über die Titel-Prüfsumme. Ein echtes Boot-ROM lässt sich
  optional per Datei laden. Es kommt kein Nintendo-Code ins Repository.
- **Umfang:** DMG und CGB mit ROM-only, MBC1, MBC2, MBC3 mit Echtzeituhr und MBC5. Dazu kommen:
  - **Super Game Boy**,
  - **Linkkabel** mit zwei Geräten in einem Prozess, deterministisch,
  - **weitere Mapper:** MBC6, MBC7, HuC1, HuC3, MMM01 und Pocket Camera.
- **Compiler:** ein Ziel mit wählbarem Modus: nur DMG, CGB-kompatibel oder nur CGB. Wahlweise kommt das
  SGB-Flag dazu. CGB- und SGB-Funktionen bietet das SDK mit Laufzeitprüfung an.
- **Logo:** Liegt im Projekt eine `logo.bin` (48 Bytes), wird sie in den ROM-Kopf bei `0x104`
  eingetragen, sonst bleibt das Feld leer. Das gleiche Prinzip gilt für `ipl3.bin` beim N64 und
  `ip.bin` beim Saturn. Ohne Logo läuft das ROM nur mit HLE-Start, nicht auf echter Hardware.
- **Statische Funktionsrahmen** wie bei NES und SNES. Die Rahmen kommen aus `compiler-core/src/frames`.
  Rekursion ist ein Fehler.

**Weitere Festlegungen dieses Plans:**
- **Kennung:** Die Plattform heißt überall `"gb"`: IDE-Kennung (fest), `Platform::id` in der GUI und
  `MachineInfo::platform`. Der Anzeigename wird „Game Boy / Game Boy Color“. Die Repo- und Crate-Namen
  bleiben `…-gbc`.
- **Modelle:**
  - Referenz sind die Revisionen DMG-B und CGB-E. Andere Revisionen werden nur nachgebildet, wenn Tests
    das verlangen.
  - SGB1 läuft mit dem vom SNES abgeleiteten Takt, SGB2 mit 4,194304 MHz.
  - Das Modell wählt sich automatisch aus dem Kopf: CGB-Flag `0x143`, SGB-Flag `0x146`. Es lässt sich
    erzwingen.
- **Takte:**
  - Der Scheduler zählt in Einheiten von 8,388608 MHz, damit einfacher und doppelter CGB-Takt ganzzahlig
    bleiben.
  - Ein Bild dauert 70224 T-Zyklen und läuft mit 59,7275 Hz.
- **Video:**
  - Das Bild hat 160×144 Pixel in RGBA, mit SGB-Rahmen 256×224. `FrameBuffer` kann die Größe bereits
    wechseln.
  - DMG-Farben sind neutrale Graustufen. CGB-Farben werden von RGB555 linear skaliert, ohne
    LCD-Farbkorrektur.
- **Audio:** Stereo mit 44,1 kHz wie beim NES.
- **Taster:** Steuerkreuz, A, B, START und SELECT, belegt wie beim NES. MBC7 nutzt für die Neigung
  `left_stick`, SGB-Mehrspieler nutzt die Pads 1 bis 4. Am Kern ändert sich dafür nichts.
- **Save-States:** Kennung `b"KGBC"`, Version ab 1.
- **Batterie und Echtzeituhr:** Die Uhr läuft mit den emulierten Takten und bleibt damit deterministisch.
  - Die Batteriedatei speichert RAM, Uhrregister und einen Zeitstempel im üblichen 48-Byte-Anhang von
    BGB und VBA. So lassen sich Spielstände mit anderen Emulatoren austauschen.
  - Nur die GUI liest die Systemzeit. Beim Laden stellt sie die Uhr per `advance_rtc(sekunden)` vor.
- **Linkkabel:** `GbLink` hält zwei `Gb` im Gleichtakt und implementiert selbst `Machine`, `DebugTarget`
  und `SaveState`.
  - Das Bild zeigt beide Geräte nebeneinander. Im Debugger schaltet man über `cpus()`/`select_cpu()`
    zwischen ihnen um.
  - Dafür ist keine Änderung am Kern nötig.
- **Compiler-Ziel:**
  - Datenlayout wie beim NES: Zeiger 2, Enum 1, Länge 2, Ausrichtung 1, maximal 0x10000.
  - Festkommatypen `fix8` (Q8.8) und `lfix8` (Q24.8), dazu `angle8`. Kein `i64` und kein Float.
  - Unterbrechungsarten `vblank`, `stat`, `timer`, `serial`, `joypad`. Jede Unterbrechung bekommt einen
    eigenen Rahmenkontext `name@vblank`.
  - Attribute:
    - `#[bank(n)]` für Code und Daten,
    - `#[hram]` für schnelle statische Daten über `LDH`,
    - `#[sram]` für Kassetten-RAM,
    - `#[wram(n)]` für die WRAM-Banken des CGB.
- **ROM:**
  - Endung `.gb` bei „nur DMG“, sonst `.gbc`.
  - Kassettentyp: ROM-only, solange das Programm in 32 KB passt, sonst MBC5. MBC1 und MBC3 lassen sich
    erzwingen.
  - Die Größe von SRAM und das Batterie-Flag ergeben sich aus der Nutzung von `#[sram]`.
  - Kopf- und Gesamtprüfsumme werden berechnet.
  - Ausgabe zusätzlich als `.sym` im Format `BB:AAAA name` (RGBDS, BGB, Emulicious) und als `.elf` mit
    DWARF ohne Code, wie beim NES.
- **Kopieren statt teilen:**
  - Die SM83-Tabellen gibt es zweimal: im Emulator (CPU, Disassembler) und im Compiler (Kodierer). Die
    Repos bleiben entkoppelt.
  - Der Aufbau des Compilers wird aus compiler-nes kopiert und angepasst. compiler-nes selbst bleibt
    unangetastet.
- **Test-ROMs:** Sie liegen außerhalb des Repos und werden über `KUPO_GB_TESTROMS` gefunden. Spiele für
  Rauchtests kommen über `KUPO_GB_GAMES`. Golden-Hashes lassen sich mit `KUPO_GB_GOLDEN_NEU` neu
  schreiben, wie beim N64. Bekannte Abweichungen stehen in der Liste `GB_OFFEN`.
- **`.gitignore`:** `*.gb`, `*.gbc` und `*.sgb` kommen in den **gemeinsamen Teil aller Repositories**,
  gleichzeitig und gleichlautend.


## Phase 0 – Gerüst der beiden Repos
- Beide Repos bekommen:
  - `Cargo.toml` (Edition 2024, keine externen Crates) und eine versionierte `Cargo.lock`,
  - `src/lib.rs` und `src/main.rs`,
  - `.cargo/config.toml.example` mit `[patch]` auf die Nachbar-Ordner,
  - `.github/actions/vorbereiten/action.yml` byte-gleich aus den anderen Repos,
  - `.github/workflows/ci.yml` nach dem Muster von NES: fmt, clippy `-D warnings`, test.
- emulator-gbc hängt von `kupocode-emulator-core` ab, compiler-gbc von `kupocode-kupo` und
  `kupocode-compiler-core`. compiler-gbc nutzt emulator-gbc nur als dev-dependency mit `opt-level 2`.
- `docs/architektur.md` (erste Fassung) und `docs/plan.md` (dieser Plan bzw. der Auszug) kommen dazu.
  Die vorhandenen Verweise in `AGENTS.md` bleiben unverändert.
- `*.gb`, `*.gbc` und `*.sgb` kommen in den gemeinsamen Teil der `.gitignore` aller Repositories, in
  einem Commit je Repo.
- Prüfen, ob das Secret `KUPO_CI_TOKEN` in beiden Repos gesetzt ist, und `core.hooksPath` setzen.

## Phase 1 – Emulator: SM83-Kern
- `cpu/` mit `Cpu`, `trait CpuBus` (ein M-Zyklus entspricht genau einem Zugriff, wie beim NES) und
  `opcodes.rs`.
- Alle Befehle, auch die mit dem Präfix `CB`, mit genauen Zugriffen und internen Zyklen.
- Unterbrechungen samt Verzögerung von `EI`, Abbruch des Sprungs zur Unterbrechung über `IE`, `HALT`,
  HALT-Bug, `STOP` und die gesperrten Opcodes.
- `disasm.rs` (`decode`/`lines`) und `hook.rs` nach dem Muster von NES.
- Tests mit einem selbst assemblierten Programm über `tests/common` (`Asm`) und je Befehl.

## Phase 2 – Emulator: Bus, Speicher, Timer, Maschine
- `bus.rs` mit WRAM, HRAM, Echo-Bereich, OAM-Sperre und Bus-Konflikten während der OAM-DMA. Timer mit
  DIV und TIMA samt Flankenfehler, verzögertem Neuladen und Schreibkonflikten.
- Joypad über `P1` mit Unterbrechung, Seriell mit internem Takt (Ausgabe für Blargg), `IF`/`IE`.
- `gb.rs` mit `Gb`, das `Machine`, `DebugTarget` und `SaveState` implementiert, nach der Vorlage
  `kupocode-emulator-core/tests/machine.rs`.
- `Model` (Dmg, Cgb, Sgb, Sgb2). Der HLE-Start setzt Register und I/O wie nach dem Boot. Ein echtes
  Boot-ROM lässt sich über `with_boot_rom` laden, `FF50` blendet es aus.
- CLI `run`, `test` und `disasm` wie beim NES.

## Phase 3 – Emulator: Kassette I
- `cart/` mit Kopf-Parser (`CartridgeInfo`), `trait Mapper` und einer Datei je Mapper:
  - ROM-only,
  - MBC1 (auch Multicart MBC1M),
  - MBC2 mit 4-Bit-RAM,
  - MBC3 mit Echtzeituhr (Latch, Halt, Übertrag),
  - MBC5 mit Rumble.
- Batterie-RAM mit Uhr im 48-Byte-Format, `advance_rtc`.

## Phase 4 – Emulator: PPU (DMG)
- Modi 0 bis 3, `LY`/`LYC`/`STAT` samt Blockieren der STAT-Unterbrechung und STAT-Schreibfehler des DMG.
- Pixel-FIFO für Hintergrund, Fenster (interner Zeilenzähler) und Objekte (10 pro Zeile, Priorität nach
  X-Position).
- Variable Länge von Mode 3, Sperren von OAM und VRAM, Ein- und Ausschalten des LCD (erstes Bild leer).
- `views.rs` mit Kacheln, BG-Map, Fenster und OAM als `graphics_views()`.

## Phase 5 – Emulator: CGB
- `KEY1` mit Doppeltakt und Umschaltpause, VRAM-Bank 1, WRAM-Banken 1 bis 7.
- BG- und OBJ-Paletten mit Auto-Inkrement und Sperre in Mode 3, BG-Attribute, CGB-Priorität, `OPRI`.
- HDMA und GDMA mit Zeitverhalten.
- DMG-Kompatibilitätsmodus mit Palettenwahl wie das CGB-Boot-ROM über Titel-Prüfsumme und Tasten,
  `KEY0`, die undokumentierten Register `FF72` bis `FF77` und die Infrarot-Schnittstelle `RP` als Stub.
- Ansichten für Paletten und VRAM-Bank 1.

## Phase 6 – Emulator: APU
- Zwei Rechteckkanäle (Kanal 1 mit Sweep), Wellenkanal, Rauschkanal und Frame-Sequencer am DIV.
- Eigenheiten von Länge, Hüllkurve („Zombie-Modus“) und Sweep, Zugriff auf das Wave-RAM (beim DMG
  beschädigt), Verhalten beim Ein- und Ausschalten.
- CGB-Register `PCM12`/`PCM34`, Ausgabe mit 44,1 kHz.

## Phase 7 – Emulator: Super Game Boy
- Paketübertragung über `P1`, `MLT_REQ` mit bis zu 4 Spielern.
- Befehle `PAL01`–`PAL_SET`, `ATTR_BLK`/`LIN`/`DIV`/`CHR`/`SET`, `MASK_EN`, `PAL_TRN`, `ATTR_TRN`,
  `CHR_TRN` und `PCT_TRN` (Rahmen). Befehle für Sound und Daten werden ignoriert und kommen in
  `GB_OFFEN`.
- Die SNES-Seite wird per HLE nachgebildet. Die Ausgabe hat 256×224 Pixel mit Rahmen, der Takt richtet
  sich nach SGB1 bzw. SGB2.

## Phase 8 – Emulator: Linkkabel
- Seriell mit externem Takt und schnellem CGB-Takt.
- `GbLink` mit zwei Geräten im Gleichtakt, das Bild zeigt sie nebeneinander, Save-State über beide.
- Test: zwei selbst assemblierte Programme tauschen Bytes aus.

## Phase 9 – Emulator: weitere Mapper
- MBC6 mit Flash.
- MBC7 mit Beschleunigungssensor über `left_stick` und 93LC56-EEPROM.
- HuC1 mit Infrarot als Stub und HuC3 mit Uhr, Infrarot als Stub.
- MMM01.
- Pocket Camera: Sensor mit Testbild oder Bilddatei, Registerverhalten und Belichtung.

## Phase 10 – Emulator: Genauigkeit
`tests/testroms.rs` arbeitet mit `KUPO_GB_TESTROMS` und folgenden Prüfmitteln:
- Blargg über seriellen Text: `cpu_instrs`, `instr_timing`, `mem_timing`, `halt_bug`, `oam_bug`,
  `dmg_sound`, `cgb_sound`.
- Mooneye über Fibonacci-Register nach `LD B,B`: `acceptance` und `emulator-only` (MBC1/2/5).
- Golden-Hashes: `dmg-acid2`, `cgb-acid2`, Mealybug Tearoom, SameSuite, Gambatte-Tests, `age` und
  `rtc3test`.

Dazu eine `GB_OFFEN`-Liste und ein Rauchtest mit `KUPO_GB_GAMES` (`#[ignore]`). Eine
Geschwindigkeitsmessung (`#[ignore]`) soll mindestens 10-fache Echtzeit im Release zeigen.

## Phase 11 – KupoCode Emulator (emulator-gui)
- `Cargo.toml` bekommt die Abhängigkeit samt `opt-level 3`, `.cargo/config.toml.example` den `[patch]`.
- `src/platform.rs`:
  - Eintrag mit `id: "gb"` und den Endungen `gb`/`gbc`/`sgb`. Erkannt wird über die Kopf-Prüfsumme bei
    `0x14D`, nicht über das Logo.
  - `uses_bios` optional, `create_gb()`, `impl System for Gb` mit `battery_ram`/`load_battery_ram` und
    der Uhr.
- `src/settings.rs`: Boot-ROMs `bios_gb_dmg`, `bios_gb_cgb` und `bios_gb_sgb`, Modellwahl Automatisch,
  DMG, CGB oder SGB, Menüpunkt für den Linkbetrieb.
- `keymap.rs` und `i18n.rs` (Deutsch und Englisch), Test `gb_programme_laufen_mit_save_states`.
- `docs/architektur.md` anpassen; dort auch den fehlenden Eintrag für n64 nachtragen.

