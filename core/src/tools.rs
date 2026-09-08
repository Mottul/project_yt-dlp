//! Gemeinsame Verwaltung der externen Programme (yt-dlp, ffmpeg, ffprobe).
//!
//! Grundsatz: Was hier ausfuehrbar auf der Platte landet, wurde vorher gegen
//! eine SHA-256-Pruefsumme geprueft. Erst danach wird die Datei an ihren Platz
//! umbenannt -- ein Abbruch hinterlaesst nie eine halbe, startbare Datei.
//! Das ersetzt keine Signaturpruefung: es sichert die Uebertragung, nicht die
//! Herkunft des Releases.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

pub const USER_AGENT: &str = concat!("MottulVideoLoader/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Netzwerkfehler: {0}")]
    Http(String),
    #[error("HTTP {status} bei {url}")]
    Status { status: u16, url: String },
    #[error(
        "Prüfsumme stimmt nicht — Download verworfen (erwartet {expected}, erhalten {actual})"
    )]
    Checksum { expected: String, actual: String },
    #[error("Dateifehler: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

pub type ToolResult<T> = Result<T, ToolError>;

fn client(timeout: Duration) -> ToolResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .build()
        .map_err(|e| ToolError::Http(e.to_string()))
}

/// Laedt eine Adresse vollstaendig in den Speicher.
pub async fn fetch_bytes(url: &str, timeout: Duration) -> ToolResult<Vec<u8>> {
    let res = client(timeout)?
        .get(url)
        .send()
        .await
        .map_err(|e| ToolError::Http(e.to_string()))?;
    if !res.status().is_success() {
        return Err(ToolError::Status {
            status: res.status().as_u16(),
            url: url.to_string(),
        });
    }
    let bytes = res
        .bytes()
        .await
        .map_err(|e| ToolError::Http(e.to_string()))?;
    Ok(bytes.to_vec())
}

pub async fn fetch_text(url: &str, timeout: Duration) -> ToolResult<String> {
    let bytes = fetch_bytes(url, timeout).await?;
    String::from_utf8(bytes).map_err(|e| ToolError::Other(e.to_string()))
}

/// Liefert das Ziel einer Weiterleitung, ohne ihr zu folgen. Wird gebraucht,
/// um die neueste Version zu ermitteln, ohne die GitHub-API zu benutzen -- die
/// ist ohne Anmeldung auf 60 Anfragen je Stunde und IP begrenzt.
pub async fn redirect_target(url: &str, timeout: Duration) -> ToolResult<String> {
    let res = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| ToolError::Http(e.to_string()))?
        .get(url)
        .send()
        .await
        .map_err(|e| ToolError::Http(e.to_string()))?;

    res.headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .ok_or_else(|| {
            ToolError::Other(format!(
                "keine Weiterleitung erhalten (HTTP {})",
                res.status()
            ))
        })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Entpackt einen gzip-Strom vollstaendig.
pub fn gunzip(bytes: &[u8]) -> ToolResult<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|e| ToolError::Other(format!("gzip fehlerhaft: {e}")))?;
    Ok(out)
}

/// Schreibt Daten ausfuehrbar an ihren Platz -- erst daneben, dann umbenennen.
pub fn place_executable(dest: &Path, data: &[u8]) -> ToolResult<()> {
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = dest.with_extension(format!("{}.part", uuid::Uuid::new_v4().simple()));
    std::fs::write(&tmp, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    if let Err(err) = std::fs::rename(&tmp, dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}

/// Laedt eine Datei, prueft die Pruefsumme und legt sie erst dann ab.
///
/// `expected` bezieht sich immer auf die Datei, die am Ende ausgefuehrt wird --
/// bei `gzipped` also auf den entpackten Inhalt.
pub async fn download_verified(
    url: &str,
    expected: &str,
    dest: &Path,
    gzipped: bool,
    timeout: Duration,
) -> ToolResult<()> {
    let raw = fetch_bytes(url, timeout).await?;
    let data = if gzipped { gunzip(&raw)? } else { raw };
    let actual = sha256_hex(&data);
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(ToolError::Checksum {
            expected: expected.chars().take(12).collect(),
            actual: actual.chars().take(12).collect(),
        });
    }
    place_executable(dest, &data)
}

/// Verzeichnis fuer die verwalteten Programme (unter den App-Daten).
pub fn bin_dir(app_data: &Path) -> PathBuf {
    app_data.join("bin")
}

/// Dateiname eines Programms je Plattform.
pub fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

/// Startet ein Programm und liefert die erste Zeile der Ausgabe.
pub fn probe_version(bin: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args);
    hide_console(&mut cmd);
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .next()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

/// Unter Windows kein Konsolenfenster aufblitzen lassen.
pub fn hide_console(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_bekannter_wert() {
        // Leerer Eingabestrom -> bekannter SHA-256.
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn exe_name_je_plattform() {
        let name = exe_name("yt-dlp");
        if cfg!(windows) {
            assert_eq!(name, "yt-dlp.exe");
        } else {
            assert_eq!(name, "yt-dlp");
        }
    }

    #[test]
    fn gunzip_stellt_original_wieder_her() {
        use flate2::write::GzEncoder;
        use std::io::Write;
        let mut enc = GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(b"Hallo Welt").unwrap();
        let packed = enc.finish().unwrap();
        assert_eq!(gunzip(&packed).unwrap(), b"Hallo Welt");
    }

    #[test]
    fn gunzip_meldet_kaputte_daten() {
        assert!(gunzip(b"kein gzip").is_err());
    }

    #[test]
    fn place_executable_ersetzt_atomar() {
        let dir = std::env::temp_dir().join(format!("mvl-test-{}", uuid::Uuid::new_v4().simple()));
        let dest = dir.join("werkzeug");
        place_executable(&dest, b"eins").unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"eins");
        place_executable(&dest, b"zwei").unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"zwei");
        // Keine Reste vom Zwischenschritt.
        let leftovers = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(leftovers, 1);
        std::fs::remove_dir_all(&dir).ok();
    }
}
