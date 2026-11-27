//! Spiele in einem Emulator auf dem Rechner starten (Standard: VICE `x64sc`, auch Denise).
//!
//! Breadbin legt eine Kopie des Inhalts in einen eigenen Ordner (auch aus ZIP-Archiven, von
//! Netzlaufwerken und vom USB-Stick eines Ultimate) und ruft den Emulator mit dieser Datei auf.
//! Die Kopie bleibt liegen: Speichert ein Spiel auf seine Diskette, ist der Stand beim nächsten
//! Start noch da. Bei Spielen mit mehreren Disketten bekommt VICE eine Wechselliste
//! (`-flipname`); die nächste Diskette legt man in VICE über „Fliplist“ ein (GTK-Fassung: Alt+N, SDL-Fassung: Menü F12).
//! VICE bekommt außerdem einen Monitor-Port: „Einlegen“ und „Bildschirmfoto“ in Breadbin gehen
//! darüber (siehe `vicemon.rs`).

use crate::db;
use crate::launch::{device_file_name, read_location};
use crate::tr;
use anyhow::{Context, Result, bail};
use rusqlite::Connection;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Aufruf, wenn nichts anderes eingestellt ist. `{file}` wird durch den Pfad ersetzt.
pub const DEFAULT_ARGS: &str = "-autostart {file}";
/// Vorgabe für Denise: Abbilder als letzte Argumente startet er selbst (`LOAD"*",8,1`, PRG, CRT …).
pub const DENISE_ARGS: &str = "{file}";

/// Vorgabe-Aufruf für dieses Programm.
pub fn default_args(emulator: &str) -> &'static str {
    if is_denise(emulator) {
        DENISE_ARGS
    } else {
        DEFAULT_ARGS
    }
}

/// Was der Emulator starten kann. VICE kennt alle diese Formate selbst (auch P00, T64 und TAP).
fn extension(format: &str) -> Option<&'static str> {
    Some(match format {
        "PRG" => "prg",
        "P00" => "p00",
        "T64" => "t64",
        "TAP" => "tap",
        "CRT" => "crt",
        "D64" => "d64",
        "D71" => "d71",
        "D81" => "d81",
        "G64" => "g64",
        _ => return None,
    })
}

#[derive(Serialize, Debug, Clone)]
pub struct EmuResult {
    pub message: String,
    /// Datei, die der Emulator bekommen hat.
    pub file: String,
}

/// Höchstzahl eingestellter Emulatoren.
pub const MAX_EMULATORS: usize = 8;

/// Ein eingestellter Emulator: Programm und Aufrufzeile (leer = Vorgabe des Programms).
#[derive(Serialize, serde::Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Emulator {
    pub path: String,
    #[serde(default)]
    pub args: String,
}

/// Schlüssel in den Einstellungen: Nr. 1 unter den alten Namen (ältere Fassungen finden ihn so
/// weiter), die übrigen als `emulator_path_2`, `emulator_args_2` …
fn keys(i: usize) -> (String, String) {
    if i == 0 {
        ("emulator_path".into(), "emulator_args".into())
    } else {
        (
            format!("emulator_path_{}", i + 1),
            format!("emulator_args_{}", i + 1),
        )
    }
}

/// Alle eingestellten Emulatoren in ihrer Reihenfolge (ohne leere).
pub fn list(conn: &Connection) -> Vec<Emulator> {
    let get = |k: &str| db::get_setting(conn, k).ok().flatten().unwrap_or_default();
    (0..MAX_EMULATORS)
        .map(|i| {
            let (p, a) = keys(i);
            Emulator {
                path: get(&p).trim().to_string(),
                args: get(&a),
            }
        })
        .filter(|e| !e.path.is_empty())
        .collect()
}

