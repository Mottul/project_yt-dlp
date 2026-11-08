---
name: oberflaeche-pruefen
description: Die Oberfläche des MottulVideoLoader ansehen und prüfen – im Browser mit Beispieldaten (Demo-Modus, alle Zustände und Phasen) oder im echten Tauri-Fenster. Verwenden nach jeder Änderung an src/ (App.tsx, styles.css, api.ts) oder an Feldern von Job/ToolsStatus, und wenn die App gestartet oder angesehen werden soll.
---

# Oberfläche prüfen

Zwei Wege. Der erste reicht für fast alles, was man sieht; der zweite für das, was nur im
Fenster geht.

## 1. Im Browser mit Beispieldaten (Demo-Modus)

Läuft die Oberfläche **nicht** in Tauri und **nicht** gebaut (`npm run dev`), nimmt `src/api.ts`
statt des Kerns `src/demo.ts`. Im Paket fällt der Demo-Teil beim Bündeln weg
(prüfen: `npm run build` und `grep -l Naturdoku dist/assets/*.js` findet nichts).

Starten über `.claude/launch.json` (Konfiguration `oberflaeche`, Port 5183) mit
`preview_start`, oder selbst mit `npm run dev`.

Was zu sehen ist:

- „Prüfe Werkzeuge…“ für gut eine Sekunde (verzögerte Statusantwort), dann die Werkzeuge.
- Hinweis „Neue Version 0.1.4 … verfügbar“ unter dem Werkzeugstatus.
- Je ein Auftrag in jedem Zustand: lädt mit „Video 1/2“, „Abfragen…“ (nur die Adresse,
  pulsierender Balken), „Zusammenfügen…“ (pulsierend), wartet, fertig, fertig mit Hinweis „war
  schon da“, Fehler, abgebrochen. Fehler und abgebrochen tragen „Nochmal“.
- Adresse eingeben und Enter (oder „Nochmal“): Der Auftrag läuft alle Phasen durch (Abfragen →
  Video 1/2 → Ton 2/2 → Zusammenfügen → fertig, alle 0,6 s ein Schritt). Mit Format „Nur Ton“:
  Ton → Umwandeln.
- Mehrere Zeilen ins Adressfeld (per `form_input`, Umschalt+Enter von Hand): Enter stellt je
  Webadresse einen Auftrag ein, Text drumherum und Doppelte fallen weg. Die Demo zerlegt
  vereinfacht; maßgeblich ist `split_addresses` im Kern (Tests dort).

Drag & Drop ohne Maus nachstellen – echte `DragEvent`s mit `DataTransfer`:

```js
const dt = new DataTransfer(); dt.setData('text/uri-list', '# Kommentar\r\nhttps://x.example/1')
document.body.dispatchEvent(new DragEvent('dragover', { dataTransfer: dt, bubbles: true, cancelable: true }))
// jetzt .drop („Loslassen zum Laden“) im DOM; dann 'drop' mit demselben dt -> neuer Auftrag.
// Eine Datei (dt.items.add(new File(...))): dropEffect 'none', kein Overlay, nichts geladen.
// Overlay wieder weg: new DragEvent('dragleave', { bubbles: true, relatedTarget: null })
```

Prüfen:

- **Zustand lesen statt raten:** per JavaScript, nicht nur per Bildschirmfoto:
  ```js
  [...document.querySelectorAll('.job')].map(j => j.querySelector('.job-title').textContent
    + ' | ' + j.querySelector('.tag').textContent + ' | ' + j.querySelector('.job-foot span').textContent)
  ```
- **Klicken über `find`/`ref`,** nicht über Koordinaten: Die Seite scrollt, und ein Klick auf
  alte Koordinaten trifft dann etwas anderes (so passiert: Text landete nicht im Adressfeld).
- Bildschirmfotos für das Aussehen: Farben nur über die Variablen in `styles.css`.
- **Bildschirmfoto zeigt einen alten Stand,** wenn der Browser-Bereich ausgeblendet ist
  (`tabs_context` sagt es): Die Seite läuft dann im Hintergrund und zeichnet nicht neu – so
  fehlte das Drop-Overlay auf dem Bild, obwohl es im DOM stand. Dann per JavaScript prüfen
  (`getComputedStyle`) oder den Nutzer bitten, den Bereich einzublenden.

Grenzen: Es wird nichts geladen. „Wählen“ (Ordnerdialog), „Im Ordner zeigen“ und „Zur
Release-Seite“ brauchen die Tauri-Erweiterungen und tun im Browser nichts. Was das Fenster
öffnen darf, steht in `src-tauri/capabilities/default.json` – eine neue Aktion dort eingrenzen.

**Neue Felder oder Zustände** im Kern (`Job`, `ToolsStatus`, `Phase`): in `src/api.ts` eintragen
und in `src/demo.ts` mitziehen – `base` bekommt das Feld (sonst meldet `tsc` den Fehler), ein
neuer Zustand bekommt einen eigenen Beispielauftrag. Texte, die der Kern formuliert
(`Phase::label`), dort gleichlautend übernehmen.

## 2. Im echten Fenster

```bash
npm run app:dev      # Tauri-Fenster mit Hot Reload; Ausgaben des Rust-Teils im Terminal
```

Nötig für alles, was über `invoke` und Ereignisse läuft: echte Downloads, Ordnerdialog, Start-
verhalten (Werkzeugprüfung im Hintergrund, Fenster bleibt bedienbar), Systembenachrichtigung
(nur wenn das Fenster NICHT vorn liegt), Links aus einem echten Browser ins Fenster ziehen. Ein
Bildschirmfoto des Fensters gibt es hier nur mit Desktop-Steuerung; sonst den Nutzer bitten,
hinzusehen, und sagen, worauf (z. B. „Phase wechselt von Video 1/2 zu Ton 2/2“).

Nur prüfen, dass die App startet (neues Plugin, geänderte `tauri.conf.json`): bei laufendem
`npm run dev` die Debug-Datei starten, nach einigen Sekunden schauen und gezielt diesen
Prozess beenden:

```powershell
cargo build -p mottul-video-loader   # cargo test baut nur die Bibliothek, nicht die .exe
$p = Start-Process target\debug\mottul-video-loader.exe -PassThru -RedirectStandardError <scratchpad>\err.txt
Start-Sleep 8; $p.Refresh(); "$(-not $p.HasExited) $($p.MainWindowTitle)"; Stop-Process -Id $p.Id -Force
```

Das Verhalten **hinter** der Oberfläche (was der Kern meldet) ohne Fenster prüfen: Wegwerf-
Starter über den Kern mit lokalem Testserver, siehe Skill `fehler-nachstellen` (3, 3a).
