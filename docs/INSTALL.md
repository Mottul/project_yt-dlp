# Installation unter Windows und macOS

Kurzanleitung für Anwender.

Die Dateien hängen am Release unter
[Releases](https://github.com/Mottul/project_yt-dlp/releases) — je Version
genau eine je System:

| System | Datei |
| --- | --- |
| Windows 10/11 (x64) | die `…-setup.exe` |
| macOS 11.3+ (Intel und Apple Silicon) | die `…universal.dmg` |

**Vorab nichts installieren.** yt-dlp und ffmpeg holt die App beim ersten Start
selbst — weder Python noch eines der beiden Programme muss von Hand eingerichtet
werden.

---

## Windows

1. `…-setup.exe` ausführen.
2. SmartScreen meldet einen unbekannten Herausgeber → **Weitere Informationen**
   → **Trotzdem ausführen**. Die Pakete sind nicht signiert (siehe unten).
3. Die Installation läuft ohne Administratorrechte für den angemeldeten Benutzer.
4. Deinstallation über *Einstellungen → Apps → MottulVideoLoader*.

Einstellungen und geladene Werkzeuge liegen unter
`%APPDATA%\com.mottul.videoloader`.

## macOS

> **Mindestens macOS 11.3 (Big Sur).** Das ist keine Einstellung, sondern eine
> Grenze des verwendeten Fenster-Unterbaus: er meldet eine WebKit-Methode an,
> die es erst ab 11.3 gibt. Auf älteren Systemen startet die App nicht.

1. Die `.dmg` öffnen, **MottulVideoLoader** in den Ordner *Programme* ziehen.
2. Beim **ersten** Start nicht doppelklicken, sondern mit gedrückter ctrl-Taste
   klicken (bzw. Rechtsklick) → **Öffnen** → im Dialog nochmals **Öffnen**.
   Gatekeeper weist einen Doppelklick sonst ab, weil die App nicht signiert und
   notarisiert ist.
   - Alternativ nach dem abgewiesenen Versuch: *Systemeinstellungen → Datenschutz
     und Sicherheit → „Dennoch öffnen"*.
   - Meldet macOS „… ist beschädigt und kann nicht geöffnet werden", liegt das am
     Quarantäne-Merkmal des Downloads:
     `xattr -dr com.apple.quarantine /Applications/MottulVideoLoader.app`
3. Ab dem zweiten Start genügt ein Doppelklick.

Das DMG enthält ein Universal-Paket und läuft auf Intel wie auf Apple Silicon.

Einstellungen und geladene Werkzeuge liegen unter
`~/Library/Application Support/com.mottul.videoloader`.

## Ältere Macs (10.13 bis 11.2)

Die Fenster-App braucht macOS 11.3. Für ältere Systeme liegt am Release
**`MottulVideoLoader-Terminal-macos.zip`** — dasselbe Programm ohne Fenster:

1. Entpacken, Doppelklick auf **`MottulVideoLoader.command`**.
2. Beim ersten Mal meldet macOS „Entwickler nicht verifiziert": Rechtsklick auf
   die Datei → **Öffnen** → im Dialog nochmals **Öffnen**.
   Bei „ist beschädigt" einmalig im Terminal:
   `xattr -dr com.apple.quarantine <entpackter Ordner>`
3. Adresse einfügen, Enter. Einstellungen über einen Buchstaben: `f` Format,
   `a` Auflösung, `o` Zielordner (Ordner ins Fenster ziehen genügt). Leere
   Eingabe beendet.

Voraussetzung ist macOS 10.13 — so weit hinunter reichen auch yt-dlp und
ffmpeg, die das Programm nachlädt.

---

## Erster Start

Die App richtet sich selbst ein und lädt dafür:

| | Windows | macOS (Apple Silicon) | macOS (Intel) |
| --- | --- | --- | --- |
| yt-dlp | ~18 MB | ~37 MB | ~37 MB |
| ffmpeg + ffprobe | ~59 MB | ~38 MB | ~51 MB |

Beides wird vor dem Ablegen gegen eine SHA-256-Prüfsumme geprüft. Ohne Internet
startet die App normal — der Hinweis steht dann oben im Fenster, und der Knopf
**Jetzt einrichten** wiederholt den Versuch.

Danach wird bei jedem Start nur noch geprüft, ob eine neue **yt-dlp**-Version
vorliegt (wenige Kilobyte). Das lässt sich unten im Fenster abschalten.
ffmpeg bleibt, wie es ist.

Hinter Firewall oder Proxy müssen `github.com` und
`objects.githubusercontent.com` erreichbar sein.

## Bedienung

1. Zielordner wählen (bleibt gespeichert).
2. Adresse einfügen, Format und höchste Auflösung wählen, **Laden**.
3. Zwei Downloads laufen gleichzeitig, der Rest wartet. Fertige Dateien lassen
   sich über **Im Ordner zeigen** öffnen.

## Aktualisieren

Neue Version installieren: unter Windows den neuen Installer ausführen, unter
macOS die App aus dem neuen DMG über die alte in *Programme* kopieren.
Einstellungen und geladene Werkzeuge bleiben erhalten.

## Warum die Sicherheitswarnungen?

Die Pakete tragen keine Code-Signatur — für den privaten Gebrauch ist ein
Zertifikat (jährliche Kosten bei Microsoft bzw. Apple) nicht vorgesehen. Die
Warnungen beider Systeme besagen genau das und nichts über den Inhalt der App.
