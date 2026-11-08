//! Gemeinsame Verwaltung der externen Programme (yt-dlp, ffmpeg, ffprobe).
//!
//! Grundsatz: Was hier ausfuehrbar auf der Platte landet, wurde vorher gegen
//! eine SHA-256-Pruefsumme geprueft. Erst danach wird die Datei an ihren Platz
//! umbenannt -- ein Abbruch hinterlaesst nie eine halbe, startbare Datei.
//! Das ersetzt keine Signaturpruefung: es sichert die Uebertragung, nicht die
//! Herkunft des Releases.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
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

/// Vergleicht die SHA-256 der Daten mit der erwarteten.
pub fn check_sha256(data: &[u8], expected: &str) -> ToolResult<()> {
    let actual = sha256_hex(data);
    if actual.eq_ignore_ascii_case(expected) {
        return Ok(());
    }
    Err(ToolError::Checksum {
        expected: expected.chars().take(12).collect(),
        actual: actual.chars().take(12).collect(),
    })
}

/// Groesser ist keine der Dateien, die hier entpackt werden (Deno: rund 100 MB).
const MAX_UNZIPPED: usize = 512 * 1024 * 1024;

fn le16(b: &[u8], at: usize) -> Option<usize> {
    let s = b.get(at..at.checked_add(2)?)?;
    Some(u16::from_le_bytes([s[0], s[1]]) as usize)
}

fn le32(b: &[u8], at: usize) -> Option<usize> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as usize)
}

/// Holt eine Datei aus einem ZIP-Archiv (Deno gibt es nur so).
///
/// Bewusst schmal: Eintraege ungepackt oder Deflate, ohne ZIP64 und ohne
/// Verschluesselung -- mehr liefern die festgenagelten Archive nicht, alles
/// andere wird abgelehnt statt halb gelesen. Groessen und Lage kommen aus dem
/// Inhaltsverzeichnis am Ende: im Kopf vor den Daten duerfen sie fehlen.
pub fn unzip_entry(zip: &[u8], name: &str) -> ToolResult<Vec<u8>> {
    let bad = |what: &str| ToolError::Other(format!("ZIP-Archiv fehlerhaft: {what}"));
    let u16_at = |at: usize| le16(zip, at).ok_or_else(|| bad("zu kurz"));
    let u32_at = |at: usize| le32(zip, at).ok_or_else(|| bad("zu kurz"));

    // Ende des Inhaltsverzeichnisses: 22 Bytes, dahinter hoechstens ein
    // Kommentar von bis zu 64 KiB.
    let last = zip.len().checked_sub(22).ok_or_else(|| bad("zu kurz"))?;
    let eocd = (last.saturating_sub(0xFFFF)..=last)
        .rev()
        .find(|&p| le32(zip, p) == Some(0x0605_4b50))
        .ok_or_else(|| bad("kein Inhaltsverzeichnis"))?;
    let count = u16_at(eocd + 10)?;
    let mut pos = u32_at(eocd + 16)?;

    for _ in 0..count {
        if le32(zip, pos) != Some(0x0201_4b50) {
            return Err(bad("Inhaltsverzeichnis"));
        }
        let flags = u16_at(pos + 8)?;
        let method = u16_at(pos + 10)?;
        let crc = u32_at(pos + 16)?;
        let packed = u32_at(pos + 20)?;
        let size = u32_at(pos + 24)?;
        let name_len = u16_at(pos + 28)?;
        let extra_len = u16_at(pos + 30)?;
        let comment_len = u16_at(pos + 32)?;
        let local = u32_at(pos + 42)?;
        let entry_name = zip.get(pos + 46..pos + 46 + name_len);
        pos += 46 + name_len + extra_len + comment_len;
        if entry_name != Some(name.as_bytes()) {
            continue;
        }

        if flags & 1 != 0 {
            return Err(bad("verschlüsselt"));
        }
        if [packed, size, local].contains(&0xFFFF_FFFF) {
            return Err(bad("ZIP64 wird nicht unterstützt"));
        }
        if size > MAX_UNZIPPED || le32(zip, local) != Some(0x0403_4b50) {
            return Err(bad("Eintrag"));
        }
        let start = local + 30 + u16_at(local + 26)? + u16_at(local + 28)?;
        let raw = zip
            .get(start..start + packed)
            .ok_or_else(|| bad("abgeschnitten"))?;

        let mut out = Vec::with_capacity(size);
        match method {
            0 => out.extend_from_slice(raw),
            8 => {
                flate2::read::DeflateDecoder::new(raw)
                    .take(size as u64 + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| bad(&format!("Deflate: {e}")))?;
            }
            other => return Err(bad(&format!("Packverfahren {other}"))),
        }
        let mut sum = flate2::Crc::new();
        sum.update(&out);
        if out.len() != size || sum.sum() as usize != crc {
            return Err(bad("Prüfsumme des Eintrags"));
        }
        return Ok(out);
    }
    Err(ToolError::Other(format!("{name} fehlt im ZIP-Archiv")))
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
    check_sha256(&data, expected)?;
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

/// Groesse und Aenderungszeit der beteiligten Dateien: aendert sich eine davon
/// (neue Version abgelegt), gilt die gemerkte Antwort nicht mehr.
type Stamp = Vec<(u64, Option<std::time::SystemTime>)>;

/// Ein Platz je Aufruf (Programm + Argumente). Der Platz bleibt waehrend der
/// Abfrage gesperrt: Fragen zwei Stellen gleichzeitig (Statusanzeige und
/// Startpruefung), startet das Programm trotzdem nur einmal.
type Slot = Arc<Mutex<Option<(Stamp, String)>>>;

/// Programm und Argumente eines Aufrufs.
type ProbeKey = (PathBuf, Vec<String>);

static VERSIONS: OnceLock<Mutex<HashMap<ProbeKey, Slot>>> = OnceLock::new();

/// Stempel aus dem Programm und allen Argumenten, die auf eine Datei zeigen
/// (die Zipapp beim Python-Weg). None, wenn das Programm nicht lesbar ist.
fn stamp(bin: &Path, args: &[&str]) -> Option<Stamp> {
    let of = |p: &Path| {
        std::fs::metadata(p)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| (m.len(), m.modified().ok()))
    };
    let mut out = vec![of(bin)?];
    out.extend(args.iter().filter_map(|a| of(Path::new(a))));
    Some(out)
}

