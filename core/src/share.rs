//! Eigene Angaben mit anderen teilen: Austauschdatei (ZIP) schreiben und einlesen.
//!
//! Aufbau, zugleich der Aufbau der späteren Gemeinschaftsdatenbank (Repository):
//! `breadbin-share.json` (Kopf), je Spiel `games/<ab>/<sha1>.json` und je Bild
//! `images/<ab>/<sha1 des Bildes>.<ext>` – Schlüssel sind Prüfsummen, keine laufenden Nummern.
//! Geteilt wird nur Selbsterstelltes: selbst geschriebene Angaben (nicht die auf Grundlage von
//! ScreenScraper bearbeiteten), ohne Bilder von ScreenScraper.
//!
//! Vorrang: eigene Angaben, dann Gemeinschaft, dann ScreenScraper (Einstellung `preferred_source`;
//! mit `screenscraper` geht ScreenScraper der Gemeinschaft vor).

use crate::metadata::{self, MEDIA_KINDS, PROVIDER_OWN};
use crate::tr;
use anyhow::{Context, Result, bail, ensure};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Kennung und Version des Formats; neuere Versionen liest diese Fassung nicht.
pub const FORMAT: &str = "breadbin-share";
pub const VERSION: u32 = 1;
/// Lizenz der geteilten Angaben.
pub const LICENSE: &str = "CC-BY-SA-4.0";
/// Von anderen übernommene Angaben (Gemeinschaft).
pub const PROVIDER_SHARED: &str = "shared";
/// Einstellung: bevorzugte Quelle, `community` (Standard) oder `screenscraper`.
pub const PREFERRED_SOURCE: &str = "preferred_source";

/// Gehen Gemeinschaftsangaben denen von ScreenScraper vor?
pub fn prefer_shared(conn: &Connection) -> Result<bool> {
    Ok(crate::db::get_setting(conn, PREFERRED_SOURCE)?.as_deref() != Some("screenscraper"))
}

const MANIFEST: &str = "breadbin-share.json";
const MAX_JSON: u64 = 256 * 1024;
const MAX_IMAGE: u64 = 8 * 1024 * 1024;
const MAX_GAMES: usize = 200_000;
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
/// Wo Breadbin die Gemeinschaftsdaten abholt: Spiegel des Daten-Repositorys auf 8bitcavern.de
/// (spiegel.yml lädt jeden neuen Stand hoch). Seit 0.7.0 über die Subdomain; sie zeigt auf denselben Ordner wie
/// die frühere Adresse `https://8bitcavern.de/breadbin/db/breadbindb/…`, die ältere Fassungen weiter benutzen.
pub const DEFAULT_COMMUNITY_URL: &str = "https://breadbindb.8bitcavern.de/breadbin-db.zip";
/// Eingebaute Adresse bis 0.6.3 (weiter erreichbar, gleicher Ordner).
pub const LEGACY_COMMUNITY_URL: &str =
    "https://8bitcavern.de/breadbin/db/breadbindb/breadbin-db.zip";
/// Wohin „Beitrag einsenden“ schickt: Annahme-Server auf 8bitcavern.de (Daten-Repository `server/`), seit 0.8.2
/// über die Subdomain (zeigt auf denselben Ordner wie die frühere Adresse).
pub const DEFAULT_SUBMIT_URL: &str = "https://breadbindbsubmit.8bitcavern.de/beitrag.php";
/// Eingebaute Adresse für Beiträge bis 0.8.1 (weiter erreichbar, gleicher Ordner).
pub const LEGACY_SUBMIT_URL: &str =
    "https://8bitcavern.de/breadbin/db/breadbin/annahme/public/api/beitrag.php";
const IMAGE_EXTS: [&str; 4] = ["png", "jpg", "gif", "webp"];

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    /// Unix-Zeit der Erstellung.
    pub created: i64,
    pub app: String,
    pub license: String,
    pub games: usize,
}

fn is_empty(s: &str) -> bool {
    s.is_empty()
}

/// Ein Spiel (genauer: ein Inhalt mit seinen Prüfsummen) mit den eigenen Angaben.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SharedGame {
    pub version: u32,
    pub sha1: String,
    pub md5: String,
    pub crc32: String,
    pub size: i64,
    pub format: String,
    /// Dateinamen, unter denen der Inhalt bekannt ist (zum Wiedererkennen, ohne Ordner).
    pub files: Vec<String>,
    /// Spielnummer bei ScreenScraper, falls bekannt (zum Zusammenfassen von Fassungen).
    #[serde(skip_serializing_if = "is_empty")]
    pub screenscraper_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub year: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub publisher: String,
    #[serde(skip_serializing_if = "is_empty")]
    pub developer: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
    #[serde(skip_serializing_if = "is_empty")]
    pub players: String,
    /// Beschreibung: Deutsch bzw. die einzige vorhandene Sprache (so lesen es auch Fassungen vor 0.8.0).
    #[serde(skip_serializing_if = "is_empty")]
    pub synopsis: String,
    /// Beschreibung je Sprache (`de`, `en`); nur geschrieben, wenn es eine englische gibt, damit der Fingerabdruck
    /// einsprachiger Einträge gleich bleibt. Ältere Fassungen überlesen das Feld.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub texts: BTreeMap<String, String>,
    /// Bildart (`sstitle`, `ss`, `box-2D`) → Pfad im Archiv.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub images: BTreeMap<String, String>,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct ImportReport {
    /// Spiele in der Datei.
    pub games: usize,
    /// Übernommen (neu oder aktualisiert).
    pub imported: usize,
    /// Davon zu Dateien, die in der eigenen Sammlung liegen.
    pub in_library: usize,
    /// Übergangen, weil schon eigene (bzw. bei Vorrang für ScreenScraper dessen) Angaben vorliegen.
    pub kept: usize,
    pub errors: Vec<String>,
    /// Erstellungszeit des eingelesenen Stands (Unix-Zeit, aus dem Kopf).
    pub created: i64,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub fn game_path(sha1: &str) -> String {
    format!("games/{}/{sha1}.json", &sha1[..2])
}

fn image_path(data: &[u8], ext: &str) -> String {
    use sha1::Digest;
    let h = hex::encode(sha1::Sha1::digest(data));
    format!("images/{}/{h}.{ext}", &h[..2])
}

/// Prüft einen Bildpfad aus einer fremden Datei: `images/<ab>/<40 hex>.<ext>`; gibt die Endung zurück.
fn check_image_path(p: &str) -> Option<&str> {
    let rest = p.strip_prefix("images/")?;
    let (dir, file) = rest.split_once('/')?;
    let (hash, ext) = file.split_once('.')?;
    (is_hex(dir, 2) && is_hex(hash, 40) && hash.starts_with(dir) && IMAGE_EXTS.contains(&ext))
        .then_some(ext)
}

