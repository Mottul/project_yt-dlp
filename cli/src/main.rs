//! MottulVideoLoader — Terminal-Variante.
//!
//! Gedacht für Rechner, auf denen die Fenster-App nicht läuft (macOS vor 11.3).
//! Bedienung: Adresse einfügen, Enter. Einstellungen über einen Buchstaben.
//! yt-dlp-Befehle tippt hier niemand.

mod settings;
mod ui;

use std::sync::{Arc, Condvar, Mutex};

use core::{paths, EnqueueRequest, Format, Job, JobSink, JobStatus, Manager};
use mottul_video_core as core;
use settings::Settings;
use ui::{paint, BOLD, DIM, GOLD, GREEN, RED};

/// Sammelt die Meldungen der Warteschlange und weckt den Hauptfaden, sobald
/// sich etwas getan hat.
#[derive(Default)]
struct Progress {
    jobs: Vec<Job>,
    changed: bool,
}

struct Screen {
    state: Mutex<Progress>,
    signal: Condvar,
}

impl JobSink for Screen {
    fn on_update(&self, job: &Job) {
        let mut guard = self.state.lock().unwrap();
        match guard.jobs.iter_mut().find(|j| j.id == job.id) {
            Some(existing) => *existing = job.clone(),
            None => guard.jobs.push(job.clone()),
        }
        guard.changed = true;
        drop(guard);
        self.signal.notify_all();
    }
}

impl Screen {
    fn done(&self) -> bool {
        self.state.lock().unwrap().jobs.iter().all(|j| {
            matches!(
                j.status,
                JobStatus::Done | JobStatus::Error | JobStatus::Canceled
            )
        })
    }

    fn take(&self) -> Vec<Job> {
        let mut guard = self.state.lock().unwrap();
        guard.changed = false;
        guard.jobs.clone()
    }

    fn clear(&self) {
        let mut guard = self.state.lock().unwrap();
        guard.jobs.clear();
        guard.changed = false;
    }
}

fn main() {
    let data_dir = paths::data_dir();
    let bin_dir = core::tools::bin_dir(&data_dir);
    let mut config = Settings::load(&data_dir);

    let screen = Arc::new(Screen {
        state: Mutex::new(Progress::default()),
        signal: Condvar::new(),
    });
    let manager = match Manager::new(screen.clone()) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("Start fehlgeschlagen: {err}");
            std::process::exit(1);
        }
    };
    let runtime = manager.handle();

    println!();
    println!("  {}", paint(BOLD, "MottulVideoLoader"));

    // Werkzeuge einrichten bzw. prüfen. Ohne Netz geht es mit dem weiter, was da ist.
    let status = if config.auto_update || core::ytdlp::installed_version(&bin_dir).is_none() {
        println!("  {}", paint(DIM, "Prüfe Werkzeuge…"));
        runtime.block_on(core::ensure_tools(&bin_dir, false))
    } else {
        core::read_status(&bin_dir, None)
    };
    print_status(&status);

    if status.ytdlp_version.is_none() {
        println!();
        println!("  {}", paint(RED, "Ohne yt-dlp geht es nicht weiter."));
        println!(
            "  {}",
            paint(DIM, "Internetverbindung prüfen und erneut starten.")
        );
        wait_for_enter();
        std::process::exit(1);
    }

    // Adressen, die beim Aufruf mitgegeben wurden, sofort abarbeiten.
    let direct: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    if !direct.is_empty() {
        run_jobs(&manager, &screen, &bin_dir, &config, &direct);
        return;
    }

    loop {
        println!();
        println!(
            "  {}",
            paint(DIM, &format!("Ziel:   {}", config.output_dir))
        );
        println!("  {}", paint(DIM, &format!("Format: {}", config.summary())));
        println!();
        println!(
            "  {}",
            paint(DIM, "Adresse einfügen + Enter    [f] Format   [a] Auflösung   [o] Ordner   [Enter] Ende")
        );

        let line = match prompt("› ") {
            Some(text) => text,
            None => break,
        };
        let trimmed = line.trim();

        match trimmed {
            "" => break,
            "f" | "F" => choose_format(&mut config, &data_dir),
            "a" | "A" => choose_height(&mut config, &data_dir),
            "o" | "O" => choose_folder(&mut config, &data_dir),
            _ => {
                let urls: Vec<String> = trimmed.split_whitespace().map(str::to_string).collect();
                run_jobs(&manager, &screen, &bin_dir, &config, &urls);
            }
        }
    }

    println!();
    println!("  {}", paint(DIM, "Bis zum nächsten Mal."));
}

