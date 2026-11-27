//! Befehle für Geräte (Ultimate, BMC64, TeensyROM, BMX): anlegen, prüfen, Ordner als Quelle, Spiele starten.
//!
//! Ein Netzwerk-Passwort, die BMC64-PIN bzw. das BMX-Passwort (falls gesetzt) liegt im Schlüsselbund.

use base64::Engine;
use breadbin_core::launch::{self, Device, DeviceInfo, LaunchResult};
use breadbin_core::tr;
use breadbin_core::{bmc64, bmx, db, emulator, metadata, power, screenshot, teensyrom};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

use crate::scraper::{secret_get, secret_set};
use crate::{AppState, CmdResult, err};

fn secret_name(host: &str) -> String {
    format!("ultimate-password:{}", host.trim())
}

/// Netzwerk-Passwort eines Geräts, leer wenn keins hinterlegt ist.
pub fn device_password(host: &str) -> String {
    secret_get(&secret_name(host)).unwrap_or_default()
}

#[tauri::command]
pub fn list_devices(state: State<AppState>) -> CmdResult<Vec<Device>> {
    launch::list_devices(&state.conn.lock().unwrap()).map_err(err)
}

/// Fragt Typ, Firmware und Laufwerke eines Geräts ab (ohne es zu speichern).
#[tauri::command]
pub async fn probe_device(host: String, password: Option<String>) -> CmdResult<DeviceInfo> {
    let password = password
        .filter(|p| !p.is_empty())
        .unwrap_or_else(|| device_password(&host));
    tauri::async_runtime::spawn_blocking(move || launch::probe(host.trim(), &password).map_err(err))
        .await
        .map_err(err)?
}

#[tauri::command]
pub fn save_device(
    state: State<AppState>,
    device: Device,
    password: Option<String>,
) -> CmdResult<i64> {
    if device.host.trim().is_empty() || device.name.trim().is_empty() {
        return Err(tr!(
            "Bitte Name und Adresse des Geräts eintragen.",
            "Please enter the device's name and address."
        ));
    }
    if let Some(pw) = password.filter(|p| !p.is_empty()) {
        secret_set(&secret_name(&device.host), &pw)?;
    }
    launch::save_device(&state.conn.lock().unwrap(), &device).map_err(err)
}

#[tauri::command]
pub fn remove_device(state: State<AppState>, id: i64) -> CmdResult<()> {
    launch::remove_device(&state.conn.lock().unwrap(), id).map_err(err)
}

/// Nimmt einen Ordner auf dem Gerät (z. B. `/USB0/Spiele`) als Quelle auf.
#[tauri::command]
pub fn add_device_folder(
    state: State<AppState>,
    device_id: i64,
    path: String,
    quick: bool,
) -> CmdResult<i64> {
    let conn = state.conn.lock().unwrap();
    let dev = launch::get_device(&conn, device_id).map_err(err)?;
    let id = db::add_device_source(&conn, &dev.kind, &dev.host, &path, &dev.name).map_err(err)?;
    db::set_source_quick(&conn, id, quick).map_err(err)?;
    Ok(id)
}

/// Startet ein Spiel auf einem Gerät. Setzt den C64 zurück.
#[tauri::command]
pub async fn launch_game(
    app: AppHandle,
    sha1: String,
    device_id: i64,
    insert_only: Option<bool>,
) -> CmdResult<LaunchResult> {
    let insert_only = insert_only.unwrap_or(false);
    let db_path = app.state::<AppState>().db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        // Eigene Verbindung: FTP-Übertragungen sollen die Oberfläche nicht blockieren.
        let conn = db::open(&db_path).map_err(err)?;
        let dev = launch::get_device(&conn, device_id).map_err(err)?;
        let result = launch::launch(
            &conn,
            &sha1,
            &dev,
            &device_password(&dev.host),
            &|h| device_password(h),
            insert_only,
        )
        .map_err(err)?;
        // Für „Zuletzt gestartet“ (nur echte Starts); ein Fehler hier soll den Start nicht verdecken.
        if !insert_only {
            let _ = db::record_launch(&conn, &sha1, &dev.name);
        }
        Ok(result)
    })
    .await
    .map_err(err)?
}

/// Wo auf diesem Rechner VICE liegt (für die Einstellungen).
#[tauri::command]
pub async fn find_emulators() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(emulator::find_emulators)
        .await
        .unwrap_or_default()
}

