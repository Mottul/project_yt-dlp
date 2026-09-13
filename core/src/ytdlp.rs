//! yt-dlp: Version ermitteln, pruefen und aktuell halten.
//!
//! Anders als ffmpeg MUSS yt-dlp aktuell bleiben -- aendert eine Plattform ihre
//! Seiten, hilft nur eine neue Version. Deshalb: beim Start die neueste stabile
//! Version ermitteln und bei Abweichung ersetzen.
//!
//! Die Version kommt aus der Weiterleitung von /releases/latest, nicht aus der
//! GitHub-API: die ist ohne Anmeldung auf 60 Anfragen je Stunde und IP begrenzt
//! und schlaegt hinter einem Firmen-NAT regelmaessig fehl.
//!
//! Zwei Wege, yt-dlp zu starten:
//!  * NATIV -- das fertige Programm aus dem Release. Der Normalfall.
//!  * PYTHON -- die plattformunabhaengige Zipapp, gestartet mit einem auf dem
//!    Rechner vorhandenen Python 3. Noetig auf Macs vor 10.15: dafuer baut
//!    yt-dlp seit dem Wegfall von `yt-dlp_macos_legacy` keine Fassung mehr.
//!    Das fertige Programm laedt dort zwar herunter, startet aber nicht.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::tools::{
    download_verified, exe_name, fetch_text, probe_version, redirect_target, ToolError, ToolResult,
};

const LATEST_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest";

/// Plattformunabhaengige Zipapp im Release (braucht Python 3.9+).
const ZIPAPP_ASSET: &str = "yt-dlp";

/// Kleinste Python-Fassung, mit der yt-dlp laeuft.
const MIN_PYTHON: (u32, u32) = (3, 9);

/// Was zu tun ist, wenn das fertige Programm hier nicht startet und auch kein
/// Python bereitsteht. Mehrzeilig -- die Oberflaechen geben es Zeile fuer Zeile aus.
pub const NO_PYTHON_HINT: &str = "\
yt-dlp wird nur noch für macOS 10.15 oder neuer gebaut.
Auf älteren Macs läuft es über Python 3 — einmalig einzurichten:
  1. python.org/downloads/macos öffnen und Python 3 installieren
     (mindestens 3.9; eine Fassung wählen, die macOS 10.13 noch nennt).
  2. Danach im Ordner „Python 3.x“ einmal „Install Certificates.command“
     doppelklicken, sonst schlagen verschlüsselte Verbindungen fehl.
  3. MottulVideoLoader erneut starten — den Rest erledigt er selbst.";

/// Asset des eigenstaendigen Builds je Plattform (kein Python noetig).
fn asset_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "yt-dlp.exe",
        "macos" => "yt-dlp_macos",
        _ => "yt-dlp_linux",
    }
}

fn release_url(tag: &str, file: &str) -> String {
    format!("https://github.com/yt-dlp/yt-dlp/releases/download/{tag}/{file}")
}

/// Release-Tags heissen 2025.01.26, optional mit vierter Zahl. Streng pruefen:
/// der Tag wird in eine Adresse eingesetzt.
pub fn is_valid_tag(tag: &str) -> bool {
    let parts: Vec<&str> = tag.split('.').collect();
    if !(3..=4).contains(&parts.len()) {
        return false;
    }
    let widths = [4usize, 2, 2];
    for (i, part) in parts.iter().enumerate() {
        if !part.chars().all(|c| c.is_ascii_digit()) || part.is_empty() {
            return false;
        }
        if i < 3 && part.len() != widths[i] {
            return false;
        }
    }
    true
}

/// Zieht die Version aus der Weiterleitung von /releases/latest. Zeigt sie
/// woandershin, wird nichts geladen -- eine umgebogene Weiterleitung darf nie
/// bestimmen, woher das Programm kommt.
pub fn tag_from_release_url(url: &str) -> Option<String> {
    let rest = url
        .trim()
        .strip_prefix("https://github.com/yt-dlp/yt-dlp/releases/tag/")?;
    if rest.contains('/') || rest.contains('?') || rest.contains('#') || rest.is_empty() {
        return None;
    }
    is_valid_tag(rest).then(|| rest.to_string())
}

/// SHA2-256SUMS des Releases: je Zeile "<hash>  <dateiname>".
pub fn parse_checksums(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(hash), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        if parts.next().is_some()
            || hash.len() != 64
            || !hash.chars().all(|c| c.is_ascii_hexdigit())
        {
            continue;
        }
        map.insert(
            name.trim_start_matches('*').to_string(),
            hash.to_ascii_lowercase(),
        );
    }
    map
}

pub fn binary_path(bin_dir: &Path) -> PathBuf {
    bin_dir.join(exe_name("yt-dlp"))
}