/// Startet ein Programm und liefert die erste Zeile der Ausgabe.
///
/// Gemerkt, solange sich die Datei nicht aendert: Ein Aufruf kostet unter
/// Windows bis zu zwei Sekunden (yt-dlp entpackt sich bei jedem Start, Deno
/// prueft der Virenscanner), und beim Programmstart fragten Statusanzeige und
/// Startpruefung jedes Werkzeug mehrfach ab. Fehlschlaege werden nicht gemerkt
/// -- eine frisch abgelegte Datei kann kurz gesperrt sein.
pub fn probe_version(bin: &Path, args: &[&str]) -> Option<String> {
    remembered(bin, args, || run_probe(bin, args))
}

fn remembered(bin: &Path, args: &[&str], probe: impl FnOnce() -> Option<String>) -> Option<String> {
    let Some(now) = stamp(bin, args) else {
        return probe();
    };
    let key = (
        bin.to_path_buf(),
        args.iter().map(|a| a.to_string()).collect(),
    );
    let slot: Slot = VERSIONS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .entry(key)
        .or_default()
        .clone();
    let mut slot = slot.lock().unwrap();
    if let Some((seen, version)) = slot.as_ref() {
        if *seen == now {
            return Some(version.clone());
        }
    }
    let version = probe()?;
    *slot = Some((now, version.clone()));
    Some(version)
}