/// Startet ein Spiel in einem eingestellten Emulator (`index`, Standard der erste). `disks` = alle
/// Disketten desselben Spiels (für den Wechsel).
#[tauri::command]
pub async fn launch_emulator(
    app: AppHandle,
    sha1: String,
    disks: Option<Vec<String>>,
    index: Option<usize>,
) -> CmdResult<emulator::EmuResult> {
    let db_path = app.state::<AppState>().db_path.clone();
    let dir = db_path
        .parent()
        .map(|p| p.join("emulator"))
        .ok_or_else(|| err("Datenordner unbekannt"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db::open(&db_path).map_err(err)?;
        let emu = emulator::list(&conn)
            .into_iter()
            .nth(index.unwrap_or(0))
            .unwrap_or_default();
        let (path, args) = (emu.path, emu.args);
        let result = emulator::start(
            &conn,
            &sha1,
            &disks.unwrap_or_default(),
            &path,
            &args,
            &dir,
            &|h| device_password(h),
        )
        .map_err(err)?;
        let _ = db::record_launch(&conn, &sha1, "Emulator");
        Ok(result)
    })
    .await
    .map_err(err)?
}

/// Diskette in das zuletzt von Breadbin gestartete VICE einlegen (ohne Neustart).
#[tauri::command]
pub async fn emu_insert(app: AppHandle, sha1: String) -> CmdResult<String> {
    let db_path = app.state::<AppState>().db_path.clone();
    let dir = db_path
        .parent()
        .map(|p| p.join("emulator"))
        .ok_or_else(|| err("Datenordner unbekannt"))?;
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db::open(&db_path).map_err(err)?;
        emulator::insert(&conn, &sha1, &dir, &|h| device_password(h)).map_err(err)
    })
    .await
    .map_err(err)?
}

/// Bildschirmfoto aus dem zuletzt von Breadbin gestarteten VICE, als eigenes Bild der Art `kind`.
#[tauri::command]
pub async fn emu_capture(
    app: AppHandle,
    sha1s: Vec<String>,
    kind: String,
    crop: bool,
) -> CmdResult<String> {
    let png = tauri::async_runtime::spawn_blocking(move || {
        breadbin_core::vicemon::screenshot_png(crop).map_err(err)
    })
    .await
    .map_err(err)??;
    let state = app.state::<AppState>();
    metadata::set_own_image(
        &state.conn.lock().unwrap(),
        &sha1s,
        &kind,
        Some((&png, "png")),
        &state.media_dir,
    )
    .map_err(err)?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    ))
}

/// Spiele in einen Ordner auf dem Gerät kopieren; was schon dort liegt, wird übersprungen.
#[tauri::command]
pub async fn copy_to_device(
    app: AppHandle,
    device_id: i64,
    sha1s: Vec<String>,
    target: String,
) -> CmdResult<launch::CopyReport> {
    let db_path = app.state::<AppState>().db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db::open(&db_path).map_err(err)?;
        let dev = launch::get_device(&conn, device_id).map_err(err)?;
        launch::copy_to_device(
            &conn,
            &sha1s,
            &dev,
            &device_password(&dev.host),
            &|h| device_password(h),
            &target,
        )
        .map_err(err)
    })
    .await
    .map_err(err)?
}

/// Geänderte Disketten (Spielstände) aus dem Startordner in einen Ordner auf dem Rechner holen.
#[tauri::command]
/// `sha1s`: nur die Disketten dieser Inhalte (Knopf in den Details eines Spiels).
pub async fn fetch_saves(
    app: AppHandle,
    device_id: i64,
    target: String,
    sha1s: Option<Vec<String>>,
) -> CmdResult<Vec<String>> {
    let db_path = app.state::<AppState>().db_path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db::open(&db_path).map_err(err)?;
        let dev = launch::get_device(&conn, device_id).map_err(err)?;
        launch::fetch_saves(
            &conn,
            &dev,
            &device_password(&dev.host),
            std::path::Path::new(&target),
            sha1s.as_deref(),
        )
        .map_err(err)
    })
    .await
    .map_err(err)?
}

