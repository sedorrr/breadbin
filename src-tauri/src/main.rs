// Unter Windows kein Konsolenfenster neben der App öffnen.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Desktop-Oberfläche von Breadbin (Tauri). Die eigentliche Arbeit
//! erledigt `breadbin-core`; hier stehen nur die Befehle für die Oberfläche.

mod devices;
mod scraper;

use breadbin_core::tr;
use breadbin_core::{db, scan, share};
use rusqlite::Connection;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

struct AppState {
    db_path: PathBuf,
    /// Bilder von ScreenScraper.
    media_dir: PathBuf,
    conn: Mutex<Connection>,
    /// Abbruch-Merker des laufenden Scans, `None` wenn keiner läuft.
    scan_cancel: Mutex<Option<Arc<AtomicBool>>>,
    /// Abbruch-Merker des laufenden Abrufs von Beschreibungen.
    scrape_cancel: Mutex<Option<Arc<AtomicBool>>>,
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}

#[tauri::command]
fn list_sources(state: State<AppState>) -> CmdResult<Vec<db::Source>> {
    db::list_sources(&state.conn.lock().unwrap()).map_err(err)
}

#[tauri::command]
fn add_folder(state: State<AppState>, path: String) -> CmdResult<i64> {
    db::add_local_source(&state.conn.lock().unwrap(), Path::new(&path)).map_err(err)
}

/// Sprache der Meldungen aus dem Rust-Teil (von der Oberfläche beim Start gesetzt).
#[tauri::command]
fn set_language(lang: String) {
    breadbin_core::i18n::set_language(&lang);
}

#[tauri::command]
fn set_source_quick(state: State<AppState>, id: i64, quick: bool) -> CmdResult<()> {
    db::set_source_quick(&state.conn.lock().unwrap(), id, quick).map_err(err)
}

#[tauri::command]
fn remove_source(state: State<AppState>, id: i64) -> CmdResult<()> {
    if state.scan_cancel.lock().unwrap().is_some() {
        return Err(tr!(
            "Während eines Scans können keine Quellen entfernt werden.",
            "Sources cannot be removed while a scan is running."
        ));
    }
    db::remove_source(&state.conn.lock().unwrap(), id).map_err(err)
}

#[tauri::command]
fn search(state: State<AppState>, options: db::Search) -> CmdResult<Vec<db::GameRow>> {
    db::search_with(&state.conn.lock().unwrap(), &options).map_err(err)
}

// ---------------------------------------------------------------- Eigene Listen und Notizen

#[tauri::command]
fn lists(state: State<AppState>) -> CmdResult<Vec<db::GameList>> {
    db::lists(&state.conn.lock().unwrap()).map_err(err)
}

#[tauri::command]
fn create_list(state: State<AppState>, name: String) -> CmdResult<i64> {
    db::create_list(&state.conn.lock().unwrap(), &name).map_err(err)
}

#[tauri::command]
fn rename_list(state: State<AppState>, id: i64, name: String) -> CmdResult<()> {
    db::rename_list(&state.conn.lock().unwrap(), id, &name).map_err(err)
}

#[tauri::command]
fn delete_list(state: State<AppState>, id: i64) -> CmdResult<()> {
    db::delete_list(&state.conn.lock().unwrap(), id).map_err(err)
}

#[tauri::command]
fn set_in_list(state: State<AppState>, id: i64, sha1s: Vec<String>, on: bool) -> CmdResult<()> {
    db::set_in_list(&state.conn.lock().unwrap(), id, &sha1s, on).map_err(err)
}

/// Notiz und Listen eines Spiels (alle Fassungen, die gezeigte zuerst).
#[derive(Serialize)]
struct GameNote {
    note: db::Note,
    lists: Vec<i64>,
}

#[tauri::command]
fn game_note(state: State<AppState>, sha1s: Vec<String>) -> CmdResult<GameNote> {
    let conn = state.conn.lock().unwrap();
    Ok(GameNote {
        note: db::note_of(&conn, &sha1s).map_err(err)?,
        lists: db::lists_of(&conn, &sha1s).map_err(err)?,
    })
}