fn print_status(status: &core::ToolsStatus) {
    let ytdlp = status
        .ytdlp_version
        .clone()
        .unwrap_or_else(|| "fehlt".into());
    let ffmpeg = status
        .ffmpeg_version
        .clone()
        .unwrap_or_else(|| "fehlt".into());
    let mark = if status.ytdlp_up_to_date == Some(false) {
        "veraltet"
    } else {
        "aktuell"
    };
    println!(
        "  {}",
        paint(DIM, &format!("yt-dlp {ytdlp} · ffmpeg {ffmpeg} · {mark}"))
    );
    if let Some(err) = &status.last_error {
        println!("  {}", paint(RED, &format!("Hinweis: {err}")));
    }
}

/// Stellt die Aufträge ein und zeichnet den Fortschritt, bis alles durch ist.
fn run_jobs(
    manager: &Manager,
    screen: &Arc<Screen>,
    bin_dir: &std::path::Path,
    config: &Settings,
    urls: &[String],
) {
    screen.clear();
    for url in urls {
        manager.enqueue(
            EnqueueRequest {
                url: url.clone(),
                format: config.format,
                max_height: if config.format == Format::Video {
                    config.max_height
                } else {
                    None
                },
                output_dir: config.output_dir.clone(),
            },
            bin_dir.to_path_buf(),
        );
    }

    println!();
    let width = ui::terminal_width();
    let mut block = ui::Block::new();
    loop {
        let jobs = screen.take();
        block.draw(
            &jobs
                .iter()
                .map(|j| ui::job_line(j, width))
                .collect::<Vec<_>>(),
        );
        if screen.done() {
            break;
        }
        // Auf die nächste Meldung warten, spätestens nach 200 ms neu zeichnen.
        let guard = screen.state.lock().unwrap();
        let _ = screen
            .signal
            .wait_timeout(guard, std::time::Duration::from_millis(200))
            .unwrap();
    }
    block.keep();

    for job in screen.take() {
        match job.status {
            JobStatus::Done => {
                let file = job.output_file.unwrap_or_else(|| job.output_dir.clone());
                println!("  {} {}", paint(GREEN, "✓"), file);
            }
            JobStatus::Error => {
                println!(
                    "  {} {}",
                    paint(RED, "✗"),
                    job.error.unwrap_or_else(|| "Fehlgeschlagen".into())
                );
            }
            _ => {}
        }
    }
}

/* ------------------------------ Abfragen -------------------------------- */

fn prompt(label: &str) -> Option<String> {
    use std::io::Write;
    print!("{}", paint(GOLD, label));
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) | Err(_) => None, // Strg-D oder Fehler
        Ok(_) => Some(line),
    }
}

/// Nummerierte Auswahl. Leere Eingabe behält das Bisherige.
fn choose(title: &str, options: &[&str], current: usize) -> Option<usize> {
    println!();
    println!("  {}", paint(BOLD, title));
    for (i, option) in options.iter().enumerate() {
        let marker = if i == current {
            paint(GOLD, "  ← aktuell")
        } else {
            String::new()
        };
        println!("  {}) {}{}", i + 1, option, marker);
    }
    println!();
    let answer = prompt("› ")?;
    let answer = answer.trim();
    if answer.is_empty() {
        return None;
    }
    answer
        .parse::<usize>()
        .ok()
        .filter(|n| *n >= 1 && *n <= options.len())
        .map(|n| n - 1)
}

