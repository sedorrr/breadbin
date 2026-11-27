//! BMC64 (Bare-Metal-VICE für den Raspberry Pi) über die HTTP-Schnittstelle seiner Weboberfläche.
//!
//! Geprüft mit BMC64 5.5.0/5.5.2 auf einem Raspberry Pi 3B+ (Ethernet und WLAN):
//! - `GET /api/status` (Version, Maschine, startbare Endungen), `GET /api/volumes`,
//!   `GET /api/fs/list|download`, `POST /api/fs/upload|autostart|delete|mkdir`.
//! - Pfade schreibt Breadbin wie beim Ultimate mit dem Laufwerk vorne: `/SD/breadbin/x.d64`
//!   wird zu `vol=SD&path=/breadbin/x.d64`.
//! - PIN (optional) als HTTP Basic Auth, Benutzername egal.
//! - Struktur-Aufrufe (`mkdir`, rekursives Löschen) brauchen die Kopfzeile `X-BMC64-Web: 1`.
//! - Der Server bedient eine Verbindung zur Zeit: alles nacheinander.
//! - `POST /api/reset` (Hard Reset, für „Spiel beenden“) am Pi 3B+ geprüft (2026-11-24): PRG endet beim
//!   Startbildschirm, ein per Autostart gestartetes Modul startet danach wieder.
//! - Autostart setzt die Maschine immer zurück; Diskwechsel ohne Neustart, Bildschirmfoto und
//!   Modul-Auswerfen gibt es nicht.
//! - 5.5.0 bricht größere Uploads oft mit 400 „upload truncated“ ab (behoben ab 5.5.1): wiederholen.
//! - Dateinamen nur ASCII (Breadbin legt ohnehin `Titel_1234abcd.ext` an).

use crate::ftp::RemoteEntry;
use crate::tr;
use anyhow::{Result, anyhow, bail};
use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

/// Älteste Fassung, mit der Uploads zuverlässig klappen.
const MIN_GOOD_VERSION: (u32, u32, u32) = (5, 5, 1);

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Status {
    pub version: String,
    /// Laufende Maschine: C64, C128, VIC20, Plus4, Plus4Emu, Pet.
    pub machine: String,
    /// Raspberry-Pi-Modell, z. B. „Raspberry Pi 3 Model B+“.
    pub model: String,
    /// Endungen, die diese Maschine per Autostart starten kann (klein geschrieben).
    pub run: Vec<String>,
}

impl Status {
    fn from_json(v: &Value) -> Option<Status> {
        if v["hostname"].as_str() != Some("bmc64") {
            return None;
        }
        let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
        Some(Status {
            version: s("version"),
            machine: s("machine"),
            model: s("model"),
            run: v["caps"]["run"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_ascii_lowercase))
                        .collect()
                })
                .unwrap_or_default(),
        })
    }

    /// Produktname für die Geräteliste.
    pub fn product(&self) -> String {
        if self.model.is_empty() {
            "BMC64".into()
        } else {
            format!("BMC64 ({})", self.model)
        }
    }
}

/// Hinweise zu einem BMC64: was damit nicht geht.
pub fn hints(st: &Status) -> Vec<String> {
    let mut out = Vec::new();
    if version_tuple(&st.version).is_some_and(|v| v < MIN_GOOD_VERSION) {
        out.push(tr!(
            "BMC64 {} bricht größere Uploads oft ab (behoben ab 5.5.1) – bitte BMC64 aktualisieren.",
            "BMC64 {} often aborts larger uploads (fixed in 5.5.1) – please update BMC64.",
            st.version
        ));
    }
    if !st.machine.is_empty() && st.machine != "C64" {
        out.push(tr!(
            "BMC64 läuft gerade als {}: Breadbin startet nur, was diese Maschine kann.",
            "BMC64 is currently running as {}: Breadbin only launches what that machine supports.",
            st.machine
        ));
    }
    out.push(tr!(
        "BMC64 kann keine Diskette ohne Neustart wechseln und liefert keine Bildschirmfotos. Ein eingelegtes Modul bleibt beim nächsten Start stecken – im BMC64-Menü entfernen.",
        "BMC64 cannot swap disks without a restart and provides no screenshots. An inserted cartridge stays in for the next launch – remove it in the BMC64 menu."
    ));
    out
}

