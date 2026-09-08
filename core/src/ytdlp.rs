//! yt-dlp: Version ermitteln, pruefen und aktuell halten.
//!
//! Anders als ffmpeg MUSS yt-dlp aktuell bleiben -- aendert eine Plattform ihre
//! Seiten, hilft nur eine neue Version. Deshalb: beim Start die neueste stabile
//! Version ermitteln und bei Abweichung ersetzen.
//!
//! Die Version kommt aus der Weiterleitung von /releases/latest, nicht aus der
//! GitHub-API: die ist ohne Anmeldung auf 60 Anfragen je Stunde und IP begrenzt
//! und schlaegt hinter einem Firmen-NAT regelmaessig fehl.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::tools::{
    download_verified, exe_name, fetch_text, probe_version, redirect_target, ToolError, ToolResult,
};

const LATEST_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest";

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

/// Installierte Version, sonst None (fehlt oder nicht ausfuehrbar).
pub fn installed_version(bin_dir: &Path) -> Option<String> {
    let path = binary_path(bin_dir);
    path.exists()
        .then(|| probe_version(&path, &["--version"]))
        .flatten()
}

/// Neueste stabile Version laut GitHub.
pub async fn latest_tag() -> ToolResult<String> {
    let target = redirect_target(LATEST_URL, Duration::from_secs(20)).await?;
    tag_from_release_url(&target)
        .ok_or_else(|| ToolError::Other(format!("unerwartete Release-Adresse: {target}")))
}

/// Laedt die angegebene Version und ersetzt die vorhandene Datei.
///
/// Der Tag wird von aussen hereingereicht, damit Pruefsummen und Programm aus
/// demselben Release stammen -- ein neues Release mitten im Vorgang kann so
/// nicht zu einer Fehlpaarung fuehren.
pub async fn install(bin_dir: &Path, tag: &str) -> ToolResult<()> {
    let name = asset_name();
    let sums = fetch_text(&release_url(tag, "SHA2-256SUMS"), Duration::from_secs(30)).await?;
    let expected = parse_checksums(&sums)
        .remove(name)
        .ok_or_else(|| ToolError::Other(format!("keine Prüfsumme für {name} im Release {tag}")))?;

    download_verified(
        &release_url(tag, name),
        &expected,
        &binary_path(bin_dir),
        false,
        Duration::from_secs(900),
    )
    .await
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
    fn asset_passt_zur_plattform() {
        let name = asset_name();
        assert!(["yt-dlp.exe", "yt-dlp_macos", "yt-dlp_linux"].contains(&name));
    }
}
