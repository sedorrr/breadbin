//! Kommandozeile zum Prüfen des Scanners ohne Oberfläche.
//!
//! c64scan --help                             diese Hilfe
//! c64scan <datenbank.sqlite> [--quick] <ordner>...   Ordner aufnehmen und scannen
//!                                            (--quick: Schnellscan für diese Ordner einschalten)
//! c64scan <datenbank.sqlite> --groups [<text>]          zusammengefasste Spiele zeigen
//! c64scan <datenbank.sqlite> --show <text> Suchen und Details ausgeben
//!
//! c64scan --probe <host> [<ordner>]          Gerät prüfen, Lader hochladen (kein Start)
//! c64scan --discover | --wake <host> <wlan-mac> | --poweroff <host>
//! c64scan --clean <host> <startordner>       von Breadbin abgelegte Dateien löschen
//! c64scan <db> --cleanup                    Übersicht zum Aufräumen (doppelt, beschädigt, ohne Angaben)
//! c64scan <db> --csv <datei.csv>                   alles als CSV exportieren
//! c64scan <db> --launch <host> <startordner> <suchtext|sha1>   ersten Treffer bzw. genau diesen Inhalt starten (Reset!)
//! c64scan <db> --screenshot <host> <datei.png> [voll]   Bildschirmfoto über den Videostrom
//! c64scan <db> --share-export <bilderordner> <datei.zip>   eigene Angaben als Austauschdatei
//! c64scan <db> --share-import <bilderordner> <datei.zip|ordner>   Austauschdatei/Datenordner einlesen
//! c64scan <db> --share-dir <bilderordner> <ordner>   Beitrag im Aufbau der Gemeinschaftsdatenbank schreiben
//! c64scan <db> --share-submit <bilderordner> <url> [name]   Beitrag an den Server schicken (nur Neues/Geändertes)
//! c64scan <db> --share-mark-submitted          alles Teilbare als schon eingesandt markieren
//! c64scan <db> --share-update <bilderordner> <url>   Gemeinschaftsdaten laden und einlesen
//! c64scan <db> --copy <host> <ordner> <sha1>...   Spiele in einen Ordner auf dem Gerät kopieren
//! c64scan <db> --saves <host> <startordner> <zielordner>   geänderte Disketten (Spielstände) holen
//! c64scan <db> --own-image <bilderordner> <art> <bild> <sha1>...   eigenes Bild setzen (ss, sstitle, box-2D)
//! c64scan <db> --own-json <datei.json> [liste]   eigene Angaben einspielen ([{sha1s:[…], title, year, …}], ersetzt abgerufene);
//!                                            Inhalte mit eigenen Angaben bleiben unberührt, optional in eine Liste
//!
//! Ein Ordner kann auch auf einem Ultimate-Gerät liegen: `ftp://192.0.2.168/USB2/Spiele`.

