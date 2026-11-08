---
name: fehler-nachstellen
description: Einen Fehler aus dem MottulVideoLoader nachstellen und eingrenzen – yt-dlp mit den Werkzeugen der App ausführlich laufen lassen, Rohbytes der Pipe prüfen, einen Download über den Kern per Wegwerf-Starter, lokaler Testserver, Strg+C in der Terminal-Variante, Gegenprobe gegen den alten Stand. Verwenden, wenn eine Fehlermeldung aus dem VideoLoader gemeldet wird, ein Video nicht lädt oder Abbrechen/Aufräumen/Phasen echt geprüft werden sollen.
---

# Fehler nachstellen

Ein gemeldeter Fehler gilt erst als verstanden, wenn er sich mit den Werkzeugen der App
nachstellen lässt, und erst als behoben, wenn die Gegenprobe ihn mit dem alten Stand zeigt und
mit dem Fix nicht mehr.

## 1. yt-dlp ausführlich, mit den Werkzeugen der App

Nie ein yt-dlp vom System nehmen: `<App-Daten>/bin` (Windows
`%APPDATA%\com.mottul.videoloader\bin`) enthält `yt-dlp`, `ffmpeg`, `ffprobe`, `deno`.

```bash
B="$APPDATA/com.mottul.videoloader/bin"
"$B/yt-dlp.exe" -v --simulate --no-playlist --cookies-from-browser firefox \
  --js-runtimes "deno:$B/deno.exe" -- "<adresse>"
```

Lesen:

- `JS runtimes: none` → keine Laufzeit: „age-restricted“ trotz Cookies, fehlende Formate,
  „Signature/n challenge solving failed“.
- `Found YouTube account cookies` → Cookies kommen an. „requiring account age-verification“ →
  das Konto ist nicht altersbestätigt (mit Laufzeit lädt es oft trotzdem).
- Die Ursache steht oft in einer **Warnung vor** der letzten Fehlerzeile.

## 2. Rohbytes der Pipe

Der Kern startet yt-dlp ohne Konsole (`CREATE_NO_WINDOW`) und liest Pipes. So nachstellen, wie es
ankommt (Python mit `-I`, im Scratchpad):

```python
import subprocess
p = subprocess.run([b + "/yt-dlp.exe", "--skip-download", "--print", "title", "--", url],
                   capture_output=True, creationflags=0x08000000)
print(p.stdout)            # Rohbytes
p.stdout.decode("utf-8")   # wirft, wenn es kein UTF-8 ist
```

Ohne `--encoding utf-8` schreibt yt-dlp unter Windows cp1252 („ä“ = `0xE4`, geschütztes
Leerzeichen = `0xA0`).

## 3. Download über den Kern (Wegwerf-Starter)

Gleiche Argumente, Pipes und Fehlertexte wie in der App, ohne die Einstellungen des Nutzers
anzufassen. Im Scratchpad ein Paket `kern-probe` anlegen:

```toml
[package]
name = "kern-probe"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
mottul-video-core = { path = "<repo>/core" }

[workspace]
```

```rust
use std::sync::Arc;
use std::time::Duration;

use mottul_video_core::queue::{EnqueueRequest, Job, JobSink, JobStatus, Manager};
use mottul_video_core::{paths, tools, CookieBrowser, Format};

struct Print;
impl JobSink for Print {
    fn on_update(&self, job: &Job) {
        if job.status != JobStatus::Running || job.error.is_some() {
            eprintln!("[{:?}] {:?} {:?}", job.status, job.title, job.error);
        }
    }
}

fn main() {
    let out = std::env::args().nth(1).expect("Zielordner");
    let url = std::env::args().nth(2).expect("Adresse");
    let m = Manager::new(Arc::new(Print)).unwrap();
    let req = EnqueueRequest {
        url,
        format: Format::Video,
        max_height: None,
        output_dir: out,
        cookies: Some(CookieBrowser::Firefox),
    };
    let id = m.enqueue(req, tools::bin_dir(&paths::data_dir()));
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let job = m.list().into_iter().find(|j| j.id == id).unwrap();
        // Fortschritt genügt als Beleg -- nie das ganze Video laden
        if job.progress >= 0.02 && job.status == JobStatus::Running {
            println!("FORTSCHRITT {:.1} % -> {:?}", job.progress * 100.0, job.output_file);
            m.cancel(&id);
            std::thread::sleep(Duration::from_secs(2));
            break;
        }
        if matches!(job.status, JobStatus::Done | JobStatus::Error | JobStatus::Canceled) {
            println!("ENDE {:?}: {:?}", job.status, job.error);
            break;
        }
    }
}
```

