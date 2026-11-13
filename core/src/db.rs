//! SQLite-Datenbank: Quellen, gefundene Dateien und deren Inhalte.
//!
//! Inhalte (`contents`) sind über die SHA-1-Prüfsumme eindeutig. Dieselbe Datei an
//! mehreren Orten ergibt mehrere Zeilen in `files`, aber nur einen Inhalt.

use crate::tr;
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::path::Path;

use crate::formats::DirEntry;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS sources(
    id        INTEGER PRIMARY KEY,
    kind      TEXT NOT NULL,            -- 'local' (später 'ultimate')
    label     TEXT NOT NULL,
    location  TEXT NOT NULL UNIQUE,     -- Ordnerpfad bzw. Geräteadresse
    last_scan INTEGER,
    quick     INTEGER NOT NULL DEFAULT 0 -- Schnellscan: Bekanntes aus anderen Quellen übernehmen
);
CREATE TABLE IF NOT EXISTS contents(
    sha1       TEXT PRIMARY KEY,
    md5        TEXT NOT NULL,
    crc32      TEXT NOT NULL,
    size       INTEGER NOT NULL,
    format     TEXT NOT NULL,
    image_name TEXT,
    detail     TEXT
);
CREATE TABLE IF NOT EXISTS entries(
    sha1   TEXT NOT NULL REFERENCES contents(sha1) ON DELETE CASCADE,
    idx    INTEGER NOT NULL,
    name   TEXT NOT NULL,
    kind   TEXT NOT NULL,
    blocks INTEGER NOT NULL,
    PRIMARY KEY(sha1, idx)
);
CREATE TABLE IF NOT EXISTS files(
    id        INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    path      TEXT NOT NULL,            -- relativ zur Quelle, mit '/'
    member    TEXT NOT NULL DEFAULT '', -- Name im ZIP-Archiv, sonst leer
    fsize     INTEGER NOT NULL,         -- Größe und Zeit der Datei auf dem Datenträger
    fmtime    INTEGER NOT NULL,
    sha1      TEXT NOT NULL REFERENCES contents(sha1),
    title     TEXT NOT NULL,
    year      TEXT,
    publisher TEXT,
    seen      INTEGER NOT NULL,         -- Kennung des letzten Scans, der die Datei sah
    quick     INTEGER NOT NULL DEFAULT 0, -- 1 = per Schnellscan übernommen, nicht selbst gelesen
    UNIQUE(source_id, path, member)
);
CREATE INDEX IF NOT EXISTS files_sha1 ON files(sha1);
-- Angaben von Online-Datenbanken, je Inhalt; bleiben erhalten, auch wenn die Datei
-- vorübergehend fehlt (spart Abfragen beim nächsten Scan).
CREATE TABLE IF NOT EXISTS metadata(
    sha1       TEXT PRIMARY KEY,
    provider   TEXT NOT NULL,          -- 'screenscraper'
    status     TEXT NOT NULL,          -- 'ok', 'notfound' oder 'rejected' (vom Nutzer als falsch markiert)
    game_id    TEXT,
    title      TEXT,
    synopsis   TEXT,
    year       TEXT,
    publisher  TEXT,
    developer  TEXT,
    genres     TEXT,                   -- durch ', ' getrennt
    players    TEXT,
    rating     REAL,                   -- 0–20
    fetched_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS media(
    sha1 TEXT NOT NULL,
    kind TEXT NOT NULL,                -- 'ss', 'sstitle', 'box-2D'
    file TEXT NOT NULL,                -- Dateiname im Bilderordner der App
    PRIMARY KEY(sha1, kind)
);
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS favorites(sha1 TEXT PRIMARY KEY, added INTEGER NOT NULL);
-- Eigene Listen („Partyspiele“) und ihre Einträge (je Fassung).
CREATE TABLE IF NOT EXISTS lists(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE);
CREATE TABLE IF NOT EXISTS list_items(
    list_id INTEGER NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    sha1    TEXT NOT NULL,
    added   INTEGER NOT NULL,
    PRIMARY KEY(list_id, sha1)
);
-- Eigene Bewertung (0 = keine, 1–5), „durchgespielt“ und Notiz je Fassung.
CREATE TABLE IF NOT EXISTS notes(
    sha1    TEXT PRIMARY KEY,
    rating  INTEGER NOT NULL DEFAULT 0,
    done    INTEGER NOT NULL DEFAULT 0,
    note    TEXT NOT NULL DEFAULT '',
    changed INTEGER NOT NULL
);
-- Vom Nutzer aus einer Gruppe gelöste Fassungen: stehen beim Zusammenfassen allein.
CREATE TABLE IF NOT EXISTS ungrouped(sha1 TEXT PRIMARY KEY);
-- Was zuletzt an die Gemeinschaftsdatenbank eingesandt wurde: Fingerabdruck aus Angaben und Bildern.
CREATE TABLE IF NOT EXISTS submitted(sha1 TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, at INTEGER NOT NULL);
-- An ScreenScraper geschickte Vorschläge mit Antwort.
CREATE TABLE IF NOT EXISTS proposals(
    id       INTEGER PRIMARY KEY,
    game_id  TEXT NOT NULL,
    typ      TEXT NOT NULL,              -- modiftypeinfo bzw. modiftypemedia
    ok       INTEGER NOT NULL,
    response TEXT NOT NULL,
    sent_at  INTEGER NOT NULL
);
-- Starts auf Ultimate-Geräten, für „Zuletzt gestartet“.
CREATE TABLE IF NOT EXISTS launches(
    id     INTEGER PRIMARY KEY,
    sha1   TEXT NOT NULL,
    device TEXT NOT NULL,
    at     INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS launches_sha1 ON launches(sha1);
-- Ultimate-Geräte, auf denen gestartet werden kann.
CREATE TABLE IF NOT EXISTS devices(
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    host       TEXT NOT NULL UNIQUE,
    product    TEXT NOT NULL,
    upload_dir TEXT NOT NULL,        -- Ordner für Dateien, die zum Starten hinkopiert werden
    wifi_mac   TEXT NOT NULL DEFAULT '' -- zum Einschalten per Wake-on-WiFi
);
";

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path).with_context(|| {
        tr!(
            "Datenbank {} lässt sich nicht öffnen",
            "Cannot open database {}",
            path.display()
        )
    })?;
    init(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    init(&conn)?;
    Ok(conn)
}

fn init(conn: &Connection) -> Result<()> {
    // busy_timeout: Scan, Abruf und Oberfläche schreiben über eigene Verbindungen.
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=10000;",
    )?;
    conn.execute_batch(SCHEMA)?;
    migrate(conn)?;
    Ok(())
}

/// Spalten, die nach dem ersten Stand dazugekommen sind, in ältere Datenbanken einbauen.
fn migrate(conn: &Connection) -> Result<()> {
    let has_column = |table: &str, column: &str| -> Result<bool> {
        let mut st = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let names: Vec<String> = st.query_map([], |r| r.get(1))?.collect::<Result<_, _>>()?;
        Ok(names.iter().any(|n| n == column))
    };
    if !has_column("devices", "wifi_mac")? {
        conn.execute_batch("ALTER TABLE devices ADD COLUMN wifi_mac TEXT NOT NULL DEFAULT ''")?;
    }
    if !has_column("sources", "quick")? {
        conn.execute_batch("ALTER TABLE sources ADD COLUMN quick INTEGER NOT NULL DEFAULT 0")?;
    }
    // Englische Beschreibung zu eigenen und Gemeinschaftsangaben (`synopsis` = deutsch bzw. bisheriger Text).
    if !has_column("metadata", "synopsis_en")? {
        conn.execute_batch("ALTER TABLE metadata ADD COLUMN synopsis_en TEXT")?;
    }
    if !has_column("files", "quick")? {
        conn.execute_batch("ALTER TABLE files ADD COLUMN quick INTEGER NOT NULL DEFAULT 0")?;
    }
    // Ordner eines Fundorts (Quelle + Verzeichnis, bei ZIP-Inhalten das Archiv) für Diskettenreihen;
    // ein Trigger hält ihn aktuell, damit der Scanner nichts davon wissen muss.
    if !has_column("files", "folder")? {
        conn.execute_batch(
            "ALTER TABLE files ADD COLUMN folder TEXT NOT NULL DEFAULT '';
             UPDATE files SET folder = CASE WHEN member <> '' THEN source_id || ':' || path || '/' ELSE source_id || ':' || rtrim(path, replace(path, '/', '')) END;",
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS files_folder ON files(folder);
         CREATE INDEX IF NOT EXISTS metadata_game ON metadata(game_id);
         CREATE TRIGGER IF NOT EXISTS files_folder_ins AFTER INSERT ON files BEGIN
             UPDATE files SET folder = CASE WHEN NEW.member <> '' THEN NEW.source_id || ':' || NEW.path || '/' ELSE NEW.source_id || ':' || rtrim(NEW.path, replace(NEW.path, '/', '')) END WHERE id = NEW.id;
         END;
         CREATE TRIGGER IF NOT EXISTS files_folder_upd AFTER UPDATE OF path, member, source_id ON files BEGIN
             UPDATE files SET folder = CASE WHEN NEW.member <> '' THEN NEW.source_id || ':' || NEW.path || '/' ELSE NEW.source_id || ':' || rtrim(NEW.path, replace(NEW.path, '/', '')) END WHERE id = NEW.id;
         END;",
    )?;
    // Eigene Angaben zu Spielen mit ScreenScraper-Nummer stammen aus der Zeit, als der Dialog die Angaben
    // von ScreenScraper vorbelegte, ohne das zu vermerken: vorsichtshalber als „bearbeitet“ (nicht teilbar).
    if get_setting(conn, "migr_own_edited")?.is_none() {
        conn.execute("UPDATE metadata SET provider = 'own:edited' WHERE provider = 'own' AND game_id IS NOT NULL", [])?;
        set_setting(conn, "migr_own_edited", "1")?;
    }
    Ok(())
}

// ---------------------------------------------------------------- Quellen

#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub id: i64,
    pub kind: String,
    pub label: String,
    pub location: String,
    pub last_scan: Option<i64>,
    pub files: i64,
    /// Schnellscan: Dateien mit gleichem Namen und gleicher Größe wie in einer anderen Quelle
    /// werden übernommen statt gelesen.
    pub quick: bool,
}

