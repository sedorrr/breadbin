//! Erkennen und Auslesen der C64-Dateiformate.
//!
//! Alle Funktionen arbeiten auf dem Dateiinhalt im Speicher und geben bei kaputten
//! Daten nie einen Fehler zurück, sondern so viel Information wie lesbar ist.

use crate::petscii::{decode_name, decode_padded_ascii};
use serde::Serialize;
use std::collections::HashSet;

/// Ein Eintrag im Verzeichnis eines Disk- oder Tape-Abbilds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DirEntry {
    pub name: String,
    pub kind: String,
    pub blocks: u32,
}

/// Was aus einer Datei herauszulesen war.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FileInfo {
    /// Diskname, Modulname oder Bandname aus der Datei selbst.
    pub image_name: Option<String>,
    /// Kurze technische Angabe, z. B. Modultyp oder Ladeadresse.
    pub detail: Option<String>,
    pub entries: Vec<DirEntry>,
}

/// Formatkennung (Großbuchstaben) zu einer Dateiendung, `None` für fremde Dateien.
pub fn format_for_ext(ext: &str) -> Option<&'static str> {
    Some(match ext.to_ascii_lowercase().as_str() {
        "prg" => "PRG",
        "p00" => "P00",
        "crt" => "CRT",
        "d64" => "D64",
        "d71" => "D71",
        "d81" => "D81",
        "g64" => "G64",
        "t64" => "T64",
        "tap" => "TAP",
        _ => return None,
    })
}

pub fn analyze(format: &str, data: &[u8]) -> FileInfo {
    match format {
        "PRG" => prg(data),
        "P00" => p00(data),
        "CRT" => crt(data),
        "D64" => cbm_disk(data, Geometry::D64),
        "D71" => cbm_disk(data, Geometry::D71),
        "D81" => cbm_disk(data, Geometry::D81),
        "G64" => g64(data),
        "T64" => t64(data),
        "TAP" => tap(data),
        _ => FileInfo::default(),
    }
}

fn damaged(what: &str) -> FileInfo {
    FileInfo {
        detail: Some(format!("{what}: Datei beschädigt oder unbekannter Aufbau")),
        ..Default::default()
    }
}

fn u16le(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}

fn u32le(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
}

// ---------------------------------------------------------------- PRG / P00

fn prg(d: &[u8]) -> FileInfo {
    if d.len() < 2 {
        return damaged("PRG");
    }
    FileInfo {
        detail: Some(format!("Ladeadresse ${:04X}", u16le(d, 0))),
        ..Default::default()
    }
}

/// PC64-Format: Kopf `C64File\0`, 16 Byte Name, danach die PRG-Daten ab Offset 26.
fn p00(d: &[u8]) -> FileInfo {
    if d.len() < 28 || &d[..8] != b"C64File\0" {
        return damaged("P00");
    }
    FileInfo {
        image_name: Some(decode_padded_ascii(&d[8..24])),
        detail: Some(format!("Ladeadresse ${:04X}", u16le(d, 26))),
        ..Default::default()
    }
}

// ---------------------------------------------------------------- CRT

/// Modultypen nach der CRT-Spezifikation von VICE (Hardware-Typ im Kopf).
pub fn crt_type_name(t: u16) -> Option<&'static str> {
    const NAMES: &[&str] = &[
        "Normales Modul",
        "Action Replay",
        "KCS Power Cartridge",
        "Final Cartridge III",
        "Simons' BASIC",
        "Ocean",
        "Expert Cartridge",
        "Fun Play / Power Play",
        "Super Games",
        "Atomic Power",
        "Epyx Fastload",
        "Westermann Learning",
        "Rex Utility",
        "Final Cartridge I",
        "Magic Formel",
        "C64 Game System / System 3",
        "Warp Speed",
        "Dinamic",
        "Zaxxon / Super Zaxxon",
        "Magic Desk / Domark / HES Australia",
        "Super Snapshot V5",
        "Comal-80",
        "Structured BASIC",
        "Ross",
        "Dela EP64",
        "Dela EP7x8",
        "Dela EP256",
        "Rex EP256",
        "Mikro Assembler",
        "Final Cartridge Plus",
        "Action Replay 4",
        "Stardos",
        "EasyFlash",
        "EasyFlash Xbank",
        "Capture",
        "Action Replay 3",
        "Retro Replay",
        "MMC64",
        "MMC Replay",
        "IDE64",
        "Super Snapshot V4",
        "IEEE-488",
        "Game Killer",
        "Prophet64",
        "EXOS",
        "Freeze Frame",
        "Freeze Machine",
        "Snapshot64",
        "Super Explode V5.0",
        "Magic Voice",
        "Action Replay 2",
        "MACH 5",
        "Diashow-Maker",
        "Pagefox",
        "Kingsoft",
        "Silverrock 128K",
        "Formel 64",
        "RGCD",
        "RR-Net MK3",
        "EasyCalc",
        "GMod2",
    ];
    NAMES.get(t as usize).copied()
}

