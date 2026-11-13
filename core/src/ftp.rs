//! Kleiner FTP-Client für die Ultimate-Geräte (Ultimate 64, 1541 Ultimate II+).
//!
//! Eigenheiten des Ultimate-FTP-Servers, auf die hier Rücksicht genommen wird:
//! - nur passiver Modus (PASV), EPSV wird abgelehnt,
//! - Dateinamen auf dem Steuerkanal in Latin-1 (solange der Server kein UTF8 meldet),
//! - ein nicht vorhandener Pfad liefert beim Auflisten eine leere Liste statt eines Fehlers,
//! - MLSD gibt es je nach Firmware; ohne MLSD wird LIST gelesen, aber keine Zeit daraus
//!   abgeleitet (Minutenauflösung, kein Jahr).
//!
//! Der Server verträgt nur wenige gleichzeitige Verbindungen: eine Verbindung für alles.

use crate::tr;
use anyhow::{Context, Result, anyhow, bail};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(6);

/// Verständliche Meldung, wenn ein Gerät nicht antwortet (meist: ausgeschaltet).
pub fn unreachable_error(host: &str, e: &std::io::Error, service: &str) -> anyhow::Error {
    use std::io::ErrorKind::*;
    // „Host is down“/„No route to host“ (macOS 64/65, Linux 112/113, Windows 10064/10065)
    // haben in Rust nicht überall eine eigene Fehlerart.
    let down = matches!(e.raw_os_error(), Some(64 | 65 | 112 | 113 | 10064 | 10065));
    match e.kind() {
        _ if down => anyhow!(tr!(
            "Gerät {host} ist nicht erreichbar – ist es eingeschaltet und im selben Netz? ({e})",
            "Device {host} is not reachable – is it switched on and on the same network? ({e})"
        )),
        ConnectionRefused => anyhow!(tr!(
            "Gerät {host} antwortet, aber {service} ist ausgeschaltet – im Ultimate-Menü unter \
             „Network Services“ einschalten.",
            "Device {host} answers, but {service} is switched off – turn it on in the Ultimate menu under \
             “Network Services”."
        )),
        HostUnreachable | NetworkUnreachable | TimedOut | WouldBlock | NetworkDown => anyhow!(tr!(
            "Gerät {host} ist nicht erreichbar – ist es eingeschaltet und im selben Netz? ({e})",
            "Device {host} is not reachable – is it switched on and on the same network? ({e})"
        )),
        _ => anyhow!(tr!(
            "Verbindung zu {host} fehlgeschlagen: {e}",
            "Connection to {host} failed: {e}"
        )),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RemoteEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Änderungszeit aus MLSD als Zahl `JJJJMMTThhmmss` (Ortszeit des Geräts), sonst 0.
    pub modify: i64,
}

pub struct Ftp {
    ctrl: BufReader<TcpStream>,
    host: String,
    utf8: bool,
    mlsd: bool,
}

fn encode(s: &str, utf8: bool) -> Vec<u8> {
    if utf8 {
        s.as_bytes().to_vec()
    } else {
        s.chars()
            .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
            .collect()
    }
}

fn decode(b: &[u8], utf8: bool) -> String {
    if utf8 {
        String::from_utf8_lossy(b).into_owned()
    } else {
        b.iter().map(|&c| c as char).collect()
    }
}

impl Ftp {
    pub fn connect(host: &str, password: &str) -> Result<Ftp> {
        let addr = (host, 21)
            .to_socket_addrs()?
            .next()
            .with_context(|| tr!("{host} nicht auflösbar", "Cannot resolve {host}"))?;
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
            .map_err(|e| unreachable_error(host, &e, "FTP"))?;
        stream.set_read_timeout(Some(TIMEOUT))?;
        stream.set_write_timeout(Some(TIMEOUT))?;
        let mut ftp = Ftp {
            ctrl: BufReader::new(stream),
            host: host.to_string(),
            utf8: false,
            mlsd: false,
        };
        ftp.expect(&[220])?;
        let (code, _) = ftp.cmd("USER anonymous")?;
        if code == 331 {
            ftp.cmd_expect(&format!("PASS {password}"), &[230, 202])?;
        } else if code != 230 {
            bail!(tr!(
                "FTP-Anmeldung abgelehnt ({code})",
                "FTP login refused ({code})"
            ));
        }
        if let Ok((211, feat)) = ftp.cmd("FEAT") {
            let feat = feat.to_uppercase();
            ftp.mlsd = feat.contains("MLSD");
            if feat.contains("UTF8") && ftp.cmd("OPTS UTF8 ON").is_ok_and(|(c, _)| c < 300) {
                ftp.utf8 = true;
            }
        }
        ftp.cmd_expect("TYPE I", &[200])?;
        Ok(ftp)
    }

    /// Liest eine (auch mehrzeilige) Antwort: Code und gesamter Text.
    fn reply(&mut self) -> Result<(u16, String)> {
        let mut text = String::new();
        let mut first_code: Option<String> = None;
        loop {
            let mut raw = Vec::new();
            let n = self.ctrl.read_until(b'\n', &mut raw)?;
            if n == 0 {
                bail!(tr!(
                    "FTP-Verbindung vom Gerät geschlossen",
                    "Device closed the FTP connection"
                ));
            }
            let line = decode(&raw, self.utf8);
            let line = line.trim_end_matches(['\r', '\n']);
            text.push_str(line);
            text.push('\n');
            let code = line.get(..3).unwrap_or("");
            match &first_code {
                None if line.as_bytes().get(3) == Some(&b'-') => {
                    first_code = Some(code.to_string())
                }
                None => return Ok((code.parse().unwrap_or(0), text)),
                Some(c)
                    if line.starts_with(c.as_str()) && line.as_bytes().get(3) == Some(&b' ') =>
                {
                    return Ok((c.parse().unwrap_or(0), text));
                }
                Some(_) => {}
            }
        }
    }

    fn expect(&mut self, ok: &[u16]) -> Result<String> {
        let (code, text) = self.reply()?;
        if !ok.contains(&code) {
            bail!(tr!(
                "FTP-Fehler {code}: {}",
                "FTP error {code}: {}",
                text.trim()
            ));
        }
        Ok(text)
    }

    fn cmd(&mut self, cmd: &str) -> Result<(u16, String)> {
        let mut line = encode(cmd, self.utf8);
        line.extend_from_slice(b"\r\n");
        self.ctrl.get_mut().write_all(&line)?;
        self.reply()
    }

    fn cmd_expect(&mut self, cmd: &str, ok: &[u16]) -> Result<String> {
        let (code, text) = self.cmd(cmd)?;
        if !ok.contains(&code) {
            // Kein Passwort in Fehlermeldungen.
            let shown = if cmd.starts_with("PASS") {
                "PASS ***"
            } else {
                cmd
            };
            bail!(tr!(
                "FTP-Fehler {code} bei „{shown}“: {}",
                "FTP error {code} for “{shown}”: {}",
                text.trim()
            ));
        }
        Ok(text)
    }

    /// Öffnet eine passive Datenverbindung und schickt den Befehl dazu.
    fn data(&mut self, cmd: &str) -> Result<TcpStream> {
        let text = self.cmd_expect("PASV", &[227])?;
        let (ip, port) =
            parse_pasv(&text).context(tr!("PASV-Antwort unlesbar", "Unreadable PASV reply"))?;
        let addr = (ip.as_str(), port)
            .to_socket_addrs()?
            .next()
            .context(tr!("PASV-Adresse", "PASV address"))?;
        let stream = TcpStream::connect_timeout(&addr, TIMEOUT)?;
        stream.set_read_timeout(Some(Duration::from_secs(60)))?;
        stream.set_write_timeout(Some(Duration::from_secs(60)))?;
        self.cmd_expect(cmd, &[125, 150])?;
        Ok(stream)
    }

    fn read_all(&mut self, cmd: &str, limit: u64) -> Result<Vec<u8>> {
        let stream = self.data(cmd).with_context(|| cmd.to_string())?;
        let mut data = Vec::new();
        stream.take(limit + 1).read_to_end(&mut data)?;
        if data.len() as u64 > limit {
            bail!(tr!(
                "Datei größer als {} MB",
                "File larger than {} MB",
                limit / 1024 / 1024
            ));
        }
        self.expect(&[226, 250])?;
        Ok(data)
    }

    pub fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>> {
        let raw = if self.mlsd {
            self.read_all(&format!("MLSD {path}"), 16 * 1024 * 1024)?
        } else {
            self.read_all(&format!("LIST {path}"), 16 * 1024 * 1024)?
        };
        let text = decode(&raw, self.utf8);
        let parse = if self.mlsd {
            parse_mlsd_line
        } else {
            parse_list_line
        };
        Ok(text
            .lines()
            .filter_map(parse)
            .filter(|e| e.name != "." && e.name != "..")
            .collect())
    }

    pub fn retrieve(&mut self, path: &str, limit: u64) -> Result<Vec<u8>> {
        self.read_all(&format!("RETR {path}"), limit)
    }

    pub fn store(&mut self, path: &str, data: &[u8]) -> Result<()> {
        let mut stream = self
            .data(&format!("STOR {path}"))
            .with_context(|| format!("STOR {path}"))?;
        stream
            .write_all(data)
            .context(tr!("Daten senden", "Sending data"))?;
        drop(stream); // Schließen beendet die Übertragung.
        // Das Schreiben auf den Stick kann beim ersten Mal deutlich länger dauern als die
        // übliche Antwortzeit (einmal > 15 s beobachtet): länger auf die Bestätigung warten.
        self.ctrl
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(90)))?;
        let result = self.expect(&[226, 250]).context(tr!(
            "Bestätigung nach dem Hochladen",
            "Confirmation after upload"
        ));
        self.ctrl.get_ref().set_read_timeout(Some(TIMEOUT))?;
        result.map(|_| ())
    }

    pub fn delete(&mut self, path: &str) -> Result<()> {
        self.cmd_expect(&format!("DELE {path}"), &[250, 200])
            .map(|_| ())
    }

    /// Legt einen Ordner an; ein schon vorhandener Ordner ist kein Fehler.
    pub fn mkdir(&mut self, path: &str) -> Result<()> {
        let (code, text) = self
            .cmd(&format!("MKD {path}"))
            .with_context(|| format!("MKD {path}"))?;
        if code == 257 || code == 250 || self.is_dir(path)? {
            return Ok(());
        }
        Err(anyhow!(tr!(
            "Ordner {path} lässt sich nicht anlegen ({code}): {}",
            "Cannot create folder {path} ({code}): {}",
            text.trim()
        )))
    }

    /// Prüft über den Elternordner, ob ein Ordner existiert (das Auflisten eines fehlenden
    /// Ordners liefert beim Ultimate nur eine leere Liste).
    pub fn is_dir(&mut self, path: &str) -> Result<bool> {
        let path = path.trim_end_matches('/');
        if path.is_empty() {
            return Ok(true);
        }
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        let parent = if parent.is_empty() { "/" } else { parent };
        Ok(self
            .list(parent)?
            .iter()
            .any(|e| e.is_dir && e.name.eq_ignore_ascii_case(name)))
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn quit(mut self) {
        let _ = self.cmd("QUIT");
    }
}

