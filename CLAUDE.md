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
  **ffmpeg** ist auf eine feste Version genagelt und wird nur einmal geladen.
  Begründung steht im Kopf der jeweiligen Datei.
- **Version ermitteln** über die Weiterleitung von `/releases/latest`, nie über
  die GitHub-API (ohne Anmeldung 60 Anfragen je Stunde und IP).

## Konventionen

- Deutsch für Oberflächentexte und Kommentare; Kommentare erklären das WARUM.
- Rust: `cargo fmt`, Clippy warnungsfrei. Reine Hilfsfunktionen (Parser,
  Prüfungen) bleiben frei von Seiteneffekten und bekommen Tests.
- TypeScript: `strict`, keine ungenutzten Bindungen (`noUnusedLocals`).
- Farben nur über die CSS-Variablen in `src/styles.css` (Gold-Marke).