#[tauri::command]
fn save_note(state: State<AppState>, sha1s: Vec<String>, note: db::Note) -> CmdResult<()> {
    db::save_note(&state.conn.lock().unwrap(), &sha1s, &note).map_err(err)
}

/// Schreibt alle Treffer der aktuellen Suche als CSV-Datei. Gibt die Zahl der Zeilen zurück.
#[tauri::command]
fn export_csv(state: State<AppState>, options: db::Search, path: String) -> CmdResult<usize> {
    let (csv, n) = db::export_csv(&state.conn.lock().unwrap(), &options).map_err(err)?;
    std::fs::write(&path, csv).map_err(err)?;
    Ok(n)
}

/// Zahl der teilbaren Spiele, die noch nicht (oder in anderer Fassung) eingesandt wurden.
#[tauri::command]
fn submit_pending_count(state: State<AppState>) -> CmdResult<usize> {
    share::pending_submission(&state.conn.lock().unwrap())
        .map(|g| g.len())
        .map_err(err)
}

/// Zahl der teilbaren Spiele (selbst geschrieben).
#[tauri::command]
fn share_count(state: State<AppState>) -> CmdResult<usize> {
    share::shareable(&state.conn.lock().unwrap())
        .map(|g| g.len())
        .map_err(err)
}

/// Eigene Angaben als Austauschdatei speichern. Gibt die Zahl der Spiele zurück.
#[tauri::command]
async fn export_share(state: State<'_, AppState>, path: String) -> CmdResult<usize> {
    let app = concat!("Breadbin ", env!("CARGO_PKG_VERSION"));
    share::export(
        &state.conn.lock().unwrap(),
        &state.media_dir,
        Path::new(&path),
        app,
    )
    .map_err(err)
}

/// Gemeinschaftsdaten von der eingestellten Adresse laden und einlesen.
#[tauri::command]
async fn community_update(state: State<'_, AppState>) -> CmdResult<share::ImportReport> {
    let url = scraper::community_url(&state);
    let agent = concat!("Breadbin/", env!("CARGO_PKG_VERSION"));
    // Erst laden (Datenbank frei), dann einlesen.
    let file = tauri::async_runtime::spawn_blocking(move || share::download(&url, agent))
        .await
        .map_err(err)?
        .map_err(err)?;
    let result =
        share::import(&mut state.conn.lock().unwrap(), &state.media_dir, &file).map_err(err);
    let _ = std::fs::remove_file(&file);
    let report = result?;
    // Merken, damit die selbsttätige Aktualisierung diesen Stand nicht noch einmal lädt.
    let conn = state.conn.lock().unwrap();
    db::set_setting(&conn, "community_checked", &unix_now().to_string()).map_err(err)?;
    db::set_setting(&conn, "community_created", &report.created.to_string()).map_err(err)?;
    Ok(report)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Fällig, wenn der letzte Lauf (Einstellung `key`, Unix-Zeit) länger als `secs` her ist.
fn due(conn: &Connection, key: &str, secs: i64) -> bool {
    let last = db::get_setting(conn, key)
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    unix_now() - last >= secs
}

/// Beim Start: Gemeinschaftsdaten selbst aktualisieren, wenn eingeschaltet und die letzte Prüfung
/// eine Woche her ist. Erst `stand.json` (klein) vergleichen, nur bei neuem Stand die Datei laden.
/// `None` = nichts zu tun.
#[tauri::command]
async fn community_auto_update(
    state: State<'_, AppState>,
) -> CmdResult<Option<share::ImportReport>> {
    {
        let conn = state.conn.lock().unwrap();
        if !scraper::flag_on(&conn, "community_auto") || !due(&conn, "community_checked", 7 * 86400)
        {
            return Ok(None);
        }
    }
    let url = scraper::community_url(&state);
    let agent = concat!("Breadbin/", env!("CARGO_PKG_VERSION"));
    let known = db::get_setting(&state.conn.lock().unwrap(), "community_created")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok());
    let stand = match share::stand_url(&url) {
        Some(su) => tauri::async_runtime::spawn_blocking(move || {
            share::fetch_json::<share::Stand>(&su, agent)
        })
        .await
        .map_err(err)?
        .ok(),
        None => None,
    };
    if let (Some(stand), Some(known)) = (&stand, known)
        && stand.created <= known
    {
        db::set_setting(
            &state.conn.lock().unwrap(),
            "community_checked",
            &unix_now().to_string(),
        )
        .map_err(err)?;
        return Ok(None);
    }
    let file = tauri::async_runtime::spawn_blocking(move || share::download(&url, agent))
        .await
        .map_err(err)?
        .map_err(err)?;
    let result =
        share::import(&mut state.conn.lock().unwrap(), &state.media_dir, &file).map_err(err);
    let _ = std::fs::remove_file(&file);
    let report = result?;
    let conn = state.conn.lock().unwrap();
    db::set_setting(&conn, "community_checked", &unix_now().to_string()).map_err(err)?;
    db::set_setting(&conn, "community_created", &report.created.to_string()).map_err(err)?;
    Ok(Some(report))
}