/// Speichert die Liste (leere Einträge fallen weg, höchstens `MAX_EMULATORS`). Die VICE-Vorgabe
/// wird nicht gespeichert, damit spätere Fassungen ihre eigene nehmen.
pub fn save_list(conn: &Connection, emulators: &[Emulator]) -> Result<()> {
    let list: Vec<&Emulator> = emulators
        .iter()
        .filter(|e| !e.path.trim().is_empty())
        .take(MAX_EMULATORS)
        .collect();
    for i in 0..MAX_EMULATORS {
        let (p, a) = keys(i);
        let (path, args) = match list.get(i) {
            Some(e) => {
                let args = e.args.trim();
                let args = if args == DEFAULT_ARGS || args == default_args(&e.path) {
                    ""
                } else {
                    args
                };
                (e.path.trim(), args)
            }
            None => ("", ""),
        };
        if i == 0 || !path.is_empty() {
            db::set_setting(conn, &p, path)?;
            db::set_setting(conn, &a, args)?;
        } else {
            conn.execute("DELETE FROM settings WHERE key IN (?, ?)", [&p, &a])?;
        }
    }
    Ok(())
}

/// Anzeigename eines Emulators: Programmname ohne Endung, z. B. „x64sc“, „denise“.
pub fn display_name(path: &str) -> String {
    let name = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    name.strip_suffix(".app")
        .or_else(|| name.strip_suffix(".exe"))
        .unwrap_or(name)
        .to_string()
}

/// Emulatorfamilie für die Optionen je Spiel: „vice“, „denise“ oder der Programmname.
pub fn family(path: &str) -> String {
    if is_vice(path) {
        "vice".into()
    } else {
        program_stem(path)
    }
}

/// Dateiname ohne Endung (.exe, .app), klein geschrieben; Windows-Pfade auch unter Unix.
fn program_stem(path: &str) -> String {
    let name = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    name.trim_end_matches(".exe")
        .trim_end_matches(".app")
        .to_string()
}

/// Sieht nach, ob der Pfad nach Denise aussieht (deniseemu.sourceforge.io).
pub fn is_denise(path: &str) -> bool {
    program_stem(path) == "denise"
}

/// Sieht nach, ob der Pfad nach VICE aussieht (dann gibt es die Wechselliste für mehrere Disketten).
pub fn is_vice(path: &str) -> bool {
    let stem = program_stem(path);
    // Snap: „vice-jz.x64sc“; Flatpak: „net.sf.VICE“
    let stem = stem.rsplit('.').next().unwrap_or_default().to_string();
    matches!(stem.as_str(), "x64sc" | "x64" | "x128" | "xscpu64" | "vice")
}

/// Übliche Orte, an denen VICE liegt – nur was es auf diesem Rechner wirklich gibt.
///
/// Durchsucht den Suchpfad, die üblichen Programmordner und – weil es VICE für Windows nur als
/// ZIP gibt – auch Benutzerordner (Downloads, Desktop …) und die Wurzel aller Festplatten und
/// Wechseldatenträger nach Ordnern mit „vice“ im Namen (eine Ebene, dazu Ordner wie
/// „Emulatoren“ oder „C64“ eine Ebene tiefer).
pub fn find_emulators() -> Vec<String> {
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    search(home.as_deref())
}