fn choose_format(config: &mut Settings, data_dir: &std::path::Path) {
    let options = ["Video (MP4)", "Nur Ton (MP3)", "Nur Ton (M4A)"];
    let current = match config.format {
        Format::Video => 0,
        Format::AudioMp3 => 1,
        Format::AudioM4a => 2,
    };
    if let Some(choice) = choose("Format", &options, current) {
        config.format = match choice {
            1 => Format::AudioMp3,
            2 => Format::AudioM4a,
            _ => Format::Video,
        };
        config.save(data_dir);
        println!(
            "  {}",
            paint(GREEN, &format!("✓ Format: {}", config.format_label()))
        );
    }
}

fn choose_height(config: &mut Settings, data_dir: &std::path::Path) {
    let options = ["Beste", "2160p (4K)", "1440p", "1080p", "720p", "480p"];
    let heights = [
        None,
        Some(2160),
        Some(1440),
        Some(1080),
        Some(720),
        Some(480),
    ];
    let current = heights
        .iter()
        .position(|h| *h == config.max_height)
        .unwrap_or(0);
    if let Some(choice) = choose("Höchste Auflösung", &options, current) {
        config.max_height = heights[choice];
        config.save(data_dir);
        println!(
            "  {}",
            paint(GREEN, &format!("✓ Auflösung: {}", options[choice]))
        );
    }
}

fn choose_folder(config: &mut Settings, data_dir: &std::path::Path) {
    println!();
    println!("  {}", paint(BOLD, "Zielordner"));
    println!(
        "  {}",
        paint(
            DIM,
            "Ordner ins Fenster ziehen oder Pfad eingeben. Leer = unverändert."
        )
    );
    println!();
    let Some(answer) = prompt("› ") else { return };
    let path = clean_path(&answer);
    if path.is_empty() {
        return;
    }
    if std::path::Path::new(&path).is_dir() {
        config.output_dir = path;
        config.save(data_dir);
        println!(
            "  {}",
            paint(GREEN, &format!("✓ Ziel: {}", config.output_dir))
        );
    } else {
        println!("  {}", paint(RED, &format!("Kein Ordner: {path}")));
    }
}

/// Räumt auf, was beim Hineinziehen eines Ordners im Terminal ankommt:
/// Anführungszeichen, maskierte Leerzeichen, Tilde.
pub fn clean_path(input: &str) -> String {
    let trimmed = input.trim().trim_matches(['"', '\''].as_ref()).to_string();
    let unescaped = trimmed.replace("\\ ", " ");
    match unescaped.strip_prefix("~") {
        Some(rest) => match std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
            Some(home) => format!("{}{}", home.to_string_lossy(), rest),
            None => unescaped,
        },
        None => unescaped,
    }
}

fn wait_for_enter() {
    if ui::interactive() {
        println!();
        let _ = prompt("Enter zum Schließen ");
    }
}

#[cfg(test)]
mod tests {
    use super::clean_path;

    #[test]
    fn pfad_aus_dem_terminal_wird_gesaeubert() {
        assert_eq!(clean_path("  /tmp/ziel  \n"), "/tmp/ziel");
        assert_eq!(clean_path("'/tmp/mein ziel'"), "/tmp/mein ziel");
        assert_eq!(clean_path("\"/tmp/ziel\""), "/tmp/ziel");
        // So liefert es die Terminal-App, wenn man einen Ordner hineinzieht:
        assert_eq!(clean_path("/Users/m/Mein\\ Ordner"), "/Users/m/Mein Ordner");
    }

    #[test]
    fn tilde_wird_zum_benutzerordner() {
        std::env::set_var(
            if cfg!(windows) { "USERPROFILE" } else { "HOME" },
            "/home/pruef",
        );
        assert_eq!(clean_path("~/Downloads"), "/home/pruef/Downloads");
        assert_eq!(clean_path("~"), "/home/pruef");
    }

    #[test]
    fn leere_eingabe_bleibt_leer() {
        assert_eq!(clean_path("   \n"), "");
    }
}
