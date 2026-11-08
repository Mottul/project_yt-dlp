//! Warteschlange der Downloads: startet yt-dlp, liest den Fortschritt Zeile
//! fuer Zeile mit und meldet jede Aenderung an die Oberflaeche.
//!
//! Die Oberflaeche haengt sich ueber [`JobSink`] ein -- der Kern weiss nicht,
//! ob daraus ein Fortschrittsbalken im Fenster oder eine Zeile im Terminal
//! wird.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::oneshot;

use crate::auth::{explain_error, CookieBrowser};
use crate::proctree::ProcessTree;

/// Nimmt jede Aenderung an einem Auftrag entgegen. Wird aus Arbeitsfaeden
/// aufgerufen, muss also selbst fuer Ordnung sorgen.
pub trait JobSink: Send + Sync + 'static {
    fn on_update(&self, job: &Job);
}

/// Verwirft alle Meldungen -- fuer Tests und einfache Faelle.
pub struct SilentSink;

impl JobSink for SilentSink {
    fn on_update(&self, _job: &Job) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Video,
    AudioMp3,
    AudioM4a,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnqueueRequest {
    pub url: String,
    pub format: Format,
    /// Hoehenbegrenzung fuer Video, None = beste verfuegbare.
    pub max_height: Option<u32>,
    pub output_dir: String,
    /// Anmeldung: Cookies aus diesem Browser, None = ohne.
    #[serde(default)]
    pub cookies: Option<CookieBrowser>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    Error,
    Canceled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub url: String,
    pub format: Format,
    pub status: JobStatus,
    /// 0..1
    pub progress: f64,
    pub title: Option<String>,
    pub speed: Option<String>,
    pub eta: Option<String>,
    pub output_dir: String,
    pub output_file: Option<String>,
    pub error: Option<String>,
    /// Hinweis ohne Fehler, z. B. dass die Datei schon da war.
    pub note: Option<String>,
    pub created_at: u64,
}

/// Hinweis, wenn yt-dlp nichts laedt, weil die Datei schon im Zielordner liegt.
/// Bei gleichem Titel kann das auch ein ANDERES Video gewesen sein.
pub const NOTE_EXISTING: &str =
    "War schon im Zielordner – nicht erneut geladen. Gleicher Titel, aber anderes Video? \
     Vorhandene Datei umbenennen und erneut laden.";

/// Aus einer Fortschrittszeile gelesene Werte.
#[derive(Debug, Default, PartialEq)]
pub struct Progress {
    pub percent: Option<f64>,
    pub speed: Option<String>,
    pub eta: Option<String>,
}

/// Liest "[download]  12.3% of 45.6MiB at 1.23MiB/s ETA 00:42".
pub fn parse_progress(line: &str) -> Option<Progress> {
    let rest = line.trim().strip_prefix("[download]")?.trim_start();
    let percent_end = rest.find('%')?;
    let percent: f64 = rest[..percent_end].trim().parse().ok()?;

    let mut out = Progress {
        percent: Some((percent / 100.0).clamp(0.0, 0.999)),
        ..Default::default()
    };
    if let Some(at) = rest.find(" at ") {
        let after = rest[at + 4..].trim_start();
        let speed: String = after
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        if !speed.is_empty() && speed != "Unknown" {
            out.speed = Some(speed);
        }
    }
    if let Some(eta) = rest.find("ETA ") {
        let value: String = rest[eta + 4..]
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_string();
        if !value.is_empty() && value != "Unknown" {
            out.eta = Some(value);
        }
    }
    Some(out)
}

/// Liest die Zieldatei aus den Meldungen von yt-dlp.
pub fn parse_destination(line: &str) -> Option<String> {
    let line = line.trim();
    for prefix in ["[download] Destination:", "[ExtractAudio] Destination:"] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(rest.trim().to_string());
        }
    }
    if let Some(rest) = line.strip_prefix("[Merger] Merging formats into ") {
        return Some(rest.trim().trim_matches('"').to_string());
    }
    None
}

/// Liest „[download] <datei> has already been downloaded“: yt-dlp laedt nicht,
/// weil die Datei (oder bei Ton die umgewandelte) schon daliegt.
pub fn parse_existing(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("[download] ")?;
    let end = rest.find(" has already been downloaded")?;
    let path = rest[..end].trim();
    (!path.is_empty()).then(|| path.to_string())
}

