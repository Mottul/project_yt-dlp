# Roadmap

Offene Punkte, grob nach Nutzen geordnet. Erledigtes streichen, Neues mit
Begründung und Stelle im Code eintragen.

## Veröffentlichen

- [ ] **Alte Entwürfe löschen.** v0.1.0 und v0.1.1 wurden nie veröffentlicht
  (Stand September) und sind durch v0.1.2 überholt. Nur nach Rückfrage –
  Löschen ist endgültig.
- [ ] **0.1.3** mit der Phasenanzeige und den Skills (Skill `veroeffentlichen`).

## Anzeige

- [ ] **Titel schon während der Abfrage.** Der Titel kommt mit der ersten
  Fortschrittszeile; davor (yt-dlp fragt die Seite ab, einige Sekunden) steht
  die Adresse. `--print` scheidet aus (schaltet die übrigen Meldungen ab);
  vielleicht `--print-to-file` – erst prüfen, ob es das auch tut.
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
  `core::ytdlp::latest_tag`). Seit v0.1.2 zeigt `/releases/latest` auf ein
  echtes Release.
- [ ] **Systembenachrichtigung**, wenn ein Download fertig ist
  (`tauri-plugin-notification`).

## Technik

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
- [ ] **macOS: Abbrechen mit echtem yt-dlp nachprüfen.** Dort beendet
  `proctree` die Nachkommen aus der Prozesstabelle. Der Mechanismus läuft in
  der CI auf macOS (`proctree::beendet_auch_das_kind`, Eltern mit Kind), mit
  dem echten `yt-dlp_macos` auf einem Mac noch nicht.
