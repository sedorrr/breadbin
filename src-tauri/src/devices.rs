//! Befehle für Ultimate-Geräte: anlegen, prüfen, Ordner als Quelle, Spiele starten.
//!
//! Ein Netzwerk-Passwort des Geräts (falls gesetzt) liegt im Schlüsselbund.

use base64::Engine;
use breadbin_core::launch::{self, Device, DeviceInfo, LaunchResult};
use breadbin_core::tr;
use breadbin_core::{db, metadata, power, screenshot};
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
    let id = db::add_ultimate_source(&conn, &dev.host, &path, &dev.name).map_err(err)?;
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
    let png = tauri::async_runtime::spawn_blocking(move || {
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

/// An oder aus? Dazu, ob sich das Gerät aus der App ein- bzw. ausschalten lässt.
#[tauri::command]
pub async fn device_status(app: AppHandle, id: i64) -> CmdResult<DeviceStatus> {
    let dev = device(&app, id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let password = device_password(&dev.host);
        let online = power::is_online(&dev.host, &password);
        let (firmware, kernal, product) = if online {
            let rest = launch::Rest::new(&dev.host, &password);
            let info = rest.info().unwrap_or_default();
            let product = if info.product.is_empty() {
                dev.product.clone()
            } else {
                info.product
            };
            (info.firmware, rest.kernal(), product)
        } else {
            Default::default()
        };
        DeviceStatus {
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
        }
    })
    .await
    .map_err(err)
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

#[tauri::command]
pub async fn discover_devices() -> CmdResult<Vec<power::Found>> {
    tauri::async_runtime::spawn_blocking(|| power::discover().map_err(err))
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
        let mut f = breadbin_core::ftp::Ftp::connect(&dev.host, &device_password(&dev.host))
            .map_err(err)?;
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
