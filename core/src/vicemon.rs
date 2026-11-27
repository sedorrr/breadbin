//! Laufendes VICE fernsteuern: Diskette einlegen und Bildschirmfoto über den Textmonitor.
//!
//! Breadbin startet VICE mit `-remotemonitor` auf einem freien Port (nur 127.0.0.1). Verbunden wird
//! erst beim ersten Knopfdruck, und die Verbindung bleibt dann offen, solange VICE läuft: VICE 3.9
//! (SDL, macOS) stürzt ab, sobald sich ein Monitor-Client trennt – auch wenn man sich in den ersten
//! Sekunden nach dem Start verbindet. Beendet sich Breadbin vorher, übernimmt unter macOS und Linux
//! ein kleiner Wächterprozess (`sleep`) die Verbindung, damit das Spiel weiterläuft
//! ([`hand_over_on_exit`]).
//!
//! Jeder Monitorbefehl hält die Emulation an; nach jedem Befehl folgt deshalb `x` (weiter).
//! Befehlszeilen über ~250 Zeichen schneidet VICE ab: lange Pfade gehen über eine Kopie im
//! Temp-Ordner.

use crate::tr;
use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// So lange nach dem Start nicht verbinden (früher stürzt VICE ab).
const STARTUP: Duration = Duration::from_secs(10);
/// Länger dürfen Pfade im Monitorbefehl nicht sein.
const MAX_PATH: usize = 200;

struct Session {
    port: u16,
    started: Instant,
    conn: Option<TcpStream>,
}

static SESSIONS: Mutex<Vec<Session>> = Mutex::new(Vec::new());

/// Freier Port auf 127.0.0.1 für den Monitor eines neuen VICE.
pub fn free_port() -> Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

/// Argumente, mit denen VICE den Monitor auf `port` öffnet.
pub fn monitor_args(port: u16) -> Vec<String> {
    vec![
        "-remotemonitor".into(),
        "-remotemonitoraddress".into(),
        format!("ip4://127.0.0.1:{port}"),
    ]
}

/// Ein eben gestartetes VICE merken. Ältere Sitzungen bleiben (ihre Verbindung darf nicht zugehen).
pub fn register(port: u16) {
    SESSIONS.lock().unwrap().push(Session {
        port,
        started: Instant::now(),
        conn: None,
    });
}

/// Gibt es ein von Breadbin gestartetes VICE (ob es noch läuft, zeigt erst der nächste Befehl)?
pub fn has_session() -> bool {
    !SESSIONS.lock().unwrap().is_empty()
}

/// Antwort lesen, bis der Monitor wieder auf einen Befehl wartet („(C:$e5cf) “ am Ende).
fn read_prompt(conn: &mut TcpStream) -> Result<String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match conn.read(&mut chunk) {
            Ok(0) => bail!("closed"),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e.into()),
        }
        let text = String::from_utf8_lossy(&buf);
        let last = text.rsplit('\n').next().unwrap_or("");
        if last.starts_with("(C:$") && last.ends_with(") ") {
            return Ok(text.into_owned());
        }
        if Instant::now() > deadline {
            bail!("timeout");
        }
    }
}

/// Befehl an das zuletzt gestartete VICE schicken und danach weiterlaufen lassen.
fn command(line: &str) -> Result<String> {
    let mut sessions = SESSIONS.lock().unwrap();
    let Some(s) = sessions.last_mut() else {
        bail!(tr!(
            "Kein laufendes VICE: Diskwechsel und Bildschirmfoto gehen nur mit einem VICE, das Breadbin gestartet hat.",
            "No running VICE: disk swaps and screenshots only work with a VICE started by Breadbin."
        ));
    };
    let wait = STARTUP.saturating_sub(s.started.elapsed());
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
    let gone = || {
        anyhow::anyhow!(tr!(
            "VICE läuft nicht mehr (oder wurde nicht von Breadbin gestartet).",
            "VICE is no longer running (or was not started by Breadbin)."
        ))
    };
    if s.conn.is_none() {
        match TcpStream::connect_timeout(&([127, 0, 0, 1], s.port).into(), Duration::from_secs(3)) {
            Ok(c) => {
                c.set_read_timeout(Some(Duration::from_millis(500)))?;
                s.conn = Some(c);
            }
            Err(_) => {
                sessions.pop();
                return Err(gone());
            }
        }
    }
    let conn = s.conn.as_mut().unwrap();
    let result = (|| -> Result<String> {
        conn.write_all(format!("{line}\n").as_bytes())?;
        let answer = read_prompt(conn)?;
        conn.write_all(b"x\n")?;
        Ok(answer)
    })();
    match result {
        Ok(a) => Ok(a),
        Err(e) if e.to_string() == "timeout" => Err(anyhow::anyhow!(tr!(
            "VICE antwortet nicht auf den Monitorbefehl.",
            "VICE does not answer the monitor command."
        ))),
        Err(_) => {
            // Verbindung weg = VICE beendet. Nicht schließen (das täte sie nur, wenn VICE noch liefe).
            if let Some(dead) = sessions.pop() {
                std::mem::forget(dead.conn);
            }
            Err(gone())
        }
    }
}