fn crt(d: &[u8]) -> FileInfo {
    if d.len() < 0x40 || &d[..16] != b"C64 CARTRIDGE   " {
        return damaged("CRT");
    }
    let hw = u16::from_be_bytes([d[0x16], d[0x17]]);
    let typ = crt_type_name(hw).map_or_else(|| format!("Modultyp {hw}"), str::to_string);
    let kb = d.len().div_ceil(1024);
    FileInfo {
        image_name: Some(decode_padded_ascii(&d[0x20..0x40])).filter(|s| !s.is_empty()),
        detail: Some(format!("{typ}, {kb} KB")),
        ..Default::default()
    }
}

// ---------------------------------------------------------------- D64 / D71 / D81

#[derive(Clone, Copy)]
enum Geometry {
    D64,
    D71,
    D81,
}

/// Sektoren je Spur einer 1541 (Spuren 36–40 bei erweiterten Abbildern wie 31–35).
fn sectors_1541(track: u8) -> u8 {
    match track {
        1..=17 => 21,
        18..=24 => 19,
        25..=30 => 18,
        _ => 17,
    }
}

fn offset_1541(track: u8, sector: u8) -> Option<usize> {
    if track == 0 || track > 40 || sector >= sectors_1541(track) {
        return None;
    }
    let before: usize = (1..track).map(|t| sectors_1541(t) as usize).sum();
    Some((before + sector as usize) * 256)
}

impl Geometry {
    fn offset(self, track: u8, sector: u8) -> Option<usize> {
        match self {
            Geometry::D64 => offset_1541(track, sector),
            // Zweite Seite der 1571 (Spuren 36–70) liegt hinter den 683 Sektoren der ersten.
            Geometry::D71 if track > 35 => offset_1541(track - 35, sector)
                .filter(|_| track <= 70)
                .map(|o| o + 683 * 256),
            Geometry::D71 => offset_1541(track, sector).filter(|_| track <= 35),
            Geometry::D81 => ((1..=80).contains(&track) && sector < 40)
                .then(|| ((track as usize - 1) * 40 + sector as usize) * 256),
        }
    }

    /// Spur/Sektor des Kopfsektors, Offset des Disknamens darin, erster Verzeichnissektor.
    fn header(self) -> ((u8, u8), usize, (u8, u8)) {
        match self {
            Geometry::D64 | Geometry::D71 => ((18, 0), 0x90, (18, 1)),
            Geometry::D81 => ((40, 0), 0x04, (40, 3)),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Geometry::D64 => "D64",
            Geometry::D71 => "D71",
            Geometry::D81 => "D81",
        }
    }
}

const FILE_TYPES: [&str; 8] = ["DEL", "SEQ", "PRG", "USR", "REL", "CBM", "DIR", "???"];

fn cbm_disk(d: &[u8], geo: Geometry) -> FileInfo {
    let (hdr, name_off, first) = geo.header();
    let Some(hdr_off) = geo.offset(hdr.0, hdr.1).filter(|o| o + 256 <= d.len()) else {
        return damaged(geo.label());
    };
    let disk_name = decode_name(&d[hdr_off + name_off..hdr_off + name_off + 16]);
    let id = decode_name(&d[hdr_off + name_off + 18..hdr_off + name_off + 23]);

    let mut entries = Vec::new();
    let mut visited = HashSet::new();
    let (mut t, mut s) = first;
    // Ein Verzeichnis hat höchstens einige Dutzend Sektoren; die Grenze schützt vor Schleifen.
    while t != 0 && visited.insert((t, s)) && visited.len() <= 300 {
        let Some(off) = geo.offset(t, s).filter(|o| o + 256 <= d.len()) else {
            break;
        };
        let sec = &d[off..off + 256];
        for e in (0..256).step_by(32) {
            let typ = sec[e + 2];
            if typ == 0 {
                continue; // leerer oder gelöschter Eintrag
            }
            let mut kind = FILE_TYPES[(typ & 7) as usize].to_string();
            if typ & 0x80 == 0 {
                kind.insert(0, '*'); // nicht ordnungsgemäß geschlossen
            }
            if typ & 0x40 != 0 {
                kind.push('<'); // schreibgeschützt
            }
            entries.push(DirEntry {
                name: decode_name(&sec[e + 5..e + 21]),
                kind,
                blocks: u16le(sec, e + 30) as u32,
            });
        }
        (t, s) = (sec[0], sec[1]);
    }

    FileInfo {
        image_name: Some(disk_name),
        detail: Some(format!("ID {id}")),
        entries,
    }
}

