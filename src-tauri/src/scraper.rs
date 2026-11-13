//! Befehle rund um ScreenScraper: Einstellungen, Zugangstest, Abruf, Bilder.
//!
//! Passwörter liegen im Schlüsselbund des Systems (macOS-Schlüsselbund, Windows-
//! Anmeldeinformationsverwaltung, Secret Service unter Linux), nie in der Datenbank.

use base64::Engine;
use breadbin_core::screenscraper::{
    ApiError, Choice, Client, Credentials, Game, Lookup, Proposal, UserInfo,
};
use breadbin_core::tr;
use breadbin_core::{db, metadata, share};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{AppState, CmdResult, err};

const KEYRING_SERVICE: &str = "de.dersedor.breadbin";
/// Höchstens so viele gleichzeitige Abfragen, auch wenn das Konto mehr erlaubt.
const MAX_THREADS: i64 = 4;
/// Programmname gegenüber ScreenScraper (so auch bei der Beantragung der Kennung angeben).
pub const SOFTNAME: &str = concat!("Breadbin-", env!("CARGO_PKG_VERSION"));
/// Entwickler-Kennung, beim Bauen über Umgebungsvariablen eingebracht (z. B. GitHub-Secrets).
/// Fehlt sie, kann sie in den Einstellungen eingetragen werden.
const BUILTIN_DEV_ID: Option<&str> = option_env!("SCREENSCRAPER_DEVID");
const BUILTIN_DEV_PASSWORD: Option<&str> = option_env!("SCREENSCRAPER_DEVPASSWORD");

fn builtin_dev() -> Option<(&'static str, &'static str)> {
    // Beim Eintragen der Secrets rutscht leicht ein Zeilenumbruch mit.
    let clean = |v: Option<&'static str>| v.map(str::trim).filter(|s| !s.is_empty());
    Some((clean(BUILTIN_DEV_ID)?, clean(BUILTIN_DEV_PASSWORD)?))
}

/// Gelesene Schlüsselbund-Einträge dieses Programmlaufs. macOS fragt bei jedem Zugriff einer
/// neu gebauten (ad hoc signierten) App nach – darum jeden Eintrag nur einmal lesen.
static SECRETS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
> = std::sync::LazyLock::new(Default::default);

pub(crate) fn secret_get(name: &str) -> Option<String> {
    if let Some(v) = SECRETS.lock().unwrap().get(name) {
        return v.clone();
    }
    let value = match keyring::Entry::new(KEYRING_SERVICE, name).and_then(|e| e.get_password()) {
        Ok(v) => Some(v),
        Err(keyring::Error::NoEntry) => None,
        // Abgelehnt oder Fehler: nicht merken, beim nächsten Bedarf erneut fragen.
        Err(_) => return None,
    };
    SECRETS
        .lock()
        .unwrap()
        .insert(name.to_string(), value.clone());
    value
}

pub(crate) fn secret_set(name: &str, value: &str) -> CmdResult<()> {
    keyring::Entry::new(KEYRING_SERVICE, name)
        .and_then(|e| e.set_password(value))
        .map_err(|e| {
            tr!(
                "Schlüsselbund nicht nutzbar: {e}",
                "Keychain not usable: {e}"
            )
        })?;
    SECRETS
        .lock()
        .unwrap()
        .insert(name.to_string(), Some(value.to_string()));
    Ok(())
}

/// Ob ein Passwort hinterlegt ist, steht als Merker in den Einstellungen, damit die Anzeige
/// den Schlüsselbund nicht anfasst. Ältere Installationen ohne Merker: einmal nachsehen.
fn has_secret(state: &AppState, flag: &str, secret: &str) -> bool {
    match setting(state, flag).as_str() {
        "1" => true,
        "0" => false,
        _ => {
            let has = secret_get(secret).is_some();
            let _ = db::set_setting(
                &state.conn.lock().unwrap(),
                flag,
                if has { "1" } else { "0" },
            );
            has
        }
    }
}

