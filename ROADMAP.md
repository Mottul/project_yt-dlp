# Roadmap

Offene Punkte, grob nach Nutzen geordnet. Erledigtes streichen, Neues mit
Begründung und Stelle im Code eintragen.

## Veröffentlichen

- [ ] **Update-Hinweis im echten Fenster sehen.** 0.1.4 ist erschienen
  (8.11.2026), `/releases/latest` leitet darauf weiter. Jetzt in einer
  installierten 0.1.3 prüfen: Hinweis „Neue Version 0.1.4“ erscheint nach der
  Startprüfung, „Zur Release-Seite“ öffnet den Browser (die Berechtigung
  `opener:allow-open-url` ist eingegrenzt und lief bisher nur im Bau, nicht im
  Fenster).
- [ ] **Benachrichtigung und Drag & Drop im echten Fenster sehen.** Geprüft
  sind die Logik (Tests in `src-tauri/src/lib.rs`), der Start der App mit
  dem Plugin und die Drag-Ereignisse im Browser – nicht die Meldung selbst
  und nicht ein echter Link aus Firefox/Chrome ins Fenster
  (`dragDropEnabled: false` in `tauri.conf.json`). Unter Windows erscheint die
  Meldung im Entwicklungsmodus unter „Windows PowerShell“, erst installiert
  unter dem Namen der App.

## Anzeige

- [ ] **Gleicher Titel, anderes Video.** Heute nur der Hinweis „war schon im
  Zielordner“. Als Einstellung die Video-ID in den Dateinamen nehmen
  (`%(title)s [%(id)s].%(ext)s`, `queue::build_args`).

## Funktionen

- [ ] **Playlists und Kanäle.** `--no-playlist` ist fest gesetzt. Braucht eine
  Abfrage vorab (Anzahl, Bestätigung) und einen Auftrag je Video.
- [ ] **Mehrere Zeilen in der Terminal-Variante.** Eingefügte Zeilen liest sie
  einzeln: Die erste startet, die nächste erst, wenn diese durch ist. Mehrere
  Adressen in EINER Zeile (durch Leerzeichen getrennt) laufen schon
  gleichzeitig (`cli/src/main.rs`, `prompt`).
- [ ] **„Nochmal“ in der Terminal-Variante.** Der Kern kann es
  (`Manager::retry`); die Terminal-Variante hat bisher keine Bedienung für
  einzelne Aufträge.

## Technik

- [ ] **Versionen über den Neustart merken** (optional). Die erste
  Werkzeugprüfung nach dem Start dauert unter Windows rund 2 s, weil yt-dlp
  und Deno je einmal starten. Mit gespeicherten Versionen (je Datei mit Größe
  und Änderungszeit) entfiele auch das; `tools::probe_version`.
- [ ] **macOS: Abbrechen mit echtem yt-dlp nachprüfen.** Dort beendet
  `proctree` die Nachkommen aus der Prozesstabelle. Der Mechanismus läuft in
  der CI auf macOS (`proctree::beendet_auch_das_kind`, Eltern mit Kind), mit
  dem echten `yt-dlp_macos` auf einem Mac noch nicht.
