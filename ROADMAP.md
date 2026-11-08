# Roadmap

Offene Punkte, grob nach Nutzen geordnet. Erledigtes streichen, Neues mit
Begründung und Stelle im Code eintragen.

## Veröffentlichen

- [ ] **0.1.3 veröffentlichen** (Version steht schon in Cargo.toml): Phasen-
  anzeige, „Nochmal“, Hinweis auf neue Versionen, sicheres Speichern, Aufräumen
  beim Schließen. Ablauf im Skill `veroeffentlichen`. Der Update-Hinweis greift
  erstmals beim Schritt von 0.1.3 auf die übernächste Version – 0.1.2 kennt ihn
  noch nicht.

## Anzeige

- [ ] **Titel schon während der Abfrage.** Der Titel kommt mit der ersten
  Fortschrittszeile; davor (yt-dlp fragt die Seite ab, einige Sekunden) steht
  die Adresse. `--print` scheidet aus (schaltet die übrigen Meldungen ab);
  vielleicht `--print-to-file` – erst prüfen, ob es das auch tut.
- [ ] **Gleicher Titel, anderes Video.** Heute nur der Hinweis „war schon im
  Zielordner“. Als Einstellung die Video-ID in den Dateinamen nehmen
  (`%(title)s [%(id)s].%(ext)s`, `queue::build_args`).

## Funktionen

- [ ] **Playlists und Kanäle.** `--no-playlist` ist fest gesetzt. Braucht eine
  Abfrage vorab (Anzahl, Bestätigung) und einen Auftrag je Video.
- [ ] **Mehrere Adressen auf einmal** einfügen (eine je Zeile) oder per
  Drag & Drop ins Fenster ziehen.
- [ ] **Systembenachrichtigung**, wenn ein Download fertig ist
  (`tauri-plugin-notification`).
- [ ] **„Nochmal“ in der Terminal-Variante.** Der Kern kann es
  (`Manager::retry`); die Terminal-Variante hat bisher keine Bedienung für
  einzelne Aufträge.

## Technik

- [ ] **Strg+C in der Terminal-Variante** räumt nicht auf: Unter Windows beendet
  das Job-Objekt yt-dlp, die `.part`-Dateien bleiben. Ein Handler, der
  `Manager::shutdown` ruft, hätte das Fenster-Verhalten.
- [ ] **Versionen über den Neustart merken** (optional). Die erste
  Werkzeugprüfung nach dem Start dauert unter Windows rund 2 s, weil yt-dlp
  und Deno je einmal starten. Mit gespeicherten Versionen (je Datei mit Größe
  und Änderungszeit) entfiele auch das; `tools::probe_version`.
- [ ] **macOS: Abbrechen mit echtem yt-dlp nachprüfen.** Dort beendet
  `proctree` die Nachkommen aus der Prozesstabelle. Der Mechanismus läuft in
  der CI auf macOS (`proctree::beendet_auch_das_kind`, Eltern mit Kind), mit
  dem echten `yt-dlp_macos` auf einem Mac noch nicht.