/// `synopsis` und `texts` aus deutscher und englischer Beschreibung (siehe [`SharedGame::texts`]).
fn texts_fields(de: String, en: String) -> SharedGame {
    let mut g = SharedGame::default();
    if en.trim().is_empty() {
        g.synopsis = de;
        return g;
    }
    if !de.trim().is_empty() {
        g.texts.insert("de".into(), de.clone());
    }
    g.texts.insert("en".into(), en.clone());
    g.synopsis = if de.trim().is_empty() { en } else { de };
    g
}

/// Deutsche und englische Beschreibung eines eingelesenen Spiels (`texts` vor `synopsis`).
pub fn texts_of(g: &SharedGame) -> (Option<String>, Option<String>) {
    let clean = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    let en = g.texts.get("en").and_then(|s| clean(s));
    let de = g
        .texts
        .get("de")
        .and_then(|s| clean(s))
        .or_else(|| clean(&g.synopsis).filter(|s| Some(s) != en.as_ref()));
    (de, en)
}

/// Ein zu teilendes Spiel mit seinen Bildern (Bildart, Datei im Bilderordner).
pub type Shareable = (SharedGame, Vec<(String, String)>);

/// Die eigenen Angaben, die geteilt werden: selbst geschrieben (`own`), nicht bearbeitete ScreenScraper-Angaben.
pub fn shareable(conn: &Connection) -> Result<Vec<Shareable>> {
    let mut st = conn.prepare(
        "SELECT c.sha1, c.md5, c.crc32, c.size, c.format, m.title, m.year, m.publisher, m.developer,
                m.genres, m.players, m.synopsis, m.game_id, m.synopsis_en
         FROM metadata m JOIN contents c ON c.sha1 = m.sha1
         WHERE m.provider = ? AND m.status = 'ok'
         ORDER BY c.sha1",
    )?;
    let opt = |s: Option<String>| s.unwrap_or_default();
    let games: Vec<SharedGame> = st
        .query_map([PROVIDER_OWN], |r| {
            Ok(SharedGame {
                version: VERSION,
                sha1: r.get(0)?,
                md5: r.get(1)?,
                crc32: r.get(2)?,
                size: r.get(3)?,
                format: r.get(4)?,
                title: opt(r.get(5)?),
                year: opt(r.get(6)?),
                publisher: opt(r.get(7)?),
                developer: opt(r.get(8)?),
                genres: opt(r.get(9)?)
                    .split(',')
                    .map(str::trim)
                    .filter(|g| !g.is_empty())
                    .map(String::from)
                    .collect(),
                players: opt(r.get(10)?),
                screenscraper_id: opt(r.get(12)?),
                ..texts_fields(opt(r.get(11)?), opt(r.get(13)?))
            })
        })?
        .collect::<rusqlite::Result<_>>()?;

    let mut names = conn.prepare(
        "SELECT DISTINCT CASE WHEN member <> '' THEN member ELSE path END FROM files WHERE sha1 = ? ORDER BY 1 LIMIT 10",
    )?;
    // Bilder von ScreenScraper heißen `ss_…` und bleiben draußen.
    let mut media = conn.prepare("SELECT kind, file FROM media WHERE sha1 = ? AND file NOT LIKE 'ss\\_%' ESCAPE '\\' ORDER BY kind")?;
    let mut out = Vec::with_capacity(games.len());
    for mut g in games {
        let mut files: Vec<String> = names
            .query_map([&g.sha1], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(&p).to_string())
            .collect();
        files.dedup();
        g.files = files;
        let imgs = media
            .query_map([&g.sha1], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        out.push((g, imgs));
    }
    Ok(out)
}

/// Ziel beim Teilen: eine Austauschdatei (ZIP) oder ein Ordner im Aufbau des Repositorys.
trait Sink {
    fn put(&mut self, path: &str, data: &[u8], compress: bool) -> Result<()>;
}

impl<W: Write + std::io::Seek> Sink for zip::ZipWriter<W> {
    fn put(&mut self, path: &str, data: &[u8], compress: bool) -> Result<()> {
        let opts = zip::write::SimpleFileOptions::default();
        let opts = if compress {
            opts
        } else {
            opts.compression_method(zip::CompressionMethod::Stored)
        };
        self.start_file(path, opts)?;
        self.write_all(data)?;
        Ok(())
    }
}

struct DirSink(std::path::PathBuf);

impl Sink for DirSink {
    fn put(&mut self, path: &str, data: &[u8], _compress: bool) -> Result<()> {
        let target = self.0.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, data).with_context(|| {
            tr!(
                "{} lässt sich nicht schreiben",
                "Cannot write {}",
                target.display()
            )
        })?;
        Ok(())
    }
}

fn nothing_to_share() -> anyhow::Error {
    anyhow::anyhow!(tr!(
        "Nichts zum Teilen: Es gibt noch keine selbst geschriebenen Angaben.",
        "Nothing to share: there are no details you wrote yourself yet."
    ))
}

/// Schreibt Spiele und Bilder; der Kopf (`breadbin-share.json`) nur, wenn `manifest` gesetzt ist.
fn write_all(
    sink: &mut dyn Sink,
    games: Vec<Shareable>,
    media_dir: &Path,
    manifest: Option<&Manifest>,
) -> Result<usize> {
    let n = games.len();
    if let Some(m) = manifest {
        sink.put(MANIFEST, &serde_json::to_vec_pretty(m)?, true)?;
    }
    let mut written = std::collections::HashSet::new();
    for (mut g, imgs) in games {
        for (kind, file) in imgs {
            // Fehlt ein Bild im Bilderordner, geht das Spiel ohne dieses Bild hinaus.
            let Ok(data) = std::fs::read(media_dir.join(&file)) else {
                continue;
            };
            let ext = file
                .rsplit_once('.')
                .map(|(_, e)| e.to_ascii_lowercase())
                .unwrap_or_default();
            if !IMAGE_EXTS.contains(&ext.as_str()) {
                continue;
            }
            let path = image_path(&data, &ext);
            if written.insert(path.clone()) {
                sink.put(&path, &data, false)?;
            }
            g.images.insert(kind, path);
        }
        let mut json = serde_json::to_vec_pretty(&g)?;
        json.push(b'\n');
        sink.put(&game_path(&g.sha1), &json, true)?;
    }
    Ok(n)
}

/// Fingerabdruck eines zu teilenden Spiels: Angaben und Bilder, ohne die Dateinamen (die ändern sich
/// mit neuen Quellen, ohne dass sich am Beitrag etwas ändert).
pub fn fingerprint((g, imgs): &Shareable) -> String {
    use sha1::Digest;
    let mut g = g.clone();
    g.files.clear();
    let mut h = sha1::Sha1::new();
    h.update(serde_json::to_vec(&g).unwrap_or_default());
    for (kind, file) in imgs {
        h.update(format!("\n{kind}={file}"));
    }
    hex::encode(h.finalize())
}

/// Zu teilende Spiele, die noch nicht oder in anderer Fassung eingesandt wurden.
pub fn pending_submission(conn: &Connection) -> Result<Vec<Shareable>> {
    let mut st = conn.prepare("SELECT fingerprint FROM submitted WHERE sha1 = ?")?;
    let mut out = Vec::new();
    for s in shareable(conn)? {
        let sent: Option<String> = st.query_row([&s.0.sha1], |r| r.get(0)).optional()?;
        if sent.as_deref() != Some(fingerprint(&s).as_str()) {
            out.push(s);
        }
    }
    Ok(out)
}

