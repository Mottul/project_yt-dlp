//! Anmeldung fuer yt-dlp: Cookies aus einem Browser. Altersbeschraenkte oder
//! nur angemeldet sichtbare Videos (YouTube: „Sign in to confirm your age“)
//! laedt yt-dlp nur damit.
//!
//! Der Kern liest, speichert und uebertraegt keine Cookies. Er nennt yt-dlp nur
//! den Browser (feste Liste), yt-dlp liest sie bei jedem Abruf selbst.

use serde::{Deserialize, Deserializer, Serialize};

/// Browser, aus denen yt-dlp Cookies lesen kann (Namen wie bei `--cookies-from-browser`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CookieBrowser {
    Firefox,
    Chrome,
    Edge,
    Brave,
    Opera,
    Vivaldi,
    Chromium,
    Safari,
}

impl CookieBrowser {
    pub const ALL: [CookieBrowser; 8] = [
        CookieBrowser::Firefox,
        CookieBrowser::Chrome,
        CookieBrowser::Edge,
        CookieBrowser::Brave,
        CookieBrowser::Opera,
        CookieBrowser::Vivaldi,
        CookieBrowser::Chromium,
        CookieBrowser::Safari,
    ];

    /// Name fuer yt-dlp.
    pub fn id(self) -> &'static str {
        match self {
            CookieBrowser::Firefox => "firefox",
            CookieBrowser::Chrome => "chrome",
            CookieBrowser::Edge => "edge",
            CookieBrowser::Brave => "brave",
            CookieBrowser::Opera => "opera",
            CookieBrowser::Vivaldi => "vivaldi",
            CookieBrowser::Chromium => "chromium",
            CookieBrowser::Safari => "safari",
        }
    }

    /// Name fuer die Anzeige.
    pub fn label(self) -> &'static str {
        match self {
            CookieBrowser::Firefox => "Firefox",
            CookieBrowser::Chrome => "Chrome",
            CookieBrowser::Edge => "Edge",
            CookieBrowser::Brave => "Brave",
            CookieBrowser::Opera => "Opera",
            CookieBrowser::Vivaldi => "Vivaldi",
            CookieBrowser::Chromium => "Chromium",
            CookieBrowser::Safari => "Safari",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        let id = id.trim();
        Self::ALL.iter().copied().find(|b| b.id() == id)
    }

    /// Was sich auf diesem System waehlen laesst: Safari gibt es nur auf dem Mac.
    pub fn available() -> Vec<CookieBrowser> {
        let mac = cfg!(target_os = "macos");
        Self::ALL
            .iter()
            .copied()
            .filter(|b| *b != CookieBrowser::Safari || mac)
            .collect()
    }
}

/// Liest die Wahl nachsichtig: Ein unbekannter Name (von Hand eingetragen, aus
/// einer neueren Fassung) gilt als „ohne Anmeldung“, statt die ganze
/// Einstellungsdatei zu verwerfen.
pub fn lenient_browser<'de, D>(d: D) -> Result<Option<CookieBrowser>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<serde_json::Value>::deserialize(d)?;
    let name = raw.as_ref().and_then(|v| v.as_str());
    Ok(name.and_then(CookieBrowser::from_id))
}

const DPAPI: &str = "Chrome und Edge verschlüsseln ihre Cookies unter Windows so, dass \
    andere Programme sie nicht lesen können. Unter „Anmeldung“ Firefox wählen.";
const LOCKED: &str = "Der Browser sperrt seine Cookies, solange er läuft – Browser \
    schließen und erneut versuchen.";
const NO_COOKIES: &str = "Im gewählten Browser wurden keine Cookies gefunden – ist er \
    installiert und schon benutzt worden? Sonst unter „Anmeldung“ einen anderen wählen.";
const JS_FAILED: &str = "YouTube verlangt eine Prüfaufgabe in JavaScript, die yt-dlp nicht \
    lösen konnte. Die Werkzeuge prüfen lassen (lädt die JavaScript-Laufzeit nach) und \
    erneut versuchen.";
const AGE_NEEDS_LOGIN: &str = "Altersbeschränktes Video: Unter „Anmeldung“ den Browser \
    wählen, der bei YouTube angemeldet ist.";
const AGE_DENIED: &str = "Altersbeschränktes Video, das YouTube diesem Konto nicht \
    freigibt: im gewählten Browser bei YouTube anmelden (volljähriges Konto, Alter \
    bestätigt) und erneut versuchen.";
const LOGIN_NEEDED: &str = "Anmeldung nötig (Bot-Prüfung oder nur angemeldet sichtbar): \
    Unter „Anmeldung“ den Browser wählen, der bei der Seite angemeldet ist.";
const LOGIN_DESPITE_COOKIES: &str = "Die Seite verlangt trotz Cookies eine Anmeldung – im \
    gewählten Browser anmelden und erneut versuchen.";

/// Letzte Fehlerzeilen, hinter denen eine gescheiterte Pruefaufgabe stecken kann.
const NO_FORMATS: [&str; 3] = [
    "age-restricted",
    "format is not available",
    "no video formats",
];
const AGE: [&str; 3] = ["confirm your age", "age-restricted", "age-verification"];
const LOGIN: [&str; 3] = [
    "sign in to confirm",
    "--cookies-from-browser or --cookies",
    "login required",
];

