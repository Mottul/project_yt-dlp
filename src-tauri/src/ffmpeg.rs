//! ffmpeg und ffprobe: werden beim ersten Bedarf geladen, statt im Installer
//! mitgeliefert zu werden -- das haelt das Paket bei rund 10 MB.
//!
//! Bewusst auf eine FESTE Version genagelt, mit im Programm hinterlegten
//! Pruefsummen. Anders als yt-dlp muss ffmpeg nicht aktuell gehalten werden:
//! Zusammenfuegen und Umwandeln aendern sich nicht, wenn eine Videoplattform
//! ihre Seiten umbaut. Eine feste Version erlaubt dafuer die staerkste
//! Pruefung -- die erwartete Pruefsumme steht im Programm, nicht auf demselben
//! Server wie die Datei.
//!
//! Aktualisieren: Version unten anheben, Pruefsummen der ENTPACKTEN Dateien
//! eintragen (`gunzip -c <asset>.gz | sha256sum`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::tools::{download_verified, exe_name, probe_version, ToolResult};

/// Immutabler Release-Tag von eugeneware/ffmpeg-static.
const TAG: &str = "b6.1.1";

/// (Programm, Asset-Name, SHA-256 der entpackten Datei) je Plattform.
struct Pinned {
    stem: &'static str,
    asset: &'static str,
    sha256: &'static str,
}

/// Fuer die aktuelle Plattform benoetigte Programme.
fn pinned() -> Option<[Pinned; 2]> {
    let (ffmpeg, ffprobe) = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", _) => (
            (
                "ffmpeg-win32-x64",
                "04e1307997530f9cf2fe35cba2ca7e8875ca91da02f89d6c7243df819c94ad00",
            ),
            (
                "ffprobe-win32-x64",
                "3a7e2dc003dc2cd1472827e4c7c4f056ae1ae0ae7c5bbc580c99b49827351ba4",
            ),
        ),
        ("macos", "aarch64") => (
            (
                "ffmpeg-darwin-arm64",
                "a90e3db6a3fd35f6074b013f948b1aa45b31c6375489d39e572bea3f18336584",
            ),
            (
                "ffprobe-darwin-arm64",
                "bb2db6f5d8cef919da12fbf592119a987202a8c060a886f3cab091f9cab90b64",
            ),
        ),
        ("macos", _) => (
            (
                "ffmpeg-darwin-x64",
                "ebdddc936f61e14049a2d4b549a412b8a40deeff6540e58a9f2a2da9e6b18894",
            ),
            (
                "ffprobe-darwin-x64",
                "fa3add0ce901f7241abe0dfc0155d958fc834aca3f8ce61f87cc712ae669c1e0",
            ),
        ),
        ("linux", "aarch64") => (
            (
                "ffmpeg-linux-arm64",
                "6bb182d0d75d23028db82e9e4f723ca69b853d055698486e6984ddb2c06fb8ce",
            ),
            (
                "ffprobe-linux-arm64",
                "d17ae9b4c297d48e2521ba14e417bb0537c6ff77c584cdbcd6bb0d8d0307a2e8",
            ),
        ),
        ("linux", _) => (
            (
                "ffmpeg-linux-x64",
                "e7e7fb30477f717e6f55f9180a70386c62677ef8a4d4d1a5d948f4098aa3eb99",
            ),
            (
                "ffprobe-linux-x64",
                "4f231a1960d83e403d08f7971e271707bec278a9ae18e21b8b5b03186668450d",
            ),
        ),
        _ => return None,
    };
    Some([
        Pinned {
            stem: "ffmpeg",
            asset: ffmpeg.0,
            sha256: ffmpeg.1,
        },
        Pinned {
            stem: "ffprobe",
            asset: ffprobe.0,
            sha256: ffprobe.1,
        },
    ])
}

fn asset_url(asset: &str) -> String {
    format!("https://github.com/eugeneware/ffmpeg-static/releases/download/{TAG}/{asset}.gz")
}

pub fn ffmpeg_path(bin_dir: &Path) -> PathBuf {
    bin_dir.join(exe_name("ffmpeg"))
}

/// Version der vorhandenen ffmpeg-Datei, sonst None.
pub fn installed_version(bin_dir: &Path) -> Option<String> {
    let path = ffmpeg_path(bin_dir);
    if !path.exists() {
        return None;
    }
    let line = probe_version(&path, &["-hide_banner", "-version"])?;
    // "ffmpeg version 7.0.2-static https://..." -> "7.0.2-static"
    line.split_whitespace()
        .nth(2)
        .map(|v| v.to_string())
        .or(Some(line))
}

/// Laedt fehlende Programme nach. Vorhandene werden nicht angefasst -- eine
/// feste Version braucht keine Aktualisierung.
pub async fn ensure(bin_dir: &Path) -> ToolResult<()> {
    let Some(specs) = pinned() else {
        return Err(crate::tools::ToolError::Other(format!(
            "Für {} / {} liegt kein ffmpeg-Build bereit",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));
    };
    for spec in specs {
        let dest = bin_dir.join(exe_name(spec.stem));
        if dest.exists() {
            continue;
        }
        download_verified(
            &asset_url(spec.asset),
            spec.sha256,
            &dest,
            true,
            Duration::from_secs(900),
        )
        .await?;
    }
    Ok(())
}

/// Grobe Groesse des Nachladens fuer die Anzeige (MB, gepackt).
pub fn download_size_mb() -> u32 {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", _) => 59,
        ("macos", "aarch64") => 38,
        ("macos", _) => 51,
        _ => 60,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuer_diese_plattform_gibt_es_einen_build() {
        let specs = pinned().expect("Plattform wird unterstützt");
        assert_eq!(specs[0].stem, "ffmpeg");
        assert_eq!(specs[1].stem, "ffprobe");
    }

    #[test]
    fn pruefsummen_haben_die_richtige_form() {
        let specs = pinned().unwrap();
        for spec in specs {
            assert_eq!(spec.sha256.len(), 64, "{} hat keine SHA-256", spec.asset);
            assert!(
                spec.sha256
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
                "{} ist nicht klein geschriebenes Hex",
                spec.asset
            );
        }
    }

    /// Laedt die festgenagelten Dateien und prueft sie gegen die im Programm
    /// hinterlegten Pruefsummen. Braucht Netz.
    #[tokio::test]
    #[ignore = "braucht Netzzugang"]
    async fn holt_ffmpeg_und_prueft_es() {
        let dir = std::env::temp_dir().join(format!("mvl-ff-{}", uuid::Uuid::new_v4().simple()));
        ensure(&dir).await.expect("Download und Prüfung");
        let version = installed_version(&dir).expect("ffmpeg läuft");
        assert!(!version.is_empty());
        assert!(dir.join(exe_name("ffprobe")).exists(), "ffprobe fehlt");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn adresse_zeigt_auf_den_festgenagelten_tag() {
        let url = asset_url("ffmpeg-linux-x64");
        assert!(url.starts_with("https://github.com/eugeneware/ffmpeg-static/releases/download/"));
        assert!(url.contains(TAG));
        assert!(url.ends_with(".gz"));
    }
}