fn parse_pasv(text: &str) -> Option<(String, u16)> {
    let inner = &text[text.find('(')? + 1..text.find(')')?];
    let n: Vec<u16> = inner
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    if n.len() != 6 {
        return None;
    }
    Some((
        format!("{}.{}.{}.{}", n[0], n[1], n[2], n[3]),
        n[4] * 256 + n[5],
    ))
}

/// `type=file;size=174848;modify=20260821221700; name.d64`
fn parse_mlsd_line(line: &str) -> Option<RemoteEntry> {
    let (facts, name) = line.split_once(' ')?;
    let mut e = RemoteEntry {
        name: name.to_string(),
        is_dir: false,
        size: 0,
        modify: 0,
    };
    for fact in facts.split(';') {
        let Some((k, v)) = fact.split_once('=') else {
            continue;
        };
        match k.to_ascii_lowercase().as_str() {
            "type" => {
                e.is_dir = v.eq_ignore_ascii_case("dir")
                    || v.eq_ignore_ascii_case("cdir")
                    || v.eq_ignore_ascii_case("pdir")
            }
            "size" => e.size = v.parse().unwrap_or(0),
            "modify" => e.modify = v.get(..14).and_then(|t| t.parse().ok()).unwrap_or(0),
            _ => {}
        }
    }
    Some(e)
}

