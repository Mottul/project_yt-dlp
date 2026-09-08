//! Einstellungen der Terminal-Variante -- dieselbe Datei, die auch die
//! Fenster-App benutzt, damit beide dieselbe Wahl kennen.

use std::path::{Path, PathBuf};

use mottul_video_core::{paths, Format};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub output_dir: String,
    pub format: Format,
    pub max_height: Option<u32>,
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: paths::default_output_dir().to_string_lossy().into_owned(),
            format: Format::Video,
            max_height: Some(1080),
            auto_update: true,
        }
    }
}

impl Settings {
    pub fn file(data_dir: &Path) -> PathBuf {
        data_dir.join("settings.json")
    }

    /// Liest die Datei; fehlt oder klemmt sie, gelten die Vorgaben.
    pub fn load(data_dir: &Path) -> Self {
        let mut settings: Self = std::fs::read_to_string(Self::file(data_dir))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        // Die Fenster-App legt beim ersten Start einen leeren Zielordner an.
        if settings.output_dir.trim().is_empty() {
            settings.output_dir = paths::default_output_dir().to_string_lossy().into_owned();
        }
        settings
    }

    pub fn save(&self, data_dir: &Path) {
        let _ = std::fs::create_dir_all(data_dir);
        if let Ok(raw) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::file(data_dir), raw);
        }
    }

    pub fn format_label(&self) -> &'static str {
        match self.format {
            Format::Video => "Video MP4",
            Format::AudioMp3 => "Nur Ton MP3",
            Format::AudioM4a => "Nur Ton M4A",
        }
    }

    /// Kurzfassung fuer die Kopfzeile.
    pub fn summary(&self) -> String {
        match (self.format, self.max_height) {
            (Format::Video, Some(h)) => format!("{} bis {h}p", self.format_label()),
            (Format::Video, None) => format!("{}, beste Auflösung", self.format_label()),
            _ => self.format_label().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vorgabe_hat_einen_zielordner() {
        assert!(!Settings::default().output_dir.trim().is_empty());
    }

    #[test]
    fn kurzfassung_nennt_die_begrenzung_nur_bei_video() {
        let mut s = Settings::default();
        assert_eq!(s.summary(), "Video MP4 bis 1080p");
        s.max_height = None;
        assert_eq!(s.summary(), "Video MP4, beste Auflösung");
        s.format = Format::AudioMp3;
        assert_eq!(s.summary(), "Nur Ton MP3");
    }

    #[test]
    fn leerer_zielordner_wird_ersetzt() {
        let dir = std::env::temp_dir().join(format!("mvl-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            Settings::file(&dir),
            r#"{"outputDir":"","format":"video","maxHeight":720,"autoUpdate":false}"#,
        )
        .unwrap();
        let loaded = Settings::load(&dir);
        assert!(!loaded.output_dir.is_empty());
        assert_eq!(loaded.max_height, Some(720));
        assert!(!loaded.auto_update);
        std::fs::remove_dir_all(&dir).ok();
    }
}
