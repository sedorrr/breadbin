//! BMX (Bare Metal X: VICE ohne Betriebssystem auf Raspberry Pi 4/5/400/500) über seine „Remote API“.
//!
//! **Ungetestet, Vorschau** – gebaut ohne echtes Gerät nach dem BMX-Quelltext (
//! `src/remote/bmx_api_router.cpp`, `src/remote/remote_service.cpp`, `src/kernel.cpp`,
//! `src/update/fat_path_policy.cpp`, zuletzt abgeglichen mit Fassung 2026.10.09.1) und den Antworten
//! des BMX-Entwicklers; geprüft nur mit dem
//! Schein-BMX (`mock`) in den Tests. Stellen, die weder Quelltext noch Entwickler bestätigt haben,
//! tragen den Kommentar `ANNAHME:`.
//!
//! ## Gesichert (Quelltext bzw. vom BMX-Entwickler bestätigt)
//! - Basis `http://<ip>/bmx/api/v1`; in BMX „Remote API“ einschalten. Passwort (falls gesetzt) als
//!   Kopfzeile `X-Password`; falsch/fehlend → 403 `{"error":"forbidden"}`.
//! - Pfade `VOL:/wurzel/…`, `VOL` ∈ SYS, USER, SD, USB, USB2, USB3, `wurzel` ∈ `disks`, `tapes`,
//!   `carts`, `snapshots` (sonst 400 `invalid_path`). Breadbin schreibt Pfade wie bei den anderen
//!   Geräten mit dem Laufwerk vorne (`/USER/disks/Breadbin/x.d64`), `api_path` setzt sie um.
//! - Dateien: `GET /files?path=…&limit=1..16&after=…` (Seiten zu höchstens 16), `GET|HEAD|DELETE|PUT
//!   /file?path=…` (Upload mit `Content-Length` und `X-BMX-SHA256`), `PUT /directory?path=…` legt den
//!   ganzen Pfad an. Eingelegte Dateien: 409 `file_in_use` beim Überschreiben/Löschen.
//! - Aktionen `POST /actions/<id>`: `media.autostart`, `media.drive.8.attach` (Inhalt
//!   `{"path":"USER:/…"}`), `media.cartridge.detach`, `media.drive.8.detach` (ohne Inhalt).
//! - `/bmx/api/v1` bleibt kompatibel: bestehende Endpunkte ändern sich nicht, es kommen nur neue
//!   dazu („work in progress“ meint fehlende Einstellungen).
//! - Fremdprogramme dürfen über `/files` und `/file` auflisten, herunterladen und löschen; erreichbar
//!   sind nur die Medienordner, Systemdateien nicht.
//! - Laufwerke: `SYS` gibt es immer (`SD` ist ein anderer Name dafür), `USER` nur, wenn die Karte eine
//!   zweite Partition hat. Vorrang: `USER`, wenn eingebunden, sonst `SYS`; beide haben `disks`,
//!   `tapes`, `carts`, `snapshots`. `GET /storage` nennt je Laufwerk `mounted` und `free_bytes`;
//!   ist das Laufwerk voll, antwortet `PUT /file` mit 507 `insufficient_space`.
//! - `media.autostart` ruft `emux_autostart_file` → VICEs `autostart_autodetect` (Disk, Band, Modul,
//!   PRG/P00), also auch TAP, G64 und CRT.
//! - `GET /state` (`machine`, `release_version`, `board`), `GET /media`.
//! - `GET /screenshot.ppm?width=160..3840` liefert das ganze zusammengesetzte Anzeigebild als binäres
//!   P6-PPM (Emulatorbild mit Rand, freie Fläche, ggf. CRT-Effekte, offenes Menü). Geplant ist
//!   `stage=emulator` für das reine Emulatorbild; ältere Fassungen antworten darauf mit 400
//!   `invalid_request` – Breadbin fragt dann ohne den Zusatz noch einmal.
//! - Netzsuche: UDP an Port 6464 `BMXDEV-DISCOVER/1 <32 Hex klein>\n`, Antwort
//!   `BMXDEV-HERE/1 <nonce> 80\n`; der Antworter läuft, sobald Remote API *oder* Entwicklermodus an ist.
//!
//! ## Annahmen (am Gerät noch zu prüfen)
//! - Autostart von TAP, G64 und CRT läuft am Gerät durch (Quelltext ja, ungeprüft).
//! - Module in `carts/`, Bänder in `tapes/` (siehe `dir_for`).
//! - Rahmenzuschnitt des Bildschirmfotos nach den VICE-PAL-Maßen (siehe `shot_to_png`).
//!
//! ## Für Mitwirkende
//! - Jede API-Funktion ist eine Methode von `Bmx`; Fehlertexte entstehen zentral in `status_error`.
//! - Der Start-Ablauf (Hochladen, Modul abziehen, Autostart/Einlegen) steht in
//!   `launch.rs::launch_bmx`, die Dateischicht (Scan, Kopieren, Leeren) in `remote.rs`.
//! - Was die Oberfläche anbietet, steuert `isBmx` in `ui/app.js` (`syncLaunchTarget`, `canShoot`).
//! - Neue Endpunkte zuerst im `mock` unten nachbilden, dann testen: `cargo test -p breadbin-core bmx`.
//! - Prüfliste für das echte Gerät: `docs/bmx-test.md` (deutsch: `docs/bmx-test.de.md`).

use crate::ftp::RemoteEntry;
use crate::tr;
use anyhow::{Result, anyhow, bail};
use serde::Serialize;
use serde_json::Value;
use std::io::Read;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

/// Ordner, die BMX unter einem Laufwerk zulässt.
pub const MEDIA_ROOTS: [&str; 4] = ["disks", "tapes", "carts", "snapshots"];
/// UDP-Port der Netzsuche.
pub const DISCOVERY_PORT: u16 = 6464;
/// Breite des Bildschirmfotos, das Breadbin anfordert.
const SHOT_WIDTH: u32 = 640;

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct State {
    /// Fassung, z. B. „v2026.08.19“.
    pub version: String,
    /// Laufende Maschine, z. B. „C64“.
    pub machine: String,
    /// Raspberry-Pi-Modell.
    pub board: String,
}

impl State {
    fn from_json(v: &Value) -> Option<State> {
        let s = |k: &str| v[k].as_str().map(str::to_string);
        Some(State {
            version: s("release_version")?,
            machine: s("machine")?,
            board: s("board").unwrap_or_default(),
        })
    }

    /// Produktname für die Geräteliste.
    pub fn product(&self) -> String {
        if self.board.is_empty() {
            "BMX".into()
        } else {
            format!("BMX ({})", self.board)
        }
    }
}

/// Hinweise zu einem BMX: was ungetestet ist und was nicht geht.
pub fn hints(st: &State) -> Vec<String> {
    let mut out = vec![tr!(
        "BMX-Unterstützung ist ungetestet (gebaut nach dem Quelltext von BMX, ohne echtes Gerät) – Rückmeldungen willkommen.",
        "BMX support is untested (built from the BMX source code, without a real device) – feedback welcome."
    )];
    if !st.machine.is_empty() && !st.machine.eq_ignore_ascii_case("C64") {
        out.push(tr!(
            "BMX läuft gerade als {}: Breadbin startet C64-Spiele – vorher im BMX-Menü auf C64 umstellen.",
            "BMX is currently running as {}: Breadbin launches C64 games – switch to C64 in the BMX menu first.",
            st.machine
        ));
    }
    out.push(tr!(
        "Startordner liegen bei BMX unter disks/, tapes/ oder carts/ eines Laufwerks: /USER/disks/Breadbin, wenn die Karte eine USER-Partition hat, sonst /SYS/disks/Breadbin. Module legt Breadbin nach carts/, Bänder nach tapes/.",
        "On BMX, launch folders live under disks/, tapes/ or carts/ of a drive: /USER/disks/Breadbin if the card has a USER partition, otherwise /SYS/disks/Breadbin. Breadbin puts cartridges into carts/ and tapes into tapes/."
    ));
    out
}

