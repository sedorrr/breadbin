//! Beschreibungen und Bilder für die erfassten Dateien abrufen und speichern.

use crate::tr;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::screenscraper::{self, ApiError, Game, Lookup, RomQuery};

/// Bildarten, die geladen werden: Spielszene, Titelbild, Verpackung.
pub const MEDIA_KINDS: [&str; 3] = ["ss", "sstitle", "box-2D"];
/// So oft wird bei „ausgelastet“ nach einer Pause neu versucht.
const BUSY_RETRIES: u32 = 3;
const BUSY_PAUSE: Duration = Duration::from_secs(15);
/// Nach so vielen Fehlern hintereinander wird abgebrochen.
const MAX_ERRORS_IN_ROW: u32 = 5;

/// Die Abfrage-Seite, getrennt vom HTTP-Client, damit sich der Ablauf testen lässt.
/// `Send`, weil mehrere Quellen (je eine pro Thread) gleichzeitig abfragen.
pub trait GameSource: Send {
    fn game_info(&mut self, rom: &RomQuery, lang: &str) -> Result<Lookup>;
    fn download(&mut self, url: &str) -> Result<Vec<u8>>;
    /// Suche über den Namen, nur ein sicher passender Treffer zählt.
    fn by_name(&mut self, _title: &str, _lang: &str) -> Result<Option<Game>> {
        Ok(None)
    }
}

impl GameSource for screenscraper::Client {
    fn game_info(&mut self, rom: &RomQuery, lang: &str) -> Result<Lookup> {
        screenscraper::Client::game_info(self, rom, lang)
    }
    fn download(&mut self, url: &str) -> Result<Vec<u8>> {
        screenscraper::Client::download(self, url)
    }
    fn by_name(&mut self, title: &str, lang: &str) -> Result<Option<Game>> {
        // Ohne Trainer- und Versionskennung suchen, sonst findet ScreenScraper nichts.
        let name = screenscraper::clean_title(title);
        // Zu kurze Namen („4K“, „IT“) nimmt ScreenScraper nicht an – dann eben kein Namenstreffer.
        if name.trim().chars().count() < screenscraper::MIN_SEARCH_CHARS {
            return Ok(None);
        }
        let hits = self
            .search_name(&name, lang)
            .map_err(|e| context_keep(e, "jeuRecherche"))?;
        let Some(best) = screenscraper::best_name_match(title, &hits) else {
            return Ok(None);
        };
        // Die Trefferliste ist teils unvollständig (Bilder, Beschreibung): Einzelabfrage. Scheitert
        // sie (ohne dass der ganze Lauf anhalten muss), reicht der Eintrag aus der Trefferliste.
        Ok(match self.game_by_id(&best.id, lang) {
            Ok(Lookup::Found(g)) => Some(*g),
            Ok(Lookup::NotFound) => Some(best.clone()),
            Err(e)
                if e.downcast_ref::<ApiError>()
                    .is_some_and(|a| !matches!(a, ApiError::Http(..) | ApiError::Busy(_))) =>
            {
                return Err(e);
            }
            Err(_) => Some(best.clone()),
        })
    }
}

pub struct Options<'a> {
    /// Nur diese Inhalte; `None` = alle, die noch keine Angaben haben.
    pub sha1s: Option<Vec<String>>,
    /// Auch Inhalte erneut abfragen, die schon Angaben haben oder nicht gefunden wurden.
    pub force: bool,
    /// Früher nicht gefundene Inhalte erneut abfragen.
    pub retry_notfound: bool,
    pub lang: &'a str,
    pub media_dir: &'a Path,
    /// Ohne Treffer über die Prüfsumme hilfsweise über den Namen suchen.
    pub name_fallback: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Progress {
    pub total: u64,
    pub done: u64,
    pub found: u64,
    pub not_found: u64,
    pub errors: Vec<String>,
    pub current: String,
    /// Grund, warum der Abruf vorzeitig endete (Kontingent, Zugang …).
    pub stopped: Option<String>,
    pub cancelled: bool,
    /// Gleichzeitige Abfragen (sinkt, wenn ScreenScraper „ausgelastet“ meldet).
    pub threads: u32,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Inhalte, die abgefragt werden sollen, mit dem Dateinamen eines Fundorts.
pub fn pending(conn: &Connection, opts: &Options) -> Result<Vec<RomQuery>> {
    let mut st = conn.prepare(
        "SELECT c.sha1, c.size, c.crc32, c.md5,
                (SELECT CASE WHEN f.member <> '' THEN f.member ELSE f.path END
                 FROM files f WHERE f.sha1 = c.sha1 ORDER BY f.id LIMIT 1),
                m.status, m.provider,
                (SELECT f.title FROM files f WHERE f.sha1 = c.sha1 ORDER BY f.id LIMIT 1)
         FROM contents c LEFT JOIN metadata m ON m.sha1 = c.sha1
         ORDER BY 5 COLLATE NOCASE",
    )?;
    let wanted: Option<HashSet<&String>> = opts.sha1s.as_ref().map(|v| v.iter().collect());
    let prefer_shared = crate::share::prefer_shared(conn)?;
    let mut out = Vec::new();
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
            r.get::<_, Option<String>>(7)?,
        ))
    })?;
    for row in rows {
        let (sha1, size, crc32, md5, path, status, provider, title) = row?;
        if wanted.as_ref().is_some_and(|w| !w.contains(&sha1)) {
            continue;
        }
        // Entscheidungen des Nutzers (abgelehnt, von Hand zugeordnet, selbst eingetragen) überschreibt kein Abruf.
        if status.as_deref() == Some("rejected")
            || matches!(
                provider.as_deref(),
                Some(PROVIDER_MANUAL | PROVIDER_OWN | PROVIDER_OWN_EDITED)
            )
        {
            continue;
        }
        // Gemeinschaftsangaben: Sind sie die bevorzugte Quelle, fragt nur ein ausdrücklicher Abruf
        // ScreenScraper; sonst wie „nicht gefunden“ – ScreenScraper ersetzt sie, sobald es das Spiel kennt.
        let shared = provider.as_deref() == Some(crate::share::PROVIDER_SHARED);
        let take = opts.force
            || match status.as_deref() {
                None => true,
                Some("notfound") => opts.retry_notfound,
                Some("ok") if shared => opts.retry_notfound && !prefer_shared,
                _ => false,
            };
        let Some(path) = path else { continue };
        if take {
            let file_name = path.rsplit(['/', '\\']).next().unwrap_or(&path).to_string();
            out.push(RomQuery {
                file_name,
                title: title.unwrap_or_default(),
                size,
                crc32,
                md5,
                sha1,
            });
        }
    }
    Ok(out)
}

