//! Fenster-Oberflaeche des MottulVideoLoader.
//!
//! Die Arbeit macht `mottul-video-core`; hier haengen nur die Tauri-Befehle,
//! der Zustand und die Weitergabe der Fortschrittsmeldungen ans Fenster.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use core::queue::{EnqueueRequest, Job, JobSink, JobStatus, Manager};
use mottul_video_core as core;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager as _, State};
use tauri_plugin_notification::NotificationExt;

pub const JOB_EVENT: &str = "job-update";
pub const STATUS_EVENT: &str = "tools-status";

/// Reicht jede Aenderung an einem Auftrag als Ereignis ans Fenster weiter und
/// meldet sich beim System, wenn alles durch ist.
struct WindowSink {
    app: AppHandle,
    finished: Mutex<Finished>,
}

impl JobSink for WindowSink {
    fn on_update(&self, job: &Job) {
        let _ = self.app.emit(JOB_EVENT, job.clone());

        // Vor der eigenen Sperre: `list()` sperrt die Warteschlange, und die
        // ruft hierher nie mit gehaltener Sperre.
        let idle = self
            .app
            .try_state::<AppState>()
            .is_some_and(|state| state.manager.list().iter().all(|j| !j.status.is_open()));
        let Some((title, body)) = self.finished.lock().unwrap().update(job, idle) else {
            return;
        };
        // Wer auf das Fenster schaut, sieht es dort schon
        let focused = self
            .app
            .get_webview_window("main")
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);
        if !focused {
            let _ = self
                .app
                .notification()
                .builder()
                .title(title)
                .body(body)
                .show();
        }
    }
}

/// Was seit der letzten Benachrichtigung fertig wurde. Gemeldet wird erst, wenn
/// nichts mehr wartet oder laeuft: Bei zehn eingefuegten Adressen kommt eine
/// Meldung, nicht zehn.
#[derive(Default)]
struct Finished {
    /// Letzter bekannter Stand je Auftrag -- erkennt den Uebergang, auch wenn
    /// derselbe Stand noch einmal gemeldet wird.
    seen: HashMap<String, JobStatus>,
    done: Vec<String>,
    failed: Vec<String>,
}

impl Finished {
    /// Nimmt eine Aenderung entgegen; liefert Titel und Text, wenn jetzt
    /// benachrichtigt werden soll. `idle`: nichts wartet oder laeuft mehr.
    fn update(&mut self, job: &Job, idle: bool) -> Option<(String, String)> {
        if self.seen.insert(job.id.clone(), job.status) == Some(job.status) {
            return None;
        }
        let name = job.title.clone().unwrap_or_else(|| job.url.clone());
        match job.status {
            JobStatus::Done => self.done.push(name),
            JobStatus::Error => self.failed.push(name),
            // Abgebrochen hat der Nutzer selbst; beendet aber trotzdem die Runde
            JobStatus::Canceled => {}
            JobStatus::Queued | JobStatus::Running => return None,
        }
        if !idle {
            return None;
        }
        let message = summary(&self.done, &self.failed);
        self.done.clear();
        self.failed.clear();
        message
    }
}

/// Titel und Text der Benachrichtigung; None, wenn nichts fertig wurde.
fn summary(done: &[String], failed: &[String]) -> Option<(String, String)> {
    let names = |list: &[String]| match list {
        [a, b, c] => format!("{a}, {b}, {c}"),
        [a, b, rest @ ..] if !rest.is_empty() => format!("{a}, {b} und {} weitere", rest.len()),
        _ => list.join(", "),
    };
    match (done.len(), failed.len()) {
        (0, 0) => None,
        (1, 0) => Some(("Download fertig".into(), names(done))),
        (n, 0) => Some((format!("{n} Downloads fertig"), names(done))),
        (0, 1) => Some(("Download fehlgeschlagen".into(), names(failed))),
        (0, n) => Some((format!("{n} Downloads fehlgeschlagen"), names(failed))),
        (d, f) => Some((
            "Downloads beendet".into(),
            format!("{d} fertig, {f} fehlgeschlagen"),
        )),
    }
}

/// Vom Benutzer gewaehlte Einstellungen; liegen als JSON bei den App-Daten.
/// Fehlende Felder (Datei aus einer aelteren Fassung) bekommen die Vorgabe,
/// statt dass die ganze Datei verworfen wird.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub output_dir: String,
    pub format: core::Format,
    pub max_height: Option<u32>,
    /// yt-dlp beim Start pruefen und bei Bedarf aktualisieren.
    pub auto_update: bool,
    /// Anmeldung: Cookies aus diesem Browser, None = ohne.
    #[serde(deserialize_with = "mottul_video_core::auth::lenient_browser")]
    pub cookies: Option<core::CookieBrowser>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // Wie die Terminal-Variante: sonst ist „Laden“ beim ersten Start
            // ohne erkennbaren Grund gesperrt.
            output_dir: default_output_dir(),
            format: core::Format::Video,
            max_height: Some(1080),
            auto_update: true,
            cookies: None,
        }
    }
}