/// `/USER/disks/x.d64` → `USER:/disks/x.d64`; `/USER` → `USER:/`.
pub fn api_path(path: &str) -> Result<String> {
    let p = path.trim().trim_matches('/');
    let (vol, rest) = p.split_once('/').unwrap_or((p, ""));
    if vol.is_empty() {
        bail!(tr!(
            "Pfad {path} nennt kein Laufwerk (z. B. /USER/disks/…)",
            "Path {path} names no drive (e.g. /USER/disks/…)"
        ));
    }
    Ok(format!(
        "{}:/{}",
        vol.to_ascii_uppercase(),
        rest.trim_end_matches('/')
    ))
}

/// Teile eines Breadbin-Pfads ohne leere Stücke.
fn parts(path: &str) -> Vec<&str> {
    path.split('/').filter(|p| !p.is_empty()).collect()
}

fn is_media_root(name: &str) -> bool {
    MEDIA_ROOTS.iter().any(|r| r.eq_ignore_ascii_case(name))
}

/// Ordner für eine Dateiart neben dem Startordner: aus `/USER/disks/Breadbin` wird für Module
/// `/USER/carts/Breadbin`, für Bänder `/USER/tapes/Breadbin`. Liegt der Startordner nicht unter
/// einer BMX-Wurzel, bleibt er, wie er ist (BMX meldet dann selbst einen Fehler).
// ANNAHME: Module in `carts/`, Bänder in `tapes/` – so vom BMX-Projekt empfohlen; am Gerät ungeprüft.
pub fn dir_for(upload_dir: &str, format: &str) -> String {
    let root = match format {
        "CRT" => "carts",
        "TAP" => "tapes",
        _ => "disks",
    };
    let p = parts(upload_dir);
    if p.len() >= 2 && is_media_root(p[1]) {
        let mut out = vec![p[0], root];
        out.extend_from_slice(&p[2..]);
        format!("/{}", out.join("/"))
    } else {
        format!("/{}", p.join("/"))
    }
}

/// Alle Ordner, in die Breadbin bei diesem Startordner Dateien legt (Disketten, Module, Bänder).
pub fn launch_dirs(upload_dir: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in ["D64", "CRT", "TAP"] {
        let d = dir_for(upload_dir, f);
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}

/// Vorschlag für den Startordner aus den eingebundenen Laufwerken: `USER`, wenn es die Partition gibt,
/// sonst `SYS` (gibt es immer) – so vom BMX-Entwickler empfohlen.
pub fn suggest_upload_dir(drives: &[String]) -> String {
    let pick = drives
        .iter()
        .find(|d| d.eq_ignore_ascii_case("USER"))
        .map_or("SYS", |d| d.as_str());
    format!("/{pick}/disks/Breadbin")
}

/// Prozent-Kodierung für Abfragewerte (Leerzeichen als %20, nicht als „+“).
fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn query(pairs: &[(&str, &str)]) -> String {
    if pairs.is_empty() {
        return String::new();
    }
    let q: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{k}={}", pct(v)))
        .collect();
    format!("?{}", q.join("&"))
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(data))
}

pub struct Bmx {
    agent: ureq::Agent,
    host: String,
    base: String,
    password: String,
}

enum Body<'a> {
    None,
    Json(String),
    Bytes(&'a [u8]),
}

struct Answer {
    status: u16,
    data: Vec<u8>,
    length: Option<u64>,
}

impl Bmx {
    pub fn new(host: &str, password: &str) -> Bmx {
        Self::with_timeout(host, password, Duration::from_secs(90))
    }

    fn with_timeout(host: &str, password: &str, timeout: Duration) -> Bmx {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .timeout_connect(Some(Duration::from_secs(4).min(timeout)))
            .build()
            .into();
        let host = host.trim().to_string();
        Bmx {
            agent,
            base: format!("http://{host}/bmx/api/v1"),
            host,
            password: password.to_string(),
        }
    }

    /// Ein Aufruf; gibt Status und Antwort zurück. Nicht erreichbar → verständlicher Fehler.
    fn request(
        &self,
        method: &str,
        path: &str,
        q: &[(&str, &str)],
        body: Body,
        limit: u64,
    ) -> Result<Answer> {
        let url = format!("{}{path}{}", self.base, query(q));
        let pw = (!self.password.is_empty()).then_some(self.password.as_str());
        let result = match method {
            "PUT" | "POST" => {
                let mut req = if method == "PUT" {
                    self.agent.put(&url)
                } else {
                    self.agent.post(&url)
                };
                if let Some(p) = pw {
                    req = req.header("X-Password", p);
                }
                match body {
                    Body::None => req.send_empty(),
                    Body::Json(j) => req.header("Content-Type", "application/json").send(&j),
                    Body::Bytes(b) => req
                        .header("Content-Type", "application/octet-stream")
                        .header("X-BMX-SHA256", &sha256_hex(b))
                        .send(b),
                }
            }
            _ => {
                let mut req = match method {
                    "HEAD" => self.agent.head(&url),
                    "DELETE" => self.agent.delete(&url),
                    _ => self.agent.get(&url),
                };
                if let Some(p) = pw {
                    req = req.header("X-Password", p);
                }
                req.call()
            }
        };
        let mut resp = match result {
            Ok(r) => r,
            Err(ureq::Error::Io(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                bail!(tr!(
                    "Gerät {} antwortet, aber die Remote API von BMX ist aus – im BMX-Menü einschalten.",
                    "Device {} answers, but the BMX Remote API is off – turn it on in the BMX menu.",
                    self.host
                ));
            }
            Err(ureq::Error::Io(e)) => {
                return Err(crate::ftp::unreachable_error(&self.host, &e, "Remote API"));
            }
            Err(ureq::Error::Timeout(_)) | Err(ureq::Error::HostNotFound) => {
                bail!(tr!(
                    "Gerät {} ist nicht erreichbar – ist es eingeschaltet und im selben Netz?",
                    "Device {} is not reachable – is it switched on and on the same network?",
                    self.host
                ));
            }
            Err(e) => {
                bail!(tr!(
                    "Gerät {} antwortet nicht: {e}",
                    "Device {} does not answer: {e}",
                    self.host
                ));
            }
        };
        let status = resp.status().as_u16();
        let length = resp
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok());
        let mut data = Vec::new();
        if method != "HEAD" {
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
        }
        Ok(Answer {
            status,
            data,
            length,
        })
    }

    /// Wie `request`, aber nur 2xx gilt; sonst Fehler mit dem Text des Geräts.
    fn call(
        &self,
        method: &str,
        path: &str,
        q: &[(&str, &str)],
        body: Body,
        limit: u64,
    ) -> Result<Vec<u8>> {
        let a = self.request(method, path, q, body, limit)?;
        if (200..300).contains(&a.status) {
            return Ok(a.data);
        }
        Err(status_error(a.status, &a.data))
    }

    fn json(&self, path: &str, q: &[(&str, &str)]) -> Result<Value> {
        let data = self.call("GET", path, q, Body::None, 1 << 20)?;
        serde_json::from_slice(&data).map_err(|e| {
            anyhow!(tr!(
                "Unerwartete Antwort von BMX: {e}",
                "Unexpected answer from BMX: {e}"
            ))
        })
    }