/// Verstaendliche Meldung zu bekannten yt-dlp-Fehlern rund um Anmeldung und
/// YouTubes Pruefaufgaben, sonst None. `tail`: die letzten Zeilen von stderr,
/// `cookies`: was eingestellt war.
pub fn explain_error(tail: &[String], cookies: Option<CookieBrowser>) -> Option<String> {
    let all = tail.join("\n").to_lowercase();
    let has = |needle: &str| all.contains(needle);
    // Die Ursache steht oft nur in einer Warnung vor der letzten Fehlerzeile.
    let last = tail
        .iter()
        .rev()
        .find(|l| l.contains("ERROR"))
        .map(|l| l.to_lowercase())
        .unwrap_or_default();
    let last_has = |needles: &[&str]| needles.iter().any(|n| last.contains(n));
    let js_failed = has("signature solving failed")
        || has("n challenge solving failed")
        || has("no supported javascript runtime");

    let text = if has("failed to decrypt with dpapi") || has("app-bound") {
        DPAPI
    } else if (has("could not copy") && has("cookie database")) || has("database is locked") {
        LOCKED
    } else if has("could not find") && has("cookies database") {
        NO_COOKIES
    } else if js_failed && last_has(&NO_FORMATS) {
        JS_FAILED
    } else if last_has(&AGE) {
        match cookies {
            Some(_) => AGE_DENIED,
            None => AGE_NEEDS_LOGIN,
        }
    } else if last_has(&LOGIN) {
        match cookies {
            Some(_) => LOGIN_DESPITE_COOKIES,
            None => LOGIN_NEEDED,
        }
    } else {
        return None;
    };
    Some(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const FF: Option<CookieBrowser> = Some(CookieBrowser::Firefox);

    #[test]
    fn namen_hin_und_zurueck() {
        for b in CookieBrowser::ALL {
            assert_eq!(CookieBrowser::from_id(b.id()), Some(b));
            let json = serde_json::to_string(&b).unwrap();
            assert_eq!(json, format!("\"{}\"", b.id()), "wie yt-dlp sie nennt");
        }
        assert_eq!(CookieBrowser::from_id(" firefox "), FF);
        for fremd in ["", "Firefox", "firefox; rm -rf", "internet-explorer"] {
            assert_eq!(CookieBrowser::from_id(fremd), None, "{fremd}");
        }
    }

    #[test]
    fn safari_nur_auf_dem_mac() {
        let list = CookieBrowser::available();
        assert!(list.contains(&CookieBrowser::Firefox));
        let mac = cfg!(target_os = "macos");
        assert_eq!(list.contains(&CookieBrowser::Safari), mac);
    }

    #[derive(Deserialize)]
    struct Probe {
        #[serde(default, deserialize_with = "lenient_browser")]
        cookies: Option<CookieBrowser>,
    }

    fn read(raw: &str) -> Option<CookieBrowser> {
        serde_json::from_str::<Probe>(raw).unwrap().cookies
    }

    #[test]
    fn unbekannter_browser_verwirft_die_datei_nicht() {
        assert_eq!(read(r#"{"cookies":"firefox"}"#), FF);
        assert_eq!(read(r#"{"cookies":"netscape"}"#), None);
        assert_eq!(read(r#"{"cookies":42}"#), None);
        assert_eq!(read(r#"{"cookies":null}"#), None);
        assert_eq!(read("{}"), None);
    }

    fn explain(text: &str, cookies: Option<CookieBrowser>) -> String {
        let tail: Vec<String> = text.lines().map(str::to_string).collect();
        explain_error(&tail, cookies).unwrap_or_default()
    }

    // So am 7.11.2026: Firefox-Cookies, angemeldet, Alter nicht bestaetigt
    const AGE_ERR: &str = "ERROR: [youtube] -Qq1MLVwmfs: Sorry, this content is \
        age-restricted. This video is age-restricted and only available on YouTube";
    // So meldet es yt-dlp ganz ohne Cookies (MottulVideoLoader 0.1.1)
    const SIGN_IN_AGE: &str = "ERROR: [youtube] -Qq1MLVwmfs: Sign in to confirm your \
        age. This video may be inappropriate for some users. Use --cookies-from-browser \
        or --cookies for the authentication.";
    const NO_JS: &str = "WARNING: [youtube] x: Signature solving failed: Some formats may \
        be missing.\nWARNING: [youtube] x: n challenge solving failed: Some formats may \
        be missing.\n";

    #[test]
    fn altersbeschraenkt_je_nach_anmeldung() {
        assert!(explain(AGE_ERR, None).contains("„Anmeldung“"));
        assert!(explain(AGE_ERR, FF).contains("Alter bestätigt"));
        let text = explain(SIGN_IN_AGE, None);
        assert!(text.starts_with("Altersbeschränktes Video: Unter"));
    }

    #[test]
    fn gescheiterte_pruefaufgabe_ist_die_ursache() {
        let both = format!("{NO_JS}{AGE_ERR}");
        assert!(explain(&both, FF).contains("JavaScript"));
        // Die Warnung allein erklaert keinen fremden Fehler
        let other = format!("{NO_JS}ERROR: unable to write data: No space left");
        assert_eq!(explain(&other, None), "");
    }

    #[test]
    fn cookie_probleme_und_bot_pruefung() {
        let chrome = Some(CookieBrowser::Chrome);
        let dpapi = "ERROR: Failed to decrypt with DPAPI. See https://github.com/x";
        assert!(explain(dpapi, chrome).contains("Firefox wählen"));
        let locked = "ERROR: Could not copy Chrome cookie database. See …";
        assert!(explain(locked, chrome).contains("Browser schließen"));
        let missing = "ERROR: could not find firefox cookies database in \"/home/a\"";
        assert!(explain(missing, FF).contains("keine Cookies gefunden"));
        let bot = "ERROR: [youtube] x: Sign in to confirm you're not a bot.";
        assert!(explain(bot, None).contains("Bot-Prüfung"));
        assert!(explain(bot, chrome).contains("trotz Cookies"));
        assert_eq!(explain("ERROR: Video unavailable", chrome), "");
    }
}