fn run_probe(bin: &Path, args: &[&str]) -> Option<String> {
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
    fn pruefsumme_vergleichen() {
        let leer = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(check_sha256(b"", leer).is_ok());
        assert!(check_sha256(b"", &leer.to_ascii_uppercase()).is_ok());
        let err = check_sha256(b"x", leer).unwrap_err();
        assert!(matches!(err, ToolError::Checksum { .. }));
    }

    /// Mit Pythons zipfile gebaut (Deflate, zwei Eintraege) -- ein echtes
    /// Archiv, nicht eines aus dem eigenen Gegenstueck des Lesers.
    const ECHTES_ZIP: [u8; 249] = [
        0x50, 0x4b, 0x03, 0x04, 0x14, 0x00, 0x00, 0x00, 0x08, 0x00, 0xae, 0x76, 0x67, 0x5d, 0x7b,
        0xed, 0x63, 0xfd, 0x12, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00,
        0x4c, 0x49, 0x45, 0x53, 0x4d, 0x49, 0x43, 0x48, 0x2e, 0x74, 0x78, 0x74, 0xcb, 0x2b, 0x2d,
        0x52, 0x48, 0xcd, 0xcc, 0x53, 0x70, 0x4a, 0xcd, 0x2c, 0x2e, 0xc8, 0x4c, 0xcd, 0x01, 0x00,
        0x50, 0x4b, 0x03, 0x04, 0x14, 0x00, 0x00, 0x00, 0x08, 0x00, 0xae, 0x76, 0x67, 0x5d, 0xb6,
        0x9b, 0x96, 0x8c, 0x11, 0x00, 0x00, 0x00, 0x60, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00,
        0x64, 0x65, 0x6e, 0x6f, 0x2e, 0x65, 0x78, 0x65, 0xf3, 0x48, 0xcc, 0xc9, 0xc9, 0x57, 0x70,
        0x49, 0xcd, 0xcb, 0x57, 0x54, 0xf0, 0xa0, 0x01, 0x1b, 0x00, 0x50, 0x4b, 0x01, 0x02, 0x14,
        0x00, 0x14, 0x00, 0x00, 0x00, 0x08, 0x00, 0xae, 0x76, 0x67, 0x5d, 0x7b, 0xed, 0x63, 0xfd,
        0x12, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x01, 0x00, 0x00, 0x00, 0x00, 0x4c, 0x49, 0x45, 0x53,
        0x4d, 0x49, 0x43, 0x48, 0x2e, 0x74, 0x78, 0x74, 0x50, 0x4b, 0x01, 0x02, 0x14, 0x00, 0x14,
        0x00, 0x00, 0x00, 0x08, 0x00, 0xae, 0x76, 0x67, 0x5d, 0xb6, 0x9b, 0x96, 0x8c, 0x11, 0x00,
        0x00, 0x00, 0x60, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x80, 0x01, 0x3c, 0x00, 0x00, 0x00, 0x64, 0x65, 0x6e, 0x6f, 0x2e, 0x65,
        0x78, 0x65, 0x50, 0x4b, 0x05, 0x06, 0x00, 0x00, 0x00, 0x00, 0x02, 0x00, 0x02, 0x00, 0x70,
        0x00, 0x00, 0x00, 0x73, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn zip_eintrag_entpacken() {
        let data = unzip_entry(&ECHTES_ZIP, "deno.exe").unwrap();
        assert_eq!(data, "Hallo Deno! ".repeat(8).as_bytes());
        assert_eq!(
            unzip_entry(&ECHTES_ZIP, "LIESMICH.txt").unwrap(),
            b"nur ein Beispiel"
        );
    }

    #[test]
    fn zip_ohne_den_eintrag() {
        let err = unzip_entry(&ECHTES_ZIP, "deno").unwrap_err();
        assert!(err.to_string().contains("fehlt"), "{err}");
    }

    #[test]
    fn zip_kaputt_wird_abgelehnt() {
        // Ein Byte in den gepackten Daten von deno.exe verfaelscht
        let mut kaputt = ECHTES_ZIP;
        kaputt[110] ^= 0x55;
        assert!(unzip_entry(&kaputt, "deno.exe").is_err());
        // Abgeschnitten: kein Inhaltsverzeichnis mehr
        assert!(unzip_entry(&ECHTES_ZIP[..200], "deno.exe").is_err());
        assert!(unzip_entry(b"", "deno.exe").is_err());
        assert!(unzip_entry(&[b'x'; 64], "deno.exe").is_err());
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

    /// Die Version wird je Datei nur einmal ermittelt -- bis eine neue Datei
    /// daliegt. Fehlschlaege zaehlen nicht.
    #[test]
    fn version_wird_gemerkt_bis_die_datei_wechselt() {
        let dir = std::env::temp_dir().join(format!("mvl-test-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("werkzeug");
        std::fs::write(&bin, b"v1").unwrap();
        let calls = std::cell::Cell::new(0);
        let ask = |answer: Option<&str>| {
            remembered(&bin, &["--version"], || {
                calls.set(calls.get() + 1);
                answer.map(String::from)
            })
        };

        assert_eq!(ask(None), None, "Fehlschlag");
        assert_eq!(ask(Some("1.0")).as_deref(), Some("1.0"));
        assert_eq!(ask(Some("anders")).as_deref(), Some("1.0"), "gemerkt");
        assert_eq!(calls.get(), 2, "nach dem Treffer nicht mehr gestartet");

        // Neue Datei (andere Groesse) -> neu fragen
        std::fs::write(&bin, b"version 2").unwrap();
        assert_eq!(ask(Some("2.0")).as_deref(), Some("2.0"));
        assert_eq!(calls.get(), 3);

        // Andere Argumente sind ein eigener Platz
        let other = remembered(&bin, &["-version"], || Some("x".into()));
        assert_eq!(other.as_deref(), Some("x"));

        // Ohne Datei wird nichts gemerkt
        std::fs::remove_file(&bin).unwrap();
        assert_eq!(ask(Some("weg")).as_deref(), Some("weg"));
        assert_eq!(ask(Some("immer neu")).as_deref(), Some("immer neu"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