fn user_secret_name(user: &str) -> String {
    format!("screenscraper-password:{user}")
}
const DEV_SECRET_NAME: &str = "screenscraper-devpassword";

#[derive(Serialize)]
pub struct ScraperSettings {
    user: String,
    has_password: bool,
    dev_builtin: bool,
    dev_id: String,
    has_dev_password: bool,
    language: String,
    name_fallback: bool,
    /// Gleichzeitige Abfragen: 0 = automatisch (laut Konto, höchstens MAX_THREADS), sonst 1–4.
    threads: i64,
    keep_awake: bool,
    /// Bevorzugte Quelle für Angaben: `community` oder `screenscraper`.
    preferred_source: &'static str,
    /// Adresse der Gemeinschaftsdaten (Austauschdatei).
    community_url: String,
    /// Adresse für eingesandte Beiträge (leer = Einsenden aus).
    submit_url: String,
    /// Eingebaute Adressen (Breadbin-Server), damit die Oberfläche „Standard“ und „eigener Server“ unterscheidet.
    default_community_url: &'static str,
    default_submit_url: &'static str,
    /// Beim Start nach neuen Fassungen sehen.
    update_check: bool,
    /// Gemeinschaftsdaten beim Start selbst aktualisieren (höchstens einmal pro Woche).
    community_auto: bool,
    /// Nach dem Start ein Titelbild aufnehmen, wenn das Spiel noch keines hat (Standard aus).
    auto_title: bool,
    /// Sekunden zwischen Start und Aufnahme.
    auto_title_delay: i64,
    softname: &'static str,
}

#[derive(Deserialize)]
pub struct ScraperSettingsInput {
    user: String,
    /// Leer oder fehlend = gespeichertes Passwort behalten.
    password: Option<String>,
    dev_id: Option<String>,
    dev_password: Option<String>,
    language: String,
    #[serde(default = "yes")]
    name_fallback: bool,
    #[serde(default = "yes")]
    keep_awake: bool,
    #[serde(default)]
    preferred_source: Option<String>,
    #[serde(default)]
    threads: Option<i64>,
    #[serde(default)]
    community_url: Option<String>,
    #[serde(default)]
    submit_url: Option<String>,
    #[serde(default)]
    update_check: Option<bool>,
    #[serde(default)]
    community_auto: Option<bool>,
    #[serde(default)]
    auto_title: Option<bool>,
    #[serde(default)]
    auto_title_delay: Option<i64>,
}

/// Wartezeit für das automatische Titelbild: Standard 25 s, erlaubt 5–180 s.
const AUTO_TITLE_DELAY: i64 = 25;

fn auto_title_delay(conn: &rusqlite::Connection) -> i64 {
    db::get_setting(conn, "auto_title_delay")
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .map_or(AUTO_TITLE_DELAY, |n| n.clamp(5, 180))
}

fn yes() -> bool {
    true
}

/// Schalter, der ohne Eintrag an ist („0“ = aus).
pub(crate) fn flag_on(conn: &rusqlite::Connection, key: &str) -> bool {
    db::get_setting(conn, key).ok().flatten().as_deref() != Some("0")
}

/// Rechner während Scan und Abruf wach halten: Standard an.
pub(crate) fn keep_awake(conn: &rusqlite::Connection) -> bool {
    db::get_setting(conn, "keep_awake")
        .ok()
        .flatten()
        .as_deref()
        != Some("0")
}

/// Namenssuche als Rückfall: Standard an.
/// Adresse der Gemeinschaftsdaten: eingestellt oder die eingebaute.
pub(crate) fn community_url(state: &AppState) -> String {
    // Die frühere eingebaute Adresse (bis 0.6.3) zählt wie „keine Angabe“ und führt zur neuen.
    Some(setting(state, "community_url"))
        .filter(|u| !u.trim().is_empty() && u.trim() != share::LEGACY_COMMUNITY_URL)
        .unwrap_or_else(|| share::DEFAULT_COMMUNITY_URL.into())
}

