//! Warteschlange der Downloads: startet yt-dlp, liest den Fortschritt Zeile
//! fuer Zeile mit und meldet jede Aenderung an die Oberflaeche.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::oneshot;

pub const JOB_EVENT: &str = "job-update";

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
    pub created_at: u64,
}

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

/// Dateiname ohne Endung -- dient als Titel in der Liste.
pub fn title_from_path(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}

/// Baut die Aufrufparameter fuer yt-dlp.
pub fn build_args(req: &EnqueueRequest, bin_dir: &Path) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--newline".into(),
        "--no-playlist".into(),
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
    args.push("-o".into());
    args.push(
        Path::new(&req.output_dir)
            .join("%(title)s.%(ext)s")
            .to_string_lossy()
            .into_owned(),
    );
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
    active: usize,
}

#[derive(Clone)]
pub struct Manager {
    inner: Arc<Mutex<Inner>>,
    concurrency: usize,
}

impl Default for Manager {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            concurrency: 2,
        }
    }
}

impl Manager {
    pub fn list(&self) -> Vec<Job> {
        self.inner.lock().unwrap().jobs.clone()
    }

    pub fn busy(&self) -> bool {
        self.inner.lock().unwrap().active > 0
    }

    fn emit(app: &AppHandle, job: &Job) {
        let _ = app.emit(JOB_EVENT, job.clone());
    }

    fn patch<F: FnOnce(&mut Job)>(&self, app: &AppHandle, id: &str, f: F) {
        let mut guard = self.inner.lock().unwrap();
        if let Some(job) = guard.jobs.iter_mut().find(|j| j.id == id) {
            f(job);
            let copy = job.clone();
            drop(guard);
            Self::emit(app, &copy);
        }
    }