fn store_not_found(conn: &Connection, sha1: &str) -> Result<()> {
    // Von anderen übernommene Angaben bleiben, solange ScreenScraper das Spiel nicht kennt.
    let provider: Option<String> = conn
        .query_row(
            "SELECT provider FROM metadata WHERE sha1 = ?",
            [sha1],
            |r| r.get(0),
        )
        .optional()?;
    if provider.as_deref() == Some(crate::share::PROVIDER_SHARED) {
        return Ok(());
    }
    conn.execute("DELETE FROM media WHERE sha1 = ?", [sha1])?;
    conn.execute(
        "INSERT OR REPLACE INTO metadata(sha1, provider, status, fetched_at) VALUES(?, 'screenscraper', 'notfound', ?)",
        params![sha1, now()],
    )?;
    Ok(())
}

/// Anbieter-Kennung: `screenscraper` (Prüfsumme) oder `screenscraper:name` (über den Namen).
pub const PROVIDER_NAME_MATCH: &str = "screenscraper:name";

/// Hängt den Namen der Anfrage an eine Fehlermeldung, ohne die Einordnung (`ApiError`) zu verlieren.
fn context_keep(e: anyhow::Error, what: &'static str) -> anyhow::Error {
    e.context(what)
}
/// Vom Nutzer ausgewähltes Spiel.
pub const PROVIDER_MANUAL: &str = "screenscraper:manual";
/// Vom Nutzer selbst geschriebene Angaben (teilbar).
pub const PROVIDER_OWN: &str = "own";
/// Vom Nutzer bearbeitete Angaben, die auf ScreenScraper beruhen (nicht teilbar).
pub const PROVIDER_OWN_EDITED: &str = "own:edited";

/// Stammen die Angaben vom Nutzer (selbst geschrieben oder bearbeitet)?
pub fn is_own(provider: &str) -> bool {
    provider == PROVIDER_OWN || provider == PROVIDER_OWN_EDITED
}

/// Selbst eingetragene Angaben; leere Felder werden als „unbekannt“ gespeichert.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(default)]
pub struct OwnMeta {
    pub title: String,
    pub year: String,
    pub publisher: String,
    pub developer: String,
    pub genres: String,
    pub players: String,
    /// Beschreibung auf Deutsch (bzw. in der einzigen vorhandenen Sprache).
    pub synopsis: String,
    /// Beschreibung auf Englisch (freiwillig).
    pub synopsis_en: String,
}

/// Speichert eigene Angaben. Eine vorhandene ScreenScraper-Spielnummer bleibt erhalten, damit
/// Zusammenfassen und Link zur Spielseite weiter gehen; Bilder bleiben unverändert.
///
/// Herkunft: Wer Angaben von ScreenScraper bearbeitet (der Dialog belegt sie vor), bekommt
/// `own:edited` – nicht teilbar. `fresh` = „Eigene Fassung schreiben“ mit leerem Dialog → `own`;
/// dabei fallen auch die Bilder von ScreenScraper (`ss_…`) weg, eigene Bilder bleiben.
/// Bearbeitete Gemeinschaftsangaben bleiben teilbar (gleiche Lizenz).
pub fn save_own(conn: &Connection, sha1s: &[String], m: &OwnMeta, fresh: bool) -> Result<()> {
    let opt = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    for sha1 in sha1s {
        let existing: Option<(String, Option<String>)> = conn
            .query_row(
                "SELECT provider, game_id FROM metadata WHERE sha1 = ? AND status = 'ok'",
                [sha1],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let game_id = existing.as_ref().and_then(|(_, id)| id.clone());
        let from_ss = existing
            .as_ref()
            .is_some_and(|(p, _)| p.starts_with("screenscraper") || p == PROVIDER_OWN_EDITED);
        let provider = if from_ss && !fresh {
            PROVIDER_OWN_EDITED
        } else {
            PROVIDER_OWN
        };
        if fresh {
            conn.execute(
                "DELETE FROM media WHERE sha1 = ? AND file LIKE 'ss\\_%' ESCAPE '\\'",
                [sha1],
            )?;
        }
        conn.execute(
            "INSERT OR REPLACE INTO metadata(sha1, provider, status, game_id, title, synopsis, year, publisher,
                                             developer, genres, players, rating, fetched_at, synopsis_en)
             VALUES(?, ?, 'ok', ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)",
            params![
                sha1,
                provider,
                game_id,
                opt(&m.title),
                opt(&m.synopsis),
                opt(&m.year),
                opt(&m.publisher),
                opt(&m.developer),
                opt(&m.genres),
                opt(&m.players),
                now(),
                opt(&m.synopsis_en)
            ],
        )?;
    }
    Ok(())
}

/// Löscht Angaben und Bilder, egal woher (ScreenScraper, von Hand, selbst eingetragen); der Inhalt
/// gilt danach als nicht nachgeschlagen. Die Bilddateien bleiben, andere Fassungen nutzen sie evtl.
pub fn clear(conn: &Connection, sha1s: &[String]) -> Result<()> {
    for sha1 in sha1s {
        conn.execute("DELETE FROM metadata WHERE sha1 = ?", [sha1])?;
        conn.execute("DELETE FROM media WHERE sha1 = ?", [sha1])?;
    }
    Ok(())
}

/// Bild aus dem Bilderordner im Format, das ScreenScraper je Bildart verlangt: Verpackung PNG,
/// Screenshots JPG. Gibt Inhalt und Dateinamen zurück.
pub fn image_for_proposal(media_dir: &Path, file: &str, typ: &str) -> Result<(Vec<u8>, String)> {
    anyhow::ensure!(
        !file.contains(['/', '\\']) && !file.starts_with('.'),
        "ungültiger Dateiname"
    );
    let img = image::open(media_dir.join(file))
        .with_context(|| tr!("Bild {file} unlesbar", "Cannot read picture {file}"))?;
    let (format, ext) = if typ.starts_with("box") {
        (image::ImageFormat::Png, "png")
    } else {
        (image::ImageFormat::Jpeg, "jpg")
    };
    let mut out = std::io::Cursor::new(Vec::new());
    // JPEG kennt keinen Alphakanal.
    if format == image::ImageFormat::Jpeg {
        image::DynamicImage::ImageRgb8(img.to_rgb8()).write_to(&mut out, format)?;
    } else {
        img.write_to(&mut out, format)?;
    }
    Ok((out.into_inner(), format!("{typ}.{ext}")))
}

/// Bildarten, die selbst gesetzt werden können, und erlaubte Dateiendungen.
const OWN_IMAGE_TYPES: [&str; 4] = ["png", "jpg", "gif", "webp"];
const OWN_IMAGE_MAX: usize = 8 * 1024 * 1024;