/// Adresse für Beiträge an den Server der Gemeinschaftsdatenbank: eingestellt oder die eingebaute;
/// „-“ schaltet das Einsenden aus.
pub(crate) fn submit_url(state: &AppState) -> String {
    match setting(state, "community_submit_url").trim() {
        "" | share::LEGACY_SUBMIT_URL => share::DEFAULT_SUBMIT_URL.to_string(),
        "-" => String::new(),
        url => url.to_string(),
    }
}

/// Eingestellte Zahl gleichzeitiger Abfragen (0 = automatisch).
fn threads_setting(state: &AppState) -> i64 {
    setting(state, "screenscraper.threads")
        .parse()
        .unwrap_or(0)
        .clamp(0, MAX_THREADS)
}

fn name_fallback(state: &AppState) -> bool {
    setting(state, "screenscraper.name_fallback") != "0"
}

fn setting(state: &AppState, key: &str) -> String {
    db::get_setting(&state.conn.lock().unwrap(), key)
        .ok()
        .flatten()
        .unwrap_or_default()
}

fn language(state: &AppState) -> String {
    Some(setting(state, "screenscraper.language"))
        .filter(|l| !l.is_empty())
        .unwrap_or_else(|| "de".into())
}

#[tauri::command]
pub fn get_scraper_settings(state: State<AppState>) -> ScraperSettings {
    let user = setting(&state, "screenscraper.user");
    // Die Sperre je Wert für sich nehmen und freigeben: Ein Wächter in einem Ausdruck lebt bis zu
    // dessen Ende, eine zweite Sperre darin hängt sich auf.
    let (keep_awake, prefer_shared, update_check, community_auto, auto_title, auto_title_delay) = {
        let conn = state.conn.lock().unwrap();
        (
            keep_awake(&conn),
            share::prefer_shared(&conn).unwrap_or(true),
            flag_on(&conn, "update_check"),
            flag_on(&conn, "community_auto"),
            db::get_setting(&conn, "auto_title")
                .ok()
                .flatten()
                .as_deref()
                == Some("1"),
            auto_title_delay(&conn),
        )
    };
    ScraperSettings {
        has_password: !user.is_empty()
            && has_secret(
                &state,
                "screenscraper.has_password",
                &user_secret_name(&user),
            ),
        user,
        dev_builtin: builtin_dev().is_some(),
        dev_id: setting(&state, "screenscraper.devid"),
        has_dev_password: builtin_dev().is_none()
            && has_secret(&state, "screenscraper.has_devpassword", DEV_SECRET_NAME),
        language: language(&state),
        name_fallback: name_fallback(&state),
        threads: threads_setting(&state),
        keep_awake,
        preferred_source: if prefer_shared {
            "community"
        } else {
            "screenscraper"
        },
        community_url: community_url(&state),
        submit_url: submit_url(&state),
        default_community_url: share::DEFAULT_COMMUNITY_URL,
        default_submit_url: share::DEFAULT_SUBMIT_URL,
        update_check,
        community_auto,
        auto_title,
        auto_title_delay,
        softname: SOFTNAME,
    }
}