pub struct AppState {
    pub bin_dir: PathBuf,
    pub settings_file: PathBuf,
    pub manager: Manager,
    /// Zuletzt ermittelte Werte, die sich nicht aus der Platte ablesen lassen.
    pub remote: Mutex<Remote>,
}

#[derive(Default)]
pub struct Remote {
    pub latest: Option<String>,
    pub busy: bool,
    pub last_error: Option<String>,
    /// Neuere Version der App, laut letzter Pruefung.
    pub app_update: Option<core::update::AppUpdate>,
}

impl AppState {
    /// Blockiert fuer die Versionsabfragen -- nur ueber [`snapshot`] aufrufen.
    fn snapshot_blocking(&self) -> core::ToolsStatus {
        // Die Sperre nicht ueber die Abfragen halten: die Startpruefung wartet
        // sonst Sekunden darauf, nur um `busy` zu setzen. `busy` und Fehler
        // erst danach lesen -- sie koennen sich waehrenddessen geaendert haben.
        let latest = self.remote.lock().unwrap().latest.clone();
        let mut status = core::read_status(&self.bin_dir, latest);
        let remote = self.remote.lock().unwrap();
        status.busy = remote.busy;
        status.last_error = remote.last_error.clone();
        status.app_update = remote.app_update.clone();
        status
    }
}

/// Zustand der Werkzeuge, abseits des Fensters ermittelt.
///
/// Befehle ohne `async` fuehrt Tauri auf dem Hauptfaden aus. Die
/// Versionsabfragen dauern beim ersten Mal Sekunden -- so lange war das
/// Fenster beim Start eingefroren.
async fn snapshot(app: &AppHandle) -> core::ToolsStatus {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || app.state::<AppState>().snapshot_blocking())
        .await
        .unwrap_or_default()
}

fn default_output_dir() -> String {
    core::paths::default_output_dir()
        .to_string_lossy()
        .into_owned()
}

fn read_settings(path: &PathBuf) -> Settings {
    let mut settings: Settings = std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    // Aeltere Fassungen speicherten einen leeren Zielordner.
    if settings.output_dir.trim().is_empty() {
        settings.output_dir = default_output_dir();
    }
    settings
}

/* ------------------------------- Befehle -------------------------------- */

#[tauri::command]
async fn tools_status(app: AppHandle) -> Result<core::ToolsStatus, String> {
    Ok(snapshot(&app).await)
}

/// Prueft yt-dlp auf eine neue Version und laedt fehlende Werkzeuge nach.
#[tauri::command]
async fn ensure_tools(app: AppHandle) -> Result<core::ToolsStatus, String> {
    // Die Sperre endet mit dem Block: vor einem `await` darf sie nicht mehr
    // gehalten werden.
    let started = {
        let state = app.state::<AppState>();
        let mut remote = state.remote.lock().unwrap();
        if remote.busy {
            None
        } else {
            remote.busy = true;
            remote.last_error = None;
            Some((state.bin_dir.clone(), state.manager.busy()))
        }
    };
    let Some((bin_dir, downloads_running)) = started else {
        return Ok(snapshot(&app).await);
    };
    let _ = app.emit(STATUS_EVENT, snapshot(&app).await);

    let result = core::ensure_tools(&bin_dir, downloads_running).await;

    {
        let state = app.state::<AppState>();
        let mut remote = state.remote.lock().unwrap();
        remote.busy = false;
        remote.last_error = result.last_error.clone();
        if result.ytdlp_latest.is_some() {
            remote.latest = result.ytdlp_latest.clone();
        }
        remote.app_update = result.app_update.clone();
    }
    let status = snapshot(&app).await;
    let _ = app.emit(STATUS_EVENT, status.clone());
    Ok(status)
}

/// Stellt einen Auftrag je Adresse ein. `req.url` darf mehrere enthalten, eine
/// je Zeile oder durch Leerraum getrennt (eingefuegte Liste, gezogene Links);
/// zerlegt wird im Kern ([`core::queue::split_addresses`]).
#[tauri::command]
fn enqueue(req: EnqueueRequest, state: State<'_, AppState>) -> Result<Vec<String>, String> {
    if req.output_dir.trim().is_empty() {
        return Err("Kein Zielordner gewählt".into());
    }
    let urls = core::queue::split_addresses(&req.url);
    if urls.is_empty() {
        return Err("Keine Adresse gefunden".into());
    }
    Ok(urls
        .into_iter()
        .map(|url| {
            state
                .manager
                .enqueue(EnqueueRequest { url, ..req.clone() }, state.bin_dir.clone())
        })
        .collect())
}

#[tauri::command]
fn list_jobs(state: State<'_, AppState>) -> Vec<Job> {
    state.manager.list()
}

#[tauri::command]
fn cancel_job(id: String, state: State<'_, AppState>) {
    state.manager.cancel(&id);
}

#[tauri::command]
fn clear_finished(state: State<'_, AppState>) {
    state.manager.clear_finished();
}