/// Pfad, den VICE im Monitorbefehl sicher liest: kurz genug, ohne Anführungszeichen. Sonst eine
/// Kopie im Temp-Ordner (Name aus der Prüfsumme bzw. dem Zweck).
fn monitor_path(path: &Path, short_name: &str) -> Result<PathBuf> {
    let s = path.to_string_lossy();
    if s.len() <= MAX_PATH && !s.contains('"') {
        return Ok(path.to_path_buf());
    }
    let dir = std::env::temp_dir().join("breadbin-vice");
    std::fs::create_dir_all(&dir)?;
    let copy = dir.join(short_name);
    if copy.to_string_lossy().len() > MAX_PATH {
        bail!(tr!(
            "Der Pfad ist für den VICE-Monitor zu lang.",
            "The path is too long for the VICE monitor."
        ));
    }
    if path.exists() {
        std::fs::copy(path, &copy)?;
    }
    Ok(copy)
}

/// Legt `image` in Laufwerk 8 des laufenden VICE ein (ohne Neustart).
pub fn attach(image: &Path, short_name: &str) -> Result<()> {
    let p = monitor_path(image, short_name)?;
    let answer = command(&format!("attach \"{}\" 8", p.display()))?;
    if answer.to_lowercase().contains("error") || answer.contains("cannot") {
        bail!(tr!(
            "VICE konnte die Diskette nicht einlegen.",
            "VICE could not insert the disk."
        ));
    }
    Ok(())
}