#[tauri::command]
pub fn save_scraper_settings(state: State<AppState>, input: ScraperSettingsInput) -> CmdResult<()> {
    let user = input.user.trim();
    let old_user = setting(&state, "screenscraper.user");
    let mut flags: Vec<(&str, &str)> = Vec::new();
    if let Some(pw) = input.password.as_deref().filter(|p| !p.is_empty()) {
        if user.is_empty() {
            return Err(tr!(
                "Bitte zuerst den Benutzernamen eintragen.",
                "Please enter the user name first."
            ));
        }
        secret_set(&user_secret_name(user), pw)?;
        flags.push(("screenscraper.has_password", "1"));
    } else if user != old_user {
        // Anderer Benutzer: beim nächsten Anzeigen neu nachsehen.
        flags.push(("screenscraper.has_password", ""));
    }
    if let Some(pw) = input.dev_password.as_deref().filter(|p| !p.is_empty()) {
        // Beim Kopieren aus der Entwicklerzone rutscht leicht ein Leerzeichen mit.
        secret_set(DEV_SECRET_NAME, pw.trim())?;
        flags.push(("screenscraper.has_devpassword", "1"));
    }
    let conn = state.conn.lock().unwrap();
    for (k, v) in flags {
        db::set_setting(&conn, k, v).map_err(err)?;
    }
    db::set_setting(&conn, "screenscraper.user", user).map_err(err)?;
    db::set_setting(&conn, "screenscraper.language", input.language.trim()).map_err(err)?;
    db::set_setting(
        &conn,
        "screenscraper.name_fallback",
        if input.name_fallback { "1" } else { "0" },
    )
    .map_err(err)?;
    if let Some(url) = input.community_url {
        let url = url.trim();
        // Leer oder die eingebaute Adresse: nichts speichern, damit spätere Fassungen ihre eigene nehmen.
        let keep = if url == share::DEFAULT_COMMUNITY_URL || url == share::LEGACY_COMMUNITY_URL {
            ""
        } else {
            url
        };
        db::set_setting(&conn, "community_url", keep).map_err(err)?;
    }
    if let Some(url) = input.submit_url {
        // Eingebaute Adresse nicht speichern (spätere Fassungen nehmen ihre eigene); leer = Einsenden aus.
        let url = url.trim();
        let keep = if url == share::DEFAULT_SUBMIT_URL || url == share::LEGACY_SUBMIT_URL {
            ""
        } else if url.is_empty() {
            "-"
        } else {
            url
        };
        db::set_setting(&conn, "community_submit_url", keep).map_err(err)?;
    }
    if let Some(n) = input.threads {
        db::set_setting(
            &conn,
            "screenscraper.threads",
            &n.clamp(0, MAX_THREADS).to_string(),
        )
        .map_err(err)?;
    }
    db::set_setting(
        &conn,
        "keep_awake",
        if input.keep_awake { "1" } else { "0" },
    )
    .map_err(err)?;
    for (key, on) in [
        ("update_check", input.update_check),
        ("community_auto", input.community_auto),
        ("auto_title", input.auto_title),
    ] {
        if let Some(on) = on {
            db::set_setting(&conn, key, if on { "1" } else { "0" }).map_err(err)?;
        }
    }
    if let Some(n) = input.auto_title_delay {
        db::set_setting(&conn, "auto_title_delay", &n.clamp(5, 180).to_string()).map_err(err)?;
    }
    if let Some(src) = input
        .preferred_source
        .as_deref()
        .filter(|s| matches!(*s, "community" | "screenscraper"))
    {
        db::set_setting(&conn, share::PREFERRED_SOURCE, src).map_err(err)?;
    }
    if let Some(id) = input.dev_id {
        db::set_setting(&conn, "screenscraper.devid", id.trim()).map_err(err)?;
    }
    Ok(())
}

fn credentials(state: &AppState) -> CmdResult<Credentials> {
    let (dev_id, dev_password) = match builtin_dev() {
        Some((id, pw)) => (id.to_string(), pw.to_string()),
        None => (
            setting(state, "screenscraper.devid"),
            secret_get(DEV_SECRET_NAME)
                .unwrap_or_default()
                .trim()
                .to_string(),
        ),
    };
    if dev_id.is_empty() || dev_password.is_empty() {
        return Err(tr!(
            "Für ScreenScraper fehlt die Entwickler-Kennung (Einstellungen).",
            "The ScreenScraper developer credentials are missing (Settings)."
        ));
    }
    let user = setting(state, "screenscraper.user");
    let password = if user.is_empty() {
        String::new()
    } else {
        secret_get(&user_secret_name(&user)).unwrap_or_default()
    };
    if user.is_empty() || password.is_empty() {
        return Err(tr!(
            "Bitte Benutzername und Passwort für ScreenScraper in den Einstellungen eintragen.",
            "Please enter your ScreenScraper user name and password in Settings."
        ));
    }
    Ok(Credentials {
        dev_id,
        dev_password,
        user,
        password,
    })
}