/// Merkt sich die Spiele als eingesandt (Fingerabdrücke aus [`fingerprint`]).
pub fn mark_submitted(conn: &Connection, prints: &[(String, String)]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for (sha1, fp) in prints {
        tx.execute(
            "INSERT OR REPLACE INTO submitted(sha1, fingerprint, at) VALUES(?, ?, ?)",
            params![sha1, fp, now()],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Schreibt die Austauschdatei. Gibt die Zahl der Spiele zurück.
pub fn export(conn: &Connection, media_dir: &Path, out: &Path, app: &str) -> Result<usize> {
    let games = shareable(conn)?;
    ensure!(!games.is_empty(), nothing_to_share());
    export_games(games, media_dir, out, app)
}

/// Schreibt die Austauschdatei mit genau diesen Spielen.
pub fn export_games(
    games: Vec<Shareable>,
    media_dir: &Path,
    out: &Path,
    app: &str,
) -> Result<usize> {
    let tmp = out.with_extension("part");
    let file = std::fs::File::create(&tmp).with_context(|| {
        tr!(
            "{} lässt sich nicht anlegen",
            "Cannot create {}",
            tmp.display()
        )
    })?;
    let mut zip = zip::ZipWriter::new(file);
    let manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        created: now(),
        app: app.into(),
        license: LICENSE.into(),
        games: games.len(),
    };
    let n = write_all(&mut zip, games, media_dir, Some(&manifest))?;
    zip.finish()?;
    std::fs::rename(&tmp, out).with_context(|| {
        tr!(
            "{} lässt sich nicht schreiben",
            "Cannot write {}",
            out.display()
        )
    })?;
    Ok(n)
}

/// „Beitrag vorbereiten“: schreibt `games/` und `images/` in einen Ordner, z. B. die Arbeitskopie
/// eines Forks der Gemeinschaftsdatenbank. Vorhandene Dateien derselben Spiele werden ersetzt, ein
/// vorhandener Kopf bleibt; fehlt er, wird er angelegt. Gibt die Zahl der Spiele zurück.
pub fn export_dir(conn: &Connection, media_dir: &Path, dir: &Path, app: &str) -> Result<usize> {
    let games = shareable(conn)?;
    ensure!(!games.is_empty(), nothing_to_share());
    let manifest = (!dir.join(MANIFEST).exists()).then(|| Manifest {
        format: FORMAT.into(),
        version: VERSION,
        created: now(),
        app: app.into(),
        license: LICENSE.into(),
        games: games.len(),
    });
    write_all(
        &mut DirSink(dir.to_path_buf()),
        games,
        media_dir,
        manifest.as_ref(),
    )
}

/// Antwort des Servers auf einen eingesandten Beitrag (siehe server/public/api/beitrag.php
/// im Daten-Repository).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SubmitResult {
    pub ok: bool,
    pub id: u64,
    pub spiele: u64,
    pub fehler: String,
    pub probleme: Vec<String>,
    /// Beim Aufteilen: Zahl der angenommenen Beiträge und ihre Nummern (der Server nennt je Beitrag eine).
    #[serde(default)]
    pub teile: u64,
    #[serde(default)]
    pub ids: Vec<u64>,
}

/// Grenzen je Beitrag: Der Annahme-Server nimmt höchstens 500 Spiele und 20 MB; mit Abstand darunter bleiben.
const SUBMIT_MAX_GAMES: usize = 400;
const SUBMIT_MAX_BYTES: u64 = 15 * 1024 * 1024;

/// Teilt die einzusendenden Spiele in Beiträge unter den Grenzen des Servers (Bildgrößen grob mitgezählt).
pub fn submission_chunks(games: Vec<Shareable>, media_dir: &Path) -> Vec<Vec<Shareable>> {
    let mut chunks: Vec<Vec<Shareable>> = Vec::new();
    let mut cur: Vec<Shareable> = Vec::new();
    let mut bytes = 0u64;
    for g in games {
        let size = 2048
            + g.1
                .iter()
                .map(|(_, f)| {
                    std::fs::metadata(media_dir.join(f))
                        .map(|m| m.len())
                        .unwrap_or(0)
                })
                .sum::<u64>();
        if !cur.is_empty() && (cur.len() >= SUBMIT_MAX_GAMES || bytes + size > SUBMIT_MAX_BYTES) {
            chunks.push(std::mem::take(&mut cur));
            bytes = 0;
        }
        bytes += size;
        cur.push(g);
    }
    if !cur.is_empty() {
        chunks.push(cur);
    }
    chunks
}

/// „Beitrag einsenden“: eigene Angaben als Austauschdatei an den Server der Gemeinschaftsdatenbank
/// schicken. `name` (für die Namensnennung) und `hinweis` sind freiwillig.
pub fn submit(
    conn: &Connection,
    media_dir: &Path,
    url: &str,
    name: &str,
    hinweis: &str,
    user_agent: &str,
) -> Result<SubmitResult> {
    // http nur für einen Testserver auf diesem Rechner.
    let local = url.starts_with("http://localhost") || url.starts_with("http://127.0.0.1");
    ensure!(
        url.starts_with("https://") || local,
        tr!(
            "Adresse muss mit https:// beginnen",
            "Address must start with https://"
        )
    );
    let games = pending_submission(conn)?;
    ensure!(!games.is_empty(), nothing_new());
    // Große Mengen (z. B. nach dem Ergänzen englischer Texte) in mehrere Beiträge aufteilen; jeder angenommene Teil
    // gilt sofort als eingesandt, ein Fehler später schickt beim nächsten Mal nur den Rest.
    let chunks = submission_chunks(games, media_dir);
    let total = chunks.len();
    let mut done = SubmitResult {
        ok: true,
        ..Default::default()
    };
    for (i, chunk) in chunks.into_iter().enumerate() {
        let prints: Vec<(String, String)> = chunk
            .iter()
            .map(|s| (s.0.sha1.clone(), fingerprint(s)))
            .collect();
        let tmp =
            std::env::temp_dir().join(format!("breadbin-beitrag-{}-{i}.zip", std::process::id()));
        export_games(chunk, media_dir, &tmp, user_agent)?;
        let hint = if total > 1 {
            format!("{} (Teil {}/{total})", hinweis.trim(), i + 1)
                .trim()
                .to_string()
        } else {
            hinweis.to_string()
        };
        let r = submit_file(&tmp, url, name, &hint, user_agent);
        let _ = std::fs::remove_file(&tmp);
        match r {
            Ok(r) if r.ok => {
                mark_submitted(conn, &prints)?;
                done.id = r.id;
                done.ids.push(r.id);
                done.spiele += r.spiele;
                done.teile += 1;
            }
            Ok(mut r) => {
                let (n, g) = (done.teile, done.spiele);
                if n > 0 {
                    r.fehler = format!(
                        "{} {}",
                        tr!(
                            "{n} Beiträge mit {g} Spielen sind schon angekommen; der Rest kommt beim nächsten Einsenden.",
                            "{n} contributions with {g} games have already arrived; the rest goes out next time."
                        ),
                        r.fehler
                    );
                }
                return Ok(r);
            }
            Err(e) if done.teile > 0 => {
                let (n, g) = (done.teile, done.spiele);
                return Err(e.context(tr!(
                    "{n} Beiträge mit {g} Spielen sind schon angekommen; der Rest kommt beim nächsten Einsenden.",
                    "{n} contributions with {g} games have already arrived; the rest goes out next time."
                )));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(done)
}

/// Alles schon eingesandt.
pub fn nothing_new() -> anyhow::Error {
    anyhow::anyhow!(tr!(
        "Nichts Neues einzusenden: Alle selbst geschriebenen Angaben sind schon in dieser Fassung eingesandt.",
        "Nothing new to submit: all details you wrote yourself have already been submitted in this form."
    ))
}

/// Schickt eine schon geschriebene Austauschdatei an den Server (ohne Datenbank, also ohne Sperre).
pub fn submit_file(
    file: &Path,
    url: &str,
    name: &str,
    hinweis: &str,
    user_agent: &str,
) -> Result<SubmitResult> {
    let local = url.starts_with("http://localhost") || url.starts_with("http://127.0.0.1");
    ensure!(
        url.starts_with("https://") || local,
        tr!(
            "Adresse muss mit https:// beginnen",
            "Address must start with https://"
        )
    );
    let data = std::fs::read(file)?;
    let boundary = format!("breadbin{:016x}", now() as u64 ^ std::process::id() as u64);
    let body = crate::screenscraper::multipart(
        &[
            ("name", name.trim().to_string()),
            ("hinweis", hinweis.trim().to_string()),
        ],
        Some(("datei", "beitrag.zip", &data)),
        &boundary,
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .user_agent(user_agent)
        .build()
        .into();
    let mut resp = agent
        .post(url)
        .header(
            "Content-Type",
            &format!("multipart/form-data; boundary={boundary}"),
        )
        .send(&body[..])
        .map_err(|e| {
            anyhow::anyhow!(tr!(
                "Server nicht erreichbar: {e}",
                "Server unreachable: {e}"
            ))
        })?;
    let status = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    let mut r: SubmitResult = serde_json::from_str(&text).with_context(|| {
        tr!(
            "Unerwartete Antwort des Servers (Fehler {status})",
            "Unexpected answer from the server (error {status})"
        )
    })?;
    r.ok = r.ok && (200..300).contains(&status);
    Ok(r)
}

/// Lädt die Gemeinschaftsdaten (Austauschdatei) von `url` in eine temporäre Datei – ohne die
/// Datenbank zu sperren; danach [`import`] und die Datei löschen.
pub fn download(url: &str, user_agent: &str) -> Result<std::path::PathBuf> {
    // http nur für einen Testserver auf diesem Rechner.
    let local = url.starts_with("http://localhost") || url.starts_with("http://127.0.0.1");
    ensure!(
        url.starts_with("https://") || local,
        tr!(
            "Adresse muss mit https:// beginnen",
            "Address must start with https://"
        )
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(300)))
        .user_agent(user_agent)
        .build()
        .into();
    let mut resp = agent.get(url).call().map_err(|e| {
        anyhow::anyhow!(tr!(
            "Gemeinschaftsdaten nicht erreichbar: {e}",
            "Community data unreachable: {e}"
        ))
    })?;
    let status = resp.status().as_u16();
    ensure!(
        status == 200,
        tr!(
            "Gemeinschaftsdaten nicht erreichbar (Fehler {status}). Ist das Repository privat, lässt sich die Datei nur im Browser herunterladen und mit „Austauschdatei einlesen“ öffnen.",
            "Community data unreachable (error {status}). If the repository is private, download the file in the browser and open it with “Read exchange file”."
        )
    );
    let tmp = std::env::temp_dir().join(format!("breadbin-db-{}.zip", std::process::id()));
    let mut data = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(MAX_DOWNLOAD + 1)
        .read_to_end(&mut data)?;
    ensure!(
        data.len() as u64 <= MAX_DOWNLOAD,
        tr!("Gemeinschaftsdaten zu groß", "Community data too large")
    );
    std::fs::write(&tmp, &data)?;
    Ok(tmp)
}

/// Kurzbeschreibung eines Stands neben der Austauschdatei (`stand.json`, vom Spiegel geschrieben).
#[derive(Debug, Deserialize)]
pub struct Stand {
    pub games: usize,
    /// Erstellungszeit wie im Kopf der Austauschdatei.
    pub created: i64,
}

/// Adresse von `stand.json` zur Adresse einer Austauschdatei `…/breadbin-db.zip`, sonst `None`.
pub fn stand_url(zip_url: &str) -> Option<String> {
    zip_url
        .strip_suffix("breadbin-db.zip")
        .map(|base| format!("{base}stand.json"))
}

/// Kleine JSON-Datei laden (Stand, Versionshinweis): nur https (bzw. localhost), höchstens 64 KB.
pub fn fetch_json<T: serde::de::DeserializeOwned>(url: &str, user_agent: &str) -> Result<T> {
    let local = url.starts_with("http://localhost") || url.starts_with("http://127.0.0.1");
    ensure!(
        url.starts_with("https://") || local,
        tr!(
            "Adresse muss mit https:// beginnen",
            "Address must start with https://"
        )
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .user_agent(user_agent)
        .build()
        .into();
    let mut resp = agent.get(url).call()?;
    let status = resp.status().as_u16();
    ensure!(
        status == 200,
        tr!(
            "{url} nicht erreichbar (Fehler {status})",
            "{url} unreachable (error {status})"
        )
    );
    let mut data = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(64 * 1024 + 1)
        .read_to_end(&mut data)?;
    ensure!(
        data.len() <= 64 * 1024,
        tr!("Antwort zu groß", "Response too large")
    );
    Ok(serde_json::from_slice(&data)?)
}

/// Woher eingelesen wird: eine Austauschdatei (ZIP, auch mit äußerem Ordner wie beim Herunterladen
/// eines GitHub-Repositorys als ZIP) oder ein ausgepackter Datenordner (Arbeitskopie des Repositorys).
enum Source {
    Zip {
        zip: zip::ZipArchive<std::io::BufReader<std::fs::File>>,
        prefix: String,
    },
    Dir(std::path::PathBuf),
}

impl Source {
    fn open(path: &Path) -> Result<Source> {
        if path.is_dir() {
            ensure!(
                path.join(MANIFEST).is_file(),
                tr!(
                    "Im Ordner fehlt {MANIFEST} – kein Breadbin-Datenordner",
                    "{MANIFEST} is missing in the folder – not a Breadbin data folder"
                )
            );
            return Ok(Source::Dir(path.to_path_buf()));
        }
        let file = std::fs::File::open(path).with_context(|| {
            tr!(
                "{} lässt sich nicht öffnen",
                "Cannot open {}",
                path.display()
            )
        })?;
        let zip = zip::ZipArchive::new(std::io::BufReader::new(file)).context(tr!(
            "keine gültige Austauschdatei (kein ZIP)",
            "not a valid exchange file (no ZIP)"
        ))?;
        // Kopf ganz oben oder eine Ebene tiefer („breadbin-db-main/breadbin-share.json“).
        let prefix = zip
            .file_names()
            .filter_map(|n| n.ok())
            .filter_map(|n| n.strip_suffix(MANIFEST).map(str::to_string))
            .filter(|p| p.is_empty() || (p.ends_with('/') && p.matches('/').count() == 1))
            .min_by_key(String::len)
            .unwrap_or_default();
        Ok(Source::Zip { zip, prefix })
    }

    /// Spieldateien (`games/…json`), relativ zur Wurzel der Daten.
    fn game_names(&mut self) -> Result<Vec<String>> {
        Ok(match self {
            Source::Zip { zip, prefix } => zip
                .file_names()
                .filter_map(|n| n.ok())
                .filter_map(|n| n.strip_prefix(prefix.as_str()).map(str::to_string))
                .filter(|n| n.starts_with("games/") && n.ends_with(".json"))
                .collect(),
            Source::Dir(root) => {
                let mut out = Vec::new();
                let games = root.join("games");
                if games.is_dir() {
                    for sub in std::fs::read_dir(&games)? {
                        let sub = sub?;
                        if !sub.file_type()?.is_dir() {
                            continue;
                        }
                        for f in std::fs::read_dir(sub.path())? {
                            let f = f?;
                            let name = f.file_name().to_string_lossy().into_owned();
                            if name.ends_with(".json") {
                                out.push(format!(
                                    "games/{}/{name}",
                                    sub.file_name().to_string_lossy()
                                ));
                            }
                        }
                    }
                }
                out.sort();
                out
            }
        })
    }

    fn read(&mut self, name: &str, limit: u64) -> Result<Vec<u8>> {
        match self {
            Source::Zip { zip, prefix } => read_entry(zip, &format!("{prefix}{name}"), limit),
            Source::Dir(root) => {
                // Nur Namen aus dem Datenaufbau, nie Pfade nach oben.
                ensure!(
                    !name.split('/').any(|p| p == ".." || p.is_empty()),
                    "ungültiger Pfad {name}"
                );
                let path = root.join(name);
                let len = std::fs::metadata(&path)
                    .with_context(|| {
                        tr!("{name} fehlt in der Datei", "{name} missing in the file")
                    })?
                    .len();
                ensure!(
                    len <= limit,
                    tr!("{name} ist zu groß", "{name} is too large")
                );
                Ok(std::fs::read(path)?)
            }
        }
    }
}

fn read_entry<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>> {
    let f = zip
        .by_name(name)
        .with_context(|| tr!("{name} fehlt in der Datei", "{name} missing in the file"))?;
    ensure!(
        f.size() <= limit,
        tr!("{name} ist zu groß", "{name} is too large")
    );
    let mut data = Vec::new();
    f.take(limit + 1).read_to_end(&mut data)?;
    ensure!(
        data.len() as u64 <= limit,
        tr!("{name} ist zu groß", "{name} is too large")
    );
    Ok(data)
}

/// Prüft ein Spiel aus einer fremden Datei, bevor etwas gespeichert wird.
fn check_game(g: &SharedGame) -> Result<()> {
    ensure!(
        g.version <= VERSION,
        tr!(
            "neueres Format, bitte Breadbin aktualisieren",
            "newer format, please update Breadbin"
        )
    );
    ensure!(
        is_hex(&g.sha1, 40),
        tr!("ungültige Prüfsumme", "invalid checksum")
    );
    ensure!(
        !g.title.trim().is_empty(),
        tr!("Titel fehlt", "title missing")
    );
    ensure!(
        g.screenscraper_id.bytes().all(|b| b.is_ascii_digit()) && g.screenscraper_id.len() <= 12,
        tr!("ungültige Spielnummer", "invalid game number")
    );
    for (kind, path) in &g.images {
        ensure!(
            MEDIA_KINDS.contains(&kind.as_str()),
            tr!("unbekannte Bildart {kind}", "unknown picture type {kind}")
        );
        ensure!(
            check_image_path(path).is_some(),
            tr!("ungültiger Bildpfad {path}", "invalid picture path {path}")
        );
    }
    Ok(())
}

/// Liest eine Austauschdatei ein. Eigene Angaben und von Hand zugeordnete Spiele bleiben
/// unangetastet, ebenso Angaben von ScreenScraper, wenn ScreenScraper die bevorzugte Quelle ist;
/// früher übernommene werden aktualisiert. Bilder von ScreenScraper bleiben für Bildarten stehen,
/// die die Gemeinschaft nicht liefert. Angaben zu Dateien, die (noch) nicht in der Sammlung liegen,
/// werden mit gespeichert und erscheinen, sobald die Datei gescannt ist.
pub fn import(conn: &mut Connection, media_dir: &Path, path: &Path) -> Result<ImportReport> {
    let mut src = Source::open(path)?;
    let manifest: Manifest = serde_json::from_slice(&src.read(MANIFEST, MAX_JSON)?).context(
        tr!("keine gültige Austauschdatei", "not a valid exchange file"),
    )?;
    if manifest.format != FORMAT {
        bail!(tr!(
            "keine Breadbin-Austauschdatei",
            "not a Breadbin exchange file"
        ));
    }
    ensure!(
        manifest.version <= VERSION,
        tr!(
            "Die Datei hat ein neueres Format (Version {}). Bitte Breadbin aktualisieren.",
            "The file uses a newer format (version {}). Please update Breadbin.",
            manifest.version
        )
    );

    let names = src.game_names()?;
    ensure!(
        names.len() <= MAX_GAMES,
        tr!("zu viele Einträge", "too many entries")
    );

    let mut report = ImportReport {
        games: names.len(),
        created: manifest.created,
        ..Default::default()
    };
    let prefer = prefer_shared(conn)?;
    let tx = conn.transaction()?;
    for name in names {
        let result = (|| -> Result<()> {
            let g: SharedGame = serde_json::from_slice(&src.read(&name, MAX_JSON)?)?;
            check_game(&g)?;
            ensure!(
                name == game_path(&g.sha1),
                tr!(
                    "Dateiname passt nicht zur Prüfsumme",
                    "file name does not match the checksum"
                )
            );
            let existing: Option<(String, String, Option<String>)> = tx
                .query_row(
                    "SELECT provider, status, game_id FROM metadata WHERE sha1 = ?",
                    [&g.sha1],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            // „Nicht gefunden“, „falsch“ und früher Übernommenes werden ersetzt.
            if let Some((provider, status, _)) = &existing
                && status == "ok"
            {
                let keep = metadata::is_own(provider)
                    || provider == metadata::PROVIDER_MANUAL
                    || (provider.starts_with("screenscraper") && !prefer);
                if keep {
                    report.kept += 1;
                    return Ok(());
                }
            }
            // Spielnummer: aus der Datei, sonst die schon bekannte von ScreenScraper.
            let game_id = Some(g.screenscraper_id.clone())
                .filter(|s| !s.is_empty())
                .or_else(|| {
                    existing
                        .as_ref()
                        .filter(|(p, s, _)| s == "ok" && p.starts_with("screenscraper"))
                        .and_then(|(_, _, id)| id.clone())
                });
            let mut images = Vec::new();
            for (kind, p) in &g.images {
                let ext = check_image_path(p).unwrap_or_default();
                let data = src.read(p, MAX_IMAGE)?;
                let fits = matches!(
                    (image::guess_format(&data), ext),
                    (Ok(image::ImageFormat::Png), "png")
                        | (Ok(image::ImageFormat::Jpeg), "jpg")
                        | (Ok(image::ImageFormat::Gif), "gif")
                        | (Ok(image::ImageFormat::WebP), "webp")
                );
                ensure!(
                    fits,
                    tr!(
                        "{p} ist kein passendes Bild",
                        "{p} is not a matching picture"
                    )
                );
                images.push((kind.clone(), data, ext.to_string()));
            }
            let opt = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
            let genres = g
                .genres
                .iter()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            // Bilder von ScreenScraper (`ss_…`) bleiben, bis die Gemeinschaft eins derselben Art liefert.
            tx.execute(
                "DELETE FROM media WHERE sha1 = ? AND file NOT LIKE 'ss\\_%' ESCAPE '\\'",
                [&g.sha1],
            )?;
            let (de, en) = texts_of(&g);
            tx.execute(
                "INSERT OR REPLACE INTO metadata(sha1, provider, status, game_id, title, synopsis, year, publisher,
                                                 developer, genres, players, rating, fetched_at, synopsis_en)
                 VALUES(?, ?, 'ok', ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)",
                params![
                    g.sha1,
                    PROVIDER_SHARED,
                    game_id,
                    opt(&g.title),
                    de,
                    opt(&g.year),
                    opt(&g.publisher),
                    opt(&g.developer),
                    opt(&genres),
                    opt(&g.players),
                    now(),
                    en
                ],
            )?;
            for (kind, data, ext) in images {
                metadata::set_own_image(
                    &tx,
                    std::slice::from_ref(&g.sha1),
                    &kind,
                    Some((&data, &ext)),
                    media_dir,
                )?;
            }
            report.imported += 1;
            let have: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM contents WHERE sha1 = ?)",
                [&g.sha1],
                |r| r.get(0),
            )?;
            if have {
                report.in_library += 1;
            }
            Ok(())
        })();
        if let Err(e) = result {
            report.errors.push(format!("{name}: {e:#}"));
        }
    }
    tx.commit()?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{db, metadata, scan};

    fn library(names: &[&str]) -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let games = dir.path().join("spiele");
        std::fs::create_dir_all(&games).unwrap();
        for (i, n) in names.iter().enumerate() {
            std::fs::write(games.join(n), [0x01, 0x08, i as u8, 0x60]).unwrap();
        }
        let conn = db::open(&dir.path().join("db.sqlite")).unwrap();
        let id = db::add_local_source(&conn, &games).unwrap();
        scan::scan_local(
            &conn,
            id,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        (dir, conn)
    }

    fn sha1_of(conn: &Connection, q: &str) -> String {
        db::search(conn, q, None, 10).unwrap()[0].sha1.clone()
    }

    fn png() -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(2, 2)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn zweisprachige_beschreibung() {
        // Ohne Englisch bleibt der Eintrag wie vor 0.8.0 (gleicher Fingerabdruck), mit Englisch kommt `texts` dazu.
        let nur_de = texts_fields("Deutsch".into(), String::new());
        assert_eq!(nur_de.synopsis, "Deutsch");
        assert!(nur_de.texts.is_empty());
        assert!(!serde_json::to_string(&nur_de).unwrap().contains("texts"));
        let beide = texts_fields("Deutsch".into(), "English".into());
        assert_eq!(beide.synopsis, "Deutsch");
        assert_eq!(beide.texts.get("en").map(String::as_str), Some("English"));
        assert_eq!(
            texts_of(&beide),
            (Some("Deutsch".into()), Some("English".into()))
        );
        let nur_en = texts_fields(String::new(), "English".into());
        assert_eq!(
            nur_en.synopsis, "English",
            "ältere Fassungen zeigen wenigstens den englischen Text"
        );
        assert_eq!(texts_of(&nur_en), (None, Some("English".into())));
        // Eintrag einer älteren Fassung: nur `synopsis`.
        let alt = SharedGame {
            synopsis: "Alt".into(),
            ..Default::default()
        };
        assert_eq!(texts_of(&alt), (Some("Alt".into()), None));
    }

    #[test]
    fn zweisprachig_teilen_und_einlesen() {
        let (dir, conn) = library(&["eigen.prg"]);
        let media = dir.path().join("media");
        let own = sha1_of(&conn, "eigen");
        let m = metadata::OwnMeta {
            title: "Space Gun".into(),
            synopsis: "Deutsch".into(),
            synopsis_en: "English".into(),
            ..Default::default()
        };
        metadata::save_own(&conn, std::slice::from_ref(&own), &m, true).unwrap();
        let meta = db::details(&conn, &own).unwrap().meta.unwrap();
        assert_eq!(meta.synopsis_en.as_deref(), Some("English"));
        let out = dir.path().join("teilen.zip");
        assert_eq!(export(&conn, &media, &out, "Breadbin test").unwrap(), 1);
        // In eine frische Bibliothek einlesen: beide Sprachen kommen an.
        let (dir2, mut conn2) = library(&["eigen.prg"]);
        import(&mut conn2, &dir2.path().join("media"), &out).unwrap();
        let meta2 = db::details(&conn2, &own).unwrap().meta.unwrap();
        assert_eq!(meta2.provider, PROVIDER_SHARED);
        assert_eq!(meta2.synopsis.as_deref(), Some("Deutsch"));
        assert_eq!(meta2.synopsis_en.as_deref(), Some("English"));
    }

    #[test]
    fn export_und_import() {
        let (dir, conn) = library(&["eigen.prg", "bekannt.prg", "frisch.prg"]);
        let media = dir.path().join("media");
        let own = sha1_of(&conn, "eigen");
        let known = sha1_of(&conn, "bekannt");
        let fresh = sha1_of(&conn, "frisch");
        let m = metadata::OwnMeta {
            title: "Space Gun".into(),
            year: "2026".into(),
            genres: "Shooter, Action".into(),
            ..Default::default()
        };
        metadata::save_own(&conn, std::slice::from_ref(&own), &m, false).unwrap();
        metadata::set_own_image(
            &conn,
            std::slice::from_ref(&own),
            "ss",
            Some((&png(), "png")),
            &media,
        )
        .unwrap();
        conn.execute(
            "INSERT INTO media(sha1, kind, file) VALUES(?, 'sstitle', 'ss_7_sstitle.png')",
            [&own],
        )
        .unwrap();
        std::fs::write(media.join("ss_7_sstitle.png"), png()).unwrap();
        // Von ScreenScraper beschrieben: bearbeitet → nicht teilbar; „Eigene Fassung“ → teilbar, mit Spielnummer.
        for sha1 in [&known, &fresh] {
            conn.execute(
                "INSERT INTO metadata(sha1, provider, status, game_id, title, fetched_at) VALUES(?, 'screenscraper', 'ok', '42', 'SS', 0)",
                [sha1],
            )
            .unwrap();
        }
        let edit = metadata::OwnMeta {
            title: "Bearbeitet".into(),
            ..Default::default()
        };
        metadata::save_own(&conn, std::slice::from_ref(&known), &edit, false).unwrap();
        let mine = metadata::OwnMeta {
            title: "Frisch".into(),
            ..Default::default()
        };
        conn.execute(
            "INSERT INTO media(sha1, kind, file) VALUES(?, 'ss', 'ss_42_ss.png')",
            [&fresh],
        )
        .unwrap();
        metadata::save_own(&conn, std::slice::from_ref(&fresh), &mine, true).unwrap();
        assert!(
            db::details(&conn, &fresh).unwrap().media.is_empty(),
            "eigene Fassung beginnt ohne ScreenScraper-Bilder"
        );
        let provider = |c: &Connection, s: &str| db::details(c, s).unwrap().meta.unwrap().provider;
        assert_eq!(provider(&conn, &known), metadata::PROVIDER_OWN_EDITED);
        // Erneutes Bearbeiten einer eigenen Fassung bleibt teilbar.
        metadata::save_own(&conn, std::slice::from_ref(&fresh), &mine, false).unwrap();
        assert_eq!(provider(&conn, &fresh), metadata::PROVIDER_OWN);

        let out = dir.path().join("teilen.zip");
        assert_eq!(export(&conn, &media, &out, "Breadbin test").unwrap(), 2);
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let g: SharedGame =
            serde_json::from_slice(&read_entry(&mut zip, &game_path(&own), MAX_JSON).unwrap())
                .unwrap();
        assert_eq!(
            (g.title.as_str(), g.genres.len(), g.files.as_slice()),
            ("Space Gun", 2, &["eigen.prg".to_string()][..])
        );
        assert_eq!(
            g.images.keys().collect::<Vec<_>>(),
            ["ss"],
            "kein ScreenScraper-Bild"
        );
        assert_eq!(
            zip.file_names()
                .filter_map(Result::ok)
                .filter(|n| n.starts_with("images/"))
                .count(),
            1
        );
        let f: SharedGame =
            serde_json::from_slice(&read_entry(&mut zip, &game_path(&fresh), MAX_JSON).unwrap())
                .unwrap();
        assert_eq!(f.screenscraper_id, "42");
        assert!(
            zip.by_name(&game_path(&known)).is_err(),
            "bearbeitete ScreenScraper-Angaben bleiben draußen"
        );

        // Andere Sammlung: dieselben Dateien, „eigen“ schon von ScreenScraper beschrieben (mit Bild).
        let (dir2, mut conn2) = library(&["eigen.prg", "bekannt.prg", "frisch.prg"]);
        let media2 = dir2.path().join("media");
        conn2
            .execute("INSERT INTO metadata(sha1, provider, status, game_id, title, fetched_at) VALUES(?, 'screenscraper', 'ok', '9', 'SS', 0)", [&own])
            .unwrap();
        conn2
            .execute(
                "INSERT INTO media(sha1, kind, file) VALUES(?, 'box-2D', 'ss_9_box-2D.png')",
                [&own],
            )
            .unwrap();
        db::set_setting(&conn2, PREFERRED_SOURCE, "screenscraper").unwrap();
        let r = import(&mut conn2, &media2, &out).unwrap();
        assert_eq!(
            (r.imported, r.kept),
            (1, 1),
            "ScreenScraper bevorzugt: dessen Angaben bleiben"
        );

        db::set_setting(&conn2, PREFERRED_SOURCE, "community").unwrap();
        let r = import(&mut conn2, &media2, &out).unwrap();
        assert_eq!(
            (r.games, r.imported, r.in_library, r.errors.len()),
            (2, 2, 2, 0),
            "Gemeinschaft geht vor"
        );
        let d = db::details(&conn2, &own).unwrap();
        let meta = d.meta.unwrap();
        assert_eq!(
            (meta.provider.as_str(), meta.game_id.as_deref()),
            (PROVIDER_SHARED, Some("9")),
            "Spielnummer bleibt"
        );
        let mut kinds: Vec<_> = d
            .media
            .iter()
            .map(|(k, f)| (k.as_str(), f.starts_with("ss_")))
            .collect();
        kinds.sort();
        assert_eq!(
            kinds,
            [("box-2D", true), ("ss", false)],
            "fehlende Bildart bleibt von ScreenScraper"
        );
        // Erneut einlesen aktualisiert, statt zu verdoppeln.
        assert_eq!(import(&mut conn2, &media2, &out).unwrap().imported, 2);
        // Übernommenes wird nicht weitergegeben; bearbeitet wird es eigenes (gleiche Lizenz).
        assert!(shareable(&conn2).unwrap().is_empty());
        metadata::save_own(&conn2, std::slice::from_ref(&own), &m, false).unwrap();
        assert_eq!(provider(&conn2, &own), metadata::PROVIDER_OWN);
        // Ein Abruf bei ScreenScraper lässt Gemeinschaftsangaben in Ruhe, solange sie bevorzugt sind.
        let o = metadata::Options {
            sha1s: None,
            force: false,
            retry_notfound: true,
            lang: "de",
            media_dir: &media2,
            name_fallback: false,
        };
        assert!(
            metadata::pending(&conn2, &o)
                .unwrap()
                .iter()
                .all(|q| q.sha1 != fresh)
        );
        db::set_setting(&conn2, PREFERRED_SOURCE, "screenscraper").unwrap();
        assert!(
            metadata::pending(&conn2, &o)
                .unwrap()
                .iter()
                .any(|q| q.sha1 == fresh)
        );
    }

    #[test]
    fn repository_zip_und_datenordner_einlesen() {
        let (dir, conn) = library(&["eigen.prg"]);
        let media = dir.path().join("media");
        let own = sha1_of(&conn, "eigen");
        let m = metadata::OwnMeta {
            title: "Space Gun".into(),
            ..Default::default()
        };
        metadata::save_own(&conn, std::slice::from_ref(&own), &m, false).unwrap();
        metadata::set_own_image(
            &conn,
            std::slice::from_ref(&own),
            "ss",
            Some((&png(), "png")),
            &media,
        )
        .unwrap();
        let out = dir.path().join("teilen.zip");
        export(&conn, &media, &out, "test").unwrap();

        // Ausgepackt als Arbeitskopie und neu gepackt mit äußerem Ordner (GitHub „Download ZIP“).
        let repo = dir.path().join("breadbin-db-main");
        let mut zip = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        zip.extract(&repo).unwrap();
        let gh = dir.path().join("gh.zip");
        let mut w = zip::ZipWriter::new(std::fs::File::create(&gh).unwrap());
        for i in 0..zip.len() {
            let mut f = zip.by_index(i).unwrap();
            let name = format!("breadbin-db-main/{}", f.name().unwrap());
            let mut data = Vec::new();
            f.read_to_end(&mut data).unwrap();
            w.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(&data).unwrap();
        }
        w.finish().unwrap();

        for source in [&repo, &gh] {
            let (d2, mut c2) = library(&["eigen.prg"]);
            let r = import(&mut c2, &d2.path().join("media"), source).unwrap();
            assert_eq!(
                (r.games, r.imported, r.in_library, r.errors.len()),
                (1, 1, 1, 0),
                "{source:?}: {:?}",
                r.errors
            );
            assert_eq!(db::details(&c2, &own).unwrap().media.len(), 1);
        }
        assert!(
            import(&mut conn_for(&dir), &media, dir.path()).is_err(),
            "Ordner ohne Kopf"
        );
    }

    #[test]
    fn einsenden_merkt_sich_was_schon_draussen_ist() {
        let (dir, conn) = library(&["eigen.prg", "zweit.prg"]);
        let media = dir.path().join("media");
        let (a, b) = (sha1_of(&conn, "eigen"), sha1_of(&conn, "zweit"));
        let m = metadata::OwnMeta {
            title: "Space Gun".into(),
            ..Default::default()
        };
        metadata::save_own(&conn, &[a.clone(), b.clone()], &m, false).unwrap();
        let pending = pending_submission(&conn).unwrap();
        assert_eq!(pending.len(), 2, "noch nichts eingesandt");
        let prints: Vec<_> = pending
            .iter()
            .map(|s| (s.0.sha1.clone(), fingerprint(s)))
            .collect();
        mark_submitted(&conn, &prints).unwrap();
        assert!(
            pending_submission(&conn).unwrap().is_empty(),
            "alles schon draußen"
        );
        // Bild dazu → nur dieses Spiel ist wieder fällig; Angaben geändert → ebenso.
        metadata::set_own_image(
            &conn,
            std::slice::from_ref(&a),
            "ss",
            Some((&png(), "png")),
            &media,
        )
        .unwrap();
        let again = pending_submission(&conn).unwrap();
        assert_eq!((again.len(), again[0].0.sha1.as_str()), (1, a.as_str()));
        let m2 = metadata::OwnMeta {
            title: "Space Gun".into(),
            year: "2025".into(),
            ..Default::default()
        };
        metadata::save_own(&conn, std::slice::from_ref(&b), &m2, false).unwrap();
        assert_eq!(pending_submission(&conn).unwrap().len(), 2);
        assert_eq!(
            shareable(&conn).unwrap().len(),
            2,
            "Austauschdatei enthält weiter alles"
        );
    }

    #[test]
    fn beitrag_in_ordner_schreiben() {
        let (dir, conn) = library(&["eigen.prg"]);
        let media = dir.path().join("media");
        let own = sha1_of(&conn, "eigen");
        let m = metadata::OwnMeta {
            title: "Space Gun".into(),
            ..Default::default()
        };
        metadata::save_own(&conn, std::slice::from_ref(&own), &m, false).unwrap();
        metadata::set_own_image(
            &conn,
            std::slice::from_ref(&own),
            "ss",
            Some((&png(), "png")),
            &media,
        )
        .unwrap();
        let repo = dir.path().join("fork");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(MANIFEST), b"{\"format\":\"breadbin-share\"}").unwrap();
        assert_eq!(export_dir(&conn, &media, &repo, "test").unwrap(), 1);
        assert_eq!(
            std::fs::read(repo.join(MANIFEST)).unwrap(),
            b"{\"format\":\"breadbin-share\"}",
            "vorhandener Kopf bleibt"
        );
        let g: SharedGame =
            serde_json::from_slice(&std::fs::read(repo.join(game_path(&own))).unwrap()).unwrap();
        assert!(repo.join(&g.images["ss"]).is_file());
        // Leerer Ordner bekommt einen Kopf und ist dann selbst einlesbar.
        let fresh = dir.path().join("neu");
        export_dir(&conn, &media, &fresh, "test").unwrap();
        let (d2, mut c2) = library(&["eigen.prg"]);
        assert_eq!(
            import(&mut c2, &d2.path().join("media"), &fresh)
                .unwrap()
                .imported,
            1
        );
    }

    fn conn_for(dir: &tempfile::TempDir) -> Connection {
        db::open(&dir.path().join("db.sqlite")).unwrap()
    }

    #[test]
    fn fremde_dateien_werden_geprueft() {
        let (dir, mut conn) = library(&["a.prg"]);
        let media = dir.path().join("media");
        let sha1 = sha1_of(&conn, "a");
        let write = |files: &[(&str, Vec<u8>)]| {
            let p = dir.path().join("x.zip");
            let mut z = zip::ZipWriter::new(std::fs::File::create(&p).unwrap());
            for (n, d) in files {
                z.start_file(*n, zip::write::SimpleFileOptions::default())
                    .unwrap();
                z.write_all(d).unwrap();
            }
            z.finish().unwrap();
            p
        };
        let manifest = |v: u32| {
            serde_json::to_vec(&Manifest {
                format: FORMAT.into(),
                version: v,
                created: 0,
                app: "x".into(),
                license: LICENSE.into(),
                games: 1,
            })
            .unwrap()
        };
        let game = |g: SharedGame| serde_json::to_vec(&g).unwrap();
        let base = SharedGame {
            version: 1,
            sha1: sha1.clone(),
            title: "A".into(),
            ..Default::default()
        };

        assert!(
            import(&mut conn, &media, &write(&[("x.txt", vec![])])).is_err(),
            "ohne Kopf"
        );
        assert!(
            import(
                &mut conn,
                &media,
                &write(&[(MANIFEST, manifest(VERSION + 1))])
            )
            .is_err(),
            "neueres Format"
        );

        let mut bad_img = base.clone();
        bad_img
            .images
            .insert("ss".into(), "../../etc/passwd".into());
        let mut fake_img = base.clone();
        fake_img.sha1 = "d".repeat(40);
        let fake = format!("images/aa/aa{}.png", "0".repeat(38));
        fake_img.images.insert("ss".into(), fake.clone());
        let mut other = base.clone();
        other.sha1 = "b".repeat(40);
        let r = import(
            &mut conn,
            &media,
            &write(&[
                (MANIFEST, manifest(VERSION)),
                (&game_path(&sha1), game(bad_img)),
                (&game_path(&"c".repeat(40)), game(other)),
                (&format!("games/dd/{}.json", "d".repeat(40)), game(fake_img)),
                (&fake, b"kein Bild".to_vec()),
            ]),
        )
        .unwrap();
        assert_eq!(
            (r.games, r.imported, r.errors.len()),
            (3, 0, 3),
            "{:?}",
            r.errors
        );
        assert!(db::details(&conn, &sha1).unwrap().meta.is_none());

        let r = import(
            &mut conn,
            &media,
            &write(&[
                (MANIFEST, manifest(VERSION)),
                (&game_path(&sha1), game(base)),
            ]),
        )
        .unwrap();
        assert_eq!((r.imported, r.in_library), (1, 1));
    }
}
