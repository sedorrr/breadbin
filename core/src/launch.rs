//! Spiele auf einem Ultimate-Gerät starten (Ultimate 64, C64 Ultimate, 1541 Ultimate II+).
//!
//! - PRG: `runners:run_prg`, CRT: `runners:run_crt` – jeweils mit Datei auf dem Gerät.
//! - Disk-Abbilder: in Laufwerk A einlegen, dann den Breadbin-Lader starten, der wie
//!   `LOAD"*",8,1` und `RUN` arbeitet (Quelltext: `core/c64/lader.asm`).
//! - P00 und T64 werden vorher in ein PRG umgewandelt.
//!
//! Liegt die Datei nicht schon auf dem Zielgerät, wird sie per FTP in den Startordner des
//! Geräts kopiert (immer derselbe Dateiname je Spiel, damit sich nichts ansammelt).
//! Jeder Start setzt den C64 zurück.

use crate::tr;
use anyhow::{Context, Result, anyhow, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{Cursor, Read};
use std::path::Path;
use std::time::Duration;

use crate::db;
use crate::ftp::{self, Ftp};

/// Der Lader als fertiges PRG (gebaut mit 64tass aus `core/c64/lader.asm`).
pub const LADER_PRG: &[u8] = include_bytes!("../c64/lader.prg");
const LADER_NAME: &str = "breadbin-lader.prg";
const MAX_FILE: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Device {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub product: String,
    /// Ordner auf dem Gerät für Dateien, die zum Starten hinkopiert werden.
    pub upload_dir: String,
    /// WLAN-MAC zum Einschalten per Wake-on-WiFi, leer wenn unbekannt.
    #[serde(default)]
    pub wifi_mac: String,
}

// ---------------------------------------------------------------- Geräteliste

pub fn list_devices(conn: &Connection) -> Result<Vec<Device>> {
    let mut st = conn.prepare(
        "SELECT id, name, host, product, upload_dir, wifi_mac FROM devices ORDER BY name COLLATE NOCASE",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Device {
            id: r.get(0)?,
            name: r.get(1)?,
            host: r.get(2)?,
            product: r.get(3)?,
            upload_dir: r.get(4)?,
            wifi_mac: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get_device(conn: &Connection, id: i64) -> Result<Device> {
    list_devices(conn)?
        .into_iter()
        .find(|d| d.id == id)
        .with_context(|| tr!("Gerät {id} gibt es nicht", "Device {id} does not exist"))
}

pub fn save_device(conn: &Connection, d: &Device) -> Result<i64> {
    let upload_dir = format!("/{}", d.upload_dir.trim().trim_matches('/'));
    if d.id > 0 {
        conn.execute(
            "UPDATE devices SET name = ?, host = ?, product = ?, upload_dir = ?,
                    wifi_mac = CASE WHEN ? = '' THEN wifi_mac ELSE ? END WHERE id = ?",
            params![
                d.name.trim(),
                d.host.trim(),
                d.product,
                upload_dir,
                d.wifi_mac,
                d.wifi_mac,
                d.id
            ],
        )?;
        return Ok(d.id);
    }
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM devices WHERE host = ?",
            [d.host.trim()],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO devices(name, host, product, upload_dir, wifi_mac) VALUES(?, ?, ?, ?, ?)",
        params![
            d.name.trim(),
            d.host.trim(),
            d.product,
            upload_dir,
            d.wifi_mac
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn remove_device(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM devices WHERE id = ?", [id])?;
    Ok(())
}

// ---------------------------------------------------------------- REST

pub struct Rest {
    agent: ureq::Agent,
    host: String,
    base: String,
    password: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct DeviceInfo {
    pub product: String,
    pub firmware: String,
    pub hostname: String,
    pub wifi_mac: String,
    /// Laufwerke laut FTP (z. B. `USB0`, `SD`), für den Vorschlag des Startordners.
    pub drives: Vec<String>,
}

impl Rest {
    pub fn new(host: &str, password: &str) -> Rest {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(20)))
            .timeout_connect(Some(Duration::from_secs(4)))
            .build()
            .into();
        Rest {
            agent,
            host: host.trim().to_string(),
            base: format!("http://{}/v1", host.trim()),
            password: password.to_string(),
        }
    }

    fn call(&self, method: &str, path: &str, query: &[(&str, &str)]) -> Result<Value> {
        let url = format!("{}/{}", self.base, path);
        let mut last_err = None;
        // Der C64 Ultimate antwortet gelegentlich nicht oder mit 404: bis zu 3 Versuche.
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs(attempt));
            }
            let result = if method == "PUT" {
                let mut req = self.agent.put(&url);
                for (k, v) in query {
                    req = req.query(*k, *v);
                }
                if !self.password.is_empty() {
                    req = req.header("X-Password", &self.password);
                }
                req.send_empty()
            } else {
                let mut req = self.agent.get(&url);
                for (k, v) in query {
                    req = req.query(*k, *v);
                }
                if !self.password.is_empty() {
                    req = req.header("X-Password", &self.password);
                }
                req.call()
            };
            let mut resp = match result {
                Ok(r) => r,
                // Ausgeschaltet oder nicht im Netz: Wiederholen bringt nichts.
                Err(ureq::Error::Io(e)) => {
                    return Err(ftp::unreachable_error(&self.host, &e, "REST"));
                }
                Err(ureq::Error::Timeout(_)) | Err(ureq::Error::HostNotFound) => {
                    return Err(anyhow!(tr!(
                        "Gerät {} ist nicht erreichbar – ist es eingeschaltet und im selben Netz?",
                        "Device {} is not reachable – is it switched on and on the same network?",
                        self.host
                    )));
                }
                Err(e) => {
                    last_err = Some(anyhow!(tr!(
                        "Gerät {} antwortet nicht: {e}",
                        "Device {} does not answer: {e}",
                        self.host
                    )));
                    continue;
                }
            };
            let status = resp.status().as_u16();
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            if status == 401 || status == 403 {
                bail!(tr!(
                    "Das Gerät verlangt ein Netzwerk-Passwort (oder es ist falsch).",
                    "The device requires a network password (or it is wrong)."
                ));
            }
            let json: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
            let errors: Vec<String> = json["errors"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if (200..300).contains(&status) && errors.is_empty() {
                return Ok(json);
            }
            let msg = if errors.is_empty() {
                tr!("Fehler {status}", "Error {status}")
            } else {
                errors.join("; ")
            };
            last_err = Some(anyhow!(tr!("Gerät meldet: {msg}", "Device reports: {msg}")));
            if status != 404 && status < 500 {
                break; // echter Fehler, Wiederholen hilft nicht
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow!(tr!("Gerät antwortet nicht", "Device does not answer"))))
    }

    pub fn info(&self) -> Result<DeviceInfo> {
        // Ältere Firmware kennt nur /v1/version; ist das Gerät aus, nicht zweimal warten.
        let v = match self.call("GET", "info", &[]) {
            Ok(v) => v,
            Err(e) if crate::i18n::is_unreachable(&e.to_string()) => return Err(e),
            Err(_) => self.call("GET", "version", &[])?,
        };
        Ok(DeviceInfo {
            product: v["product"].as_str().unwrap_or("Ultimate").to_string(),
            firmware: v["firmware_version"].as_str().unwrap_or("").to_string(),
            hostname: v["hostname"].as_str().unwrap_or("").to_string(),
            wifi_mac: v["wifi_mac"].as_str().unwrap_or("").to_string(),
            drives: Vec::new(),
        })
    }

    pub fn run_prg(&self, path: &str) -> Result<()> {
        self.call("PUT", "runners:run_prg", &[("file", path)])
            .map(|_| ())
    }

    pub fn run_crt(&self, path: &str) -> Result<()> {
        self.call("PUT", "runners:run_crt", &[("file", path)])
            .map(|_| ())
    }

    /// Videostrom (VIC) an `target` („ip:port“) schicken lassen.
    pub fn video_start(&self, target: &str) -> Result<()> {
        self.call("PUT", "streams/video:start", &[("ip", target)])
            .map(|_| ())
    }

    pub fn video_stop(&self) -> Result<()> {
        self.call("PUT", "streams/video:stop", &[]).map(|_| ())
    }

    /// Eingestellter KERNAL (Dateiname), leer wenn das Gerät die Einstellung nicht kennt.
    pub fn kernal(&self) -> String {
        self.call(
            "GET",
            "configs/C64%20and%20Cartridge%20Settings/Kernal%20ROM",
            &[],
        )
        .ok()
        .and_then(|v| {
            v["C64 and Cartridge Settings"]["Kernal ROM"]["current"]
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_default()
    }

    pub fn mount(&self, path: &str, kind: &str) -> Result<()> {
        self.call(
            "PUT",
            "drives/a:mount",
            &[("image", path), ("type", kind), ("mode", "readwrite")],
        )
        .map(|_| ())
    }
}

/// Hinweise zu einem Gerät aus Typ, Firmware und KERNAL: was damit nicht geht.
pub fn device_hints(product: &str, firmware: &str, kernal: &str) -> Vec<String> {
    let mut out = Vec::new();
    let nums: Vec<u32> = firmware
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|p| p.parse().ok())
        .collect();
    // Ultimate 64 und 1541 Ultimate II+ zählen ab 3.x; der C64 Ultimate hat eigene Nummern (1.x).
    if let [major, minor, ..] = nums[..]
        && major >= 3
        && (major, minor) < (3, 11)
    {
        out.push(tr!(
            "Firmware {firmware} ist älter als 3.11: Starten aus Breadbin braucht die REST-Schnittstelle (ab 3.11) – bitte die Firmware aktualisieren.",
            "Firmware {firmware} is older than 3.11: launching from Breadbin needs the REST interface (3.11 and later) – please update the firmware."
        ));
    }
    if product.contains("1541") || product.contains("II+") {
        out.push(tr!(
            "Bildschirmfotos gehen nur mit Ultimate 64 und C64 Ultimate.",
            "Screenshots only work with Ultimate 64 and C64 Ultimate."
        ));
    }
    if kernal.to_ascii_lowercase().contains("jiffy") {
        out.push(tr!(
            "KERNAL JiffyDOS: Bandabbilder (TAP) laden damit nicht – auch nicht über das Menü der Ultimate. Dafür in den Einstellungen des Geräts den Standard-KERNAL wählen.",
            "KERNAL JiffyDOS: tape images (TAP) do not load with it – not even from the Ultimate menu. Choose the standard KERNAL in the device settings for that."
        ));
    }
    out
}

/// Fragt ein Gerät ab: REST für Typ und Firmware, FTP für die Laufwerke.
pub fn probe(host: &str, password: &str) -> Result<DeviceInfo> {
    let mut info = Rest::new(host, password).info().map_err(|e| {
        if crate::i18n::is_unreachable(&e.to_string()) {
            e
        } else {
            e.context(tr!(
                "Keine Antwort der REST-Schnittstelle. Bei 1541 Ultimate II+ und Ultimate 64 ist dafür \
                 Firmware 3.11 oder neuer und im Menü „Web Remote Control Service“ nötig.",
                "No answer from the REST interface. On 1541 Ultimate II+ and Ultimate 64 this needs \
                 firmware 3.11 or later and “Web Remote Control Service” enabled in the menu."
            ))
        }
    })?;
    let mut ftp = Ftp::connect(host, password)?;
    info.drives = ftp
        .list("/")?
        .into_iter()
        .filter(|e| e.is_dir)
        .map(|e| e.name)
        .collect();
    ftp.quit();
    Ok(info)
}

/// Vorschlag für den Startordner: erster USB-Stick, sonst SD-Karte.
pub fn suggest_upload_dir(drives: &[String]) -> String {
    let pick = drives
        .iter()
        .find(|d| d.to_ascii_uppercase().starts_with("USB"))
        .or_else(|| drives.iter().find(|d| d.eq_ignore_ascii_case("SD")))
        .cloned()
        .unwrap_or_else(|| "USB0".into());
    format!("/{pick}/Breadbin")
}

// ---------------------------------------------------------------- Dateien umwandeln

/// P00 (PC64): 26 Byte Kopf, danach das PRG.
pub fn p00_to_prg(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 28 || &data[..8] != b"C64File\0" {
        bail!(tr!("P00-Datei beschädigt", "P00 file is damaged"));
    }
    Ok(data[26..].to_vec())
}

/// Erste Programmdatei aus einem T64-Bandabbild als PRG. Viele T64 tragen eine falsche
/// Endadresse ein; dann reicht die Datei bis zum nächsten Eintrag bzw. Dateiende.
pub fn t64_to_prg(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 0x60 || &data[..3] != b"C64" {
        bail!(tr!("T64-Datei beschädigt", "T64 file is damaged"));
    }
    let max = u16::from_le_bytes([data[0x22], data[0x23]]) as usize;
    let entries: Vec<(u16, u16, usize)> = (0..max.clamp(1, 512))
        .map(|i| 0x40 + i * 32)
        .take_while(|&o| o + 32 <= data.len())
        .filter(|&o| data[o] == 1)
        .map(|o| {
            let start = u16::from_le_bytes([data[o + 2], data[o + 3]]);
            let end = u16::from_le_bytes([data[o + 4], data[o + 5]]);
            let off =
                u32::from_le_bytes([data[o + 8], data[o + 9], data[o + 10], data[o + 11]]) as usize;
            (start, end, off)
        })
        .collect();
    let &(start, end, off) = entries.iter().min_by_key(|e| e.2).context(tr!(
        "T64 enthält keine Programmdatei",
        "T64 contains no program file"
    ))?;
    if off >= data.len() {
        bail!(tr!(
            "T64-Eintrag zeigt hinter das Dateiende",
            "T64 entry points past the end of the file"
        ));
    }
    let next = entries
        .iter()
        .map(|e| e.2)
        .filter(|&o| o > off)
        .min()
        .unwrap_or(data.len());
    let mut len = (end as usize).saturating_sub(start as usize);
    if len == 0 || off + len > next {
        len = next - off;
    }
    let mut prg = start.to_le_bytes().to_vec();
    prg.extend_from_slice(&data[off..off + len]);
    Ok(prg)
}

// ---------------------------------------------------------------- Start

/// Liest einen Fundort: lokale Datei, Datei auf einem Gerät, jeweils auch aus einem ZIP.
fn read_location(loc: &db::Location, password_for: &dyn Fn(&str) -> String) -> Result<Vec<u8>> {
    let raw = if loc.source_kind == "ultimate" {
        let (host, root) = ftp::split_location(&loc.source_location)
            .context(tr!("Geräteort ungültig", "Invalid device location"))?;
        let mut f = Ftp::connect(host, &password_for(host))?;
        let limit = if loc.member.is_empty() {
            MAX_FILE
        } else {
            512 * 1024 * 1024
        };
        let data = f.retrieve(
            &format!("{}/{}", root.trim_end_matches('/'), loc.path),
            limit,
        )?;
        f.quit();
        data
    } else {
        let path = Path::new(&loc.source_location).join(&loc.path);
        if loc.member.is_empty() {
            let mut data = Vec::new();
            std::fs::File::open(&path)?
                .take(MAX_FILE + 1)
                .read_to_end(&mut data)?;
            data
        } else {
            std::fs::read(&path)?
        }
    };
    if loc.member.is_empty() {
        return Ok(raw);
    }
    let mut zip =
        zip::ZipArchive::new(Cursor::new(raw)).context(tr!("ZIP unlesbar", "Unreadable ZIP"))?;
    let mut f = zip.by_name(&loc.member)?;
    let mut data = Vec::new();
    (&mut f).take(MAX_FILE + 1).read_to_end(&mut data)?;
    Ok(data)
}

/// Dateiname auf dem Gerät: nur unproblematische Zeichen, Endung passend zum Startweg.
fn device_file_name(title: &str, sha1: &str, ext: &str) -> String {
    let base: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .chars()
        .take(40)
        .collect();
    let base = if base.is_empty() {
        "spiel".to_string()
    } else {
        base
    };
    format!("{base}_{}.{ext}", &sha1[..8])
}

/// Hat Breadbin diese Datei im Startordner angelegt? (`Titel_1234abcd.d64` oder der Lader)
pub fn is_breadbin_file(name: &str) -> bool {
    if name == LADER_NAME {
        return true;
    }
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    let Some((_, hash)) = stem.rsplit_once('_') else {
        return false;
    };
    matches!(ext, "prg" | "crt" | "d64" | "d71" | "d81" | "g64")
        && hash.len() == 8
        && hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Löscht im Startordner des Geräts alle Dateien, die Breadbin dort abgelegt hat – außer Disketten,
/// die sich seitdem geändert haben (Spielstände). Andere Dateien bleiben liegen.
/// Gibt (gelöscht, übrig gelassen) zurück.
pub fn clean_upload_dir(dev: &Device, password: &str) -> Result<(usize, usize)> {
    let dir = format!("/{}", dev.upload_dir.trim().trim_matches('/'));
    let mut f = Ftp::connect(&dev.host, password)?;
    if !f.is_dir(&dir)? {
        f.quit();
        return Ok((0, 0));
    }
    // Geänderte Disketten (Spielstände) bleiben liegen, bis sie zurückgeholt sind.
    let changed: Vec<String> = changed_disks_in(&mut f, &dir)?
        .into_iter()
        .map(|c| c.name)
        .collect();
    let files: Vec<String> = f
        .list(&dir)?
        .into_iter()
        .filter(|e| !e.is_dir)
        .map(|e| e.name)
        .collect();
    let (ours, others): (Vec<_>, Vec<_>) = files
        .into_iter()
        .partition(|n| is_breadbin_file(n) && !changed.contains(n));
    for name in &ours {
        f.delete(&format!("{dir}/{name}"))?;
    }
    f.quit();
    Ok((ours.len(), others.len()))
}

/// Legt einen Ordner auf dem Gerät Stück für Stück an (MKD legt nur eine Ebene an). Die erste Ebene
/// ist das Laufwerk und muss es geben.
fn make_dirs(f: &mut Ftp, dir: &str) -> Result<()> {
    let mut path = String::new();
    for part in dir.split('/').filter(|p| !p.is_empty()) {
        path.push('/');
        path.push_str(part);
        if path.matches('/').count() > 1 {
            f.mkdir(&path)?;
        } else if !f.is_dir(&path)? {
            bail!(tr!(
                "Laufwerk {path} gibt es auf dem Gerät nicht.",
                "Drive {path} does not exist on the device."
            ));
        }
    }
    Ok(())
}

/// Dateiname zum Kopieren: der ursprüngliche Name (bzw. der im ZIP), ohne Zeichen, die auf dem
/// Gerät stören.
fn copy_name(loc: &db::Location) -> String {
    let raw = if loc.member.is_empty() {
        loc.path.as_str()
    } else {
        loc.member.as_str()
    };
    let base = raw.rsplit('/').next().unwrap_or(raw);
    let clean: String = base
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"') {
                '_'
            } else {
                c
            }
        })
        .collect();
    if clean.trim().is_empty() {
        "spiel".into()
    } else {
        clean
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CopyReport {
    /// Hinkopiert (Pfad auf dem Gerät).
    pub copied: Vec<String>,
    /// Lag schon auf dem Gerät (Titel: Pfad) – nicht noch einmal kopiert.
    pub present: Vec<String>,
    /// Nicht kopiert, mit Grund.
    pub failed: Vec<String>,
}

/// Kopiert Spiele in einen Ordner auf dem Gerät, etwa für Favoriten, die man ohne Rechner im Menü der
/// Ultimate finden will. Was schon irgendwo auf dem Gerät liegt (laut Scan) oder im Zielordner mit
/// gleichem Namen und gleicher Größe, wird nicht noch einmal kopiert.
pub fn copy_to_device(
    conn: &Connection,
    sha1s: &[String],
    dev: &Device,
    device_password: &str,
    password_for: &dyn Fn(&str) -> String,
    target_dir: &str,
) -> Result<CopyReport> {
    let dir = format!("/{}", target_dir.trim().trim_matches('/'));
    let mut f = Ftp::connect(&dev.host, device_password)?;
    make_dirs(&mut f, &dir)?;
    let mut existing: Vec<(String, u64)> = f
        .list(&dir)?
        .into_iter()
        .filter(|e| !e.is_dir)
        .map(|e| (e.name, e.size))
        .collect();
    let mut report = CopyReport::default();
    for sha1 in sha1s {
        let d = db::details(conn, sha1)?;
        let on_device = d.locations.iter().find(|l| {
            l.source_kind == "ultimate"
                && ftp::split_location(&l.source_location)
                    .is_some_and(|(host, _)| host.eq_ignore_ascii_case(dev.host.trim()))
        });
        if let Some(l) = on_device {
            let (_, root) = ftp::split_location(&l.source_location).unwrap_or_default();
            let member = if l.member.is_empty() {
                String::new()
            } else {
                format!(" → {}", l.member)
            };
            report.present.push(format!(
                "{}: {}/{}{member}",
                d.row.title,
                root.trim_end_matches('/'),
                l.path
            ));
            continue;
        }
        let Some(loc) = d
            .locations
            .iter()
            .find(|l| l.source_kind == "local")
            .or_else(|| d.locations.first())
        else {
            report.failed.push(format!(
                "{}: {}",
                d.row.title,
                tr!("kein Speicherort bekannt", "no storage location known")
            ));
            continue;
        };
        let mut name = copy_name(loc);
        if existing
            .iter()
            .any(|(n, size)| n.eq_ignore_ascii_case(&name) && *size == d.row.size as u64)
        {
            report
                .present
                .push(format!("{}: {dir}/{name}", d.row.title));
            continue;
        }
        if existing.iter().any(|(n, _)| n.eq_ignore_ascii_case(&name)) {
            // Gleicher Name, anderer Inhalt: Prüfsumme anhängen.
            name = match name.rsplit_once('.') {
                Some((stem, ext)) => format!("{stem}_{}.{ext}", &sha1[..8]),
                None => format!("{name}_{}", &sha1[..8]),
            };
        }
        match read_location(loc, password_for).and_then(|data| {
            let path = format!("{dir}/{name}");
            f.store(&path, &data)?;
            Ok((path, data.len() as u64))
        }) {
            Ok((path, size)) => {
                existing.push((name, size));
                report.copied.push(path);
            }
            Err(e) => report.failed.push(format!("{}: {e}", d.row.title)),
        }
    }
    f.quit();
    Ok(report)
}

/// Disk-Abbild im Startordner, das sich seit dem Hinkopieren geändert hat (Spielstand).
#[derive(Debug, Clone, Serialize)]
pub struct ChangedDisk {
    /// Name im Startordner.
    pub name: String,
    /// SHA-1 des Inhalts jetzt.
    pub sha1: String,
    /// Prüfsummen-Anfang des ursprünglich kopierten Inhalts (aus dem Dateinamen).
    pub original: String,
    #[serde(skip)]
    pub data: Vec<u8>,
}

const DISK_EXTS: [&str; 4] = ["d64", "d71", "d81", "g64"];

/// Lädt die Disk-Abbilder, die Breadbin in den Startordner kopiert hat, und gibt die geänderten zurück.
fn changed_disks_in(f: &mut Ftp, dir: &str) -> Result<Vec<ChangedDisk>> {
    use sha1::Digest;
    if !f.is_dir(dir)? {
        return Ok(Vec::new());
    }
    let names: Vec<String> = f
        .list(dir)?
        .into_iter()
        .filter(|e| !e.is_dir && is_breadbin_file(&e.name))
        .map(|e| e.name)
        .filter(|n| {
            n.rsplit_once('.')
                .is_some_and(|(_, ext)| DISK_EXTS.contains(&ext))
        })
        .collect();
    let mut out = Vec::new();
    for name in names {
        let Some(original) = name
            .rsplit_once('.')
            .and_then(|(stem, _)| stem.rsplit_once('_'))
            .map(|(_, h)| h.to_string())
        else {
            continue;
        };
        let data = f.retrieve(&format!("{dir}/{name}"), MAX_FILE)?;
        let sha1 = hex::encode(sha1::Sha1::digest(&data));
        if !sha1.starts_with(&original) {
            out.push(ChangedDisk {
                name,
                sha1,
                original,
                data,
            });
        }
    }
    Ok(out)
}

/// Geänderte Disketten (Spielstände) im Startordner des Geräts.
pub fn changed_disks(dev: &Device, password: &str) -> Result<Vec<ChangedDisk>> {
    let dir = format!("/{}", dev.upload_dir.trim().trim_matches('/'));
    let mut f = Ftp::connect(&dev.host, password)?;
    let out = changed_disks_in(&mut f, &dir);
    f.quit();
    out
}

/// Holt die geänderten Disketten in einen Ordner auf dem Rechner: „Titel (Spielstand 1a2b3c4d).d64“.
/// Gleicher Stand schon dort: übersprungen. `only`: nur Disketten dieser Inhalte (SHA-1), etwa
/// der Fassungen eines Spiels. Gibt die geschriebenen Dateien zurück.
pub fn fetch_saves(
    conn: &Connection,
    dev: &Device,
    password: &str,
    target: &Path,
    only: Option<&[String]>,
) -> Result<Vec<String>> {
    let mut disks = changed_disks(dev, password)?;
    if let Some(only) = only {
        disks.retain(|d| only.iter().any(|s| s.starts_with(&d.original)));
    }
    std::fs::create_dir_all(target)?;
    let mut written = Vec::new();
    for disk in disks {
        let (stem, ext) = disk
            .name
            .rsplit_once('.')
            .unwrap_or((disk.name.as_str(), "d64"));
        let fallback = stem
            .rsplit_once('_')
            .map_or(stem, |(t, _)| t)
            .replace('_', " ");
        let title: String = conn
            .query_row(
                "SELECT COALESCE(m.title, MIN(f.title)) FROM files f LEFT JOIN metadata m ON m.sha1 = f.sha1
                 WHERE f.sha1 LIKE ? || '%' GROUP BY f.sha1 LIMIT 1",
                [&disk.original],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(fallback);
        let safe: String = title
            .chars()
            .map(|c| {
                if c.is_control()
                    || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let file = target.join(format!(
            "{} ({} {}).{ext}",
            safe.trim(),
            tr!("Spielstand", "saved game"),
            &disk.sha1[..8]
        ));
        if file.exists() {
            continue;
        }
        std::fs::write(&file, &disk.data)?;
        written.push(file.display().to_string());
    }
    Ok(written)
}

#[derive(Debug, Clone, Serialize)]
pub struct LaunchResult {
    pub message: String,
    /// Pfad auf dem Gerät, der gestartet bzw. eingelegt wurde.
    pub device_path: String,
    pub copied: bool,
}

/// Startet einen Inhalt auf einem Gerät. Setzt den C64 zurück!
/// Mit `insert_only` wird ein Disk-Abbild nur in Laufwerk A eingelegt – ohne Reset, für den
/// Diskwechsel in einem laufenden Spiel.
pub fn launch(
    conn: &Connection,
    sha1: &str,
    dev: &Device,
    device_password: &str,
    password_for: &dyn Fn(&str) -> String,
    insert_only: bool,
) -> Result<LaunchResult> {
    let d = db::details(conn, sha1)?;
    let format = d.row.format.as_str();
    let (run_ext, disk_type) = match format {
        "PRG" | "P00" | "T64" => ("prg", None),
        "CRT" => ("crt", None),
        "D64" => ("d64", Some("d64")),
        "D71" => ("d71", Some("d71")),
        "D81" => ("d81", Some("d81")),
        "G64" => ("g64", Some("g64")),
        "TAP" => bail!(tr!(
            "Bandabbilder (TAP) lassen sich noch nicht starten.",
            "Tape images (TAP) cannot be launched yet."
        )),
        other => bail!(tr!(
            "Format {other} lässt sich nicht starten.",
            "Format {other} cannot be launched."
        )),
    };
    if insert_only && disk_type.is_none() {
        bail!(tr!(
            "Nur Disk-Abbilder lassen sich einlegen.",
            "Only disk images can be inserted."
        ));
    }

    // Liegt die Datei schon unverändert auf dem Zielgerät?
    let on_device = d.locations.iter().find_map(|l| {
        let (host, root) = ftp::split_location(&l.source_location)?;
        (l.source_kind == "ultimate"
            && host.eq_ignore_ascii_case(dev.host.trim())
            && l.member.is_empty()
            && matches!(format, "PRG" | "CRT" | "D64" | "D71" | "D81" | "G64"))
        .then(|| format!("{}/{}", root.trim_end_matches('/'), l.path))
    });

    let rest = Rest::new(&dev.host, device_password);
    let upload_dir = format!("/{}", dev.upload_dir.trim().trim_matches('/'));
    let mut ftp_conn: Option<Ftp> = None;
    let ftp = |ftp_conn: &mut Option<Ftp>| -> Result<()> {
        if ftp_conn.is_none() {
            let mut f = Ftp::connect(&dev.host, device_password)?;
            // Startordner Stück für Stück anlegen (MKD legt nur eine Ebene an).
            let mut path = String::new();
            for part in upload_dir.split('/').filter(|p| !p.is_empty()) {
                path.push('/');
                path.push_str(part);
                if path.matches('/').count() > 1 {
                    f.mkdir(&path)?;
                } else if !f.is_dir(&path)? {
                    bail!(tr!(
                        "Laufwerk {path} gibt es auf dem Gerät nicht – Startordner in den Geräte-Einstellungen prüfen.",
                        "Drive {path} does not exist on the device – check the launch folder in the device settings."
                    ));
                }
            }
            *ftp_conn = Some(f);
        }
        Ok(())
    };

    let (device_path, copied) = match on_device {
        Some(p) => (p, false),
        None => {
            let loc = d
                .locations
                .iter()
                .find(|l| l.source_kind == "local")
                .or_else(|| d.locations.first())
                .context(tr!(
                    "Für diesen Eintrag ist kein Speicherort bekannt",
                    "No storage location is known for this entry"
                ))?;
            let mut data = read_location(loc, password_for)?;
            data = match format {
                "P00" => p00_to_prg(&data)?,
                "T64" => t64_to_prg(&data)?,
                _ => data,
            };
            ftp(&mut ftp_conn)?;
            let path = format!(
                "{upload_dir}/{}",
                device_file_name(&d.row.title, sha1, run_ext)
            );
            ftp_conn.as_mut().unwrap().store(&path, &data)?;
            (path, true)
        }
    };

    let message = match disk_type {
        None if run_ext == "crt" => {
            rest.run_crt(&device_path)?;
            tr!(
                "Modul gestartet: {device_path}",
                "Cartridge started: {device_path}"
            )
        }
        None => {
            rest.run_prg(&device_path)?;
            tr!(
                "Programm gestartet: {device_path}",
                "Program started: {device_path}"
            )
        }
        Some(kind) if insert_only => {
            rest.mount(&device_path, kind)?;
            tr!(
                "Diskette in Laufwerk A eingelegt (ohne Neustart): {device_path}",
                "Disk inserted into drive A (no restart): {device_path}"
            )
        }
        Some(kind) => {
            ftp(&mut ftp_conn)?;
            let lader = format!("{upload_dir}/{LADER_NAME}");
            ftp_conn.as_mut().unwrap().store(&lader, LADER_PRG)?;
            rest.mount(&device_path, kind)?;
            rest.run_prg(&lader)?;
            tr!(
                "Diskette in Laufwerk A eingelegt und geladen: {device_path}",
                "Disk inserted into drive A and loaded: {device_path}"
            )
        }
    };
    if let Some(f) = ftp_conn {
        f.quit();
    }
    Ok(LaunchResult {
        message,
        device_path,
        copied,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::testdata;

    #[test]
    fn hinweise_zum_geraet() {
        assert!(device_hints("Ultimate 64 Elite", "3.15a", "kernal.bin").is_empty());
        assert_eq!(
            device_hints("Ultimate 64", "3.10", "").len(),
            1,
            "zu alt für REST"
        );
        assert!(
            device_hints("ULTIMATE 64", "1.1.0", "").is_empty(),
            "C64 Ultimate zählt eigene Nummern"
        );
        assert_eq!(
            device_hints("Ultimate 64-II", "3.15a", "JiffyDOS_C64.bin").len(),
            1
        );
        assert_eq!(device_hints("1541 Ultimate II+", "3.12", "").len(), 1);
    }

    #[test]
    fn kopiername_aus_dem_fundort() {
        let loc = |path: &str, member: &str| db::Location {
            source_id: 1,
            source_label: String::new(),
            source_kind: "local".into(),
            source_location: "/x".into(),
            path: path.into(),
            member: member.into(),
            quick: false,
        };
        assert_eq!(
            copy_name(&loc("Spiele/Giana Sisters.d64", "")),
            "Giana Sisters.d64"
        );
        assert_eq!(copy_name(&loc("pack.zip", "a/b:c.prg")), "b_c.prg");
    }

    #[test]
    fn lader_ist_eingebaut() {
        assert_eq!(&LADER_PRG[..2], &[0x01, 0x08], "Ladeadresse $0801");
        assert!(LADER_PRG.len() < 200);
    }

    #[test]
    fn t64_und_p00_umwandeln() {
        let mut img = testdata::t64("TAPE", &[("GAME", 0x0801, 0x0805)]);
        let off = img.len() as u32;
        img[0x40 + 8..0x40 + 12].copy_from_slice(&off.to_le_bytes());
        img.extend_from_slice(&[1, 2, 3, 4, 5, 6]);
        assert_eq!(t64_to_prg(&img).unwrap(), [0x01, 0x08, 1, 2, 3, 4]);

        // Falsche Endadresse ($C3C6 ist ein bekannter Fehler alter Werkzeuge): bis Dateiende.
        img[0x40 + 4..0x40 + 6].copy_from_slice(&0xC3C6u16.to_le_bytes());
        assert_eq!(t64_to_prg(&img).unwrap(), [0x01, 0x08, 1, 2, 3, 4, 5, 6]);

        let mut p00 = b"C64File\0GAME\0\0\0\0\0\0\0\0\0\0\0\0\0\0".to_vec();
        p00.extend_from_slice(&[0x01, 0x08, 0xAA]);
        assert_eq!(p00_to_prg(&p00).unwrap(), [0x01, 0x08, 0xAA]);
    }

    #[test]
    fn eigene_dateien_im_startordner_erkennen() {
        assert!(is_breadbin_file("Great_Giana_Sisters__The_89428785.d64"));
        assert!(is_breadbin_file("breadbin-lader.prg"));
        assert!(is_breadbin_file(&device_file_name(
            "Turrican",
            "0123abcd99",
            "crt"
        )));
        assert!(!is_breadbin_file("Turrican.crt"), "Datei des Nutzers");
        assert!(!is_breadbin_file("disk_1.d64"));
        assert!(
            !is_breadbin_file("save_0123ABCD.d64"),
            "Großbuchstaben erzeugt Breadbin nie"
        );
        assert!(!is_breadbin_file("notiz_0123abcd.txt"));
    }

    #[test]
    fn namen_und_startordner() {
        assert_eq!(
            device_file_name("Great Giana Sisters, The", "89428785ce16", "d64"),
            "Great_Giana_Sisters__The_89428785.d64"
        );
        assert_eq!(
            device_file_name("***", "0123456789", "prg"),
            "spiel_01234567.prg"
        );
        let drives = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            suggest_upload_dir(&drives(&["SD", "Flash", "Temp", "USB2"])),
            "/USB2/Breadbin"
        );
        assert_eq!(
            suggest_upload_dir(&drives(&["SD", "Flash"])),
            "/SD/Breadbin"
        );
    }
}
