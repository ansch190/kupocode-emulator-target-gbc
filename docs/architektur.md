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
  lib.rs          Crate-Wurzel, Re-Exporte
  main.rs         CLI `kupocode-emulator-gbc`
```

## Invarianten und Fallstricke

- **Keine Abbilder im Repository:** Spiele, Test-ROMs und Boot-ROMs bleiben außerhalb
  (siehe `.gitignore`). Eigene Testprogramme werden im Test assembliert.
- **Kein Nintendo-Code:** Der Start läuft per HLE; ein echtes Boot-ROM kommt nur aus einer vom
  Anwender angegebenen Datei.
- **Deterministisch:** Zeit ist nur der emulierte Takt, auch für die Echtzeituhr der Kassette;
  gleiche Eingabe ergibt gleiche Bilder.
- **Optimiert auch im Debug-Build:** `Cargo.toml` setzt `opt-level = 2` für Abhängigkeiten
  und Tests, sonst laufen Testprogramme zu langsam.

## Tests

- Unit-Tests in den Modulen.