/// Bildschirmfoto über den Videostrom des Geräts, als eigenes Bild der Art `kind` gespeichert.
/// `crop` schneidet den Rahmen ab (320 × 200). Gibt das Bild als Daten-URL für die Vorschau zurück.
#[tauri::command]
pub async fn capture_screen(
    app: AppHandle,
    device_id: i64,
    sha1s: Vec<String>,
    kind: String,
    crop: bool,
) -> CmdResult<String> {
    let dev = device(&app, device_id)?;
    if dev.is_bmc64() {
        return Err(tr!(
            "BMC64 liefert keine Bildschirmfotos.",
            "BMC64 provides no screenshots."
        ));
    }
    if launch::is_cartridge_ultimate(&dev.product) {
        return Err(tr!(
            "Das 1541 Ultimate II/II+ liefert keine Bildschirmfotos – das geht nur mit Ultimate 64 und C64 Ultimate.",
            "The 1541 Ultimate II/II+ provides no screenshots – only Ultimate 64 and C64 Ultimate can do that."
        ));
    }
    if dev.is_teensyrom() {
        return Err(tr!(
            "TeensyROM liefert keine Bildschirmfotos.",
            "TeensyROM provides no screenshots."
        ));
    }
    let png = tauri::async_runtime::spawn_blocking(move || {
        if dev.is_bmx() {
            return bmx::Bmx::new(&dev.host, &device_password(&dev.host))
                .screenshot_png(crop)
                .map_err(err);
        }
        let frame = screenshot::capture(
            &dev.host,
            &device_password(&dev.host),
            Duration::from_secs(6),
        )
        .map_err(err)?;
        screenshot::to_png(&frame, crop).map_err(err)
    })
    .await
    .map_err(err)??;
    let state = app.state::<AppState>();
    metadata::set_own_image(
        &state.conn.lock().unwrap(),
        &sha1s,
        &kind,
        Some((&png, "png")),
        &state.media_dir,
    )
    .map_err(err)?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    ))
}

#[derive(serde::Serialize)]
pub struct DeviceStatus {
    online: bool,
    can_wake: bool,
    can_power_off: bool,
    /// Nur wenn an: Firmware, KERNAL und Hinweise, was damit nicht geht.
    firmware: String,
    kernal: String,
    hints: Vec<String>,
}

fn device(app: &AppHandle, id: i64) -> CmdResult<Device> {
    launch::get_device(&app.state::<AppState>().conn.lock().unwrap(), id).map_err(err)
}

/// An oder aus? Dazu, ob sich das Gerät aus der App ein- bzw. ausschalten lässt. Ist es erreichbar, merkt sich
/// Breadbin Kennung und MAC – damit findet „Im Netz suchen“ es nach einem Adresswechsel wieder (gesucht wird
/// nur, wenn der Nutzer das auslöst).
#[tauri::command]
pub async fn device_status(app: AppHandle, id: i64) -> CmdResult<DeviceStatus> {
    let dev = device(&app, id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let (st, unique_id, lan_mac) = status_of(&dev);
        if st.online {
            remember_identity(&app, &dev, &unique_id, &lan_mac);
        }
        st
    })
    .await
    .map_err(err)
}