#[tauri::command]
pub async fn test_scraper(app: AppHandle) -> CmdResult<UserInfo> {
    let creds = credentials(&app.state::<AppState>())?;
    // Netzwerkzugriff nicht im Hauptthread der Oberfläche.
    tauri::async_runtime::spawn_blocking(move || {
        Client::new(creds, SOFTNAME).user_info().map_err(|e| match e.downcast_ref::<ApiError>() {
            // Die eingebaute Kennung kann der Nutzer nicht ändern: sagen, dass es am Paket liegt.
            Some(ApiError::DevLogin(_)) if builtin_dev().is_some() => tr!(
                "{e} – die im Programm eingebaute Entwickler-Kennung wird abgelehnt, nicht dein Konto. Bitte melden.",
                "{e} – the developer credentials built into the program are rejected, not your account. Please report this."
            ),
            _ => err(e),
        })
    })
    .await
    .map_err(err)?
}

#[derive(Clone, Serialize)]
struct ScrapeEvent {
    progress: metadata::Progress,
    finished: bool,
    error: Option<String>,
}

/// Startet den Abruf in einem eigenen Thread. Ereignisse: `scrape-progress`.
#[tauri::command]
pub fn start_scrape(
    app: AppHandle,
    state: State<AppState>,
    sha1s: Option<Vec<String>>,
    force: bool,
    retry_notfound: bool,
) -> CmdResult<()> {
    let creds = credentials(&state)?;
    let mut running = state.scrape_cancel.lock().unwrap();
    if running.is_some() {
        return Err(tr!(
            "Es läuft bereits ein Abruf.",
            "A fetch is already running."
        ));
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *running = Some(cancel.clone());
    let (db_path, media_dir, lang) = (
        state.db_path.clone(),
        state.media_dir.clone(),
        language(&state),
    );
    let name_fallback = name_fallback(&state);
    let wanted_threads = threads_setting(&state);
    let awake = keep_awake(&state.conn.lock().unwrap());

    std::thread::spawn(move || {
        let _awake = awake.then(|| breadbin_core::awake::KeepAwake::start("ScreenScraper-Abruf"));
        let emit = |p: &metadata::Progress, finished: bool, error: Option<String>| {
            let _ = app.emit(
                "scrape-progress",
                ScrapeEvent {
                    progress: p.clone(),
                    finished,
                    error,
                },
            );
        };
        let result = db::open(&db_path).and_then(|conn| {
            // So viele Threads, wie das Konto erlaubt (höchstens MAX_THREADS); ohne Auskunft einer.
            let info = Client::new(creds.clone(), SOFTNAME).user_info().ok();
            let allowed = info
                .as_ref()
                .map_or(1, |u| u.max_threads.clamp(1, MAX_THREADS));
            let threads = if wanted_threads > 0 {
                wanted_threads.min(allowed)
            } else {
                allowed
            } as usize;
            let per_min = info.as_ref().map_or(0, |u| u.max_requests_per_min);
            let mut clients = Client::pool(&creds, SOFTNAME, threads, per_min);
            let sources: Vec<&mut dyn metadata::GameSource> = clients
                .iter_mut()
                .map(|c| c as &mut dyn metadata::GameSource)
                .collect();
            let opts = metadata::Options {
                sha1s,
                force,
                retry_notfound,
                lang: &lang,
                media_dir: &media_dir,
                name_fallback,
            };
            let mut last = Instant::now() - Duration::from_secs(1);
            metadata::run_with(&conn, sources, &opts, &cancel, &mut |p| {
                if last.elapsed() >= Duration::from_millis(200) {
                    last = Instant::now();
                    emit(p, false, None);
                }
            })
        });
        *app.state::<AppState>().scrape_cancel.lock().unwrap() = None;
        match result {
            Ok(p) => emit(&p, true, None),
            Err(e) => emit(&metadata::Progress::default(), true, Some(err(e))),
        }
    });
    Ok(())
}

#[tauri::command]
pub fn cancel_scrape(state: State<AppState>) {
    if let Some(c) = state.scrape_cancel.lock().unwrap().as_ref() {
        c.store(true, Ordering::Relaxed);
    }
}

/// Bild aus dem Bilderordner als data-URL (die Oberfläche darf keine Dateien lesen).
#[tauri::command]
pub fn media_data(state: State<AppState>, file: String) -> CmdResult<String> {
    if file.contains(['/', '\\']) || file.starts_with('.') {
        return Err(tr!("ungültiger Dateiname", "invalid file name"));
    }
    let data = std::fs::read(state.media_dir.join(&file)).map_err(err)?;
    let mime = match file.rsplit('.').next() {
        Some("jpg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/png",
    };
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(data)
    ))
}

