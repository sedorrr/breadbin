//! Dateien auf einem Gerät, gleich ob Ultimate (FTP), BMC64 (HTTP), BMX (HTTP) oder TeensyROM (TCP 2112).
//!
//! Pfade beginnen immer mit dem Laufwerk: `/USB0/Spiele` beim Ultimate, `/SD/Spiele` bei BMC64,
//! `/SD/…` oder `/USB/…` beim TeensyROM, `/USER/disks/…` bei BMX.
//! Geräte-Quellen tragen das Protokoll im Ort: `ftp://host/…`, `bmc64://host/…`, `teensyrom://host/…`,
//! `bmx://host/…`.

use crate::bmc64::Bmc64;
use crate::bmx::Bmx;
use crate::ftp::{self, Ftp, RemoteEntry};
use crate::teensyrom::TeensyRom;
use crate::tr;
use anyhow::{Context, Result};

/// Geräteart „Ultimate“ (Ultimate 64, C64 Ultimate, 1541 Ultimate II+).
pub const ULTIMATE: &str = "ultimate";
/// Geräteart „BMC64“ (Raspberry Pi).
pub const BMC64: &str = "bmc64";
/// Geräteart „TeensyROM“ (Modul, Fernsteuerung über TCP 2112) – ungetestet.
pub const TEENSYROM: &str = "teensyrom";
/// Geräteart „BMX“ (Bare Metal X auf Raspberry Pi 4/5, Remote API).
pub const BMX: &str = "bmx";

/// Bekannte Geräteart zu einem gespeicherten Wert (Unbekanntes gilt als Ultimate).
pub fn kind_of(value: &str) -> &'static str {
    match value {
        BMC64 => BMC64,
        TEENSYROM => TEENSYROM,
        BMX => BMX,
        _ => ULTIMATE,
    }
}

pub enum Remote {
    Ftp(Ftp),
    Bmc64(Bmc64),
    TeensyRom(TeensyRom),
    Bmx(Bmx),
}

/// Ort einer Geräte-Quelle: `ftp://host/pfad` bzw. `bmc64://host/pfad`.
pub fn location(kind: &str, host: &str, path: &str) -> String {
    let scheme = match kind {
        BMC64 => "bmc64",
        TEENSYROM => "teensyrom",
        BMX => "bmx",
        _ => "ftp",
    };
    format!(
        "{scheme}://{}/{}",
        host.trim(),
        path.trim().trim_matches('/')
    )
}

/// Geräteart aus dem Ort einer Quelle.
pub fn location_kind(location: &str) -> &'static str {
    if location.starts_with("bmc64://") {
        BMC64
    } else if location.starts_with("teensyrom://") {
        TEENSYROM
    } else if location.starts_with("bmx://") {
        BMX
    } else {
        ULTIMATE
    }
}

impl Remote {
    pub fn connect(kind: &str, host: &str, password: &str) -> Result<Remote> {
        Ok(match kind {
            BMC64 => Remote::Bmc64(Bmc64::new(host, password)),
            TEENSYROM => Remote::TeensyRom(TeensyRom::new(host)),
            BMX => Remote::Bmx(Bmx::new(host, password)),
            _ => Remote::Ftp(Ftp::connect(host, password)?),
        })
    }

    /// Verbindung zum Gerät einer Quelle; gibt dazu Host und Pfad zurück.
    pub fn for_location<'a>(
        location: &'a str,
        password_for: &dyn Fn(&str) -> String,
    ) -> Result<(Remote, &'a str, &'a str)> {
        let (host, root) = ftp::split_location(location)
            .context(tr!("Geräteort ungültig", "Invalid device location"))?;
        let r = Remote::connect(location_kind(location), host, &password_for(host))?;
        Ok((r, host, root))
    }

    pub fn list(&mut self, path: &str) -> Result<Vec<RemoteEntry>> {
        match self {
            Remote::Ftp(f) => f.list(path),
            Remote::Bmc64(b) => b.list(path),
            Remote::TeensyRom(t) => t.list(path),
            Remote::Bmx(b) => b.list(path),
        }
    }

    pub fn retrieve(&mut self, path: &str, limit: u64) -> Result<Vec<u8>> {
        match self {
            Remote::Ftp(f) => f.retrieve(path, limit),
            Remote::Bmc64(b) => b.download(path, limit),
            Remote::TeensyRom(t) => t.download(path, limit),
            Remote::Bmx(b) => b.download(path, limit),
        }
    }

    pub fn store(&mut self, path: &str, data: &[u8]) -> Result<()> {
        match self {
            Remote::Ftp(f) => f.store(path, data),
            Remote::Bmc64(b) => b.upload(path, data),
            Remote::TeensyRom(t) => t.upload(path, data, true).map(|_| ()),
            Remote::Bmx(b) => b.upload(path, data),
        }
    }

    pub fn delete(&mut self, path: &str) -> Result<()> {
        match self {
            Remote::Ftp(f) => f.delete(path),
            Remote::Bmc64(b) => b.delete(path),
            Remote::TeensyRom(t) => t.delete(path),
            Remote::Bmx(b) => b.delete(path),
        }
    }

    /// Legt einen Ordner an; ein schon vorhandener Ordner ist kein Fehler.
    pub fn mkdir(&mut self, path: &str) -> Result<()> {
        match self {
            Remote::Ftp(f) => f.mkdir(path),
            Remote::Bmc64(b) => b.mkdir(path),
            Remote::TeensyRom(t) => t.mkdir(path),
            Remote::Bmx(b) => b.mkdir(path),
        }
    }

    pub fn is_dir(&mut self, path: &str) -> Result<bool> {
        match self {
            Remote::Ftp(f) => f.is_dir(path),
            Remote::Bmc64(b) => b.is_dir(path),
            Remote::TeensyRom(t) => t.is_dir(path),
            Remote::Bmx(b) => b.is_dir(path),
        }
    }

    pub fn quit(self) {
        if let Remote::Ftp(f) = self {
            f.quit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orte() {
        let l = location(BMC64, "192.0.2.7", "/SD/Spiele/");
        assert_eq!(l, "bmc64://192.0.2.7/SD/Spiele");
        assert_eq!(location_kind(&l), BMC64);
        assert_eq!(ftp::split_location(&l), Some(("192.0.2.7", "/SD/Spiele")));
        let u = location(ULTIMATE, "192.0.2.8", "USB0/Games");
        assert_eq!(u, "ftp://192.0.2.8/USB0/Games");
        assert_eq!(location_kind(&u), ULTIMATE);
        let t = location(TEENSYROM, "192.0.2.9", "/SD/Spiele");
        assert_eq!(t, "teensyrom://192.0.2.9/SD/Spiele");
        assert_eq!(location_kind(&t), TEENSYROM);
        assert_eq!(ftp::split_location(&t), Some(("192.0.2.9", "/SD/Spiele")));
        assert_eq!(kind_of("teensyrom"), TEENSYROM);
        let b = location(BMX, "192.0.2.10", "/USER/disks/Spiele");
        assert_eq!(b, "bmx://192.0.2.10/USER/disks/Spiele");
        assert_eq!(location_kind(&b), BMX);
        assert_eq!(
            ftp::split_location(&b),
            Some(("192.0.2.10", "/USER/disks/Spiele"))
        );
        assert_eq!(kind_of("bmx"), BMX);
        assert_eq!(kind_of(""), ULTIMATE);
    }
}