Starten mit `cargo run -q --target-dir <repo>/target -- <zielordner im Scratchpad> <adresse>`
(`--target-dir` spart den Neubau der Abhängigkeiten).

## 3a. Ohne fremde Seite: lokaler Testserver

Für Abbruch, Reste und „schon vorhanden“ braucht es kein echtes Video. Testdatei mit dem ffmpeg
der App, dazu ein kleiner Node-Server im Scratchpad:

```bash
"$B/ffmpeg.exe" -f lavfi -i testsrc=duration=2:size=320x240:rate=25 -f lavfi -i sine=duration=2 \
  -shortest -c:v libx264 -c:a aac www/probe.mp4
```

Der Server (`<scratchpad>/server.js`, daneben der Ordner `www`; im Hintergrund starten und am
Ende über die Hintergrundaufgabe beenden – in der Prozessliste heißt er nur `node server.js`):

```js
const http = require('http'), fs = require('fs'), path = require('path')
const root = path.join(__dirname, 'www')
const types = { '.mpd': 'application/dash+xml', '.m4s': 'video/iso.segment', '.mp4': 'video/mp4' }
http.createServer((req, res) => {
  const url = decodeURIComponent(req.url.split('?')[0])
  if (url === '/slow.mp4') {
    res.writeHead(200, { 'Content-Type': 'video/mp4', 'Content-Length': 50 * 1024 * 1024 })
    const t = setInterval(() => res.write(Buffer.alloc(4096)), 100)
    return req.on('close', () => clearInterval(t))
  }
  const file = path.join(root, url)
  if (!file.startsWith(root) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
    res.writeHead(404); return res.end()
  }
  res.writeHead(200, { 'Content-Type': types[path.extname(file)] ?? 'application/octet-stream' })
  fs.createReadStream(file).pipe(res)
}).listen(8767, '127.0.0.1')
```

- `/probe.mp4` liefert die Datei → zweimal laden zeigt „has already been downloaded“.
- `/slow.mp4` meldet 50 MB und schreibt alle 100 ms 4 KB → yt-dlp steckt mitten im Download.
- **Bild und Ton getrennt wie bei YouTube** (Phasen, Zusammenfügen): daraus einen DASH-Strom
  machen und `/dash/manifest.mpd` mit `Content-Type: application/dash+xml` ausliefern –
  yt-dlp wählt dann `0+1`, lädt beide nacheinander und fügt sie zusammen:
  ```bash
  "$B/ffmpeg.exe" -i www/clip.mp4 -map 0:v -map 0:a -c copy -f dash -seg_duration 2 www/dash/manifest.mpd
  ```
  Mit `-x --audio-format mp3` auf `/clip.mp4` kommt die Phase „Umwandeln“.

Die Rohzeilen der Fortschrittsvorlage (`queue::PROGRESS_TEMPLATES`) direkt ansehen: yt-dlp mit
denselben `--progress-template`-Argumenten starten und `| cat -A` anhängen – Tabulatoren
erscheinen als `^I`. Felder ohne Wert heißen `NA`.

