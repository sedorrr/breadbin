//! Spiele auf einem Gerät starten: Ultimate (Ultimate 64, C64 Ultimate, 1541 Ultimate II+) oder
//! BMC64 auf dem Raspberry Pi (Upload und Autostart über dessen Weboberfläche, siehe `bmc64.rs`)
//! oder TeensyROM (Fernsteuer-Protokoll über TCP 2112, siehe `teensyrom.rs`, ungetestet)
//! oder BMX auf dem Raspberry Pi 4/5 (Remote API, siehe `bmx.rs`).
//!
//! Ultimate:
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

use crate::bmc64::{self, Bmc64};
use crate::bmx::{self, Bmx};
use crate::db;
use crate::ftp::{self, Ftp};
use crate::remote::{self, Remote};
use crate::teensyrom::{self, TeensyRom};

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
    /// Geräteart: `ultimate`, `bmc64`, `teensyrom` oder `bmx`.
    #[serde(default = "default_kind")]
    pub kind: String,
    /// Kennung des Ultimate (`unique_id` aus `/v1/info`) und MAC der zuletzt erreichten Adresse: Damit findet
    /// Breadbin ein Gerät wieder, wenn der Router ihm eine neue IP gibt. Werden beim Prüfen selbst gemerkt.
    #[serde(default)]
    pub unique_id: String,
    #[serde(default)]
    pub mac: String,
}

fn default_kind() -> String {
    remote::ULTIMATE.into()
}

impl Device {
    pub fn is_bmc64(&self) -> bool {
        self.kind == remote::BMC64
    }

    pub fn is_teensyrom(&self) -> bool {
        self.kind == remote::TEENSYROM
    }

    pub fn is_bmx(&self) -> bool {
        self.kind == remote::BMX
    }

    /// Ordner, in die Breadbin zum Starten Dateien legt: der Startordner, bei BMX dazu die
    /// Geschwister unter `carts/` und `tapes/`.
    pub fn launch_dirs(&self) -> Vec<String> {
        let dir = format!("/{}", self.upload_dir.trim().trim_matches('/'));
        if self.is_bmx() {
            bmx::launch_dirs(&dir)
        } else {
            vec![dir]
        }
    }
}

// ---------------------------------------------------------------- Geräteliste

