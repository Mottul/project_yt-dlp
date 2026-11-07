# CLAUDE.md

MottulVideoLoader: schlanke Tauri-App als Oberfläche für yt-dlp.
UI-Sprache und Code-Kommentare sind Deutsch.

## Kommandos

```bash
npm run app:dev        # Fenster mit Hot Reload
npm run build          # Oberfläche: tsc --noEmit + vite build
npm run app:build      # Paket für das aktuelle System

cargo run -p mottul-video-cli   # Terminal-Variante
cargo test   --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt    --all
```

Ein einzelner Rust-Test: `cargo test --workspace parse_progress`
Tests mit Netzzugang: `cargo test --workspace -- --ignored`
(z. B. `holt_deno_und_prueft_es`: lädt, prüft, entpackt und startet Deno)

Fremde Fehler mit `yt-dlp -v` nachstellen, mit den Werkzeugen der App aus
`<App-Daten>/bin` (z. B. `--cookies-from-browser firefox --js-runtimes
deno:<bin>/deno.exe --simulate -- <adresse>`): Die Zeile `JS runtimes:` zeigt,
ob eine Laufzeit gefunden wurde.

Die CI (`.github/workflows/ci.yml`) nimmt das jeweils neueste stabile Rust: Ein
neues rustfmt kann alte Stellen anders umbrechen und `cargo fmt --check` rot
machen, ohne dass sich etwas geändert hat. Dann `cargo fmt --all` laufen lassen.

## Architektur

- **Ein Kern, zwei Oberflächen.** `core/` macht die Arbeit und kennt weder
  Fenster noch Terminal; `src-tauri/` ist die Fenster-App, `cli/` die
  Terminal-Variante für macOS vor 11.3. Neue Logik gehört in den Kern, nicht
  in eine Oberfläche — sonst hat die andere sie nicht.
- **Oberflächen hängen sich über `queue::JobSink` ein** und bekommen darüber
  jede Änderung an einem Auftrag gemeldet.
- **Rust ist autoritativ.** Die Oberfläche zeigt nur an und schickt Befehle.
- **Ein Weg in den Kern:** `src/api.ts` bündelt alle `invoke`-Aufrufe und die
  beiden Ereignisse (`job-update`, `tools-status`). Komponenten rufen niemals
  `invoke` direkt auf.
- **Neuer Befehl** = Funktion mit `#[tauri::command]` + Eintrag in
  `generate_handler!` (lib.rs) + Methode in `src/api.ts`.
- **`core::paths`** bestimmt Datenverzeichnis und Zielordner. Beide
  Oberflächen benutzen dieselbe `settings.json` und dieselben Werkzeuge.
- **Werkzeuge** liegen unter `<App-Daten>/bin`. Alles, was dort ausführbar
  landet, geht durch `tools::download_verified` — Prüfsumme vor dem Ablegen,
  dann atomares Umbenennen. Diesen Weg nicht umgehen.
- **yt-dlp** wird bei jedem Start geprüft (neueste stabile Version),
  **ffmpeg** und **Deno** sind auf eine feste Version genagelt (Prüfsummen im
  Programm) und werden nur einmal geladen. Begründung steht im Kopf der
  jeweiligen Datei.
- **Deno** ist die JavaScript-Laufzeit für yt-dlp: YouTube stellt Prüfaufgaben,
  die yt-dlp seit 2025.11 nur in JavaScript löst. Ohne sie kam trotz Cookies
  „age-restricted“. yt-dlp bekommt den Pfad über `--js-runtimes deno:<Pfad>`
  (`core::deno::runtime_args`). Archive nur über `tools::unzip_entry`, erst nach
  `check_sha256` des ganzen Archivs.
- **Anmeldung** nur über `core::auth::CookieBrowser` (feste Liste, nie ein
  freier Text an `--cookies-from-browser`). Fehlertexte von yt-dlp laufen durch
  `auth::explain_error`; die Ursache steht oft in einer Warnung VOR der letzten
  Fehlerzeile, deshalb das ganze Ende von stderr prüfen.
- **Adresse immer nach `--`** (`queue::build_args`): eine Eingabe, die mit „-“
  beginnt, darf nie als Option gelten.
- **Einstellungen** (`Settings` in `src-tauri` und `cli`, eine gemeinsame Datei)
  tragen `#[serde(default)]` am Struct: Ein neues Feld darf eine ältere Datei nie
  verwerfen (sonst ist der Zielordner weg). Neue Felder in BEIDEN Structs
  eintragen – wer speichert, schreibt die ganze Datei. Felder mit fester Liste
  nachsichtig lesen (`auth::lenient_browser`).
- **Version ermitteln** über die Weiterleitung von `/releases/latest`, nie über
  die GitHub-API (ohne Anmeldung 60 Anfragen je Stunde und IP).

## Konventionen

- Deutsch für Oberflächentexte und Kommentare; Kommentare erklären das WARUM.
- Rust: `cargo fmt`, Clippy warnungsfrei. Reine Hilfsfunktionen (Parser,
  Prüfungen) bleiben frei von Seiteneffekten und bekommen Tests.
- TypeScript: `strict`, keine ungenutzten Bindungen (`noUnusedLocals`).
- Farben nur über die CSS-Variablen in `src/styles.css` (Gold-Marke).