/// Neue Breadbin-Fassung? Beim Start höchstens einmal am Tag (mit `force` immer), nur wenn
/// eingeschaltet. `None` = keine neuere Fassung.
#[tauri::command]
async fn check_update(
    state: State<'_, AppState>,
    force: bool,
) -> CmdResult<Option<breadbin_core::update::VersionInfo>> {
    {
        let conn = state.conn.lock().unwrap();
        if !force
            && (!scraper::flag_on(&conn, "update_check") || !due(&conn, "update_checked", 86400))
        {
            // Schon heute gesehen: die gemerkte Fassung weiter anzeigen.
            let seen = db::get_setting(&conn, "update_seen")
                .ok()
                .flatten()
                .and_then(|v| serde_json::from_str(&v).ok());
            return Ok(seen.filter(|v: &breadbin_core::update::VersionInfo| {
                scraper::flag_on(&conn, "update_check")
                    && breadbin_core::update::is_newer(&v.version, env!("CARGO_PKG_VERSION"))
            }));
        }
    }
    let agent = concat!("Breadbin/", env!("CARGO_PKG_VERSION"));
    let info = tauri::async_runtime::spawn_blocking(move || {
        breadbin_core::update::fetch(breadbin_core::update::VERSION_URL, agent)
    })
    .await
    .map_err(err)?
    .map_err(err)?;
    let conn = state.conn.lock().unwrap();
    db::set_setting(&conn, "update_checked", &unix_now().to_string()).map_err(err)?;
    db::set_setting(
        &conn,
        "update_seen",
        &serde_json::to_string(&info).map_err(err)?,
    )
    .map_err(err)?;
    Ok(Some(info)
        .filter(|v| breadbin_core::update::is_newer(&v.version, env!("CARGO_PKG_VERSION"))))
}

/// „Beitrag vorbereiten“: eigene Angaben im Aufbau der Gemeinschaftsdatenbank in einen Ordner schreiben.
#[tauri::command]
async fn community_contribute(state: State<'_, AppState>, dir: String) -> CmdResult<usize> {
    let app = concat!("Breadbin ", env!("CARGO_PKG_VERSION"));
    share::export_dir(
        &state.conn.lock().unwrap(),
        &state.media_dir,
        Path::new(&dir),
        app,
    )
    .map_err(err)
}

