# Roadmap

Offene Punkte, grob nach Nutzen geordnet. Erledigtes streichen, Neues mit
Begründung und Stelle im Code eintragen.

## Veröffentlichen

- [ ] **0.1.2 veröffentlichen.** Auf GitHub liegen nur die nie veröffentlichten
  Entwürfe v0.1.0 und v0.1.1 (Stand September); die Anmeldung, Deno, die
  UTF-8-Ausgabe und die Fehlerbehebungen vom November stecken in keinem
  Release. Die alten Entwürfe löschen.

## Anzeige

- [ ] **Fortschritt in Phasen.** Bei Video + Ton läuft der Balken zweimal von
  0 auf 100 %, Zusammenfügen und MP3-Umwandlung stehen minutenlang als
  „lädt 99 %“ da. Phase anzeigen (Video · Ton · Umwandeln).
  Dazu die Ausgabe maschinenlesbar anfordern (`--progress-template`,
  `--print before_dl:title`, `--print after_move:filepath`), statt Text zu
  zerlegen: Dann steht auch der Titel vor dem Download da, und die fertige
  Datei kommt sicher an. `core/src/queue.rs` (`parse_progress`,
  `parse_destination`).
- [ ] **„Nochmal“ bei Fehlern.** Die Anfrage liegt noch im Speicher
  (`Inner::requests`); heute muss man die Adresse neu einfügen.
- [ ] **Gleicher Titel, anderes Video.** Heute nur der Hinweis „war schon im
  Zielordner“. Als Einstellung die Video-ID in den Dateinamen nehmen
  (`%(title)s [%(id)s].%(ext)s`, `queue::build_args`).

## Funktionen

- [ ] **Playlists und Kanäle.** `--no-playlist` ist fest gesetzt. Braucht eine
  Abfrage vorab (Anzahl, Bestätigung) und einen Auftrag je Video.
- [ ] **Mehrere Adressen auf einmal** einfügen (eine je Zeile) oder per
  Drag & Drop ins Fenster ziehen.
- [ ] **Hinweis „neue Version verfügbar“** für die App selbst, über die
  Weiterleitung von `/releases/latest` des eigenen Repos (wie bei yt-dlp,
  `core::ytdlp::latest_tag`). Setzt veröffentlichte Releases voraus.
- [ ] **Systembenachrichtigung**, wenn ein Download fertig ist
  (`tauri-plugin-notification`).

## Technik

- [ ] **CI auch unter Windows** (und macOS). Windows ist die Hauptplattform;
  der Errno-22-Fehler und das Weiterladen nach dem Abbruch gab es nur dort.
  Der Test `proctree::beendet_auch_das_kind` prüft unter Windows das
  Job-Objekt, läuft aber bisher nur lokal.
- [ ] **`settings.json` atomar schreiben** (Zwischendatei, dann umbenennen),
  in `src-tauri/src/lib.rs` (`save_settings`) und `cli/src/settings.rs`. Ein
  Absturz mitten im Schreiben kostet sonst den Zielordner.
- [ ] **Reste beim Schließen.** Schließt man die App mitten im Download,
  beendet sie yt-dlp (`Manager::shutdown`), räumt `.part`-Dateien aber nicht
  weg – das geschieht nur beim Abbrechen.
- [ ] **Versionen über den Neustart merken** (optional). Die erste
  Werkzeugprüfung nach dem Start dauert unter Windows rund 2 s, weil yt-dlp
  und Deno je einmal starten. Mit gespeicherten Versionen (je Datei mit Größe
  und Änderungszeit) entfiele auch das; `tools::probe_version`.
- [ ] **macOS: Abbrechen nachprüfen.** Dort beendet `proctree` die Nachkommen
  aus der Prozesstabelle; gebaut und mit Clippy geprüft, aber nicht auf einem
  Mac gelaufen.