/// Ablage der Zipapp. Bewusst ein anderer Name als das fertige Programm, damit
/// beide nebeneinander liegen koennen.
pub fn zipapp_path(bin_dir: &Path) -> PathBuf {
    bin_dir.join("yt-dlp.pyz")
}

/// Wie yt-dlp gestartet wird.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launcher {
    /// Das fertige Programm aus dem Release.
    Native(PathBuf),
    /// Die Zipapp, gestartet mit dem gefundenen Python 3.
    Python { python: PathBuf, script: PathBuf },
}

impl Launcher {
    /// Das Programm, das gestartet wird.
    pub fn program(&self) -> &Path {
        match self {
            Launcher::Native(path) => path,
            Launcher::Python { python, .. } => python,
        }
    }

    /// Argumente VOR den eigentlichen yt-dlp-Argumenten.
    pub fn prefix(&self) -> Vec<String> {
        match self {
            Launcher::Native(_) => Vec::new(),
            Launcher::Python { script, .. } => vec![script.to_string_lossy().into_owned()],
        }
    }

    /// Kurzwort fuer die Anzeige.
    pub fn kind(&self) -> &'static str {
        match self {
            Launcher::Native(_) => "nativ",
            Launcher::Python { .. } => "Python",
        }
    }

    /// Version -- und zugleich die Probe, ob sich yt-dlp hier ueberhaupt starten laesst.
    pub fn version(&self) -> Option<String> {
        let prefix = self.prefix();
        let mut args: Vec<&str> = prefix.iter().map(String::as_str).collect();
        args.push("--version");
        probe_version(self.program(), &args)
    }
}

/// Welcher Weg auf diesem Rechner bereitsteht -- ohne das Programm zu starten.
///
/// Das fertige Programm hat Vorrang. Konnte es beim Einrichten nicht gestartet
/// werden, wurde es entfernt; dann bleibt die Zipapp uebrig.
pub fn resolve_launcher(bin_dir: &Path) -> Option<Launcher> {
    let native = binary_path(bin_dir);
    if native.exists() {
        return Some(Launcher::Native(native));
    }
    let script = zipapp_path(bin_dir);
    if script.exists() {
        return find_python().map(|python| Launcher::Python { python, script });
    }
    None
}

/// Installierte Version, sonst None (fehlt oder laesst sich nicht starten).
pub fn installed_version(bin_dir: &Path) -> Option<String> {
    resolve_launcher(bin_dir).and_then(|l| l.version())
}

/// "Python 3.11.6" -> (3, 11). Getrennt vom Aufruf, damit pruefbar.
fn parse_python_version(text: &str) -> Option<(u32, u32)> {
    let rest = text.trim().strip_prefix("Python ")?;
    let mut parts = rest.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Version eines Python-Programms.
fn python_version(path: &Path) -> Option<(u32, u32)> {
    parse_python_version(&probe_version(path, &["--version"])?)
}

/// Uebliche Orte fuer Python 3. Der Doppelklick-Start bringt ein knappes PATH
/// mit, deshalb stehen die festen Pfade mit in der Liste.
fn python_candidates() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let names: &[&str] = if cfg!(windows) {
        &["python3", "python"]
    } else {
        &["python3"]
    };
    out.extend(names.iter().map(PathBuf::from));
    for fixed in [
        "/usr/local/bin/python3",
        "/opt/homebrew/bin/python3",
        "/usr/bin/python3",
    ] {
        out.push(PathBuf::from(fixed));
    }
    // Installationen von python.org liegen als Framework -- neueste zuerst.
    let framework = Path::new("/Library/Frameworks/Python.framework/Versions");
    if let Ok(entries) = std::fs::read_dir(framework) {
        let mut versions: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("bin/python3").exists())
            .collect();
        versions.sort();
        versions.reverse();
        out.extend(versions.into_iter().map(|p| p.join("bin/python3")));
    }
    out
}

/// Erstes brauchbares Python 3 auf diesem Rechner.
pub fn find_python() -> Option<PathBuf> {
    let mut seen: Vec<PathBuf> = Vec::new();
    for candidate in python_candidates() {
        if seen.contains(&candidate) {
            continue;
        }
        seen.push(candidate.clone());
        if let Some(version) = python_version(&candidate) {
            if version >= MIN_PYTHON {
                return Some(candidate);
            }
        }
    }
    None
}

/// Neueste stabile Version laut GitHub.
pub async fn latest_tag() -> ToolResult<String> {
    let target = redirect_target(LATEST_URL, Duration::from_secs(20)).await?;
    tag_from_release_url(&target)
        .ok_or_else(|| ToolError::Other(format!("unerwartete Release-Adresse: {target}")))
}

