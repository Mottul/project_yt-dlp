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
    /// Wie yt-dlp startet: "nativ" oder "Python" (alte Macs). None = gar nicht.
    pub ytdlp_mode: Option<String>,
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
    let launcher = ytdlp::resolve_launcher(bin_dir);
    let ytdlp_version = launcher.as_ref().and_then(|l| l.version());
    let ytdlp_mode = launcher
        .filter(|_| ytdlp_version.is_some())
        .map(|l| l.kind().to_string());
    let ffmpeg_version = ffmpeg::installed_version(bin_dir);
    let ffmpeg_ready = ffmpeg_version.is_some();
    ToolsStatus {
        ytdlp_up_to_date: match (&ytdlp_version, &latest) {
            (Some(v), Some(l)) => Some(v == l),
            _ => None,
        },
        ytdlp_version,
        ytdlp_latest: latest,
        ytdlp_mode,
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

/// Holt yt-dlp und sorgt dafuer, dass es sich hier auch starten laesst.
///
/// Das fertige Programm laedt ueberall herunter -- auf Macs vor 10.15 startet es
/// aber nicht, weil yt-dlp dafuer keine Fassung mehr baut. Dann wird auf die
/// Python-Zipapp ausgewichen und die unbrauchbare Datei entfernt, damit sie
/// nicht wieder gewaehlt wird.
async fn install_ytdlp(bin_dir: &Path, tag: &str) -> Result<(), String> {
    let native = ytdlp::binary_path(bin_dir);
    let script = ytdlp::zipapp_path(bin_dir);

    // Lief hier bisher schon die Python-Fassung, gleich wieder so -- sonst
    // wuerde bei jeder Aktualisierung das nutzlose Programm mitgeladen.
    if script.exists() && !native.exists() {
        if let Some(python) = ytdlp::find_python() {
            ytdlp::install_zipapp(bin_dir, tag)
                .await
                .map_err(|e| e.to_string())?;
            let launcher = ytdlp::Launcher::Python { python, script };
            return match launcher.version() {
                Some(_) => Ok(()),
                None => Err("yt-dlp startet auch mit Python nicht".into()),
            };
        }
        return Err(ytdlp::NO_PYTHON_HINT.to_string());
    }

    ytdlp::install(bin_dir, tag).await.map_err(|e| e.to_string())?;
    if ytdlp::Launcher::Native(native.clone()).version().is_some() {
        return Ok(());
    }

    // Heruntergeladen, aber nicht startbar -- praktisch immer ein zu altes macOS.
    let Some(python) = ytdlp::find_python() else {
        std::fs::remove_file(&native).ok();
        return Err(ytdlp::NO_PYTHON_HINT.to_string());
    };
    ytdlp::install_zipapp(bin_dir, tag)
        .await
        .map_err(|e| e.to_string())?;
    let launcher = ytdlp::Launcher::Python { python, script };
    if launcher.version().is_none() {
        return Err("yt-dlp startet auch mit Python nicht".into());
    }
    std::fs::remove_file(&native).ok();
    Ok(())
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
                } else if let Err(err) = install_ytdlp(bin_dir, &tag).await {
                    error = Some(err);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Weg, den ein alter Mac bei JEDEM Start nimmt: liegt die Zipapp da und
    /// das fertige Programm nicht, darf letzteres nicht wieder geladen werden --
    /// es wuerde ohnehin nicht starten und jedes Mal ~35 MB kosten.
    ///
    /// Mit festem Tag statt ueber `latest_tag()`: GitHub weist haeufige Abrufe
    /// von /releases/latest zeitweise mit 403 ab, das hat mit dieser Frage
    /// nichts zu tun. Braucht Netz: `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "braucht Netzzugang"]
    async fn bleibt_auf_dem_python_weg() {
        const TAG: &str = "2026.08.19";
        assert!(
            ytdlp::find_python().is_some(),
            "für diesen Test wird Python 3.9+ gebraucht"
        );
        let dir = std::env::temp_dir().join(format!("mvl-weg-{}", uuid::Uuid::new_v4().simple()));
        ytdlp::install_zipapp(&dir, TAG)
            .await
            .expect("Zipapp einrichten");

        install_ytdlp(&dir, TAG).await.expect("muss durchlaufen");

        assert!(
            !ytdlp::binary_path(&dir).exists(),
            "das fertige Programm wurde nachgeladen, obwohl der Python-Weg läuft"
        );
        let status = read_status(&dir, Some(TAG.to_string()));
        assert_eq!(status.ytdlp_mode.as_deref(), Some("Python"));
        assert_eq!(status.ytdlp_version.as_deref(), Some(TAG));
        assert_eq!(status.ytdlp_up_to_date, Some(true));
        std::fs::remove_dir_all(&dir).ok();
    }
}