Abbruch prüfen im Starter: Fortschritt > 0 abwarten, `m.cancel(&id)`, dann `m.busy()` beobachten.
**Der Platz wird erst frei, wenn kein Prozess die Pipe mehr hält.** Bleibt `busy()` wahr, läuft
ein yt-dlp weiter. Ebenso prüfbar: `m.retry(&id, bin)` direkt nach dem Abbruch muss `false`
geben (der alte Lauf räumt noch auf), danach `true`; `m.shutdown(Duration::from_secs(3))`
mitten im Download hinterlässt weder Prozesse noch Dateien.

Den Starter mit **synchronem** `main` schreiben: Wird die Warteschlange (`Manager`, eigener
tokio-Unterbau) in einem `#[tokio::main]` abgeräumt, bricht tokio am Ende mit „Cannot drop a
runtime in a context where blocking is not allowed“ ab. Für eine async-Abfrage (z. B.
`update::check()`) `m.handle().block_on(…)` nehmen. Die App und die Terminal-Variante legen die
Warteschlange außerhalb von async an. Prozessbaum ansehen:

```powershell
Get-CimInstance Win32_Process -Filter "Name='yt-dlp.exe'" | Select ProcessId, ParentProcessId
```

Während des Downloads sind es **zwei** Einträge (Entpacker → ladender Prozess). Nach dem Abbruch
darf keiner übrig sein, und im Zielordner keine `.part`-Datei.

Phasen mit Zeitstempel: Ein `JobSink`, der jede geänderte Zeile `status | phase_label |
progress | title` mit `start.elapsed()` druckt. Ein DASH-Lauf zeigt dann (8.11.2026, lokal)
2,2 s „Abfragen…“, dann Video 1/2 → Ton 2/2 → Zusammenfügen… → Done.

## 3b. Strg+C in der Terminal-Variante

Strg+C lässt sich unter Windows nicht aus der Bash schicken. Die Terminal-Variante in einer
**eigenen, versteckten Konsole** starten und das Signal von einem Hilfsprozess senden, der sich
an diese Konsole hängt (`<scratchpad>/ctrlc.ps1`):

```powershell
param([uint32]$TargetPid)
Add-Type @'
using System; using System.Runtime.InteropServices;
public static class Ctrl {
  [DllImport("kernel32.dll")] public static extern bool FreeConsole();
  [DllImport("kernel32.dll")] public static extern bool AttachConsole(uint pid);
  [DllImport("kernel32.dll")] public static extern bool SetConsoleCtrlHandler(IntPtr h, bool add);
  [DllImport("kernel32.dll")] public static extern bool GenerateConsoleCtrlEvent(uint ev, uint group);
}
'@
[void][Ctrl]::FreeConsole(); [void][Ctrl]::AttachConsole($TargetPid)
[void][Ctrl]::SetConsoleCtrlHandler([IntPtr]::Zero, $true); [void][Ctrl]::GenerateConsoleCtrlEvent(0, 0)
```

Ablauf (PowerShell):