/// Legt ein eigenes Bild in den Bilderordner (Name nach Inhalt, also je Bild nur einmal) und
/// setzt es für die Inhalte als Bildart `kind`. `data = None` entfernt das Bild dieser Art.
pub fn set_own_image(
    conn: &Connection,
    sha1s: &[String],
    kind: &str,
    image: Option<(&[u8], &str)>,
    media_dir: &Path,
) -> Result<()> {
    anyhow::ensure!(MEDIA_KINDS.contains(&kind), "unbekannte Bildart {kind}");
    let Some((data, ext)) = image else {
        for sha1 in sha1s {
            conn.execute(
                "DELETE FROM media WHERE sha1 = ? AND kind = ?",
                params![sha1, kind],
            )?;
        }
        return Ok(());
    };
    let ext = ext.to_ascii_lowercase().replace("jpeg", "jpg");
    anyhow::ensure!(
        OWN_IMAGE_TYPES.contains(&ext.as_str()),
        tr!(
            "Nur PNG, JPG, GIF oder WebP als Bild.",
            "Only PNG, JPG, GIF or WebP images."
        )
    );
    anyhow::ensure!(
        data.len() <= OWN_IMAGE_MAX,
        tr!("Bild größer als 8 MB.", "Image larger than 8 MB.")
    );
    use sha1::Digest;
    let name = format!("own_{}.{ext}", &hex::encode(sha1::Sha1::digest(data))[..16]);
    let path = media_dir.join(&name);
    if !path.exists() {
        std::fs::create_dir_all(media_dir)?;
        std::fs::write(&path, data)?;
    }
    for sha1 in sha1s {
        conn.execute(
            "INSERT OR REPLACE INTO media(sha1, kind, file) VALUES(?, ?, ?)",
            params![sha1, kind, name],
        )?;
    }
    Ok(())
}

/// Ordnet ein Bild einer anderen Bildart zu (z. B. versehentlich als Titelbild gespeicherte
/// Spielszene). Liegt dort schon ein Bild, tauschen beide den Platz.
pub fn move_image(conn: &Connection, sha1s: &[String], from: &str, to: &str) -> Result<()> {
    anyhow::ensure!(
        MEDIA_KINDS.contains(&from) && MEDIA_KINDS.contains(&to),
        "unbekannte Bildart"
    );
    if from == to {
        return Ok(());
    }
    let file = |sha1: &str, kind: &str| -> Result<Option<String>> {
        Ok(conn
            .query_row(
                "SELECT file FROM media WHERE sha1 = ? AND kind = ?",
                params![sha1, kind],
                |r| r.get(0),
            )
            .optional()?)
    };
    let tx = conn.unchecked_transaction()?;
    for sha1 in sha1s {
        let (a, b) = (file(sha1, from)?, file(sha1, to)?);
        for (kind, f) in [(to, &a), (from, &b)] {
            match f {
                Some(f) => tx.execute(
                    "INSERT OR REPLACE INTO media(sha1, kind, file) VALUES(?, ?, ?)",
                    params![sha1, kind, f],
                )?,
                None => tx.execute(
                    "DELETE FROM media WHERE sha1 = ? AND kind = ?",
                    params![sha1, kind],
                )?,
            };
        }
    }
    tx.commit()?;
    Ok(())
}

/// Treffer beim Zuordnen aus der eigenen Bibliothek: ein Eintrag mit Angaben (selbst eingetragen,
/// aus der Gemeinschaft oder von ScreenScraper), je Titel und Herkunft einmal.
#[derive(Debug, Clone, Serialize)]
pub struct LibraryHit {
    /// Eine Fassung mit diesen Angaben (Vorlage für `copy_meta`).
    pub sha1: String,
    pub title: String,
    pub year: Option<String>,
    pub publisher: Option<String>,
    pub genres: Option<String>,
    pub synopsis: Option<String>,
    pub provider: String,
    /// Wie viele Fassungen diese Angaben schon tragen.
    pub versions: i64,
}

/// Längste gemeinsame Zeichenfolge zweier (normalisierter) Titel.
fn common_run(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev = vec![0usize; b.len() + 1];
    let mut best = 0;
    for ca in &a {
        let mut cur = vec![0usize; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            if ca == cb {
                cur[j + 1] = prev[j] + 1;
                best = best.max(cur[j + 1]);
            }
        }
        prev = cur;
    }
    best
}

/// Sucht Einträge mit Angaben, deren Titel zu `query` passt (ohne die Inhalte in `exclude`):
/// Titel enthält die Suche (ohne Leer- und Satzzeichen), alle Wörter kommen vor, oder beide teilen
/// ein langes Stück (Schreibvarianten wie „cromoshock“ / „Chromo Shock“). Bessere Treffer zuerst,
/// bei gleicher Güte eigene Angaben vor Gemeinschaft vor ScreenScraper.
pub fn search_library(
    conn: &Connection,
    query: &str,
    exclude: &[String],
    limit: usize,
) -> Result<Vec<LibraryHit>> {
    let words = screenscraper::title_words(query);
    let nq = words.concat();
    if nq.is_empty() {
        return Ok(Vec::new());
    }
    let mut st = conn.prepare(
        "SELECT m.sha1, m.title, m.year, m.publisher, m.genres, m.synopsis, m.provider,
                COUNT(*) OVER (PARTITION BY LOWER(m.title),
                    CASE WHEN m.provider LIKE 'own%' THEN 0 WHEN m.provider = 'shared' THEN 1 ELSE 2 END)
         FROM metadata m WHERE m.status = 'ok' AND m.title IS NOT NULL
         ORDER BY m.provider = 'screenscraper:name', m.sha1",
    )?;
    let rows = st.query_map([], |r| {
        Ok(LibraryHit {
            sha1: r.get(0)?,
            title: r.get(1)?,
            year: r.get(2)?,
            publisher: r.get(3)?,
            genres: r.get(4)?,
            synopsis: r.get(5)?,
            provider: r.get(6)?,
            versions: r.get(7)?,
        })
    })?;
    let need = (nq.chars().count() * 3).div_ceil(5).max(5);
    let mut seen = HashSet::new();
    let mut scored = Vec::new();
    for hit in rows {
        let hit = hit?;
        if exclude.contains(&hit.sha1) {
            continue;
        }
        let tw = screenscraper::title_words(&hit.title);
        let nt = tw.concat();
        let score = if nt == nq {
            0
        } else if nt.contains(&nq) || (nt.chars().count() >= 4 && nq.contains(&nt)) {
            1
        } else if words.iter().all(|w| nt.contains(w.as_str())) {
            2
        } else if common_run(&nq, &nt) >= need {
            3
        } else {
            continue;
        };
        let rank = match hit.provider.as_str() {
            p if p.starts_with("own") => 0,
            crate::share::PROVIDER_SHARED => 1,
            _ => 2,
        };
        if !seen.insert((hit.title.to_lowercase(), rank)) {
            continue;
        }
        scored.push((score, rank, hit.title.to_lowercase(), hit));
    }
    scored.sort_by(|a, b| (a.0, a.1, &a.2).cmp(&(b.0, b.1, &b.2)));
    Ok(scored.into_iter().take(limit).map(|(.., h)| h).collect())
}