/// Unix-artige LIST-Zeile: `-rw-rw-rw-   1 user ftp   174848 Aug 21 22:17 Name mit Leerzeichen`
fn parse_list_line(line: &str) -> Option<RemoteEntry> {
    let mut rest = line;
    let mut fields = Vec::new();
    for _ in 0..8 {
        rest = rest.trim_start();
        let end = rest.find(' ')?;
        fields.push(&rest[..end]);
        rest = &rest[end..];
    }
    let name = rest.strip_prefix(' ').unwrap_or(rest).trim_start();
    if name.is_empty() {
        return None;
    }
    Some(RemoteEntry {
        name: name.to_string(),
        is_dir: fields[0].starts_with('d'),
        size: fields[4].parse().unwrap_or(0),
        modify: 0,
    })
}

/// Zerlegt `ftp://host/USB2/Spiele` in Host und Pfad.
pub fn split_location(location: &str) -> Option<(&str, &str)> {
    let rest = location.strip_prefix("ftp://")?;
    let slash = rest.find('/')?;
    Some((&rest[..slash], &rest[slash..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mlsd_und_list_zeilen() {
        let e =
            parse_mlsd_line("type=file;size=174848;modify=20260821221700; U64 Slide.d64").unwrap();
        assert_eq!(
            e,
            RemoteEntry {
                name: "U64 Slide.d64".into(),
                is_dir: false,
                size: 174848,
                modify: 20260821221700
            }
        );
        assert!(
            parse_mlsd_line("type=dir;size=0;modify=20260110083310; !Games")
                .unwrap()
                .is_dir
        );

        let e = parse_list_line(
            "-rw-rw-rw-   1 user     ftp       174848 Aug 21 22:17 U64Slide  2.d64",
        )
        .unwrap();
        assert_eq!(
            (e.name.as_str(), e.size, e.is_dir),
            ("U64Slide  2.d64", 174848, false)
        );
        let e = parse_list_line("drw-rw-rw-   1 user     ftp            0 Jan 10 08:33 !!Quantum")
            .unwrap();
        assert!(e.is_dir);
        assert!(parse_list_line("total 3").is_none());
    }

    #[test]
    fn pasv_und_ort() {
        assert_eq!(
            parse_pasv("227 Entering Passive Mode (192,0,2,168,19,137)."),
            Some(("192.0.2.168".into(), 5001))
        );
        assert_eq!(
            split_location("ftp://192.0.2.168/USB2/!Games"),
            Some(("192.0.2.168", "/USB2/!Games"))
        );
        assert_eq!(decode(&encode("Prüfung", false), false), "Prüfung");
    }
}
