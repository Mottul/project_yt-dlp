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

/// Schreibt eine Datei so, dass sie nie halb daliegt: erst daneben, dann
/// umbenennen (ersetzt die alte in einem Schritt, auch unter Windows).
///
/// Fuer `settings.json`: Ein Absturz mitten im Schreiben hinterliess sonst
/// eine kaputte Datei, die beim naechsten Start als „keine Einstellungen“
/// gilt -- der Zielordner waere weg.
pub fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let written = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(data)?;
        file.sync_all()
    })();
    let result = written.and_then(|_| std::fs::rename(&tmp, path));
    if result.is_err() {
        std::fs::remove_file(&tmp).ok();
    }
    result
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

    #[test]
    fn schreibt_ganz_oder_gar_nicht() {
        let dir =
            std::env::temp_dir().join(format!("mvl-atomar-{}", uuid::Uuid::new_v4().simple()));
        let file = dir.join("settings.json");
        write_atomic(&file, b"{\"a\":1}").unwrap();
        write_atomic(&file, b"{\"a\":2}").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"{\"a\":2}");
        // Keine Zwischendateien uebrig
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

        // Ziel ist ein Ordner: Fehler, und die Zwischendatei raeumt sich weg
        let blocked = dir.join("ordner");
        std::fs::create_dir_all(blocked.join("drin")).unwrap();
        assert!(write_atomic(&blocked, b"x").is_err());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }
}
