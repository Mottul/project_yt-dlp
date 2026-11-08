//! yt-dlp samt allen Unterprozessen beenden.
//!
//! `yt-dlp.exe` ist ein PyInstaller-Programm: Der gestartete Prozess entpackt
//! sich und startet sich dann ein zweites Mal -- dieser zweite Prozess laedt,
//! und er startet seinerseits ffmpeg. Beendet man nur den ersten, laeuft der
//! Download unbemerkt weiter (so unter Windows nachgestellt, 8.11.2026: nach
//! dem Abbruch kam weiter Fortschritt, die `.part`-Datei blieb offen, und das
//! Ersetzen von yt-dlp scheiterte an der belegten Datei).
//!
//! - Windows: ein Job-Objekt mit `KILL_ON_JOB_CLOSE`. Alles, was der Prozess
//!   startet, gehoert automatisch dazu; endet die App (auch durch Absturz),
//!   schliesst das System den Job und beendet den ganzen Baum.
//! - Unix: die Nachkommen zum Zeitpunkt des Abbruchs aus der Prozesstabelle.
//!   Bewusst keine eigene Prozessgruppe: Strg+C im Terminal erreicht yt-dlp
//!   nur, solange es in der Gruppe der Terminal-Variante bleibt.

pub use imp::ProcessTree;

/// Alle Nachkommen von `root` aus einer Prozesstabelle mit „pid ppid“ je Zeile
/// (`ps -A -o pid= -o ppid=`). Unlesbare Zeilen werden uebersprungen.
#[cfg(any(unix, test))]
pub fn descendants(table: &str, root: u32) -> Vec<u32> {
    let pairs: Vec<(u32, u32)> = table
        .lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
        })
        .collect();
    let mut found = Vec::new();
    let mut open = vec![root];
    while let Some(parent) = open.pop() {
        for &(pid, ppid) in &pairs {
            if ppid == parent && pid != root && !found.contains(&pid) {
                found.push(pid);
                open.push(pid);
            }
        }
    }
    found
}

#[cfg(windows)]
mod imp {
    use winapi::um::handleapi::CloseHandle;
    use winapi::um::jobapi2::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject, TerminateJobObject,
    };
    use winapi::um::winnt::{
        JobObjectExtendedLimitInformation, HANDLE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// Haelt einen gestarteten Prozess und alles, was er startet, zusammen.
    pub struct ProcessTree {
        job: HANDLE,
    }

    // Ein Job-Handle darf von jedem Faden benutzt werden.
    unsafe impl Send for ProcessTree {}
    unsafe impl Sync for ProcessTree {}

    impl ProcessTree {
        /// Ordnet den Prozess einem neuen Job zu. Zwischen Start und Zuordnung
        /// liegen Mikrosekunden, bis zum zweiten Prozess entpackt sich yt-dlp
        /// rund zwei Sekunden lang -- der faellt also sicher in den Job.
        pub fn attach(child: &tokio::process::Child) -> Option<Self> {
            let process = child.raw_handle()? as HANDLE;
            // SAFETY: Aufrufe nach Doku; `job` wird bei jedem Fehlschlag wieder
            // geschlossen, `info` ist eine vollstaendig genullte C-Struktur.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &mut info as *mut _ as *mut _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ) != 0
                    && AssignProcessToJobObject(job, process) != 0;
                if !ok {
                    CloseHandle(job);
                    return None;
                }
                Some(Self { job })
            }
        }

        /// Beendet den ganzen Baum.
        pub fn kill(&self) {
            // SAFETY: `job` ist bis zum Drop gueltig.
            unsafe {
                TerminateJobObject(self.job, 1);
            }
        }

        /// Wie viele Prozesse gerade zum Baum gehoeren.
        #[cfg(test)]
        pub fn active_processes(&self) -> u32 {
            use winapi::um::jobapi2::QueryInformationJobObject;
            use winapi::um::winnt::{
                JobObjectBasicAccountingInformation, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            };
            // SAFETY: genullte Struktur in passender Groesse
            unsafe {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
                QueryInformationJobObject(
                    self.job,
                    JobObjectBasicAccountingInformation,
                    &mut info as *mut _ as *mut _,
                    std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    std::ptr::null_mut(),
                );
                info.ActiveProcesses
            }
        }
    }

    impl Drop for ProcessTree {
        /// Schliesst den Job -- was dann noch laeuft, beendet das System.
        fn drop(&mut self) {
            // SAFETY: `job` stammt aus CreateJobObjectW und wird nur hier geschlossen.
            unsafe {
                CloseHandle(self.job);
            }
        }
    }
}