fn search(home: Option<&Path>) -> Vec<String> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut add = |p: PathBuf| {
        if (p.is_file() || (cfg!(target_os = "macos") && p.is_dir())) && !found.contains(&p) {
            found.push(p);
        }
    };
    let exe = if cfg!(windows) { "x64sc.exe" } else { "x64sc" };

    // Suchpfad (Linux-Pakete, Homebrew, Windows mit PATH-Eintrag)
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            add(dir.join(exe));
        }
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(target_os = "macos") {
        roots.push("/Applications".into());
        add("/opt/homebrew/bin/x64sc".into());
        add("/usr/local/bin/x64sc".into());
    } else if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(v) = std::env::var_os(var) {
                roots.push(v.into());
            }
        }
        if let Some(v) = std::env::var_os("LOCALAPPDATA") {
            roots.push(Path::new(&v).join("Programs"));
        }
        roots.extend(drive_roots());
    } else {
        add("/usr/bin/x64sc".into());
        add("/usr/local/bin/x64sc".into());
        add("/var/lib/flatpak/exports/bin/net.sf.VICE".into());
        roots.push("/opt".into());
        // Snap: Befehle heißen dort „<paket>.x64sc“
        if let Ok(entries) = std::fs::read_dir("/snap/bin") {
            for e in entries.flatten() {
                if e.file_name().to_string_lossy().ends_with("x64sc") {
                    add(e.path());
                }
            }
        }
    }
    if let Some(home) = home {
        if cfg!(target_os = "linux") {
            add(home.join(".local/share/flatpak/exports/bin/net.sf.VICE"));
            add(home.join(".local/bin/x64sc"));
        }
        roots.push(home.to_path_buf());
        for sub in [
            "Applications",
            "Downloads",
            "Desktop",
            "Documents",
            "Programme",
            "Programs",
        ] {
            roots.push(home.join(sub));
        }
        if cfg!(windows) {
            // Desktop und Dokumente liegen oft in OneDrive
            roots.push(home.join("OneDrive"));
            roots.push(home.join("OneDrive").join("Desktop"));
            roots.push(home.join("OneDrive").join("Documents"));
        }
    }

    // Ordner wie „vice-arm64-sdl2-3.9“, „GTK3VICE-3.9-win64“, „WinVICE-3.1-x64“; dazu Sammelordner
    // wie „C:\Emulatoren“ eine Ebene tiefer und doppelt verschachtelte ZIP-Ordner.
    let mut vice_dirs: Vec<PathBuf> = Vec::new();
    for root in &roots {
        for d in subdirs(root) {
            let name = lower_name(&d);
            if name.contains("vice") {
                vice_dirs.push(d);
            } else if ["emu", "c64", "commodore", "retro"]
                .iter()
                .any(|k| name.contains(k))
            {
                vice_dirs.extend(
                    subdirs(&d)
                        .into_iter()
                        .filter(|s| lower_name(s).contains("vice")),
                );
            }
        }
    }
    let nested: Vec<PathBuf> = vice_dirs
        .iter()
        .flat_map(|d| {
            subdirs(d)
                .into_iter()
                .filter(|s| lower_name(s).contains("vice"))
        })
        .collect();
    vice_dirs.extend(nested);
    // Denise: „Denise.app“ direkt im Programmordner, sonst Ordner mit „denise“ im Namen
    let denise_exe = if cfg!(windows) {
        "Denise.exe"
    } else {
        "denise"
    };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            add(dir.join(denise_exe));
        }
    }
    let mut denise_dirs: Vec<PathBuf> = Vec::new();
    for root in &roots {
        if cfg!(target_os = "macos") {
            add(root.join("Denise.app"));
            add(root.join("denise.app"));
        }
        for d in subdirs(root) {
            let name = lower_name(&d);
            if name.contains("denise") {
                denise_dirs.push(d);
            } else if ["emu", "c64", "commodore", "retro"]
                .iter()
                .any(|k| name.contains(k))
            {
                denise_dirs.extend(
                    subdirs(&d)
                        .into_iter()
                        .filter(|s| lower_name(s).contains("denise")),
                );
            }
        }
    }
    vice_dirs.sort();
    vice_dirs.dedup();
    vice_dirs.reverse(); // neueste Fassung zuerst
    for d in vice_dirs {
        if cfg!(target_os = "macos") {
            add(d.join("x64sc.app"));
        }
        add(d.join("bin").join(exe));
        add(d.join(exe));
    }
    denise_dirs.sort();
    denise_dirs.dedup();
    for d in denise_dirs {
        if cfg!(target_os = "macos") {
            add(d.join("Denise.app"));
            add(d.join("denise.app"));
        }
        add(d.join(denise_exe));
    }
    found
        .into_iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// Unterordner (ohne versteckte); Fehler zählen als leer.
fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .collect()
}

fn lower_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Wurzeln aller Festplatten und Wechseldatenträger (C:\, D:\ …), ohne Netz- und CD-Laufwerke,
/// damit getrennte Netzlaufwerke die Suche nicht aufhalten.
#[cfg(windows)]
fn drive_roots() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    use windows_sys::Win32::System::Diagnostics::Debug::SetThreadErrorMode;
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    // Keine Meldung „Kein Datenträger im Laufwerk“ bei leeren Kartenlesern
    const SEM_FAILCRITICALERRORS: u32 = 0x0001;
    const SEM_NOOPENFILEERRORBOX: u32 = 0x8000;
    let mut old = 0u32;
    // SAFETY: einfache Win32-Aufrufe ohne Zeiger außer dem eigenen Puffer bzw. `old`.
    unsafe {
        SetThreadErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX, &mut old);
    }
    let mask = unsafe { GetLogicalDrives() };
    let mut out = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i as u8) as char);
        let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
        let kind = unsafe { GetDriveTypeW(wide.as_ptr()) };
        if kind == DRIVE_FIXED || kind == DRIVE_REMOVABLE {
            out.push(PathBuf::from(root));
        }
    }
    unsafe {
        SetThreadErrorMode(old, std::ptr::null_mut());
    }
    out
}

