//! Ordner durchsuchen und gefundene C64-Dateien in die Datenbank eintragen.
//!
//! Ein erneuter Scan liest nur Dateien neu, deren Größe oder Änderungszeit sich
//! geändert hat. Dateien, die nicht mehr da sind, fallen am Ende heraus – aber nur,
//! wenn der Scan vollständig durchlief.
//!
//! Mit Schnellscan (je Quelle einstellbar) werden Dateien, die mit gleichem Namen und
//! gleicher Größe schon in einer anderen Quelle liegen, von dort übernommen statt gelesen.
//! Das spart bei Gerätesticks Stunden, kann aber bei gleich großen, verschiedenen Dateien
//! (etwa einem D64 mit Spielstand) danebenliegen; solche Einträge sind gekennzeichnet.

use crate::tr;
use anyhow::{Context, Result, bail};
use md5::{Digest, Md5};
use rusqlite::{Connection, params};
use serde::Serialize;
use sha1::Sha1;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Cursor, Read, Seek};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

use crate::ftp::{self, Ftp};
use crate::{db, formats, names};

/// Größte einzelne C64-Datei, die eingelesen wird (größte übliche: EasyFlash-CRT, 1 MB).
const MAX_FILE: u64 = 16 * 1024 * 1024;
/// Größtes ZIP-Archiv, das geöffnet wird (vom Gerät wird es ganz in den Speicher geladen).
const MAX_ZIP: u64 = 512 * 1024 * 1024;
/// Nach so vielen Dateien wird zwischengespeichert.
const COMMIT_EVERY: u64 = 200;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Progress {
    /// Gelesene und eingetragene C64-Dateien (auch aus ZIP-Archiven).
    pub read: u64,
    /// Seit dem letzten Scan unveränderte Dateien.
    pub unchanged: u64,
    /// Per Schnellscan aus anderen Quellen übernommene Dateien (nicht gelesen).
    pub quick: u64,
    /// Gelöschte Einträge (erst am Ende bekannt).
    pub removed: u64,
    pub errors: Vec<String>,
    pub current: String,
    pub cancelled: bool,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

pub struct Hashes {
    pub crc32: String,
    pub md5: String,
    pub sha1: String,
}

pub fn hashes(data: &[u8]) -> Hashes {
    Hashes {
        crc32: format!("{:08x}", crc32fast::hash(data)),
        md5: hex::encode(Md5::digest(data)),
        sha1: hex::encode(Sha1::digest(data)),
    }
}

/// Eine Datei (oder ein ZIP-Mitglied) eintragen. `name` liefert Titel, Jahr und Herausgeber.
#[allow(clippy::too_many_arguments)]
pub fn ingest(
    conn: &Connection,
    source_id: i64,
    path: &str,
    member: &str,
    fsize: i64,
    fmtime: i64,
    format: &str,
    name_stem: &str,
    data: &[u8],
    scan_id: i64,
) -> Result<()> {
    let h = hashes(data);
    let new_content = conn.execute(
        "INSERT OR IGNORE INTO contents(sha1, md5, crc32, size, format) VALUES(?, ?, ?, ?, ?)",
        params![h.sha1, h.md5, h.crc32, data.len() as i64, format],
    )? == 1;
    if new_content {
        let info = formats::analyze(format, data);
        conn.execute(
            "UPDATE contents SET image_name = ?, detail = ? WHERE sha1 = ?",
            params![info.image_name, info.detail, h.sha1],
        )?;
        let mut st = conn.prepare_cached(
            "INSERT INTO entries(sha1, idx, name, kind, blocks) VALUES(?, ?, ?, ?, ?)",
        )?;
        for (i, e) in info.entries.iter().enumerate() {
            st.execute(params![h.sha1, i as i64, e.name, e.kind, e.blocks])?;
        }
    }
    let n = names::parse_file_stem(name_stem);
    conn.execute(
        "INSERT OR REPLACE INTO files(source_id, path, member, fsize, fmtime, sha1, title, year, publisher, seen)
         VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![source_id, path, member, fsize, fmtime, h.sha1, n.title, n.year, n.publisher, scan_id],
    )?;
    Ok(())
}

fn stem_and_ext(name: &str) -> (&str, &str) {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match base.rfind('.') {
        Some(i) if i > 0 => (&base[..i], &base[i + 1..]),
        _ => (base, ""),
    }
}

fn read_limited(r: impl Read, limit: u64) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    r.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        bail!(tr!(
            "größer als {} MB",
            "larger than {} MB",
            limit / 1024 / 1024
        ));
    }
    Ok(data)
}

