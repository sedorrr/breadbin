//! Hinweis auf neue Breadbin-Fassungen: `version.json` auf 8bitcavern.de (liegt im Daten-Repository
//! unter `app/version.json`, der Spiegel lädt sie mit hoch).

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Wo die App nach neuen Fassungen sieht.
/// Seit 0.7.0 über die Subdomain (gleicher Ordner wie früher `https://8bitcavern.de/breadbin/db/breadbindb/`).
pub const VERSION_URL: &str = "https://breadbindb.8bitcavern.de/version.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    /// Neueste Fassung, z. B. „0.5.8“.
    pub version: String,
    /// Seite zum Herunterladen.
    pub url: String,
    #[serde(default)]
    pub notes_de: String,
    #[serde(default)]
    pub notes_en: String,
}

/// Ist `other` neuer als `current`? Verglichen werden die Zahlen („0.5.10“ > „0.5.9“), Zusätze
/// wie „-beta“ zählen nicht.
pub fn is_newer(other: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim()
            .trim_start_matches(['v', 'V'])
            .split(['.', '-', '+'])
            .map_while(|p| p.parse().ok())
            .collect()
    };
    let (a, b) = (parts(other), parts(current));
    !a.is_empty() && a > b
}

/// Neueste Fassung laden.
pub fn fetch(url: &str, user_agent: &str) -> Result<VersionInfo> {
    crate::share::fetch_json(url, user_agent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versionen_vergleichen() {
        assert!(is_newer("0.5.8", "0.5.7"));
        assert!(is_newer("0.5.10", "0.5.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("v0.6", "0.5.7"));
        assert!(!is_newer("0.5.7", "0.5.7"));
        assert!(!is_newer("0.5.6", "0.5.7"));
        assert!(!is_newer("", "0.5.7"));
        assert!(!is_newer("kaputt", "0.5.7"));
    }

    /// Netz nötig: `cargo test -p breadbin-core -- --ignored live`
    #[test]
    #[ignore]
    fn live_version_und_stand() {
        let v = fetch(VERSION_URL, "Breadbin-Test").unwrap();
        assert!(!v.version.is_empty() && v.url.starts_with("https://"));
        let su = crate::share::stand_url(crate::share::DEFAULT_COMMUNITY_URL).unwrap();
        let st: crate::share::Stand = crate::share::fetch_json(&su, "Breadbin-Test").unwrap();
        assert!(st.games > 0 && st.created > 0);
    }

    #[test]
    fn stand_neben_der_austauschdatei() {
        assert_eq!(
            crate::share::stand_url("https://breadbindb.8bitcavern.de/breadbin-db.zip").as_deref(),
            Some("https://breadbindb.8bitcavern.de/stand.json")
        );
        assert_eq!(
            crate::share::stand_url("https://example.org/andere.zip"),
            None
        );
    }
}