#[cfg(not(windows))]
fn drive_roots() -> Vec<PathBuf> {
    Vec::new()
}

/// Zerlegt die Aufrufzeile in Teile; `"…"` hält Leerzeichen zusammen.
fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

/// Programm und Argumente für den Aufruf. Unter macOS wird eine `.app` über `open` gestartet.
pub fn command_line(
    emulator: &str,
    args: &str,
    file: &Path,
    fliplist: Option<&Path>,
) -> (String, Vec<String>) {
    command_line_with(emulator, args, "", file, fliplist)
}

/// Wie `command_line`, mit zusätzlichen Optionen dieses Spiels (`extra`, z. B. „-ntsc -reu“). Sie
/// stehen vor den allgemeinen Optionen und damit vor der Datei, wie VICE es verlangt.
pub fn command_line_with(
    emulator: &str,
    args: &str,
    extra: &str,
    file: &Path,
    fliplist: Option<&Path>,
) -> (String, Vec<String>) {
    command_line_full(emulator, args, extra, &[], file, fliplist)
}

/// Wie `command_line_with`, dazu fertige Argumente `pre` (z. B. das REU-Abbild), die unverändert
/// vor den Optionen des Spiels stehen – Pfade darin dürfen Leerzeichen enthalten.
pub fn command_line_full(
    emulator: &str,
    args: &str,
    extra: &str,
    pre: &[String],
    file: &Path,
    fliplist: Option<&Path>,
) -> (String, Vec<String>) {
    let file_s = file.to_string_lossy();
    // Leer oder die VICE-Vorgabe: Vorgabe des eingestellten Programms (Denise kennt „-autostart“ nicht).
    let template = if args.trim().is_empty() || args.trim() == DEFAULT_ARGS {
        default_args(emulator)
    } else {
        args
    };
    let mut parts = split_args(template);
    if !parts.iter().any(|p| p.contains("{file}")) {
        parts.push("{file}".into());
    }
    let mut list: Vec<String> = Vec::new();
    if let Some(f) = fliplist.filter(|_| is_vice(emulator)) {
        list.push("-flipname".into());
        list.push(f.to_string_lossy().into_owned());
    }
    list.extend(pre.iter().cloned());
    list.extend(
        split_args(extra)
            .into_iter()
            .filter(|p| !p.contains("{file}")),
    );
    list.extend(parts.into_iter().map(|p| p.replace("{file}", &file_s)));

    let emulator = emulator.trim();
    if cfg!(target_os = "macos") && emulator.trim_end_matches('/').ends_with(".app") {
        let mut v = vec![
            "-n".to_string(),
            "-a".to_string(),
            emulator.to_string(),
            "--args".to_string(),
        ];
        v.extend(list);
        ("open".into(), v)
    } else {
        (emulator.to_string(), list)
    }
}