/// Gleiche Datei wie beim letzten Scan? Dann nur als gesehen markieren.
/// Ohne Schnellscan gelten übernommene Einträge nicht als unverändert, damit sie gelesen werden.
fn mark_if_unchanged(
    conn: &Connection,
    source_id: i64,
    path: &str,
    fsize: i64,
    fmtime: i64,
    scan_id: i64,
    accept_quick: bool,
) -> Result<bool> {
    let same: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE source_id = ? AND path = ? AND fsize = ? AND fmtime = ? AND (quick = 0 OR ?)",
        params![source_id, path, fsize, fmtime, accept_quick],
        |r| r.get(0),
    )?;
    if same == 0 {
        return Ok(false);
    }
    conn.execute(
        "UPDATE files SET seen = ? WHERE source_id = ? AND path = ?",
        params![scan_id, source_id, path],
    )?;
    Ok(true)
}

/// Eine Zeile aus einer anderen Quelle, die der Schnellscan übernehmen kann.
#[derive(Debug, Clone, PartialEq)]
struct Known {
    member: String,
    sha1: String,
    title: String,
    year: Option<String>,
    publisher: Option<String>,
}

/// (Dateiname klein geschrieben, Größe) → Einträge dieser Datei; `None`, wenn es unter
/// Name und Größe verschiedene Inhalte gibt – dann wird lieber gelesen.
type QuickMap = HashMap<(String, i64), Option<Vec<Known>>>;

fn base_name(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_lowercase()
}

/// Bekannte Dateien aller anderen Quellen, die selbst gelesen wurden.
fn quick_map(conn: &Connection, source_id: i64) -> Result<QuickMap> {
    let mut st = conn.prepare(
        "SELECT source_id, path, fsize, member, sha1, title, year, publisher FROM files
         WHERE source_id <> ? AND quick = 0 ORDER BY source_id, path, member",
    )?;
    // Erst je Datei (Quelle + Pfad) sammeln – ein ZIP hat mehrere Mitglieder.
    let mut per_file: Vec<((String, i64), Vec<Known>)> = Vec::new();
    let mut last: Option<(i64, String)> = None;
    let rows = st.query_map([source_id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            Known {
                member: r.get(3)?,
                sha1: r.get(4)?,
                title: r.get(5)?,
                year: r.get(6)?,
                publisher: r.get(7)?,
            },
        ))
    })?;
    for row in rows {
        let (src, path, fsize, known) = row?;
        let this = (src, path);
        if last.as_ref() != Some(&this) {
            per_file.push(((base_name(&this.1), fsize), Vec::new()));
            last = Some(this);
        }
        per_file.last_mut().unwrap().1.push(known);
    }
    let mut map = QuickMap::new();
    for (key, rows) in per_file {
        let sig = |v: &[Known]| {
            v.iter()
                .map(|k| (k.member.clone(), k.sha1.clone()))
                .collect::<Vec<_>>()
        };
        match map.get(&key) {
            None => {
                map.insert(key, Some(rows));
            }
            Some(Some(prev)) if sig(prev) != sig(&rows) => {
                map.insert(key, None);
            }
            _ => {}
        }
    }
    Ok(map)
}

trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// Trägt eine Datei ein; ZIP-Archive werden nach C64-Dateien durchsucht.
/// `open(limit)` liefert den Inhalt (lokale Datei oder FTP-Download) und wird nur
/// aufgerufen, wenn sich die Datei seit dem letzten Scan geändert hat.
/// Gibt die Zahl der eingetragenen Dateien zurück.
#[allow(clippy::too_many_arguments)]
fn scan_item(
    conn: &Connection,
    source_id: i64,
    rel: &str,
    fsize: i64,
    fmtime: i64,
    scan_id: i64,
    progress: &mut Progress,
    quick: Option<&QuickMap>,
    open: &mut dyn FnMut(u64) -> Result<Box<dyn ReadSeek>>,
) -> Result<u64> {
    let (stem, ext) = stem_and_ext(rel);
    let is_zip = ext.eq_ignore_ascii_case("zip");
    let Some(format) = formats::format_for_ext(ext).or(is_zip.then_some("ZIP")) else {
        return Ok(0);
    };
    if mark_if_unchanged(
        conn,
        source_id,
        rel,
        fsize,
        fmtime,
        scan_id,
        quick.is_some(),
    )? {
        progress.unchanged += 1;
        return Ok(0);
    }
    conn.execute(
        "DELETE FROM files WHERE source_id = ? AND path = ?",
        params![source_id, rel],
    )?;

    if let Some(Some(known)) = quick.and_then(|m| m.get(&(base_name(rel), fsize))) {
        let mut st = conn.prepare_cached(
            "INSERT INTO files(source_id, path, member, fsize, fmtime, sha1, title, year, publisher, seen, quick)
             VALUES(?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1)",
        )?;
        for k in known {
            st.execute(params![
                source_id,
                rel,
                k.member,
                fsize,
                fmtime,
                k.sha1,
                k.title,
                k.year,
                k.publisher,
                scan_id
            ])?;
        }
        progress.quick += 1;
        return Ok(0);
    }

    if !is_zip {
        let data = read_limited(open(MAX_FILE)?, MAX_FILE)?;
        ingest(
            conn, source_id, rel, "", fsize, fmtime, format, stem, &data, scan_id,
        )?;
        return Ok(1);
    }

    if fsize as u64 > MAX_ZIP {
        bail!(tr!(
            "ZIP größer als {} MB, übersprungen",
            "ZIP larger than {} MB, skipped",
            MAX_ZIP / 1024 / 1024
        ));
    }
    let mut zip = zip::ZipArchive::new(open(MAX_ZIP)?)
        .context(tr!("kein gültiges ZIP", "not a valid ZIP"))?;
    let mut count = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        if f.is_dir() {
            continue;
        }
        let member = f.name()?.to_string();
        let (mstem, mext) = stem_and_ext(&member);
        let Some(mformat) = formats::format_for_ext(mext) else {
            continue;
        };
        if mstem.starts_with("._") || member.contains("__MACOSX/") {
            continue;
        }
        if f.size() > MAX_FILE {
            progress.errors.push(tr!(
                "{rel} → {member}: zu groß, übersprungen",
                "{rel} → {member}: too large, skipped"
            ));
            continue;
        }
        let data = read_limited(&mut f, MAX_FILE)?;
        ingest(
            conn, source_id, rel, &member, fsize, fmtime, mformat, mstem, &data, scan_id,
        )?;
        count += 1;
    }
    Ok(count)
}

/// Gemeinsamer Rahmen eines Scans: Kennung, Transaktionen, Aufräumen am Ende.
struct Run<'a> {
    conn: &'a Connection,
    source_id: i64,
    scan_id: i64,
    since_commit: u64,
    quick: Option<QuickMap>,
    p: Progress,
}

impl<'a> Run<'a> {
    fn begin(conn: &'a Connection, source_id: i64) -> Result<Self> {
        // Kennung muss größer sein als die des letzten Scans, auch wenn beide in dieselbe
        // Millisekunde fallen – sonst blieben gelöschte Dateien stehen.
        let last: i64 =
            conn.query_row("SELECT COALESCE(MAX(seen), 0) FROM files", [], |r| r.get(0))?;
        let quick = if db::get_source(conn, source_id)?.quick {
            Some(quick_map(conn, source_id)?)
        } else {
            None
        };
        conn.execute_batch("BEGIN")?;
        Ok(Run {
            conn,
            source_id,
            scan_id: now_ms().max(last + 1),
            since_commit: 0,
            quick,
            p: Progress::default(),
        })
    }

    fn item(
        &mut self,
        rel: &str,
        fsize: i64,
        fmtime: i64,
        open: &mut dyn FnMut(u64) -> Result<Box<dyn ReadSeek>>,
        report: &mut dyn FnMut(&Progress),
    ) -> Result<()> {
        self.p.current = rel.to_string();
        match scan_item(
            self.conn,
            self.source_id,
            rel,
            fsize,
            fmtime,
            self.scan_id,
            &mut self.p,
            self.quick.as_ref(),
            open,
        ) {
            Ok(n) => self.p.read += n,
            Err(e) => self.p.errors.push(format!("{rel}: {e:#}")),
        }
        self.since_commit += 1;
        if self.since_commit >= COMMIT_EVERY {
            self.conn.execute_batch("COMMIT; BEGIN")?;
            self.since_commit = 0;
        }
        report(&self.p);
        Ok(())
    }

