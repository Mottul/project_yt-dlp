//! Hinweis auf eine neue Version der App selbst.
//!
//! Nur ein Hinweis mit Verweis auf die Release-Seite, kein Selbst-Update: Die
//! Pakete sind nicht signiert, ein stilles Ersetzen waere ohne Pruefung der
//! Herkunft nicht zu verantworten. Die Version kommt wie bei yt-dlp aus der
//! Weiterleitung von /releases/latest, nicht aus der GitHub-API (ohne Anmeldung
//! 60 Anfragen je Stunde und IP).

use std::time::Duration;

use crate::tools::redirect_target;

const REPO: &str = "https://github.com/Mottul/project_yt-dlp";

/// Version dieses Programms (aus `[workspace.package]` in Cargo.toml).
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Eine neuere Version, als gerade laeuft.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUpdate {
    /// z. B. „0.1.4“
    pub version: String,
    /// die laufende Version, zum Vergleich in der Anzeige
    pub current: String,
    /// Release-Seite mit den Installern
    pub url: String,
}

/// „https://github.com/Mottul/project_yt-dlp/releases/tag/v0.1.3“ -> „0.1.3“.
/// Streng: Zeigt die Weiterleitung woandershin oder ist der Tag keine
/// Versionsnummer, gibt es keinen Hinweis.
pub fn version_from_release_url(url: &str) -> Option<String> {
    let tag = url
        .trim()
        .strip_prefix(REPO)?
        .strip_prefix("/releases/tag/v")?;
    parse_version(tag).map(|_| tag.to_string())
}

/// „0.1.10“ -> (0, 1, 10). Genau drei Zahlen, sonst None.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.split('.');
    let mut next = || -> Option<u64> {
        let part = parts.next()?;
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        part.parse().ok()
    };
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// Ist `candidate` neuer als `current`? Zahlenweise, nicht als Text
/// („0.1.10“ ist neuer als „0.1.9“). Unlesbares ist nie neuer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(c), Some(now)) => c > now,
        _ => false,
    }
}

/// Fragt nach der neuesten veroeffentlichten Version; Some nur, wenn sie
/// neuer ist als diese. Ohne Netz oder ohne Release still None -- ein
/// fehlender Hinweis ist kein Fehler, den man melden muesste.
pub async fn check() -> Option<AppUpdate> {
    let target = redirect_target(&format!("{REPO}/releases/latest"), Duration::from_secs(10))
        .await
        .ok()?;
    let version = version_from_release_url(&target)?;
    is_newer(&version, CURRENT).then(|| AppUpdate {
        url: format!("{REPO}/releases/tag/v{version}"),
        version,
        current: CURRENT.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_aus_der_weiterleitung() {
        assert_eq!(
            version_from_release_url(
                "https://github.com/Mottul/project_yt-dlp/releases/tag/v0.1.3"
            )
            .as_deref(),
            Some("0.1.3")
        );
        // Ohne Release leitet GitHub auf die Liste um
        assert_eq!(
            version_from_release_url("https://github.com/Mottul/project_yt-dlp/releases"),
            None
        );
        for fremd in [
            "https://github.com/andere/project_yt-dlp/releases/tag/v9.9.9",
            "https://github.com/Mottul/project_yt-dlp/releases/tag/v1.2",
            "https://github.com/Mottul/project_yt-dlp/releases/tag/v1.2.3-beta",
            "https://github.com/Mottul/project_yt-dlp/releases/tag/v1.2.3/../x",
            "https://github.com/Mottul/project_yt-dlp/releases/tag/0.1.3",
        ] {
            assert_eq!(version_from_release_url(fremd), None, "{fremd}");
        }
    }

    #[test]
    fn zahlenweise_vergleichen() {
        assert!(is_newer("0.1.3", "0.1.2"));
        assert!(is_newer("0.1.10", "0.1.9"), "nicht als Text vergleichen");
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.1.2", "0.1.2"));
        assert!(!is_newer("0.1.1", "0.1.2"));
        assert!(!is_newer("kaputt", "0.1.2"));
        assert!(!is_newer("0.1.3", "kaputt"));
    }

    #[test]
    fn eigene_version_ist_lesbar() {
        assert!(parse_version(CURRENT).is_some(), "{CURRENT}");
    }
}