    pub fn state(&self) -> Result<State> {
        State::from_json(&self.json("/state", &[])?)
            .ok_or_else(|| anyhow!(tr!("{} ist kein BMX.", "{} is not a BMX.", self.host)))
    }

    /// Eingebundene Laufwerke mit freiem Platz (`None`, wenn BMX keinen nennt). `SD` ist bei BMX nur
    /// ein anderer Name für `SYS` und fällt weg, wenn `SYS` dabei ist.
    fn storage(&self) -> Result<Vec<(String, Option<u64>)>> {
        let v = self.json("/storage", &[])?;
        let all: Vec<(String, Option<u64>)> = v["volumes"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|x| x["mounted"].as_bool().unwrap_or(true))
                    .filter_map(|x| {
                        Some((x["name"].as_str()?.to_string(), x["free_bytes"].as_u64()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let has_sys = all.iter().any(|(n, _)| n.eq_ignore_ascii_case("SYS"));
        Ok(all
            .into_iter()
            .filter(|(n, _)| !(has_sys && n.eq_ignore_ascii_case("SD")))
            .collect())
    }

    /// Eingebundene Laufwerke (`SYS`, `USER`, `USB` …).
    pub fn volumes(&self) -> Result<Vec<String>> {
        Ok(self.storage()?.into_iter().map(|(n, _)| n).collect())
    }

    /// Prüft vor dem Hochladen, ob auf dem Laufwerk von `path` genug Platz ist. Nennt BMX keinen
    /// freien Platz oder kennt das Laufwerk nicht, entscheidet das Gerät selbst beim Hochladen.
    fn check_space(&self, path: &str, needed: u64) -> Result<()> {
        let p = parts(path);
        let Some(vol) = p.first() else { return Ok(()) };
        let vol = if vol.eq_ignore_ascii_case("SD") {
            "SYS"
        } else {
            vol
        };
        let free = self
            .storage()?
            .into_iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(vol))
            .and_then(|(_, f)| f);
        match free {
            Some(free) if free < needed => bail!(tr!(
                "Auf dem BMX-Laufwerk {vol} ist nicht genug Platz: frei {} KB, nötig {} KB.",
                "Not enough space on BMX drive {vol}: {} KB free, {} KB needed.",
                free / 1024,
                needed.div_ceil(1024)
            )),
            _ => Ok(()),
        }
    }

    /// Einträge eines Ordners; `/` liefert die Laufwerke, `/USER` die BMX-Wurzeln als Ordner.
    pub fn list(&self, path: &str) -> Result<Vec<RemoteEntry>> {
        let dir = |name: String| RemoteEntry {
            name,
            is_dir: true,
            size: 0,
            modify: 0,
        };
        let p = parts(path);
        match p.len() {
            0 => return Ok(self.volumes()?.into_iter().map(dir).collect()),
            1 => return Ok(MEDIA_ROOTS.iter().map(|r| dir(r.to_string())).collect()),
            _ => {}
        }
        let api = api_path(path)?;
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        // Höchstens 16 Einträge je Seite; Obergrenze gegen eine endlose Schleife.
        for _ in 0..10_000 {
            let mut q = vec![("path", api.as_str()), ("limit", "16")];
            if let Some(a) = after.as_deref() {
                q.push(("after", a));
            }
            let v = self.json("/files", &q)?;
            for e in v["entries"].as_array().into_iter().flatten() {
                let Some(name) = e["name"].as_str() else {
                    continue;
                };
                out.push(RemoteEntry {
                    name: name.to_string(),
                    is_dir: e["type"].as_str() == Some("directory"),
                    size: e["size"].as_u64().unwrap_or(0),
                    modify: 0,
                });
            }
            match v["next_after"].as_str() {
                Some(n) if after.as_deref() != Some(n) => after = Some(n.to_string()),
                _ => return Ok(out),
            }
        }
        Ok(out)
    }

    pub fn download(&self, path: &str, limit: u64) -> Result<Vec<u8>> {
        self.call(
            "GET",
            "/file",
            &[("path", &api_path(path)?)],
            Body::None,
            limit,
        )
    }

    /// Größe einer Datei auf dem Gerät, `None` wenn es sie nicht gibt.
    pub fn size_of(&self, path: &str) -> Result<Option<u64>> {
        let a = self.request(
            "HEAD",
            "/file",
            &[("path", &api_path(path)?)],
            Body::None,
            0,
        )?;
        match a.status {
            200..=299 => Ok(Some(a.length.unwrap_or(0))),
            404 => Ok(None),
            s => Err(status_error(s, &a.data)),
        }
    }

    /// Lädt eine Datei hoch (überschreibt). Eine gerade eingelegte Datei lehnt BMX ab.
    pub fn upload(&self, path: &str, data: &[u8]) -> Result<()> {
        self.call(
            "PUT",
            "/file",
            &[("path", &api_path(path)?)],
            Body::Bytes(data),
            64 * 1024,
        )
        .map(|_| ())
    }

    /// Lädt hoch, außer die Datei liegt mit gleicher Größe schon dort. Breadbin nimmt die Prüfsumme
    /// in den Dateinamen, gleicher Name und gleiche Größe heißen also gleicher Inhalt – und eine
    /// gerade eingelegte Diskette ließe sich ohnehin nicht überschreiben.
    pub fn upload_if_missing(&self, path: &str, data: &[u8]) -> Result<bool> {
        let old = self.size_of(path)?;
        if old == Some(data.len() as u64) {
            return Ok(false);
        }
        self.check_space(path, (data.len() as u64).saturating_sub(old.unwrap_or(0)))?;
        self.upload(path, data)?;
        Ok(true)
    }

    pub fn delete(&self, path: &str) -> Result<()> {
        self.call(
            "DELETE",
            "/file",
            &[("path", &api_path(path)?)],
            Body::None,
            64 * 1024,
        )
        .map(|_| ())
    }

    /// Legt einen Ordner samt Elternordnern an; vorhandene Ordner sind kein Fehler. Laufwerk und
    /// BMX-Wurzel (`/USER/disks`) lassen sich nicht anlegen – die gibt es oder BMX lehnt den Pfad ab.
    pub fn mkdir(&self, path: &str) -> Result<()> {
        if parts(path).len() <= 2 {
            return Ok(());
        }
        self.call(
            "PUT",
            "/directory",
            &[("path", &api_path(path)?)],
            Body::None,
            64 * 1024,
        )
        .map(|_| ())
    }

    /// Gibt es diesen Ordner?
    pub fn is_dir(&self, path: &str) -> Result<bool> {
        let p = parts(path);
        match p.len() {
            0 => return Ok(true),
            1 => {
                return Ok(self.volumes()?.iter().any(|v| v.eq_ignore_ascii_case(p[0])));
            }
            2 => return Ok(is_media_root(p[1])),
            _ => {}
        }
        let api = api_path(path)?;
        let a = self.request(
            "GET",
            "/files",
            &[("path", &api), ("limit", "1")],
            Body::None,
            1 << 20,
        )?;
        match a.status {
            200..=299 => Ok(true),
            404 | 409 => Ok(false),
            s => Err(status_error(s, &a.data)),
        }
    }

    fn action(&self, id: &str, path: Option<&str>) -> Result<()> {
        let body = match path {
            Some(p) => Body::Json(serde_json::json!({ "path": api_path(p)? }).to_string()),
            None => Body::None,
        };
        let a = self.request("POST", &format!("/actions/{id}"), &[], body, 1 << 20)?;
        if (200..300).contains(&a.status) {
            return Ok(());
        }
        if a.status == 404 && error_code(&a.data).as_deref() == Some("not_found") {
            bail!(tr!(
                "Diese BMX-Fassung kennt die Aktion „{id}“ nicht – bitte BMX aktualisieren.",
                "This BMX version does not know the action “{id}” – please update BMX."
            ));
        }
        Err(status_error(a.status, &a.data))
    }

    /// Startet eine Datei wie „Autostart“ im BMX-Menü (setzt die Maschine zurück).
    /// Gilt für alle Formate (VICE `autostart_autodetect`); am Gerät ungeprüft.
    pub fn autostart(&self, path: &str) -> Result<()> {
        self.action("media.autostart", Some(path))
    }

    /// Legt ein Disk-Abbild in Laufwerk 8 ein, ohne Neustart.
    pub fn attach_disk(&self, path: &str) -> Result<()> {
        self.action("media.drive.8.attach", Some(path))
    }

    /// Zieht ein eingestecktes Modul ab (sonst startet es beim nächsten Autostart wieder).
    pub fn detach_cartridge(&self) -> Result<()> {
        self.action("media.cartridge.detach", None)
    }

    /// Eingelegte Datenträger: (Platz, Art, Pfad wie bei Breadbin).
    pub fn media(&self) -> Result<Vec<(String, String, String)>> {
        let v = self.json("/media", &[])?;
        Ok(v["slots"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                let t = |k: &str| s[k].as_str().unwrap_or("").to_string();
                (t("key"), t("kind"), breadbin_path(&t("path")))
            })
            .collect())
    }

    /// Bildschirmfoto als PNG. `crop` schneidet den C64-Rahmen ab (320 × 200).
    /// Fragt zuerst nach dem reinen Emulatorbild (`stage=emulator`, vom BMX-Entwickler geplant); kennt
    /// die BMX-Fassung den Zusatz nicht (400 `invalid_request`), noch einmal nach dem ganzen Anzeigebild.
    pub fn screenshot_png(&self, crop: bool) -> Result<Vec<u8>> {
        let width = SHOT_WIDTH.to_string();
        let limit = 64 << 20;
        let a = self.request(
            "GET",
            "/screenshot.ppm",
            &[("width", &width), ("stage", "emulator")],
            Body::None,
            limit,
        )?;
        let data = if (200..300).contains(&a.status) {
            a.data
        } else if a.status == 400 {
            self.call(
                "GET",
                "/screenshot.ppm",
                &[("width", &width)],
                Body::None,
                limit,
            )?
        } else {
            return Err(status_error(a.status, &a.data));
        };
        let img = ppm_to_image(&data)?;
        shot_to_png(img, crop)
    }
}

/// `USER:/disks/x.d64` → `/USER/disks/x.d64` (leer bleibt leer).
pub fn breadbin_path(api: &str) -> String {
    match api.split_once(":/") {
        Some((vol, rest)) => format!("/{vol}/{rest}").trim_end_matches('/').to_string(),
        None => api.to_string(),
    }
}

fn error_code(body: &[u8]) -> Option<String> {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_string))
}