pub fn list_devices(conn: &Connection) -> Result<Vec<Device>> {
    let mut st = conn.prepare(
        "SELECT id, name, host, product, upload_dir, wifi_mac, kind, unique_id, mac FROM devices ORDER BY name COLLATE NOCASE",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Device {
            id: r.get(0)?,
            name: r.get(1)?,
            host: r.get(2)?,
            product: r.get(3)?,
            upload_dir: r.get(4)?,
            wifi_mac: r.get(5)?,
            kind: r.get(6)?,
            unique_id: r.get(7)?,
            mac: r.get(8)?,
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
    let kind = remote::kind_of(&d.kind);
    if d.id > 0 {
        conn.execute(
            "UPDATE devices SET name = ?, host = ?, product = ?, upload_dir = ?, kind = ?,
                    wifi_mac = CASE WHEN ? = '' THEN wifi_mac ELSE ? END WHERE id = ?",
            params![
                d.name.trim(),
                d.host.trim(),
                d.product,
                upload_dir,
                kind,
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
        "INSERT INTO devices(name, host, product, upload_dir, wifi_mac, kind) VALUES(?, ?, ?, ?, ?, ?)",
        params![
            d.name.trim(),
            d.host.trim(),
            d.product,
            upload_dir,
            d.wifi_mac,
            kind
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Kennung und MAC eines gerade erreichten Geräts merken (leere Werte lassen das Gespeicherte stehen).
/// Gibt zurück, ob sich etwas geändert hat.
pub fn remember_identity(conn: &Connection, id: i64, unique_id: &str, mac: &str) -> Result<bool> {
    let n = conn.execute(
        "UPDATE devices SET unique_id = CASE WHEN ?1 <> '' THEN ?1 ELSE unique_id END,
                            mac = CASE WHEN ?2 <> '' THEN ?2 ELSE mac END
          WHERE id = ?3 AND ((?1 <> '' AND unique_id <> ?1) OR (?2 <> '' AND mac <> ?2))",
        params![unique_id.trim(), mac.trim(), id],
    )?;
    Ok(n > 0)
}

/// Ist das Gerät unter einer anderen Adresse im Netz? Abgleich über die Kennung des Ultimate und – auch wenn
/// es eine Kennung hat, falls sie sich geändert hat – über die gemerkten MACs (LAN wie WLAN). Nur Geräte
/// derselben Art; die eigene Adresse zählt nicht.
pub fn find_moved(dev: &Device, found: &[crate::power::Found]) -> Option<String> {
    let same_mac = |a: &str, b: &str| !a.is_empty() && a.eq_ignore_ascii_case(b);
    found
        .iter()
        .filter(|f| remote::kind_of(&f.kind) == remote::kind_of(&dev.kind))
        .filter(|f| f.host.trim() != dev.host.trim())
        .find(|f| {
            let id_hit = !dev.unique_id.is_empty() && f.unique_id == dev.unique_id;
            let mac_hit = [dev.mac.as_str(), dev.wifi_mac.as_str()].iter().any(|m| {
                [f.mac.as_str(), f.wifi_mac.as_str(), f.ethernet_mac.as_str()]
                    .iter()
                    .any(|x| same_mac(m, x))
            });
            id_hit || mac_hit
        })
        .map(|f| f.host.trim().to_string())
}

/// Gerät auf eine neue Adresse umziehen – samt der Ordner, die auf ihm als Quelle gescannt wurden
/// (die Dateipfade darin sind relativ und bleiben). Gibt die Zahl der umgezogenen Quellen zurück.
pub fn move_device(conn: &Connection, dev: &Device, new_host: &str) -> Result<usize> {
    let new_host = new_host.trim();
    let tx = conn.unchecked_transaction()?;
    if tx
        .query_row(
            "SELECT 1 FROM devices WHERE host = ? AND id <> ?",
            params![new_host, dev.id],
            |_| Ok(()),
        )
        .optional()?
        .is_some()
    {
        anyhow::bail!(tr!(
            "Unter {new_host} ist schon ein anderes Gerät eingetragen.",
            "Another device is already registered at {new_host}."
        ));
    }
    tx.execute(
        "UPDATE devices SET host = ? WHERE id = ?",
        params![new_host, dev.id],
    )?;
    let old = remote::location(&dev.kind, &dev.host, "/");
    let new = remote::location(&dev.kind, new_host, "/");
    let n = tx.execute(
        "UPDATE sources SET location = ?1 || substr(location, length(?2) + 1)
          WHERE substr(location, 1, length(?2)) = ?2",
        params![new, old],
    )?;
    tx.commit()?;
    Ok(n)
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
    /// Geräteart: `ultimate`, `bmc64`, `teensyrom` oder `bmx`.
    pub kind: String,
    /// Kennung des Ultimate (`unique_id`), sonst leer.
    pub unique_id: String,
    /// LAN-MAC, wie der Ultimate sie selbst meldet (Ersatz, wenn der ARP-Speicher nichts liefert).
    #[serde(skip)]
    pub ethernet_mac: String,
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
            kind: remote::ULTIMATE.into(),
            unique_id: v["unique_id"].as_str().unwrap_or("").to_string(),
            ethernet_mac: crate::power::normalize_mac(v["ethernet_mac"].as_str().unwrap_or(""))
                .unwrap_or_default(),
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

    /// Werte einer Einstellungs-Kategorie (z. B. „C64 and Cartridge Settings“).
    pub fn config(&self, category: &str) -> Result<Value> {
        let v = self.call("GET", &format!("configs/{}", urlenc(category)), &[])?;
        Ok(v[category].clone())
    }

    /// Eine Einstellung setzen – nur im RAM, nicht ins Flash (nach dem Ausschalten gilt der alte Wert).
    pub fn set_config(&self, category: &str, item: &str, value: &str) -> Result<()> {
        self.call(
            "PUT",
            &format!("configs/{}/{}", urlenc(category), urlenc(item)),
            &[("value", value)],
        )
        .map(|_| ())
    }

    /// Speicher des C64 lesen (DMA). Die Antwort ist binär, nicht JSON.
    pub fn readmem(&self, address: u16, length: u16) -> Result<Vec<u8>> {
        let url = format!("{}/machine:readmem", self.base);
        let mut req = self
            .agent
            .get(&url)
            .query("address", format!("{address:04X}"))
            .query("length", length.to_string());
        if !self.password.is_empty() {
            req = req.header("X-Password", &self.password);
        }
        let mut resp = req.call().map_err(|e| {
            anyhow!(tr!(
                "Gerät {} antwortet nicht: {e}",
                "Device {} does not answer: {e}",
                self.host
            ))
        })?;
        if resp.status().as_u16() != 200 {
            bail!(tr!(
                "Gerät meldet: Fehler {}",
                "Device reports: error {}",
                resp.status().as_u16()
            ));
        }
        Ok(resp.body_mut().read_to_vec()?)
    }

    /// Wirft die Diskette aus Laufwerk A aus.
    pub fn remove_drive_a(&self) -> Result<()> {
        self.call("PUT", "drives/a:remove", &[]).map(|_| ())
    }

    /// Startet ein Programm, das im Aufruf mitgeschickt wird (`POST runners:run_prg`).
    pub fn run_prg_data(&self, data: &[u8]) -> Result<()> {
        let mut req = self
            .agent
            .post(format!("{}/runners:run_prg", self.base))
            .header("Content-Type", "application/octet-stream");
        if !self.password.is_empty() {
            req = req.header("X-Password", &self.password);
        }
        let mut resp = match req.send(data) {
            Ok(r) => r,
            Err(ureq::Error::Io(e)) => return Err(ftp::unreachable_error(&self.host, &e, "REST")),
            Err(e) => bail!(tr!(
                "Gerät {} antwortet nicht: {e}",
                "Device {} does not answer: {e}",
                self.host
            )),
        };
        let status = resp.status().as_u16();
        if (200..300).contains(&status) {
            return Ok(());
        }
        let body = resp.body_mut().read_to_string().unwrap_or_default();
        bail!(tr!(
            "Gerät meldet: Fehler {status} {}",
            "Device reports: error {status} {}",
            body.trim().chars().take(200).collect::<String>()
        ))
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

/// Teil einer REST-Adresse kodieren (Leerzeichen und Sonderzeichen in Einstellungsnamen).
fn urlenc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Steck-Ultimate (1541 Ultimate II, II+, II+L) statt Ultimate 64 / C64 Ultimate? Die Firmware meldet
/// „Ultimate II“, „Ultimate II+“, „Ultimate II+L“ (Rechnername „Ultimate-IIp“ …), die Ultimate 64
/// dagegen „Ultimate 64“, „Ultimate 64 Elite“, „Ultimate 64-II“. Ohne Videostrom, Tasten und Ausschalten.
pub fn is_cartridge_ultimate(product: &str) -> bool {
    let p = product.to_ascii_lowercase().replace('-', " ");
    p.contains("1541") || (p.contains("ultimate ii") && !p.contains("64"))
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
    if is_cartridge_ultimate(product) {
        out.push(tr!(
            "1541 Ultimate II/II+: mit Breadbin noch ungetestet – Starten, Einlegen, Kopieren und Spielstände sollten gehen. Bildschirmfotos, automatisches Titelbild und Ausschalten gibt es nur mit Ultimate 64 und C64 Ultimate.",
            "1541 Ultimate II/II+: not yet tested with Breadbin – launching, inserting, copying and saved games should work. Screenshots, automatic title pictures and powering off are only available with Ultimate 64 and C64 Ultimate."
        ));
    }
    if kernal.to_ascii_lowercase().contains("jiffy") {
        out.push(tr!(
            "KERNAL JiffyDOS: Bandabbilder (TAP) laden damit nicht – auch nicht über das Menü des Ultimate. Dafür in den Einstellungen des Geräts den Standard-KERNAL wählen.",
            "KERNAL JiffyDOS: tape images (TAP) do not load with it – not even from the Ultimate menu. Choose the standard KERNAL in the device settings for that."
        ));
    }
    out
}

/// Fragt ein Gerät ab: erst, ob es ein TeensyROM (TCP 2112), BMX oder BMC64 ist; sonst beim Ultimate REST
/// für Typ und Firmware, FTP für die Laufwerke.
pub fn probe(host: &str, password: &str) -> Result<DeviceInfo> {
    // Zuerst TeensyROM: der hat keinen Webserver, die BMC64-Abfrage scheiterte sonst an ihm.
    if let Some(st) = teensyrom::detect(host) {
        return Ok(DeviceInfo {
            product: st.product.clone(),
            firmware: st.version.clone(),
            hostname: "teensyrom".into(),
            wifi_mac: String::new(),
            drives: TeensyRom::new(host).volumes().unwrap_or_default(),
            kind: remote::TEENSYROM.into(),
            unique_id: String::new(),
            ethernet_mac: String::new(),
        });
    }
    if let Some(st) = bmx::detect(host, password)? {
        return Ok(DeviceInfo {
            product: st.product(),
            firmware: st.version.clone(),
            hostname: "bmx".into(),
            wifi_mac: String::new(),
            drives: Bmx::new(host, password).volumes()?,
            kind: remote::BMX.into(),
            unique_id: String::new(),
            ethernet_mac: String::new(),
        });
    }
    if let Some(st) = bmc64::detect(host, password)? {
        return Ok(DeviceInfo {
            product: st.product(),
            firmware: st.version.clone(),
            hostname: "bmc64".into(),
            wifi_mac: String::new(),
            drives: Bmc64::new(host, password).volumes()?,
            kind: remote::BMC64.into(),
            unique_id: String::new(),
            ethernet_mac: String::new(),
        });
    }
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
pub(crate) fn read_location(
    loc: &db::Location,
    password_for: &dyn Fn(&str) -> String,
) -> Result<Vec<u8>> {
    let raw = if loc.source_kind == "ultimate" {
        let (mut f, _, root) = Remote::for_location(&loc.source_location, password_for)?;
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
pub(crate) fn device_file_name(title: &str, sha1: &str, ext: &str) -> String {
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
    matches!(
        ext,
        "prg" | "crt" | "d64" | "d71" | "d81" | "g64" | "t64" | "p00" | "tap"
    ) && hash.len() == 8
        && hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Löscht im Startordner des Geräts alle Dateien, die Breadbin dort abgelegt hat – außer Disketten,
/// die sich seitdem geändert haben (Spielstände). Andere Dateien bleiben liegen.
/// Gibt (gelöscht, übrig gelassen) zurück.
pub fn clean_upload_dir(dev: &Device, password: &str) -> Result<(usize, usize)> {
    let mut f = connect(dev, password)?;
    let mut total = (0, 0);
    for dir in dev.launch_dirs() {
        match clean_dir(&mut f, &dir) {
            Ok((d, k)) => {
                total.0 += d;
                total.1 += k;
            }
            Err(e) => {
                f.quit();
                return Err(e);
            }
        }
    }
    f.quit();
    Ok(total)
}

fn clean_dir(f: &mut Remote, dir: &str) -> Result<(usize, usize)> {
    if !f.is_dir(dir)? {
        return Ok((0, 0));
    }
    // Geänderte Disketten (Spielstände) bleiben liegen, bis sie zurückgeholt sind.
    let changed: Vec<String> = changed_disks_in(f, dir)?
        .into_iter()
        .map(|c| c.name)
        .collect();
    let files: Vec<String> = f
        .list(dir)?
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
    Ok((ours.len(), others.len()))
}

/// Verbindung zum Gerät (FTP beim Ultimate, HTTP bei BMC64 und BMX, TCP beim TeensyROM).
pub fn connect(dev: &Device, password: &str) -> Result<Remote> {
    Remote::connect(&dev.kind, &dev.host, password)
}

/// Legt einen Ordner auf dem Gerät Stück für Stück an (MKD legt nur eine Ebene an). Die erste Ebene
/// ist das Laufwerk und muss es geben.
fn make_dirs(f: &mut Remote, dir: &str) -> Result<()> {
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
    let mut f = connect(dev, device_password)?;
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
fn changed_disks_in(f: &mut Remote, dir: &str) -> Result<Vec<ChangedDisk>> {
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
    let mut f = connect(dev, password)?;
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
/// Leeres BASIC-Programm: `run_prg` setzt zurück, lädt nichts und landet bei READY.
const EMPTY_PRG: [u8; 4] = [0x01, 0x08, 0x00, 0x00];

/// „Spiel beenden“: setzt den C64 zurück, ohne etwas Neues zu starten.
/// - Ultimate: Laufwerk A auswerfen und ein leeres Programm starten (`machine:reset` würde ein per
///   `run_crt` gestartetes Modul gleich wieder starten).
/// - BMX: Modul abziehen, Hard Reset.
/// - BMC64: Hard Reset (ein Modul bleibt stecken).
/// - TeensyROM: zurück ins TeensyROM-Menü.
pub fn stop_game(dev: &Device, password: &str) -> Result<()> {
    if dev.is_bmx() {
        return Bmx::new(&dev.host, password).stop();
    }
    if dev.is_bmc64() {
        return Bmc64::new(&dev.host, password).reset();
    }
    if dev.is_teensyrom() {
        return TeensyRom::new(&dev.host).reset();
    }
    let rest = Rest::new(&dev.host, password);
    // Leeres Laufwerk meldet einen Fehler – das ist hier egal, nur Unerreichbarkeit zählt.
    if let Err(e) = rest.remove_drive_a()
        && crate::i18n::is_unreachable(&e.to_string())
    {
        return Err(e);
    }
    rest.run_prg_data(&EMPTY_PRG)
}

pub fn launch(
    conn: &Connection,
    sha1: &str,
    dev: &Device,
    device_password: &str,
    password_for: &dyn Fn(&str) -> String,
    insert_only: bool,
) -> Result<LaunchResult> {
    let d = db::details(conn, sha1)?;
    if dev.is_bmc64() {
        return launch_bmc64(&d, sha1, dev, device_password, password_for, insert_only);
    }
    if dev.is_teensyrom() {
        return launch_teensyrom(&d, sha1, dev, password_for, insert_only);
    }
    if dev.is_bmx() {
        return launch_bmx(&d, sha1, dev, device_password, password_for, insert_only);
    }
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

    // REU-Abbild neben der Startdatei: vor dem Start ins REU laden; sonst gemerkte Einstellungen
    // eines früheren REU-Starts auf diesem Gerät wiederherstellen.
    let mut reu_note = String::new();
    if !insert_only {
        match crate::reu::find(&d, true, password_for) {
            Some(comp) => {
                ftp(&mut ftp_conn)?;
                let f = ftp_conn.as_mut().unwrap();
                reu_note = prepare_reu(
                    conn,
                    &rest,
                    f,
                    dev,
                    &upload_dir,
                    &d.row.title,
                    sha1,
                    &comp,
                    password_for,
                )?;
            }
            None => restore_reu_settings(conn, &rest, &dev.host),
        }
    }

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
    let message = message + &reu_note;
    Ok(LaunchResult {
        message,
        device_path,
        copied,
    })
}

const REU_CATEGORY: &str = "C64 and Cartridge Settings";

/// REU-Abbild auf den Stick bringen, REU einschalten und per Lader füllen. Gibt einen Zusatz für die
/// Meldung zurück.
#[allow(clippy::too_many_arguments)]
fn prepare_reu(
    conn: &Connection,
    rest: &Rest,
    f: &mut Ftp,
    dev: &Device,
    upload_dir: &str,
    title: &str,
    sha1: &str,
    comp: &crate::reu::Companion,
    password_for: &dyn Fn(&str) -> String,
) -> Result<String> {
    use crate::reu::{self, Companion};
    let size = comp.size();
    // Liegt die Datei schon auf diesem Gerät, nimmt der Lader sie von dort.
    let on_device = match comp {
        Companion::Remote { location, path, .. } => ftp::split_location(location)
            .filter(|(h, _)| h.eq_ignore_ascii_case(dev.host.trim()))
            .map(|_| path.clone()),
        Companion::Local { .. } => None,
    };
    let reu_path = match on_device {
        Some(p) => p,
        None => {
            let name = device_file_name(title, sha1, "reu");
            let path = format!("{upload_dir}/{name}");
            let present = f
                .list(upload_dir)
                .unwrap_or_default()
                .iter()
                .any(|e| e.name == name && e.size == size);
            if !present {
                let data = comp.read(password_for)?;
                f.store(&path, &data)?;
            }
            path
        }
    };

    // Einstellungen merken (nur beim ersten REU-Start; sonst bleiben die ursprünglichen gemerkt).
    let cfg = rest.config(REU_CATEGORY)?;
    let key = reu::restore_key(&dev.host);
    if db::get_setting(conn, &key)?.is_none() {
        let mut old = serde_json::Map::new();
        for item in reu::ULTIMATE_ITEMS {
            if let Some(v) = cfg[item].as_str() {
                old.insert(item.to_string(), Value::String(v.to_string()));
            }
        }
        db::set_setting(conn, &key, &Value::Object(old).to_string())?;
    }
    let kb = reu::size_kb(size);
    let want = [
        ("RAM Expansion Unit", "Enabled".to_string()),
        ("REU Size", reu::ultimate_size_value(kb)),
        ("Command Interface", "Enabled".to_string()),
    ];
    for (item, value) in &want {
        if cfg[*item].as_str() != Some(value.as_str()) {
            rest.set_config(REU_CATEGORY, item, value)?;
        }
    }

    // Lader hochladen, starten und auf die Marke warten.
    let lader = format!("{upload_dir}/{}", reu::REU_LADER_NAME);
    f.store(&lader, &reu::loader(&reu_path, size)?)?;
    rest.run_prg(&lader)?;
    let start = std::time::Instant::now();
    let mark = loop {
        std::thread::sleep(Duration::from_millis(1000));
        let m = rest
            .readmem(0x02A7, 1)
            .ok()
            .and_then(|v| v.first().copied())
            .unwrap_or(0);
        if matches!(m, 0xA5 | 0xE0 | 0xE1 | 0xE2) {
            break m;
        }
        if start.elapsed() > Duration::from_secs(150) {
            break 0;
        }
    };
    match mark {
        0xA5 => Ok(tr!(
            " REU-Abbild geladen ({kb} KB).",
            " REU image loaded ({kb} KB)."
        )),
        0xE0 => bail!(tr!(
            "REU-Abbild nicht geladen: Das Command Interface des Ultimate ist aus.",
            "REU image not loaded: the Ultimate's Command Interface is off."
        )),
        0xE1 => bail!(tr!(
            "REU-Abbild nicht geladen: Der Ultimate findet {reu_path} nicht.",
            "REU image not loaded: the Ultimate cannot find {reu_path}."
        )),
        0xE2 => bail!(tr!(
            "REU-Abbild nicht geladen: Laden ins REU gescheitert.",
            "REU image not loaded: loading into the REU failed."
        )),
        _ => bail!(tr!(
            "REU-Abbild nicht geladen: keine Rückmeldung vom Lader.",
            "REU image not loaded: no answer from the loader."
        )),
    }
}

/// Einstellungen eines früheren REU-Starts auf diesem Gerät zurückstellen (Fehler werden übergangen –
/// spätestens nach dem Aus- und Einschalten gelten ohnehin die gespeicherten Werte).
fn restore_reu_settings(conn: &Connection, rest: &Rest, host: &str) {
    let key = crate::reu::restore_key(host);
    let Ok(Some(saved)) = db::get_setting(conn, &key) else {
        return;
    };
    let old: Value = serde_json::from_str(&saved).unwrap_or(Value::Null);
    let cfg = rest.config(REU_CATEGORY).unwrap_or(Value::Null);
    let mut ok = true;
    // Reihenfolge wie beim Einschalten umgekehrt: erst Größe, dann REU und Command Interface.
    for item in ["REU Size", "RAM Expansion Unit", "Command Interface"] {
        if let Some(v) = old[item].as_str()
            && cfg[item].as_str() != Some(v)
        {
            ok &= rest.set_config(REU_CATEGORY, item, v).is_ok();
        }
    }
    if ok {
        let _ = conn.execute("DELETE FROM settings WHERE key = ?", [&key]);
    }
}

/// Start auf BMC64: Datei in den Startordner hochladen (falls sie nicht schon auf dem Gerät liegt)
/// und per Autostart starten. BMC64 startet PRG, P00, T64, TAP, CRT und Disk-Abbilder selbst.
fn launch_bmc64(
    d: &db::Details,
    sha1: &str,
    dev: &Device,
    password: &str,
    password_for: &dyn Fn(&str) -> String,
    insert_only: bool,
) -> Result<LaunchResult> {
    if insert_only {
        bail!(tr!(
            "BMC64 kann keine Diskette ohne Neustart wechseln – bitte „Starten“ nehmen oder im BMC64-Menü wechseln.",
            "BMC64 cannot swap disks without a restart – please use “Launch” or swap in the BMC64 menu."
        ));
    }
    let format = d.row.format.as_str();
    let ext = match format {
        "PRG" | "P00" | "T64" | "TAP" | "CRT" | "D64" | "D71" | "D81" | "G64" => {
            format.to_ascii_lowercase()
        }
        other => bail!(tr!(
            "Format {other} lässt sich nicht starten.",
            "Format {other} cannot be launched."
        )),
    };
    let client = Bmc64::new(&dev.host, password);
    let st = client.status()?;
    if !st.run.is_empty() && !st.run.contains(&ext) {
        bail!(tr!(
            "BMC64 läuft gerade als {} und kann {format} nicht starten.",
            "BMC64 is currently running as {} and cannot launch {format}.",
            st.machine
        ));
    }

    // Liegt die Datei schon unverändert auf diesem Gerät?
    let on_device = d.locations.iter().find_map(|l| {
        let (host, root) = ftp::split_location(&l.source_location)?;
        (l.source_kind == "ultimate"
            && remote::location_kind(&l.source_location) == remote::BMC64
            && host.eq_ignore_ascii_case(dev.host.trim())
            && l.member.is_empty())
        .then(|| format!("{}/{}", root.trim_end_matches('/'), l.path))
    });
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
            let data = read_location(loc, password_for)?;
            let upload_dir = format!("/{}", dev.upload_dir.trim().trim_matches('/'));
            let mut r = Remote::Bmc64(Bmc64::new(&dev.host, password));
            make_dirs(&mut r, &upload_dir).map_err(|e| {
                e.context(tr!(
                    "Startordner in den Geräte-Einstellungen prüfen",
                    "Check the launch folder in the device settings"
                ))
            })?;
            let path = format!(
                "{upload_dir}/{}",
                device_file_name(&d.row.title, sha1, &ext)
            );
            client.upload(&path, &data)?;
            (path, true)
        }
    };
    client.autostart(&device_path)?;
    let message = if format == "CRT" {
        tr!(
            "Modul eingelegt und gestartet: {device_path}",
            "Cartridge inserted and started: {device_path}"
        )
    } else {
        tr!("Gestartet: {device_path}", "Started: {device_path}")
    };
    Ok(LaunchResult {
        message,
        device_path,
        copied,
    })
}

/// Start auf BMX: Datei hochladen (Disketten, PRG, T64 nach `…/disks/…`, Module nach
/// `…/carts/…`, Bänder nach `…/tapes/…`) und per Autostart starten. VICE startet PRG, P00, T64, TAP,
/// CRT und alle Disk-Abbilder selbst. `insert_only` legt ein Disk-Abbild in Laufwerk 8 ein, ohne
/// Neustart. Vor dem Start von etwas anderem als einem Modul zieht Breadbin ein steckendes Modul ab.
fn launch_bmx(
    d: &db::Details,
    sha1: &str,
    dev: &Device,
    password: &str,
    password_for: &dyn Fn(&str) -> String,
    insert_only: bool,
) -> Result<LaunchResult> {
    let format = d.row.format.as_str();
    let ext = match format {
        "PRG" | "P00" | "T64" | "TAP" | "CRT" | "D64" | "D71" | "D81" | "G64" => {
            format.to_ascii_lowercase()
        }
        other => bail!(tr!(
            "Format {other} lässt sich nicht starten.",
            "Format {other} cannot be launched."
        )),
    };
    let is_disk = matches!(format, "D64" | "D71" | "D81" | "G64");
    if insert_only && !is_disk {
        bail!(tr!(
            "Nur Disk-Abbilder lassen sich einlegen.",
            "Only disk images can be inserted."
        ));
    }
    let client = Bmx::new(&dev.host, password);

    // Liegt die Datei schon unverändert auf diesem Gerät?
    let on_device = d.locations.iter().find_map(|l| {
        let (host, root) = ftp::split_location(&l.source_location)?;
        (l.source_kind == "ultimate"
            && remote::location_kind(&l.source_location) == remote::BMX
            && host.eq_ignore_ascii_case(dev.host.trim())
            && l.member.is_empty())
        .then(|| format!("{}/{}", root.trim_end_matches('/'), l.path))
    });
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
            let data = read_location(loc, password_for)?;
            let dir = bmx::dir_for(&dev.upload_dir, format);
            let mut r = Remote::Bmx(Bmx::new(&dev.host, password));
            make_dirs(&mut r, &dir).map_err(|e| {
                e.context(tr!(
                    "Startordner in den Geräte-Einstellungen prüfen",
                    "Check the launch folder in the device settings"
                ))
            })?;
            let path = format!("{dir}/{}", device_file_name(&d.row.title, sha1, &ext));
            // Gleicher Name = gleicher Inhalt (Prüfsumme im Namen); eine eingelegte Diskette ließe
            // sich ohnehin nicht überschreiben.
            let copied = client.upload_if_missing(&path, &data)?;
            (path, copied)
        }
    };
    if insert_only {
        client.attach_disk(&device_path)?;
        return Ok(LaunchResult {
            message: tr!(
                "Diskette in Laufwerk 8 eingelegt (ohne Neustart): {device_path}",
                "Disk inserted into drive 8 (no restart): {device_path}"
            ),
            device_path,
            copied,
        });
    }
    if format != "CRT" {
        // Ein steckendes Modul würde sonst statt des Spiels starten; ohne Modul ist das egal.
        let _ = client.detach_cartridge();
    }
    client.autostart(&device_path)?;
    let message = if format == "CRT" {
        tr!(
            "Modul eingelegt und gestartet: {device_path}",
            "Cartridge inserted and started: {device_path}"
        )
    } else {
        tr!("Gestartet: {device_path}", "Started: {device_path}")
    };
    Ok(LaunchResult {
        message,
        device_path,
        copied,
    })
}

/// Start auf dem TeensyROM (ungetestet): Datei in den Startordner hochladen und starten.
///
/// Der TeensyROM lädt nur einzelne Dateien (PRG/P00, CRT). T64 wird zum PRG (erstes Programm), ein
/// D64/D71/D81 nur, wenn es genau eine Programmdatei enthält – die geht dann als PRG hinüber. Läuft
/// gerade ein Programm, lehnt der TeensyROM das Hochladen ab („Busy!“): dann zurücksetzen, aufs Menü
/// warten und noch einmal.
fn launch_teensyrom(
    d: &db::Details,
    sha1: &str,
    dev: &Device,
    password_for: &dyn Fn(&str) -> String,
    insert_only: bool,
) -> Result<LaunchResult> {
    if insert_only {
        bail!(tr!(
            "TeensyROM kann keine Diskette einlegen – er lädt nur einzelne Dateien.",
            "TeensyROM cannot insert disks – it only loads single files."
        ));
    }
    let format = d.row.format.as_str();
    let run_ext = match format {
        "PRG" | "P00" | "T64" | "D64" | "D71" | "D81" => "prg",
        "CRT" => "crt",
        "TAP" | "G64" => bail!(tr!(
            "TeensyROM kann {format} nicht starten (nur PRG, P00, T64, CRT und Disketten mit einer Programmdatei).",
            "TeensyROM cannot launch {format} (only PRG, P00, T64, CRT and disks with a single program file)."
        )),
        other => bail!(tr!(
            "Format {other} lässt sich nicht starten.",
            "Format {other} cannot be launched."
        )),
    };
    let client = TeensyRom::new(&dev.host);
    let upload_dir = format!("/{}", dev.upload_dir.trim().trim_matches('/'));

    // Liegt die Datei schon unverändert auf diesem Gerät (PRG, P00, CRT startet er von dort)?
    let on_device = d.locations.iter().find_map(|l| {
        let (host, root) = ftp::split_location(&l.source_location)?;
        (l.source_kind == "ultimate"
            && remote::location_kind(&l.source_location) == remote::TEENSYROM
            && host.eq_ignore_ascii_case(dev.host.trim())
            && l.member.is_empty()
            && matches!(format, "PRG" | "P00" | "CRT"))
        .then(|| format!("{}/{}", root.trim_end_matches('/'), l.path))
    });
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
            let data = teensyrom_payload(format, read_location(loc, password_for)?)?;
            if format == "CRT"
                && data.len() > teensyrom::MAX_CRT_FROM_USB
                && teensyrom::is_usb_path(&upload_dir)
            {
                bail!(tr!(
                    "Module über 850 KB startet der TeensyROM nur von der SD-Karte – Startordner auf /SD/… stellen.",
                    "TeensyROM only launches cartridges over 850 KB from the SD card – set the launch folder to /SD/…."
                ));
            }
            let path = format!(
                "{upload_dir}/{}",
                device_file_name(&d.row.title, sha1, run_ext)
            );
            // Gleicher Name = gleicher Inhalt (Prüfsumme im Namen): vorhandene Datei einfach nehmen.
            match client.upload(&path, &data, false) {
                Err(e) if teensyrom::is_busy(&e) => {
                    client.reset_and_wait()?;
                    client.upload(&path, &data, false)?;
                }
                r => {
                    r.map_err(|e| {
                        e.context(tr!(
                            "Hochladen auf den TeensyROM fehlgeschlagen – Startordner in den Geräte-Einstellungen prüfen",
                            "Upload to the TeensyROM failed – check the launch folder in the device settings"
                        ))
                    })?;
                }
            }
            (path, true)
        }
    };
    client.launch(&device_path)?;
    let message = if run_ext == "crt" {
        tr!(
            "Modul gestartet: {device_path}",
            "Cartridge started: {device_path}"
        )
    } else if matches!(format, "D64" | "D71" | "D81") {
        tr!(
            "Programm der Diskette gestartet: {device_path}",
            "Program from the disk started: {device_path}"
        )
    } else {
        tr!(
            "Programm gestartet: {device_path}",
            "Program started: {device_path}"
        )
    };
    Ok(LaunchResult {
        message,
        device_path,
        copied,
    })
}

/// Was zum TeensyROM hinübergeht: PRG aus P00/T64/Diskette, CRT unverändert.
pub(crate) fn teensyrom_payload(format: &str, data: Vec<u8>) -> Result<Vec<u8>> {
    use crate::formats::{DiskProgram, disk_program};
    Ok(match format {
        "P00" => p00_to_prg(&data)?,
        "T64" => t64_to_prg(&data)?,
        "D64" | "D71" | "D81" => match disk_program(format, &data) {
            DiskProgram::One(prg) => prg,
            DiskProgram::Several(n) => bail!(tr!(
                "Die Diskette hat {n} Dateien – der TeensyROM lädt nur einzelne Dateien, mehrteilige Disk-Spiele gehen nicht. Eine Modul- oder PRG-Fassung nehmen.",
                "The disk has {n} files – TeensyROM only loads single files, multi-file disk games do not work. Use a cartridge or PRG version.",
            )),
            DiskProgram::NoProgram => bail!(tr!(
                "Die Diskette enthält keine startbare Programmdatei.",
                "The disk contains no launchable program file."
            )),
            DiskProgram::Damaged => {
                bail!(tr!("{format}-Datei beschädigt", "{format} file is damaged"))
            }
        },
        _ => data,
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
        assert_eq!(device_hints("Ultimate II", "3.15a", "").len(), 1);
    }

    #[test]
    fn steck_ultimate_erkennen() {
        for p in [
            "Ultimate II",
            "Ultimate II+",
            "Ultimate II+L",
            "1541 Ultimate-II+",
            "Ultimate-IIp",
        ] {
            assert!(is_cartridge_ultimate(p), "{p}");
        }
        for p in [
            "Ultimate 64",
            "Ultimate 64 Elite",
            "Ultimate 64-II",
            "C64 Ultimate",
            "Ultimate",
            "",
        ] {
            assert!(!is_cartridge_ultimate(p), "{p}");
        }
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
        assert!(
            is_breadbin_file("Pitfall_0123abcd.tap"),
            "BMC64 startet TAP selbst"
        );
    }

    #[test]
    fn auf_teensyrom_starten() {
        use crate::formats::testdata;
        use crate::teensyrom::mock;
        let dir = tempfile::tempdir().unwrap();
        let w = |name: &str, data: &[u8]| std::fs::write(dir.path().join(name), data).unwrap();
        w("Hopper.prg", &[0x01, 0x08, 0x60]);
        // Diskette mit genau einer Programmdatei (17/0, 3 Byte).
        let mut one = testdata::d64(b"ONE", &[(b"ONE", 0x82, 1)]);
        let o = 16 * 21 * 256; // Spur 17, Sektor 0
        one[o + 1] = 4;
        one[o + 2..o + 5].copy_from_slice(&[0x01, 0x08, 0xEA]);
        w("One.d64", &one);
        w(
            "Multi.d64",
            &testdata::d64(b"M", &[(b"LOADER", 0x82, 1), (b"LEVEL1", 0x82, 9)]),
        );
        w(
            "Band.tap",
            b"C64-TAPE-RAW\x01\0\0\0\x04\0\0\0\x30\x30\x30\x30",
        );
        let conn = db::open_in_memory().unwrap();
        let id = db::add_local_source(&conn, dir.path()).unwrap();
        crate::scan::scan_local(
            &conn,
            id,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        let sha = |name: &str| -> String {
            conn.query_row("SELECT sha1 FROM files WHERE path = ?", [name], |r| {
                r.get(0)
            })
            .unwrap()
        };

        let m = mock::start();
        m.state.lock().unwrap().busy = true; // es läuft gerade ein Spiel
        let dev = Device {
            id: 1,
            name: "TR".into(),
            host: m.addr.clone(),
            product: "TeensyROM+".into(),
            upload_dir: "/SD/Breadbin".into(),
            wifi_mac: String::new(),
            kind: remote::TEENSYROM.into(),
            unique_id: String::new(),
            mac: String::new(),
        };
        let none = |_: &str| String::new();
        let r = launch(&conn, &sha("Hopper.prg"), &dev, "", &none, false).unwrap();
        assert!(r.copied);
        assert!(
            r.device_path.starts_with("/SD/Breadbin/Hopper_"),
            "{}",
            r.device_path
        );
        {
            let g = m.state.lock().unwrap();
            assert_eq!(g.resets, 1, "beschäftigt → Reset, dann hochladen");
            assert_eq!(g.launched.last().unwrap().0, 1, "Laufwerk SD");
            assert!(
                g.launched
                    .last()
                    .unwrap()
                    .1
                    .starts_with("/Breadbin/Hopper_")
            );
        }
        // Zweiter Start: Datei liegt schon dort (gleicher Name) – kein zweites Hochladen nötig.
        m.state.lock().unwrap().busy = false;
        launch(&conn, &sha("Hopper.prg"), &dev, "", &none, false).unwrap();
        assert_eq!(m.state.lock().unwrap().files.len(), 1);

        let r = launch(&conn, &sha("One.d64"), &dev, "", &none, false).unwrap();
        assert!(r.device_path.ends_with(".prg"), "{}", r.device_path);
        let g = m.state.lock().unwrap();
        let sent = g
            .files
            .iter()
            .find(|((_, p), _)| p.contains("One_"))
            .unwrap()
            .1
            .clone();
        drop(g);
        assert_eq!(sent, [0x01, 0x08, 0xEA]);

        let e = launch(&conn, &sha("Multi.d64"), &dev, "", &none, false).unwrap_err();
        assert!(e.to_string().contains("2 Dateien"), "{e}");
        let e = launch(&conn, &sha("Band.tap"), &dev, "", &none, false).unwrap_err();
        assert!(e.to_string().contains("TAP"), "{e}");
        let e = launch(&conn, &sha("Hopper.prg"), &dev, "", &none, true).unwrap_err();
        assert!(e.to_string().contains("einlegen"), "{e}");

        // Startordner leeren geht nur im Menü (das letzte Spiel läuft noch) – dann über die
        // gemeinsame Dateischicht.
        let e = clean_upload_dir(&dev, "").unwrap_err();
        assert!(crate::teensyrom::is_busy(&e), "{e}");
        m.state.lock().unwrap().busy = false;
        let (deleted, kept) = clean_upload_dir(&dev, "").unwrap();
        assert_eq!((deleted, kept), (2, 0));
        assert!(m.state.lock().unwrap().files.is_empty());
    }

    #[test]
    fn auf_bmx_starten() {
        use crate::bmx::mock;
        let dir = tempfile::tempdir().unwrap();
        let w = |name: &str, data: &[u8]| std::fs::write(dir.path().join(name), data).unwrap();
        w("Hopper.prg", &[0x01, 0x08, 0x60]);
        w("Disk.d64", &testdata::d64(b"D", &[(b"GAME", 0x82, 1)]));
        w("Modul.crt", b"C64 CARTRIDGE   \0\0\0\x40");
        w(
            "Band.tap",
            b"C64-TAPE-RAW\x01\0\0\0\x04\0\0\0\x30\x30\x30\x30",
        );
        let conn = db::open_in_memory().unwrap();
        let id = db::add_local_source(&conn, dir.path()).unwrap();
        crate::scan::scan_local(
            &conn,
            id,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        let sha = |name: &str| -> String {
            conn.query_row("SELECT sha1 FROM files WHERE path = ?", [name], |r| {
                r.get(0)
            })
            .unwrap()
        };
        let m = mock::start();
        m.state.lock().unwrap().password = "pw".into();
        let dev = Device {
            id: 1,
            name: "BMX".into(),
            host: m.addr.clone(),
            product: "BMX".into(),
            upload_dir: "/USER/disks/Breadbin".into(),
            wifi_mac: String::new(),
            kind: remote::BMX.into(),
            unique_id: String::new(),
            mac: String::new(),
        };
        let none = |_: &str| String::new();
        let r = launch(&conn, &sha("Hopper.prg"), &dev, "pw", &none, false).unwrap();
        assert!(r.copied);
        assert!(
            r.device_path.starts_with("/USER/disks/Breadbin/Hopper_"),
            "{}",
            r.device_path
        );
        {
            let g = m.state.lock().unwrap();
            let a: Vec<&str> = g.actions.iter().map(|(a, _)| a.as_str()).collect();
            assert_eq!(a, ["media.cartridge.detach", "media.autostart"]);
            assert!(g.actions[1].1.starts_with("USER:/disks/Breadbin/Hopper_"));
        }
        // Zweiter Start: liegt schon dort – nicht noch einmal hochladen.
        let r = launch(&conn, &sha("Hopper.prg"), &dev, "pw", &none, false).unwrap();
        assert!(!r.copied);
        assert_eq!(m.state.lock().unwrap().uploads, 1);

        let r = launch(&conn, &sha("Modul.crt"), &dev, "pw", &none, false).unwrap();
        assert!(
            r.device_path.starts_with("/USER/carts/Breadbin/Modul_"),
            "{}",
            r.device_path
        );
        let r = launch(&conn, &sha("Band.tap"), &dev, "pw", &none, false).unwrap();
        assert!(
            r.device_path.starts_with("/USER/tapes/Breadbin/Band_"),
            "{}",
            r.device_path
        );
        assert_eq!(
            m.state.lock().unwrap().cart,
            "",
            "Modul vor dem Bandstart abgezogen"
        );

        // Einlegen ohne Neustart, auch wenn die Diskette schon eingelegt ist.
        let r = launch(&conn, &sha("Disk.d64"), &dev, "pw", &none, true).unwrap();
        assert!(r.message.contains("Laufwerk 8"), "{}", r.message);
        let r2 = launch(&conn, &sha("Disk.d64"), &dev, "pw", &none, true).unwrap();
        assert!(!r2.copied);
        assert_eq!(
            m.state.lock().unwrap().actions.last().unwrap().0,
            "media.drive.8.attach"
        );
        let e = launch(&conn, &sha("Hopper.prg"), &dev, "pw", &none, true).unwrap_err();
        assert!(e.to_string().contains("Disk-Abbilder"), "{e}");

        // Spielstand: die eingelegte Diskette ändert sich auf dem Gerät.
        {
            let mut g = m.state.lock().unwrap();
            let key = g
                .files
                .keys()
                .find(|k| k.ends_with(".d64"))
                .unwrap()
                .clone();
            g.files.get_mut(&key).unwrap()[0] ^= 0xFF;
            g.drive8.clear();
        }
        let changed = changed_disks(&dev, "pw").unwrap();
        assert_eq!(changed.len(), 1);
        // Leeren: in disks/, carts/ und tapes/; der Spielstand bleibt.
        let (deleted, kept) = clean_upload_dir(&dev, "pw").unwrap();
        assert_eq!(
            (deleted, kept),
            (3, 1),
            "Spielstand zählt als übrig gelassen"
        );
        assert_eq!(m.state.lock().unwrap().files.len(), 1);
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

    fn found(host: &str, kind: &str, uid: &str, mac: &str) -> crate::power::Found {
        crate::power::Found {
            host: host.into(),
            product: String::new(),
            hostname: String::new(),
            firmware: String::new(),
            unique_id: uid.into(),
            wifi_mac: String::new(),
            ethernet_mac: String::new(),
            mac: mac.into(),
            other_hosts: Vec::new(),
            kind: kind.into(),
        }
    }

    fn dev(host: &str, kind: &str, uid: &str, mac: &str, wifi: &str) -> Device {
        Device {
            id: 1,
            name: "Gerät".into(),
            host: host.into(),
            product: String::new(),
            upload_dir: "/USB0/Breadbin".into(),
            wifi_mac: wifi.into(),
            kind: kind.into(),
            unique_id: uid.into(),
            mac: mac.into(),
        }
    }

    #[test]
    fn geraet_unter_neuer_adresse_wiederfinden() {
        let list = vec![
            found(
                "192.0.2.20",
                remote::ULTIMATE,
                "ABC123",
                "02:15:41:00:00:01",
            ),
            found("192.0.2.30", remote::BMX, "", "D8:3A:DD:00:00:05"),
            found(
                "192.0.2.40",
                remote::ULTIMATE,
                "ZZZ999",
                "02:15:41:00:00:09",
            ),
        ];
        // Über die Kennung des Ultimate.
        let d = dev("192.0.2.10", remote::ULTIMATE, "ABC123", "", "");
        assert_eq!(find_moved(&d, &list).as_deref(), Some("192.0.2.20"));
        // Kennung geändert, MAC passt noch (Groß-/Kleinschreibung egal).
        let d = dev(
            "192.0.2.10",
            remote::ULTIMATE,
            "NEU",
            "02:15:41:00:00:09",
            "",
        );
        assert_eq!(find_moved(&d, &list).as_deref(), Some("192.0.2.40"));
        // BMX ohne Kennung: nur über die MAC.
        let d = dev("192.0.2.11", remote::BMX, "", "d8:3a:dd:00:00:05", "");
        assert_eq!(find_moved(&d, &list).as_deref(), Some("192.0.2.30"));
        // Gleiche MAC, aber andere Geräteart: kein Treffer.
        let d = dev("192.0.2.11", remote::BMC64, "", "D8:3A:DD:00:00:05", "");
        assert_eq!(find_moved(&d, &list), None);
        // Ohne gemerkte Kennung und MAC: nichts zu finden.
        let d = dev("192.0.2.11", remote::BMX, "", "", "");
        assert_eq!(find_moved(&d, &list), None);
        // Noch unter der eigenen Adresse: kein Umzug.
        let d = dev("192.0.2.20", remote::ULTIMATE, "ABC123", "", "");
        assert_eq!(find_moved(&d, &list), None);
        // WLAN-MAC des Geräts (vom Wecken bekannt) passt zur gemeldeten WLAN-MAC.
        let mut f = found("192.0.2.50", remote::ULTIMATE, "", "");
        f.wifi_mac = "98:88:E0:06:F6:78".into();
        let d = dev("192.0.2.10", remote::ULTIMATE, "", "", "98:88:E0:06:F6:78");
        assert_eq!(find_moved(&d, &[f]).as_deref(), Some("192.0.2.50"));
    }

    #[test]
    fn umzug_nimmt_quellen_mit() {
        let conn = crate::db::open_in_memory().unwrap();
        let mut d = dev("192.0.2.10", remote::BMX, "", "", "");
        d.id = 0;
        d.id = save_device(&conn, &d).unwrap();
        let other = dev("192.0.2.99", remote::ULTIMATE, "", "", "");
        let other_id = save_device(&conn, &Device { id: 0, ..other }).unwrap();
        let s1 =
            crate::db::add_device_source(&conn, remote::BMX, "192.0.2.10", "/USER/disks", "BMX")
                .unwrap();
        let s2 = crate::db::add_device_source(
            &conn,
            remote::BMX,
            "192.0.2.100",
            "/USER/disks",
            "anderer",
        )
        .unwrap();
        assert!(remember_identity(&conn, d.id, "", "AA:BB:CC:DD:EE:FF").unwrap());
        assert!(
            !remember_identity(&conn, d.id, "", "AA:BB:CC:DD:EE:FF").unwrap(),
            "unverändert"
        );
        assert!(
            !remember_identity(&conn, d.id, "", "").unwrap(),
            "leer überschreibt nichts"
        );

        assert_eq!(move_device(&conn, &d, "192.0.2.20").unwrap(), 1);
        let loc = |id: i64| -> String {
            conn.query_row("SELECT location FROM sources WHERE id = ?", [id], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_eq!(loc(s1), "bmx://192.0.2.20/USER/disks");
        // 192.0.2.100 beginnt wie 192.0.2.10, ist aber ein anderes Gerät.
        assert_eq!(loc(s2), "bmx://192.0.2.100/USER/disks");
        let moved = get_device(&conn, d.id).unwrap();
        assert_eq!(moved.host, "192.0.2.20");
        assert_eq!(moved.mac, "AA:BB:CC:DD:EE:FF");
        // Adresse schon von einem anderen Gerät belegt: Fehler, nichts geändert.
        assert!(move_device(&conn, &moved, "192.0.2.99").is_err());
        assert_eq!(get_device(&conn, d.id).unwrap().host, "192.0.2.20");
        assert_eq!(get_device(&conn, other_id).unwrap().host, "192.0.2.99");
    }
}
