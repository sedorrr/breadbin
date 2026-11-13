//! Den Rechner während langer Arbeiten (Scan, Abruf) nicht einschlafen lassen. Der Bildschirm darf
//! aus; geschlossene Notebook-Deckel schicken den Rechner trotzdem schlafen.

/// Hält den Rechner wach, solange der Wert lebt. Auf dem Thread anlegen, der die Arbeit macht
/// (unter Windows gilt die Einstellung je Thread).
pub struct KeepAwake {
    #[cfg(unix)]
    child: Option<std::process::Child>,
}

impl KeepAwake {
    /// Gelingt das nicht (Werkzeug fehlt), läuft die Arbeit einfach ohne.
    pub fn start(why: &str) -> Self {
        #[cfg(target_os = "macos")]
        {
            let _ = why;
            // -i: kein Ruhezustand bei Untätigkeit; -w: endet spätestens mit diesem Programm.
            let child = std::process::Command::new("caffeinate")
                .args(["-i", "-w", &std::process::id().to_string()])
                .spawn()
                .ok();
            KeepAwake { child }
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            let child = std::process::Command::new("systemd-inhibit")
                .args([
                    "--what=idle:sleep",
                    "--who=Breadbin",
                    &format!("--why={why}"),
                    "--mode=block",
                    "sleep",
                    "infinity",
                ])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .ok();
            KeepAwake { child }
        }
        #[cfg(windows)]
        {
            let _ = why;
            use windows_sys::Win32::System::Power::{
                ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
            };
            // SAFETY: setzt nur einen Merker des aktuellen Threads.
            unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            KeepAwake {}
        }
    }
}

impl Drop for KeepAwake {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        #[cfg(windows)]
        {
            use windows_sys::Win32::System::Power::{ES_CONTINUOUS, SetThreadExecutionState};
            // SAFETY: wie oben.
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "macos")]
    fn caffeinate_laeuft_nur_solange_der_wert_lebt() {
        let alive = |pid: u32| {
            std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .unwrap()
                .success()
        };
        let a = super::KeepAwake::start("Test");
        let pid = a.child.as_ref().expect("caffeinate gestartet").id();
        assert!(alive(pid));
        drop(a);
        assert!(!alive(pid), "beendet, sobald der Wert weg ist");
    }
}