/// Ein Treffer der Spielsuche für die Auswahl von Hand.
#[derive(Serialize)]
pub struct GameHit {
    id: String,
    title: Option<String>,
    year: Option<String>,
    publisher: Option<String>,
    genres: String,
    synopsis: Option<String>,
}

impl From<Game> for GameHit {
    fn from(g: Game) -> Self {
        // Für die Liste reicht der Anfang der Beschreibung.
        let synopsis = g.synopsis.map(|s| {
            let short: String = s.chars().take(220).collect();
            if short.len() < s.len() {
                format!("{}…", short.trim_end())
            } else {
                short
            }
        });
        GameHit {
            id: g.id,
            title: g.title,
            year: g.year,
            publisher: g.publisher,
            genres: g.genres.join(", "),
            synopsis,
        }
    }
}

/// Sucht Spiele bei ScreenScraper; nur Ziffern = Spielnummer.
#[tauri::command]
pub async fn search_games(app: AppHandle, query: String) -> CmdResult<Vec<GameHit>> {
    let state = app.state::<AppState>();
    let (creds, lang) = (credentials(&state)?, language(&state));
    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = Client::new(creds, SOFTNAME);
        let games = if query.chars().all(|c| c.is_ascii_digit()) {
            match client.game_by_id(&query, &lang).map_err(err)? {
                Lookup::Found(g) => vec![*g],
                Lookup::NotFound => Vec::new(),
            }
        } else {
            client.search_name(&query, &lang).map_err(err)?
        };
        Ok(games.into_iter().map(GameHit::from).collect())
    })
    .await
    .map_err(err)?
}

/// Ordnet die Inhalte dem gewählten Spiel zu (Angaben und Bilder neu von ScreenScraper).
/// Gibt einen Hinweis zurück, wenn die Bilder nicht geladen werden konnten.
#[tauri::command]
pub async fn assign_game(
    app: AppHandle,
    sha1s: Vec<String>,
    game_id: String,
) -> CmdResult<Option<String>> {
    let state = app.state::<AppState>();
    let (creds, lang) = (credentials(&state)?, language(&state));
    let (db_path, media_dir) = (state.db_path.clone(), state.media_dir.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let mut client = Client::new(creds, SOFTNAME);
        let game = match client.game_by_id(&game_id, &lang).map_err(err)? {
            Lookup::Found(g) => *g,
            Lookup::NotFound => {
                return Err(tr!(
                    "Spiel {game_id} bei ScreenScraper nicht gefunden.",
                    "Game {game_id} not found on ScreenScraper."
                ));
            }
        };
        let conn = db::open(&db_path).map_err(err)?;
        metadata::assign(&conn, &mut client, &sha1s, &game, &media_dir).map_err(err)
    })
    .await
    .map_err(err)?
}

/// Zuordnung als falsch markieren (`on`) oder das zurücknehmen.
#[tauri::command]
pub fn reject_metadata(state: State<AppState>, sha1s: Vec<String>, on: bool) -> CmdResult<()> {
    let conn = state.conn.lock().unwrap();
    if on {
        metadata::reject(&conn, &sha1s)
    } else {
        metadata::unreject(&conn, &sha1s)
    }
    .map_err(err)
}