/// Überträgt Angaben und Bilder der Fassung `from` auf `to` – so landen verschiedene Dateien
/// desselben Spiels (auch ohne ScreenScraper) in einer Zeile. Eine „Nicht zusammenfassen“-Markierung
/// der Ziele fällt weg. Von ScreenScraper stammende Angaben gelten danach als von Hand zugeordnet.
pub fn copy_meta(conn: &Connection, from: &str, to: &[String]) -> Result<()> {
    type Row = (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<f64>,
        Option<String>,
    );
    let row: Option<Row> = conn
        .query_row(
            "SELECT provider, game_id, title, synopsis, year, publisher, developer, genres, players, rating, synopsis_en
             FROM metadata WHERE sha1 = ? AND status = 'ok'",
            [from],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?)),
        )
        .optional()?;
    let Some((
        provider,
        game_id,
        title,
        synopsis,
        year,
        publisher,
        developer,
        genres,
        players,
        rating,
        synopsis_en,
    )) = row
    else {
        anyhow::bail!(tr!(
            "Der gewählte Eintrag hat keine Angaben mehr.",
            "The chosen entry no longer has details."
        ));
    };
    let provider = if provider.starts_with("screenscraper") {
        PROVIDER_MANUAL.to_string()
    } else {
        provider
    };
    let media: Vec<(String, String)> = conn
        .prepare("SELECT kind, file FROM media WHERE sha1 = ?")?
        .query_map([from], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let tx = conn.unchecked_transaction()?;
    for sha1 in to.iter().filter(|s| s.as_str() != from) {
        tx.execute(
            "INSERT OR REPLACE INTO metadata(sha1, provider, status, game_id, title, synopsis, year, publisher,
                                             developer, genres, players, rating, fetched_at, synopsis_en)
             VALUES(?, ?, 'ok', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![sha1, provider, game_id, title, synopsis, year, publisher, developer, genres, players, rating, now(), synopsis_en],
        )?;
        store_media_rows(&tx, sha1, &media)?;
        tx.execute("DELETE FROM ungrouped WHERE sha1 = ?", [sha1])?;
    }
    tx.commit()?;
    Ok(())
}