fn status_error(status: u16, body: &[u8]) -> anyhow::Error {
    let code = error_code(body).unwrap_or_else(|| {
        String::from_utf8_lossy(body)
            .trim()
            .chars()
            .take(200)
            .collect()
    });
    match (status, code.as_str()) {
        (403, _) => anyhow!(tr!(
            "BMX verlangt ein Passwort (oder es ist falsch) – unter „Bearbeiten“ eintragen.",
            "BMX requires a password (or it is wrong) – enter it under “Edit”."
        )),
        (409, "file_in_use") => anyhow!(tr!(
            "Die Datei ist in BMX gerade eingelegt und lässt sich nicht ändern oder löschen.",
            "The file is currently inserted in BMX and cannot be changed or deleted."
        )),
        (409, "busy") => anyhow!(tr!(
            "BMX ist gerade beschäftigt – bitte gleich noch einmal versuchen.",
            "BMX is busy right now – please try again in a moment."
        )),
        (503, "safe_point_timeout") => anyhow!(tr!(
            "BMX hat nicht rechtzeitig geantwortet (ist das BMX-Menü offen?).",
            "BMX did not answer in time (is the BMX menu open?)."
        )),
        (400, "invalid_path") => anyhow!(tr!(
            "BMX lehnt den Pfad ab – Startordner müssen unter disks/, tapes/ oder carts/ eines Laufwerks liegen (z. B. /USER/disks/Breadbin).",
            "BMX rejects the path – launch folders must be under disks/, tapes/ or carts/ of a drive (e.g. /USER/disks/Breadbin)."
        )),
        (507, _) => anyhow!(tr!(
            "Auf dem BMX-Laufwerk ist nicht genug Platz.",
            "Not enough space on the BMX drive."
        )),
        (422, "hash_mismatch") => anyhow!(tr!(
            "Die Datei kam beschädigt bei BMX an (Prüfsumme falsch) – bitte noch einmal versuchen.",
            "The file arrived damaged at BMX (checksum wrong) – please try again."
        )),
        (404, "volume_unavailable") => anyhow!(tr!(
            "Das Laufwerk gibt es in BMX nicht (USB-Stick eingesteckt? USER gibt es nur mit zweiter Partition auf der Karte).",
            "The drive does not exist in BMX (USB stick plugged in? USER only exists with a second partition on the card)."
        )),
        _ => anyhow!(tr!(
            "BMX meldet {status}: {code}",
            "BMX reports {status}: {code}"
        )),
    }
}

// ---------------------------------------------------------------- Bildschirmfoto