    fn finish(mut self, report: &mut dyn FnMut(&Progress)) -> Result<Progress> {
        if !self.p.cancelled {
            self.p.removed = self.conn.execute(
                "DELETE FROM files WHERE source_id = ? AND seen <> ?",
                params![self.source_id, self.scan_id],
            )? as u64;
            db::remove_orphans(self.conn)?;
            self.conn.execute(
                "UPDATE sources SET last_scan = ? WHERE id = ?",
                params![self.scan_id / 1000, self.source_id],
            )?;
        }
        self.conn.execute_batch("COMMIT")?;
        self.p.current.clear();
        report(&self.p);
        Ok(self.p)
    }
}

/// Scannt eine Quelle: lokaler Ordner oder Ordner auf einem Ultimate-Gerät.
pub fn scan_source(
    conn: &Connection,
    source_id: i64,
    ftp_password: &str,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(&Progress),
) -> Result<Progress> {
    match db::get_source(conn, source_id)?.kind.as_str() {
        "ultimate" => scan_ultimate(conn, source_id, ftp_password, cancel, report),
        _ => scan_local(conn, source_id, cancel, report),
    }
}

/// Durchsucht einen lokalen Ordner (auch ein eingebundenes Netzlaufwerk).
///
/// `report` wird nach jeder Datei aufgerufen; über `cancel` lässt sich abbrechen.
pub fn scan_local(
    conn: &Connection,
    source_id: i64,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(&Progress),
) -> Result<Progress> {
    let source = db::get_source(conn, source_id)?;
    let root = Path::new(&source.location);
    // Wichtig: ein nicht eingebundenes Netzlaufwerk darf nicht als „alles gelöscht“ gelten.
    if !root.is_dir() {
        bail!(tr!(
            "Ordner {} ist nicht erreichbar (Netzlaufwerk nicht verbunden?)",
            "Folder {} is not reachable (network drive not connected?)",
            root.display()
        ));
    }
    let mut run = Run::begin(conn, source_id)?;

    let walker = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !e.file_name().to_string_lossy().starts_with('.'));
    for entry in walker {
        if cancel.load(Ordering::Relaxed) {
            run.p.cancelled = true;
            break;
        }
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                run.p.errors.push(e.to_string());
                continue;
            }
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        let (fsize, fmtime) = match entry.metadata() {
            Ok(m) => (m.len() as i64, mtime_secs(&m)),
            Err(e) => {
                run.p.errors.push(format!("{rel}: {e}"));
                continue;
            }
        };
        let path = entry.path().to_path_buf();
        run.item(
            &rel,
            fsize,
            fmtime,
            &mut |_| Ok(Box::new(File::open(&path)?)),
            report,
        )?;
    }
    run.finish(report)
}