/// Fehlgeschlagenen oder abgebrochenen Auftrag mit derselben Anfrage neu starten.
#[tauri::command]
fn retry_job(id: String, state: State<'_, AppState>) -> Result<(), String> {
    if state.manager.retry(&id, state.bin_dir.clone()) {
        Ok(())
    } else {
        Err(
            "Dieser Auftrag lässt sich gerade nicht neu starten – gleich noch einmal versuchen."
                .into(),
        )
    }
}

#[tauri::command]
fn load_settings(state: State<'_, AppState>) -> Settings {
    read_settings(&state.settings_file)
}

#[tauri::command]
fn save_settings(settings: Settings, state: State<'_, AppState>) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    core::paths::write_atomic(&state.settings_file, raw.as_bytes()).map_err(|e| e.to_string())
}

/// Wo die Werkzeuge liegen -- fuer den Hinweis am Fensterfuss.
#[tauri::command]
fn bin_dir(state: State<'_, AppState>) -> String {
    state.bin_dir.to_string_lossy().into_owned()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let manager = Manager::new(Arc::new(WindowSink {
                app: app.handle().clone(),
                finished: Mutex::new(Finished::default()),
            }))?;
            let state = AppState {
                bin_dir: core::tools::bin_dir(&data_dir),
                settings_file: data_dir.join("settings.json"),
                manager,
                remote: Mutex::new(Remote::default()),
            };
            let auto_update = read_settings(&state.settings_file).auto_update;
            app.manage(state);

            // Startpruefung im Hintergrund: ohne Netz startet die App normal,
            // der Fehler steht nur im Status.
            if auto_update {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let _ = ensure_tools(handle).await;
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            tools_status,
            ensure_tools,
            enqueue,
            list_jobs,
            cancel_job,
            clear_finished,
            retry_job,
            load_settings,
            save_settings,
            bin_dir,
        ])
        .build(tauri::generate_context!())
        .expect("Anwendung konnte nicht gestartet werden")
        .run(|app, event| {
            // Laufende Downloads samt Unterprozessen beenden, statt sie
            // verwaist im Hintergrund weiterladen zu lassen, und ihre halben
            // Dateien wegraeumen. Laenger als drei Sekunden haelt das das
            // Schliessen nicht auf.
            if let tauri::RunEvent::Exit = event {
                app.state::<AppState>()
                    .manager
                    .shutdown(std::time::Duration::from_secs(3));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(id: &str, status: JobStatus, title: Option<&str>) -> Job {
        Job {
            id: id.into(),
            url: format!("https://example.com/{id}"),
            format: core::Format::Video,
            status,
            progress: 0.0,
            title: title.map(str::to_string),
            speed: None,
            eta: None,
            output_dir: "z".into(),
            output_file: None,
            error: None,
            note: None,
            phase: None,
            phase_label: None,
            created_at: 0,
        }
    }

    #[test]
    fn eine_meldung_wenn_alles_durch_ist() {
        let mut f = Finished::default();
        assert_eq!(f.update(&job("a", JobStatus::Running, None), false), None);
        // b laeuft noch: nichts melden
        assert_eq!(
            f.update(&job("a", JobStatus::Done, Some("Film")), false),
            None
        );
        assert_eq!(
            f.update(&job("b", JobStatus::Done, Some("Clip")), true),
            Some(("2 Downloads fertig".into(), "Film, Clip".into()))
        );
        // Dieselbe Meldung noch einmal ist kein neuer Uebergang
        assert_eq!(
            f.update(&job("b", JobStatus::Done, Some("Clip")), true),
            None
        );
    }

    #[test]
    fn fehler_und_abbruch() {
        let mut f = Finished::default();
        assert_eq!(
            f.update(&job("a", JobStatus::Error, None), true),
            Some((
                "Download fehlgeschlagen".into(),
                "https://example.com/a".into()
            )),
            "ohne Titel die Adresse"
        );
        // Selbst abgebrochen: keine Meldung, aber die Runde ist zu Ende
        assert_eq!(f.update(&job("b", JobStatus::Done, Some("X")), false), None);
        assert_eq!(
            f.update(&job("c", JobStatus::Canceled, None), true),
            Some(("Download fertig".into(), "X".into()))
        );
        assert_eq!(f.update(&job("d", JobStatus::Canceled, None), true), None);
        // „Nochmal“: Fehler -> wartet -> fertig ist ein neuer Uebergang
        f.update(&job("a", JobStatus::Queued, None), false);
        assert_eq!(
            f.update(&job("a", JobStatus::Done, Some("A")), true),
            Some(("Download fertig".into(), "A".into()))
        );
    }

    #[test]
    fn texte_der_meldung() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(summary(&[], &[]), None);
        assert_eq!(
            summary(&s(&["a", "b", "c", "d", "e"]), &[]),
            Some(("5 Downloads fertig".into(), "a, b und 3 weitere".into()))
        );
        assert_eq!(
            summary(&s(&["a", "b"]), &s(&["c"])),
            Some((
                "Downloads beendet".into(),
                "2 fertig, 1 fehlgeschlagen".into()
            ))
        );
        assert_eq!(
            summary(&[], &s(&["x", "y"])),
            Some(("2 Downloads fehlgeschlagen".into(), "x, y".into()))
        );
    }
}