/// Was ein abgebrochener Download zu einer gemeldeten Zieldatei hinterlaesst.
/// `names` sind die Dateinamen in ihrem Ordner; zurueck kommen die zu loeschenden.
///
/// Die Datei selbst gehoert dazu: yt-dlp meldet ein Ziel nur, wenn es die
/// Datei in DIESEM Lauf schreibt -- liegt sie schon da, heisst es „has already
/// been downloaded“ und es gibt keine Zielmeldung. Dazu `.part`, `.ytdl`,
/// `.part-Frag<n>` (halb geladen) und `<name>.temp.<endung>` (halb
/// zusammengefuegt oder umgewandelt).
pub fn leftovers(dest_name: &str, names: &[String]) -> Vec<String> {
    let temp = match dest_name.rfind('.') {
        Some(i) if i > 0 => format!("{}.temp{}", &dest_name[..i], &dest_name[i..]),
        _ => format!("{dest_name}.temp"),
    };
    let part = format!("{dest_name}.part");
    let ytdl = format!("{dest_name}.ytdl");
    let frag = format!("{dest_name}.part-Frag");
    names
        .iter()
        .filter(|n| {
            *n == dest_name || **n == part || **n == ytdl || **n == temp || n.starts_with(&frag)
        })
        .cloned()
        .collect()
}

/// Loescht die Reste eines abgebrochenen Auftrags -- nur im eigenen Zielordner.
async fn remove_leftovers(destinations: &[String], output_dir: &str) {
    let mut pending = Vec::new();
    for dest in destinations {
        let dest = Path::new(dest);
        let (Some(dir), Some(name)) = (dest.parent(), dest.file_name()) else {
            continue;
        };
        if dir != Path::new(output_dir) {
            continue;
        }
        let names: Vec<String> = std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        for left in leftovers(&name.to_string_lossy(), &names) {
            let path = dir.join(left);
            if !pending.contains(&path) {
                pending.push(path);
            }
        }
    }
    // Unter Windows haelt ein gerade beendeter Prozess seine Dateien noch
    // einen Moment offen.
    for _ in 0..10 {
        pending.retain(|p| std::fs::remove_file(p).is_err() && p.exists());
        if pending.is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}

/// Eine Zeile aus der Pipe: Zeilenende weg, ungueltiges UTF-8 ersetzt statt
/// verworfen.
pub fn decode_line(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    text.trim_end_matches(['\r', '\n']).to_string()
}

/// Liest Zeilen, bis der Strom endet, und gibt jede weiter.
///
/// Bewusst Bytes statt `lines()`: Das endet bei der ersten Zeile, die kein
/// gueltiges UTF-8 ist. Dann ist die Pipe zu, und yt-dlp scheitert unter
/// Windows beim naechsten Schreiben mit „Unable to download video: [Errno 22]
/// Invalid argument“ -- so bei einem ARTE-Titel mit geschuetztem Leerzeichen
/// (0xA0 in cp1252), noch vor dem ersten Fortschritt.
pub async fn read_lines<R, F>(reader: R, mut on_line: F)
where
    R: AsyncRead + Unpin,
    F: FnMut(String),
{
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => on_line(decode_line(&buf)),
        }
    }
}

/// Dateiname ohne Endung -- dient als Titel in der Liste.
pub fn title_from_path(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}