/// Bildschirmfoto des laufenden VICE als PNG. `crop` schneidet den Rahmen ab (320 × 200).
pub fn screenshot_png(crop: bool) -> Result<Vec<u8>> {
    let dir = std::env::temp_dir().join("breadbin-vice");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!("shot-{}.png", std::process::id()));
    let _ = std::fs::remove_file(&file);
    let target = monitor_path(&file, "shot.png")?;
    command(&format!("screenshot \"{}\" 2", target.display()))?;
    // VICE schreibt die Datei, während die Emulation steht; kurz warten, falls das Dateisystem trödelt.
    let deadline = Instant::now() + Duration::from_secs(3);
    let data = loop {
        if let Ok(d) = std::fs::read(&target)
            && !d.is_empty()
        {
            break d;
        }
        if Instant::now() > deadline {
            bail!(tr!(
                "VICE hat kein Bildschirmfoto geschrieben.",
                "VICE did not write a screenshot."
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = std::fs::remove_file(&target);
    let img = image::load_from_memory(&data)
        .context("Bildschirmfoto von VICE")?
        .to_rgb8();
    to_png(img, crop)
}

/// VICE-Bild (mit Rahmen, PAL 384 × 272, Bildschirm ab 32/35) → PNG wie beim Ultimate.
fn to_png(img: image::RgbImage, crop: bool) -> Result<Vec<u8>> {
    let img = if crop {
        let (w, h) = img.dimensions();
        anyhow::ensure!(w >= 320 && h >= 200, "Bild zu klein");
        // PAL wie gemessen; andere Größen (NTSC, anderer Rahmen) mittig.
        let (x0, y0) = if (w, h) == (384, 272) {
            (32, 35)
        } else {
            ((w - 320) / 2, (h - 200) / 2)
        };
        image::imageops::crop_imm(&img, x0, y0, 320, 200).to_image()
    } else {
        img
    };
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

/// Beim Beenden von Breadbin: offene Monitor-Verbindungen an einen Wächterprozess übergeben, damit
/// VICE nicht abstürzt (er hält eine Kopie der Verbindung höchstens einen Tag). Unter Windows geht
/// das so nicht; dort bleibt es beim Schließen.
pub fn hand_over_on_exit() {
    let mut sessions = SESSIONS.lock().unwrap();
    for s in sessions.iter_mut() {
        let Some(conn) = s.conn.take() else { continue };
        if !alive(&conn) {
            std::mem::forget(conn);
            continue;
        }
        keep(conn);
    }
}

/// Liegt auf der Verbindung noch kein Ende (VICE läuft)?
fn alive(conn: &TcpStream) -> bool {
    let mut b = [0u8; 1];
    if conn.set_nonblocking(true).is_err() {
        return false;
    }
    let r = conn.peek(&mut b);
    let _ = conn.set_nonblocking(false);
    match r {
        Ok(0) => false,
        Ok(_) => true,
        Err(e) => e.kind() == std::io::ErrorKind::WouldBlock,
    }
}

#[cfg(unix)]
fn keep(conn: TcpStream) {
    use std::os::fd::OwnedFd;
    use std::process::{Command, Stdio};
    let fd: OwnedFd = conn.into();
    let spawned = Command::new("sleep")
        .arg("86400")
        .stdin(Stdio::from(fd))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut child) = spawned {
        // Nicht warten; den Prozess nicht mit Breadbin beenden.
        std::thread::spawn(move || {
            let _ = child.try_wait();
        });
    }
}

#[cfg(not(unix))]
fn keep(conn: TcpStream) {
    // Kein Wächter: die Verbindung offen lassen, bis der Prozess endet.
    std::mem::forget(conn);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nachgebauter Monitor: antwortet auf jede Zeile mit einem Prompt.
    fn fake_monitor() -> (u16, std::thread::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let h = std::thread::spawn(move || {
            let (mut c, _) = l.accept().unwrap();
            let mut lines = Vec::new();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 512];
            while lines.len() < 2 {
                let n = c.read(&mut chunk).unwrap();
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                while let Some(i) = buf.iter().position(|&b| b == b'\n') {
                    let line = String::from_utf8_lossy(&buf[..i]).into_owned();
                    buf.drain(..=i);
                    if line != "x" {
                        c.write_all(b"\n(C:$e5cf) ").unwrap();
                    }
                    lines.push(line);
                }
            }
            lines
        });
        (port, h)
    }

    #[test]
    fn befehl_und_weiter() {
        let (port, h) = fake_monitor();
        SESSIONS.lock().unwrap().push(Session {
            port,
            started: Instant::now() - STARTUP,
            conn: None,
        });
        attach(Path::new("/tmp/a.d64"), "a.d64").unwrap();
        assert_eq!(h.join().unwrap(), ["attach \"/tmp/a.d64\" 8", "x"]);
        SESSIONS.lock().unwrap().clear();
    }

    /// Echtes VICE: `VICE_X64SC=/pfad/x64sc VICE_D64=/pfad/b.d64 cargo test -- --ignored echtes_vice`.
    /// Startet VICE mit Monitor, legt eine Diskette ein, macht ein Foto und übergibt die Verbindung;
    /// gibt die PID aus, damit man prüfen kann, dass VICE das Testende überlebt.
    #[test]
    #[ignore]
    fn echtes_vice() {
        let x64 = std::env::var("VICE_X64SC").unwrap();
        let d64 = std::env::var("VICE_D64").unwrap();
        let port = free_port().unwrap();
        let child = std::process::Command::new(x64)
            .args(["-default", "-minimized", "+sound", "-sounddev", "dummy"])
            .args(monitor_args(port))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        register(port);
        attach(Path::new(&d64), "b.d64").unwrap();
        let png = screenshot_png(true).unwrap();
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!((img.width(), img.height()), (320, 200));
        hand_over_on_exit();
        println!("VICE-PID {}", child.id());
        // VICE läuft absichtlich weiter (Übergabe prüfen); den Prozess nicht einsammeln.
        std::mem::forget(child);
    }

    #[test]
    fn zuschnitt_pal() {
        let mut img = image::RgbImage::new(384, 272);
        img.put_pixel(32, 35, image::Rgb([1, 2, 3]));
        let png = to_png(img, true).unwrap();
        let back = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(back.dimensions(), (320, 200));
        assert_eq!(back.get_pixel(0, 0), &image::Rgb([1, 2, 3]));
    }
}
