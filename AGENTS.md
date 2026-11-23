# AGENTS.md – `kupocode-emulator-gbc`

## Projektübersicht
`kupocode-emulator-gbc` ist der Emulator für den **GameBoy & GameBoy Color**.

## Vorgaben für alle Kupo-Repositories
Diese Vorgaben gelten gleichlautend in allen Repositories des Kupo Environment.

- **Sprache:** Commit-Messages, Dokumentation und Code-Kommentare (`///`, `//!`, `//`) immer auf **Deutsch**.
- **100 % Rust:** Der gesamte Code ist in Rust geschrieben (Edition 2024).
- **Lizenz:** Alle Repositories haben dieselbe `LICENSE.md` (PolyForm Noncommercial 1.0.0). Sie wird nur in allen Repositories gleichzeitig und gleichlautend geändert.
- **Repository-Konfiguration:** `.gitignore`, `.gitattributes` und `.editorconfig` bestehen aus einem gemeinsamen Teil, der in allen Repositories gleich ist und nur überall gleichzeitig geändert wird, und darunter dem Abschnitt „Repository-eigene Regeln“ für Abweichungen dieses Repositorys. Der Pre-commit-Hook `.githooks/pre-commit`, `rust-toolchain.toml` und die CI-Vorbereitung `.github/actions/vorbereiten/action.yml` sind überall identisch; den Hook schaltet man je Klon mit `git config core.hooksPath .githooks` ein.
- **Dokumentation:** liegt in `docs/`. Unter `docs/kupo/` liegen die **KupoDocs**: projektunabhängige Dokumentation, die für sich steht und nicht auf den Code dieses Repositorys verweist.
- **Weiterentwicklung:** Neue Dokumentation und Pläne werden als eigene Dateien unter `docs/` des jeweiligen Repositorys angelegt. `AGENTS.md` enthält dazu nur Verweise auf diese Dateien, keine Inhalte.
- **Orientierung für Agenten:** `docs/architektur.md` beschreibt Aufbau, zentrale Typen, Invarianten und Tests, aber weder Funktionsumfang noch Projektstand. Ändert sich die Struktur (neues oder verschobenes Modul, neuer zentraler Typ, neue Sondertests), wird die Datei im selben Commit angepasst. Repositories ohne Code haben die Datei noch nicht; sie entsteht mit dem ersten Code.
- **`CLAUDE.md`:** enthält nur den Verweis `@AGENTS.md`. Alle Anweisungen für Agenten stehen in `AGENTS.md`.
- **`README.md`:** nur Titel, eine ultrakurze Beschreibung, die bei der Weiterentwicklung nicht angepasst werden muss, und der Verweis auf `LICENSE.md`. Keine Beispiele, keine Funktionslisten, kein Code, kein Lizenztext.

## Projektspezifische Konventionen
- **Aufbau und Tests:** siehe [docs/architektur.md](docs/architektur.md).
- **Plan der Unterstützung:** siehe [docs/plan.md](docs/plan.md).
- **Fehlermeldungen** auf Deutsch.
- **Keine externen Crates.**