/// Liest ein binäres PPM (P6, Höchstwert 255).
pub fn ppm_to_image(data: &[u8]) -> Result<image::RgbImage> {
    let bad = || {
        anyhow!(tr!(
            "Bildschirmfoto von BMX unlesbar",
            "Unreadable screenshot from BMX"
        ))
    };
    let mut pos = 0usize;
    let mut token = || -> Option<String> {
        loop {
            while pos < data.len() && data[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos < data.len() && data[pos] == b'#' {
                while pos < data.len() && data[pos] != b'\n' {
                    pos += 1;
                }
                continue;
            }
            break;
        }
        let start = pos;
        while pos < data.len() && !data[pos].is_ascii_whitespace() {
            pos += 1;
        }
        (pos > start).then(|| String::from_utf8_lossy(&data[start..pos]).into_owned())
    };
    if token().as_deref() != Some("P6") {
        return Err(bad());
    }
    let w: u32 = token().and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    let h: u32 = token().and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    let max: u32 = token().and_then(|t| t.parse().ok()).ok_or_else(bad)?;
    if max != 255 || w == 0 || h == 0 || w > 8192 || h > 8192 {
        return Err(bad());
    }
    let start = pos + 1; // genau ein Leerzeichen nach dem Höchstwert
    let need = w as usize * h as usize * 3;
    let pixels = data.get(start..start + need).ok_or_else(bad)?;
    image::RgbImage::from_raw(w, h, pixels.to_vec()).ok_or_else(bad)
}

/// Schneidet schwarze Balken der Anzeige ab – nur paarweise (links/rechts, oben/unten gleich viel),
/// damit ein schwarzer C64-Rahmen nicht einseitig verschwindet.
fn strip_bars(img: &image::RgbImage) -> (u32, u32, u32, u32) {
    let (w, h) = img.dimensions();
    let black = |x: u32, y: u32| img.get_pixel(x, y).0 == [0, 0, 0];
    let row_black = |y: u32| (0..w).all(|x| black(x, y));
    let col_black = |x: u32| (0..h).all(|y| black(x, y));
    let top = (0..h).take_while(|&y| row_black(y)).count() as u32;
    if top == h {
        return (0, 0, w, h); // ganz schwarz: nichts abschneiden
    }
    let bottom = (0..h).rev().take_while(|&y| row_black(y)).count() as u32;
    let left = (0..w).take_while(|&x| col_black(x)).count() as u32;
    let right = (0..w).rev().take_while(|&x| col_black(x)).count() as u32;
    let v = top.min(bottom);
    let hz = left.min(right);
    (hz, v, w - 2 * hz, h - 2 * v)
}

/// Bild der Anzeige → PNG wie beim Ultimate: ohne `crop` 384 Punkte breit (mit Rahmen), mit `crop`
/// 320 × 200. Den Rahmen schneidet Breadbin nach den Maßen des üblichen VICE-PAL-Bildes ab
/// (384 × 272, Bildschirm ab 32/35) – bei BMX ungetestet.
// ANNAHME: Die Emulatorebene zeigt das übliche VICE-PAL-Bild; bei anderen Rahmen-Einstellungen,
// Shadern oder NTSC sitzt der Zuschnitt daneben.
pub fn shot_to_png(img: image::RgbImage, crop: bool) -> Result<Vec<u8>> {
    use image::imageops::{self, FilterType};
    let (x, y, w, h) = strip_bars(&img);
    let pic = imageops::crop_imm(&img, x, y, w, h).to_image();
    let out = if crop {
        let cx = w * 32 / 384;
        let cy = h * 35 / 272;
        let cw = (w * 320 / 384).max(1);
        let ch = (h * 200 / 272).max(1);
        let inner = imageops::crop_imm(&pic, cx, cy, cw, ch).to_image();
        imageops::resize(&inner, 320, 200, FilterType::Triangle)
    } else {
        let nh = ((h as u64 * 384) / w.max(1) as u64).max(1) as u32;
        imageops::resize(&pic, 384, nh, FilterType::Triangle)
    };
    let mut buf = std::io::Cursor::new(Vec::new());
    out.write_to(&mut buf, image::ImageFormat::Png)?;
    Ok(buf.into_inner())
}

// ---------------------------------------------------------------- Erkennung

/// Ist unter `host` ein BMX mit eingeschalteter Remote API? `Ok(None)`: antwortet, ist aber keiner.
/// Mit Passwort am Gerät und ohne (richtiges) Passwort: Fehler mit Hinweis darauf.
pub fn detect(host: &str, password: &str) -> Result<Option<State>> {
    let c = Bmx::with_timeout(host, password, Duration::from_secs(4));
    let a = c.request("GET", "/state", &[], Body::None, 1 << 20)?;
    if a.status == 403 && error_code(&a.data).as_deref() == Some("forbidden") {
        return Err(status_error(403, &a.data));
    }
    if !(200..300).contains(&a.status) {
        return Ok(None);
    }
    Ok(serde_json::from_slice::<Value>(&a.data)
        .ok()
        .as_ref()
        .and_then(State::from_json))
}

/// Kurze Abfrage für Statusanzeige und Netzsuche: `Some` nur bei einem BMX, der antwortet (auch mit
/// Passwort-Sperre – dann ohne Angaben).
pub fn quick_status(host: &str, password: &str) -> Option<State> {
    let c = Bmx::with_timeout(host, password, Duration::from_millis(1500));
    let a = c.request("GET", "/state", &[], Body::None, 1 << 20).ok()?;
    if a.status == 403 && error_code(&a.data).as_deref() == Some("forbidden") {
        return Some(State::default());
    }
    serde_json::from_slice::<Value>(&a.data)
        .ok()
        .as_ref()
        .and_then(State::from_json)
}

/// Zufällige Kennung für die Netzsuche: 32 Hex-Zeichen, klein.
fn nonce() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..2u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

/// Prüft eine Antwort der Netzsuche: `BMXDEV-HERE/1 <nonce> <port>\n` → Port.
fn parse_reply(data: &[u8], nonce: &str) -> Option<u16> {
    let text = std::str::from_utf8(data).ok()?.strip_suffix('\n')?;
    let mut it = text.split(' ');
    (it.next()? == "BMXDEV-HERE/1" && it.next()? == nonce).then_some(())?;
    let port = it.next()?.parse().ok()?;
    it.next().is_none().then_some(port)
}

/// Fragt per UDP an die Ziele und sammelt die Absender gültiger Antworten (Adresse, HTTP-Port).
pub fn discover_at(targets: &[SocketAddr], wait: Duration) -> Vec<(IpAddr, u16)> {
    let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
        return Vec::new();
    };
    let _ = sock.set_broadcast(true);
    let n = nonce();
    let probe = format!("BMXDEV-DISCOVER/1 {n}\n");
    for t in targets {
        let _ = sock.send_to(probe.as_bytes(), t);
    }
    let mut found: Vec<(IpAddr, u16)> = Vec::new();
    let end = Instant::now() + wait;
    let mut buf = [0u8; 256];
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if left.is_zero() || sock.set_read_timeout(Some(left)).is_err() {
            break;
        }
        match sock.recv_from(&mut buf) {
            Ok((len, from)) => {
                if let Some(port) = parse_reply(&buf[..len], &n)
                    && !found.iter().any(|(ip, _)| *ip == from.ip())
                {
                    found.push((from.ip(), port));
                }
            }
            Err(_) => break,
        }
    }
    found
}