/// Eigene Angaben speichern.
#[tauri::command]
/// `fresh`: „Eigene Fassung schreiben“ (leerer Dialog) – auch bei Spielen von ScreenScraper teilbar.
pub fn save_own_meta(
    state: State<AppState>,
    sha1s: Vec<String>,
    meta: metadata::OwnMeta,
    fresh: Option<bool>,
) -> CmdResult<()> {
    metadata::save_own(
        &state.conn.lock().unwrap(),
        &sha1s,
        &meta,
        fresh.unwrap_or(false),
    )
    .map_err(err)
}

/// Angaben und Bilder löschen (abgerufen oder selbst eingetragen).
#[tauri::command]
pub fn clear_meta(state: State<AppState>, sha1s: Vec<String>) -> CmdResult<()> {
    metadata::clear(&state.conn.lock().unwrap(), &sha1s).map_err(err)
}

/// Eigenes Bild aus einer vom Nutzer gewählten Datei setzen; ohne `path` entfernen.
#[tauri::command]
pub fn set_own_image(
    state: State<AppState>,
    sha1s: Vec<String>,
    kind: String,
    path: Option<String>,
) -> CmdResult<()> {
    let image = match &path {
        Some(p) => {
            let p = std::path::Path::new(p);
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_string();
            Some((std::fs::read(p).map_err(err)?, ext))
        }
        None => None,
    };
    let conn = state.conn.lock().unwrap();
    metadata::set_own_image(
        &conn,
        &sha1s,
        &kind,
        image.as_ref().map(|(d, e)| (d.as_slice(), e.as_str())),
        &state.media_dir,
    )
    .map_err(err)
}

/// Einträge mit Angaben in der eigenen Bibliothek (eigene, Gemeinschaft, ScreenScraper) zum Zuordnen.
#[tauri::command]
pub fn search_library(
    state: State<AppState>,
    query: String,
    exclude: Vec<String>,
) -> CmdResult<Vec<metadata::LibraryHit>> {
    metadata::search_library(&state.conn.lock().unwrap(), &query, &exclude, 30).map_err(err)
}

/// Angaben und Bilder eines Eintrags der Bibliothek auf andere Fassungen übertragen.
#[tauri::command]
pub fn copy_meta(state: State<AppState>, from: String, to: Vec<String>) -> CmdResult<()> {
    metadata::copy_meta(&state.conn.lock().unwrap(), &from, &to).map_err(err)
}

/// Bild einer anderen Bildart zuordnen; liegt dort schon eins, tauschen beide.
#[tauri::command]
pub fn move_image(
    state: State<AppState>,
    sha1s: Vec<String>,
    from: String,
    to: String,
) -> CmdResult<()> {
    metadata::move_image(&state.conn.lock().unwrap(), &sha1s, &from, &to).map_err(err)
}

/// Angaben eines Spiels bei ScreenScraper, wie sie dort gerade stehen (für den Vergleich beim Vorschlagen).
#[derive(Serialize)]
pub struct GameNow {
    title: Option<String>,
    year: Option<String>,
    publisher: Option<String>,
    developer: Option<String>,
    synopsis: Option<String>,
    genres: Vec<String>,
    players: Option<String>,
    media: Vec<String>,
}

#[tauri::command]
pub async fn game_now(app: AppHandle, game_id: String) -> CmdResult<GameNow> {
    let state = app.state::<AppState>();
    let (creds, lang) = (credentials(&state)?, language(&state));
    tauri::async_runtime::spawn_blocking(move || {
        match Client::new(creds, SOFTNAME)
            .game_by_id(&game_id, &lang)
            .map_err(err)?
        {
            Lookup::Found(g) => {
                let mut media: Vec<String> = g.media.iter().map(|m| m.kind.clone()).collect();
                media.dedup();
                Ok(GameNow {
                    title: g.title,
                    year: g.year,
                    publisher: g.publisher,
                    developer: g.developer,
                    synopsis: g.synopsis,
                    genres: g.genres,
                    players: g.players,
                    media,
                })
            }
            Lookup::NotFound => Err(tr!(
                "Spiel {game_id} bei ScreenScraper nicht gefunden.",
                "Game {game_id} not found on ScreenScraper."
            )),
        }
    })
    .await
    .map_err(err)?
}

