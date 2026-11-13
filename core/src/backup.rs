//! Sicherung der ganzen Bibliothek: Datenbank (Quellen, Geräte, eigene Angaben, Listen, Notizen,
//! Einstellungen) und alle Bilder als eine ZIP-Datei – zum Zurückspielen nach einem Rechnerwechsel.
//! Passwörter liegen im Schlüsselbund und gehören nicht dazu.

use crate::tr;
use anyhow::{Context, Result, ensure};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;

pub const FORMAT: &str = "breadbin-backup";
pub const VERSION: u32 = 1;
const MANIFEST: &str = "breadbin-backup.json";
const DB_NAME: &str = "spiele.sqlite";
const MAX_DB: u64 = 4 * 1024 * 1024 * 1024;
const MAX_IMAGE: u64 = 16 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub created: i64,
    pub app: String,
    pub images: usize,
}

#[derive(Debug, Default, Serialize)]
pub struct BackupInfo {
    pub images: usize,
    pub bytes: u64,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Bibliothek sichern. Die Datenbank wird mit `VACUUM INTO` kopiert, also auch bei laufender App
/// in einem stimmigen Zustand.
pub fn backup(conn: &Connection, media_dir: &Path, out: &Path, app: &str) -> Result<BackupInfo> {
    let tmp =
        std::env::temp_dir().join(format!("breadbin-sicherung-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    conn.execute("VACUUM INTO ?", [tmp.to_string_lossy()])?;
    let result = (|| -> Result<BackupInfo> {
        let mut images: Vec<std::path::PathBuf> = match std::fs::read_dir(media_dir) {
            Ok(rd) => rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_file())
                .collect(),
            Err(_) => Vec::new(),
        };
        images.sort();
        let part = out.with_extension("part");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&part)?);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let manifest = Manifest {
            format: FORMAT.into(),
            version: VERSION,
            created: now(),
            app: app.into(),
            images: images.len(),
        };
        zip.start_file(MANIFEST, opts)?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        zip.start_file(DB_NAME, opts)?;
        std::io::copy(&mut std::fs::File::open(&tmp)?, &mut zip)?;
        for p in &images {
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .context("Dateiname")?;
            // Bilder sind schon gepackt (PNG, JPG): nicht noch einmal komprimieren.
            zip.start_file(format!("media/{name}"), stored)?;
            std::io::copy(&mut std::fs::File::open(p)?, &mut zip)?;
        }
        zip.finish()?;
        std::fs::rename(&part, out)?;
        Ok(BackupInfo {
            images: images.len(),
            bytes: std::fs::metadata(out)?.len(),
        })
    })();
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Prüft eine Sicherung und packt die Datenbank nach `db_out` aus (noch nicht an ihren Platz).
pub fn unpack_db(path: &Path, db_out: &Path) -> Result<Manifest> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)
        .context(tr!("keine gültige Sicherung", "not a valid backup"))?;
    let manifest: Manifest = {
        let mut f = zip
            .by_name(MANIFEST)
            .context(tr!("keine Breadbin-Sicherung", "not a Breadbin backup"))?;
        let mut data = Vec::new();
        (&mut f).take(64 * 1024).read_to_end(&mut data)?;
        serde_json::from_slice(&data)
            .context(tr!("keine Breadbin-Sicherung", "not a Breadbin backup"))?
    };
    ensure!(
        manifest.format == FORMAT,
        tr!("keine Breadbin-Sicherung", "not a Breadbin backup")
    );
    ensure!(
        manifest.version <= VERSION,
        tr!(
            "Die Sicherung stammt aus einer neueren Fassung von Breadbin.",
            "The backup comes from a newer version of Breadbin."
        )
    );
    let mut f = zip
        .by_name(DB_NAME)
        .context(tr!("Sicherung ohne Datenbank", "backup without database"))?;
    let mut out = std::fs::File::create(db_out)?;
    std::io::copy(&mut (&mut f).take(MAX_DB), &mut out)?;
    drop(out);
    // Lässt sie sich öffnen und hat sie die Tabellen?
    let conn = Connection::open(db_out)?;
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('sources', 'files', 'contents')", [], |r| r.get(0))
        .context(tr!("Datenbank in der Sicherung ist beschädigt", "database in the backup is damaged"))?;
    ensure!(
        n == 3,
        tr!(
            "Datenbank in der Sicherung ist unvollständig",
            "database in the backup is incomplete"
        )
    );
    Ok(manifest)
}

/// Bilder aus der Sicherung in den Bilderordner (vorhandene gleichen Namens werden ersetzt).
pub fn restore_media(path: &Path, media_dir: &Path) -> Result<usize> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    std::fs::create_dir_all(media_dir)?;
    let mut n = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i)?;
        let full = f.name()?.to_string();
        let Some(name) = full.strip_prefix("media/").map(str::to_string) else {
            continue;
        };
        // Nur einfache Dateinamen – nichts außerhalb des Bilderordners.
        if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
            continue;
        }
        let mut data = Vec::new();
        (&mut f).take(MAX_IMAGE + 1).read_to_end(&mut data)?;
        if data.len() as u64 > MAX_IMAGE {
            continue;
        }
        std::fs::write(media_dir.join(&name), &data)?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sichern_und_zurueckspielen() {
        let dir = std::env::temp_dir().join(format!("breadbin-backup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let media = dir.join("media");
        std::fs::create_dir_all(&media).unwrap();
        std::fs::write(media.join("own_1.png"), b"bild").unwrap();
        let db_path = dir.join("spiele.sqlite");
        let conn = crate::db::open(&db_path).unwrap();
        crate::db::create_list(&conn, "Partyspiele").unwrap();
        let out = dir.join("sicherung.zip");
        let info = backup(&conn, &media, &out, "test").unwrap();
        assert_eq!(info.images, 1);

        let restored = dir.join("neu.sqlite");
        let m = unpack_db(&out, &restored).unwrap();
        assert_eq!((m.format.as_str(), m.images), (FORMAT, 1));
        let conn2 = crate::db::open(&restored).unwrap();
        assert_eq!(crate::db::lists(&conn2).unwrap()[0].name, "Partyspiele");
        let media2 = dir.join("media2");
        assert_eq!(restore_media(&out, &media2).unwrap(), 1);
        assert_eq!(std::fs::read(media2.join("own_1.png")).unwrap(), b"bild");

        // Fremde ZIP-Datei wird abgelehnt.
        let fremd = dir.join("fremd.zip");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&fremd).unwrap());
        z.start_file("x.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        z.write_all(b"x").unwrap();
        z.finish().unwrap();
        assert!(unpack_db(&fremd, &dir.join("x.sqlite")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