use anyhow::{Context, Result};
use breadbin_core::{db, ftp, launch, power, scan};
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// Aufrufhilfe: die `//!`-Zeilen oben in dieser Datei.
fn usage() -> String {
    include_str!("c64scan.rs")
        .lines()
        .take_while(|l| l.starts_with("//!"))
        .map(|l| l.trim_start_matches("//!").strip_prefix(' ').unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((dbfile, rest)) = args.split_first() else {
        eprintln!("{}", usage());
        std::process::exit(2);
    };
    if matches!(dbfile.as_str(), "-h" | "--help" | "help") {
        println!("{}", usage());
        return Ok(());
    }
    // Geräte suchen, einschalten, ausschalten.
    match dbfile.as_str() {
        "--discover" => {
            for f in power::discover()? {
                println!("{f:?}");
            }
            return Ok(());
        }
        "--wake" => {
            let t =
                power::wake_and_wait(&rest[0], &rest[1], "", std::time::Duration::from_secs(60))?;
            println!("{} antwortet nach {:.1} s", rest[0], t.as_secs_f32());
            return Ok(());
        }
        "--poweroff" => {
            println!("{:?}", power::power_off(&rest[0], "")?);
            return Ok(());
        }
        "--clean" => {
            let dev = launch::Device {
                id: 0,
                name: rest[0].clone(),
                host: rest[0].clone(),
                product: String::new(),
                upload_dir: rest[1].clone(),
                wifi_mac: String::new(),
            };
            let (deleted, kept) = launch::clean_upload_dir(&dev, "")?;
            println!("{deleted} gelöscht, {kept} fremde Dateien behalten");
            return Ok(());
        }
        _ => {}
    }

    // Geräteprüfung ohne Start: Info, Laufwerke, Lader hochladen und zurücklesen.
    if dbfile == "--probe" {
        let (host, dir) = (
            &rest[0],
            rest.get(1)
                .map_or("/USB0/Claude/Breadbin_Test", String::as_str),
        );
        let info = launch::probe(host, "")?;
        println!("{info:?}");
        let kernal = launch::Rest::new(host, "").kernal();
        println!("KERNAL: {kernal}");
        for h in launch::device_hints(&info.product, &info.firmware, &kernal) {
            println!("Hinweis: {h}");
        }
        let mut f = ftp::Ftp::connect(host, "")?;
        let mut path = String::new();
        for part in dir.split('/').filter(|p| !p.is_empty()) {
            path = format!("{path}/{part}");
            if path.matches('/').count() > 1 {
                f.mkdir(&path)?;
            }
        }
        let target = format!("{dir}/breadbin-lader.prg");
        f.store(&target, launch::LADER_PRG)?;
        let back = f.retrieve(&target, 4096)?;
        println!(
            "{target}: {} Byte hochgeladen, zurückgelesen gleich: {}",
            launch::LADER_PRG.len(),
            back == launch::LADER_PRG
        );
        f.quit();
        return Ok(());
    }

    // Bildschirmfoto über den Videostrom: --screenshot <host> <datei.png> [voll]
    if rest.first().map(String::as_str) == Some("--screenshot") {
        let (host, out) = (&rest[1], &rest[2]);
        let t = std::time::Instant::now();
        let frame =
            breadbin_core::screenshot::capture(host, "", std::time::Duration::from_secs(6))?;
        let png = breadbin_core::screenshot::to_png(
            &frame,
            rest.get(3).map(String::as_str) != Some("voll"),
        )?;
        std::fs::write(out, &png)?;
        println!(
            "{out}: 384×{} empfangen in {:.1} s, {} Byte PNG",
            frame.height,
            t.elapsed().as_secs_f64(),
            png.len()
        );
        return Ok(());
    }

    // Unbekannte Option an erster Stelle ist kein Datenbankname – sonst entstünde eine leere Datei „--xyz“.
    if dbfile.starts_with('-') {
        eprintln!("Unbekannte Option {dbfile}\n\n{}", usage());
        std::process::exit(2);
    }
    let conn = db::open(Path::new(dbfile))?;

    // Teilen: Austauschdatei schreiben bzw. einlesen (auch einen ausgepackten Datenordner).
    if rest.first().map(String::as_str) == Some("--share-export") {
        let n = breadbin_core::share::export(
            &conn,
            Path::new(&rest[1]),
            Path::new(&rest[2]),
            "c64scan",
        )?;
        println!("{n} Spiele nach {} geschrieben", rest[2]);
        return Ok(());
    }
    if rest.first().map(String::as_str) == Some("--share-dir") {
        let n = breadbin_core::share::export_dir(
            &conn,
            Path::new(&rest[1]),
            Path::new(&rest[2]),
            "c64scan",
        )?;
        println!("{n} Spiele nach {} geschrieben", rest[2]);
        return Ok(());
    }
    if rest.first().map(String::as_str) == Some("--share-update") {
        let file = breadbin_core::share::download(&rest[2], "c64scan")?;
        let mut conn = conn;
        let r = breadbin_core::share::import(&mut conn, Path::new(&rest[1]), &file);
        let _ = std::fs::remove_file(&file);
        println!("{:?}", r?);
        return Ok(());
    }
    if rest.first().map(String::as_str) == Some("--share-submit") {
        let name = rest.get(3).map_or("", String::as_str);
        let r = breadbin_core::share::submit(
            &conn,
            Path::new(&rest[1]),
            &rest[2],
            name,
            "",
            "c64scan",
        )?;
        println!("{r:?}");
        return Ok(());
    }
    // Alles Teilbare als eingesandt markieren (nach einem Einsenden ohne Merkliste): --share-mark-submitted
    if rest.first().map(String::as_str) == Some("--share-mark-submitted") {
        let games = breadbin_core::share::shareable(&conn)?;
        let prints: Vec<(String, String)> = games
            .iter()
            .map(|s| (s.0.sha1.clone(), breadbin_core::share::fingerprint(s)))
            .collect();
        breadbin_core::share::mark_submitted(&conn, &prints)?;
        println!("{} Spiele als eingesandt markiert", prints.len());
        return Ok(());
    }
    // Auf ein Gerät kopieren: --copy <host> <ordner> <sha1>...
    if rest.first().map(String::as_str) == Some("--copy") {
        let (host, dir) = (&rest[1], &rest[2]);
        let dev = launch::Device {
            id: 0,
            name: host.clone(),
            host: host.clone(),
            product: String::new(),
            upload_dir: dir.clone(),
            wifi_mac: String::new(),
        };
        let r = launch::copy_to_device(&conn, &rest[3..], &dev, "", &|_| String::new(), dir)?;
        println!("{r:#?}");
        return Ok(());
    }
    // Spielstände holen: --saves <host> <startordner> <zielordner>
    if rest.first().map(String::as_str) == Some("--saves") {
        let (host, dir, target) = (&rest[1], &rest[2], &rest[3]);
        let dev = launch::Device {
            id: 0,
            name: host.clone(),
            host: host.clone(),
            product: String::new(),
            upload_dir: dir.clone(),
            wifi_mac: String::new(),
        };
        for f in launch::fetch_saves(&conn, &dev, "", Path::new(target), None)? {
            println!("geholt: {f}");
        }
        return Ok(());
    }
    // Eigenes Bild setzen: --own-image <bilderordner> <ss|sstitle|box-2D> <bild> <sha1>...
    if rest.first().map(String::as_str) == Some("--own-image") {
        let (media, kind, file) = (Path::new(&rest[1]), rest[2].as_str(), Path::new(&rest[3]));
        let sha1s: Vec<String> = rest[4..].to_vec();
        let ext = file
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        breadbin_core::metadata::set_own_image(
            &conn,
            &sha1s,
            kind,
            Some((&std::fs::read(file)?, ext)),
            media,
        )?;
        println!("{kind} für {} Inhalte gesetzt", sha1s.len());
        return Ok(());
    }
    // Eigene Angaben aus JSON: --own-json <datei.json> [liste]
    if rest.first().map(String::as_str) == Some("--own-json") {
        #[derive(serde::Deserialize)]
        struct Item {
            sha1s: Vec<String>,
            #[serde(flatten)]
            meta: breadbin_core::metadata::OwnMeta,
        }
        let items: Vec<Item> =
            serde_json::from_slice(&std::fs::read(&rest[1])?).context("JSON lesen")?;
        let list = rest.get(2).map(|n| db::create_list(&conn, n)).transpose()?;
        let (mut saved, mut skipped) = (0, 0);
        for it in &items {
            let mut fresh = Vec::new();
            for sha1 in &it.sha1s {
                // Selbst eingetragene Angaben bleiben; abgerufene (ScreenScraper, Gemeinschaft) ersetzt
                // die eigene Fassung – sie ist teilbar.
                let has: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM metadata WHERE sha1 = ? AND status = 'ok' AND provider LIKE 'own%')",
                    [sha1],
                    |r| r.get(0),
                )?;
                if has {
                    skipped += 1;
                } else {
                    fresh.push(sha1.clone());
                }
            }
            // In die Liste kommen alle Inhalte des Eintrags, auch übersprungene.
            if let Some(id) = list {
                db::set_in_list(&conn, id, &it.sha1s, true)?;
            }
            if fresh.is_empty() {
                continue;
            }
            breadbin_core::metadata::save_own(&conn, &fresh, &it.meta, true)?;
            saved += fresh.len();
            println!("{}: {} Inhalte", it.meta.title, fresh.len());
        }
        println!(
            "{saved} Inhalte mit eigenen Angaben, {skipped} übersprungen (hatten schon eigene Angaben)"
        );
        return Ok(());
    }
    if rest.first().map(String::as_str) == Some("--share-import") {
        let mut conn = conn;
        let r = breadbin_core::share::import(&mut conn, Path::new(&rest[1]), Path::new(&rest[2]))?;
        println!("{r:?}");
        return Ok(());
    }

    // Start auf einem Gerät: erster Treffer der Suche. Setzt den C64 zurück!
    if matches!(
        rest.first().map(String::as_str),
        Some("--launch" | "--insert")
    ) {
        // --insert: Disk-Abbild nur einlegen, ohne Reset (Diskwechsel).
        let insert = rest[0] == "--insert";
        let (host, dir, query) = (&rest[1], &rest[2], rest.get(3).map_or("", String::as_str));
        // Eine SHA-1 (40 Hex-Zeichen) startet genau diesen Inhalt, sonst der erste Suchtreffer.
        let (sha1, label) = if query.len() == 40 && query.chars().all(|c| c.is_ascii_hexdigit()) {
            (query.to_ascii_lowercase(), query.to_string())
        } else {
            let row = db::search(&conn, query, None, 1)?
                .into_iter()
                .next()
                .context("kein Treffer")?;
            (row.sha1, format!("{} [{}]", row.title, row.format))
        };
        let dev = launch::Device {
            id: 0,
            name: host.clone(),
            host: host.clone(),
            product: String::new(),
            upload_dir: dir.clone(),
            wifi_mac: String::new(),
        };
        println!("{} „{label}“ …", if insert { "Lege ein" } else { "Starte" });
        let r = launch::launch(&conn, &sha1, &dev, "", &|_| String::new(), insert)?;
        println!("{} (kopiert: {})", r.message, r.copied);
        return Ok(());
    }

    if rest.first().map(String::as_str) == Some("--cleanup") {
        println!("{:#?}", db::cleanup(&conn)?);
        return Ok(());
    }

    if rest.first().map(String::as_str) == Some("--csv") {
        let (csv, n) = db::export_csv(&conn, &db::Search::default())?;
        std::fs::write(&rest[1], csv)?;
        println!("{n} Einträge nach {} geschrieben", rest[1]);
        return Ok(());
    }

    if rest.first().map(String::as_str) == Some("--groups") {
        // Zusammengefasste Spiele mit mehreren Fassungen, die größten zuerst.
        let query = rest.get(1).cloned().unwrap_or_default();
        let t = std::time::Instant::now();
        let mut rows = db::search_with(
            &conn,
            &db::Search {
                query,
                group: true,
                limit: i64::MAX,
                ..Default::default()
            },
        )?;
        let took = t.elapsed();
        let total = rows.len();
        // ALL=1 zeigt auch Einzelzeilen (zum Prüfen, was nicht zusammengefasst wurde).
        if std::env::var_os("ALL").is_none() {
            rows.retain(|r| r.variants.len() > 1);
        }
        println!(
            "{total} Zeilen, davon {} mit mehreren Fassungen ({took:.0?})",
            rows.iter().filter(|r| r.variants.len() > 1).count()
        );
        rows.sort_by_key(|r| std::cmp::Reverse(r.variants.len()));
        for r in rows.iter().take(
            std::env::var("N")
                .ok()
                .and_then(|n| n.parse().ok())
                .unwrap_or(25),
        ) {
            let disks = if r.disks > 0 {
                format!(" – {} Disks/Seiten", r.disks)
            } else {
                String::new()
            };
            println!(
                "{:>4} × {} [{}]{disks}",
                r.variants.len(),
                r.title,
                r.formats
            );
        }
        return Ok(());
    }

    if rest.first().map(String::as_str) == Some("--show") {
        let query = rest.get(1).map_or("", String::as_str);
        for row in db::search(&conn, query, None, 20)? {
            let d = db::details(&conn, &row.sha1)?;
            println!(
                "{} [{}] {:?} {:?} {:?}",
                row.title, row.format, row.image_name, row.detail, row.year
            );
            for e in &d.entries {
                println!("    {:>4} \"{}\" {}", e.blocks, e.name, e.kind);
            }
            for l in &d.locations {
                println!("    @ {} / {} {}", l.source_label, l.path, l.member);
            }
        }
        return Ok(());
    }

    let quick = rest.first().map(String::as_str) == Some("--quick");
    for folder in rest.iter().skip(quick as usize) {
        let id = match ftp_target(folder) {
            Some((host, path)) => db::add_ultimate_source(&conn, host, path, host)?,
            None => db::add_local_source(&conn, Path::new(folder))?,
        };
        db::set_source_quick(&conn, id, quick)?;
        let t = std::time::Instant::now();
        let mut last = std::time::Instant::now();
        let p = scan::scan_source(&conn, id, "", &AtomicBool::new(false), &mut |p| {
            if last.elapsed().as_secs() >= 5 {
                last = std::time::Instant::now();
                eprintln!(
                    "  … {} gelesen, {} übernommen, {} unverändert: {}",
                    p.read, p.quick, p.unchanged, p.current
                );
            }
        })?;
        println!(
            "{folder}: {} gelesen, {} übernommen, {} unverändert, {} entfernt, {} Fehler ({:.1?})",
            p.read,
            p.quick,
            p.unchanged,
            p.removed,
            p.errors.len(),
            t.elapsed()
        );
        for e in p.errors.iter().take(20) {
            println!("  ! {e}");
        }
    }
    let s = db::stats(&conn)?;
    println!(
        "Gesamt: {} Dateien, {} verschiedene Inhalte, {:?}",
        s.files, s.contents, s.by_format
    );
    Ok(())
}

fn ftp_target(arg: &str) -> Option<(&str, &str)> {
    ftp::split_location(arg)
}