1. Eigener Datenordner, damit die Variante nicht in den echten Zielordner lädt:
   `<scratchpad>\fakeappdata\com.mottul.videoloader\` mit `bin` als **Junction** auf den echten
   Werkzeugordner und einer `settings.json` (`outputDir` im Scratchpad, `autoUpdate: false`).
   **Ohne BOM schreiben:** `[IO.File]::WriteAllText(..., New-Object Text.UTF8Encoding($false))`.
   `Out-File -Encoding utf8` setzt in PowerShell 5.1 ein BOM, serde verwirft die Datei, und es
   gilt wieder der echte Downloads-Ordner.
2. `$env:APPDATA` kurz umbiegen, `Start-Process <exe> -ArgumentList <adresse> -PassThru
   -WindowStyle Hidden` (ShellExecute = neue Konsole; **nicht** mit `-Redirect…`, sonst erbt sie
   die Konsole der Shell, und das Signal träfe die Shell mit), sofort zurücksetzen.
3. Warten, bis die `.part`-Datei da ist; yt-dlp im Baum notieren (Kinder und Enkel der PID).
4. `Start-Process powershell -ArgumentList '-NoProfile','-File','ctrlc.ps1','-TargetPid',<pid>
   -WindowStyle Hidden -Wait`.

Erwartet: Code 130, keine `.part`, kein yt-dlp aus dem Baum. Die alte Fassung (vor 0.1.4)
endete mit `-1073741510` (STATUS_CONTROL_C_EXIT) und ließ `slow.mp4.part` liegen.
**Aufräumen: die Junction zuerst einzeln entfernen** (`cmd /c rmdir <...>\bin`), erst dann den
Ordner löschen – ein rekursives Löschen durch die Junction träfe die echten Werkzeuge.

## 4. Gegenprobe

Bei vielen offenen Änderungen statt `git stash` nur die eine Datei tauschen:

```bash
cp core/src/<datei>.rs <scratchpad>/neu.rs
git show HEAD:core/src/<datei>.rs > core/src/<datei>.rs   # alter Stand: Fehler muss kommen
# Starter neu bauen und laufen lassen
cp <scratchpad>/neu.rs core/src/<datei>.rs                # mit Fix
```

Danach Zielordner und `.part`-Dateien löschen. **Nur eigene Prozesse beenden**, gezielt per PID
(Waise aus der Liste oben, Testserver anhand von `CommandLine` und `CreationDate`), nie alle
`yt-dlp.exe` – der Nutzer kann gerade selbst laden.

## Fallen

- **`APPDATA` umbiegen** (eigener Datenordner für die Terminal-Variante) lässt yt-dlp das
  Firefox-Profil nicht mehr finden (`%APPDATA%\Mozilla`) → „keine Cookies gefunden“. Für
  Cookie-Tests den Wegwerf-Starter nehmen, für „Werkzeuge fehlen“ den umgebogenen Ordner.
- Die Terminal-Variante mit Adresse lädt in den Zielordner aus der **echten** `settings.json` –
  nicht zum Testen.
- Ein heruntergeladenes Programm (Deno) nur mit Zustimmung des Nutzers starten; der Netz-Test
  `holt_deno_und_prueft_es` tut genau das.

## Bekannte Fehlerbilder

| Meldung | Ursache | Stelle |
| --- | --- | --- |
| `Unable to download video: [Errno 22] Invalid argument`, ohne Fortschritt | Pipe zu: Leser hörte bei ungültigem UTF-8 auf (cp1252) | `queue::read_lines`, `--encoding utf-8` |
| `Sign in to confirm your age` | keine Cookies | `auth::CookieBrowser`, Auswahl „Anmeldung“ |
| `Sorry, this content is age-restricted` trotz Cookies | keine JavaScript-Laufzeit, sonst Konto nicht altersbestätigt | `core::deno` |
| `could not find firefox cookies database` | falsches/umgebogenes Profil, Browser nie benutzt | `auth::explain_error` |
| `Failed to decrypt with DPAPI` | Chrome/Edge unter Windows | Firefox nehmen |
| „Abbrechen“ wirkt nicht: `.part` wächst weiter, nächster Auftrag startet nicht | nur der erste der beiden yt-dlp-Prozesse beendet (PyInstaller) | `proctree::ProcessTree` |
| „fertig“, aber alte Datei; kein „Im Ordner zeigen“ | `has already been downloaded` (gleicher Titel) | `queue::parse_existing`, Hinweis `NOTE_EXISTING` |
| Fenster beim Start Sekunden eingefroren | Versionsabfragen auf dem Hauptfaden (Befehl ohne `async`) | `snapshot` in `src-tauri`, `tools::probe_version` |

Ein neues Fehlerbild: in `auth::explain_error` aufnehmen (Test mit der echten Zeile) und hier
eintragen.