/// Beitrag an den Server der Gemeinschaftsdatenbank schicken (Adresse in den Einstellungen).
#[tauri::command]
async fn community_submit(
    state: State<'_, AppState>,
    name: String,
    hinweis: String,
    all: Option<bool>,
) -> CmdResult<share::SubmitResult> {
    let url = scraper::submit_url(&state);
    if url.is_empty() {
        return Err(tr!(
            "Keine Adresse für Beiträge eingestellt (Einstellungen).",
            "No address for contributions set (Settings)."
        ));
    }
    let agent = concat!("Breadbin/", env!("CARGO_PKG_VERSION"));
    // Standard: nur Neues und Geändertes. Große Mengen gehen in mehreren Beiträgen hinaus (Grenzen des Servers);
    // jeder angenommene Teil gilt sofort als eingesandt, ein späterer Fehler schickt beim nächsten Mal nur den Rest.
    let chunks = {
        let conn = state.conn.lock().unwrap();
        let games = if all.unwrap_or(false) {
            share::shareable(&conn)
        } else {
            share::pending_submission(&conn)
        }
        .map_err(err)?;
        if games.is_empty() {
            return Err(share::nothing_new().to_string());
        }
        share::submission_chunks(games, &state.media_dir)
    };
    let total = chunks.len();
    let mut done = share::SubmitResult { ok: true, ..Default::default() };
    for (i, chunk) in chunks.into_iter().enumerate() {
        let tmp = std::env::temp_dir().join(format!("breadbin-beitrag-{}-{i}.zip", std::process::id()));
        let prints: Vec<(String, String)> = chunk
            .iter()
            .map(|s| (s.0.sha1.clone(), share::fingerprint(s)))
            .collect();
        // Unter der Sperre nur die Datei schreiben, hochladen ohne Sperre.
        share::export_games(chunk, &state.media_dir, &tmp, agent).map_err(err)?;
        let hint = if total > 1 {
            format!("{} (Teil {}/{total})", hinweis.trim(), i + 1).trim().to_string()
        } else {
            hinweis.clone()
        };
        let result = tauri::async_runtime::spawn_blocking({
            let (tmp, url, name) = (tmp.clone(), url.clone(), name.clone());
            move || share::submit_file(&tmp, &url, &name, &hint, agent)
        })
        .await
        .map_err(err)?;
        let _ = std::fs::remove_file(&tmp);
        let (n, g) = (done.teile, done.spiele);
        let schon = if n > 0 {
            tr!(
                "{n} Beiträge mit {g} Spielen sind schon angekommen; der Rest kommt beim nächsten Einsenden. ",
                "{n} contributions with {g} games have already arrived; the rest goes out next time. "
            )
        } else {
            String::new()
        };
        let mut result = result.map_err(|e| format!("{schon}{}", err(e)))?;
        if !result.ok {
            result.fehler = format!("{schon}{}", result.fehler);
            return Ok(result);
        }
        share::mark_submitted(&state.conn.lock().unwrap(), &prints).map_err(err)?;
        done.id = result.id;
        done.ids.push(result.id);
        done.spiele += result.spiele;
        done.teile += 1;
    }
    Ok(done)
}

/// Austauschdatei eines anderen einlesen.
#[tauri::command]
async fn import_share(state: State<'_, AppState>, path: String) -> CmdResult<share::ImportReport> {
    share::import(
        &mut state.conn.lock().unwrap(),
        &state.media_dir,
        Path::new(&path),
    )
    .map_err(err)
}