// ---------------------------------------------------------------- G64 / TAP

fn g64(d: &[u8]) -> FileInfo {
    if d.len() < 12 || &d[..8] != b"GCR-1541" {
        return damaged("G64");
    }
    FileInfo {
        detail: Some(format!("GCR-Abbild, {} Halbspuren", d[9])),
        ..Default::default()
    }
}

fn tap(d: &[u8]) -> FileInfo {
    if d.len() < 20 || &d[..12] != b"C64-TAPE-RAW" {
        return damaged("TAP");
    }
    FileInfo {
        detail: Some(format!("Band-Abbild Version {}", d[12])),
        ..Default::default()
    }
}

// ---------------------------------------------------------------- T64

fn t64(d: &[u8]) -> FileInfo {
    if d.len() < 0x40 || &d[..3] != b"C64" {
        return damaged("T64");
    }
    let max = u16le(d, 0x22) as usize;
    let used = u16le(d, 0x24) as usize;
    // Manche Werkzeuge tragen 0 als belegte Einträge ein; dann alle Plätze prüfen.
    let slots = if used == 0 { max } else { used.max(1) }.min(512);
    let mut entries = Vec::new();
    for i in 0..slots {
        let o = 0x40 + i * 32;
        if o + 32 > d.len() {
            break;
        }
        if d[o] == 0 {
            continue;
        }
        let start = u16le(d, o + 2) as u32;
        let end = u16le(d, o + 4) as u32;
        let _offset = u32le(d, o + 8);
        let bytes = end.saturating_sub(start) + 2;
        entries.push(DirEntry {
            name: decode_padded_ascii(&d[o + 16..o + 32]),
            kind: if d[o] == 1 { "PRG" } else { "FRZ" }.to_string(),
            blocks: bytes.div_ceil(254),
        });
    }
    FileInfo {
        image_name: Some(decode_padded_ascii(&d[0x28..0x40])).filter(|s| !s.is_empty()),
        detail: None,
        entries,
    }
}

#[cfg(test)]
pub(crate) mod testdata {
    //! Kleine, im Test erzeugte Abbilder.
    use super::*;

    fn pad16(name: &[u8]) -> [u8; 16] {
        let mut n = [0xA0u8; 16];
        n[..name.len()].copy_from_slice(name);
        n
    }

    /// Leeres D64 mit Disknamen und den angegebenen Dateien (Name, Typbyte, Blöcke).
    pub fn d64(disk: &[u8], files: &[(&[u8], u8, u16)]) -> Vec<u8> {
        let mut d = vec![0u8; 174_848];
        let bam = offset_1541(18, 0).unwrap();
        d[bam] = 18;
        d[bam + 1] = 1;
        d[bam + 0x90..bam + 0xA0].copy_from_slice(&pad16(disk));
        d[bam + 0xA0] = 0xA0;
        d[bam + 0xA1] = 0xA0;
        d[bam + 0xA2..bam + 0xA7].copy_from_slice(b"01\xA02A");
        // Verzeichnis über zwei Sektoren (18/1 → 18/4), damit die Kette geprüft wird.
        for (i, (name, typ, blocks)) in files.iter().enumerate() {
            let sector = if i < 8 { 1 } else { 4 };
            let off = offset_1541(18, sector).unwrap();
            let e = off + (i % 8) * 32;
            d[e + 2] = *typ;
            d[e + 3] = 17;
            d[e + 5..e + 21].copy_from_slice(&pad16(name));
            d[e + 30..e + 32].copy_from_slice(&blocks.to_le_bytes());
        }
        let s1 = offset_1541(18, 1).unwrap();
        if files.len() > 8 {
            d[s1] = 18;
            d[s1 + 1] = 4;
            d[offset_1541(18, 4).unwrap() + 1] = 0xFF;
        } else {
            d[s1 + 1] = 0xFF;
        }
        d
    }

    pub fn crt(name: &str, hw: u16) -> Vec<u8> {
        let mut d = vec![0u8; 0x40 + 16 + 8192];
        d[..16].copy_from_slice(b"C64 CARTRIDGE   ");
        d[0x10..0x14].copy_from_slice(&0x40u32.to_be_bytes());
        d[0x14] = 1;
        d[0x16..0x18].copy_from_slice(&hw.to_be_bytes());
        d[0x20..0x20 + name.len()].copy_from_slice(name.as_bytes());
        d
    }