#[cfg(unix)]
mod imp {
    /// Haelt die Prozessnummer des gestarteten Prozesses fest.
    pub struct ProcessTree {
        root: u32,
    }

    impl ProcessTree {
        pub fn attach(child: &tokio::process::Child) -> Option<Self> {
            child.id().map(|root| Self { root })
        }

        /// Beendet den Prozess und alle Nachkommen. Die Tabelle wird gelesen,
        /// solange die Kette noch steht -- endet der erste Prozess zuerst,
        /// haengen seine Kinder nicht mehr an ihm.
        pub fn kill(&self) {
            let table = std::process::Command::new("ps")
                .args(["-A", "-o", "pid=", "-o", "ppid="])
                .output()
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
                .unwrap_or_default();
            let mut pids = super::descendants(&table, self.root);
            pids.push(self.root);
            for pid in pids {
                if let Ok(pid) = i32::try_from(pid) {
                    // SAFETY: kill(2) mit gueltiger Signalnummer; schlimmstenfalls ESRCH.
                    unsafe {
                        libc::kill(pid, libc::SIGKILL);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nachkommen_aus_der_prozesstabelle() {
        // 100 = yt-dlp (Entpacker), 101 = yt-dlp (laedt), 102 = ffmpeg,
        // 200 = fremder Prozess, 300 = Kind des fremden
        let table = "  1     0\n100     1\n101   100\n102   101\n200     1\n300   200\nkaputt\n\n";
        let mut found = descendants(table, 100);
        found.sort_unstable();
        assert_eq!(found, [101, 102]);
        assert!(descendants(table, 102).is_empty());
        assert!(descendants("", 100).is_empty());
    }

    #[test]
    fn schleife_in_der_tabelle_haengt_nicht() {
        // Kann bei wiederverwendeten Nummern zwischen zwei Lesungen vorkommen
        let mut found = descendants("10 20\n20 10\n", 10);
        found.sort_unstable();
        assert_eq!(found, [20]);
    }

    /// Ein Prozess, der ein Kind startet -- wie yt-dlp seinen zweiten Prozess.
    fn eltern_mit_kind() -> tokio::process::Command {
        #[cfg(windows)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("cmd");
            c.args(["/c", "ping -n 30 127.0.0.1 >NUL"]);
            c.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);
            c
        };
        #[cfg(unix)]
        let mut cmd = {
            let mut c = tokio::process::Command::new("sh");
            c.args(["-c", "sleep 30 & wait"]);
            c
        };
        cmd.kill_on_drop(true);
        cmd
    }

    #[tokio::test]
    async fn beendet_auch_das_kind() {
        let mut child = eltern_mit_kind().spawn().expect("Start");
        let tree = ProcessTree::attach(&child).expect("zuordnen");
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;

        #[cfg(windows)]
        assert!(tree.active_processes() >= 2, "Kind gehoert nicht zum Job");
        #[cfg(unix)]
        let kids = {
            let ps = std::process::Command::new("ps")
                .args(["-A", "-o", "pid=", "-o", "ppid="])
                .output()
                .unwrap();
            let kids = descendants(&String::from_utf8_lossy(&ps.stdout), child.id().unwrap());
            assert!(!kids.is_empty(), "kein Kind gefunden");
            kids
        };

        tree.kill();
        let status = tokio::time::timeout(std::time::Duration::from_secs(5), child.wait())
            .await
            .expect("Eltern laufen noch")
            .unwrap();
        assert!(!status.success());
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        #[cfg(windows)]
        assert_eq!(tree.active_processes(), 0, "Kind laeuft weiter");
        #[cfg(unix)]
        for pid in kids {
            // Weg oder nur noch als Zombie, bis der Uebernehmende ihn abholt
            let out = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            let stat = String::from_utf8_lossy(&out.stdout);
            assert!(
                stat.trim().is_empty() || stat.trim().starts_with('Z'),
                "Kind {pid} laeuft weiter: {stat}"
            );
        }
    }
}
