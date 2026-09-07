# MottulVideoLoader

Schlanke Desktop-App für [yt-dlp](https://github.com/yt-dlp/yt-dlp): Adresse
einfügen, Format wählen, laden. Gebaut mit [Tauri](https://tauri.app) — die
Anwendung selbst wiegt rund 10 MB, yt-dlp und ffmpeg holt sie sich beim ersten
Start selbst.

Für Anwender: [`docs/INSTALL.md`](docs/INSTALL.md) beschreibt Installation und
ersten Start unter Windows und macOS.

## Was die App macht

- Warteschlange mit zwei gleichzeitigen Downloads, Fortschritt, Tempo,
  Restzeit, Abbrechen
- Video als MP4 (Auflösung deckelbar) oder nur Ton als MP3/M4A
- **yt-dlp bleibt aktuell:** beim Start wird die neueste stabile Version
  ermittelt und bei Abweichung ersetzt (abschaltbar)
- **ffmpeg** wird einmalig nachgeladen und danach nicht mehr angefasst

## Woher die Werkzeuge kommen

| Werkzeug | Quelle | Prüfung |
| --- | --- | --- |
| yt-dlp | offizielles Release, jeweils die neueste stabile Version | `SHA2-256SUMS` des Releases |
| ffmpeg, ffprobe | `eugeneware/ffmpeg-static`, feste Version `b6.1.1` | SHA-256 im Programm hinterlegt |

Beides landet unter `<App-Daten>/bin`. Was dort ausführbar abgelegt wird, wurde
vorher gegen eine SHA-256-Prüfsumme geprüft; erst danach wird die Datei an ihren
Platz umbenannt — ein Abbruch hinterlässt nie eine halbe, startbare Datei.

Das ersetzt **keine** Signaturprüfung: es sichert die Übertragung, nicht die
Herkunft des Releases. Dafür bräuchte es den GPG-Schlüssel des jeweiligen
Projekts.

Warum yt-dlp mitwächst und ffmpeg nicht: yt-dlp bricht, sobald eine Plattform
ihre Seiten umbaut, und muss deshalb aktuell bleiben. Zusammenfügen und
Umwandeln ändern sich nicht — eine feste ffmpeg-Version erlaubt dafür die
stärkere Prüfung, weil die erwartete Prüfsumme im Programm steht und nicht auf
demselben Server liegt wie die Datei.

Die Version anheben: `TAG` in `src-tauri/src/ffmpeg.rs` ändern und die
Prüfsummen der **entpackten** Dateien eintragen
(`curl -sL <asset>.gz | gunzip | sha256sum`).

## Entwicklung

```bash
npm install
npm run app:dev        # Fenster mit Hot Reload
npm run app:build      # Paket für das aktuelle System (dist unter src-tauri/target)

npm run build                                   # Oberfläche: Typen + Bundle
cargo test --manifest-path src-tauri/Cargo.toml # Rust-Tests
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path src-tauri/Cargo.toml
```

Voraussetzungen: Node ≥ 22, Rust (stable). Unter Linux zusätzlich
`libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`.

## Aufbau

```
src/                  Oberfläche (React + TypeScript, eigenes CSS)
  api.ts              typisierte Brücke zum Rust-Teil
  App.tsx             die eine Seite
src-tauri/src/
  lib.rs              Zustand, Befehle, Startprüfung
  tools.rs            Laden, Prüfsummen, atomares Ablegen
  ytdlp.rs            Version ermitteln und aktuell halten
  ffmpeg.rs           feste Version, hinterlegte Prüfsummen
  queue.rs            Warteschlange, Fortschritts-Auswertung
```

Die Oberfläche ruft nichts direkt auf, sondern geht immer über `src/api.ts`.

## Veröffentlichen

Actions → **Release** → *Run workflow* → Version eintragen (z. B. `v0.1.0`).
Der Workflow baut die Installer für Windows und macOS und hängt sie an einen
**Release-Entwurf**; der Tag entsteht, sobald der Entwurf veröffentlicht wird.
Ein bereits vorhandener Tag `v*` löst denselben Lauf aus. Ohne Versionsangabe
wird nur gebaut, die Ergebnisse liegen dann sieben Tage als Artefakt am Lauf.

Das macOS-Paket ist universal — es läuft auf Intel und Apple Silicon. Bauzeit
je System rund drei bis fünf Minuten.

Die Pakete sind nicht signiert; Kosten wären ein Zertifikat je Plattform. Die
Warnungen, die Windows und macOS deshalb zeigen, sind in der Anleitung erklärt.

## Rechtliches

Die App ist eine Oberfläche für yt-dlp und lädt nur, was ihr aufgetragen wird.
Ob ein Download zulässig ist, richtet sich nach den Nutzungsbedingungen der
jeweiligen Plattform und dem geltenden Recht — das entscheidet der Anwender.