/// Status eines Geräts unter seiner gespeicherten Adresse; dazu die Kennung (nur Ultimate).
fn status_of(dev: &Device) -> (DeviceStatus, String, String) {
    let password = device_password(&dev.host);
    if dev.is_bmc64() {
        let st = bmc64::quick_status(&dev.host, &password);
        return (
            DeviceStatus {
                online: st.is_some(),
                can_wake: false,
                can_power_off: false,
                firmware: st.as_ref().map(|s| s.version.clone()).unwrap_or_default(),
                kernal: String::new(),
                hints: st.as_ref().map(bmc64::hints).unwrap_or_default(),
            },
            String::new(),
            String::new(),
        );
    }
    if dev.is_bmx() {
        let st = bmx::quick_status(&dev.host, &password);
        return (
            DeviceStatus {
                online: st.is_some(),
                can_wake: false,
                can_power_off: false,
                firmware: st.as_ref().map(|s| s.version.clone()).unwrap_or_default(),
                kernal: String::new(),
                hints: st.as_ref().map(bmx::hints).unwrap_or_default(),
            },
            String::new(),
            String::new(),
        );
    }
    if dev.is_teensyrom() {
        let st = teensyrom::quick_status(&dev.host);
        return (
            DeviceStatus {
                online: st.is_some(),
                can_wake: false,
                can_power_off: false,
                firmware: st.as_ref().map(|s| s.version.clone()).unwrap_or_default(),
                kernal: String::new(),
                hints: st.as_ref().map(teensyrom::hints).unwrap_or_default(),
            },
            String::new(),
            String::new(),
        );
    }
    let online = power::is_online(&dev.host, &password);
    let (firmware, kernal, product, unique_id, lan_mac) = if online {
        let rest = launch::Rest::new(&dev.host, &password);
        let info = rest.info().unwrap_or_default();
        let product = if info.product.is_empty() {
            dev.product.clone()
        } else {
            info.product
        };
        let (uid, lan) = (info.unique_id.clone(), info.ethernet_mac.clone());
        (info.firmware, rest.kernal(), product, uid, lan)
    } else {
        Default::default()
    };
    let st = DeviceStatus {
        online,
        can_wake: !dev.wifi_mac.is_empty(),
        can_power_off: power::can_power_off(&dev.product),
        hints: if online {
            launch::device_hints(&product, &firmware, &kernal)
        } else {
            Vec::new()
        },
        firmware,
        kernal,
    };
    (st, unique_id, lan_mac)
}

/// Kennung und MAC einmal je Programmlauf und Gerät merken (die MAC steht nach dem Gespräch im ARP-Speicher;
/// liefert der nichts, gilt die LAN-MAC, die ein Ultimate selbst meldet).
fn remember_identity(app: &AppHandle, dev: &Device, unique_id: &str, lan_mac: &str) {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static DONE: Mutex<Option<HashSet<(i64, String)>>> = Mutex::new(None);
    let key = (dev.id, dev.host.clone());
    {
        let mut done = DONE.lock().unwrap_or_else(|e| e.into_inner());
        if !done.get_or_insert_with(HashSet::new).insert(key) {
            return;
        }
    }
    let mac = power::arp_mac(dev.host.trim()).unwrap_or_else(|| lan_mac.to_string());
    let state = app.state::<AppState>();
    let _ = launch::remember_identity(&state.conn.lock().unwrap(), dev.id, unique_id, &mac);
}

/// Umzug in der Datenbank und im Schlüsselbund (das Passwort hängt an der Adresse).
fn move_to(app: &AppHandle, dev: &Device, new_host: &str) -> CmdResult<()> {
    let state = app.state::<AppState>();
    launch::move_device(&state.conn.lock().unwrap(), dev, new_host).map_err(err)?;
    let password = device_password(&dev.host);
    if !password.is_empty() {
        secret_set(&secret_name(new_host), &password)?;
    }
    Ok(())
}

