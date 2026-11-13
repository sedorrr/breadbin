//! Sprache der Meldungen, die in der Oberfläche erscheinen (Deutsch oder Englisch).
//!
//! Die Oberfläche stellt die Sprache beim Start ein; ohne Einstellung bleibt es Deutsch
//! (Kommandozeile, Tests). Meldungen werden mit [`tr!`](crate::tr) in beiden Sprachen
//! an Ort und Stelle geschrieben.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

/// Sprache einstellen: `"en"` für Englisch, alles andere für Deutsch.
pub fn set_language(lang: &str) {
    ENGLISH.store(lang.eq_ignore_ascii_case("en"), Ordering::Relaxed);
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// Meldung in der eingestellten Sprache, wie `format!`:
/// `tr!("Gerät {host} antwortet nicht", "Device {host} does not answer")`.
#[macro_export]
macro_rules! tr {
    ($de:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::is_english() { format!($en $(, $arg)*) } else { format!($de $(, $arg)*) }
    };
}

/// Erkennt die Meldung „Gerät nicht erreichbar“ in beiden Sprachen.
pub fn is_unreachable(msg: &str) -> bool {
    msg.contains("nicht erreichbar") || msg.contains("not reachable")
}

#[cfg(test)]
mod tests {
    // Nur Deutsch prüfen: die Sprache gilt für den ganzen Prozess, und andere Tests laufen
    // gleichzeitig und erwarten deutsche Meldungen.
    #[test]
    fn standard_ist_deutsch() {
        let host = "u64";
        assert_eq!(
            tr!("Gerät {host} fehlt", "Device {host} missing"),
            "Gerät u64 fehlt"
        );
        assert_eq!(tr!("{} von {}", "{} of {}", 1, 2), "1 von 2");
        assert!(
            super::is_unreachable("Device x is not reachable")
                && super::is_unreachable("ist nicht erreichbar")
        );
    }
}
