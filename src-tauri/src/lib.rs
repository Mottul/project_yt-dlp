//! Fenster-Oberflaeche des MottulVideoLoader.
//!
//! Die Arbeit macht `mottul-video-core`; hier haengen nur die Tauri-Befehle,
//! der Zustand und die Weitergabe der Fortschrittsmeldungen ans Fenster.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use core::queue::{EnqueueRequest, Job, JobSink, Manager};
use mottul_video_core as core;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager as _, State};

pub const JOB_EVENT: &str = "job-update";
pub const STATUS_EVENT: &str = "tools-status";

/// Reicht jede Aenderung an einem Auftrag als Ereignis ans Fenster weiter.
struct WindowSink(AppHandle);

impl JobSink for WindowSink {
    fn on_update(&self, job: &Job) {
        let _ = self.0.emit(JOB_EVENT, job.clone());
    }
}

/// Vom Benutzer gewaehlte Einstellungen; liegen als JSON bei den App-Daten.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub output_dir: String,
    pub format: core::Format,
    pub max_height: Option<u32>,
    /// yt-dlp beim Start pruefen und bei Bedarf aktualisieren.
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: String::new(),
            format: core::Format::Video,
            max_height: Some(1080),
            auto_update: true,
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
}

impl AppState {
    fn snapshot(&self) -> core::ToolsStatus {
        let remote = self.remote.lock().unwrap();
        let mut status = core::read_status(&self.bin_dir, remote.latest.clone());
        status.busy = remote.busy;
        status.last_error = remote.last_error.clone();
        status
    }
}

fn read_settings(path: &PathBuf) -> Settings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

/* ------------------------------- Befehle -------------------------------- */

#[tauri::command]
fn tools_status(state: State<'_, AppState>) -> core::ToolsStatus {
    state.snapshot()
}

/// Prueft yt-dlp auf eine neue Version und laedt fehlende Werkzeuge nach.
#[tauri::command]
async fn ensure_tools(app: AppHandle) -> Result<core::ToolsStatus, String> {
    let (bin_dir, downloads_running) = {
        let state = app.state::<AppState>();
        let mut remote = state.remote.lock().unwrap();
        if remote.busy {
            drop(remote);
            return Ok(state.snapshot());
        }
        remote.busy = true;
        remote.last_error = None;
        drop(remote);
        let value = (state.bin_dir.clone(), state.manager.busy());
        let _ = app.emit(STATUS_EVENT, state.snapshot());
        value
    };

    let result = core::ensure_tools(&bin_dir, downloads_running).await;

    let state = app.state::<AppState>();
    {
        let mut remote = state.remote.lock().unwrap();
        remote.busy = false;
        remote.last_error = result.last_error.clone();
        if result.ytdlp_latest.is_some() {
            remote.latest = result.ytdlp_latest.clone();
        }
    }
    let status = state.snapshot();
    let _ = app.emit(STATUS_EVENT, status.clone());
    Ok(status)
}

#[tauri::command]
fn enqueue(req: EnqueueRequest, state: State<'_, AppState>) -> Result<String, String> {
    if req.url.trim().is_empty() {
        return Err("Keine Adresse angegeben".into());
    }
    if req.output_dir.trim().is_empty() {
        return Err("Kein Zielordner gewählt".into());
    }
    Ok(state.manager.enqueue(req, state.bin_dir.clone()))
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

#[tauri::command]
fn load_settings(state: State<'_, AppState>) -> Settings {
    read_settings(&state.settings_file)
}

#[tauri::command]
fn save_settings(settings: Settings, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(dir) = state.settings_file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let raw = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&state.settings_file, raw).map_err(|e| e.to_string())
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
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let manager = Manager::new(Arc::new(WindowSink(app.handle().clone())))?;
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
            load_settings,
            save_settings,
            bin_dir,
        ])
        .run(tauri::generate_context!())
        .expect("Anwendung konnte nicht gestartet werden");
}