/// Ganze Bibliothek (Datenbank und Bilder) als ZIP sichern.
#[tauri::command]
async fn backup_library(
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<breadbin_core::backup::BackupInfo> {
    let app = concat!("Breadbin ", env!("CARGO_PKG_VERSION"));
    breadbin_core::backup::backup(
        &state.conn.lock().unwrap(),
        &state.media_dir,
        Path::new(&path),
        app,
    )
    .map_err(err)
}

/// Sicherung zurückspielen: ersetzt die Datenbank (die bisherige bleibt als
/// `spiele.vor-wiederherstellung.sqlite` liegen) und ergänzt die Bilder.
#[tauri::command]
async fn restore_library(state: State<'_, AppState>, path: String) -> CmdResult<usize> {
    use breadbin_core::backup;
    let src = PathBuf::from(&path);
    let staged = state.db_path.with_extension("wiederherstellung.sqlite");
    let _ = std::fs::remove_file(&staged);
    // Erst prüfen und auspacken, dann tauschen – eine kaputte Sicherung lässt alles, wie es ist.
    {
        let (src, out) = (src.clone(), staged.clone());
        let unpacked = tauri::async_runtime::spawn_blocking(move || backup::unpack_db(&src, &out))
            .await
            .map_err(err)?;
        if let Err(e) = unpacked {
            let _ = std::fs::remove_file(&staged);
            return Err(err(e));
        }
    }
    // Tausch in einem eigenen Block: die Sperre darf nicht über ein await reichen.
    {
        let mut conn = state.conn.lock().unwrap();
        // Alte Verbindung schließen, damit die Datei frei ist.
        drop(std::mem::replace(
            &mut *conn,
            Connection::open_in_memory().map_err(err)?,
        ));
        let keep = state.db_path.with_extension("vor-wiederherstellung.sqlite");
        let _ = std::fs::remove_file(&keep);
        std::fs::rename(&state.db_path, &keep).map_err(err)?;
        for ext in ["sqlite-wal", "sqlite-shm"] {
            let _ = std::fs::remove_file(state.db_path.with_extension(ext));
        }
        let swapped = std::fs::rename(&staged, &state.db_path);
        *conn = match swapped
            .map_err(anyhow::Error::from)
            .and_then(|_| db::open(&state.db_path))
        {
            Ok(c) => c,
            Err(e) => {
                // Zurück zur bisherigen Datenbank.
                let _ = std::fs::rename(&keep, &state.db_path);
                *conn = db::open(&state.db_path).map_err(err)?;
                return Err(err(e));
            }
        };
    }
    let media = state.media_dir.clone();
    tauri::async_runtime::spawn_blocking(move || backup::restore_media(&src, &media))
        .await
        .map_err(err)?
        .map_err(err)
}

/// Pfad der Datenbank (für „Datenordner zeigen“).
#[tauri::command]
fn data_path(state: State<AppState>) -> String {
    state.db_path.display().to_string()
}

/// Fassung beim Zusammenfassen allein stehen lassen oder wieder einordnen.
#[tauri::command]
fn set_ungrouped(state: State<AppState>, sha1: String, on: bool) -> CmdResult<()> {
    db::set_ungrouped(&state.conn.lock().unwrap(), &sha1, on).map_err(err)
}

#[tauri::command]
fn toggle_favorite(state: State<AppState>, sha1: String) -> CmdResult<bool> {
    db::toggle_favorite(&state.conn.lock().unwrap(), &sha1).map_err(err)
}

#[tauri::command]
fn details(state: State<AppState>, sha1: String) -> CmdResult<db::Details> {
    db::details(&state.conn.lock().unwrap(), &sha1).map_err(err)
}

/// Die Fassungen eines zusammengefassten Spiels.
#[tauri::command]
fn game_variants(state: State<AppState>, sha1s: Vec<String>) -> CmdResult<Vec<db::GameRow>> {
    db::rows_for(&state.conn.lock().unwrap(), &sha1s).map_err(err)
}

#[tauri::command]
fn cleanup(state: State<AppState>) -> CmdResult<db::Cleanup> {
    db::cleanup(&state.conn.lock().unwrap()).map_err(err)
}

#[tauri::command]
fn stats(state: State<AppState>) -> CmdResult<db::Stats> {
    db::stats(&state.conn.lock().unwrap()).map_err(err)
}

#[derive(Clone, Serialize)]
struct ScanEvent {
    source_id: i64,
    source_label: String,
    progress: scan::Progress,
    finished: bool,
    error: Option<String>,
}

/// Scannt die angegebenen Quellen (alle, wenn leer) nacheinander in einem eigenen Thread.
/// Fortschritt kommt als Ereignis `scan-progress`, das Ende als `scan-finished`.
#[tauri::command]
fn start_scan(app: AppHandle, state: State<AppState>, ids: Vec<i64>) -> CmdResult<()> {
    let mut running = state.scan_cancel.lock().unwrap();
    if running.is_some() {
        return Err(tr!(
            "Es läuft bereits ein Scan.",
            "A scan is already running."
        ));
    }
    let sources: Vec<db::Source> = db::list_sources(&state.conn.lock().unwrap())
        .map_err(err)?
        .into_iter()
        .filter(|s| ids.is_empty() || ids.contains(&s.id))
        .collect();
    let cancel = Arc::new(AtomicBool::new(false));
    *running = Some(cancel.clone());
    let db_path = state.db_path.clone();
    let awake = scraper::keep_awake(&state.conn.lock().unwrap());

    std::thread::spawn(move || {
        let _awake = awake.then(|| breadbin_core::awake::KeepAwake::start("Scan"));
        // Eigene Verbindung, damit Suche und Anzeige während des Scans weiterlaufen.
        let result = db::open(&db_path).map(|conn| {
            for s in &sources {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let mut last = Instant::now() - Duration::from_secs(1);
                let emit = |p: &scan::Progress, finished: bool, error: Option<String>| {
                    let _ = app.emit(
                        "scan-progress",
                        ScanEvent {
                            source_id: s.id,
                            source_label: s.label.clone(),
                            progress: p.clone(),
                            finished,
                            error,
                        },
                    );
                };
                let password = ftp_host(&s.location)
                    .map(devices::device_password)
                    .unwrap_or_default();
                let outcome = scan::scan_source(&conn, s.id, &password, &cancel, &mut |p| {
                    if last.elapsed() >= Duration::from_millis(150) {
                        last = Instant::now();
                        emit(p, false, None);
                    }
                });
                match outcome {
                    Ok(p) => emit(&p, true, None),
                    Err(e) => emit(&scan::Progress::default(), true, Some(err(e))),
                }
            }
        });
        if let Err(e) = result {
            let _ = app.emit("scan-error", err(e));
        }
        *app.state::<AppState>().scan_cancel.lock().unwrap() = None;
        let _ = app.emit("scan-finished", ());
    });
    Ok(())
}

#[tauri::command]
fn cancel_scan(state: State<AppState>) {
    if let Some(c) = state.scan_cancel.lock().unwrap().as_ref() {
        c.store(true, Ordering::Relaxed);
    }
}

fn ftp_host(location: &str) -> Option<&str> {
    breadbin_core::ftp::split_location(location).map(|(h, _)| h)
}

/// Voller Pfad eines Fundorts auf diesem Rechner (für „Im Ordner zeigen“).
#[tauri::command]
fn local_path(state: State<AppState>, source_id: i64, path: String) -> CmdResult<String> {
    let source = db::get_source(&state.conn.lock().unwrap(), source_id).map_err(err)?;
    Ok(Path::new(&source.location)
        .join(path)
        .to_string_lossy()
        .to_string())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Fenstergröße und -lage beim nächsten Start wiederherstellen.
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db_path = dir.join("spiele.sqlite");
            let conn = db::open(&db_path)?;
            app.manage(AppState {
                db_path,
                media_dir: dir.join("media"),
                conn: Mutex::new(conn),
                scan_cancel: Mutex::new(None),
                scrape_cancel: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_sources,
            add_folder,
            remove_source,
            set_source_quick,
            set_language,
            search,
            details,
            game_variants,
            stats,
            start_scan,
            cancel_scan,
            local_path,
            export_csv,
            share_count,
            submit_pending_count,
            export_share,
            import_share,
            community_update,
            community_auto_update,
            check_update,
            community_contribute,
            community_submit,
            toggle_favorite,
            cleanup,
            backup_library,
            restore_library,
            lists,
            create_list,
            rename_list,
            delete_list,
            set_in_list,
            game_note,
            save_note,
            set_ungrouped,
            data_path,
            scraper::get_scraper_settings,
            scraper::save_scraper_settings,
            scraper::test_scraper,
            scraper::start_scrape,
            scraper::cancel_scrape,
            scraper::media_data,
            scraper::search_games,
            scraper::assign_game,
            scraper::reject_metadata,
            scraper::save_own_meta,
            scraper::clear_meta,
            scraper::set_own_image,
            scraper::move_image,
            scraper::search_library,
            scraper::copy_meta,
            scraper::game_now,
            scraper::proposal_choices,
            scraper::propose,
            scraper::proposals,
            devices::list_devices,
            devices::probe_device,
            devices::save_device,
            devices::remove_device,
            devices::add_device_folder,
            devices::launch_game,
            devices::capture_screen,
            devices::device_status,
            devices::wake_device,
            devices::power_off_device,
            devices::discover_devices,
            devices::list_device_dirs,
            devices::clean_upload_dir,
            devices::copy_to_device,
            devices::fetch_saves,
        ])
        .run(tauri::generate_context!())
        .expect("Fehler beim Start der App");
}