    pub fn enqueue(&self, app: &AppHandle, req: EnqueueRequest, bin_dir: PathBuf) -> String {
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
            created_at: now_ms(),
        };
        {
            let mut guard = self.inner.lock().unwrap();
            guard.jobs.insert(0, job.clone());
            guard.requests.insert(id.clone(), req);
        }
        Self::emit(app, &job);
        self.schedule(app, bin_dir);
        id
    }

    /// Startet wartende Jobs, solange Plaetze frei sind.
    fn schedule(&self, app: &AppHandle, bin_dir: PathBuf) {
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
                Self::emit(app, &copy);
                req.map(|r| (id, r))
            };
            let Some((id, req)) = next else { return };

            let manager = self.clone();
            let app_handle = app.clone();
            let dir = bin_dir.clone();
            tauri::async_runtime::spawn(async move {
                manager.run(&app_handle, &id, req, &dir).await;
                {
                    let mut guard = manager.inner.lock().unwrap();
                    guard.active = guard.active.saturating_sub(1);
                }
                manager.schedule(&app_handle, dir);
            });
        }
    }

    async fn run(&self, app: &AppHandle, id: &str, req: EnqueueRequest, bin_dir: &Path) {
        let ytdlp = crate::ytdlp::binary_path(bin_dir);
        if !ytdlp.exists() {
            self.patch(app, id, |job| {
                job.status = JobStatus::Error;
                job.error = Some("yt-dlp fehlt — bitte zuerst die Werkzeuge einrichten.".into());
            });
            return;
        }

        let mut cmd = tokio::process::Command::new(&ytdlp);
        cmd.args(build_args(&req, bin_dir))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // tokio::process::Command bringt creation_flags unter Windows selbst mit.
        #[cfg(windows)]
        cmd.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(err) => {
                self.patch(app, id, |job| {
                    job.status = JobStatus::Error;
                    job.error = Some(format!("Start fehlgeschlagen: {err}"));
                });
                return;
            }
        };

        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        self.inner
            .lock()
            .unwrap()
            .cancels
            .insert(id.to_string(), cancel_tx);

        let stdout = child.stdout.take().expect("stdout ist gesetzt");
        let stderr = child.stderr.take().expect("stderr ist gesetzt");

        // Beide Ausgaben lesen eigene Aufgaben: `next_line()` ist nicht
        // abbruchsicher, in einem select! koennte also mitten in einer Zeile
        // abgebrochen und der Rest verworfen werden.
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        let err_tail = Arc::new(Mutex::new(Vec::<String>::new()));
        let tail = err_tail.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                let mut guard = tail.lock().unwrap();
                guard.push(line);
                if guard.len() > 20 {
                    guard.remove(0);
                }
            }
        });

        let status = loop {
            tokio::select! {
                _ = &mut cancel_rx => {
                    let _ = child.kill().await;
                    break child.wait().await;
                }
                Some(line) = rx.recv() => self.handle_line(app, id, &line),
                // `wait()` ist abbruchsicher und darf hier wiederholt werden.
                res = child.wait() => break res,
            }
        };

        // Was noch im Puffer liegt, gehoert zum Ergebnis: der Kanal endet erst,
        // wenn die lesende Aufgabe fertig ist.
        while let Some(line) = rx.recv().await {
            self.handle_line(app, id, &line);
        }

        self.inner.lock().unwrap().cancels.remove(id);

        let canceled = self
            .inner
            .lock()
            .unwrap()
            .jobs
            .iter()
            .any(|j| j.id == id && j.status == JobStatus::Canceled);
        if canceled {
            return;
        }

        match status {
            Ok(code) if code.success() => self.patch(app, id, |job| {
                job.status = JobStatus::Done;
                job.progress = 1.0;
                job.speed = None;
                job.eta = None;
            }),
            Ok(code) => {
                let tail = err_tail.lock().unwrap().clone();
                let message = tail
                    .iter()
                    .rev()
                    .find(|l| l.contains("ERROR"))
                    .or_else(|| tail.last())
                    .cloned()
                    .unwrap_or_else(|| {
                        format!("yt-dlp endete mit Code {}", code.code().unwrap_or(-1))
                    });
                self.patch(app, id, |job| {
                    job.status = JobStatus::Error;
                    job.error = Some(message.clone());
                });
            }
            Err(err) => self.patch(app, id, |job| {
                job.status = JobStatus::Error;
                job.error = Some(err.to_string());
            }),
        }
    }

    fn handle_line(&self, app: &AppHandle, id: &str, line: &str) {
        if let Some(progress) = parse_progress(line) {
            self.patch(app, id, |job| {
                if let Some(p) = progress.percent {
                    job.progress = p;
                }
                job.speed = progress.speed;
                job.eta = progress.eta;
            });
        }
        if let Some(dest) = parse_destination(line) {
            let title = title_from_path(&dest);
            self.patch(app, id, |job| {
                job.output_file = Some(dest.clone());
                job.title = Some(title.clone());
            });
        }
    }

    pub fn cancel(&self, app: &AppHandle, id: &str) {
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
            drop(guard);
            Self::emit(app, &copy);
            sender
        };
        if let Some(tx) = sender {
            let _ = tx.send(());
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
        }
    }

    #[test]
    fn video_mit_hoehenbegrenzung() {
        let args = build_args(&req(Format::Video, Some(1080)), Path::new("/bin"));
        assert!(args.contains(&"--ffmpeg-location".to_string()));
        assert!(args.contains(&"/bin".to_string()));
        let idx = args.iter().position(|a| a == "-f").unwrap();
        assert_eq!(args[idx + 1], "bv*[height<=1080]+ba/b[height<=1080]/b");
        assert!(args.contains(&"mp4".to_string()));
        assert_eq!(args.last().unwrap(), "https://example.com/watch?v=1");
    }

    #[test]
    fn video_ohne_begrenzung() {
        let args = build_args(&req(Format::Video, None), Path::new("/bin"));
        let idx = args.iter().position(|a| a == "-f").unwrap();
        assert_eq!(args[idx + 1], "bv*+ba/b");
    }

    #[test]
    fn audio_waehlt_das_format() {
        let mp3 = build_args(&req(Format::AudioMp3, Some(720)), Path::new("/bin"));
        assert!(mp3.contains(&"-x".to_string()));
        let idx = mp3.iter().position(|a| a == "--audio-format").unwrap();
        assert_eq!(mp3[idx + 1], "mp3");
        // Die Hoehenbegrenzung ist bei Audio bedeutungslos und taucht nicht auf.
        assert!(!mp3.iter().any(|a| a.contains("height")));

        let m4a = build_args(&req(Format::AudioM4a, None), Path::new("/bin"));
        let idx = m4a.iter().position(|a| a == "--audio-format").unwrap();
        assert_eq!(m4a[idx + 1], "m4a");
    }

    #[test]
    fn zielmuster_liegt_im_zielordner() {
        let args = build_args(&req(Format::Video, None), Path::new("/bin"));
        let idx = args.iter().position(|a| a == "-o").unwrap();
        assert!(args[idx + 1].starts_with("/ziel"));
        assert!(args[idx + 1].ends_with("%(title)s.%(ext)s"));
    }
}