fn version_tuple(v: &str) -> Option<(u32, u32, u32)> {
    let mut it = v
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

/// `/SD/ordner/datei` → (`SD`, `/ordner/datei`); `/SD` → (`SD`, `/`).
fn split_vol(path: &str) -> Result<(&str, String)> {
    let p = path.trim_start_matches('/');
    let (vol, rest) = p.split_once('/').unwrap_or((p, ""));
    if vol.is_empty() {
        bail!(tr!(
            "Pfad {path} nennt kein Laufwerk (z. B. /SD/…)",
            "Path {path} names no drive (e.g. /SD/…)"
        ));
    }
    Ok((vol, format!("/{}", rest.trim_end_matches('/'))))
}

/// `2026-11-20T18:10:30` → 20261120181030 (wie die MLSD-Zeit beim Ultimate).
fn parse_mtime(s: &str) -> i64 {
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).take(14).collect();
    if digits.len() == 14 {
        digits.parse().unwrap_or(0)
    } else {
        0
    }
}

pub struct Bmc64 {
    agent: ureq::Agent,
    host: String,
    base: String,
    auth: Option<String>,
}

enum Body<'a> {
    None,
    Bytes(&'a [u8]),
}

impl Bmc64 {
    pub fn new(host: &str, pin: &str) -> Bmc64 {
        Self::with_timeout(host, pin, Duration::from_secs(90))
    }

    fn with_timeout(host: &str, pin: &str, timeout: Duration) -> Bmc64 {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .timeout_connect(Some(Duration::from_secs(4).min(timeout)))
            .build()
            .into();
        let host = host.trim().to_string();
        Bmc64 {
            agent,
            base: format!("http://{host}"),
            host,
            auth: (!pin.is_empty()).then(|| {
                format!(
                    "Basic {}",
                    base64::engine::general_purpose::STANDARD.encode(format!("breadbin:{pin}"))
                )
            }),
        }
    }

    /// Ein Aufruf; gibt Status und Antwort zurück. Nicht erreichbar → verständlicher Fehler.
    fn request(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Body,
        web_header: bool,
        limit: u64,
    ) -> Result<(u16, Vec<u8>, String)> {
        let url = format!("{}{path}", self.base);
        let result = if method == "POST" {
            let mut req = self.agent.post(&url);
            for (k, v) in query {
                req = req.query(*k, *v);
            }
            if let Some(a) = &self.auth {
                req = req.header("Authorization", a);
            }
            if web_header {
                req = req.header("X-BMC64-Web", "1");
            }
            match body {
                Body::None => req.send_empty(),
                Body::Bytes(b) => req
                    .header("Content-Type", "application/octet-stream")
                    .send(b),
            }
        } else {
            let mut req = self.agent.get(&url);
            for (k, v) in query {
                req = req.query(*k, *v);
            }
            if let Some(a) = &self.auth {
                req = req.header("Authorization", a);
            }
            req.call()
        };
        let mut resp = match result {
            Ok(r) => r,
            Err(ureq::Error::Io(e)) => {
                return Err(crate::ftp::unreachable_error(&self.host, &e, "Web UI"));
            }
            Err(ureq::Error::Timeout(_)) | Err(ureq::Error::HostNotFound) => {
                return Err(anyhow!(tr!(
                    "Gerät {} ist nicht erreichbar – ist es eingeschaltet und im selben Netz?",
                    "Device {} is not reachable – is it switched on and on the same network?",
                    self.host
                )));
            }
            Err(e) => {
                return Err(anyhow!(tr!(
                    "Gerät {} antwortet nicht: {e}",
                    "Device {} does not answer: {e}",
                    self.host
                )));
            }
        };
        let status = resp.status().as_u16();
        let realm = resp
            .headers()
            .get("www-authenticate")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let mut data = Vec::new();
        resp.body_mut()
            .with_config()
            .limit(limit + 1)
            .reader()
            .take(limit + 1)
            .read_to_end(&mut data)?;
        if data.len() as u64 > limit {
            bail!(tr!(
                "Datei ist zu groß (über {} MB)",
                "File is too large (over {} MB)",
                limit / (1024 * 1024)
            ));
        }
        Ok((status, data, realm))
    }

