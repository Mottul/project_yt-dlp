# CLAUDE.md

MottulVideoLoader: schlanke Tauri-App als Oberfläche für yt-dlp.
UI-Sprache und Code-Kommentare sind Deutsch.

## Kommandos

```bash
npm run app:dev        # Fenster mit Hot Reload
npm run build          # Oberfläche: tsc --noEmit + vite build
npm run app:build      # Paket für das aktuelle System

cargo test   --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt    --manifest-path src-tauri/Cargo.toml
```

Ein einzelner Rust-Test: `cargo test --manifest-path src-tauri/Cargo.toml parse_progress`

## Architektur

- **Rust ist autoritativ.** Werkzeugverwaltung, Warteschlange und Prozessstart
  liegen in `src-tauri/src`; die Oberfläche zeigt nur an und schickt Befehle.
- **Ein Weg in den Kern:** `src/api.ts` bündelt alle `invoke`-Aufrufe und die
  beiden Ereignisse (`job-update`, `tools-status`). Komponenten rufen niemals
  `invoke` direkt auf.
- **Neuer Befehl** = Funktion mit `#[tauri::command]` + Eintrag in
  `generate_handler!` (lib.rs) + Methode in `src/api.ts`.
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
