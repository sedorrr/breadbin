//! Titel, Jahr und Herausgeber aus Dateinamen ableiten.
//!
//! Viele Sammlungen benennen nach TOSEC: `Titel (Jahr)(Herausgeber)(Zusatz)[Flags].d64`.
//! Ohne diese Angaben bleibt der bereinigte Dateiname der Titel.

#[derive(Debug, Default, PartialEq)]
pub struct NameInfo {
    pub title: String,
    pub year: Option<String>,
    pub publisher: Option<String>,
}

fn is_year(s: &str) -> bool {
    let y = s.get(..4).unwrap_or("");
    y.len() == 4
        && (y.starts_with("19") || y.starts_with("20"))
        && y[2..]
            .chars()
            .all(|c| c.is_ascii_digit() || c == 'x' || c == 'X')
        && (s.len() == 4 || s[4..].starts_with('-'))
}

/// Inhalte aller runden Klammern in Reihenfolge.
fn paren_groups(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find('(') {
        let Some(len) = rest[start + 1..].find(')') else {
            break;
        };
        out.push(rest[start + 1..start + 1 + len].trim());
        rest = &rest[start + 1 + len + 1..];
    }
    out
}

pub fn parse_file_stem(stem: &str) -> NameInfo {
    let clean = stem.replace('_', " ");
    let cut = clean.find(['(', '[']).unwrap_or(clean.len());
    let mut title = clean[..cut]
        .trim()
        .trim_end_matches(['-', ','])
        .trim()
        .to_string();
    if title.is_empty() {
        title = clean.trim().to_string();
    }
    let groups = paren_groups(&clean[cut..]);
    let year_pos = groups.iter().position(|g| is_year(g));
    if let Some(i) = year_pos {
        let publisher = groups
            .get(i + 1)
            .filter(|p| !p.is_empty() && **p != "-")
            .map(|p| p.to_string());
        return NameInfo {
            title,
            year: Some(groups[i][..4].to_string()),
            publisher,
        };
    }
    // „Titel (Herausgeber - 2012)“
    for g in &groups {
        if let Some((publisher, year)) = g.rsplit_once(" - ")
            && is_year(year.trim())
        {
            return NameInfo {
                title,
                year: Some(year.trim()[..4].to_string()),
                publisher: Some(publisher.trim().to_string()).filter(|p| !p.is_empty()),
            };
        }
    }
    // „Titel 1987 Herausgeber“ (ursprünglich mit Unterstrichen); die Jahreszahl als
    // erstes Wort zählt nicht, sonst würde aus „1942“ ein Jahr.
    if groups.is_empty() {
        let words: Vec<&str> = title.split(' ').collect();
        if let Some(i) = (1..words.len()).find(|&i| words[i].len() == 4 && is_year(words[i])) {
            let publisher = words[i + 1..].join(" ");
            return NameInfo {
                title: words[..i].join(" "),
                year: Some(words[i].to_string()),
                publisher: Some(publisher).filter(|p| !p.is_empty()),
            };
        }
    }
    NameInfo {
        title,
        year: None,
        publisher: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tosec_name() {
        let n = parse_file_stem("Great Giana Sisters, The (1987)(Rainbow Arts)[cr Triad]");
        assert_eq!(n.title, "Great Giana Sisters, The");
        assert_eq!(n.year.as_deref(), Some("1987"));
        assert_eq!(n.publisher.as_deref(), Some("Rainbow Arts"));
    }

    #[test]
    fn tosec_unbekanntes_jahr_und_disk() {
        let n = parse_file_stem("Ultima IV (198x)(Origin)(Disk 1 of 2)");
        assert_eq!(n.title, "Ultima IV");
        assert_eq!(n.year.as_deref(), Some("198x"));
        assert_eq!(n.publisher.as_deref(), Some("Origin"));
    }

    #[test]
    fn andere_sammlungsmuster() {
        let n = parse_file_stem("RG Rampage (Cosine Systems - 2012)");
        assert_eq!(
            (n.title.as_str(), n.year.as_deref(), n.publisher.as_deref()),
            ("RG Rampage", Some("2012"), Some("Cosine Systems"))
        );
        let n = parse_file_stem("Great_Giana_Sisters_1987_Rainbow_Arts");
        assert_eq!(
            (n.title.as_str(), n.year.as_deref(), n.publisher.as_deref()),
            ("Great Giana Sisters", Some("1987"), Some("Rainbow Arts"))
        );
        let n = parse_file_stem("1942");
        assert_eq!((n.title.as_str(), n.year), ("1942", None));
        let n = parse_file_stem("1942 1986 Elite");
        assert_eq!(
            (n.title.as_str(), n.year.as_deref()),
            ("1942", Some("1986"))
        );
    }

    #[test]
    fn einfacher_name() {
        let n = parse_file_stem("boulder_dash");
        assert_eq!(
            n,
            NameInfo {
                title: "boulder dash".into(),
                year: None,
                publisher: None
            }
        );
        let n = parse_file_stem("Turrican (Disk 2)");
        assert_eq!(n.title, "Turrican");
        assert_eq!(n.year, None);
        assert_eq!(parse_file_stem("[intro]").title, "[intro]");
    }
}
