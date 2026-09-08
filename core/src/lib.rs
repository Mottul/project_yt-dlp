//! Kern des MottulVideoLoader: haelt yt-dlp und ffmpeg bereit und fuehrt die
//! Downloads aus. Kennt weder Fenster noch Terminal -- die Oberflaechen setzen
//! darauf auf und bekommen Aenderungen ueber [`queue::JobSink`] gemeldet.

pub mod ffmpeg;
pub mod paths;
pub mod queue;
pub mod tools;
pub mod ytdlp;

pub use queue::{EnqueueRequest, Format, Job, JobSink, JobStatus, Manager};
pub use tools::{ToolError, ToolResult};

use std::path::Path;

/// Zustand der externen Werkzeuge.
#[derive(Debug, Clone, Default, serde::Serialize)]
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

/// Liest den Ist-Zustand der Werkzeuge im angegebenen Verzeichnis.
pub fn read_status(bin_dir: &Path, latest: Option<String>) -> ToolsStatus {
    let ytdlp_version = ytdlp::installed_version(bin_dir);
    let ffmpeg_version = ffmpeg::installed_version(bin_dir);
    let ffmpeg_ready = ffmpeg_version.is_some();
    ToolsStatus {
        ytdlp_up_to_date: match (&ytdlp_version, &latest) {
            (Some(v), Some(l)) => Some(v == l),
            _ => None,
        },
        ytdlp_version,
        ytdlp_latest: latest,
        ffmpeg_version,
        ffmpeg_ready,
        busy: false,
        last_error: None,
        pending_mb: if ffmpeg_ready {
            0
        } else {
            ffmpeg::download_size_mb()
        },
    }
}

/// Prueft auf eine neue yt-dlp-Version und laedt fehlende Werkzeuge nach.
///
/// `busy` meldet, ob gerade ein Download laeuft: dann wird yt-dlp nicht
/// ersetzt, weil das unter Windows an der laufenden Datei scheitert.
/// Fehler werden gesammelt statt geworfen -- ohne Netz soll die Anwendung
/// normal weiterlaufen.
pub async fn ensure_tools(bin_dir: &Path, downloads_running: bool) -> ToolsStatus {
    let mut error: Option<String> = None;
    let mut latest: Option<String> = None;

    match ytdlp::latest_tag().await {
        Ok(tag) => {
            latest = Some(tag.clone());
            if ytdlp::installed_version(bin_dir).as_deref() != Some(tag.as_str()) {
                if downloads_running {
                    error = Some("Download läuft — Aktualisierung später möglich".into());
                } else if let Err(err) = ytdlp::install(bin_dir, &tag).await {
                    error = Some(err.to_string());
                }
            }
        }
        Err(err) => error = Some(err.to_string()),
    }

    if let Err(err) = ffmpeg::ensure(bin_dir).await {
        let message = format!("ffmpeg: {err}");
        error = Some(match error {
            Some(prev) => format!("{prev}; {message}"),
            None => message,
        });
    }

    let mut status = read_status(bin_dir, latest);
    status.last_error = error;
    status
}
