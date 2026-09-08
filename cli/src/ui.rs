//! Ausgabe im Terminal: Farben, Fortschrittsbalken und die Abfragen.
//!
//! Bewusst ohne Fremdbibliothek. Wenn die Ausgabe kein Terminal ist (Pipe,
//! Logdatei), fallen Farben und das Neuzeichnen weg -- sonst stehen dort
//! Steuerzeichen statt Text.

use std::io::{IsTerminal, Write};

use mottul_video_core::{Job, JobStatus};

pub const RESET: &str = "\x1b[0m";
pub const DIM: &str = "\x1b[2m";
pub const BOLD: &str = "\x1b[1m";
pub const GOLD: &str = "\x1b[38;5;220m";
pub const GREEN: &str = "\x1b[32m";
pub const RED: &str = "\x1b[31m";

pub fn interactive() -> bool {
    std::io::stdout().is_terminal() && std::io::stdin().is_terminal()
}

pub fn paint(code: &str, text: &str) -> String {
    if interactive() {
        format!("{code}{text}{RESET}")
    } else {
        text.to_string()
    }
}

/// Balken aus Blockzeichen, z. B. `████████░░░░░░░░`.
pub fn bar(progress: f64, width: usize) -> String {
    let filled = ((progress.clamp(0.0, 1.0)) * width as f64).round() as usize;
    let mut out = String::with_capacity(width * 3);
    for i in 0..width {
        out.push(if i < filled { '█' } else { '░' });
    }
    out
}

/// Eine Zeile je Auftrag: Titel, Balken, Tempo, Restzeit.
pub fn job_line(job: &Job, width: usize) -> String {
    let title = job.title.clone().unwrap_or_else(|| shorten_url(&job.url));
    let percent = format!("{:>3} %", (job.progress * 100.0).round() as u32);
    let detail = match job.status {
        JobStatus::Queued => "wartet".to_string(),
        JobStatus::Running => {
            let mut parts = Vec::new();
            if let Some(speed) = &job.speed {
                parts.push(speed.clone());
            }
            if let Some(eta) = &job.eta {
                parts.push(format!("Rest {eta}"));
            }
            parts.join("  ")
        }
        JobStatus::Done => "fertig".to_string(),
        JobStatus::Error => job.error.clone().unwrap_or_else(|| "Fehler".into()),
        JobStatus::Canceled => "abgebrochen".to_string(),
    };

    let bar_width = 20;
    let head = truncate(&title, width.saturating_sub(bar_width + 24).max(12));
    match job.status {
        JobStatus::Done => format!("  {} {}", paint(GREEN, "✓"), head),
        JobStatus::Error => format!("  {} {}  {}", paint(RED, "✗"), head, paint(DIM, &detail)),
        JobStatus::Canceled => format!("  {} {}", paint(DIM, "–"), head),
        _ => format!(
            "  {}  {} {}  {}",
            head,
            paint(GOLD, &bar(job.progress, bar_width)),
            percent,
            paint(DIM, &detail)
        ),
    }
}

/// Kuerzt auf `max` Zeichen (nicht Bytes) und haengt ein Auslassungszeichen an.
pub fn truncate(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(1);
    text.chars().take(keep).collect::<String>() + "…"
}

/// Aus einer Adresse etwas Lesbares machen, solange der Titel fehlt.
pub fn shorten_url(url: &str) -> String {
    let without_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    truncate(without_scheme, 60)
}

/// Zeichnet einen Block von Zeilen und setzt den Cursor danach zurueck, damit
/// die naechste Ausgabe ihn ueberschreibt.
pub struct Block {
    lines: usize,
}

impl Block {
    pub fn new() -> Self {
        Self { lines: 0 }
    }

    pub fn draw(&mut self, lines: &[String]) {
        let mut out = std::io::stdout().lock();
        if interactive() && self.lines > 0 {
            // Cursor um die zuletzt gezeichneten Zeilen hoch, Rest loeschen.
            let _ = write!(out, "\x1b[{}A\x1b[J", self.lines);
        }
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
        let _ = out.flush();
        self.lines = if interactive() { lines.len() } else { 0 };
    }

    /// Den Block stehen lassen und den naechsten neu beginnen.
    pub fn keep(&mut self) {
        self.lines = 0;
    }
}

impl Default for Block {
    fn default() -> Self {
        Self::new()
    }
}

/// Breite des Terminals, mit brauchbarem Rueckfall.
pub fn terminal_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|w| *w >= 40)
        .unwrap_or(90)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balken_faellt_nie_aus_dem_rahmen() {
        assert_eq!(bar(0.0, 4).chars().filter(|c| *c == '█').count(), 0);
        assert_eq!(bar(1.0, 4).chars().filter(|c| *c == '█').count(), 4);
        assert_eq!(bar(2.5, 4).chars().filter(|c| *c == '█').count(), 4);
        assert_eq!(bar(-1.0, 4).chars().filter(|c| *c == '█').count(), 0);
        assert_eq!(bar(0.5, 10).chars().count(), 10);
    }

    #[test]
    fn kuerzen_zaehlt_zeichen_nicht_bytes() {
        assert_eq!(truncate("Bühnenprobe", 5), "Bühn…");
        assert_eq!(truncate("kurz", 10), "kurz");
        assert_eq!(truncate("Öäü", 3), "Öäü");
    }

    #[test]
    fn adresse_wird_lesbar_gekuerzt() {
        assert_eq!(shorten_url("https://example.com/x"), "example.com/x");
        assert!(shorten_url(&format!("https://example.com/{}", "a".repeat(100))).ends_with('…'));
    }
}