/// Legt einen Inhalt im Emulator-Ordner ab (vorhandene Kopie bleibt – sie kann Spielstände enthalten).
fn place(
    conn: &Connection,
    sha1: &str,
    dir: &Path,
    password_for: &dyn Fn(&str) -> String,
) -> Result<PathBuf> {
    let d = db::details(conn, sha1)?;
    let format = d.row.format.as_str();
    let Some(ext) = extension(format) else {
        bail!(tr!(
            "Format {format} kann der Emulator nicht starten.",
            "The emulator cannot start format {format}."
        ));
    };
    let path = dir.join(device_file_name(&d.row.title, sha1, ext));
    if path.exists() {
        return Ok(path);
    }
    // Erst lokale Fundorte, dann Netz und Gerät
    let mut locs: Vec<&db::Location> = d.locations.iter().collect();
    locs.sort_by_key(|l| l.source_kind != "local");
    let mut last_err = None;
    for loc in locs {
        match read_location(loc, password_for) {
            Ok(data) => {
                std::fs::create_dir_all(dir)?;
                let tmp = path.with_extension("tmp");
                std::fs::write(&tmp, &data)?;
                std::fs::rename(&tmp, &path)?;
                return Ok(path);
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| {
        anyhow::anyhow!(tr!(
            "Für diesen Eintrag ist kein Speicherort bekannt",
            "No storage location is known for this entry"
        ))
    }))
}

/// Legt `sha1` in Laufwerk 8 des zuletzt gestarteten VICE ein (Kopie im Emulator-Ordner wie beim Start).
pub fn insert(
    conn: &Connection,
    sha1: &str,
    dir: &Path,
    password_for: &dyn Fn(&str) -> String,
) -> Result<String> {
    let format = db::details(conn, sha1)?.row.format;
    if !matches!(format.as_str(), "D64" | "D71" | "D81" | "G64") {
        bail!(tr!(
            "Einlegen geht nur mit Disketten.",
            "Only disks can be inserted."
        ));
    }
    let file = place(conn, sha1, dir, password_for)?;
    let short = format!("{}.{}", &sha1[..sha1.len().min(12)], format.to_lowercase());
    crate::vicemon::attach(&file, &short)?;
    Ok(tr!(
        "In VICE eingelegt (Laufwerk 8).",
        "Inserted in VICE (drive 8)."
    ))
}

/// Startet `sha1` im Emulator. `disks` sind (in Reihenfolge) alle Disketten desselben Spiels,
/// einschließlich `sha1`; sie kommen in die Wechselliste.
pub fn start(
    conn: &Connection,
    sha1: &str,
    disks: &[String],
    emulator: &str,
    args: &str,
    dir: &Path,
    password_for: &dyn Fn(&str) -> String,
) -> Result<EmuResult> {
    let emulator = emulator.trim();
    if emulator.is_empty() {
        bail!(tr!(
            "Kein Emulator eingestellt (Einstellungen).",
            "No emulator set (Settings)."
        ));
    }
    if !Path::new(emulator).exists() {
        bail!(tr!(
            "Emulator nicht gefunden: {emulator}",
            "Emulator not found: {emulator}"
        ));
    }
    let file = place(conn, sha1, dir, password_for)?;
    let mut fliplist = None;
    if disks.len() > 1 && disks.iter().any(|s| s == sha1) {
        let mut text = String::from("# Vice fliplist file\n\nUNIT 8\n");
        // Ab der gestarteten Diskette, die davor kommen ans Ende (VICE blättert im Kreis).
        let pos = disks.iter().position(|s| s == sha1).unwrap_or(0);
        for s in disks[pos..].iter().chain(disks[..pos].iter()) {
            let p = place(conn, s, dir, password_for)?;
            text.push_str(&p.to_string_lossy());
            text.push('\n');
        }
        let list = file.with_extension("vfl");
        std::fs::write(&list, text)?;
        fliplist = Some(list);
    }
    let extra =
        crate::db::emu_options_of(conn, &[sha1.to_string()], &family(emulator)).unwrap_or_default();
    // REU-Abbild neben der Startdatei? Dann als Kopie neben die Startdatei im Emulator-Ordner.
    let mut pre = Vec::new();
    let mut reu_note = String::new();
    let details = db::details(conn, sha1)?;
    if let Some(comp) = crate::reu::find(&details, true, password_for) {
        let image = file.with_extension("reu");
        let have = std::fs::metadata(&image).map(|m| m.len()).unwrap_or(0);
        if crate::reu::emulator_args(emulator, &image, comp.size()).is_some() {
            if have != comp.size() {
                let data = comp.read(password_for)?;
                let tmp = image.with_extension("reu.tmp");
                std::fs::write(&tmp, &data)?;
                std::fs::rename(&tmp, &image)?;
            }
            pre = crate::reu::emulator_args(emulator, &image, comp.size()).unwrap_or_default();
            let kb = crate::reu::size_kb(comp.size());
            reu_note = tr!(" Mit REU-Abbild ({kb} KB).", " With REU image ({kb} KB).");
        } else {
            reu_note = tr!(
                " Das REU-Abbild neben der Datei lädt Breadbin nur in VICE und Denise mit.",
                " Breadbin loads the REU image next to the file only in VICE and Denise."
            );
        }
    }
    // VICE: Monitor-Port für Einlegen und Bildschirmfoto (nicht, wenn der Nutzer selbst einen setzt).
    let mut monitor = None;
    if is_vice(emulator) && !format!("{args} {extra}").contains("remotemonitor") {
        let port = crate::vicemon::free_port()?;
        let mut with = crate::vicemon::monitor_args(port);
        with.extend(pre);
        pre = with;
        monitor = Some(port);
    }
    let (program, argv) =
        command_line_full(emulator, args, &extra, &pre, &file, fliplist.as_deref());
    let mut child = Command::new(&program)
        .args(&argv)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            tr!(
                "Emulator lässt sich nicht starten: {program}",
                "Cannot start the emulator: {program}"
            )
        })?;
    // Nicht warten, aber den Prozess später einsammeln (sonst bleibt unter Unix ein Zombie).
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    if let Some(port) = monitor {
        crate::vicemon::register(port);
    }
    let name = Path::new(emulator)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let message = if fliplist.is_some() && is_vice(emulator) {
        tr!(
            "Im Emulator gestartet ({name}); weitere Disketten mit „Einlegen“ in den Details oder über die Wechselliste (Fliplist) von VICE, meist Alt+N.",
            "Started in the emulator ({name}); insert further disks with “Insert” in the details or via VICE's fliplist, usually Alt+N."
        )
    } else {
        tr!(
            "Im Emulator gestartet ({name}).",
            "Started in the emulator ({name})."
        )
    };
    let message = message + &reu_note;
    Ok(EmuResult {
        message,
        file: file.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vice_erkennen() {
        assert!(is_vice("/Applications/vice-arm64-sdl2-3.9/x64sc.app"));
        assert!(is_vice("/Applications/vice-arm64-sdl2-3.9/x64sc.app/"));
        assert!(is_vice("C:\\GTK3VICE-3.9-win64\\bin\\x64sc.exe"));
        assert!(is_vice("/usr/bin/x64"));
        assert!(is_vice("/var/lib/flatpak/exports/bin/net.sf.VICE"));
        assert!(is_vice("/snap/bin/vice-jz.x64sc"));
        assert!(!is_vice("/opt/denise/Denise"));
        assert!(is_denise("/Applications/denise.app"));
        assert!(is_denise("C:\\Emu\\Denise\\Denise.exe"));
        assert!(!is_denise("/usr/bin/x64sc"));
    }

    #[test]
    fn aufrufzeile_denise() {
        let f = Path::new("/tmp/a b.d64");
        let (_, a) = command_line("/opt/denise/denise", "", f, None);
        assert_eq!(a, ["/tmp/a b.d64"]);
        let (_, a) = command_line("/opt/denise/denise", DEFAULT_ARGS, f, Some(Path::new("x")));
        assert_eq!(
            a,
            ["/tmp/a b.d64"],
            "VICE-Vorgabe und Wechselliste nicht an Denise"
        );
        let (_, a) = command_line_with("/opt/denise/denise", "", "-sid-8580", f, None);
        assert_eq!(a, ["-sid-8580", "/tmp/a b.d64"]);
    }

    #[test]
    fn aufrufzeile() {
        let f = Path::new("/tmp/a b.d64");
        let (p, a) = command_line("/usr/bin/x64sc", "", f, None);
        assert_eq!(p, "/usr/bin/x64sc");
        assert_eq!(a, ["-autostart", "/tmp/a b.d64"]);

        let (_, a) = command_line_with("/usr/bin/x64sc", "", "-ntsc -reu", f, None);
        assert_eq!(
            a,
            ["-ntsc", "-reu", "-autostart", "/tmp/a b.d64"],
            "Optionen des Spiels vor den allgemeinen"
        );
        let (_, a) = command_line("/usr/bin/x64sc", "-warp", f, None);
        assert_eq!(
            a,
            ["-warp", "/tmp/a b.d64"],
            "ohne {{file}} hinten angehängt"
        );

        let (_, a) = command_line(
            "/usr/bin/x64sc",
            "-default -autostart {file}",
            f,
            Some(Path::new("/tmp/l.vfl")),
        );
        assert_eq!(
            a,
            [
                "-flipname",
                "/tmp/l.vfl",
                "-default",
                "-autostart",
                "/tmp/a b.d64"
            ]
        );

        let (_, a) = command_line(
            "/opt/other",
            "--image=\"{file}\" --fast",
            f,
            Some(Path::new("x")),
        );
        assert_eq!(
            a,
            ["--image=/tmp/a b.d64", "--fast"],
            "Wechselliste nur für VICE"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn app_ueber_open() {
        let (p, a) = command_line("/Applications/V/x64sc.app", "", Path::new("/t.prg"), None);
        assert_eq!(p, "open");
        assert_eq!(
            a,
            [
                "-n",
                "-a",
                "/Applications/V/x64sc.app",
                "--args",
                "-autostart",
                "/t.prg"
            ]
        );
    }

    #[test]
    fn suche_in_benutzerordnern() {
        let h = tempfile::tempdir().unwrap();
        let exe = if cfg!(windows) { "x64sc.exe" } else { "x64sc" };
        let mk = |rel: &str| {
            let p = h.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"").unwrap();
            p
        };
        // ZIP in „Downloads“ doppelt ausgepackt, Sammelordner im Benutzerordner, Fremdes
        let a = mk(&format!(
            "Downloads/GTK3VICE-3.9-win64/GTK3VICE-3.9-win64/bin/{exe}"
        ));
        let b = mk(&format!("Emulatoren/WinVICE-3.1-x64/{exe}"));
        let c = mk(&format!("Desktop/vice-3.8/{exe}"));
        let dexe = if cfg!(windows) {
            "Denise.exe"
        } else {
            "denise"
        };
        let d = mk(&format!("Emulatoren/Denise_2.9/{dexe}"));
        mk(&format!("Desktop/andere/{exe}"));
        mk(&format!("Projekte/vice/{exe}"));
        let got = search(Some(h.path()));
        for p in [&a, &b, &c, &d] {
            // Als Pfad vergleichen: unter Windows mischt der Test „/“ und „\“.
            assert!(
                got.iter().any(|g| Path::new(g) == p.as_path()),
                "{p:?} fehlt in {got:?}"
            );
        }
        assert!(
            !got.iter()
                .any(|g| g.contains("andere") || g.contains("Projekte")),
            "{got:?}"
        );
    }

    #[test]
    fn liste_speichern() {
        let conn = db::open_in_memory().unwrap();
        let e = |p: &str, a: &str| Emulator {
            path: p.into(),
            args: a.into(),
        };
        save_list(
            &conn,
            &[
                e("/usr/bin/x64sc", DEFAULT_ARGS),
                e("", "x"),
                e("/opt/denise/denise", "{file}"),
                e("/opt/x/emu", "--go {file}"),
            ],
        )
        .unwrap();
        let l = list(&conn);
        assert_eq!(
            l,
            [
                e("/usr/bin/x64sc", ""),
                e("/opt/denise/denise", ""),
                e("/opt/x/emu", "--go {file}")
            ]
        );
        assert_eq!(
            db::get_setting(&conn, "emulator_path_2")
                .unwrap()
                .as_deref(),
            Some("/opt/denise/denise")
        );
        save_list(&conn, &[e("/opt/denise/denise", "")]).unwrap();
        assert_eq!(list(&conn).len(), 1);
        assert_eq!(db::get_setting(&conn, "emulator_path_3").unwrap(), None);
        save_list(&conn, &[]).unwrap();
        assert!(list(&conn).is_empty());
        assert_eq!(display_name("/Applications/denise.app/"), "denise");
        assert_eq!(display_name("C:\\V\\x64sc.exe"), "x64sc");
        assert_eq!(family("/snap/bin/vice-jz.x64sc"), "vice");
        assert_eq!(family("D:\\Denise\\Denise.exe"), "denise");
    }

    #[test]
    fn argumente_zerlegen() {
        assert_eq!(split_args("  a \"b c\"  d"), ["a", "b c", "d"]);
        assert_eq!(split_args("\"\""), [""]);
        assert!(split_args("   ").is_empty());
    }
}