/// Netzsuche: Broadcast an die Netze aller aktiven IPv4-Schnittstellen und an 255.255.255.255.
/// Gibt die Adressen der BMX zurück (mit `:port`, falls nicht 80).
// Antwortet, sobald die Remote API an ist (vom BMX-Entwickler bestätigt).
// Das /24-Netz der übergebenen Adresse bleibt als Rückfall erhalten.
pub fn discover(local: Option<std::net::Ipv4Addr>, wait: Duration) -> Vec<String> {
    let mut targets = vec![SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT))];
    if let Some(ip) = local {
        let o = ip.octets();
        targets.push(SocketAddr::from(([o[0], o[1], o[2], 255], DISCOVERY_PORT)));
    }
    if let Ok(interfaces) = if_addrs::get_if_addrs() {
        for iface in interfaces {
            if iface.is_loopback() || !iface.is_oper_up() {
                continue;
            }
            if let if_addrs::IfAddr::V4(addr) = iface.addr
                && let Some(broadcast) = addr.broadcast
            {
                let target = SocketAddr::from((broadcast, DISCOVERY_PORT));
                if !targets.contains(&target) {
                    targets.push(target);
                }
            }
        }
    }
    discover_at(&targets, wait)
        .into_iter()
        .map(|(ip, port)| {
            if port == 80 {
                ip.to_string()
            } else {
                format!("{ip}:{port}")
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) mod mock {
    //! Schein-BMX: kleiner HTTP-Server, der die Remote API nach dem BMX-Quelltext nachstellt.
    use super::*;
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    pub struct State {
        /// Dateien je API-Pfad (`USER:/disks/…`, Groß-/Kleinschreibung wie hochgeladen).
        pub files: BTreeMap<String, Vec<u8>>,
        /// Ordner (API-Pfade).
        pub dirs: Vec<String>,
        /// Ausgeführte Aktionen mit Pfad.
        pub actions: Vec<(String, String)>,
        /// Eingelegt: Laufwerk 8, Modul.
        pub drive8: String,
        pub cart: String,
        /// Verlangtes Passwort (leer = keins).
        pub password: String,
        pub machine: String,
        /// Anzahl der Uploads.
        pub uploads: usize,
        /// Gibt es die Partition `USER`?
        pub user: bool,
        /// Platz je Laufwerk (Byte), davon geht ab, was in `files` liegt.
        pub free: u64,
        /// Kennt `screenshot.ppm` schon `stage=emulator`?
        pub stage: bool,
        /// `stage` je Bildschirmfoto-Abfrage (leer = ohne).
        pub shots: Vec<String>,
        /// Bildschirmfoto (PPM).
        pub ppm: Vec<u8>,
    }

    impl Default for State {
        fn default() -> State {
            State {
                files: BTreeMap::new(),
                dirs: Vec::new(),
                actions: Vec::new(),
                drive8: String::new(),
                cart: String::new(),
                password: String::new(),
                machine: "C64".into(),
                uploads: 0,
                user: true,
                free: 1 << 28,
                stage: false,
                shots: Vec::new(),
                ppm: Vec::new(),
            }
        }
    }

    pub struct Mock {
        pub addr: String,
        pub state: Arc<Mutex<State>>,
    }

    pub fn start() -> Mock {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap().to_string();
        let state = Arc::new(Mutex::new(State::default()));
        let st = state.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let st = st.clone();
                std::thread::spawn(move || {
                    let _ = serve(s, &st);
                });
            }
        });
        Mock { addr, state }
    }

    fn unpct(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%'
                && i + 3 <= b.len()
                && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
            {
                out.push(v);
                i += 3;
                continue;
            }
            assert_ne!(b[i], b'+', "BMX dekodiert „+“ nicht als Leerzeichen");
            out.push(b[i]);
            i += 1;
        }
        String::from_utf8(out).unwrap()
    }

    /// Wie `ParseDecodedMediaPath` in BMX: Laufwerk aus der Liste, Wurzel aus `MEDIA_ROOTS`.
    fn valid(p: &str, require_file: bool) -> bool {
        let Some((vol, rest)) = p.split_once(":/") else {
            return false;
        };
        let root = rest.split('/').next().unwrap_or("");
        ["SYS", "USER", "SD", "USB", "USB2", "USB3"].contains(&vol.to_ascii_uppercase().as_str())
            && !rest.is_empty()
            && is_media_root(root)
            && (!require_file || rest.contains('/'))
            && !rest.contains(['"', '*', '<', '>', '?', '\\', '|', ':'])
    }

    fn respond(s: &mut TcpStream, status: u16, ctype: &str, body: &[u8], head_len: Option<usize>) {
        let len = head_len.unwrap_or(body.len());
        let _ = write!(
            s,
            "HTTP/1.1 {status} X\r\nContent-Type: {ctype}\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n"
        );
        if head_len.is_none() {
            let _ = s.write_all(body);
        }
    }

    fn err(s: &mut TcpStream, status: u16, code: &str) {
        respond(
            s,
            status,
            "application/json",
            format!("{{\"error\":\"{code}\"}}\n").as_bytes(),
            None,
        );
    }

    fn fail(s: &mut TcpStream, status: u16, code: &str) -> std::io::Result<()> {
        err(s, status, code);
        Ok(())
    }

    fn serve(mut s: TcpStream, st: &Arc<Mutex<State>>) -> std::io::Result<()> {
        let mut r = BufReader::new(s.try_clone()?);
        let mut line = String::new();
        r.read_line(&mut line)?;
        let mut it = line.split_whitespace();
        let method = it.next().unwrap_or("").to_string();
        let target = it.next().unwrap_or("").to_string();
        let mut headers: Vec<(String, String)> = Vec::new();
        loop {
            let mut h = String::new();
            r.read_line(&mut h)?;
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
            }
        }
        let header = |k: &str| headers.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let len: usize = header("content-length")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; len];
        r.read_exact(&mut body)?;
        let (path, q) = target.split_once('?').unwrap_or((&target, ""));
        let qv = |name: &str| {
            q.split('&')
                .filter_map(|kv| kv.split_once('='))
                .find(|(k, _)| *k == name)
                .map(|(_, v)| unpct(v))
        };
        let mut g = st.lock().unwrap();
        if !g.password.is_empty() && header("x-password").as_deref() != Some(g.password.as_str()) {
            err(&mut s, 403, "forbidden");
            return Ok(());
        }
        let Some(route) = path.strip_prefix("/bmx/api/v1") else {
            respond(&mut s, 404, "text/plain", b"not found", None);
            return Ok(());
        };
        let in_use =
            |g: &State, p: &str| g.drive8.eq_ignore_ascii_case(p) || g.cart.eq_ignore_ascii_case(p);
        let find = |g: &State, p: &str| g.files.keys().find(|k| k.eq_ignore_ascii_case(p)).cloned();
        match (method.as_str(), route) {
            ("GET", "/state") => {
                let body = serde_json::json!({
                    "board": "Raspberry Pi 4 Model B", "machine": g.machine,
                    "release_version": "v2026.08.19", "uptime_ms": 1, "menu_visible": false
                })
                .to_string();
                respond(&mut s, 200, "application/json", body.as_bytes(), None);
            }
            ("GET", "/storage") => {
                let used: u64 = g.files.values().map(|v| v.len() as u64).sum();
                let free = g.free.saturating_sub(used);
                let body = serde_json::json!({"volumes": [
                    {"name": "SYS", "mounted": true, "total_bytes": 1 << 29, "free_bytes": free},
                    {"name": "USER", "mounted": g.user, "total_bytes": if g.user { 1 << 29 } else { 0 },
                     "free_bytes": if g.user { free } else { 0 }},
                    {"name": "SD", "mounted": true, "total_bytes": 1 << 29, "free_bytes": free},
                    {"name": "USB", "mounted": false, "total_bytes": 0, "free_bytes": 0}
                ]})
                .to_string();
                respond(&mut s, 200, "application/json", body.as_bytes(), None);
            }
            ("GET", "/media") => {
                let slot = |key: &str, kind: &str, p: &str| {
                    serde_json::json!({"key": key, "kind": kind,
                        "path": if p.is_empty() { Value::Null } else { Value::from(p) }})
                };
                let body = serde_json::json!({"slots": [
                    slot("drive.8", "disk", &g.drive8), slot("cartridge", "cartridge", &g.cart)
                ]})
                .to_string();
                respond(&mut s, 200, "application/json", body.as_bytes(), None);
            }
            ("GET", "/files") => {
                let Some(p) = qv("path").filter(|p| valid(p, false)) else {
                    return fail(&mut s, 400, "invalid_path");
                };
                let limit: usize = qv("limit").and_then(|l| l.parse().ok()).unwrap_or(16);
                assert!(limit <= 16, "BMX liefert höchstens 16 Einträge je Seite");
                let is_root = p.split_once(":/").is_some_and(|(_, r)| !r.contains('/'));
                if !is_root && !g.dirs.iter().any(|d| d.eq_ignore_ascii_case(&p)) {
                    return fail(&mut s, 404, "not_found");
                }
                let prefix = format!("{}/", p.to_ascii_lowercase());
                let mut entries: Vec<(String, bool, usize)> = Vec::new();
                for (k, v) in &g.files {
                    if let Some(rest) = k.to_ascii_lowercase().strip_prefix(&prefix)
                        && !rest.contains('/')
                    {
                        entries.push((k[prefix.len()..].to_string(), false, v.len()));
                    }
                }
                for d in &g.dirs {
                    if let Some(rest) = d.to_ascii_lowercase().strip_prefix(&prefix)
                        && !rest.contains('/')
                    {
                        entries.push((d[prefix.len()..].to_string(), true, 0));
                    }
                }
                entries.sort();
                let after = qv("after");
                let rest: Vec<_> = entries
                    .into_iter()
                    .filter(|(n, _, _)| after.as_ref().is_none_or(|a| n > a))
                    .collect();
                let page: Vec<_> = rest.iter().take(limit).collect();
                let next = (rest.len() > limit).then(|| page.last().unwrap().0.clone());
                let body = serde_json::json!({
                    "path": p,
                    "entries": page.iter().map(|(n, d, sz)| serde_json::json!({
                        "name": n, "type": if *d { "directory" } else { "file" },
                        "size": sz, "read_only": false })).collect::<Vec<_>>(),
                    "next_after": next,
                })
                .to_string();
                respond(&mut s, 200, "application/json", body.as_bytes(), None);
            }
            (m, "/file") => {
                let Some(p) = qv("path").filter(|p| valid(p, true)) else {
                    return fail(&mut s, 400, "invalid_path");
                };
                match m {
                    "GET" | "HEAD" => match find(&g, &p) {
                        Some(k) => {
                            let data = g.files[&k].clone();
                            let head = (m == "HEAD").then_some(data.len());
                            respond(&mut s, 200, "application/octet-stream", &data, head);
                        }
                        None => err(&mut s, 404, "not_found"),
                    },
                    "DELETE" => {
                        if in_use(&g, &p) {
                            return fail(&mut s, 409, "file_in_use");
                        }
                        if let Some(k) = find(&g, &p) {
                            g.files.remove(&k);
                        }
                        respond(&mut s, 204, "text/plain", b"", None);
                    }
                    "PUT" => {
                        let Some(hash) = header("x-bmx-sha256").filter(|h| h.len() == 64) else {
                            return fail(&mut s, 400, "sha256_required");
                        };
                        if in_use(&g, &p) {
                            return fail(&mut s, 409, "file_in_use");
                        }
                        if hash != sha256_hex(&body) {
                            return fail(&mut s, 422, "hash_mismatch");
                        }
                        let parent = p.rsplit_once('/').unwrap().0;
                        let parent_root = parent
                            .split_once(":/")
                            .is_some_and(|(_, r)| !r.contains('/'));
                        if !parent_root && !g.dirs.iter().any(|d| d.eq_ignore_ascii_case(parent)) {
                            return fail(&mut s, 404, "not_found");
                        }
                        if let Some(k) = find(&g, &p) {
                            g.files.remove(&k);
                        }
                        g.files.insert(p, body);
                        g.uploads += 1;
                        respond(&mut s, 204, "text/plain", b"", None);
                    }
                    _ => err(&mut s, 405, "method_not_allowed"),
                }
            }
            ("PUT", "/directory") => {
                let Some(p) = qv("path").filter(|p| valid(p, true)) else {
                    return fail(&mut s, 400, "invalid_path");
                };
                // Ganzer Pfad wird angelegt (CreateDirectoryTree).
                let (vol, rest) = p.split_once(":/").unwrap();
                let segs: Vec<&str> = rest.split('/').collect();
                for i in 2..=segs.len() {
                    let d = format!("{vol}:/{}", segs[..i].join("/"));
                    if !g.dirs.iter().any(|x| x.eq_ignore_ascii_case(&d)) {
                        g.dirs.push(d);
                    }
                }
                respond(&mut s, 204, "text/plain", b"", None);
            }
            ("POST", a) if a.starts_with("/actions/") => {
                let id = &a["/actions/".len()..];
                let needs_path = matches!(id, "media.autostart" | "media.drive.8.attach");
                let known =
                    needs_path || matches!(id, "media.cartridge.detach" | "media.drive.8.detach");
                if !known {
                    return fail(&mut s, 404, "not_found");
                }
                let p = if needs_path {
                    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    match v["path"].as_str().filter(|p| valid(p, true)) {
                        Some(p) if find(&g, p).is_some() => p.to_string(),
                        _ => return fail(&mut s, 422, "invalid_value"),
                    }
                } else {
                    assert!(body.is_empty(), "Aktion ohne Pfad darf keinen Inhalt haben");
                    String::new()
                };
                match id {
                    "media.autostart" if p.to_ascii_lowercase().ends_with(".crt") => {
                        g.cart = p.clone()
                    }
                    "media.autostart" if p.to_ascii_lowercase().ends_with(".d64") => {
                        g.drive8 = p.clone()
                    }
                    "media.drive.8.attach" => g.drive8 = p.clone(),
                    "media.cartridge.detach" => g.cart.clear(),
                    "media.drive.8.detach" => g.drive8.clear(),
                    _ => {}
                }
                g.actions.push((id.to_string(), p));
                respond(&mut s, 200, "application/json", br#"{"key":"x"}"#, None);
            }
            ("GET", "/screenshot.ppm") => {
                g.shots.push(qv("stage").unwrap_or_default());
                if qv("stage").is_some() && !g.stage {
                    return fail(&mut s, 400, "invalid_request");
                }
                let data = g.ppm.clone();
                respond(&mut s, 200, "image/x-portable-pixmap", &data, None);
            }
            _ => err(&mut s, 404, "not_found"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pfade() {
        assert_eq!(
            api_path("/USER/disks/Breadbin/x.d64").unwrap(),
            "USER:/disks/Breadbin/x.d64"
        );
        assert_eq!(api_path("/usb/disks/").unwrap(), "USB:/disks");
        assert!(api_path("/").is_err());
        assert_eq!(breadbin_path("USER:/disks/a.d64"), "/USER/disks/a.d64");
        assert_eq!(breadbin_path(""), "");
        assert_eq!(
            dir_for("/USER/disks/Breadbin", "CRT"),
            "/USER/carts/Breadbin"
        );
        assert_eq!(
            dir_for("/USER/disks/Breadbin/", "TAP"),
            "/USER/tapes/Breadbin"
        );
        assert_eq!(
            dir_for("/USER/disks/Breadbin", "D64"),
            "/USER/disks/Breadbin"
        );
        assert_eq!(dir_for("/SD/Breadbin", "CRT"), "/SD/Breadbin");
        assert_eq!(launch_dirs("/USER/disks/B").len(), 3);
        assert_eq!(launch_dirs("/SD/B"), ["/SD/B"]);
        let d = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            suggest_upload_dir(&d(&["SYS", "USER"])),
            "/USER/disks/Breadbin"
        );
        assert_eq!(suggest_upload_dir(&d(&["SYS"])), "/SYS/disks/Breadbin");
        assert_eq!(suggest_upload_dir(&d(&[])), "/SYS/disks/Breadbin");
        assert_eq!(
            pct("USER:/disks/A b+c.d64"),
            "USER%3A%2Fdisks%2FA%20b%2Bc.d64"
        );
    }

    #[test]
    fn fehlertexte() {
        let t = |st: u16, code: &str| {
            status_error(st, format!("{{\"error\":\"{code}\"}}").as_bytes()).to_string()
        };
        assert!(t(507, "insufficient_space").contains("Platz"));
        assert!(t(422, "hash_mismatch").contains("Prüfsumme"));
        assert!(t(404, "volume_unavailable").contains("USER"));
        assert!(t(409, "file_in_use").contains("eingelegt"));
        assert!(t(418, "teapot").contains("418: teapot"));
    }

    #[test]
    fn zustand_lesen() {
        let v: Value = serde_json::from_str(
            r#"{"board":"Raspberry Pi 5","machine":"C128","release_version":"v2026.08.19","uptime_ms":5}"#,
        )
        .unwrap();
        let st = State::from_json(&v).unwrap();
        assert_eq!(st.product(), "BMX (Raspberry Pi 5)");
        assert_eq!(hints(&st).len(), 3, "ungetestet, Maschine, Startordner");
        assert!(State::from_json(&serde_json::json!({"version":"3.15"})).is_none());
    }

    #[test]
    fn suchantwort_pruefen() {
        let n = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            parse_reply(format!("BMXDEV-HERE/1 {n} 80\n").as_bytes(), n),
            Some(80)
        );
        assert_eq!(
            parse_reply(format!("BMXDEV-HERE/1 {n} 80").as_bytes(), n),
            None
        );
        assert_eq!(parse_reply(b"BMXDEV-HERE/1 ffff 80\n", n), None);
        let a = nonce();
        assert_eq!(a.len(), 32);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        );
    }

    #[test]
    fn netzsuche_ueber_udp() {
        // Schein-BMX als UDP-Antworter auf einem freien Port.
        let responder = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = responder.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut buf = [0u8; 128];
            let (len, from) = responder.recv_from(&mut buf).unwrap();
            let text = std::str::from_utf8(&buf[..len]).unwrap();
            let nonce = text
                .strip_prefix("BMXDEV-DISCOVER/1 ")
                .and_then(|t| t.strip_suffix('\n'))
                .unwrap();
            assert_eq!(nonce.len(), 32);
            // Erst eine falsche Antwort (fremde Kennung), dann die richtige.
            responder
                .send_to(b"BMXDEV-HERE/1 00000000000000000000000000000000 80\n", from)
                .unwrap();
            responder
                .send_to(format!("BMXDEV-HERE/1 {nonce} 80\n").as_bytes(), from)
                .unwrap();
        });
        let found = discover_at(&[addr], Duration::from_millis(800));
        assert_eq!(found, [(addr.ip(), 80)]);
    }

    fn ppm(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut out = format!("P6\n# BMX\n{w} {h}\n255\n").into_bytes();
        for y in 0..h {
            for x in 0..w {
                out.extend_from_slice(&f(x, y));
            }
        }
        out
    }

    #[test]
    fn bildschirmfoto_wandeln() {
        // 480 × 272: 48 Punkte schwarze Balken links und rechts, blauer Rahmen, weißer Bildschirm.
        let data = ppm(480, 272, |x, y| {
            if !(48..432).contains(&x) {
                [0, 0, 0]
            } else if (48 + 32..48 + 352).contains(&x) && (35..235).contains(&y) {
                [255, 255, 255]
            } else {
                [0, 0, 170]
            }
        });
        let img = ppm_to_image(&data).unwrap();
        assert_eq!(img.dimensions(), (480, 272));
        assert_eq!(strip_bars(&img), (48, 0, 384, 272));
        let png = shot_to_png(img.clone(), true).unwrap();
        let out = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(out.dimensions(), (320, 200));
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255], "kein Rahmen mehr");
        assert_eq!(out.get_pixel(319, 199).0, [255, 255, 255]);
        let png = shot_to_png(img, false).unwrap();
        let out = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(out.dimensions(), (384, 272));
        assert_eq!(out.get_pixel(0, 0).0, [0, 0, 170], "Rahmen bleibt");
        assert!(ppm_to_image(b"P3\n1 1\n255\n0 0 0").is_err());
        assert!(ppm_to_image(b"P6\n2 2\n255\n\0\0\0").is_err(), "zu kurz");
    }

    #[test]
    fn mit_schein_bmx() {
        let m = mock::start();
        m.state.lock().unwrap().password = "geheim".into();
        let st = detect(&m.addr, "").unwrap_err();
        assert!(st.to_string().contains("Passwort"), "{st}");
        assert_eq!(quick_status(&m.addr, ""), Some(State::default()));
        let st = detect(&m.addr, "geheim").unwrap().unwrap();
        assert_eq!(st.version, "v2026.08.19");
        let c = Bmx::new(&m.addr, "geheim");
        assert_eq!(
            c.volumes().unwrap(),
            ["SYS", "USER"],
            "USB nicht eingebunden, SD nur anderer Name für SYS"
        );
        assert_eq!(c.list("/USER").unwrap().len(), 4);
        assert!(c.is_dir("/USER/disks").unwrap());
        assert!(!c.is_dir("/USER/disks/Breadbin").unwrap());
        c.mkdir("/USER/disks").unwrap();
        c.mkdir("/USER/disks/Breadbin").unwrap();
        assert!(c.is_dir("/USER/disks/Breadbin").unwrap());

        // Mehr als eine Seite (16 Einträge) und Leerzeichen im Namen.
        for i in 0..20 {
            c.upload(
                &format!("/USER/disks/Breadbin/Spiel {i:02}.d64"),
                &[i as u8; 3],
            )
            .unwrap();
        }
        let l = c.list("/USER/disks/Breadbin").unwrap();
        assert_eq!(l.len(), 20);
        assert_eq!(l[0].name, "Spiel 00.d64");
        assert_eq!(l[0].size, 3);
        assert_eq!(
            c.download("/USER/disks/Breadbin/Spiel 05.d64", 100)
                .unwrap(),
            [5; 3]
        );
        assert_eq!(
            c.size_of("/USER/disks/Breadbin/Spiel 05.d64").unwrap(),
            Some(3)
        );
        assert_eq!(c.size_of("/USER/disks/Breadbin/fehlt.d64").unwrap(), None);
        assert!(
            !c.upload_if_missing("/USER/disks/Breadbin/Spiel 05.d64", &[5; 3])
                .unwrap()
        );

        // Eingelegt → weder überschreiben noch löschen.
        c.attach_disk("/USER/disks/Breadbin/Spiel 05.d64").unwrap();
        assert_eq!(c.media().unwrap()[0].2, "/USER/disks/Breadbin/Spiel 05.d64");
        let e = c.delete("/USER/disks/Breadbin/Spiel 05.d64").unwrap_err();
        assert!(e.to_string().contains("eingelegt"), "{e}");
        c.delete("/USER/disks/Breadbin/Spiel 06.d64").unwrap();
        assert_eq!(c.list("/USER/disks/Breadbin").unwrap().len(), 19);

        let e = c.upload("/USER/Breadbin/x.d64", b"x").unwrap_err();
        assert!(e.to_string().contains("disks/"), "{e}");
        c.detach_cartridge().unwrap();
        let e = c.autostart("/USER/disks/Breadbin/fehlt.d64").unwrap_err();
        assert!(e.to_string().contains("422"), "{e}");

        // Bildschirmfoto: ältere Fassung ohne `stage` → zweiter Versuch ohne; neuere → ein Versuch.
        m.state.lock().unwrap().ppm = ppm(160, 100, |_, _| [9, 9, 9]);
        let size = |png: Vec<u8>| {
            image::load_from_memory(&png)
                .unwrap()
                .to_rgb8()
                .dimensions()
        };
        assert_eq!(size(c.screenshot_png(true).unwrap()), (320, 200));
        assert_eq!(m.state.lock().unwrap().shots, ["emulator", ""]);
        {
            let mut g = m.state.lock().unwrap();
            g.stage = true;
            g.shots.clear();
        }
        assert_eq!(size(c.screenshot_png(false).unwrap()).0, 384);
        assert_eq!(m.state.lock().unwrap().shots, ["emulator"]);

        // Kein Platz mehr → verständliche Meldung, nichts hochgeladen.
        m.state.lock().unwrap().free = 0;
        let e = c
            .upload_if_missing("/USER/disks/Breadbin/neu.d64", &[1; 2048])
            .unwrap_err();
        assert!(e.to_string().contains("nicht genug Platz"), "{e}");
        assert_eq!(c.size_of("/USER/disks/Breadbin/neu.d64").unwrap(), None);

        // Ohne USER-Partition: nur SYS.
        m.state.lock().unwrap().user = false;
        assert_eq!(c.volumes().unwrap(), ["SYS"]);
        assert_eq!(
            suggest_upload_dir(&c.volumes().unwrap()),
            "/SYS/disks/Breadbin"
        );
    }
}
