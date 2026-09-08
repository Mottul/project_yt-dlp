//! Wo die App ihre Daten ablegt. Bewusst dieselben Orte, die Tauri waehlt --
//! Fenster-App und Terminal-Variante teilen sich damit Einstellungen und die
//! einmal geladenen Werkzeuge.

use std::path::PathBuf;

/// Kennung der Anwendung; bestimmt den Ordnernamen.
pub const APP_ID: &str = "com.mottul.videoloader";

/// Datenverzeichnis der Anwendung.
///
/// - Windows: `%APPDATA%\<APP_ID>`
/// - macOS: `~/Library/Application Support/<APP_ID>`
/// - sonst: `$XDG_DATA_HOME/<APP_ID>` bzw. `~/.local/share/<APP_ID>`
pub fn data_dir() -> PathBuf {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| home().map(|h| h.join(".local/share")))
    };
    base.unwrap_or_else(std::env::temp_dir).join(APP_ID)
}

/// Voreingestellter Zielordner fuer fertige Dateien.
pub fn default_output_dir() -> PathBuf {
    match home() {
        Some(h) if h.join("Downloads").is_dir() => h.join("Downloads"),
        Some(h) => h,
        None => std::env::temp_dir(),
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datenverzeichnis_endet_auf_die_kennung() {
        let dir = data_dir();
        assert_eq!(dir.file_name().unwrap(), APP_ID);
        assert!(dir.is_absolute(), "muss ein absoluter Pfad sein: {dir:?}");
    }

    #[test]
    fn zielordner_ist_absolut() {
        assert!(default_output_dir().is_absolute());
    }
}