/// Baut die Aufrufparameter fuer yt-dlp. `deno`: die eigene JavaScript-Laufzeit,
/// wenn sie daliegt (siehe [`crate::deno`]).
pub fn build_args(req: &EnqueueRequest, bin_dir: &Path, deno: Option<&Path>) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--newline".into(),
        "--no-playlist".into(),
        // Unter Windows schreibt yt-dlp in eine Pipe sonst in der
        // Systemkodierung (cp1252) -- „ä“ oder ein geschuetztes Leerzeichen
        // kaemen als einzelnes Byte an, das kein UTF-8 ist.
        "--encoding".into(),
        "utf-8".into(),
        "--ffmpeg-location".into(),
        bin_dir.to_string_lossy().into_owned(),
    ];
    match req.format {
        Format::Video => {
            let selector = match req.max_height {
                Some(h) => format!("bv*[height<={h}]+ba/b[height<={h}]/b"),
                None => "bv*+ba/b".to_string(),
            };
            args.push("-f".into());
            args.push(selector);
            args.push("--merge-output-format".into());
            args.push("mp4".into());
        }
        Format::AudioMp3 | Format::AudioM4a => {
            args.push("-x".into());
            args.push("--audio-format".into());
            args.push(if req.format == Format::AudioMp3 {
                "mp3".into()
            } else {
                "m4a".into()
            });
        }
    }
    if let Some(browser) = req.cookies {
        args.push("--cookies-from-browser".into());
        args.push(browser.id().into());
    }
    args.extend(crate::deno::runtime_args(deno));
    args.push("-o".into());
    args.push(
        Path::new(&req.output_dir)
            .join("%(title)s.%(ext)s")
            .to_string_lossy()
            .into_owned(),
    );
    // Die Adresse nach `--`: Eine Eingabe, die mit „-“ beginnt, darf nie als
    // Option gelesen werden.
    args.push("--".into());
    args.push(req.url.clone());
    args
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Default)]
struct Inner {
    jobs: Vec<Job>,
    requests: HashMap<String, EnqueueRequest>,
    cancels: HashMap<String, oneshot::Sender<()>>,
    /// yt-dlp samt Unterprozessen je laufendem Auftrag (siehe [`ProcessTree`]).
    trees: HashMap<String, Arc<ProcessTree>>,
    active: usize,
}

#[derive(Clone)]
pub struct Manager {
    inner: Arc<Mutex<Inner>>,
    sink: Arc<dyn JobSink>,
    /// Eigener Laufzeitunterbau: die Warteschlange laeuft unabhaengig davon,
    /// ob die Oberflaeche gerade in einem async-Zusammenhang steckt.
    runtime: Arc<tokio::runtime::Runtime>,
    concurrency: usize,
}