    pub fn t64(tape: &str, files: &[(&str, u16, u16)]) -> Vec<u8> {
        let mut d = vec![0u8; 0x40 + files.len() * 32];
        d[..32].copy_from_slice(b"C64 tape image file\0\0\0\0\0\0\0\0\0\0\0\0\0");
        d[0x20..0x22].copy_from_slice(&0x0101u16.to_le_bytes());
        d[0x22..0x24].copy_from_slice(&(files.len() as u16).to_le_bytes());
        d[0x24..0x26].copy_from_slice(&(files.len() as u16).to_le_bytes());
        let mut name = [0x20u8; 24];
        name[..tape.len()].copy_from_slice(tape.as_bytes());
        d[0x28..0x40].copy_from_slice(&name);
        for (i, (n, start, end)) in files.iter().enumerate() {
            let o = 0x40 + i * 32;
            d[o] = 1;
            d[o + 1] = 0x82;
            d[o + 2..o + 4].copy_from_slice(&start.to_le_bytes());
            d[o + 4..o + 6].copy_from_slice(&end.to_le_bytes());
            let mut fname = [0x20u8; 16];
            fname[..n.len()].copy_from_slice(n.as_bytes());
            d[o + 16..o + 32].copy_from_slice(&fname);
        }
        d
    }
}

#[cfg(test)]
mod tests {
    use super::testdata;
    use super::*;

    #[test]
    fn d64_offsets() {
        assert_eq!(offset_1541(1, 0), Some(0));
        assert_eq!(offset_1541(18, 0), Some(0x16500));
        assert_eq!(offset_1541(35, 16), Some(174_848 - 256));
        assert_eq!(offset_1541(18, 19), None);
        assert_eq!(Geometry::D71.offset(36, 0), Some(683 * 256));
        assert_eq!(Geometry::D81.offset(40, 0), Some(39 * 40 * 256));
    }

    #[test]
    fn d64_verzeichnis_ueber_zwei_sektoren() {
        let names: Vec<Vec<u8>> = (0..10).map(|i| format!("GAME {i}").into_bytes()).collect();
        let files: Vec<(&[u8], u8, u16)> = names
            .iter()
            .map(|n| (n.as_slice(), 0x82u8, 10u16))
            .collect();
        let img = testdata::d64(b"COMPILATION", &files);
        let info = analyze("D64", &img);
        assert_eq!(info.image_name.as_deref(), Some("COMPILATION"));
        assert_eq!(info.detail.as_deref(), Some("ID 01 2A"));
        assert_eq!(info.entries.len(), 10);
        assert_eq!(info.entries[9].name, "GAME 9");
        assert_eq!(info.entries[0].kind, "PRG");
        assert_eq!(info.entries[0].blocks, 10);
    }

    #[test]
    fn d64_typbits_und_schleife() {
        let mut img = testdata::d64(
            b"X",
            &[(b"OPEN", 0x02, 1), (b"LOCKED", 0xC2, 1), (b"ART", 0x80, 0)],
        );
        let info = analyze("D64", &img);
        let kinds: Vec<_> = info.entries.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["*PRG", "PRG<", "DEL"]);
        // Verzeichniskette zeigt auf sich selbst → darf nicht hängen.
        let s1 = offset_1541(18, 1).unwrap();
        img[s1] = 18;
        img[s1 + 1] = 1;
        assert_eq!(analyze("D64", &img).entries.len(), 3);
    }

    #[test]
    fn kaputte_dateien() {
        for f in [
            "PRG", "P00", "CRT", "D64", "D71", "D81", "G64", "T64", "TAP",
        ] {
            let info = analyze(f, &[1]);
            assert!(info.detail.unwrap().contains("beschädigt"), "{f}");
        }
    }

    #[test]
    fn crt_kopf() {
        let info = analyze("CRT", &testdata::crt("GIANA SISTERS", 32));
        assert_eq!(info.image_name.as_deref(), Some("GIANA SISTERS"));
        assert_eq!(info.detail.as_deref(), Some("EasyFlash, 9 KB"));
        let info = analyze("CRT", &testdata::crt("", 999));
        assert_eq!(info.image_name, None);
        assert!(info.detail.unwrap().starts_with("Modultyp 999"));
    }

    #[test]
    fn t64_eintraege() {
        let img = testdata::t64("MY TAPE", &[("BOULDER DASH", 0x0801, 0x3000)]);
        let info = analyze("T64", &img);
        assert_eq!(info.image_name.as_deref(), Some("MY TAPE"));
        assert_eq!(info.entries.len(), 1);
        assert_eq!(info.entries[0].name, "BOULDER DASH");
        assert_eq!(info.entries[0].blocks, 41);
    }

    #[test]
    fn prg_und_endungen() {
        assert_eq!(
            analyze("PRG", &[0x01, 0x08, 0]).detail.as_deref(),
            Some("Ladeadresse $0801")
        );
        assert_eq!(format_for_ext("D64"), Some("D64"));
        assert_eq!(format_for_ext("txt"), None);
    }
}