/// Markiert die Zuordnung als falsch: Angaben und Bilder weg, die Spielnummer bleibt
/// (für den Link zur Fehlermeldung bei ScreenScraper). Kein Abruf holt sie zurück.
pub fn reject(conn: &Connection, sha1s: &[String]) -> Result<()> {
    for sha1 in sha1s {
        conn.execute("DELETE FROM media WHERE sha1 = ?", [sha1])?;
        let game_id: Option<String> = conn
            .query_row(
                "SELECT game_id FROM metadata WHERE sha1 = ? AND status = 'ok'",
                [sha1],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        conn.execute(
            "INSERT OR REPLACE INTO metadata(sha1, provider, status, game_id, fetched_at) VALUES(?, 'screenscraper', 'rejected', ?, ?)",
            params![sha1, game_id, now()],
        )?;
    }
    Ok(())
}

/// Nimmt „falsch“ zurück: Der Inhalt gilt wieder als nicht nachgeschlagen.
pub fn unreject(conn: &Connection, sha1s: &[String]) -> Result<()> {
    for sha1 in sha1s {
        conn.execute(
            "DELETE FROM metadata WHERE sha1 = ? AND status = 'rejected'",
            [sha1],
        )?;
    }
    Ok(())
}

/// Ordnet die Inhalte dem vom Nutzer gewählten Spiel zu, mit Bildern. Ein Fehler beim
/// Laden der Bilder lässt die Zuordnung stehen und kommt als Text zurück.
pub fn assign(
    conn: &Connection,
    src: &mut dyn GameSource,
    sha1s: &[String],
    g: &Game,
    media_dir: &Path,
) -> Result<Option<String>> {
    let mut media_error = None;
    for sha1 in sha1s {
        store_game(conn, sha1, g, PROVIDER_MANUAL)?;
        // Nach einem Fehler nicht für jede Fassung erneut versuchen.
        if media_error.is_none()
            && let Err(e) = store_media(conn, src, sha1, g, media_dir)
        {
            media_error = Some(tr!("Bilder: {e:#}", "pictures: {e:#}"));
        }
    }
    Ok(media_error)
}

fn store_game(conn: &Connection, sha1: &str, g: &Game, provider: &str) -> Result<()> {
    let genres = (!g.genres.is_empty()).then(|| g.genres.join(", "));
    conn.execute(
        "INSERT OR REPLACE INTO metadata(sha1, provider, status, game_id, title, synopsis, year, publisher,
                                         developer, genres, players, rating, fetched_at)
         VALUES(?, ?, 'ok', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![sha1, provider, g.id, g.title, g.synopsis, g.year, g.publisher, g.developer, genres, g.players, g.rating, now()],
    )?;
    Ok(())
}

/// Dateiname eines Bildes; je Spiel und Art nur einmal geladen.
fn media_file_name(game_id: &str, kind: &str, format: &str) -> String {
    let safe = |s: &str| {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect::<String>()
    };
    let ext = if format.eq_ignore_ascii_case("jpg") || format.eq_ignore_ascii_case("jpeg") {
        "jpg"
    } else {
        "png"
    };
    format!("ss_{}_{}.{ext}", safe(game_id), safe(kind))
}

/// Lädt die Bilder eines Spiels in den Bilderordner (je Spiel und Art nur einmal). Gibt die
/// geladenen Bildarten mit Dateinamen zurück und, falls ein Bild scheiterte, den Fehler.
fn fetch_media(
    src: &mut dyn GameSource,
    g: &Game,
    dir: &Path,
) -> (Vec<(String, String)>, Option<anyhow::Error>) {
    let mut out = Vec::new();
    for kind in MEDIA_KINDS {
        let Some(m) = screenscraper::best_media(g, kind) else {
            continue;
        };
        let name = media_file_name(&g.id, kind, &m.format);
        let path = dir.join(&name);
        if !path.exists() {
            let saved = src.download(&m.url).and_then(|data| {
                std::fs::create_dir_all(dir)?;
                // Erst unter anderem Namen schreiben: Ein zweiter Thread mit demselben Spiel sieht
                // nie eine halbe Datei.
                let tmp = dir.join(
                    format!("{name}.{:?}.part", std::thread::current().id())
                        .replace(['(', ')'], ""),
                );
                std::fs::write(&tmp, data)?;
                std::fs::rename(&tmp, &path)?;
                Ok(())
            });
            if let Err(e) = saved {
                return (out, Some(e));
            }
        }
        out.push((kind.to_string(), name));
    }
    (out, None)
}

fn store_media_rows(conn: &Connection, sha1: &str, media: &[(String, String)]) -> Result<()> {
    conn.execute("DELETE FROM media WHERE sha1 = ?", [sha1])?;
    for (kind, name) in media {
        conn.execute(
            "INSERT OR REPLACE INTO media(sha1, kind, file) VALUES(?, ?, ?)",
            params![sha1, kind, name],
        )?;
    }
    Ok(())
}

fn store_media(
    conn: &Connection,
    src: &mut dyn GameSource,
    sha1: &str,
    g: &Game,
    dir: &Path,
) -> Result<()> {
    let (media, error) = fetch_media(src, g, dir);
    store_media_rows(conn, sha1, &media)?;
    error.map_or(Ok(()), Err)
}

/// Wartet in kleinen Schritten, damit ein Abbruch sofort greift.
fn pause(d: Duration, cancel: &AtomicBool) {
    let end = std::time::Instant::now() + d;
    while std::time::Instant::now() < end && !cancel.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Ergebnis der Netzarbeit für einen Inhalt, in einem Arbeits-Thread ermittelt; gespeichert
/// wird im aufrufenden Thread (eine Datenbankverbindung).
enum Fetched {
    NotFound,
    /// Über die Prüfsumme nicht gefunden, die Namenssuche scheiterte (Fehlertext): zählt als
    /// „nicht gefunden“ und hält den Lauf nicht an.
    NotFoundNameError(String),
    Found {
        game: Box<Game>,
        provider: &'static str,
        media: Vec<(String, String)>,
        media_error: Option<anyhow::Error>,
    },
}

/// Nachrichten der Arbeits-Threads an den speichernden Thread.
enum Msg {
    Started(usize),
    Done(usize, Result<Fetched>),
}

/// Abfrage, ggf. Namenssuche und Bilder für einen Inhalt. „Ausgelastet“ wird nach einer Pause
/// wiederholt; dabei schaltet der Lauf einen Thread zurück (`allowed`).
fn fetch_one(
    src: &mut dyn GameSource,
    rom: &RomQuery,
    opts: &Options,
    cancel: &AtomicBool,
    allowed: &AtomicUsize,
) -> Result<Fetched> {
    let mut attempt = 0;
    let lookup = loop {
        match src.game_info(rom, opts.lang) {
            Ok(l) => break l,
            Err(e) => match e.downcast_ref::<ApiError>() {
                Some(ApiError::Busy(_)) if attempt < BUSY_RETRIES => {
                    attempt += 1;
                    let n = allowed.load(Ordering::Relaxed);
                    if n > 1 {
                        allowed.store(n - 1, Ordering::Relaxed);
                    }
                    pause(BUSY_PAUSE * attempt, cancel);
                }
                _ => return Err(e),
            },
        }
    };
    // Ohne Treffer über die Prüfsumme: hilfsweise über den Namen.
    let (lookup, provider) = match lookup {
        Lookup::NotFound if opts.name_fallback && !rom.title.is_empty() => {
            match src.by_name(&rom.title, opts.lang) {
                Ok(Some(g)) => (Lookup::Found(Box::new(g)), PROVIDER_NAME_MATCH),
                Ok(None) => (Lookup::NotFound, "screenscraper"),
                Err(e) => match e.downcast_ref::<ApiError>() {
                    Some(fatal) if !matches!(fatal, ApiError::Http(..) | ApiError::Busy(_)) => {
                        return Err(e);
                    }
                    // Nur der Zusatzversuch scheiterte: als „nicht gefunden“ werten, Fehler vermerken.
                    _ => {
                        return Ok(Fetched::NotFoundNameError(tr!(
                            "Namenssuche „{}“: {e:#}",
                            "name search “{}”: {e:#}",
                            rom.title
                        )));
                    }
                },
            }
        }
        // Treffer ohne passende Prüfsumme: wie ein Namenstreffer kennzeichnen („bitte prüfen“).
        Lookup::Found(g) if g.name_only => (Lookup::Found(g), PROVIDER_NAME_MATCH),
        other => (other, "screenscraper"),
    };
    Ok(match lookup {
        Lookup::NotFound => Fetched::NotFound,
        Lookup::Found(game) => {
            let (media, media_error) = fetch_media(src, &game, opts.media_dir);
            Fetched::Found {
                game,
                provider,
                media,
                media_error,
            }
        }
    })
}

/// Fragt die Inhalte mit einer Quelle nacheinander ab (siehe [`run_with`]).
pub fn run(
    conn: &Connection,
    src: &mut dyn GameSource,
    opts: &Options,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(&Progress),
) -> Result<Progress> {
    run_with(conn, vec![src], opts, cancel, report)
}

/// Fragt die Inhalte ab, je Quelle in einem eigenen Thread (so viele, wie das Konto bei
/// ScreenScraper gleichzeitig erlaubt). Gespeichert wird nur in diesem Thread. Fehler, die jede
/// weitere Anfrage sinnlos machen (Zugang, Kontingent, gesperrte API), beenden den Lauf mit
/// Grund in `stopped`; Anfragen, die schon unterwegs sind, werden noch gespeichert.
pub fn run_with(
    conn: &Connection,
    sources: Vec<&mut dyn GameSource>,
    opts: &Options,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(&Progress),
) -> Result<Progress> {
    let todo = pending(conn, opts)?;
    let mut p = Progress {
        total: todo.len() as u64,
        ..Default::default()
    };
    report(&p);
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let allowed = AtomicUsize::new(sources.len().max(1));
    p.threads = sources.len().max(1) as u32;
    let (tx, rx) = std::sync::mpsc::channel::<Msg>();

    let mut fatal_db: Option<anyhow::Error> = None;
    std::thread::scope(|scope| {
        for (i, src) in sources.into_iter().enumerate() {
            let (tx, todo, next, stop, allowed) = (tx.clone(), &todo, &next, &stop, &allowed);
            scope.spawn(move || {
                loop {
                    if cancel.load(Ordering::Relaxed)
                        || stop.load(Ordering::Relaxed)
                        || i >= allowed.load(Ordering::Relaxed)
                    {
                        break;
                    }
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(rom) = todo.get(k) else { break };
                    let _ = tx.send(Msg::Started(k));
                    let r = fetch_one(src, rom, opts, cancel, allowed);
                    // Zugang, Kontingent, gesperrte API: sofort keine neuen Anfragen mehr.
                    let fatal = match &r {
                        Err(e) => e
                            .downcast_ref::<ApiError>()
                            .is_some_and(|a| !matches!(a, ApiError::Http(..))),
                        Ok(Fetched::Found {
                            media_error: Some(e),
                            ..
                        }) => {
                            matches!(
                                e.downcast_ref::<ApiError>(),
                                Some(ApiError::Quota | ApiError::Closed(_) | ApiError::DevLogin(_))
                            )
                        }
                        Ok(_) => false,
                    };
                    if fatal {
                        stop.store(true, Ordering::Relaxed);
                    }
                    if tx.send(Msg::Done(k, r)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);

        let mut errors_in_row = 0;
        for msg in rx {
            let (k, result) = match msg {
                Msg::Started(k) => {
                    p.current = todo[k].file_name.clone();
                    p.threads = allowed.load(Ordering::Relaxed) as u32;
                    report(&p);
                    continue;
                }
                Msg::Done(k, r) => (k, r),
            };
            let rom = &todo[k];
            let stopped = p.stopped.is_some() || fatal_db.is_some();
            let outcome: Result<bool> = result.and_then(|f| match f {
                Fetched::NotFound => store_not_found(conn, &rom.sha1).map(|_| false),
                Fetched::NotFoundNameError(msg) => {
                    p.errors.push(format!("{}: {msg}", rom.file_name));
                    store_not_found(conn, &rom.sha1).map(|_| false)
                }
                Fetched::Found {
                    game,
                    provider,
                    media,
                    media_error,
                } => {
                    store_game(conn, &rom.sha1, &game, provider)?;
                    store_media_rows(conn, &rom.sha1, &media)?;
                    if let Some(e) = media_error {
                        // Bilder: ein Fehler hier lässt die Beschreibung stehen.
                        if let Some(
                            fatal @ (ApiError::Quota | ApiError::Closed(_) | ApiError::DevLogin(_)),
                        ) = e.downcast_ref::<ApiError>()
                        {
                            return Err(anyhow::anyhow!(fatal.clone()));
                        }
                        p.errors.push(tr!(
                            "{}: Bilder: {e:#}",
                            "{}: pictures: {e:#}",
                            rom.file_name
                        ));
                    }
                    Ok(true)
                }
            });
            match outcome {
                Ok(found) => {
                    errors_in_row = 0;
                    if found {
                        p.found += 1
                    } else {
                        p.not_found += 1
                    }
                }
                // Nach dem Anhalten zählen Fehler der restlichen Anfragen nicht mehr.
                Err(_) if stopped => {}
                Err(e) => {
                    if let Some(fatal) = e
                        .downcast_ref::<ApiError>()
                        .filter(|a| !matches!(a, ApiError::Http(..)))
                    {
                        p.stopped = Some(fatal.to_string());
                        stop.store(true, Ordering::Relaxed);
                    } else if e.downcast_ref::<rusqlite::Error>().is_some() {
                        // Die Datenbank streikt: anhalten und den Fehler weitergeben.
                        stop.store(true, Ordering::Relaxed);
                        fatal_db = Some(e);
                    } else {
                        p.errors.push(format!("{}: {e:#}", rom.file_name));
                        errors_in_row += 1;
                        if errors_in_row >= MAX_ERRORS_IN_ROW {
                            p.stopped = Some(format!(
                                "{MAX_ERRORS_IN_ROW} Fehler hintereinander, zuletzt: {e:#}"
                            ));
                            stop.store(true, Ordering::Relaxed);
                        }
                    }
                }
            }
            p.done += 1;
            report(&p);
        }
    });
    if let Some(e) = fatal_db {
        return Err(e);
    }
    p.cancelled = cancel.load(Ordering::Relaxed) && p.stopped.is_none() && p.done < p.total;
    p.current.clear();
    report(&p);
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::screenscraper::Media;
    use crate::{db, scan};

    struct Fake {
        calls: Vec<String>,
        downloads: u32,
        /// Antwort je Dateiname; fehlt der Name → nicht gefunden.
        answers: Vec<(&'static str, Result<Game, ApiError>)>,
    }

    impl GameSource for Fake {
        fn game_info(&mut self, rom: &RomQuery, _lang: &str) -> Result<Lookup> {
            self.calls.push(rom.file_name.clone());
            match self.answers.iter().find(|(n, _)| *n == rom.file_name) {
                Some((_, Ok(g))) => Ok(Lookup::Found(Box::new(g.clone()))),
                Some((_, Err(e))) => Err(e.clone().into()),
                None => Ok(Lookup::NotFound),
            }
        }
        fn download(&mut self, _url: &str) -> Result<Vec<u8>> {
            self.downloads += 1;
            Ok(b"\x89PNG".to_vec())
        }
        fn by_name(&mut self, title: &str, _lang: &str) -> Result<Option<Game>> {
            self.calls.push(format!("name:{title}"));
            Ok((title == "katakis").then(|| game("99", "Katakis")))
        }
    }

    fn game(id: &str, title: &str) -> Game {
        Game {
            id: id.into(),
            title: Some(title.into()),
            synopsis: Some("Text".into()),
            genres: vec!["Plattform".into(), "Action".into()],
            media: vec![Media {
                kind: "ss".into(),
                region: "wor".into(),
                url: "u".into(),
                format: "png".into(),
            }],
            ..Default::default()
        }
    }

    fn setup(files: &[&str]) -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        for (i, f) in files.iter().enumerate() {
            std::fs::write(dir.path().join(f), [1, 8, i as u8]).unwrap();
        }
        let conn = db::open_in_memory().unwrap();
        let id = db::add_local_source(&conn, dir.path()).unwrap();
        scan::scan_local(&conn, id, &AtomicBool::new(false), &mut |_| {}).unwrap();
        (dir, conn)
    }

    fn opts(media: &Path) -> Options<'_> {
        Options {
            sha1s: None,
            force: false,
            retry_notfound: false,
            lang: "de",
            media_dir: media,
            name_fallback: false,
        }
    }

    #[test]
    fn abruf_speichert_angaben_und_bilder_einmal_je_spiel() {
        let (_d, conn) = setup(&["giana.prg", "giana2.prg", "unbekannt.prg"]);
        let media = tempfile::tempdir().unwrap();
        let mut src = Fake {
            calls: vec![],
            downloads: 0,
            answers: vec![
                ("giana.prg", Ok(game("7", "Great Giana Sisters"))),
                ("giana2.prg", Ok(game("7", "Great Giana Sisters"))),
            ],
        };
        let p = run(
            &conn,
            &mut src,
            &opts(media.path()),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!((p.total, p.found, p.not_found, p.stopped), (3, 2, 1, None));
        assert_eq!(src.downloads, 1, "gleiches Spiel → Bild nur einmal laden");

        let rows = db::search(&conn, "plattform", None, 10).unwrap();
        assert_eq!(rows.len(), 2, "Suche findet Genre aus den Angaben");
        assert_eq!(rows[0].title, "Great Giana Sisters");
        let d = db::details(&conn, &rows[0].sha1).unwrap();
        assert_eq!(d.meta.unwrap().synopsis.as_deref(), Some("Text"));
        assert_eq!(d.media, [("ss".to_string(), "ss_7_ss.png".to_string())]);

        // Zweiter Lauf: nichts mehr offen; mit retry_notfound nur der nicht gefundene.
        assert_eq!(
            run(
                &conn,
                &mut src,
                &opts(media.path()),
                &AtomicBool::new(false),
                &mut |_| {}
            )
            .unwrap()
            .total,
            0
        );
        let o = Options {
            retry_notfound: true,
            ..opts(media.path())
        };
        assert_eq!(pending(&conn, &o).unwrap().len(), 1);
    }

    #[test]
    fn zu_kurze_namen_werden_nicht_gesucht() {
        // ScreenScraper lehnt Suchbegriffe unter 3 Zeichen ab; ohne Netz: kein Aufruf, kein Fehler.
        let mut c = screenscraper::Client::new(Default::default(), "test");
        assert!(GameSource::by_name(&mut c, "4K", "de").unwrap().is_none());
        assert!(GameSource::by_name(&mut c, "!7", "de").unwrap().is_none());
        assert!(
            c.search_name("IT", "de")
                .unwrap_err()
                .to_string()
                .contains('3')
        );
    }

    #[test]
    fn mehrere_threads_arbeiten_alles_ab() {
        let names = [
            "a.prg", "b.prg", "c.prg", "d.prg", "e.prg", "f.prg", "g.prg", "h.prg",
        ];
        let (_d, conn) = setup(&names);
        let media = tempfile::tempdir().unwrap();
        let answers = || -> Vec<(&'static str, Result<Game, ApiError>)> {
            vec![
                ("a.prg", Ok(game("1", "Eins"))),
                ("c.prg", Ok(game("1", "Eins"))),
                ("e.prg", Ok(game("5", "Fünf"))),
            ]
        };
        let mut srcs: Vec<Fake> = (0..3)
            .map(|_| Fake {
                calls: vec![],
                downloads: 0,
                answers: answers(),
            })
            .collect();
        let sources: Vec<&mut dyn GameSource> =
            srcs.iter_mut().map(|s| s as &mut dyn GameSource).collect();
        let mut reports = 0;
        let p = run_with(
            &conn,
            sources,
            &opts(media.path()),
            &AtomicBool::new(false),
            &mut |_| reports += 1,
        )
        .unwrap();
        assert_eq!(
            (
                p.total,
                p.done,
                p.found,
                p.not_found,
                p.stopped,
                p.cancelled
            ),
            (8, 8, 3, 5, None, false)
        );
        let mut all: Vec<String> = srcs.iter().flat_map(|s| s.calls.clone()).collect();
        all.sort();
        assert_eq!(all, names, "jede Datei genau einmal abgefragt");
        assert!(reports > 8);
        assert_eq!(db::search(&conn, "Eins", None, 10).unwrap().len(), 2);
        assert!(
            media.path().join("ss_1_ss.png").exists() && media.path().join("ss_5_ss.png").exists()
        );
        assert!(
            std::fs::read_dir(media.path()).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".part"))
        );
    }

    #[test]
    fn treffer_ohne_passende_pruefsumme_gilt_als_namenstreffer() {
        let (_d, conn) = setup(&["warlock.prg"]);
        let media = tempfile::tempdir().unwrap();
        let wrong = Game {
            name_only: true,
            ..game("109757", "Warlock's Quest")
        };
        let mut src = Fake {
            calls: vec![],
            downloads: 0,
            answers: vec![("warlock.prg", Ok(wrong))],
        };
        let p = run(
            &conn,
            &mut src,
            &opts(media.path()),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(p.found, 1);
        let sha1 = db::search(&conn, "", None, 10).unwrap()[0].sha1.clone();
        assert_eq!(
            db::details(&conn, &sha1).unwrap().meta.unwrap().provider,
            PROVIDER_NAME_MATCH
        );
    }

    #[test]
    fn uebernommene_angaben_weichen_screenscraper() {
        let (_d, conn) = setup(&["giana.prg", "neu.prg"]);
        let media = tempfile::tempdir().unwrap();
        // Nur wenn ScreenScraper die bevorzugte Quelle ist (sonst siehe share::tests).
        db::set_setting(&conn, crate::share::PREFERRED_SOURCE, "screenscraper").unwrap();
        for r in db::search(&conn, "", None, 10).unwrap() {
            conn.execute(
                "INSERT INTO metadata(sha1, provider, status, title, fetched_at) VALUES(?, 'shared', 'ok', 'Geteilt', 0)",
                [&r.sha1],
            )
            .unwrap();
        }
        let mut src = Fake {
            calls: vec![],
            downloads: 0,
            answers: vec![("giana.prg", Ok(game("7", "Great Giana Sisters")))],
        };
        assert_eq!(
            run(
                &conn,
                &mut src,
                &opts(media.path()),
                &AtomicBool::new(false),
                &mut |_| {}
            )
            .unwrap()
            .total,
            0,
            "ohne Wiederholung nichts"
        );
        let o = Options {
            retry_notfound: true,
            ..opts(media.path())
        };
        let p = run(&conn, &mut src, &o, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!((p.found, p.not_found), (1, 1));
        assert_eq!(
            db::search(&conn, "Great Giana", None, 10).unwrap().len(),
            1,
            "ScreenScraper ersetzt"
        );
        let rows = db::search(&conn, "Geteilt", None, 10).unwrap();
        assert_eq!(
            db::details(&conn, &rows[0].sha1)
                .unwrap()
                .meta
                .unwrap()
                .provider,
            "shared",
            "nicht gefunden: bleibt"
        );
    }

    #[test]
    fn suche_ueber_den_namen_als_rueckfall() {
        let (_d, conn) = setup(&["katakis.prg", "unbekannt.prg"]);
        let media = tempfile::tempdir().unwrap();
        let mut src = Fake {
            calls: vec![],
            downloads: 0,
            answers: vec![],
        };
        let o = Options {
            name_fallback: true,
            ..opts(media.path())
        };
        let p = run(&conn, &mut src, &o, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!((p.found, p.not_found), (1, 1));
        assert_eq!(
            src.calls,
            [
                "katakis.prg",
                "name:katakis",
                "unbekannt.prg",
                "name:unbekannt"
            ]
        );
        let rows = db::search(&conn, "Katakis", None, 10).unwrap();
        let d = db::details(&conn, &rows[0].sha1).unwrap();
        assert_eq!(d.meta.unwrap().provider, PROVIDER_NAME_MATCH);
    }

    #[test]
    fn kontingent_beendet_den_lauf() {
        let (_d, conn) = setup(&["a.prg", "b.prg", "c.prg"]);
        let media = tempfile::tempdir().unwrap();
        let mut src = Fake {
            calls: vec![],
            downloads: 0,
            answers: vec![("b.prg", Err(ApiError::Quota))],
        };
        let p = run(
            &conn,
            &mut src,
            &opts(media.path()),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(src.calls, ["a.prg", "b.prg"]);
        assert!(p.stopped.unwrap().contains("Tageskontingent"));
        assert_eq!(
            pending(&conn, &opts(media.path())).unwrap().len(),
            2,
            "b und c bleiben offen"
        );
    }

    #[test]
    fn auswahl_und_erzwingen() {
        let (_d, conn) = setup(&["a.prg", "b.prg"]);
        let media = tempfile::tempdir().unwrap();
        let all = pending(&conn, &opts(media.path())).unwrap();
        let one = vec![all[0].sha1.clone()];
        let mut src = Fake {
            calls: vec![],
            downloads: 0,
            answers: vec![],
        };
        let o = Options {
            sha1s: Some(one.clone()),
            ..opts(media.path())
        };
        run(&conn, &mut src, &o, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(src.calls, ["a.prg"]);
        // Schon abgefragt: ohne force nicht noch einmal, mit force schon.
        assert!(pending(&conn, &o).unwrap().is_empty());
        let o = Options {
            sha1s: Some(one),
            force: true,
            ..opts(media.path())
        };
        assert_eq!(pending(&conn, &o).unwrap().len(), 1);
    }

    #[test]
    fn ablehnen_und_von_hand_zuordnen_uebersteht_jeden_abruf() {
        let (_d, conn) = setup(&["a.prg", "b.prg"]);
        let media = tempfile::tempdir().unwrap();
        let mut src = Fake {
            calls: vec![],
            downloads: 0,
            answers: vec![
                ("a.prg", Ok(game("7", "Falsch"))),
                ("b.prg", Ok(game("7", "Falsch"))),
            ],
        };
        run(
            &conn,
            &mut src,
            &opts(media.path()),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        let all: Vec<String> = db::search(&conn, "", None, 10)
            .unwrap()
            .into_iter()
            .map(|r| r.sha1)
            .collect();
        let (a, b) = (vec![all[0].clone()], vec![all[1].clone()]);

        reject(&conn, &a).unwrap();
        let d = db::details(&conn, &a[0]).unwrap();
        let m = d.meta.unwrap();
        assert_eq!(
            (m.status.as_str(), m.game_id.as_deref(), m.title),
            ("rejected", Some("7"), None)
        );
        assert!(d.media.is_empty());
        assert_eq!(
            d.row.game_id, None,
            "abgelehnt zählt nicht fürs Zusammenfassen"
        );

        assign(&conn, &mut src, &b, &game("8", "Richtig"), media.path()).unwrap();
        assert_eq!(
            db::details(&conn, &b[0]).unwrap().meta.unwrap().provider,
            PROVIDER_MANUAL
        );

        let force = Options {
            force: true,
            retry_notfound: true,
            ..opts(media.path())
        };
        assert!(
            pending(&conn, &force).unwrap().is_empty(),
            "weder abgelehnt noch von Hand zugeordnet wird neu geholt"
        );

        unreject(&conn, &a).unwrap();
        assert!(db::details(&conn, &a[0]).unwrap().meta.is_none());
        assert_eq!(pending(&conn, &opts(media.path())).unwrap().len(), 1);
    }

    #[test]
    fn aus_der_bibliothek_zuordnen() {
        let (_d, conn) = setup(&["chromo.prg", "cromoshock.d64"]);
        let media = tempfile::tempdir().unwrap();
        let mut all: Vec<String> = db::search(&conn, "", None, 10)
            .unwrap()
            .into_iter()
            .map(|r| r.sha1)
            .collect();
        all.sort();
        let (src, dst) = (vec![all[0].clone()], vec![all[1].clone()]);
        let m = OwnMeta {
            title: "Chromo Shock Reloaded".into(),
            year: "2026".into(),
            ..Default::default()
        };
        save_own(&conn, &src, &m, true).unwrap();
        set_own_image(
            &conn,
            &src,
            "sstitle",
            Some((b"\x89PNG", "png")),
            media.path(),
        )
        .unwrap();
        db::set_ungrouped(&conn, &dst[0], true).unwrap();

        let hits = search_library(&conn, "shock", &dst, 10).unwrap();
        assert_eq!(
            (
                hits.len(),
                hits[0].title.as_str(),
                hits[0].provider.as_str()
            ),
            (1, "Chromo Shock Reloaded", PROVIDER_OWN)
        );
        assert!(
            search_library(&conn, "shock", &src, 10).unwrap().is_empty(),
            "eigene Fassung nicht als Treffer"
        );
        assert!(search_library(&conn, "100%", &[], 10).unwrap().is_empty());
        assert_eq!(
            search_library(&conn, "cromoshock", &dst, 10).unwrap().len(),
            1,
            "Schreibvariante"
        );
        assert!(
            search_library(&conn, "Giana Sisters", &dst, 10)
                .unwrap()
                .is_empty()
        );

        copy_meta(&conn, &hits[0].sha1, &dst).unwrap();
        let (a, b) = (
            db::details(&conn, &src[0]).unwrap(),
            db::details(&conn, &dst[0]).unwrap(),
        );
        assert_eq!(
            b.meta.as_ref().unwrap().title.as_deref(),
            Some("Chromo Shock Reloaded")
        );
        assert_eq!(b.meta.unwrap().provider, PROVIDER_OWN);
        assert_eq!(a.media, b.media);
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM ungrouped", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0, "Zusammenfassen hebt „nicht zusammenfassen“ auf");
        assert_eq!(
            search_library(&conn, "chromo", &[], 10).unwrap()[0].versions,
            2
        );
    }

    #[test]
    fn eigene_angaben_und_bilder() {
        let (_d, conn) = setup(&["mein.prg"]);
        let media = tempfile::tempdir().unwrap();
        let sha1 = vec![db::search(&conn, "", None, 10).unwrap()[0].sha1.clone()];
        let m = OwnMeta {
            title: "Space Gun (Arbeitsstand)".into(),
            year: "2026".into(),
            genres: " ".into(),
            ..Default::default()
        };
        save_own(&conn, &sha1, &m, false).unwrap();
        set_own_image(&conn, &sha1, "ss", Some((b"\x89PNG", "PNG")), media.path()).unwrap();
        assert!(set_own_image(&conn, &sha1, "ss", Some((b"x", "exe")), media.path()).is_err());

        let rows = db::search(&conn, "arbeitsstand", None, 10).unwrap();
        assert_eq!(
            (rows[0].year.as_deref(), rows[0].genres.as_deref()),
            (Some("2026"), None),
            "leeres Feld = unbekannt"
        );
        let d = db::details(&conn, &sha1[0]).unwrap();
        assert_eq!(d.meta.unwrap().provider, PROVIDER_OWN);
        assert!(d.media[0].1.starts_with("own_") && d.media[0].1.ends_with(".png"));
        let force = Options {
            force: true,
            retry_notfound: true,
            ..opts(media.path())
        };
        assert!(
            pending(&conn, &force).unwrap().is_empty(),
            "eigene Angaben überschreibt kein Abruf"
        );

        // Für einen Vorschlag: Screenshot als JPG, Verpackung als PNG.
        let png = media.path().join("t.png");
        image::RgbaImage::new(4, 3).save(&png).unwrap();
        let (data, name) = image_for_proposal(media.path(), "t.png", "ss").unwrap();
        assert_eq!((name.as_str(), &data[..2]), ("ss.jpg", &[0xFF, 0xD8][..]));
        let (data, name) = image_for_proposal(media.path(), "t.png", "box-2D").unwrap();
        assert_eq!((name.as_str(), &data[1..4]), ("box-2D.png", &b"PNG"[..]));
        assert!(image_for_proposal(media.path(), "../t.png", "ss").is_err());

        // Umordnen: Spielszene → Titelbild (leer), dann tauschen mit neuer Spielszene.
        let kinds = |conn: &Connection| {
            let mut m = db::details(conn, &sha1[0]).unwrap().media;
            m.sort();
            m
        };
        let first = kinds(&conn)[0].1.clone();
        move_image(&conn, &sha1, "ss", "sstitle").unwrap();
        assert_eq!(kinds(&conn), vec![("sstitle".to_string(), first.clone())]);
        set_own_image(&conn, &sha1, "ss", Some((b"\x89PNG2", "png")), media.path()).unwrap();
        let second = kinds(&conn)[0].1.clone();
        move_image(&conn, &sha1, "sstitle", "ss").unwrap();
        assert_eq!(
            kinds(&conn),
            vec![("ss".to_string(), first), ("sstitle".to_string(), second)]
        );
        assert!(move_image(&conn, &sha1, "ss", "foo").is_err());

        clear(&conn, &sha1).unwrap();
        let d = db::details(&conn, &sha1[0]).unwrap();
        assert!(d.meta.is_none() && d.media.is_empty());
    }
}