impl Manager {
    pub fn new(sink: Arc<dyn JobSink>) -> std::io::Result<Self> {
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            sink,
            runtime: Arc::new(
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?,
            ),
            concurrency: 2,
        })
    }

    /// Zugriff auf den Laufzeitunterbau, damit die Oberflaeche eigene
    /// async-Arbeiten (etwa die Werkzeugpruefung) darauf ausfuehren kann.
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }

    pub fn list(&self) -> Vec<Job> {
        self.inner.lock().unwrap().jobs.clone()
    }

    /// Laeuft gerade ein Auftrag? Dann die Werkzeuge nicht ersetzen.
    pub fn busy(&self) -> bool {
        self.inner.lock().unwrap().active > 0
    }

    fn patch<F: FnOnce(&mut Job)>(&self, id: &str, f: F) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(job) = guard.jobs.iter_mut().find(|j| j.id == id) {
            f(job);
            let copy = job.clone();
            drop(guard);
            self.sink.on_update(&copy);
        }
    }

    pub fn enqueue(&self, req: EnqueueRequest, bin_dir: PathBuf) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let job = Job {
            id: id.clone(),
            url: req.url.clone(),
            format: req.format,
            status: JobStatus::Queued,
            progress: 0.0,
            title: None,
            speed: None,
            eta: None,
            output_dir: req.output_dir.clone(),
            output_file: None,
            error: None,
            note: None,
            created_at: now_ms(),
        };
        {
            let mut guard = self.inner.lock().unwrap();
            guard.jobs.insert(0, job.clone());
            guard.requests.insert(id.clone(), req);
        }
        self.sink.on_update(&job);
        self.schedule(bin_dir);
        id
    }

    /// Startet wartende Auftraege, solange Plaetze frei sind.
    fn schedule(&self, bin_dir: PathBuf) {
        loop {
            let next = {
                let mut guard = self.inner.lock().unwrap();
                if guard.active >= self.concurrency {
                    return;
                }
                let Some(job) = guard
                    .jobs
                    .iter_mut()
                    .rev()
                    .find(|j| j.status == JobStatus::Queued)
                else {
                    return;
                };
                job.status = JobStatus::Running;
                let id = job.id.clone();
                let copy = job.clone();
                guard.active += 1;
                let req = guard.requests.get(&id).cloned();
                drop(guard);
                self.sink.on_update(&copy);
                req.map(|r| (id, r))
            };
            let Some((id, req)) = next else { return };

            let manager = self.clone();
            let dir = bin_dir.clone();
            self.runtime.spawn(async move {
                manager.run(&id, req, &dir).await;
                {
                    let mut guard = manager.inner.lock().unwrap();
                    guard.active = guard.active.saturating_sub(1);
                }
                manager.schedule(dir);
            });
        }
    }

    async fn run(&self, id: &str, req: EnqueueRequest, bin_dir: &Path) {
        let Some(launcher) = crate::ytdlp::resolve_launcher(bin_dir) else {
            self.patch(id, |job| {
                job.status = JobStatus::Error;
                job.error = Some("yt-dlp fehlt — bitte zuerst die Werkzeuge einrichten.".into());
            });
            return;
        };

        let deno_file = crate::deno::deno_path(bin_dir);
        let deno = deno_file.exists().then_some(deno_file.as_path());
        let mut cmd = tokio::process::Command::new(launcher.program());
        cmd.args(launcher.prefix())
            .args(build_args(&req, bin_dir, deno))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // tokio::process::Command bringt creation_flags unter Windows selbst mit.
        #[cfg(windows)]
        cmd.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(err) => {
                self.patch(id, |job| {
                    job.status = JobStatus::Error;
                    job.error = Some(format!("Start fehlgeschlagen: {err}"));
                });
                return;
            }
        };

        let tree = ProcessTree::attach(&child).map(Arc::new);
        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        {
            let mut guard = self.inner.lock().unwrap();
            // Kam der Abbruch, waehrend yt-dlp startete, gab es fuer cancel()
            // noch nichts zu beenden. Dann den Sender fallen lassen: Der
            // Empfaenger meldet sich sofort, und unten wird beendet.
            let canceled = guard
                .jobs
                .iter()
                .any(|j| j.id == id && j.status == JobStatus::Canceled);
            if !canceled {
                guard.cancels.insert(id.to_string(), cancel_tx);
                if let Some(tree) = &tree {
                    guard.trees.insert(id.to_string(), tree.clone());
                }
            }
        }

        let stdout = child.stdout.take().expect("stdout ist gesetzt");
        let stderr = child.stderr.take().expect("stderr ist gesetzt");

        // Beide Ausgaben lesen eigene Aufgaben: Lesen ist nicht abbruchsicher,
        // in einem select! koennte also mitten in einer Zeile abgebrochen und
        // der Rest verworfen werden. Gelesen wird bis zum Ende, auch wenn
        // niemand mehr zuhoert -- eine geschlossene Pipe liesse yt-dlp scheitern.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        self.runtime.spawn(read_lines(stdout, move |line| {
            let _ = tx.send(line);
        }));

        let err_tail = Arc::new(Mutex::new(Vec::<String>::new()));
        let tail = err_tail.clone();
        self.runtime.spawn(read_lines(stderr, move |line| {
            if line.trim().is_empty() {
                return;
            }
            let mut guard = tail.lock().unwrap();
            guard.push(line);
            if guard.len() > 20 {
                guard.remove(0);
            }
        }));

        // Alle Dateien, die yt-dlp in diesem Lauf schreibt -- fuer das
        // Aufraeumen nach einem Abbruch.
        let mut destinations: Vec<String> = Vec::new();
        let status = loop {
            tokio::select! {
                _ = &mut cancel_rx => {
                    // Den ganzen Baum: der erste yt-dlp-Prozess allein laedt nicht.
                    if let Some(tree) = &tree {
                        tree.kill();
                    }
                    let _ = child.kill().await;
                    break child.wait().await;
                }
                Some(line) = rx.recv() => destinations.extend(self.handle_line(id, &line)),
                // `wait()` ist abbruchsicher und darf hier wiederholt werden.
                res = child.wait() => break res,
            }
        };

        // Was noch im Puffer liegt, gehoert zum Ergebnis: der Kanal endet erst,
        // wenn die lesende Aufgabe fertig ist -- also wenn kein Prozess des
        // Baums die Pipe mehr offen haelt.
        while let Some(line) = rx.recv().await {
            destinations.extend(self.handle_line(id, &line));
        }

        let canceled = {
            let mut guard = self.inner.lock().unwrap();
            guard.cancels.remove(id);
            guard.trees.remove(id);
            guard
                .jobs
                .iter()
                .any(|j| j.id == id && j.status == JobStatus::Canceled)
        };
        drop(tree);
        if canceled {
            remove_leftovers(&destinations, &req.output_dir).await;
            return;
        }

        match status {
            Ok(code) if code.success() => self.patch(id, |job| {
                job.status = JobStatus::Done;
                job.progress = 1.0;
                job.speed = None;
                job.eta = None;
            }),
            Ok(code) => {
                let tail = err_tail.lock().unwrap().clone();
                let raw = tail
                    .iter()
                    .rev()
                    .find(|l| l.contains("ERROR"))
                    .or_else(|| tail.last())
                    .cloned()
                    .unwrap_or_else(|| {
                        format!("yt-dlp endete mit Code {}", code.code().unwrap_or(-1))
                    });
                // Bekannte Fehler mit dem naechsten Schritt statt der englischen Zeile
                let message = explain_error(&tail, req.cookies).unwrap_or(raw);
                self.patch(id, |job| {
                    job.status = JobStatus::Error;
                    job.error = Some(message.clone());
                });
            }
            Err(err) => self.patch(id, |job| {
                job.status = JobStatus::Error;
                job.error = Some(err.to_string());
            }),
        }
    }

    /// Wertet eine Zeile aus; liefert die Zieldatei, wenn sie eine meldet.
    fn handle_line(&self, id: &str, line: &str) -> Option<String> {
        if let Some(progress) = parse_progress(line) {
            self.patch(id, |job| {
                if let Some(p) = progress.percent {
                    job.progress = p;
                }
                job.speed = progress.speed;
                job.eta = progress.eta;
            });
        }
        if let Some(existing) = parse_existing(line) {
            let title = title_from_path(&existing);
            self.patch(id, |job| {
                job.output_file = Some(existing.clone());
                job.title = Some(title.clone());
                job.note = Some(NOTE_EXISTING.into());
            });
        }
        let dest = parse_destination(line)?;
        let title = title_from_path(&dest);
        self.patch(id, |job| {
            job.output_file = Some(dest.clone());
            job.title = Some(title.clone());
        });
        Some(dest)
    }

    pub fn cancel(&self, id: &str) {
        let sender = {
            let mut guard = self.inner.lock().unwrap();
            let Some(job) = guard.jobs.iter_mut().find(|j| j.id == id) else {
                return;
            };
            if matches!(
                job.status,
                JobStatus::Done | JobStatus::Error | JobStatus::Canceled
            ) {
                return;
            }
            job.status = JobStatus::Canceled;
            job.speed = None;
            job.eta = None;
            let copy = job.clone();
            let sender = guard.cancels.remove(id);
            let tree = guard.trees.get(id).cloned();
            drop(guard);
            self.sink.on_update(&copy);
            // Sofort beenden, nicht erst, wenn die Aufgabe dazu kommt
            if let Some(tree) = tree {
                tree.kill();
            }
            sender
        };
        if let Some(tx) = sender {
            let _ = tx.send(());
        }
    }

    /// Beendet alle laufenden Downloads samt Unterprozessen, ohne zu warten --
    /// fuer das Schliessen der Oberflaeche. Unter Windows erledigt das zur Not
    /// auch das System (Job-Objekt), unter macOS liefen sie sonst weiter.
    pub fn shutdown(&self) {
        let trees: Vec<Arc<ProcessTree>> = self
            .inner
            .lock()
            .unwrap()
            .trees
            .drain()
            .map(|(_, tree)| tree)
            .collect();
        for tree in trees {
            tree.kill();
        }
    }

    pub fn clear_finished(&self) {
        let mut guard = self.inner.lock().unwrap();
        let keep: Vec<String> = guard
            .jobs
            .iter()
            .filter(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running))
            .map(|j| j.id.clone())
            .collect();
        guard.jobs.retain(|j| keep.contains(&j.id));
        guard.requests.retain(|id, _| keep.contains(id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fortschritt_mit_tempo_und_restzeit() {
        let p = parse_progress("[download]  12.3% of 45.60MiB at  1.23MiB/s ETA 00:42").unwrap();
        assert!((p.percent.unwrap() - 0.123).abs() < 1e-9);
        assert_eq!(p.speed.as_deref(), Some("1.23MiB/s"));
        assert_eq!(p.eta.as_deref(), Some("00:42"));
    }

    #[test]
    fn fortschritt_ohne_zusatzangaben() {
        let p = parse_progress("[download]   0.0% of ~12.00MiB").unwrap();
        assert_eq!(p.percent, Some(0.0));
        assert_eq!(p.speed, None);
        assert_eq!(p.eta, None);
    }

    #[test]
    fn fortschritt_deckelt_bei_100_prozent() {
        // 100 % meldet yt-dlp vor dem Zusammenfuegen -- fertig ist erst der
        // Prozess, deshalb bleibt die Anzeige knapp darunter.
        let p = parse_progress("[download] 100.0% of 45.60MiB in 00:12").unwrap();
        assert_eq!(p.percent, Some(0.999));
    }

    #[test]
    fn unbekanntes_tempo_wird_verworfen() {
        let p = parse_progress("[download]  50.0% of 10.00MiB at Unknown ETA Unknown").unwrap();
        assert_eq!(p.speed, None);
        assert_eq!(p.eta, None);
    }

    #[test]
    fn nicht_fortschritt_gibt_nichts() {
        assert!(parse_progress("[info] Verfügbare Formate").is_none());
        assert!(parse_progress("[download] Destination: /tmp/film.mp4").is_none());
        assert!(parse_progress("").is_none());
    }

    /// Zeilen, die eine echte yt-dlp-Ausfuehrung (2026.08.19) ausgegeben hat.
    /// Beim Start fehlen Tempo und Restzeit noch, am Ende faellt die Prozent-
    /// Nachkommastelle weg und die Reihenfolge dreht sich ("in ... at ...").
    #[test]
    fn echte_ausgabe_von_yt_dlp() {
        let start =
            parse_progress("[download]   1.3% of   75.94KiB at  Unknown B/s ETA Unknown").unwrap();
        assert!((start.percent.unwrap() - 0.013).abs() < 1e-9);
        assert_eq!(start.speed, None, "\"Unknown B/s\" ist keine Angabe");
        assert_eq!(start.eta, None);

        let mitte =
            parse_progress("[download]  40.8% of   75.94KiB at   18.59MiB/s ETA 00:00").unwrap();
        assert!((mitte.percent.unwrap() - 0.408).abs() < 1e-9);
        assert_eq!(mitte.speed.as_deref(), Some("18.59MiB/s"));
        assert_eq!(mitte.eta.as_deref(), Some("00:00"));

        let ende =
            parse_progress("[download] 100% of   75.94KiB in 00:00:00 at 13.34MiB/s").unwrap();
        assert_eq!(ende.percent, Some(0.999), "fertig meldet erst der Prozess");
        assert_eq!(ende.speed.as_deref(), Some("13.34MiB/s"));
        assert_eq!(ende.eta, None);

        assert_eq!(
            parse_destination("[download] Destination: /ziel/probe.mp4").as_deref(),
            Some("/ziel/probe.mp4")
        );
        // Meldungen ohne Fortschritt duerfen nichts ausloesen.
        for line in [
            "[generic] Extracting URL: http://localhost:8123/probe.mp4",
            "[info] probe: Downloading 1 format(s): mp4",
        ] {
            assert!(parse_progress(line).is_none(), "{line}");
            assert!(parse_destination(line).is_none(), "{line}");
        }
    }

    #[test]
    fn zieldatei_aus_allen_meldungen() {
        assert_eq!(
            parse_destination("[download] Destination: /tmp/film.f137.mp4").as_deref(),
            Some("/tmp/film.f137.mp4")
        );
        assert_eq!(
            parse_destination("[Merger] Merging formats into \"/tmp/film.mp4\"").as_deref(),
            Some("/tmp/film.mp4")
        );
        assert_eq!(
            parse_destination("[ExtractAudio] Destination: /tmp/lied.mp3").as_deref(),
            Some("/tmp/lied.mp3")
        );
        assert!(parse_destination("[download]  12.3% of 45.60MiB").is_none());
    }

    #[test]
    fn schon_vorhandene_datei() {
        assert_eq!(
            parse_existing("[download] C:\\Ziel\\Trailer.mp4 has already been downloaded")
                .as_deref(),
            Some("C:\\Ziel\\Trailer.mp4")
        );
        // Aeltere Fassungen haengen noch etwas an
        assert_eq!(
            parse_existing("[download] /z/a b.mp4 has already been downloaded and merged")
                .as_deref(),
            Some("/z/a b.mp4")
        );
        assert!(parse_existing("[download] Destination: /z/a.mp4").is_none());
        assert!(parse_existing("[download]  12.3% of 45.60MiB").is_none());
        assert!(parse_existing("[download]  has already been downloaded").is_none());
    }

    #[test]
    fn reste_eines_abbruchs() {
        let names: Vec<String> = [
            "Film.f137.mp4.part",
            "Film.f137.mp4.ytdl",
            "Film.f137.mp4.part-Frag3",
            "Film.f140.m4a",
            "Film.temp.mp4",
            "Film.mp4.bak",
            "Film 2.f137.mp4.part",
            "Anderes.mp4",
        ]
        .map(String::from)
        .to_vec();
        let mut left = leftovers("Film.f137.mp4", &names);
        left.sort();
        assert_eq!(
            left,
            [
                "Film.f137.mp4.part",
                "Film.f137.mp4.part-Frag3",
                "Film.f137.mp4.ytdl"
            ]
        );
        // Fertig geladener Teilstrom, nie zusammengefuegt
        assert_eq!(leftovers("Film.f140.m4a", &names), ["Film.f140.m4a"]);
        // Halb zusammengefuegt
        assert_eq!(leftovers("Film.mp4", &names), ["Film.temp.mp4"]);
        assert!(leftovers("Gibts nicht.mp4", &names).is_empty());
        assert!(leftovers("ohne-endung", &["ohne-endung.temp".into()]).len() == 1);
    }

    #[test]
    fn titel_aus_pfad() {
        assert_eq!(title_from_path("/tmp/Mein Film.mp4"), "Mein Film");
        assert_eq!(title_from_path("C:\\Videos\\Clip.f137.mp4"), "Clip.f137");
        assert_eq!(title_from_path("ohne-endung"), "ohne-endung");
    }

    fn req(format: Format, max_height: Option<u32>) -> EnqueueRequest {
        EnqueueRequest {
            url: "https://example.com/watch?v=1".into(),
            format,
            max_height,
            output_dir: "/ziel".into(),
            cookies: None,
        }
    }

    #[test]
    fn verlangt_utf8_von_yt_dlp() {
        let args = build_args(&req(Format::Video, None), Path::new("/bin"), None);
        let idx = args.iter().position(|a| a == "--encoding").unwrap();
        assert_eq!(args[idx + 1], "utf-8");
    }

    #[test]
    fn zeile_aus_der_pipe() {
        // So kam der ARTE-Titel ohne --encoding an: 0xA0 ist cp1252
        let cp1252 = decode_line(b"Mein Maga-Leben |\xa0ARTE Reportage\r\n");
        assert_eq!(cp1252, "Mein Maga-Leben |\u{fffd}ARTE Reportage");
        let utf8 = decode_line("Mein Maga-Leben |\u{a0}ARTE Reportage\n".as_bytes());
        assert_eq!(utf8, "Mein Maga-Leben |\u{a0}ARTE Reportage");
        assert_eq!(decode_line(b""), "");
    }

    /// Eine ungueltige Zeile darf das Lesen nicht beenden -- sonst ist die
    /// Pipe zu, und yt-dlp scheitert mit „[Errno 22] Invalid argument“.
    #[tokio::test]
    async fn liest_nach_ungueltigem_utf8_weiter() {
        let data: &[u8] = b"[download] Destination: C:\\z\\Leben |\xa0ARTE.mp4\n\
            [download]   1.0% of 10.00MiB\n\
            letzte ohne Zeilenende";
        let mut lines = Vec::new();
        read_lines(data, |l| lines.push(l)).await;
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].ends_with("ARTE.mp4"));
        assert_eq!(parse_progress(&lines[1]).unwrap().percent, Some(0.01));
        assert_eq!(lines[2], "letzte ohne Zeilenende");
    }

    #[test]
    fn adresse_steht_hinter_doppelstrich() {
        let mut r = req(Format::Video, None);
        r.url = "-Qq1MLVwmfs".into();
        let args = build_args(&r, Path::new("/bin"), None);
        let n = args.len();
        assert_eq!(args[n - 2..], ["--", "-Qq1MLVwmfs"]);
    }

    #[test]
    fn anmeldung_und_laufzeit_vor_der_adresse() {
        let mut r = req(Format::Video, None);
        r.cookies = Some(CookieBrowser::Firefox);
        let args = build_args(&r, Path::new("/bin"), Some(Path::new("/bin/deno")));
        let at = |flag: &str| args.iter().position(|a| a == flag).unwrap();
        assert_eq!(args[at("--cookies-from-browser") + 1], "firefox");
        assert_eq!(args[at("--js-runtimes") + 1], "deno:/bin/deno");
        assert!(at("--cookies-from-browser") < at("--"));
        assert!(at("--js-runtimes") < at("--"));

        let ohne = build_args(&req(Format::Video, None), Path::new("/bin"), None);
        let flags = |a: &String| a.starts_with("--cookies") || a == "--js-runtimes";
        assert!(!ohne.iter().any(flags));
    }

    #[test]
    fn anfrage_ohne_anmeldung_bleibt_gueltig() {
        // So schickt sie eine Oberflaeche, die das Feld noch nicht kennt
        let raw = r#"{"url":"u","format":"video","maxHeight":null,"outputDir":"/z"}"#;
        let r: EnqueueRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(r.cookies, None);
        let raw = r#"{"url":"u","format":"video","outputDir":"/z","cookies":"firefox"}"#;
        let r: EnqueueRequest = serde_json::from_str(raw).unwrap();
        assert_eq!(r.cookies, Some(CookieBrowser::Firefox));
    }

    #[test]
    fn video_mit_hoehenbegrenzung() {
        let args = build_args(&req(Format::Video, Some(1080)), Path::new("/bin"), None);
        assert!(args.contains(&"--ffmpeg-location".to_string()));
        assert!(args.contains(&"/bin".to_string()));
        let idx = args.iter().position(|a| a == "-f").unwrap();
        assert_eq!(args[idx + 1], "bv*[height<=1080]+ba/b[height<=1080]/b");
        assert!(args.contains(&"mp4".to_string()));
        assert_eq!(args.last().unwrap(), "https://example.com/watch?v=1");
    }

    #[test]
    fn video_ohne_begrenzung() {
        let args = build_args(&req(Format::Video, None), Path::new("/bin"), None);
        let idx = args.iter().position(|a| a == "-f").unwrap();
        assert_eq!(args[idx + 1], "bv*+ba/b");
    }

    #[test]
    fn audio_waehlt_das_format() {
        let mp3 = build_args(&req(Format::AudioMp3, Some(720)), Path::new("/bin"), None);
        assert!(mp3.contains(&"-x".to_string()));
        let idx = mp3.iter().position(|a| a == "--audio-format").unwrap();
        assert_eq!(mp3[idx + 1], "mp3");
        // Die Hoehenbegrenzung ist bei Audio bedeutungslos und taucht nicht auf.
        assert!(!mp3.iter().any(|a| a.contains("height")));

        let m4a = build_args(&req(Format::AudioM4a, None), Path::new("/bin"), None);
        let idx = m4a.iter().position(|a| a == "--audio-format").unwrap();
        assert_eq!(m4a[idx + 1], "m4a");
    }

    #[test]
    fn zielmuster_liegt_im_zielordner() {
        let args = build_args(&req(Format::Video, None), Path::new("/bin"), None);
        let idx = args.iter().position(|a| a == "-o").unwrap();
        assert!(args[idx + 1].starts_with("/ziel"));
        assert!(args[idx + 1].ends_with("%(title)s.%(ext)s"));
    }
}