/// Laedt ein Release-Asset, prueft es und legt es ab.
///
/// Der Tag wird von aussen hereingereicht, damit Pruefsummen und Programm aus
/// demselben Release stammen -- ein neues Release mitten im Vorgang kann so
/// nicht zu einer Fehlpaarung fuehren.
async fn install_asset(tag: &str, asset: &str, dest: &Path) -> ToolResult<()> {
    let sums = fetch_text(&release_url(tag, "SHA2-256SUMS"), Duration::from_secs(30)).await?;
    let expected = parse_checksums(&sums)
        .remove(asset)
        .ok_or_else(|| ToolError::Other(format!("keine Prüfsumme für {asset} im Release {tag}")))?;

    download_verified(
        &release_url(tag, asset),
        &expected,
        dest,
        false,
        Duration::from_secs(900),
    )
    .await
}

/// Laedt das fertige Programm der Plattform.
pub async fn install(bin_dir: &Path, tag: &str) -> ToolResult<()> {
    install_asset(tag, asset_name(), &binary_path(bin_dir)).await
}

/// Laedt die Python-Zipapp (fuer Rechner, auf denen das fertige Programm nicht startet).
pub async fn install_zipapp(bin_dir: &Path, tag: &str) -> ToolResult<()> {
    install_asset(tag, ZIPAPP_ASSET, &zipapp_path(bin_dir)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gueltige_tags() {
        for tag in ["2025.01.26", "2024.12.13", "2026.08.19", "2025.01.26.1"] {
            assert!(is_valid_tag(tag), "{tag} sollte gültig sein");
        }
    }

    #[test]
    fn ungueltige_tags() {
        for tag in [
            "",
            "latest",
            "2025.1.26",
            "25.01.26",
            "2025.01",
            "2025.01.26.x",
            "2025.01.26.",
            "../etc",
        ] {
            assert!(!is_valid_tag(tag), "{tag} sollte ungültig sein");
        }
    }

    #[test]
    fn version_aus_der_weiterleitung() {
        assert_eq!(
            tag_from_release_url("https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19")
                .as_deref(),
            Some("2026.08.19")
        );
        assert_eq!(
            tag_from_release_url("  https://github.com/yt-dlp/yt-dlp/releases/tag/2025.01.26.1  ")
                .as_deref(),
            Some("2025.01.26.1")
        );
    }

    #[test]
    fn weiterleitung_woandershin_wird_verweigert() {
        for url in [
            "https://github.com/boese/yt-dlp/releases/tag/2026.08.19",
            "https://github.com/yt-dlp/yt-dlp-evil/releases/tag/2026.08.19",
            "http://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19",
            "https://evil.example/github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19",
            "https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19/../../evil",
            "https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19?x=1",
            "https://github.com/yt-dlp/yt-dlp/releases/tag/latest",
            "",
        ] {
            assert!(
                tag_from_release_url(url).is_none(),
                "{url} hätte abgelehnt werden müssen"
            );
        }
    }

    #[test]
    fn pruefsummen_zuordnen() {
        let text = "\
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  yt-dlp
5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03  yt-dlp.exe
d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592  yt-dlp_macos";
        let map = parse_checksums(text);
        assert_eq!(map.len(), 3);
        assert_eq!(
            map.get("yt-dlp.exe").unwrap(),
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }

    #[test]
    fn pruefsummen_ignorieren_muell() {
        let map = parse_checksums("kurz  yt-dlp.exe\nKommentar\n\n  \nzzz  datei");
        assert!(map.is_empty());
    }

    #[test]
    fn pruefsummen_vertragen_binaermarkierung_und_crlf() {
        let map = parse_checksums(
            "\r\n5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03 *yt-dlp.exe\r\n",
        );
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("yt-dlp.exe"));
    }

    /// Ein bekanntes, unveraenderliches Release vollstaendig durchspielen:
    /// Pruefsummen holen, Datei laden, vergleichen, ablegen, starten.
    /// Braucht Netz: `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "braucht Netzzugang"]
    async fn laedt_bekanntes_release_und_prueft_es() {
        const TAG: &str = "2026.08.19";
        let dir = std::env::temp_dir().join(format!("mvl-net-{}", uuid::Uuid::new_v4().simple()));
        install(&dir, TAG).await.expect("Download und Prüfung");
        assert_eq!(
            installed_version(&dir).as_deref(),
            Some(TAG),
            "installierte Version"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Eine falsche Pruefsumme muss den Download verwerfen -- und nichts ablegen.
    #[tokio::test]
    #[ignore = "braucht Netzzugang"]
    async fn falsche_pruefsumme_wird_abgelehnt() {
        let dir = std::env::temp_dir().join(format!("mvl-bad-{}", uuid::Uuid::new_v4().simple()));
        let dest = dir.join(exe_name("yt-dlp"));
        let err = crate::tools::download_verified(
            &release_url("2026.08.19", asset_name()),
            "0000000000000000000000000000000000000000000000000000000000000000",
            &dest,
            false,
            Duration::from_secs(900),
        )
        .await
        .expect_err("muss scheitern");
        assert!(
            matches!(err, ToolError::Checksum { .. }),
            "unerwarteter Fehler: {err}"
        );
        assert!(!dest.exists(), "nichts darf abgelegt worden sein");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Die Versionsermittlung ueber die Weiterleitung. Getrennt, weil GitHub
    /// haeufige Abrufe derselben Adresse zeitweise mit 403 abweist -- das
    /// betrifft nur diese Abfrage, nicht die Release-Downloads.
    #[tokio::test]
    #[ignore = "braucht Netzzugang"]
    async fn ermittelt_die_neueste_version() {
        let tag = latest_tag().await.expect("Version ermittelbar");
        assert!(is_valid_tag(&tag), "unerwartete Version: {tag}");
    }

    #[test]
    fn python_versionen_lesen() {
        assert_eq!(parse_python_version("Python 3.11.6"), Some((3, 11)));
        assert_eq!(parse_python_version("  Python 3.9.13  "), Some((3, 9)));
        assert_eq!(parse_python_version("Python 2.7.16"), Some((2, 7)));
        for schrott in ["", "python3", "Python", "Python x.y", "PyPy 3.9"] {
            assert_eq!(parse_python_version(schrott), None, "{schrott}");
        }
    }

    #[test]
    fn launcher_baut_den_aufruf() {
        let native = Launcher::Native(PathBuf::from("/bin/yt-dlp"));
        assert_eq!(native.program(), Path::new("/bin/yt-dlp"));
        assert!(native.prefix().is_empty(), "nativ braucht kein Vorspann");
        assert_eq!(native.kind(), "nativ");

        let py = Launcher::Python {
            python: PathBuf::from("/usr/bin/python3"),
            script: PathBuf::from("/bin/yt-dlp.pyz"),
        };
        assert_eq!(py.program(), Path::new("/usr/bin/python3"));
        assert_eq!(py.prefix(), vec!["/bin/yt-dlp.pyz".to_string()]);
        assert_eq!(py.kind(), "Python");
    }

    /// Der Weg wird ohne Programmstart bestimmt: das fertige Programm hat
    /// Vorrang, sonst die Zipapp, sonst nichts.
    #[test]
    fn launcher_wird_richtig_gewaehlt() {
        let dir = std::env::temp_dir().join(format!("mvl-sel-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(resolve_launcher(&dir).is_none(), "leeres Verzeichnis");

        std::fs::write(zipapp_path(&dir), b"nicht wirklich python").unwrap();
        match resolve_launcher(&dir) {
            Some(Launcher::Python { script, .. }) => assert_eq!(script, zipapp_path(&dir)),
            // Ohne Python 3 auf dem Rechner bleibt nur None -- auch richtig.
            None => assert!(find_python().is_none(), "Python vorhanden, aber nicht gewählt"),
            other => panic!("unerwartet: {other:?}"),
        }

        std::fs::write(binary_path(&dir), b"auch kein Programm").unwrap();
        assert!(
            matches!(resolve_launcher(&dir), Some(Launcher::Native(_))),
            "das fertige Programm hat Vorrang"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Die Python-Suche muss auf einem Rechner MIT Python auch fuendig werden --
    /// sonst faende die Ausweichloesung auf dem alten Mac nie etwas.
    #[test]
    fn findet_vorhandenes_python() {
        let Some(path) = find_python() else {
            return; // Rechner ohne Python 3 -- nichts zu pruefen.
        };
        let version = python_version(&path).expect("Version lesbar");
        assert!(version >= MIN_PYTHON, "zu alt: {version:?}");
    }

    /// Der Ausweichweg vollstaendig: Zipapp aus einem bekannten Release laden,
    /// pruefen, mit dem gefundenen Python starten. Braucht Netz:
    /// `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore = "braucht Netzzugang"]
    async fn zipapp_laeuft_mit_python() {
        const TAG: &str = "2026.08.19";
        let Some(python) = find_python() else {
            panic!("für diesen Test wird Python 3.9+ gebraucht");
        };
        let dir = std::env::temp_dir().join(format!("mvl-pyz-{}", uuid::Uuid::new_v4().simple()));
        install_zipapp(&dir, TAG).await.expect("Download und Prüfung");
        let launcher = Launcher::Python {
            python,
            script: zipapp_path(&dir),
        };
        assert_eq!(launcher.version().as_deref(), Some(TAG), "gestartete Version");
        assert_eq!(
            resolve_launcher(&dir),
            Some(launcher),
            "ohne fertiges Programm muss die Zipapp gewählt werden"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn asset_passt_zur_plattform() {
        let name = asset_name();
        assert!(["yt-dlp.exe", "yt-dlp_macos", "yt-dlp_linux"].contains(&name));
    }
}