/// Durchsucht einen Ordner auf einem Ultimate-Gerät per FTP (`ftp://host/USB0/Spiele`).
///
/// Unverändert gilt eine Datei bei gleicher Größe und gleicher MLSD-Zeit; kann das Gerät
/// kein MLSD, nur bei gleicher Größe.
pub fn scan_ultimate(
    conn: &Connection,
    source_id: i64,
    password: &str,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(&Progress),
) -> Result<Progress> {
    let source = db::get_source(conn, source_id)?;
    let (host, root) = ftp::split_location(&source.location).with_context(|| {
        tr!(
            "ungültiger Geräteort {}",
            "invalid device location {}",
            source.location
        )
    })?;
    let mut ftp = Ftp::connect(host, password)?;
    // Ein fehlender Ordner (z. B. Stick abgezogen) liefert beim Ultimate eine leere Liste –
    // das darf nicht als „alles gelöscht“ gelten.
    if !ftp.is_dir(root)? {
        bail!(tr!(
            "Ordner {root} gibt es auf {host} nicht (Stick nicht eingesteckt?)",
            "Folder {root} does not exist on {host} (stick not plugged in?)"
        ));
    }
    let root = root.trim_end_matches('/').to_string();
    let mut run = Run::begin(conn, source_id)?;
    let mut dirs = vec![String::new()];

    'walk: while let Some(rel_dir) = dirs.pop() {
        let abs_dir = if rel_dir.is_empty() {
            root.clone()
        } else {
            format!("{root}/{rel_dir}")
        };
        let entries = match ftp.list(&abs_dir) {
            Ok(e) => e,
            Err(e) => {
                // Verbindung abgerissen: einmal neu verbinden, sonst den Lauf abbrechen,
                // damit nichts als gelöscht gilt.
                ftp = Ftp::connect(host, password).with_context(|| format!("{abs_dir}: {e:#}"))?;
                ftp.list(&abs_dir)?
            }
        };
        let mut subdirs = Vec::new();
        for e in entries {
            if cancel.load(Ordering::Relaxed) {
                run.p.cancelled = true;
                break 'walk;
            }
            if e.name.starts_with('.') {
                continue;
            }
            let rel = if rel_dir.is_empty() {
                e.name.clone()
            } else {
                format!("{rel_dir}/{}", e.name)
            };
            if e.is_dir {
                subdirs.push(rel);
                continue;
            }
            let abs = format!("{root}/{rel}");
            let ftp_ref = &mut ftp;
            run.item(
                &rel,
                e.size as i64,
                e.modify,
                &mut |limit| Ok(Box::new(Cursor::new(ftp_ref.retrieve(&abs, limit)?))),
                report,
            )?;
        }
        // Rückwärts auf den Stapel, damit die Ordner alphabetisch abgearbeitet werden.
        subdirs.sort_by_key(|d| std::cmp::Reverse(d.to_lowercase()));
        dirs.extend(subdirs);
    }
    ftp.quit();
    run.finish(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::testdata;
    use std::io::Write;

    fn write(dir: &Path, rel: &str, data: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }

    fn zip_with(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut z = zip::ZipWriter::new(&mut buf);
        for (name, data) in files {
            z.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(data).unwrap();
        }
        z.finish().unwrap();
        buf.into_inner()
    }

    fn scan(conn: &Connection, id: i64) -> Progress {
        scan_local(conn, id, &AtomicBool::new(false), &mut |_| {}).unwrap()
    }

    #[test]
    fn ordner_scannen_doppelte_und_zip() {
        let dir = tempfile::tempdir().unwrap();
        let d64 = testdata::d64(b"GIANA", &[(b"GIANA SISTERS", 0x82, 150)]);
        write(
            dir.path(),
            "Spiele/Great Giana Sisters (1987)(Rainbow Arts).d64",
            &d64,
        );
        write(dir.path(), "Kopie/giana.D64", &d64);
        write(dir.path(), "Module/ef.crt", &testdata::crt("EF GAME", 32));
        write(dir.path(), "readme.txt", b"kein C64");
        write(dir.path(), ".versteckt/x.prg", &[1, 8, 0]);
        write(
            dir.path(),
            "Archiv/pack.zip",
            &zip_with(&[("a/boulder.prg", &[1, 8, 1, 2]), ("info.nfo", b"x")]),
        );

        let conn = db::open_in_memory().unwrap();
        let id = db::add_local_source(&conn, dir.path()).unwrap();
        let p = scan(&conn, id);
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        assert_eq!(p.read, 4);

        let rows = db::search(&conn, "", None, 100).unwrap();
        assert_eq!(rows.len(), 3, "D64 doppelt vorhanden, aber ein Inhalt");
        let giana = rows.iter().find(|r| r.format == "D64").unwrap();
        assert_eq!(giana.locations, 2);
        assert_eq!(giana.year.as_deref(), Some("1987"));
        assert_eq!(giana.publisher.as_deref(), Some("Rainbow Arts"));

        // Suche findet auch Namen im Disk-Verzeichnis und Modulnamen.
        assert_eq!(db::search(&conn, "sisters", None, 10).unwrap().len(), 1);
        assert_eq!(db::search(&conn, "EF GAME", None, 10).unwrap().len(), 1);
        assert_eq!(
            db::search(&conn, "", Some("PRG"), 10).unwrap()[0].title,
            "boulder"
        );

        let d = db::details(&conn, &giana.sha1).unwrap();
        assert_eq!(d.entries[0].name, "GIANA SISTERS");
        assert_eq!(d.locations.len(), 2);
        let zip = db::search(&conn, "boulder", None, 10).unwrap();
        let d = db::details(&conn, &zip[0].sha1).unwrap();
        assert_eq!(d.locations[0].path, "Archiv/pack.zip");
        assert_eq!(d.locations[0].member, "a/boulder.prg");
    }

    #[test]
    fn schnellscan_uebernimmt_gleichen_namen_und_groesse() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        write(a.path(), "Spiele/Giana.d64", &[1, 2, 3, 4]);
        write(
            a.path(),
            "pack.zip",
            &zip_with(&[("x.prg", &[1, 8, 1]), ("y.prg", &[1, 8, 2])]),
        );
        write(a.path(), "eins/dopp.prg", &[1, 8, 5]);
        write(a.path(), "zwei/dopp.prg", &[1, 8, 6]);
        // Auf dem „Stick“: gleiche Namen und Größen, aber andere Inhalte – der Schnellscan
        // merkt das nicht, genau das ist sein bekanntes Risiko.
        write(b.path(), "GIANA.D64", &[9, 9, 9, 9]);
        write(
            b.path(),
            "Archiv/pack.zip",
            &zip_with(&[("x.prg", &[1, 8, 1]), ("y.prg", &[1, 8, 2])]),
        );
        write(b.path(), "dopp.prg", &[1, 8, 7]);
        write(b.path(), "neu.prg", &[1, 8, 8, 8]);

        let conn = db::open_in_memory().unwrap();
        let ia = db::add_local_source(&conn, a.path()).unwrap();
        let ib = db::add_local_source(&conn, b.path()).unwrap();
        assert_eq!(scan(&conn, ia).read, 5);
        db::set_source_quick(&conn, ib, true).unwrap();
        let p = scan(&conn, ib);
        assert!(p.errors.is_empty(), "{:?}", p.errors);
        // GIANA.D64 und pack.zip übernommen; dopp.prg ist mehrdeutig und neu.prg unbekannt.
        assert_eq!((p.quick, p.read), (2, 2));
        let sha_a: String = conn
            .query_row(
                "SELECT sha1 FROM files WHERE source_id = ? AND path = 'Spiele/Giana.d64'",
                [ia],
                |r| r.get(0),
            )
            .unwrap();
        let d = db::details(&conn, &sha_a).unwrap();
        assert_eq!(d.locations.len(), 2);
        assert!(d.locations.iter().any(|l| l.quick && l.path == "GIANA.D64"));
        let members: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE source_id = ? AND quick = 1",
                [ib],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(members, 3, "D64 plus beide ZIP-Mitglieder");

        // Zweiter Schnellscan: alles unverändert.
        let p = scan(&conn, ib);
        assert_eq!((p.quick, p.read, p.unchanged), (0, 0, 4));

        // Schnellscan aus: Übernommenes wird jetzt doch gelesen und korrigiert.
        db::set_source_quick(&conn, ib, false).unwrap();
        let p = scan(&conn, ib);
        assert_eq!((p.quick, p.read, p.unchanged), (0, 3, 2));
        let wrong: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM files WHERE source_id = ? AND sha1 = ?",
                params![ib, sha_a],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(wrong, 0, "der falsch übernommene Inhalt ist ersetzt");
    }

    #[test]
    fn zweiter_scan_ist_inkrementell_und_raeumt_auf() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.prg", &[1, 8, 0]);
        write(dir.path(), "b.prg", &[1, 8, 1]);
        let conn = db::open_in_memory().unwrap();
        let id = db::add_local_source(&conn, dir.path()).unwrap();
        assert_eq!(scan(&conn, id).read, 2);

        std::fs::remove_file(dir.path().join("b.prg")).unwrap();
        let p = scan(&conn, id);
        assert_eq!((p.read, p.unchanged, p.removed), (0, 1, 1));
        assert_eq!(
            db::stats(&conn).unwrap().contents,
            1,
            "verwaister Inhalt gelöscht"
        );
    }

    #[test]
    fn abbruch_loescht_nichts_und_fehlender_ordner_bricht_ab() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.prg", &[1, 8, 0]);
        let conn = db::open_in_memory().unwrap();
        let id = db::add_local_source(&conn, dir.path()).unwrap();
        scan(&conn, id);

        let p = scan_local(&conn, id, &AtomicBool::new(true), &mut |_| {}).unwrap();
        assert!(p.cancelled);
        assert_eq!(db::stats(&conn).unwrap().files, 1);

        let path = dir.path().to_path_buf();
        drop(dir);
        assert!(!path.exists());
        assert!(scan_local(&conn, id, &AtomicBool::new(false), &mut |_| {}).is_err());
        assert_eq!(
            db::stats(&conn).unwrap().files,
            1,
            "Einträge bleiben bei fehlendem Ordner"
        );
    }
}