/// Schaltet das Gerät per Wake-on-WiFi ein und wartet, bis es antwortet. Gibt Sekunden zurück.
#[tauri::command]
pub async fn wake_device(app: AppHandle, id: i64) -> CmdResult<f32> {
    let dev = device(&app, id)?;
    if dev.wifi_mac.is_empty() {
        return Err(tr!(
            "Die WLAN-MAC des Geräts ist unbekannt. Gerät einmal von Hand einschalten und unter \
             „Bearbeiten“ auf „Verbinden und prüfen“ klicken.",
            "The device's Wi-Fi MAC is unknown. Switch the device on by hand once, then click \
             “Connect and check” under “Edit”."
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        power::wake_and_wait(
            &dev.host,
            &dev.wifi_mac,
            &device_password(&dev.host),
            Duration::from_secs(60),
        )
        .map(|d| d.as_secs_f32())
        .map_err(err)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn power_off_device(app: AppHandle, id: i64) -> CmdResult<power::PowerOff> {
    let dev = device(&app, id)?;
    tauri::async_runtime::spawn_blocking(move || {
        power::power_off(&dev.host, &device_password(&dev.host)).map_err(err)
    })
    .await
    .map_err(err)?
}

/// „Spiel beenden“: setzt den C64 auf dem Gerät zurück, ohne etwas Neues zu starten.
#[tauri::command]
pub async fn stop_game(app: AppHandle, id: i64) -> CmdResult<()> {
    let dev = device(&app, id)?;
    tauri::async_runtime::spawn_blocking(move || {
        launch::stop_game(&dev, &device_password(&dev.host)).map_err(err)
    })
    .await
    .map_err(err)?
}

/// „Neu suchen“ an einem nicht erreichbaren Gerät: sucht es im Netz (über Kennung bzw. MAC) und übernimmt bei einem
/// Treffer die neue Adresse samt Quellen und Passwort. `Some(adresse)` = gefunden (bei gleicher Adresse: wieder
/// erreichbar), `None` = nicht gefunden.
#[tauri::command]
pub async fn relocate_device(app: AppHandle, id: i64) -> CmdResult<Option<String>> {
    let dev = device(&app, id)?;
    if dev.is_teensyrom() {
        return Err(tr!(
            "Den TeensyROM kann Breadbin nicht im Netz suchen. Neue Adresse bitte unter „Bearbeiten“ eintragen.",
            "Breadbin cannot search the network for a TeensyROM. Please enter the new address under “Edit”."
        ));
    }
    if dev.unique_id.is_empty() && dev.mac.is_empty() && dev.wifi_mac.is_empty() {
        return Err(tr!(
            "Breadbin kennt Kennung und MAC dieses Geräts noch nicht (es war seit dem Update nicht erreichbar). \
             Neue Adresse bitte unter „Bearbeiten“ eintragen.",
            "Breadbin does not know this device's ID and MAC yet (it has not been reachable since the update). \
             Please enter the new address under “Edit”."
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let found = power::discover_cached(Duration::from_secs(5)).map_err(err)?;
        if found
            .iter()
            .any(|f| f.host.trim() == dev.host.trim() || f.other_hosts.contains(&dev.host))
        {
            return Ok(Some(dev.host.clone()));
        }
        match launch::find_moved(&dev, &found) {
            Some(new_host) => {
                move_to(&app, &dev, &new_host)?;
                Ok(Some(new_host))
            }
            None => Ok(None),
        }
    })
    .await
    .map_err(err)?
}

/// „Im Netz suchen“. Findet dabei ein schon eingetragenes Gerät unter neuer Adresse wieder, zieht es dorthin um.
#[tauri::command]
pub async fn discover_devices(app: AppHandle) -> CmdResult<Vec<power::Found>> {
    tauri::async_runtime::spawn_blocking(move || {
        let found = power::discover_cached(Duration::from_secs(5)).map_err(err)?;
        let known =
            launch::list_devices(&app.state::<AppState>().conn.lock().unwrap()).map_err(err)?;
        for dev in known.iter().filter(|d| !d.is_teensyrom()) {
            // Noch unter der alten Adresse da (z. B. LAN und WLAN): nichts umziehen.
            if found
                .iter()
                .any(|f| f.host.trim() == dev.host.trim() || f.other_hosts.contains(&dev.host))
            {
                continue;
            }
            if let Some(new_host) = launch::find_moved(dev, &found) {
                let _ = move_to(&app, dev, &new_host);
            }
        }
        Ok(found)
    })
    .await
    .map_err(err)?
}

/// Unterordner eines Ordners auf dem Gerät, für die Ordnerauswahl (`/` = Laufwerke).
#[tauri::command]
pub async fn list_device_dirs(
    app: AppHandle,
    device_id: i64,
    path: String,
) -> CmdResult<Vec<String>> {
    let dev = device(&app, device_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut f = launch::connect(&dev, &device_password(&dev.host)).map_err(err)?;
        let path = if path.trim().is_empty() {
            "/".to_string()
        } else {
            path
        };
        let mut dirs: Vec<String> = f
            .list(&path)
            .map_err(err)?
            .into_iter()
            .filter(|e| e.is_dir && !e.name.starts_with('.'))
            .map(|e| e.name)
            .collect();
        f.quit();
        dirs.sort_by_key(|d| d.to_lowercase());
        Ok(dirs)
    })
    .await
    .map_err(err)?
}

/// Entfernt im Startordner die Dateien, die Breadbin zum Starten hinkopiert hat.
#[tauri::command]
pub async fn clean_upload_dir(app: AppHandle, id: i64) -> CmdResult<(usize, usize)> {
    let dev = device(&app, id)?;
    tauri::async_runtime::spawn_blocking(move || {
        launch::clean_upload_dir(&dev, &device_password(&dev.host)).map_err(err)
    })
    .await
    .map_err(err)?
}