    /// Wie `request`, aber nur 2xx gilt; sonst Fehler mit dem Text des Geräts.
    fn call(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, &str)],
        body: Body,
        web_header: bool,
        limit: u64,
    ) -> Result<Vec<u8>> {
        let (status, data, _) = self.request(method, path, query, body, web_header, limit)?;
        if (200..300).contains(&status) {
            return Ok(data);
        }
        Err(status_error(status, &data))
    }

    fn json(&self, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let data = self.call("GET", path, query, Body::None, false, 16 * 1024 * 1024)?;
        serde_json::from_slice(&data).map_err(|e| {
            anyhow!(tr!(
                "Unerwartete Antwort von BMC64: {e}",
                "Unexpected answer from BMC64: {e}"
            ))
        })
    }

    pub fn status(&self) -> Result<Status> {
        Status::from_json(&self.json("/api/status", &[])?)
            .ok_or_else(|| anyhow!(tr!("{} ist kein BMC64.", "{} is not a BMC64.", self.host)))
    }

    /// Laufwerke: `SD`, dazu eingesteckte USB-Sticks (`USB`, …).
    pub fn volumes(&self) -> Result<Vec<String>> {
        let v = self.json("/api/volumes", &[])?;
        Ok(v["volumes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Einträge eines Ordners; `/` liefert die Laufwerke als Ordner.
    pub fn list(&self, path: &str) -> Result<Vec<RemoteEntry>> {
        if path.trim_matches('/').is_empty() {
            return Ok(self
                .volumes()?
                .into_iter()
                .map(|name| RemoteEntry {
                    name,
                    is_dir: true,
                    size: 0,
                    modify: 0,
                })
                .collect());
        }
        let (vol, p) = split_vol(path)?;
        let v = self.json("/api/fs/list", &[("vol", vol), ("path", &p)])?;
        if v["truncated"].as_bool() == Some(true) {
            bail!(tr!(
                "Ordner {path} hat zu viele Einträge für BMC64 – bitte auf Unterordner verteilen.",
                "Folder {path} has too many entries for BMC64 – please split it into subfolders."
            ));
        }
        Ok(v["entries"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| {
                        Some(RemoteEntry {
                            name: e["name"].as_str()?.to_string(),
                            is_dir: e["dir"].as_bool().unwrap_or(false),
                            size: e["size"].as_u64().unwrap_or(0),
                            modify: parse_mtime(e["mtime"].as_str().unwrap_or("")),
                        })
                    })
                    .filter(|e| e.name != "." && e.name != "..")
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn download(&self, path: &str, limit: u64) -> Result<Vec<u8>> {
        let (vol, p) = split_vol(path)?;
        self.call(
            "GET",
            "/api/fs/download",
            &[("vol", vol), ("path", &p)],
            Body::None,
            false,
            limit,
        )
    }

    /// Lädt eine Datei hoch (überschreibt). Abgebrochene Uploads (BMC64 5.5.0) werden wiederholt.
    pub fn upload(&self, path: &str, data: &[u8]) -> Result<()> {
        let (vol, p) = split_vol(path)?;
        let mut last = None;
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(500 * attempt));
            }
            let (status, body, _) = self.request(
                "POST",
                "/api/fs/upload",
                &[("vol", vol), ("path", &p), ("overwrite", "1")],
                Body::Bytes(data),
                false,
                64 * 1024,
            )?;
            if (200..300).contains(&status) {
                return Ok(());
            }
            let err = status_error(status, &body);
            if !(status == 400 && String::from_utf8_lossy(&body).contains("truncated")) {
                return Err(err);
            }
            last = Some(err);
        }
        Err(last.unwrap())
    }

    /// Startet eine Datei wie der Menüpunkt „Autostart“ – setzt die Maschine zurück.
    pub fn autostart(&self, path: &str) -> Result<()> {
        let (vol, p) = split_vol(path)?;
        self.call(
            "POST",
            "/api/fs/autostart",
            &[("vol", vol), ("path", &p)],
            Body::None,
            false,
            64 * 1024,
        )
        .map(|_| ())
    }

    /// Hard Reset der Emulation (`POST /api/reset`). Ein per Autostart eingelegtes Modul bleibt
    /// stecken – BMC64 kann es über die Weboberfläche nicht auswerfen.
    pub fn reset(&self) -> Result<()> {
        self.call("POST", "/api/reset", &[], Body::None, false, 64 * 1024)
            .map(|_| ())
    }

    pub fn delete(&self, path: &str) -> Result<()> {
        let (vol, p) = split_vol(path)?;
        self.call(
            "POST",
            "/api/fs/delete",
            &[("vol", vol), ("path", &p)],
            Body::None,
            false,
            64 * 1024,
        )
        .map(|_| ())
    }

    /// Legt einen Ordner an; ein schon vorhandener Ordner ist kein Fehler.
    pub fn mkdir(&self, path: &str) -> Result<()> {
        let (vol, p) = split_vol(path)?;
        let (status, body, _) = self.request(
            "POST",
            "/api/fs/mkdir",
            &[("vol", vol), ("path", &p)],
            Body::None,
            true,
            64 * 1024,
        )?;
        if (200..300).contains(&status) || self.is_dir(path)? {
            return Ok(());
        }
        Err(status_error(status, &body))
    }

    /// Gibt es diesen Ordner? (über den Elternordner bzw. die Laufwerksliste)
    pub fn is_dir(&self, path: &str) -> Result<bool> {
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
}

fn status_error(status: u16, body: &[u8]) -> anyhow::Error {
    let text = String::from_utf8_lossy(body);
    let msg = serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| text.trim().chars().take(200).collect());
    if status == 401 {
        return anyhow!(tr!(
            "BMC64 verlangt eine PIN (oder sie ist falsch) – unter „Bearbeiten“ als Passwort eintragen.",
            "BMC64 requires a PIN (or it is wrong) – enter it as the password under “Edit”."
        ));
    }
    anyhow!(tr!(
        "BMC64 meldet {status}: {msg}",
        "BMC64 reports {status}: {msg}"
    ))
}

/// Ist unter `host` ein BMC64? `Ok(None)`: antwortet, ist aber keiner.
/// Mit gesetzter PIN und ohne (richtige) PIN: Fehler mit Hinweis auf die PIN.
pub fn detect(host: &str, pin: &str) -> Result<Option<Status>> {
    let c = Bmc64::with_timeout(host, pin, Duration::from_secs(6));
    let (status, data, realm) = c.request("GET", "/api/status", &[], Body::None, false, 1 << 20)?;
    if status == 401 && realm.contains("BMC64") {
        return Err(status_error(401, &data));
    }
    if !(200..300).contains(&status) {
        return Ok(None);
    }
    Ok(serde_json::from_slice::<Value>(&data)
        .ok()
        .as_ref()
        .and_then(Status::from_json))
}

/// Kurze Abfrage für Statusanzeige und Netzsuche: `Some` nur bei einem BMC64, der antwortet
/// (auch mit PIN-Sperre – dann ohne Angaben).
pub fn quick_status(host: &str, pin: &str) -> Option<Status> {
    let c = Bmc64::with_timeout(host, pin, Duration::from_millis(1500));
    let (status, data, realm) = c
        .request("GET", "/api/status", &[], Body::None, false, 1 << 20)
        .ok()?;
    if status == 401 && realm.contains("BMC64") {
        return Some(Status::default());
    }
    serde_json::from_slice::<Value>(&data)
        .ok()
        .as_ref()
        .and_then(Status::from_json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pfade_und_zeiten() {
        assert_eq!(
            split_vol("/SD/breadbin/x.d64").unwrap(),
            ("SD", "/breadbin/x.d64".into())
        );
        assert_eq!(split_vol("/USB").unwrap(), ("USB", "/".into()));
        assert_eq!(split_vol("/SD/a/").unwrap(), ("SD", "/a".into()));
        assert!(split_vol("/").is_err());
        assert_eq!(parse_mtime("2026-11-20T18:10:30"), 20261120181030);
        assert_eq!(parse_mtime(""), 0);
    }

    #[test]
    fn status_lesen() {
        let v: Value = serde_json::from_str(
            r#"{"hostname":"bmc64","version":"5.5.2","machine":"C64","model":"Raspberry Pi 3 Model B+",
                "caps":{"run":["d64","PRG","crt"]}}"#,
        )
        .unwrap();
        let st = Status::from_json(&v).unwrap();
        assert_eq!(st.run, ["d64", "prg", "crt"]);
        assert_eq!(st.product(), "BMC64 (Raspberry Pi 3 Model B+)");
        assert_eq!(hints(&st).len(), 1, "nur der allgemeine Hinweis");
        let alt = Status {
            version: "5.5.0".into(),
            machine: "C128".into(),
            ..st
        };
        assert_eq!(hints(&alt).len(), 3);
        assert!(Status::from_json(&serde_json::json!({"hostname":"router"})).is_none());
        assert_eq!(version_tuple("5.5"), Some((5, 5, 0)));
        assert_eq!(version_tuple("v5.5.2-beta"), Some((5, 5, 2)));
    }
}
