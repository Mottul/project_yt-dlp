//! MottulVideoLoader: schlanke Oberflaeche fuer yt-dlp.
//!
//! Der Rust-Teil haelt die Werkzeuge (yt-dlp, ffmpeg) aktuell und fuehrt die
//! Downloads aus; die Oberflaeche zeigt nur an und nimmt Eingaben entgegen.

mod ffmpeg;
mod queue;
mod tools;
mod ytdlp;

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager as _, State};

pub const STATUS_EVENT: &str = "tools-status";

/// Zustand der externen Werkzeuge, wie ihn die Oberflaeche anzeigt.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolsStatus {
    pub ytdlp_version: Option<String>,
    pub ytdlp_latest: Option<String>,
    /// None = unbekannt (kein Netz oder nichts installiert).
    pub ytdlp_up_to_date: Option<bool>,
    pub ffmpeg_version: Option<String>,
    pub ffmpeg_ready: bool,
    /// Pruefung oder Download laeuft gerade.
    pub busy: bool,
    /// Was zuletzt schiefging (z. B. kein Netz), sonst None.
    pub last_error: Option<String>,
    /// Ungefaehre Groesse des noch ausstehenden Nachladens in MB.
    pub pending_mb: u32,
}

/// Vom Benutzer gewaehlte Einstellungen; liegen als JSON bei den App-Daten.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub output_dir: String,
    pub format: queue::Format,
    pub max_height: Option<u32>,
    /// yt-dlp beim Start pruefen und bei Bedarf aktualisieren.
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: String::new(),
            format: queue::Format::Video,
            max_height: Some(1080),
            auto_update: true,
        }
    }
}

pub struct AppState {
    pub bin_dir: PathBuf,
    pub settings_file: PathBuf,
    pub manager: queue::Manager,
    pub status: Mutex<ToolsStatus>,
}

impl AppState {
    fn snapshot(&self) -> ToolsStatus {
        let mut status = self.status.lock().unwrap().clone();
        status.ytdlp_version = ytdlp::installed_version(&self.bin_dir);
        status.ffmpeg_version = ffmpeg::installed_version(&self.bin_dir);
        status.ffmpeg_ready = status.ffmpeg_version.is_some();
        status.ytdlp_up_to_date = match (&status.ytdlp_version, &status.ytdlp_latest) {
            (Some(v), Some(l)) => Some(v == l),
            _ => None,
        };
        status.pending_mb = if status.ffmpeg_ready {
            0
        } else {
            ffmpeg::download_size_mb()
        };
        status
    }

    fn publish(&self, app: &AppHandle) {
        let _ = app.emit(STATUS_EVENT, self.snapshot());
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
fn tools_status(state: State<'_, AppState>) -> ToolsStatus {
    state.snapshot()
}

/// Prueft yt-dlp auf eine neue Version und laedt fehlende Werkzeuge nach.
#[tauri::command]
async fn ensure_tools(app: AppHandle) -> Result<ToolsStatus, String> {
    let already_busy = {
        let state = app.state::<AppState>();
        let mut status = state.status.lock().unwrap();
        if status.busy {
            true
        } else {
            status.busy = true;
            status.last_error = None;
            false
        }
    };
    if already_busy {
        return Ok(app.state::<AppState>().snapshot());
    }
    {
        let state = app.state::<AppState>();
        state.publish(&app);
    }

    let (bin_dir, busy_downloads) = {
        let state = app.state::<AppState>();
        (state.bin_dir.clone(), state.manager.busy())
    };

    let mut error: Option<String> = None;
    let mut latest: Option<String> = None;

    match ytdlp::latest_tag().await {
        Ok(tag) => {
            latest = Some(tag.clone());
            let installed = ytdlp::installed_version(&bin_dir);
            if installed.as_deref() != Some(tag.as_str()) {
                // Waehrend eines laufenden Downloads laesst sich die Datei unter
                // Windows nicht ersetzen -- dann beim naechsten Start.
                if busy_downloads {
                    error = Some("Download läuft — Aktualisierung später möglich".into());
                } else if let Err(err) = ytdlp::install(&bin_dir, &tag).await {
                    error = Some(err.to_string());
                }
            }
        }
        Err(err) => error = Some(err.to_string()),
    }

    if let Err(err) = ffmpeg::ensure(&bin_dir).await {
        let message = err.to_string();
        error = Some(match error {
            Some(prev) => format!("{prev}; ffmpeg: {message}"),
            None => format!("ffmpeg: {message}"),
        });
    }

    let state = app.state::<AppState>();
    {
        let mut status = state.status.lock().unwrap();
        status.busy = false;
        status.last_error = error;
        if latest.is_some() {
            status.ytdlp_latest = latest;
        }
    }
    state.publish(&app);
    Ok(state.snapshot())
}

#[tauri::command]
fn enqueue(
    app: AppHandle,
    req: queue::EnqueueRequest,
    state: State<'_, AppState>,
) -> Result<String, String> {
    if req.url.trim().is_empty() {
        return Err("Keine Adresse angegeben".into());
    }
    if req.output_dir.trim().is_empty() {
        return Err("Kein Zielordner gewählt".into());
    }
    Ok(state.manager.enqueue(&app, req, state.bin_dir.clone()))
}

#[tauri::command]
fn list_jobs(state: State<'_, AppState>) -> Vec<queue::Job> {
    state.manager.list()
}

#[tauri::command]
fn cancel_job(app: AppHandle, id: String, state: State<'_, AppState>) {
    state.manager.cancel(&app, &id);
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

/// Wo die Werkzeuge liegen -- fuer die Anzeige in den Hinweisen.
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
            let state = AppState {
                bin_dir: tools::bin_dir(&data_dir),
                settings_file: data_dir.join("settings.json"),
                manager: queue::Manager::default(),
                status: Mutex::new(ToolsStatus::default()),
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
