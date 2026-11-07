//! Deno: JavaScript-Laufzeit fuer yt-dlp.
//!
//! YouTube stellt Pruefaufgaben (Signatur, „n“), die yt-dlp seit 2025.11 nur
//! noch in JavaScript loest. Ohne Laufzeit fehlen Zugaenge: Ein
//! altersbeschraenktes Video scheiterte trotz Cookies mit „age-restricted“,
//! andere kommen nur in schlechter Qualitaet. yt-dlp empfiehlt Deno; der Kern
//! laedt es wie ffmpeg einmal nach und nennt yt-dlp den Pfad. Das Loesungsskript
//! bringt yt-dlp selbst mit, Deno gibt einem Skript von sich aus weder Datei-
//! noch Netzzugriff.
//!
//! Fest genagelt wie ffmpeg, mit im Programm hinterlegten Pruefsummen der
//! ZIP-Archive (wie sie Deno je Release als `<asset>.zip.sha256sum`
//! veroeffentlicht). Startet die Datei hier nicht (zu altes System), bleibt sie
//! liegen -- yt-dlp meldet die Laufzeit dann als nicht verfuegbar, und es wird
//! nicht bei jedem Start neu geladen.
//!
//! Aktualisieren: `TAG` anheben, Pruefsummen aus den `.sha256sum`-Dateien des
//! Releases eintragen.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::tools::{
    check_sha256, exe_name, fetch_bytes, place_executable, probe_version, unzip_entry, ToolError,
    ToolResult,
};

/// Release-Tag von denoland/deno.
const TAG: &str = "v2.9.8";

/// (Asset ohne `.zip`, SHA-256 des Archivs) fuer die aktuelle Plattform.
fn pinned() -> Option<(&'static str, &'static str)> {
    let spec = match (std::env::consts::OS, std::env::consts::ARCH) {
        // Windows auf ARM laeuft mit dem x64-Build (Emulation)
        ("windows", _) => (
            "deno-x86_64-pc-windows-msvc",
            "cba84b26e0df5ae7f74fc66b86a95b866872a86292ebd84fa1e959927edd93bf",
        ),
        ("macos", "aarch64") => (
            "deno-aarch64-apple-darwin",
            "9f0bf660393da2eaff044cdc7444cb08c73f235849b61d224d825a194ce7c98e",
        ),
        ("macos", _) => (
            "deno-x86_64-apple-darwin",
            "4b2f5edd2639b7470f034238b321d7002d8dfb4726d8558f52c17afac370eb2c",
        ),
        ("linux", "aarch64") => (
            "deno-aarch64-unknown-linux-gnu",
            "d49596c20b9788cf8f5d8ddf644103d4bde56ed239753f0bdb92ba9361d51905",
        ),
        ("linux", _) => (
            "deno-x86_64-unknown-linux-gnu",
            "e890975bc2549807bbcc0b584ffafda72cc7c93349dd99c80483dd784f6c4609",
        ),
        _ => return None,
    };
    Some(spec)
}

fn asset_url(asset: &str) -> String {
    format!("https://github.com/denoland/deno/releases/download/{TAG}/{asset}.zip")
}

pub fn deno_path(bin_dir: &Path) -> PathBuf {
    bin_dir.join(exe_name("deno"))
}

/// "deno 2.9.8 (stable, release, x86_64-pc-windows-msvc)" -> "2.9.8"
fn parse_version(line: &str) -> Option<String> {
    let mut words = line.split_whitespace();
    if words.next()? != "deno" {
        return None;
    }
    words.next().map(str::to_string)
}

/// Version der vorhandenen Datei, sonst None (fehlt oder startet hier nicht).
pub fn installed_version(bin_dir: &Path) -> Option<String> {
    let path = deno_path(bin_dir);
    if !path.exists() {
        return None;
    }
    parse_version(&probe_version(&path, &["--version"])?)
}

/// Argumente fuer yt-dlp: die eigene Laufzeit, sobald sie daliegt. Fehlt sie,
/// sucht yt-dlp selbst nach einem installierten Deno.
pub fn runtime_args(deno: Option<&Path>) -> Vec<String> {
    match deno {
        Some(path) => vec![
            "--js-runtimes".into(),
            format!("deno:{}", path.to_string_lossy()),
        ],
        None => Vec::new(),
    }
}

/// Laedt Deno, wenn es fehlt: Archiv laden, gegen die hinterlegte Pruefsumme
/// pruefen, erst dann entpacken und ablegen.
pub async fn ensure(bin_dir: &Path) -> ToolResult<()> {
    let dest = deno_path(bin_dir);
    if dest.exists() {
        return Ok(());
    }
    let Some((asset, sha256)) = pinned() else {
        return Err(ToolError::Other(format!(
            "Für {} / {} liegt kein Deno-Build bereit",
            std::env::consts::OS,
            std::env::consts::ARCH
        )));
    };
    let zip = fetch_bytes(&asset_url(asset), Duration::from_secs(900)).await?;
    check_sha256(&zip, sha256)?;
    let data = unzip_entry(&zip, &exe_name("deno"))?;
    place_executable(&dest, &data)
}

/// Grobe Groesse des Nachladens fuer die Anzeige (MB, gepackt).
pub fn download_size_mb() -> u32 {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => 39,
        ("linux", "aarch64") => 40,
        ("linux", _) => 42,
        _ => 43,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuer_diese_plattform_gibt_es_einen_build() {
        let (asset, sha256) = pinned().expect("Plattform wird unterstützt");
        assert!(asset.starts_with("deno-"), "{asset}");
        assert_eq!(sha256.len(), 64, "{asset} hat keine SHA-256");
        let hex = |c: char| c.is_ascii_hexdigit() && !c.is_ascii_uppercase();
        assert!(sha256.chars().all(hex), "{asset}: klein geschriebenes Hex");
    }

    #[test]
    fn adresse_zeigt_auf_den_festgenagelten_tag() {
        let url = asset_url("deno-x86_64-unknown-linux-gnu");
        assert!(url.starts_with("https://github.com/denoland/deno/releases/download/"));
        assert!(url.contains(TAG));
        assert!(url.ends_with(".zip"));
    }

    #[test]
    fn version_lesen() {
        let line = "deno 2.9.8 (stable, release, x86_64-pc-windows-msvc)";
        assert_eq!(parse_version(line).as_deref(), Some("2.9.8"));
        assert_eq!(parse_version("node v24.1.0"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn laufzeit_nur_wenn_sie_daliegt() {
        assert!(runtime_args(None).is_empty());
        let args = runtime_args(Some(Path::new("/bin/deno")));
        assert_eq!(args, vec!["--js-runtimes", "deno:/bin/deno"]);
    }

    /// Laedt das festgenagelte Archiv, prueft es, entpackt und startet Deno.
    /// Braucht Netz: `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "braucht Netzzugang"]
    async fn holt_deno_und_prueft_es() {
        let id = uuid::Uuid::new_v4().simple();
        let dir = std::env::temp_dir().join(format!("mvl-deno-{id}"));
        ensure(&dir).await.expect("Download und Prüfung");
        let version = installed_version(&dir).expect("Deno läuft");
        assert_eq!(format!("v{version}"), TAG);
        std::fs::remove_dir_all(&dir).ok();
    }
}
