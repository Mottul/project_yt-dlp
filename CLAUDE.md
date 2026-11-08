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
ob eine Laufzeit gefunden wurde. Der ganze Ablauf (Rohbytes der Pipe, Download
über den Kern per Wegwerf-Starter, Gegenprobe, bekannte Fehlerbilder) steht im
Skill `.claude/skills/fehler-nachstellen/`.

Offene Punkte stehen in `ROADMAP.md`: Erledigtes streichen, Neues mit Grund
und Stelle eintragen.

Wiederkehrende Abläufe als Projekt-Skills in `.claude/skills/`. Jede Änderung
zieht sie mit: neue Fehlerbilder, Muster, Lehren.

- `fehler-nachstellen` – Fehler mit den Werkzeugen der App nachstellen, lokaler
  Testserver (auch DASH mit getrenntem Bild und Ton), Strg+C in der
  Terminal-Variante, Gegenprobe
- `oberflaeche-pruefen` – Oberfläche im Browser mit Beispieldaten (`src/demo.ts`,
  greift nur bei `npm run dev` außerhalb von Tauri), Drag & Drop nachstellen,
  oder im echten Fenster
- `veroeffentlichen` – Version anheben, Release-Workflow, Entwurf prüfen, Notizen,
  veröffentlichen

Die CI (`.github/workflows/ci.yml`) prüft Rust auf Linux, Windows und macOS,
die Oberfläche nur auf Linux. Sie nimmt das jeweils neueste stabile Rust: Ein
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
- **yt-dlp immer als ganzen Prozessbaum beenden** (`proctree::ProcessTree`):
  `yt-dlp.exe` (PyInstaller) startet sich ein zweites Mal, und erst dieser
  Prozess lädt und startet ffmpeg. Wer nur den ersten beendet, lässt den
  Download weiterlaufen; der Platz in der Warteschlange bleibt belegt, weil die
  Pipe offen ist. Windows: Job-Objekt (beendet auch bei Absturz der App), Unix:
  Nachkommen aus `ps`. Nach dem Abbruch räumt `queue::leftovers` die Dateien
  weg, die yt-dlp in diesem Lauf gemeldet hat.
- **Nichts Blockierendes auf dem Hauptfaden:** Tauri führt Befehle OHNE `async`
  auf dem Hauptfaden aus. Was Programme startet (Versionsabfragen,
  `core::read_status`), läuft über `spawn_blocking` – sonst friert das Fenster
  ein (so beim Start, bis 8.11.2026: rund 4 s). `tools::probe_version` merkt
  sich Versionen je Datei (Größe + Änderungszeit); ein Aufruf kostet unter
  Windows bis zu 2 s.
- **Ausgabe von yt-dlp** immer mit `--encoding utf-8` und über
  `queue::read_lines` lesen (Bytes, ungültiges UTF-8 ersetzt, liest bis zum
  Ende). `lines()` endet bei der ersten ungültigen Zeile; dann ist die Pipe zu,
  und yt-dlp scheitert unter Windows mit „Unable to download video: [Errno 22]
  Invalid argument“ – ohne `--encoding` schreibt es in cp1252 (ARTE-Titel mit
  geschütztem Leerzeichen, 7.11.2026).
- **Fortschritt** über `queue::PROGRESS_TEMPLATES` (`--progress-template`:
  Rohwerte, Tab-getrennt, Codecs zeigen Bild/Ton) und `queue::LineState`, nicht
  über die Anzeige für Menschen – die wechselt Felder je nach Lage. `LineState`
  ist rein und wird mit echten, aufgezeichneten Zeilen getestet; neue Zeilenarten
  ebenso. Texte der Phasen formuliert der Kern (`Phase::label`), nicht die
  Oberflächen. Die gewohnte Zeile `[download] x%` bleibt als Rückfall. Bis zum
  ersten Fortschritt gilt `Phase::Prepare` („Abfragen…“, gesetzt in
  `schedule`): Titel und Größe stehen erst nach der Abfrage der Seite fest,
  früher geht es nicht.
- **Adressen zerlegt der Kern** (`queue::split_addresses`), für Fenster
  (`enqueue` nimmt Text mit mehreren Adressen) und Terminal. Das Adressfeld ist
  ein `textarea`: Ein einzeiliges `input` entfernt beim Einfügen die
  Zeilenumbrüche und klebt die Adressen zusammen.
- **Drag & Drop** über HTML5, dafür `dragDropEnabled: false` im Fenster (sonst
  fängt Tauri unter Windows jedes Ziehen ab). Jedes Ziehen mit
  `preventDefault` abfangen: Eine fallen gelassene Datei öffnete das Fenster
  sonst an Stelle der App.
- **Benachrichtigung** nur aus Rust (`WindowSink` in `src-tauri`), die
  Oberfläche bekommt dafür keine Berechtigung. Eine Meldung, wenn nichts mehr
  wartet oder läuft und das Fenster nicht vorn liegt – nicht eine je Download.
- **Einstellungen** (`Settings` in `src-tauri` und `cli`, eine gemeinsame Datei)
  tragen `#[serde(default)]` am Struct: Ein neues Feld darf eine ältere Datei nie
  verwerfen (sonst ist der Zielordner weg). Neue Felder in BEIDEN Structs
  eintragen – wer speichert, schreibt die ganze Datei. Felder mit fester Liste
  nachsichtig lesen (`auth::lenient_browser`). Gespeichert wird nur über
  `paths::write_atomic` (daneben schreiben, dann umbenennen).
- **Neu starten und Schließen** (`Manager::retry`, `Manager::shutdown`): Ein Lauf
  gilt erst als beendet, wenn auch sein Aufräumen durch ist (`Inner::running`).
  Vorher darf derselbe Auftrag nicht neu starten – das Aufräumen löschte sonst
  die `.part`-Datei des neuen Laufs. Die Terminal-Variante ruft `shutdown` bei
  Strg+C (`stop_on_ctrl_c`); danach zeichnet ihr Hauptfaden nichts mehr
  (`STOPPING`), das Programm endet mit Code 130.
- **Hinweis auf eine neue App-Version** (`core::update`): über die Weiterleitung
  von `/releases/latest`, streng geprüft, still ohne Netz. Nur Hinweis mit
  Verweis, kein Selbst-Update (Pakete unsigniert). Die Fenster-App darf nur die
  eigene Release-Seite öffnen (`opener:allow-open-url`, eingegrenzt in
  `capabilities/default.json`).
- **Version ermitteln** über die Weiterleitung von `/releases/latest`, nie über
  die GitHub-API (ohne Anmeldung 60 Anfragen je Stunde und IP).
- **Eigene Version** nur in `Cargo.toml` (`[workspace.package]`). Tauri liest
  sie von dort, der Release-Workflow leitet den Tag daraus ab und bricht ab,
  wenn es den Release schon gibt. Keine `version` in `tauri.conf.json` oder
  `package.json` eintragen – zwei Stellen laufen auseinander (0.1.1 stand im
  Lockfile noch als 0.1.0).

## Konventionen

- Deutsch für Oberflächentexte und Kommentare; Kommentare erklären das WARUM.
- Rust: `cargo fmt`, Clippy warnungsfrei. Reine Hilfsfunktionen (Parser,
  Prüfungen) bleiben frei von Seiteneffekten und bekommen Tests.
- TypeScript: `strict`, keine ungenutzten Bindungen (`noUnusedLocals`).
- Farben nur über die CSS-Variablen in `src/styles.css` (Gold-Marke).