/// Auswahllisten für Vorschläge; selten geändert, darum in den Einstellungen zwischengespeichert.
#[derive(Serialize, Deserialize)]
pub struct ProposalChoices {
    fetched: i64,
    genres: Vec<Choice>,
    players: Vec<Choice>,
}

const CHOICES_KEY: &str = "screenscraper.choices";
const CHOICES_MAX_AGE: i64 = 30 * 24 * 3600;

#[tauri::command]
pub async fn proposal_choices(app: AppHandle) -> CmdResult<ProposalChoices> {
    let state = app.state::<AppState>();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let cached: Option<ProposalChoices> = serde_json::from_str(&setting(&state, CHOICES_KEY)).ok();
    if let Some(c) = cached.filter(|c| {
        now - c.fetched < CHOICES_MAX_AGE && !c.genres.is_empty() && !c.players.is_empty()
    }) {
        return Ok(c);
    }
    let creds = credentials(&state)?;
    let fresh = tauri::async_runtime::spawn_blocking(move || -> CmdResult<ProposalChoices> {
        let mut client = Client::new(creds, SOFTNAME);
        let genres = client.genre_list().map_err(err)?;
        let players = client.player_list().map_err(err)?;
        Ok(ProposalChoices {
            fetched: now,
            genres,
            players,
        })
    })
    .await
    .map_err(err)??;
    if let Ok(json) = serde_json::to_string(&fresh) {
        let _ = db::set_setting(&state.conn.lock().unwrap(), CHOICES_KEY, &json);
    }
    Ok(fresh)
}

#[derive(Serialize)]
pub struct ProposalResult {
    typ: String,
    ok: bool,
    text: String,
}

/// Schickt die Vorschläge nacheinander und protokolliert jede Antwort. Bei Bildern ist
/// `file_name` eine Datei im Bilderordner; sie wird ins verlangte Format gewandelt.
#[tauri::command]
pub async fn propose(app: AppHandle, items: Vec<Proposal>) -> CmdResult<Vec<ProposalResult>> {
    let state = app.state::<AppState>();
    let creds = credentials(&state)?;
    let (db_path, media_dir) = (state.db_path.clone(), state.media_dir.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let conn = db::open(&db_path).map_err(err)?;
        let mut client = Client::new(creds, SOFTNAME);
        let mut out = Vec::new();
        for mut item in items {
            let (game_id, typ) = match &item {
                Proposal::Info { game_id, typ, .. } | Proposal::Media { game_id, typ, .. } => {
                    (game_id.clone(), typ.clone())
                }
            };
            let sent = (|| {
                if let Proposal::Media {
                    typ,
                    data,
                    file_name,
                    ..
                } = &mut item
                {
                    let (bytes, name) = metadata::image_for_proposal(&media_dir, file_name, typ)?;
                    *data = bytes;
                    *file_name = name;
                }
                client.propose(&item)
            })();
            let (ok, text) = match sent {
                Ok(t) => (true, t),
                Err(e) => (false, format!("{e:#}")),
            };
            let _ = db::log_proposal(&conn, &game_id, &typ, ok, &text);
            out.push(ProposalResult { typ, ok, text });
        }
        Ok(out)
    })
    .await
    .map_err(err)?
}

/// Bisher geschickte Vorschläge zu einem Spiel.
#[tauri::command]
pub fn proposals(state: State<AppState>, game_id: String) -> CmdResult<Vec<db::ProposalLog>> {
    db::proposals(&state.conn.lock().unwrap(), &game_id).map_err(err)
}