pub fn list_sources(conn: &Connection) -> Result<Vec<Source>> {
    let mut st = conn.prepare(
        "SELECT s.id, s.kind, s.label, s.location, s.last_scan,
                (SELECT COUNT(*) FROM files f WHERE f.source_id = s.id), s.quick
         FROM sources s ORDER BY s.label COLLATE NOCASE",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Source {
            id: r.get(0)?,
            kind: r.get(1)?,
            label: r.get(2)?,
            location: r.get(3)?,
            last_scan: r.get(4)?,
            files: r.get(5)?,
            quick: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get_source(conn: &Connection, id: i64) -> Result<Source> {
    list_sources(conn)?
        .into_iter()
        .find(|s| s.id == id)
        .with_context(|| tr!("Quelle {id} gibt es nicht", "Source {id} does not exist"))
}

pub fn add_local_source(conn: &Connection, folder: &Path) -> Result<i64> {
    if !folder.is_dir() {
        bail!(tr!(
            "{} ist kein erreichbarer Ordner",
            "{} is not a reachable folder",
            folder.display()
        ));
    }
    let location = folder.to_string_lossy().to_string();
    let label = folder
        .file_name()
        .map_or_else(|| location.clone(), |n| n.to_string_lossy().to_string());
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM sources WHERE location = ?",
            [&location],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO sources(kind, label, location) VALUES('local', ?, ?)",
        params![label, location],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Ordner auf einem Ultimate-Gerät als Quelle, `path` z. B. `/USB0/Spiele`.
pub fn add_ultimate_source(conn: &Connection, host: &str, path: &str, label: &str) -> Result<i64> {
    let path = format!("/{}", path.trim().trim_matches('/'));
    let location = format!("ftp://{}{}", host.trim(), path);
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM sources WHERE location = ?",
            [&location],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO sources(kind, label, location) VALUES('ultimate', ?, ?)",
        params![format!("{label}: {path}"), location],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Schnellscan für eine Quelle ein- oder ausschalten. Ausgeschaltet liest der nächste Scan
/// die übernommenen Dateien doch noch selbst.
pub fn set_source_quick(conn: &Connection, id: i64, quick: bool) -> Result<()> {
    conn.execute(
        "UPDATE sources SET quick = ? WHERE id = ?",
        params![quick, id],
    )?;
    Ok(())
}

pub fn remove_source(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM files WHERE source_id = ?", [id])?;
    conn.execute("DELETE FROM sources WHERE id = ?", [id])?;
    remove_orphans(conn)?;
    Ok(())
}

/// Inhalte löschen, auf die keine Datei mehr verweist.
pub fn remove_orphans(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM contents WHERE sha1 NOT IN (SELECT sha1 FROM files)",
        [],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- Liste und Details

#[derive(Debug, Clone, Serialize)]
pub struct GameRow {
    pub sha1: String,
    pub title: String,
    pub format: String,
    pub size: i64,
    pub image_name: Option<String>,
    pub detail: Option<String>,
    pub year: Option<String>,
    pub publisher: Option<String>,
    pub locations: i64,
    pub genres: Option<String>,
    /// 'ok', 'notfound', 'rejected' oder `None`, wenn noch nicht nachgeschlagen.
    pub meta_status: Option<String>,
    pub favorite: bool,
    /// Zeitpunkt des letzten Starts (Unix-Sekunden).
    pub last_launched: Option<i64>,
    /// Spielnummer bei ScreenScraper, wenn bekannt.
    pub game_id: Option<String>,
    /// Beim Zusammenfassen: alle Inhalte dieses Spiels, dieser zuerst; sonst nur dieser.
    pub variants: Vec<String>,
    /// Beim Zusammenfassen: die Formate aller Fassungen, z. B. „D64, CRT“; sonst das Format.
    pub formats: String,
    /// Vom Nutzer aus der Gruppe gelöst.
    pub ungrouped: bool,
    /// Disk-/Seitenkennung aus dem Dateinamen („1“, „2“, „A“, „1/B“), sonst `None`.
    pub disk: Option<String>,
    /// Kennung der Diskettenreihe (Format + bereinigter Name): gleiche Reihe, andere Seite.
    pub disk_set: Option<String>,
    /// Beim Zusammenfassen: Zahl der Disks/Seiten der größten Reihe, sonst 0.
    pub disks: i64,
    /// Eigene Bewertung 0–5 (beim Zusammenfassen die höchste der Fassungen).
    pub rating: i64,
    /// Als durchgespielt markiert (eine Fassung genügt).
    pub done: bool,
    /// Name aus dem Dateinamen und Ordner (Quelle + Pfad, ZIP zählt als Ordner) – für Diskettenreihen.
    #[serde(skip)]
    file_title: String,
    #[serde(skip)]
    folder: String,
}

const ROW_SELECT: &str =
    "SELECT c.sha1, COALESCE(m.title, MIN(f.title)), c.format, c.size, c.image_name, c.detail,
                COALESCE(m.year, MIN(f.year)), COALESCE(m.publisher, MIN(f.publisher)), COUNT(*),
                m.genres, m.status,
                EXISTS(SELECT 1 FROM favorites fv WHERE fv.sha1 = c.sha1),
                (SELECT MAX(l.at) FROM launches l WHERE l.sha1 = c.sha1),
                CASE WHEN m.status = 'ok' THEN m.game_id END,
                EXISTS(SELECT 1 FROM ungrouped u WHERE u.sha1 = c.sha1),
                MIN(f.title),
                MIN(f.folder),
                (SELECT n.rating FROM notes n WHERE n.sha1 = c.sha1),
                (SELECT n.done FROM notes n WHERE n.sha1 = c.sha1)
         FROM files f JOIN contents c ON c.sha1 = f.sha1 LEFT JOIN metadata m ON m.sha1 = c.sha1";

fn game_row(r: &rusqlite::Row) -> rusqlite::Result<GameRow> {
    let sha1: String = r.get(0)?;
    let format: String = r.get(2)?;
    // Die Seite steht im Dateinamen, nicht im Titel von ScreenScraper.
    let file_title: String = r.get(15)?;
    let disk = crate::screenscraper::disk_side(&file_title);
    let disk_set = disk.as_ref().map(|_| {
        format!(
            "{format}:{}",
            crate::screenscraper::normalize_title(&crate::screenscraper::clean_title(&file_title))
        )
    });
    Ok(GameRow {
        variants: vec![sha1.clone()],
        formats: format.clone(),
        sha1,
        title: r.get(1)?,
        format,
        size: r.get(3)?,
        image_name: r.get(4)?,
        detail: r.get(5)?,
        year: r.get(6)?,
        publisher: r.get(7)?,
        locations: r.get(8)?,
        genres: r.get(9)?,
        meta_status: r.get(10)?,
        favorite: r.get(11)?,
        last_launched: r.get(12)?,
        game_id: r.get(13)?,
        ungrouped: r.get(14)?,
        disk,
        disk_set,
        disks: 0,
        rating: r.get::<_, Option<i64>>(17)?.unwrap_or(0),
        done: r.get::<_, Option<i64>>(18)?.unwrap_or(0) != 0,
        file_title,
        folder: r.get(16)?,
    })
}

/// Such- und Filtereinstellungen der Liste.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct Search {
    pub query: String,
    pub format: Option<String>,
    /// Nur Inhalte, die (auch) in dieser Quelle liegen.
    pub source_id: Option<i64>,
    /// Nur Einträge mit Beschreibung.
    pub only_meta: bool,
    /// Nur Inhalte, die an mehreren Orten liegen.
    pub only_duplicates: bool,
    /// Nur Favoriten.
    pub only_favorites: bool,
    /// Nur schon gestartete, der zuletzt gestartete zuerst (ersetzt die Sortierung).
    pub recent: bool,
    /// Nur selbst geschriebene Angaben (die geteilt werden können).
    pub shareable: bool,
    /// Nur Einträge dieser eigenen Liste.
    pub list_id: Option<i64>,
    /// Nur Abbilder, die beim Scan als beschädigt bzw. unbekannt aufgebaut erkannt wurden.
    pub only_damaged: bool,
    /// Nur Einträge ohne Angaben (nicht erkannt oder noch nicht nachgeschlagen).
    pub without_meta: bool,
    /// Spalte: title, year, publisher, genre, format, locations (sonst title).
    pub sort: String,
    pub desc: bool,
    pub limit: i64,
    /// Verschiedene Dateien desselben Spiels zu einer Zeile zusammenfassen.
    pub group: bool,
}

/// Sucht in Titel, Disk-/Modulname, Pfad, Angaben von ScreenScraper und den Dateinamen im
/// Verzeichnis eines Abbilds.
pub fn search(
    conn: &Connection,
    query: &str,
    format: Option<&str>,
    limit: i64,
) -> Result<Vec<GameRow>> {
    search_with(
        conn,
        &Search {
            query: query.into(),
            format: format.map(str::to_string),
            limit,
            ..Default::default()
        },
    )
}

pub fn search_with(conn: &Connection, s: &Search) -> Result<Vec<GameRow>> {
    // % und _ sind Platzhalter in LIKE: wörtlich suchen.
    let escaped = s
        .query
        .trim()
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let pattern = format!("%{escaped}%");
    // Ohne Leerzeichen verglichen: „spacegun“ findet „Space Gun“ und umgekehrt.
    let squeezed = format!("%{}%", escaped.replace(' ', ""));
    // Ausdrücke statt Spaltennummern: „7 IS NULL“ wäre in SQLite die Zahl 7, nicht Spalte 7.
    let sort = match s.sort.as_str() {
        "year" => "COALESCE(m.year, MIN(f.year))",
        "publisher" => "COALESCE(m.publisher, MIN(f.publisher))",
        "genre" => "m.genres",
        "format" => "c.format",
        "locations" => "COUNT(*)",
        _ => "COALESCE(m.title, MIN(f.title))",
    };
    let (sort, dir) = if s.recent {
        (
            "-(SELECT MAX(l.at) FROM launches l WHERE l.sha1 = c.sha1)",
            "ASC",
        )
    } else {
        (sort, if s.desc { "DESC" } else { "ASC" })
    };
    let mut st = conn.prepare(&format!(
        "{ROW_SELECT}
         WHERE (?1 = '%%' OR f.title LIKE ?1 ESCAPE '\\' OR c.image_name LIKE ?1 ESCAPE '\\'
                OR f.path LIKE ?1 ESCAPE '\\' OR m.title LIKE ?1 ESCAPE '\\'
                OR replace(f.title, ' ', '') LIKE ?9 ESCAPE '\\' OR replace(m.title, ' ', '') LIKE ?9 ESCAPE '\\'
                OR m.genres LIKE ?1 ESCAPE '\\' OR m.publisher LIKE ?1 ESCAPE '\\'
                OR EXISTS(SELECT 1 FROM entries e WHERE e.sha1 = c.sha1 AND e.name LIKE ?1 ESCAPE '\\'))
           AND (?2 IS NULL OR c.format = ?2)
           AND (?3 IS NULL OR EXISTS(SELECT 1 FROM files f3 WHERE f3.sha1 = c.sha1 AND f3.source_id = ?3))
           AND (?4 = 0 OR m.status = 'ok')
           AND (?7 = 0 OR EXISTS(SELECT 1 FROM favorites fv WHERE fv.sha1 = c.sha1))
           AND (?8 = 0 OR EXISTS(SELECT 1 FROM launches l WHERE l.sha1 = c.sha1))
           AND (?10 = 0 OR (m.provider = 'own' AND m.status = 'ok'))
           AND (?11 IS NULL OR EXISTS(SELECT 1 FROM list_items li WHERE li.sha1 = c.sha1 AND li.list_id = ?11))
           AND (?12 = 0 OR c.detail LIKE '%beschädigt%')
           AND (?13 = 0 OR m.status IS NULL OR m.status <> 'ok')
         GROUP BY c.sha1
         HAVING (?5 = 0 OR COUNT(*) > 1)
         ORDER BY ({sort} IS NULL OR {sort} = ''), {sort} COLLATE NOCASE {dir}, 2 COLLATE NOCASE, c.format
         LIMIT ?6"
    ))?;
    let limit = if s.limit > 0 { s.limit } else { 1000 };
    // Zusammengefasst wird erst nach dem Lesen; daher hier ohne Obergrenze.
    let sql_limit = if s.group { i64::MAX } else { limit };
    let rows = st.query_map(
        params![
            pattern,
            s.format,
            s.source_id,
            s.only_meta as i64,
            s.only_duplicates as i64,
            sql_limit,
            s.only_favorites as i64,
            s.recent as i64,
            squeezed,
            s.shareable as i64,
            s.list_id,
            s.only_damaged as i64,
            s.without_meta as i64
        ],
        game_row,
    )?;
    let rows: Vec<GameRow> = rows.collect::<Result<_, _>>()?;
    if !s.group {
        return Ok(rows);
    }
    let mut rows = rows;
    // Gefiltert (Suchtext, nur mit Beschreibung …): die übrigen Fassungen und Disks gefundener Spiele
    // dazuholen – sonst fehlt etwa SUMMER1A, wenn nach „Summer Games“ gesucht wird. Format- und
    // Quellenfilter bleiben streng.
    let filtered = !s.query.trim().is_empty()
        || s.only_meta
        || s.only_duplicates
        || s.only_favorites
        || s.recent
        || s.shareable
        || s.list_id.is_some()
        || s.only_damaged;
    let hits: Option<std::collections::HashSet<String>> =
        (filtered && s.format.is_none() && s.source_id.is_none())
            .then(|| rows.iter().map(|r| r.sha1.clone()).collect());
    if let Some(hits) = &hits {
        rows.extend(
            group_members(conn, &rows)?
                .into_iter()
                .filter(|r| !hits.contains(&r.sha1)),
        );
    }
    mark_folder_sets(conn, &mut rows)?;
    let mut groups = group_rows(rows);
    if let Some(hits) = &hits {
        groups.retain(|g| g.variants.iter().any(|v| hits.contains(v)));
    }
    groups.truncate(limit.try_into().unwrap_or(usize::MAX));
    Ok(groups)
}

/// Alle Inhalte mit derselben ScreenScraper-Spielnummer oder aus denselben Ordnern wie `rows`
/// (Kandidaten fürs Zusammenfassen).
fn group_members(conn: &Connection, rows: &[GameRow]) -> Result<Vec<GameRow>> {
    let mut ids: Vec<&str> = rows.iter().filter_map(|r| r.game_id.as_deref()).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut folders: Vec<&str> = rows
        .iter()
        .filter(|r| matches!(r.format.as_str(), "D64" | "D71" | "D81" | "G64" | "TAP"))
        .map(|r| r.folder.as_str())
        .collect();
    folders.sort_unstable();
    folders.dedup();
    if ids.is_empty() && folders.is_empty() {
        return Ok(Vec::new());
    }
    let marks = |n: usize| vec!["?"; n].join(",");
    let sql = format!(
        "{ROW_SELECT} WHERE c.sha1 IN (SELECT sha1 FROM metadata WHERE status = 'ok' AND game_id IN ({}))
                              OR c.sha1 IN (SELECT sha1 FROM files WHERE folder IN ({})) GROUP BY c.sha1",
        if ids.is_empty() { "NULL".into() } else { marks(ids.len()) },
        if folders.is_empty() { "NULL".into() } else { marks(folders.len()) },
    );
    let mut st = conn.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::ToSql> = ids
        .iter()
        .chain(folders.iter())
        .map(|v| v as &dyn rusqlite::ToSql)
        .collect();
    let out = st
        .query_map(params.as_slice(), game_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(out)
}

/// Allerweltsnamen von Abbildern („side1“, „disk b“, „image3“), unter denen verschiedene
/// Spiele liegen – sie fassen nichts zusammen.
const GENERIC_TITLES: &[&str] = &[
    "side", "disk", "disc", "seite", "image", "part", "teil", "tape", "game", "file", "data",
    "intro", "menu", "boot", "loader", "demo", "untitled", "noname", "copy",
];

/// Kennung, unter der ein Inhalt mit anderen zusammengefasst wird: die Spielnummer von
/// ScreenScraper, sonst der bereinigte Titel. Zu kurze, rein numerische und allgemeine Titel
/// („a“, „1“, „side2“) fassen nichts zusammen.
fn title_key(title: &str) -> Option<String> {
    let t = crate::screenscraper::normalize_title(title);
    let base = t.trim_end_matches(|c: char| c.is_ascii_digit());
    let generic = GENERIC_TITLES.contains(&base)
        || base
            .strip_suffix(['a', 'b', 'c', 'd'])
            .is_some_and(|b| GENERIC_TITLES.contains(&b));
    (t.chars().count() >= 4 && !base.is_empty() && !generic).then_some(t)
}

/// Fasst Zeilen desselben Spiels zusammen. Reihenfolge: wie die erste Fassung in `rows`.
/// Vertreter einer Gruppe ist die Fassung mit Beschreibung, sonst eine ohne Szene-Kennung im
/// Namen, sonst die mit den meisten Orten.
/// Ordner mit höchstens so vielen Inhalten gelten als Ordner eines Spiels (Sammlungen wie
/// „GamesCollection“: `SUMMER1A_07545_03/SUMMER1A.G64, SUMMER1B.D64 …`); größere sind Sammelordner.
const SET_FOLDER_MAX: i64 = 16;

/// Erkennt Diskettenreihen ohne Seitenkennung im Namen: gleicher Ordner, gleicher Name bis auf das
/// letzte Zeichen (A–F oder Ziffer), z. B. SUMMER1A/SUMMER1B/SUMMER1C.
fn mark_folder_sets(conn: &Connection, rows: &mut [GameRow]) -> Result<()> {
    use std::collections::HashMap;
    let cand: Vec<(usize, String, String)> = rows
        .iter()
        .enumerate()
        // Nur Disketten und Bänder; bei Modulen und PRGs sind A/B/1/2 eher Teile oder Bände.
        .filter(|(_, r)| {
            r.disk.is_none()
                && !r.folder.is_empty()
                && matches!(r.format.as_str(), "D64" | "D71" | "D81" | "G64" | "TAP")
        })
        .filter_map(|(i, r)| {
            let stem = r.file_title.trim();
            let last = stem.chars().last()?;
            let last = last.to_ascii_uppercase();
            if !(last.is_ascii_digit() || matches!(last, 'A'..='F')) {
                return None;
            }
            let prefix = stem[..stem.len() - last.len_utf8()]
                .trim_end_matches([' ', '_', '-'])
                .to_uppercase();
            (prefix.chars().count() >= 3).then(|| (i, prefix, last.to_string()))
        })
        .collect();
    let mut count: HashMap<(String, String), usize> = HashMap::new();
    for (i, prefix, _) in &cand {
        *count
            .entry((rows[*i].folder.clone(), prefix.clone()))
            .or_default() += 1;
    }
    if !count.values().any(|&n| n > 1) {
        return Ok(());
    }
    let mut wanted: Vec<&str> = count
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|((f, _), _)| f.as_str())
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    let mut st = conn.prepare(&format!(
        "SELECT folder, COUNT(DISTINCT sha1) FROM files WHERE folder IN ({}) GROUP BY folder",
        vec!["?"; wanted.len()].join(",")
    ))?;
    let params: Vec<&dyn rusqlite::ToSql> =
        wanted.iter().map(|v| v as &dyn rusqlite::ToSql).collect();
    let sizes: HashMap<String, i64> = st
        .query_map(params.as_slice(), |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let marks: Vec<(usize, String, String)> = cand
        .into_iter()
        .filter(|(i, prefix, _)| {
            let folder = rows[*i].folder.as_str();
            count
                .get(&(folder.to_string(), prefix.clone()))
                .is_some_and(|&n| n > 1)
                && sizes.get(folder).is_some_and(|&n| n <= SET_FOLDER_MAX)
        })
        .map(|(i, prefix, side)| (i, format!("dir:{}|{prefix}", rows[i].folder), side))
        .collect();
    for (i, set, side) in marks {
        rows[i].disk = Some(side);
        rows[i].disk_set = Some(set);
    }
    Ok(())
}

fn group_rows(rows: Vec<GameRow>) -> Vec<GameRow> {
    use std::collections::HashMap;
    // Titel → Spielnummer, damit Fassungen ohne Abruf zu abgerufenen finden.
    // Zwei Spielnummern unter einem Titel: nicht über den Titel zuordnen.
    let mut by_title: HashMap<String, Option<String>> = HashMap::new();
    for r in rows.iter().filter(|r| r.game_id.is_some() && !r.ungrouped) {
        if let Some(t) = title_key(&r.title) {
            by_title
                .entry(t)
                .and_modify(|g| {
                    if g.as_ref() != r.game_id.as_ref() {
                        *g = None;
                    }
                })
                .or_insert(r.game_id.clone());
        }
    }
    // Ordner-Diskettenreihen halten zusammen; kennt ScreenScraper eine Disk, zählt deren Spielnummer.
    let mut by_set: HashMap<String, Option<String>> = HashMap::new();
    for r in rows.iter().filter(|r| !r.ungrouped) {
        if let Some(set) = r.disk_set.as_deref().filter(|s| s.starts_with("dir:")) {
            let e = by_set.entry(set.to_string()).or_default();
            if e.is_none() {
                e.clone_from(&r.game_id);
            }
        }
    }
    let key = |r: &GameRow| -> String {
        if r.ungrouped {
            return format!("sha:{}", r.sha1);
        }
        if let Some(set) = r.disk_set.as_deref().filter(|s| s.starts_with("dir:")) {
            return match by_set.get(set).cloned().flatten() {
                Some(g) => format!("ss:{g}"),
                None => format!("set:{set}"),
            };
        }
        if let Some(g) = &r.game_id {
            return format!("ss:{g}");
        }
        match title_key(&r.title) {
            Some(t) => match by_title.get(&t) {
                Some(Some(g)) => format!("ss:{g}"),
                _ => format!("t:{t}"),
            },
            None => format!("sha:{}", r.sha1),
        }
    };
    let mut order: Vec<String> = Vec::new();
    let mut members: HashMap<String, Vec<GameRow>> = HashMap::new();
    for r in rows {
        let k = key(&r);
        if !members.contains_key(&k) {
            order.push(k.clone());
        }
        members.entry(k).or_default().push(r);
    }
    order
        .into_iter()
        .map(|k| {
            let mut v = members.remove(&k).unwrap();
            // Vertreter: mit Beschreibung, sonst ein Name ohne Trainer-/Versionskennung, sonst die meisten Orte.
            let plain = |r: &GameRow| crate::screenscraper::clean_title(&r.title) == r.title.trim();
            // Bei Diskettenreihen die erste Disk/Seite, damit „Starten“ vorn beginnt.
            let mut first: HashMap<&str, &str> = HashMap::new();
            for r in &v {
                if let (Some(set), Some(d)) = (r.disk_set.as_deref(), r.disk.as_deref()) {
                    let e = first.entry(set).or_insert(d);
                    if d < *e {
                        *e = d;
                    }
                }
            }
            let first: HashMap<String, String> = first
                .into_iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let first_disk = |r: &GameRow| match (r.disk_set.as_deref(), r.disk.as_deref()) {
                (Some(set), Some(d)) => first.get(set).is_some_and(|f| f == d),
                _ => true,
            };
            let best = (0..v.len())
                .max_by_key(|&i| {
                    (
                        v[i].meta_status.as_deref() == Some("ok"),
                        first_disk(&v[i]),
                        plain(&v[i]),
                        v[i].locations,
                        std::cmp::Reverse(i),
                    )
                })
                .unwrap();
            v.swap(0, best);
            let mut formats: Vec<String> = Vec::new();
            for r in &v {
                if !formats.contains(&r.format) {
                    formats.push(r.format.clone());
                }
            }
            let mut sets: HashMap<&str, std::collections::HashSet<&str>> = HashMap::new();
            for r in &v {
                if let (Some(set), Some(d)) = (&r.disk_set, &r.disk) {
                    sets.entry(set).or_default().insert(d);
                }
            }
            let disks = sets
                .values()
                .map(|s| s.len() as i64)
                .max()
                .filter(|&n| n > 1)
                .unwrap_or(0);
            let variants = v.iter().map(|r| r.sha1.clone()).collect();
            let locations = v.iter().map(|r| r.locations).sum();
            let favorite = v.iter().any(|r| r.favorite);
            let rating = v.iter().map(|r| r.rating).max().unwrap_or(0);
            let done = v.iter().any(|r| r.done);
            let last_launched = v.iter().filter_map(|r| r.last_launched).max();
            let mut head = v.swap_remove(0);
            head.variants = variants;
            head.formats = formats.join(", ");
            head.locations = locations;
            head.favorite = favorite;
            head.rating = rating;
            head.done = done;
            head.last_launched = last_launched;
            head.disks = disks;
            // Name ohne „- Side 1“, die Seiten stehen in den Details.
            if disks > 0 && head.meta_status.as_deref() != Some("ok") {
                head.title = match head
                    .disk_set
                    .as_deref()
                    .and_then(|s| s.strip_prefix("dir:"))
                {
                    // Ordnerreihe: Name ohne das Seitenzeichen (SUMMER1A → SUMMER1).
                    Some(set) => set.rsplit('|').next().unwrap_or(&head.title).to_string(),
                    None => crate::screenscraper::clean_title(&head.title)
                        .trim_end_matches([' ', '-'])
                        .to_string(),
                };
            }
            head
        })
        .collect()
}

/// Zeilen zu bestimmten Inhalten (für die Fassungen eines zusammengefassten Spiels).
pub fn rows_for(conn: &Connection, sha1s: &[String]) -> Result<Vec<GameRow>> {
    let mut st = conn.prepare_cached(&format!("{ROW_SELECT} WHERE c.sha1 = ? GROUP BY c.sha1"))?;
    let mut out = Vec::new();
    for sha1 in sha1s {
        if let Some(r) = st.query_row([sha1], game_row).optional()? {
            out.push(r);
        }
    }
    mark_folder_sets(conn, &mut out)?;
    Ok(out)
}

fn csv_field(s: &str) -> String {
    if s.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Alle Treffer einer Suche (ohne Obergrenze) als CSV: Semikolon getrennt und mit BOM,
/// damit Excel in deutscher Einstellung die Spalten und Umlaute richtig erkennt.
pub fn export_csv(conn: &Connection, search: &Search) -> Result<(String, usize)> {
    let rows = search_with(
        conn,
        &Search {
            limit: i64::MAX,
            group: false,
            ..search.clone()
        },
    )?;
    let mut st = conn.prepare(
        "SELECT f.sha1, group_concat(s.label || ': ' || f.path
                    || CASE WHEN f.member <> '' THEN ' → ' || f.member ELSE '' END, ' | ')
         FROM files f JOIN sources s ON s.id = f.source_id GROUP BY f.sha1",
    )?;
    let places: std::collections::HashMap<String, String> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut out = String::from(
        "\u{feff}Titel;Jahr;Herausgeber;Genre;Format;Name im Abbild;Technik;Orte;Speicherorte;SHA-1\r\n",
    );
    for r in &rows {
        let fields = [
            r.title.clone(),
            r.year.clone().unwrap_or_default(),
            r.publisher.clone().unwrap_or_default(),
            r.genres.clone().unwrap_or_default(),
            r.format.clone(),
            r.image_name.clone().unwrap_or_default(),
            r.detail.clone().unwrap_or_default(),
            r.locations.to_string(),
            places.get(&r.sha1).cloned().unwrap_or_default(),
            r.sha1.clone(),
        ];
        out.push_str(
            &fields
                .iter()
                .map(|f| csv_field(f))
                .collect::<Vec<_>>()
                .join(";"),
        );
        out.push_str("\r\n");
    }
    Ok((out, rows.len()))
}

#[derive(Debug, Clone, Serialize)]
pub struct Location {
    pub source_id: i64,
    pub source_label: String,
    pub source_kind: String,
    pub source_location: String,
    pub path: String,
    pub member: String,
    /// Per Schnellscan übernommen, Inhalt nicht selbst gelesen.
    pub quick: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Details {
    pub row: GameRow,
    pub md5: String,
    pub crc32: String,
    pub entries: Vec<DirEntry>,
    pub locations: Vec<Location>,
    pub meta: Option<Meta>,
    /// (Bildart, Dateiname im Bilderordner)
    pub media: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Meta {
    pub provider: String,
    pub status: String,
    pub game_id: Option<String>,
    pub title: Option<String>,
    pub synopsis: Option<String>,
    /// Englische Beschreibung (eigene und Gemeinschaftsangaben).
    pub synopsis_en: Option<String>,
    pub developer: Option<String>,
    pub players: Option<String>,
    pub rating: Option<f64>,
    pub fetched_at: i64,
}

pub fn details(conn: &Connection, sha1: &str) -> Result<Details> {
    let row = conn.query_row(
        &format!("{ROW_SELECT} WHERE c.sha1 = ? GROUP BY c.sha1"),
        [sha1],
        game_row,
    )?;
    let (md5, crc32): (String, String) = conn.query_row(
        "SELECT md5, crc32 FROM contents WHERE sha1 = ?",
        [sha1],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let meta = conn
        .query_row(
            "SELECT provider, status, game_id, title, synopsis, developer, players, rating, fetched_at, synopsis_en
             FROM metadata WHERE sha1 = ?",
            [sha1],
            |r| {
                Ok(Meta {
                    provider: r.get(0)?,
                    status: r.get(1)?,
                    game_id: r.get(2)?,
                    title: r.get(3)?,
                    synopsis: r.get(4)?,
                    synopsis_en: r.get(9)?,
                    developer: r.get(5)?,
                    players: r.get(6)?,
                    rating: r.get(7)?,
                    fetched_at: r.get(8)?,
                })
            },
        )
        .optional()?;
    let mut st = conn.prepare("SELECT kind, file FROM media WHERE sha1 = ? ORDER BY kind DESC")?;
    let media = st
        .query_map([sha1], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut st =
        conn.prepare("SELECT name, kind, blocks FROM entries WHERE sha1 = ? ORDER BY idx")?;
    let entries = st
        .query_map([sha1], |r| {
            Ok(DirEntry {
                name: r.get(0)?,
                kind: r.get(1)?,
                blocks: r.get(2)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    let mut st = conn.prepare(
        "SELECT s.id, s.label, s.kind, s.location, f.path, f.member, f.quick
         FROM files f JOIN sources s ON s.id = f.source_id
         WHERE f.sha1 = ? ORDER BY s.label, f.path",
    )?;
    let locations = st
        .query_map([sha1], |r| {
            Ok(Location {
                source_id: r.get(0)?,
                source_label: r.get(1)?,
                source_kind: r.get(2)?,
                source_location: r.get(3)?,
                path: r.get(4)?,
                member: r.get(5)?,
                quick: r.get(6)?,
            })
        })?
        .collect::<Result<_, _>>()?;
    Ok(Details {
        row,
        md5,
        crc32,
        entries,
        locations,
        meta,
        media,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub files: i64,
    pub contents: i64,
    pub by_format: Vec<(String, i64)>,
    /// Inhalte mit Beschreibung (abgerufen oder selbst eingetragen).
    pub with_meta: i64,
    /// Bei ScreenScraper nicht gefunden oder als falsch markiert.
    pub not_found: i64,
}

pub fn stats(conn: &Connection) -> Result<Stats> {
    let files = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
    let contents = conn.query_row("SELECT COUNT(*) FROM contents", [], |r| r.get(0))?;
    let mut st =
        conn.prepare("SELECT format, COUNT(*) FROM contents GROUP BY format ORDER BY 2 DESC")?;
    let by_format = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let (with_meta, not_found) = conn.query_row(
        "SELECT COALESCE(SUM(m.status = 'ok'), 0), COALESCE(SUM(m.status <> 'ok'), 0)
         FROM metadata m JOIN contents c ON c.sha1 = m.sha1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(Stats {
        files,
        contents,
        by_format,
        with_meta,
        not_found,
    })
}

// ---------------------------------------------------------------- Aufräumen

/// Übersicht zum Aufräumen. Breadbin löscht selbst nichts – die Zahlen führen zu Listen, die man
/// durchgehen und als CSV speichern kann.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Cleanup {
    /// Inhalte, die an mehreren Orten liegen, und alle ihre Dateien.
    pub dup_contents: i64,
    pub dup_files: i64,
    /// Platz der zusätzlichen Kopien (je Inhalt alle Orte außer einem).
    pub dup_bytes: i64,
    /// Davon innerhalb derselben Quelle (echte Doppel, keine Kopie auf einem anderen Datenträger).
    pub same_source_contents: i64,
    pub same_source_bytes: i64,
    /// Beschädigte bzw. unbekannt aufgebaute Abbilder.
    pub damaged: i64,
    /// Inhalte ohne Angaben (nicht erkannt oder noch nicht nachgeschlagen).
    pub without_meta: i64,
}

pub fn cleanup(conn: &Connection) -> Result<Cleanup> {
    let (dup_contents, dup_files, dup_bytes) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(n), 0), COALESCE(SUM((n - 1) * size), 0)
         FROM (SELECT c.size AS size, COUNT(*) AS n FROM files f JOIN contents c ON c.sha1 = f.sha1
               GROUP BY f.sha1 HAVING COUNT(*) > 1)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let (same_source_contents, same_source_bytes) = conn.query_row(
        "SELECT COUNT(DISTINCT sha1), COALESCE(SUM((n - 1) * size), 0)
         FROM (SELECT f.sha1 AS sha1, c.size AS size, COUNT(*) AS n FROM files f JOIN contents c ON c.sha1 = f.sha1
               GROUP BY f.sha1, f.source_id HAVING COUNT(*) > 1)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let damaged = conn.query_row(
        "SELECT COUNT(*) FROM contents WHERE detail LIKE '%beschädigt%'",
        [],
        |r| r.get(0),
    )?;
    let without_meta = conn.query_row(
        "SELECT COUNT(*) FROM contents c LEFT JOIN metadata m ON m.sha1 = c.sha1 WHERE m.status IS NULL OR m.status <> 'ok'",
        [],
        |r| r.get(0),
    )?;
    Ok(Cleanup {
        dup_contents,
        dup_files,
        dup_bytes,
        same_source_contents,
        same_source_bytes,
        damaged,
        without_meta,
    })
}

// ---------------------------------------------------------------- Favoriten und Starts

/// Schaltet den Favoriten-Stern um; gibt den neuen Zustand zurück.
pub fn toggle_favorite(conn: &Connection, sha1: &str) -> Result<bool> {
    if conn.execute("DELETE FROM favorites WHERE sha1 = ?", [sha1])? > 0 {
        return Ok(false);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    conn.execute(
        "INSERT INTO favorites(sha1, added) VALUES(?, ?)",
        params![sha1, now],
    )?;
    Ok(true)
}

// ---------------------------------------------------------------- Eigene Listen und Notizen

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct GameList {
    pub id: i64,
    pub name: String,
    /// Fassungen in der Liste.
    pub count: i64,
}

pub fn lists(conn: &Connection) -> Result<Vec<GameList>> {
    let mut st = conn.prepare(
        "SELECT l.id, l.name, (SELECT COUNT(*) FROM list_items li WHERE li.list_id = l.id)
         FROM lists l ORDER BY l.name COLLATE NOCASE",
    )?;
    let out = st.query_map([], |r| {
        Ok(GameList {
            id: r.get(0)?,
            name: r.get(1)?,
            count: r.get(2)?,
        })
    })?;
    Ok(out.collect::<rusqlite::Result<_>>()?)
}

/// Legt eine Liste an; gibt es den Namen schon, deren Nummer.
pub fn create_list(conn: &Connection, name: &str) -> Result<i64> {
    let name = name.trim();
    anyhow::ensure!(
        !name.is_empty(),
        crate::tr!("Bitte einen Namen eingeben.", "Please enter a name.")
    );
    conn.execute("INSERT OR IGNORE INTO lists(name) VALUES(?)", [name])?;
    Ok(conn.query_row("SELECT id FROM lists WHERE name = ?", [name], |r| r.get(0))?)
}

pub fn rename_list(conn: &Connection, id: i64, name: &str) -> Result<()> {
    let name = name.trim();
    anyhow::ensure!(
        !name.is_empty(),
        crate::tr!("Bitte einen Namen eingeben.", "Please enter a name.")
    );
    conn.execute("UPDATE lists SET name = ? WHERE id = ?", params![name, id])
        .map_err(|_| {
            anyhow::anyhow!(crate::tr!(
                "Eine Liste „{name}“ gibt es schon.",
                "A list “{name}” already exists."
            ))
        })?;
    Ok(())
}

pub fn delete_list(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM list_items WHERE list_id = ?", [id])?;
    conn.execute("DELETE FROM lists WHERE id = ?", [id])?;
    Ok(())
}

/// Fassungen in eine Liste aufnehmen (`on`) oder herausnehmen.
pub fn set_in_list(conn: &Connection, id: i64, sha1s: &[String], on: bool) -> Result<()> {
    for sha1 in sha1s {
        if on {
            conn.execute(
                "INSERT OR IGNORE INTO list_items(list_id, sha1, added) VALUES(?, ?, ?)",
                params![id, sha1, now_secs()],
            )?;
        } else {
            conn.execute(
                "DELETE FROM list_items WHERE list_id = ? AND sha1 = ?",
                params![id, sha1],
            )?;
        }
    }
    Ok(())
}

/// Listen, in denen mindestens eine der Fassungen steht.
pub fn lists_of(conn: &Connection, sha1s: &[String]) -> Result<Vec<i64>> {
    let mut st = conn.prepare_cached("SELECT list_id FROM list_items WHERE sha1 = ?")?;
    let mut ids: Vec<i64> = Vec::new();
    for sha1 in sha1s {
        for id in st.query_map([sha1], |r| r.get(0))? {
            ids.push(id?);
        }
    }
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, PartialEq)]
pub struct Note {
    pub rating: i64,
    pub done: bool,
    pub note: String,
}

/// Notiz zu einem Spiel: die erste vorhandene unter den Fassungen (die gezeigte zuerst übergeben).
pub fn note_of(conn: &Connection, sha1s: &[String]) -> Result<Note> {
    let mut st = conn.prepare_cached("SELECT rating, done, note FROM notes WHERE sha1 = ?")?;
    for sha1 in sha1s {
        if let Some(n) = st
            .query_row([sha1], |r| {
                Ok(Note {
                    rating: r.get(0)?,
                    done: r.get::<_, i64>(1)? != 0,
                    note: r.get(2)?,
                })
            })
            .optional()?
        {
            return Ok(n);
        }
    }
    Ok(Note::default())
}

/// Notiz für alle Fassungen eines Spiels speichern; leer = löschen.
pub fn save_note(conn: &Connection, sha1s: &[String], n: &Note) -> Result<()> {
    let rating = n.rating.clamp(0, 5);
    let note = n.note.trim();
    for sha1 in sha1s {
        if rating == 0 && !n.done && note.is_empty() {
            conn.execute("DELETE FROM notes WHERE sha1 = ?", [sha1])?;
        } else {
            conn.execute(
                "INSERT OR REPLACE INTO notes(sha1, rating, done, note, changed) VALUES(?, ?, ?, ?, ?)",
                params![sha1, rating, n.done as i64, note, now_secs()],
            )?;
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct ProposalLog {
    pub typ: String,
    pub ok: bool,
    pub response: String,
    pub sent_at: i64,
}

pub fn log_proposal(
    conn: &Connection,
    game_id: &str,
    typ: &str,
    ok: bool,
    response: &str,
) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    conn.execute(
        "INSERT INTO proposals(game_id, typ, ok, response, sent_at) VALUES(?, ?, ?, ?, ?)",
        params![game_id, typ, ok, response, now],
    )?;
    Ok(())
}

/// Bisher geschickte Vorschläge zu einem Spiel, neueste zuerst.
pub fn proposals(conn: &Connection, game_id: &str) -> Result<Vec<ProposalLog>> {
    let mut st = conn.prepare(
        "SELECT typ, ok, response, sent_at FROM proposals WHERE game_id = ? ORDER BY id DESC",
    )?;
    let rows = st.query_map([game_id], |r| {
        Ok(ProposalLog {
            typ: r.get(0)?,
            ok: r.get(1)?,
            response: r.get(2)?,
            sent_at: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Fassung beim Zusammenfassen allein stehen lassen (`true`) oder wieder einordnen.
pub fn set_ungrouped(conn: &Connection, sha1: &str, on: bool) -> Result<()> {
    if on {
        conn.execute("INSERT OR IGNORE INTO ungrouped(sha1) VALUES(?)", [sha1])?;
    } else {
        conn.execute("DELETE FROM ungrouped WHERE sha1 = ?", [sha1])?;
    }
    Ok(())
}

pub fn record_launch(conn: &Connection, sha1: &str, device: &str) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    conn.execute(
        "INSERT INTO launches(sha1, device, at) VALUES(?, ?, ?)",
        params![sha1, device, now],
    )?;
    Ok(())
}

// ---------------------------------------------------------------- Einstellungen

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(
        conn: &Connection,
        source: i64,
        path: &str,
        sha1: &str,
        title: &str,
        year: Option<&str>,
    ) {
        conn.execute(
            "INSERT OR IGNORE INTO contents(sha1, md5, crc32, size, format) VALUES(?, '', '', 1, 'PRG')",
            [sha1],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO files(source_id, path, fsize, fmtime, sha1, title, year, seen) VALUES(?, ?, 1, 0, ?, ?, ?, 1)",
            params![source, path, sha1, title, year],
        )
        .unwrap();
    }

    #[test]
    fn filter_und_sortierung() {
        let conn = open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO sources(id, kind, label, location) VALUES(1, 'local', 'A', '/a'), (2, 'local', 'B', '/b')",
        )
        .unwrap();
        file(&conn, 1, "x/100%_game.prg", "s1", "Zaxxon", Some("1984"));
        file(&conn, 1, "x/b.prg", "s2", "Archon", Some("1983"));
        file(&conn, 2, "y/b.prg", "s2", "Archon", Some("1983"));
        file(&conn, 2, "y/c.prg", "s3", "Boulder Dash", None);
        conn.execute("INSERT INTO metadata(sha1, provider, status, fetched_at) VALUES('s3', 'screenscraper', 'ok', 0)", [])
            .unwrap();

        let titles = |s: Search| {
            search_with(&conn, &s)
                .unwrap()
                .into_iter()
                .map(|r| r.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            titles(Search::default()),
            ["Archon", "Boulder Dash", "Zaxxon"]
        );
        assert_eq!(
            titles(Search {
                desc: true,
                ..Default::default()
            }),
            ["Zaxxon", "Boulder Dash", "Archon"]
        );
        // Jahr: fehlende Angaben stehen immer am Ende.
        assert_eq!(
            titles(Search {
                sort: "year".into(),
                ..Default::default()
            }),
            ["Archon", "Zaxxon", "Boulder Dash"]
        );
        assert_eq!(
            titles(Search {
                sort: "year".into(),
                desc: true,
                ..Default::default()
            }),
            ["Zaxxon", "Archon", "Boulder Dash"]
        );
        assert_eq!(
            titles(Search {
                source_id: Some(2),
                ..Default::default()
            }),
            ["Archon", "Boulder Dash"]
        );
        assert_eq!(
            titles(Search {
                only_duplicates: true,
                ..Default::default()
            }),
            ["Archon"]
        );
        assert_eq!(
            titles(Search {
                only_meta: true,
                ..Default::default()
            }),
            ["Boulder Dash"]
        );
        // % und _ werden wörtlich gesucht.
        assert_eq!(
            titles(Search {
                query: "100%_".into(),
                ..Default::default()
            }),
            ["Zaxxon"]
        );
        assert!(
            titles(Search {
                query: "1000".into(),
                ..Default::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn gleiche_spiele_zusammenfassen() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sources(id, kind, label, location) VALUES(1, 'local', 'A', '/a')",
            [],
        )
        .unwrap();
        file(
            &conn,
            1,
            "giana.d64",
            "g1",
            "Great Giana Sisters, The",
            Some("1987"),
        );
        file(
            &conn,
            1,
            "giana2.d64",
            "g1",
            "Great Giana Sisters, The",
            Some("1987"),
        );
        file(
            &conn,
            1,
            "Giana (crack).prg",
            "g2",
            "Great Giana Sisters",
            None,
        );
        file(&conn, 1, "gs.crt", "g3", "GS EasyFlash", None);
        file(&conn, 1, "boulder.prg", "b1", "Boulder Dash", None);
        file(&conn, 1, "boulder2.prg", "b2", "Boulder Dash II", None);
        file(&conn, 1, "x/a.prg", "x1", "a", None);
        file(&conn, 1, "u/side1.d64", "u1", "side1", None);
        file(&conn, 1, "v/side1.d64", "u2", "side1", None);
        file(&conn, 1, "y/a.prg", "x2", "a", None);
        conn.execute("UPDATE contents SET format = 'CRT' WHERE sha1 = 'g3'", [])
            .unwrap();
        // g3 hat einen anderen Namen, aber dieselbe Spielnummer wie g2.
        conn.execute_batch(
            "INSERT INTO metadata(sha1, provider, status, game_id, title, fetched_at)
               VALUES('g3', 'screenscraper', 'ok', '42', 'Great Giana Sisters', 0),
                     ('b1', 'screenscraper', 'ok', '7', 'Boulder Dash', 0)",
        )
        .unwrap();

        let rows = search_with(
            &conn,
            &Search {
                group: true,
                ..Default::default()
            },
        )
        .unwrap();
        let summary: Vec<(String, usize, String)> = rows
            .iter()
            .map(|r| (r.title.clone(), r.variants.len(), r.formats.clone()))
            .collect();
        assert_eq!(
            summary,
            [
                ("a".to_string(), 1, "PRG".to_string()),
                ("a".to_string(), 1, "PRG".to_string()),
                ("Boulder Dash".to_string(), 1, "PRG".to_string()),
                ("Boulder Dash II".to_string(), 1, "PRG".to_string()),
                ("Great Giana Sisters".to_string(), 3, "CRT, PRG".to_string()),
                ("side1".to_string(), 1, "PRG".to_string()),
                ("side1".to_string(), 1, "PRG".to_string()),
            ]
        );
        let giana = &rows[4];
        assert_eq!(
            giana.sha1, "g3",
            "Fassung mit Beschreibung vertritt die Gruppe"
        );
        assert_eq!(giana.locations, 4);
        assert_eq!(rows_for(&conn, &giana.variants).unwrap().len(), 3);
        assert_eq!(
            search_with(
                &conn,
                &Search {
                    group: true,
                    limit: 2,
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            2
        );
        assert_eq!(
            search_with(&conn, &Search::default()).unwrap().len(),
            9,
            "ohne Zusammenfassen je Inhalt"
        );
    }

    #[test]
    fn aufraeum_uebersicht() {
        let conn = open_in_memory().unwrap();
        conn.execute("INSERT INTO sources(id, kind, label, location) VALUES(1, 'local', 'A', '/a'), (2, 'local', 'B', '/b')", [])
            .unwrap();
        file(&conn, 1, "x.d64", "s1", "Alpha", None);
        file(&conn, 1, "kopie/x.d64", "s1", "Alpha", None);
        file(&conn, 2, "x.d64", "s1", "Alpha", None);
        file(&conn, 1, "y.prg", "s2", "Beta", None);
        conn.execute("UPDATE contents SET size = 100", []).unwrap();
        conn.execute("UPDATE contents SET detail = 'D64: Datei beschädigt oder unbekannter Aufbau' WHERE sha1 = 's2'", [])
            .unwrap();
        conn.execute(
            "INSERT INTO metadata(sha1, provider, status, fetched_at) VALUES('s1', 'own', 'ok', 0)",
            [],
        )
        .unwrap();
        let c = cleanup(&conn).unwrap();
        assert_eq!((c.dup_contents, c.dup_files, c.dup_bytes), (1, 3, 200));
        assert_eq!((c.same_source_contents, c.same_source_bytes), (1, 100));
        assert_eq!((c.damaged, c.without_meta), (1, 1));
        let titles = |s: Search| {
            search_with(&conn, &s)
                .unwrap()
                .into_iter()
                .map(|r| r.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            titles(Search {
                only_damaged: true,
                ..Default::default()
            }),
            ["Beta"]
        );
        assert_eq!(
            titles(Search {
                without_meta: true,
                ..Default::default()
            }),
            ["Beta"]
        );
    }

    #[test]
    fn listen_und_notizen() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sources(id, kind, label, location) VALUES(1, 'local', 'A', '/a')",
            [],
        )
        .unwrap();
        file(&conn, 1, "a.prg", "s1", "Alpha", None);
        file(&conn, 1, "b.prg", "s2", "Beta", None);
        let id = create_list(&conn, "Partyspiele").unwrap();
        assert_eq!(
            create_list(&conn, "partyspiele").unwrap(),
            id,
            "gleicher Name ohne Groß/klein = dieselbe Liste"
        );
        set_in_list(&conn, id, &["s2".into()], true).unwrap();
        let titles = |s: Search| {
            search_with(&conn, &s)
                .unwrap()
                .into_iter()
                .map(|r| r.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            titles(Search {
                list_id: Some(id),
                ..Default::default()
            }),
            ["Beta"]
        );
        assert_eq!(
            lists(&conn).unwrap(),
            [GameList {
                id,
                name: "Partyspiele".into(),
                count: 1
            }]
        );
        assert_eq!(lists_of(&conn, &["s1".into(), "s2".into()]).unwrap(), [id]);
        rename_list(&conn, id, "Mit Freunden").unwrap();
        assert_eq!(lists(&conn).unwrap()[0].name, "Mit Freunden");

        let n = Note {
            rating: 4,
            done: true,
            note: "Code: 1234".into(),
        };
        save_note(&conn, &["s1".into()], &n).unwrap();
        assert_eq!(note_of(&conn, &["s2".into(), "s1".into()]).unwrap(), n);
        let r = search_with(&conn, &Search::default()).unwrap();
        let alpha = r.iter().find(|r| r.title == "Alpha").unwrap();
        assert_eq!((alpha.rating, alpha.done), (4, true));
        save_note(&conn, &["s1".into()], &Note::default()).unwrap();
        assert_eq!(note_of(&conn, &["s1".into()]).unwrap(), Note::default());
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM notes", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "leere Notiz wird gelöscht");

        delete_list(&conn, id).unwrap();
        assert!(lists(&conn).unwrap().is_empty());
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM list_items", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn favoriten_und_zuletzt_gestartet() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sources(id, kind, label, location) VALUES(1, 'local', 'A', '/a')",
            [],
        )
        .unwrap();
        file(&conn, 1, "a.prg", "s1", "Alpha", None);
        file(&conn, 1, "b.prg", "s2", "Beta", None);
        file(&conn, 1, "c.prg", "s3", "Gamma", None);
        assert!(toggle_favorite(&conn, "s2").unwrap());
        let titles = |s: Search| {
            search_with(&conn, &s)
                .unwrap()
                .into_iter()
                .map(|r| r.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            titles(Search {
                only_favorites: true,
                ..Default::default()
            }),
            ["Beta"]
        );
        assert!(!toggle_favorite(&conn, "s2").unwrap());
        assert!(
            titles(Search {
                only_favorites: true,
                ..Default::default()
            })
            .is_empty()
        );

        conn.execute("INSERT INTO launches(sha1, device, at) VALUES('s1', 'E2', 100), ('s3', 'E2', 200), ('s1', 'C64U', 50)", [])
            .unwrap();
        assert_eq!(
            titles(Search {
                recent: true,
                ..Default::default()
            }),
            ["Gamma", "Alpha"]
        );
        let rows = search_with(&conn, &Search::default()).unwrap();
        assert_eq!(rows[0].last_launched, Some(100));
        assert_eq!(rows[1].last_launched, None);

        // Selbst geschrieben (teilbar): eigene Angaben, nicht die auf Grundlage von ScreenScraper bearbeiteten.
        conn.execute(
            "INSERT INTO metadata(sha1, provider, status, game_id, title, fetched_at) VALUES
             ('s1', 'own', 'ok', NULL, 'Alpha', 0), ('s2', 'own:edited', 'ok', '123', 'Beta', 0),
             ('s3', 'screenscraper', 'notfound', NULL, NULL, 0)",
            [],
        )
        .unwrap();
        assert_eq!(
            titles(Search {
                shareable: true,
                ..Default::default()
            }),
            ["Alpha"]
        );
    }

    #[test]
    fn csv_export() {
        let conn = open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO sources(id, kind, label, location) VALUES(1, 'local', 'Spiele', '/s')",
            [],
        )
        .unwrap();
        file(
            &conn,
            1,
            "a/x.prg",
            "s1",
            "Spy vs. Spy; Island \"Caper\"",
            Some("1985"),
        );
        file(
            &conn,
            1,
            "b/x.prg",
            "s1",
            "Spy vs. Spy; Island \"Caper\"",
            Some("1985"),
        );
        let (csv, n) = export_csv(&conn, &Search::default()).unwrap();
        assert_eq!(n, 1);
        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[0].starts_with("\u{feff}Titel;Jahr"));
        assert!(
            lines[1].starts_with("\"Spy vs. Spy; Island \"\"Caper\"\"\";1985;"),
            "{}",
            lines[1]
        );
        assert!(
            lines[1].contains("Spiele: a/x.prg | Spiele: b/x.prg")
                || lines[1].contains("Spiele: b/x.prg | Spiele: a/x.prg")
        );
    }

    #[test]
    fn alte_geraetetabelle_bekommt_wlan_mac() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE devices(id INTEGER PRIMARY KEY, name TEXT NOT NULL, host TEXT NOT NULL UNIQUE,
                                  product TEXT NOT NULL, upload_dir TEXT NOT NULL);
             INSERT INTO devices(name, host, product, upload_dir) VALUES('E2', '1.2.3.4', 'U64', '/USB0/B');",
        )
        .unwrap();
        init(&conn).unwrap();
        init(&conn).unwrap(); // zweimal: darf nichts doppelt anlegen
        let mac: String = conn
            .query_row("SELECT wifi_mac FROM devices", [], |r| r.get(0))
            .unwrap();
        assert_eq!(mac, "");
    }

    #[test]
    fn alte_eigene_angaben_mit_spielnummer_gelten_als_bearbeitet() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO metadata(sha1, provider, status, game_id, fetched_at) VALUES('a', 'own', 'ok', '7', 0), ('b', 'own', 'ok', NULL, 0)",
            [],
        )
        .unwrap();
        init(&conn).unwrap();
        // Danach selbst geschrieben, mit Spielnummer: darf ein zweiter Start nicht umstellen.
        conn.execute("INSERT INTO metadata(sha1, provider, status, game_id, fetched_at) VALUES('c', 'own', 'ok', '8', 0)", []).unwrap();
        init(&conn).unwrap();
        let p = |s: &str| {
            conn.query_row("SELECT provider FROM metadata WHERE sha1 = ?", [s], |r| {
                r.get::<_, String>(0)
            })
            .unwrap()
        };
        assert_eq!(
            (p("a"), p("b"), p("c")),
            ("own:edited".into(), "own".into(), "own".into())
        );
    }

    #[test]
    fn titelkennung_mit_umlaut_am_ende() {
        // Früher Absturz beim Start: Abschneiden des letzten Bytes traf mitten ins „ö“.
        assert_eq!(title_key("Schlümpfö").as_deref(), Some("schlümpfö"));
        assert_eq!(title_key("Göö"), None);
        assert_eq!(title_key("Disk B"), None);
        assert_eq!(title_key("Side 2"), None);
        assert!(title_key("Diskö").is_some());
    }

    #[test]
    fn aus_gruppe_geloeste_fassung_steht_allein() {
        let conn = open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let src = add_local_source(&conn, dir.path()).unwrap();
        for (sha1, title) in [("s1", "Speed"), ("s2", "SPEED")] {
            conn.execute(
                "INSERT INTO contents(sha1, size, crc32, md5, format) VALUES(?, 1, '', '', 'D64')",
                [sha1],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO files(source_id, path, fsize, fmtime, sha1, title, seen) VALUES(?, ?, 1, 0, ?, ?, 0)",
                params![src, format!("{sha1}.d64"), sha1, title],
            )
            .unwrap();
        }
        let grouped = |c: &Connection| {
            search_with(
                c,
                &Search {
                    group: true,
                    ..Default::default()
                },
            )
            .unwrap()
            .len()
        };
        assert_eq!(grouped(&conn), 1);
        set_ungrouped(&conn, "s2", true).unwrap();
        assert_eq!(grouped(&conn), 2);
        set_ungrouped(&conn, "s2", false).unwrap();
        assert_eq!(grouped(&conn), 1);
    }

    #[test]
    fn diskettenreihe_zaehlt_seiten_und_beginnt_mit_disk_1() {
        let conn = open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let src = add_local_source(&conn, dir.path()).unwrap();
        for (sha1, title) in [
            ("d2", "Comaland - Side 2"),
            ("d1", "Comaland - Side 1"),
            ("d3", "Comaland - Side 3"),
        ] {
            conn.execute(
                "INSERT INTO contents(sha1, size, crc32, md5, format) VALUES(?, 1, '', '', 'D64')",
                [sha1],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO files(source_id, path, fsize, fmtime, sha1, title, seen) VALUES(?, ?, 1, 0, ?, ?, 0)",
                params![src, format!("{sha1}.d64"), sha1, title],
            )
            .unwrap();
        }
        let rows = search_with(
            &conn,
            &Search {
                group: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(
            (
                r.sha1.as_str(),
                r.disk.as_deref(),
                r.disks,
                r.title.as_str()
            ),
            ("d1", Some("1"), 3, "Comaland")
        );
        assert_eq!(r.disk_set.as_deref(), Some("D64:comaland"));
    }

    #[test]
    fn diskettenreihe_ueber_den_ordner() {
        let conn = open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let src = add_local_source(&conn, dir.path()).unwrap();
        let files = [
            ("a", "SUMMER1A", "s5/SUMMER1A_07545_03/SUMMER1A.G64", "G64"),
            ("b", "SUMMER1B", "s5/SUMMER1A_07545_03/SUMMER1B.D64", "D64"),
            ("c", "SUMMER1C", "s5/SUMMER1A_07545_03/SUMMER1C.D64", "D64"),
            // gleicher Name bis aufs letzte Zeichen, aber anderer Ordner: keine Reihe
            ("x", "SUMMER1D", "anderswo/SUMMER1D.D64", "D64"),
        ];
        for (sha1, title, path, format) in files {
            conn.execute(
                "INSERT INTO contents(sha1, size, crc32, md5, format) VALUES(?, 1, '', '', ?)",
                [sha1, format],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO files(source_id, path, fsize, fmtime, sha1, title, seen) VALUES(?, ?, 1, 0, ?, ?, 0)",
                params![src, path, sha1, title],
            )
            .unwrap();
        }
        let rows = search_with(
            &conn,
            &Search {
                group: true,
                ..Default::default()
            },
        )
        .unwrap();
        let set = rows
            .iter()
            .find(|r| r.variants.len() == 3)
            .expect("A, B, C zusammen");
        assert_eq!(
            (set.sha1.as_str(), set.disks, set.title.as_str()),
            ("a", 3, "SUMMER1")
        );
        let v = rows_for(&conn, &set.variants).unwrap();
        assert_eq!(
            v.iter()
                .map(|r| r.disk.clone().unwrap())
                .collect::<Vec<_>>(),
            ["A", "B", "C"]
        );
        assert!(rows.iter().any(|r| r.sha1 == "x" && r.disk.is_none()));
    }
}
