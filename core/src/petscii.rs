//! PETSCII-Namen aus Disk-Verzeichnissen in lesbaren Text umwandeln.
//!
//! Abbildung für den Großschrift-/Grafik-Zeichensatz, wie ihn ein C64 nach dem
//! Einschalten zeigt. Grafikzeichen werden durch `·` ersetzt.

/// Füllbyte am Ende von Namen in CBM-Disk-Abbildern.
pub const PAD: u8 = 0xA0;

pub fn petscii_char(b: u8) -> char {
    match b {
        0x20..=0x5B | 0x5D => b as char,
        0x5C => '£',
        0x5E => '↑',
        0x5F => '←',
        // Von PC-Werkzeugen geschriebene Kleinbuchstaben und geshiftete Buchstaben
        // erscheinen auf dem C64 im Großschrift-Satz als Großbuchstaben bzw. Grafik;
        // für die Liste sind Großbuchstaben die brauchbarere Näherung.
        0x61..=0x7A => (b - 0x20) as char,
        0xC1..=0xDA => (b - 0x80) as char,
        0xA0 => ' ',
        _ => '·',
    }
}

/// Wandelt einen Namen um und entfernt das Füllbyte `$A0` am Ende.
pub fn decode_name(bytes: &[u8]) -> String {
    let end = bytes.iter().rposition(|&b| b != PAD).map_or(0, |p| p + 1);
    bytes[..end].iter().map(|&b| petscii_char(b)).collect()
}

/// Für ASCII-artige Felder (T64-, CRT-Namen): Nullbytes und Leerzeichen am Ende entfernen.
pub fn decode_padded_ascii(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let s: String = bytes[..end].iter().map(|&b| petscii_char(b)).collect();
    s.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_mit_fuellbytes() {
        assert_eq!(decode_name(b"GIANA\xA0\xA0\xA0"), "GIANA");
        assert_eq!(decode_name(b"\xC7AME 1"), "GAME 1");
        assert_eq!(decode_name(b"\xA0\xA0"), "");
    }
}
