//! Abfrage von ScreenScraper.fr (API v2): Spielinfos und Bilder über die Prüfsummen.
//!
//! Regeln der API, die hier eingehalten werden:
//! - höchstens eine Anfrage gleichzeitig und mindestens 1,2 s Abstand (auch für Bilder),
//! - Tageskontingent (Fehler 430) und gesperrte API (423, 426) beenden den Abruf,
//! - „zu viele Threads“ (429) und überlastete API (401) werden mit Pause wiederholt.
//!
//! Die JSON-Feldnamen folgen dem Quelltext von Skyscraper (Gemba/skyscraper), weil die
//! offizielle Beschreibung an einigen Stellen davon abweicht.

use crate::tr;
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BASE: &str = "https://api.screenscraper.fr/api2";
/// Systemkennung des Commodore 64 bei ScreenScraper.
pub const SYSTEM_C64: u32 = 66;
/// Abstand zwischen zwei Anfragen, wie von ScreenScraper erbeten.
/// Kürzere Suchbegriffe lehnt `jeuRecherche.php` ab („Erreur : informations obligatoires manquantes !“).
pub const MIN_SEARCH_CHARS: usize = 3;
const MIN_GAP: Duration = Duration::from_millis(1200);
const MAX_MEDIA: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub dev_id: String,
    pub dev_password: String,
    pub user: String,
    pub password: String,
}

/// Fehler, nach denen der Abruf anders weitergehen muss als bei einem einfachen Netzfehler.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiError {
    /// 403: Anmeldung abgelehnt (meist Entwickler-Kennung); mit dem Text von ScreenScraper.
    DevLogin(String),
    /// Benutzername oder Passwort falsch (Benutzerstufe 0).
    UserLogin,
    /// 430/431: Tageskontingent aufgebraucht.
    Quota,
    /// 423/426: API geschlossen oder Programm gesperrt.
    Closed(String),
    /// 401/429: API überlastet oder zu viele gleichzeitige Anfragen – später erneut versuchen.
    Busy(String),
    /// Sonstiger HTTP-Fehler.
    Http(u16, String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::DevLogin(t) if t.is_empty() || t.contains("développeur") => {
                f.write_str(&tr!(
                    "ScreenScraper lehnt die Entwickler-Kennung ab (403)",
                    "ScreenScraper rejects the developer credentials (403)"
                ))
            }
            ApiError::DevLogin(t) => f.write_str(&tr!(
                "ScreenScraper lehnt die Anmeldung ab (403): {t}",
                "ScreenScraper refuses the login (403): {t}"
            )),
            ApiError::UserLogin => f.write_str(&tr!(
                "ScreenScraper: Benutzername oder Passwort falsch",
                "ScreenScraper: wrong user name or password"
            )),
            ApiError::Quota => f.write_str(&tr!(
                "ScreenScraper: Tageskontingent aufgebraucht, morgen weiter",
                "ScreenScraper: daily quota used up, continue tomorrow"
            )),
            ApiError::Closed(t) => f.write_str(&tr!(
                "ScreenScraper ist derzeit nicht nutzbar: {t}",
                "ScreenScraper is currently unavailable: {t}"
            )),
            ApiError::Busy(t) => f.write_str(&tr!(
                "ScreenScraper ist ausgelastet: {t}",
                "ScreenScraper is overloaded: {t}"
            )),
            ApiError::Http(c, t) => f.write_str(&tr!(
                "ScreenScraper antwortet mit Fehler {c}: {t}",
                "ScreenScraper answers with error {c}: {t}"
            )),
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct UserInfo {
    pub id: String,
    pub level: i64,
    pub max_threads: i64,
    /// Höchstzahl Anfragen je Minute (0 = nicht angegeben).
    pub max_requests_per_min: i64,
    pub requests_today: i64,
    pub max_requests_per_day: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Media {
    pub kind: String,
    pub region: String,
    pub url: String,
    pub format: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Game {
    pub id: String,
    pub title: Option<String>,
    pub synopsis: Option<String>,
    pub year: Option<String>,
    pub publisher: Option<String>,
    pub developer: Option<String>,
    pub genres: Vec<String>,
    pub players: Option<String>,
    /// Bewertung 0–20 wie bei ScreenScraper.
    pub rating: Option<f64>,
    pub media: Vec<Media>,
    /// Treffer ohne passende Prüfsumme: ScreenScraper hat (vermutlich über den Dateinamen) ein
    /// anderes Spiel geliefert – wie ein Namenstreffer zu behandeln.
    pub name_only: bool,
}

/// Was zu einer Datei bekannt ist, um sie nachzuschlagen.
#[derive(Debug, Clone)]
pub struct RomQuery {
    pub file_name: String,
    /// Titel aus dem Dateinamen, für die Suche über den Namen.
    pub title: String,
    pub size: i64,
    pub crc32: String,
    pub md5: String,
    pub sha1: String,
}

pub enum Lookup {
    Found(Box<Game>),
    NotFound,
}

pub struct Client {
    agent: ureq::Agent,
    creds: Credentials,
    softname: String,
    last_request: Option<Instant>,
    /// Gemeinsamer Takt mehrerer Clients (Threads): frühester Zeitpunkt der nächsten Anfrage
    /// und Mindestabstand, damit zusammen das Minutenkontingent eingehalten wird.
    shared: Option<(Arc<Mutex<Instant>>, Duration)>,
}

impl Client {
    pub fn new(creds: Credentials, softname: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .user_agent(softname)
            .build()
            .into();
        Client {
            agent,
            creds,
            softname: softname.to_string(),
            last_request: None,
            shared: None,
        }
    }

    /// Mehrere Clients für gleichzeitige Abfragen: je einer pro Thread, zusammen höchstens
    /// `per_min` Anfragen je Minute (0 = nur der Abstand je Client).
    pub fn pool(creds: &Credentials, softname: &str, threads: usize, per_min: i64) -> Vec<Client> {
        let gap = if per_min > 0 {
            Duration::from_millis(60_000 / per_min as u64)
        } else {
            Duration::ZERO
        };
        let gate = Arc::new(Mutex::new(Instant::now()));
        (0..threads.max(1))
            .map(|_| {
                let mut c = Client::new(creds.clone(), softname);
                c.shared = Some((gate.clone(), gap));
                c
            })
            .collect()
    }

    fn pace(&mut self) {
        if let Some(t) = self.last_request {
            let elapsed = t.elapsed();
            if elapsed < MIN_GAP {
                std::thread::sleep(MIN_GAP - elapsed);
            }
        }
        if let Some((gate, gap)) = &self.shared {
            // Platz im gemeinsamen Takt reservieren, dann außerhalb der Sperre warten.
            let wait = {
                let mut next = gate.lock().unwrap_or_else(|e| e.into_inner());
                let now = Instant::now();
                let slot = (*next).max(now);
                *next = slot + *gap;
                slot - now
            };
            std::thread::sleep(wait);
        }
        self.last_request = Some(Instant::now());
    }

    fn get(&mut self, endpoint: &str, params: &[(&str, String)]) -> Result<(u16, String)> {
        self.pace();
        let mut req = self
            .agent
            .get(format!("{BASE}/{endpoint}"))
            .query("devid", &self.creds.dev_id)
            .query("devpassword", &self.creds.dev_password)
            .query("softname", &self.softname)
            .query("output", "json");
        if !self.creds.user.is_empty() {
            req = req
                .query("ssid", &self.creds.user)
                .query("sspassword", &self.creds.password);
        }
        for (k, v) in params {
            req = req.query(*k, v);
        }
        // Keine URL in Fehlermeldungen: sie enthält die Passwörter.
        let mut resp = req.call().map_err(|e| {
            anyhow::anyhow!(tr!(
                "Verbindung zu ScreenScraper fehlgeschlagen: {e}",
                "Connection to ScreenScraper failed: {e}"
            ))
        })?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().read_to_string().context(tr!(
            "Antwort von ScreenScraper unlesbar",
            "Unreadable answer from ScreenScraper"
        ))?;
        Ok((status, body))
    }

    /// Wie [`check_status`], aber mit richtiggestellter Anmeldemeldung: ScreenScraper antwortet
    /// auch auf eine falsche Entwickler-Kennung mit „Vérifier les identifiants utilisateurs“.
    /// Dann die Entwickler-Kennung allein prüfen (ssinfraInfos braucht kein Benutzerkonto).
    fn checked(&mut self, status: u16, body: &str) -> Result<(), ApiError> {
        match check_status(status, body) {
            Err(ApiError::UserLogin) => {
                let user = std::mem::take(&mut self.creds.user);
                let dev = self.get("ssinfraInfos.php", &[]);
                self.creds.user = user;
                match dev {
                    Ok((s, b)) => match check_status(s, &b) {
                        Err(e @ ApiError::DevLogin(_)) => Err(e),
                        _ => Err(ApiError::UserLogin),
                    },
                    Err(_) => Err(ApiError::UserLogin),
                }
            }
            other => other,
        }
    }

    pub fn user_info(&mut self) -> Result<UserInfo> {
        let (status, body) = self.get("ssuserInfos.php", &[])?;
        self.checked(status, &body)?;
        let json = parse_json(&body)?;
        let info = parse_user(&json["response"]["ssuser"]);
        if !self.creds.user.is_empty() && info.level == 0 {
            return Err(ApiError::UserLogin.into());
        }
        Ok(info)
    }

    pub fn game_info(&mut self, rom: &RomQuery, lang: &str) -> Result<Lookup> {
        let params = [
            ("systemeid", SYSTEM_C64.to_string()),
            ("romtype", "rom".to_string()),
            ("romnom", rom.file_name.clone()),
            ("romtaille", rom.size.to_string()),
            ("crc", rom.crc32.to_uppercase()),
            ("md5", rom.md5.to_uppercase()),
            ("sha1", rom.sha1.to_uppercase()),
        ];
        let (status, body) = self.get("jeuInfos.php", &params)?;
        if status == 404 || body.contains("non trouvée") {
            return Ok(Lookup::NotFound);
        }
        self.checked(status, &body)?;
        let json = parse_json(&body)?;
        let jeu = &json["response"]["jeu"];
        if jeu.is_null() {
            return Ok(Lookup::NotFound);
        }
        let mut game = parse_game(jeu, lang);
        game.name_only = !rom_matches(jeu, rom);
        Ok(Lookup::Found(Box::new(game)))
    }

    /// Vollständige Angaben zu einer Spielnummer (nach einer Namenssuche).
    pub fn game_by_id(&mut self, id: &str, lang: &str) -> Result<Lookup> {
        let params = [
            ("systemeid", SYSTEM_C64.to_string()),
            ("gameid", id.to_string()),
        ];
        let (status, body) = self.get("jeuInfos.php", &params)?;
        if status == 404 || body.contains("non trouvée") {
            return Ok(Lookup::NotFound);
        }
        self.checked(status, &body)?;
        let json = parse_json(&body)?;
        if json["response"]["jeu"].is_null() {
            return Ok(Lookup::NotFound);
        }
        Ok(Lookup::Found(Box::new(parse_game(
            &json["response"]["jeu"],
            lang,
        ))))
    }

    /// Namenssuche (bis zu 30 Treffer, nach Wahrscheinlichkeit sortiert).
    /// Genres, wie ScreenScraper sie für Vorschläge erwartet (Wert = französischer Name).
    pub fn genre_list(&mut self) -> Result<Vec<Choice>> {
        self.choice_list("genresListe.php")
    }

    /// Spieleranzahlen, wie ScreenScraper sie für Vorschläge erwartet (Wert = Name der Gruppe).
    pub fn player_list(&mut self) -> Result<Vec<Choice>> {
        self.choice_list("nbJoueursListe.php")
    }

    fn choice_list(&mut self, endpoint: &str) -> Result<Vec<Choice>> {
        let (status, body) = self.get(endpoint, &[])?;
        self.checked(status, &body)?;
        Ok(parse_choices(&parse_json(&body)?["response"]))
    }

    pub fn search_name(&mut self, name: &str, lang: &str) -> Result<Vec<Game>> {
        anyhow::ensure!(
            name.trim().chars().count() >= MIN_SEARCH_CHARS,
            tr!(
                "Suchbegriff zu kurz – ScreenScraper sucht erst ab {} Zeichen.",
                "Search term too short – ScreenScraper needs at least {} characters.",
                MIN_SEARCH_CHARS
            )
        );
        let params = [
            ("systemeid", SYSTEM_C64.to_string()),
            ("recherche", name.to_string()),
        ];
        let (status, body) = self.get("jeuRecherche.php", &params)?;
        if status == 404 || body.contains("non trouvée") {
            return Ok(Vec::new());
        }
        self.checked(status, &body)?;
        let json = parse_json(&body)?;
        Ok(json["response"]["jeux"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|j| parse_game(j, lang))
                    .filter(|g| !g.id.is_empty())
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Lädt ein Bild; die URL kommt so, wie ScreenScraper sie geliefert hat.
    /// Schickt einen Vorschlag an die Moderation von ScreenScraper (botProposition.php) und gibt
    /// die Textantwort zurück. Braucht das Konto des Nutzers.
    pub fn propose(&mut self, p: &Proposal) -> Result<String> {
        anyhow::ensure!(
            !self.creds.user.is_empty(),
            tr!(
                "Für Vorschläge braucht es ein ScreenScraper-Konto (Einstellungen).",
                "Proposals need a ScreenScraper account (Settings)."
            )
        );
        self.pace();
        let boundary = format!(
            "breadbin{:016x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        );
        let (fields, file) = p.form();
        let body = multipart(&fields, file, &boundary);
        let mut resp = self
            .agent
            .post(format!("{BASE}/botProposition.php"))
            .query("devid", &self.creds.dev_id)
            .query("devpassword", &self.creds.dev_password)
            .query("softname", &self.softname)
            .query("ssid", &self.creds.user)
            .query("sspassword", &self.creds.password)
            .header(
                "Content-Type",
                &format!("multipart/form-data; boundary={boundary}"),
            )
            .send(&body[..])
            .map_err(|e| {
                anyhow::anyhow!(tr!(
                    "Verbindung zu ScreenScraper fehlgeschlagen: {e}",
                    "Connection to ScreenScraper failed: {e}"
                ))
            })?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        check_status_text(status, &text)?;
        Ok(plain_text(&text))
    }

    pub fn download(&mut self, url: &str) -> Result<Vec<u8>> {
        self.pace();
        let mut resp = self.agent.get(url).call().map_err(|e| {
            anyhow::anyhow!(tr!(
                "Bild-Download fehlgeschlagen: {e}",
                "Picture download failed: {e}"
            ))
        })?;
        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            check_status(status, &body)?;
            anyhow::bail!(tr!(
                "Bild-Download: Fehler {status}",
                "Picture download: error {status}"
            ));
        }
        let mut data = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(MAX_MEDIA)
            .read_to_end(&mut data)?;
        Ok(data)
    }
}

/// Eintrag einer Auswahlliste von ScreenScraper (Genre, Spieleranzahl).
#[derive(Debug, Clone, Default, PartialEq, Serialize, serde::Deserialize)]
pub struct Choice {
    pub id: String,
    /// Übergeordneter Eintrag (leer bei Hauptgenres).
    pub parent: String,
    /// Namen je Sprache (`de`, `en`, `fr` …); ohne Sprache unter "".
    pub names: std::collections::BTreeMap<String, String>,
}

/// Liest eine Auswahlliste; die Einträge stehen je nach Liste unter einem anderen Schlüssel und
/// mal als Feld, mal als Objekt mit den Nummern als Schlüssel.
pub fn parse_choices(response: &Value) -> Vec<Choice> {
    let Some(obj) = response.as_object() else {
        return Vec::new();
    };
    for (key, v) in obj {
        if matches!(key.as_str(), "serveurs" | "ssuser") {
            continue;
        }
        let items: Vec<&Value> = match v {
            Value::Array(a) => a.iter().collect(),
            Value::Object(o) => o.values().collect(),
            _ => continue,
        };
        let list: Vec<Choice> = items.into_iter().filter_map(parse_choice).collect();
        if !list.is_empty() {
            return list;
        }
    }
    Vec::new()
}

fn parse_choice(v: &Value) -> Option<Choice> {
    let o = v.as_object()?;
    let scalar = |v: &Value| match v {
        Value::Number(n) => Some(n.to_string()),
        v => text(v),
    };
    let names: std::collections::BTreeMap<String, String> = o
        .iter()
        .filter_map(|(k, v)| {
            let lang = if k == "nom" {
                ""
            } else {
                k.strip_prefix("nom_")?
            };
            Some((lang.to_string(), text(v)?))
        })
        .collect();
    if names.is_empty() {
        return None;
    }
    let parent = o
        .get("parent")
        .or_else(|| o.get("parentid"))
        .and_then(scalar)
        .filter(|p| p != "0")
        .unwrap_or_default();
    Some(Choice {
        id: scalar(o.get("id")?)?,
        parent,
        names,
    })
}

/// Ein Vorschlag an ScreenScraper zu einem vorhandenen Spiel: Textangabe oder Bild.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Proposal {
    /// `typ` wie in der API-Liste „modiftypeinfo“ (name, editeur, developpeur, datessortie, description …).
    Info {
        game_id: String,
        typ: String,
        region: String,
        langue: String,
        text: String,
        source: String,
    },
    /// `typ` wie in „modiftypemedia“ (sstitle, ss, box-2D …); Bild als Bytes mit Dateinamen.
    Media {
        game_id: String,
        typ: String,
        region: String,
        #[serde(skip)]
        data: Vec<u8>,
        file_name: String,
        source: String,
    },
}

/// Formularfelder und ggf. eine Datei (Feldname, Dateiname, Inhalt).
type Form<'a> = (
    Vec<(&'static str, String)>,
    Option<(&'static str, &'a str, &'a [u8])>,
);

impl Proposal {
    fn form(&self) -> Form<'_> {
        match self {
            Proposal::Info {
                game_id,
                typ,
                region,
                langue,
                text,
                source,
            } => (
                vec![
                    ("gameid", game_id.clone()),
                    ("modiftypeinfo", typ.clone()),
                    ("modifregion", region.clone()),
                    ("modiflangue", langue.clone()),
                    ("modifversion", String::new()),
                    ("modiftexte", text.clone()),
                    ("modifsource", source.clone()),
                ],
                None,
            ),
            Proposal::Media {
                game_id,
                typ,
                region,
                data,
                file_name,
                source,
            } => (
                vec![
                    ("gameid", game_id.clone()),
                    ("modiftypemedia", typ.clone()),
                    ("modiftyperegion", region.clone()),
                    // Verpackung braucht eine Datenträgernummer; 0 = erster.
                    (
                        "modiftypenumsupport",
                        if typ.starts_with("box") {
                            "0".into()
                        } else {
                            String::new()
                        },
                    ),
                    ("modiftypeversion", String::new()),
                    ("modifmediasource", source.clone()),
                ],
                Some(("modifmediafile", file_name.as_str(), data.as_slice())),
            ),
        }
    }
}

/// Baut einen multipart/form-data-Rumpf.
pub(crate) fn multipart(
    fields: &[(&str, String)],
    file: Option<(&str, &str, &[u8])>,
    boundary: &str,
) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in fields {
        out.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    if let Some((name, file_name, data)) = file {
        let safe: String = file_name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
            .collect();
        let mime = if safe.ends_with(".png") {
            "image/png"
        } else if safe.ends_with(".zip") {
            "application/zip"
        } else {
            "image/jpeg"
        };
        out.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{safe}\"\r\nContent-Type: {mime}\r\n\r\n").as_bytes(),
        );
        out.extend_from_slice(data);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    out
}

/// Antworttext ohne HTML-Tags und überflüssige Leerzeichen.
fn plain_text(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Ordnet HTTP-Status und Fehlertexte den Fällen aus der API-Beschreibung zu.
/// Lesbarer Anfang einer Antwort für Fehlermeldungen: ohne HTML-Tags, Leerraum zusammengefasst.
fn snippet(body: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in body.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(if c.is_whitespace() { ' ' } else { c }),
            _ => {}
        }
    }
    let t = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > 160 {
        format!("{} …", t.chars().take(160).collect::<String>())
    } else {
        t
    }
}

/// Antwort mit Status 200, aber ohne JSON: ScreenScraper meldet manche Zustände als reinen Text
/// (zu viele Threads, Kontingent, gesperrt) oder liefert eine leere Seite.
fn classify_text(body: &str) -> ApiError {
    let text = snippet(body);
    let low = text.to_lowercase();
    if text.is_empty() {
        ApiError::Busy(tr!("leere Antwort", "empty answer"))
    } else if low.contains("thread") || low.contains("trop de") || low.contains("surcharg") {
        ApiError::Busy(text)
    } else if low.contains("demain") || low.contains("quota") || low.contains("dépassé") {
        ApiError::Quota
    } else if low.contains("blacklist") || low.contains("fermé") {
        ApiError::Closed(text)
    } else {
        ApiError::Http(200, text)
    }
}

/// Prüft eine Antwort, die JSON sein muss; Text mit Status 200 wird über [`classify_text`] eingeordnet.
pub fn check_status(status: u16, body: &str) -> Result<(), ApiError> {
    if (200..300).contains(&status)
        && !body.contains("Erreur de login")
        && !body.trim_start().starts_with(['{', '['])
    {
        return Err(classify_text(body));
    }
    check_status_text(status, body)
}

/// Prüft eine Antwort, die auch reiner Text sein darf (Vorschläge).
pub fn check_status_text(status: u16, body: &str) -> Result<(), ApiError> {
    let text: String = body
        .chars()
        .take(200)
        .collect::<String>()
        .trim()
        .to_string();
    match status {
        200..=299 if !body.contains("Erreur de login") => Ok(()),
        // Der Text sagt, welche Anmeldung scheitert (Entwickler oder Benutzer); er enthält keine Passwörter.
        _ if body.contains("Erreur de login") && body.contains("utilisateur") => {
            Err(ApiError::UserLogin)
        }
        403 => Err(ApiError::DevLogin(text)),
        _ if body.contains("Erreur de login") => Err(ApiError::DevLogin(text)),
        430 | 431 => Err(ApiError::Quota),
        423 | 426 => Err(ApiError::Closed(text)),
        401 | 429 => Err(ApiError::Busy(text)),
        _ => Err(ApiError::Http(status, text)),
    }
}

/// Entfernt Kommas vor `}` oder `]` außerhalb von Zeichenketten. ScreenScraper liefert
/// gelegentlich solches JSON (Skyscraper flickt dieselbe Stelle).
fn strip_trailing_commas(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    let mut escaped = false;
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        if c == '"' {
            in_str = true;
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// ScreenScraper liefert in Texten (Beschreibungen) gelegentlich rohe Steuerzeichen (Tab, Zeilenumbruch,
/// auch seltene wie U+0001), die JSON innerhalb von Zeichenketten nicht erlaubt. Innerhalb von
/// Zeichenketten werden sie maskiert; außerhalb bleiben sie als Leerraum stehen.
fn escape_controls(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let (mut in_str, mut esc) = (false, false);
    for c in s.chars() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            } else if (c as u32) < 0x20 {
                match c {
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    _ => out.push_str(&format!("\\u{:04x}", c as u32)),
                }
                continue;
            }
        } else if c == '"' {
            in_str = true;
        }
        out.push(c);
    }
    out
}

/// Passt eine der Prüfsummen, die ScreenScraper zum Treffer liefert (`rom`, sonst `roms`), zur Datei?
/// Ohne passende Prüfsumme hat ScreenScraper über den Namen zugeordnet (2026-11-02 beobachtet:
/// `warlock.prg` eines neuen Spiels → „Warlock's Quest“ von 1988).
pub fn rom_matches(jeu: &Value, rom: &RomQuery) -> bool {
    let norm = |s: &str| s.trim().trim_start_matches("0x").to_ascii_uppercase();
    let crc = |s: &str| norm(s).trim_start_matches('0').to_string();
    let hit = |r: &Value| {
        let f = |k: &str| r[k].as_str().map(str::trim).filter(|s| !s.is_empty());
        f("romsha1").is_some_and(|s| norm(s) == norm(&rom.sha1))
            || f("rommd5").is_some_and(|s| norm(s) == norm(&rom.md5))
            || f("romcrc").is_some_and(|s| crc(s) == crc(&rom.crc32))
    };
    hit(&jeu["rom"]) || jeu["roms"].as_array().is_some_and(|a| a.iter().any(hit))
}

pub fn parse_json(body: &str) -> Result<Value> {
    serde_json::from_str(body)
        .or_else(|_| serde_json::from_str(&strip_trailing_commas(&escape_controls(body))))
        .with_context(|| {
            tr!(
                "ScreenScraper lieferte keine lesbare Antwort („{}“)",
                "ScreenScraper returned no readable answer (“{}”)",
                snippet(body)
            )
        })
}

fn as_i64(v: &Value) -> i64 {
    match v {
        Value::Number(n) => n.as_i64().unwrap_or(0),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn text(v: &Value) -> Option<String> {
    let t = match v {
        Value::String(s) => s.as_str(),
        Value::Object(o) => o.get("text").and_then(Value::as_str)?,
        _ => return None,
    };
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// Aus einer Liste `[{key: code, text}]` den Text in der ersten passenden Sprache/Region.
fn pick(list: &Value, key: &str, prefs: &[&str]) -> Option<String> {
    let items = list.as_array()?;
    for p in prefs {
        if let Some(item) = items.iter().find(|i| i[key].as_str() == Some(p))
            && let Some(t) = text(item)
        {
            return Some(t);
        }
    }
    items.iter().find_map(text)
}

fn lang_prefs(lang: &str) -> Vec<&str> {
    let mut v = vec![lang];
    for l in ["en", "wor", "de", "fr"] {
        if !v.contains(&l) {
            v.push(l);
        }
    }
    v
}

const REGIONS: [&str; 6] = ["eu", "wor", "us", "ss", "uk", "jp"];

pub fn parse_user(u: &Value) -> UserInfo {
    UserInfo {
        id: text(&u["id"]).unwrap_or_default(),
        level: as_i64(&u["niveau"]),
        max_threads: as_i64(&u["maxthreads"]),
        max_requests_per_min: as_i64(&u["maxrequestspermin"]),
        requests_today: as_i64(&u["requeststoday"]),
        max_requests_per_day: as_i64(&u["maxrequestsperday"]),
    }
}

pub fn parse_game(jeu: &Value, lang: &str) -> Game {
    let langs = lang_prefs(lang);
    let id = match &jeu["id"] {
        Value::Number(n) => n.to_string(),
        v => text(v).unwrap_or_default(),
    };
    let title =
        pick(&jeu["noms"], "region", &REGIONS).filter(|t| !t.to_lowercase().contains("notgame"));

    // Früheste Jahreszahl über alle Regionen.
    let year = jeu["dates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(text)
        .filter_map(|d| {
            d.get(..4)
                .filter(|y| y.chars().all(|c| c.is_ascii_digit()))
                .map(str::to_string)
        })
        .min();

    // Genres: laut Skyscraper eine Liste mit lokalisierten Namen, laut Doku teils ein Objekt.
    let genres = match &jeu["genres"] {
        Value::Array(list) => list
            .iter()
            .filter_map(|g| pick(&g["noms"], "langue", &langs).or_else(|| text(g)))
            .collect(),
        Value::Object(o) => o
            .get(&format!("genres_{lang}"))
            .or_else(|| o.get("genres_en"))
            .and_then(Value::as_str)
            .map(|s| {
                s.split(',')
                    .map(|g| g.trim().to_string())
                    .filter(|g| !g.is_empty())
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    // ScreenScraper liefert Ober- und Untergenre („Plattform“ und „Plattform / Jump 'n' Run“):
    // das Obergenre steckt schon im Untergenre und fällt weg, Doppelte ebenso.
    let mut genres: Vec<String> = genres
        .iter()
        .filter(|g| !genres.iter().any(|o| o.starts_with(&format!("{g} / "))))
        .cloned()
        .collect();
    genres.dedup();

    let rating = text(&jeu["note"]).and_then(|n| n.parse::<f64>().ok());
    let media = jeu["medias"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some(Media {
                kind: m["type"].as_str()?.to_string(),
                region: m["region"].as_str().unwrap_or("").to_string(),
                url: m["url"].as_str()?.to_string(),
                format: m["format"].as_str().unwrap_or("png").to_string(),
            })
        })
        .collect();

    Game {
        id,
        title,
        synopsis: pick(&jeu["synopsis"], "langue", &langs),
        year,
        publisher: text(&jeu["editeur"]),
        developer: text(&jeu["developpeur"]),
        genres,
        players: text(&jeu["joueurs"]),
        rating,
        media,
        name_only: false,
    }
}

/// Entfernt Szene-Kennungen, die nicht zum Spielnamen gehören: Trainer („+10HDG“, „+3“),
/// Versionen („v1n“, „v2b3“), Datumsangaben („24092026“, auch „24092026-fix1“) und am Ende
/// Disk-Seiten („s1“, „side b“, „disk 2“). „+“ ohne Ziffer bleibt („International Karate +“).
/// Für die Namenssuche bei ScreenScraper und das Zusammenfassen.
pub fn clean_title(s: &str) -> String {
    split_title(s).0
}

/// Disk- bzw. Seitenkennung am Namensende („Edge of Disgrace side 2“ → „2“, „Game Disk 1 Side B“ →
/// „1/B“), sonst `None`. Gleicher Name ohne Kennung = dieselbe Diskettenreihe.
pub fn disk_side(s: &str) -> Option<String> {
    split_title(s).1
}

/// Bereinigter Name (siehe [`clean_title`]) und die entfernte Disk-/Seitenkennung.
fn split_title(s: &str) -> (String, Option<String>) {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '+' && chars.peek().is_some_and(|n| n.is_ascii_digit()) {
            while chars.peek().is_some_and(|n| n.is_ascii_alphanumeric()) {
                chars.next();
            }
            continue;
        }
        out.push(c);
    }
    let is_version = |w: &str| {
        let mut it = w.chars();
        matches!(it.next(), Some('v' | 'V'))
            && it.next().is_some_and(|c| c.is_ascii_digit())
            && w.chars().all(|c| c.is_ascii_alphanumeric())
    };
    // Auch mit Anhang: „24092026-fire2fix1“.
    let is_date = |w: &str| {
        let d = w.split(['-', '_']).next().unwrap_or(w);
        matches!(d.len(), 6 | 8) && d.chars().all(|c| c.is_ascii_digit())
    };
    let mut words: Vec<&str> = out
        .split_whitespace()
        .filter(|w| !is_version(w) && !is_date(w))
        .collect();
    // Disk-Seiten am Ende, nur wenn davor noch ein Name steht.
    let side_no = |w: &str| {
        w.len() == 1
            && w.chars()
                .all(|c| matches!(c.to_ascii_lowercase(), 'a' | 'b' | '1'..='9'))
    };
    let mut sides: Vec<String> = Vec::new();
    loop {
        let n = words.len();
        let lower: Vec<String> = words
            .iter()
            .rev()
            .take(2)
            .map(|w| w.to_lowercase())
            .collect();
        let (cut, label) = match lower.as_slice() {
            [last, prev, ..]
                if n > 2 && matches!(prev.as_str(), "side" | "disk") && side_no(last) =>
            {
                (2, last.clone())
            }
            [last, ..] if n > 1 => {
                let rest = |p: &str| {
                    last.strip_prefix(p)
                        .filter(|r| side_no(r))
                        .map(str::to_string)
                };
                match rest("side")
                    .or_else(|| rest("disk"))
                    .or_else(|| rest("s").filter(|_| last.len() == 2))
                {
                    Some(r) => (1, r),
                    None => (0, String::new()),
                }
            }
            _ => (0, String::new()),
        };
        if cut == 0 {
            break;
        }
        sides.insert(0, label.to_uppercase());
        words.truncate(n - cut);
    }
    (
        words.join(" "),
        (!sides.is_empty()).then(|| sides.join("/")),
    )
}

/// Wörter eines Namens für den Vergleich: Kleinbuchstaben, nur Buchstaben und Ziffern,
/// ohne Szene-Kennungen (siehe [`clean_title`]), „+“ als „plus“, ohne „The“ und ohne führende
/// Sammlungsnummer („274 Great Giana Sisters“).
pub fn title_words(s: &str) -> Vec<String> {
    let lower = clean_title(s).to_lowercase().replace('+', " plus ");
    let mut words: Vec<String> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect();
    if words.len() > 1 && words[0].chars().all(|c| c.is_ascii_digit()) && words[0].len() != 4 {
        words.remove(0);
    }
    words.retain(|w| w != "the");
    words
}

pub fn normalize_title(s: &str) -> String {
    title_words(s).concat()
}

/// Fortsetzungs-Kennzeichen: „Boulder Dash II“ ist ein anderes Spiel als „Boulder Dash“.
fn is_sequel_marker(w: &str) -> bool {
    w.chars().all(|c| c.is_ascii_digit())
        || matches!(w, "ii" | "iii" | "iv" | "v" | "vi" | "plus" | "deluxe")
}

/// Passt ein Suchtreffer sicher genug zum gesuchten Titel? Gleich nach dem Normalisieren,
/// oder der eine Name steckt im anderen und ist fast so lang (Schreibvarianten) – aber nie,
/// wenn nur einer der beiden eine Fortsetzungsnummer trägt.
pub fn names_match(wanted: &str, found: &str) -> bool {
    let (wa, wb) = (title_words(wanted), title_words(found));
    let (a, b) = (wa.concat(), wb.concat());
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    let extra_a = wa.iter().filter(|w| !wb.contains(w));
    let extra_b = wb.iter().filter(|w| !wa.contains(w));
    if extra_a.chain(extra_b).any(|w| is_sequel_marker(w)) {
        return false;
    }
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    // Untertitel: „USS John Young : The Naval Warfare Simulator“ passt zu „U.S.S. John Young“.
    let main = |s: &str| {
        normalize_title(
            s.split(':')
                .next()
                .unwrap_or(s)
                .split(" - ")
                .next()
                .unwrap_or(s),
        )
    };
    if (main(found).len() >= 4 && main(found) == a)
        || (main(wanted).len() >= 4 && main(wanted) == b)
    {
        return true;
    }
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    long.contains(short.as_str()) && short.len() * 10 >= long.len() * 8
}

/// Bester Treffer einer Namenssuche, der zum Titel passt (Rangfolge von ScreenScraper).
pub fn best_name_match<'a>(wanted: &str, games: &'a [Game]) -> Option<&'a Game> {
    games
        .iter()
        .find(|g| g.title.as_deref().is_some_and(|t| names_match(wanted, t)))
}

/// Das passendste Bild einer Art (Region nach Vorrang, sonst das erste).
pub fn best_media<'a>(game: &'a Game, kind: &str) -> Option<&'a Media> {
    let of_kind: Vec<&Media> = game.media.iter().filter(|m| m.kind == kind).collect();
    REGIONS
        .iter()
        .find_map(|r| of_kind.iter().find(|m| m.region == *r).copied())
        .or_else(|| of_kind.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Gekürzte Antwort im Aufbau, den Skyscraper ausliest – mit dem bekannten Komma-Fehler.
    const SAMPLE: &str = r#"{
        "header": {"success": "true"},
        "response": {
            "ssuser": {"id": "tester", "niveau": "1", "maxthreads": "1",
                       "requeststoday": "12", "maxrequestsperday": "20000"},
            "jeu": {
                "id": "4711",
                "noms": [{"region": "us", "text": "Giana Sisters"},
                         {"region": "eu", "text": "The Great Giana Sisters"}],
                "synopsis": [{"langue": "en", "text": "Maria and Giana ..."},
                             {"langue": "de", "text": "Maria und Giana ..."}],
                "dates": [{"region": "us", "text": "1988"}, {"region": "eu", "text": "1987-05-01"}],
                "editeur": {"id": "1", "text": "Rainbow Arts"},
                "developpeur": {"id": "2", "text": "Time Warp"},
                "joueurs": {"text": "1-2"},
                "note": {"text": "16"},
                "genres": [{"id": "1", "noms": [{"langue": "en", "text": "Platform"},
                                                 {"langue": "de", "text": "Plattform"}]}],
                "medias": [
                    {"type": "ss", "region": "wor", "url": "https://example/ss.png", "format": "png"},
                    {"type": "box-2D", "region": "us", "url": "https://example/us.png", "format": "png"},
                    {"type": "box-2D", "region": "eu", "url": "https://example/eu.png", "format": "png"},
                ],
            }
        }
    }"#;

    #[test]
    fn spiel_auslesen() {
        let json = parse_json(SAMPLE).unwrap();
        let g = parse_game(&json["response"]["jeu"], "de");
        assert_eq!(g.id, "4711");
        assert_eq!(g.title.as_deref(), Some("The Great Giana Sisters"));
        assert_eq!(g.synopsis.as_deref(), Some("Maria und Giana ..."));
        assert_eq!(g.year.as_deref(), Some("1987"));
        assert_eq!(g.publisher.as_deref(), Some("Rainbow Arts"));
        assert_eq!(g.developer.as_deref(), Some("Time Warp"));
        assert_eq!(g.genres, ["Plattform"]);
        assert_eq!(g.players.as_deref(), Some("1-2"));
        assert_eq!(g.rating, Some(16.0));
        assert_eq!(
            best_media(&g, "box-2D").unwrap().url,
            "https://example/eu.png"
        );
        assert_eq!(best_media(&g, "ss").unwrap().region, "wor");
        assert!(best_media(&g, "sstitle").is_none());

        let g = parse_game(&json["response"]["jeu"], "fr");
        assert_eq!(
            g.synopsis.as_deref(),
            Some("Maria and Giana ..."),
            "Rückfall auf Englisch"
        );

        let u = parse_user(&json["response"]["ssuser"]);
        assert_eq!(
            (u.level, u.requests_today, u.max_requests_per_day),
            (1, 12, 20000)
        );
    }

    #[test]
    fn genres_als_objekt_und_leere_felder() {
        let jeu: Value = serde_json::from_str(
            r#"{"id": 5, "genres": {"genres_id": "1,2", "genres_de": "Action, Sport"},
                "noms": [{"region": "ss", "text": "ZZZ(notgame)"}], "editeur": {"text": " "}}"#,
        )
        .unwrap();
        let g = parse_game(&jeu, "de");
        assert_eq!(g.id, "5");
        assert_eq!(g.genres, ["Action", "Sport"]);
        let g = parse_game(
            &serde_json::json!({"id": 6, "genres": {"genres_de": "Plattform / Jump 'n' Run Scrolling, Plattform"}}),
            "de",
        );
        assert_eq!(
            g.genres,
            ["Plattform / Jump 'n' Run Scrolling"],
            "Obergenre fällt weg"
        );
        assert_eq!(g.title, None);
        assert_eq!(g.publisher, None);
    }

    #[test]
    fn namensvergleich() {
        assert!(names_match(
            "274 Great Giana Sisters",
            "The Great Giana Sisters"
        ));
        assert!(names_match(
            "Great Giana Sisters, The",
            "Great Giana Sisters"
        ));
        assert!(
            !names_match("Boulder Dash", "Boulder Dash II"),
            "Fortsetzung ist ein anderes Spiel"
        );
        assert!(!names_match("Boulder Dash 2", "Boulder Dash"));
        assert!(names_match("Wizball", "Wiz Ball"));
        assert!(!names_match("Turrican", "Turrican II - The Final Fight"));
        assert!(!names_match("Elite", "Elite Beat Agents"));
        assert!(names_match("1942", "1942"), "Jahreszahl als Titel bleibt");
        assert!(!names_match("IK", "IK+"));
        assert_eq!(
            normalize_title("274_Great Giana-Sisters!"),
            "greatgianasisters"
        );

        let g = |id: &str, t: &str| Game {
            id: id.into(),
            title: Some(t.into()),
            ..Default::default()
        };
        let list = [
            g("1", "Giana Sisters II"),
            g("2", "The Great Giana Sisters"),
        ];
        assert_eq!(
            best_name_match("Great Giana Sisters", &list).unwrap().id,
            "2"
        );
        // Trainer-, Versions- und Datumskennungen gehören nicht zum Namen.
        assert_eq!(clean_title("Space Gun +10HDG"), "Space Gun");
        assert_eq!(clean_title("spacegun v1q 24092026"), "spacegun");
        assert_eq!(
            clean_title("International Karate +"),
            "International Karate +"
        );
        assert_eq!(clean_title("spacegun v1q 24092026-fire2fix1"), "spacegun");
        assert_eq!(clean_title("spacegun s1"), "spacegun");
        assert_eq!(
            clean_title("Defender of the Crown Disk 2"),
            "Defender of the Crown"
        );
        assert_eq!(clean_title("Elite side B"), "Elite");
        assert_eq!(clean_title("S1"), "S1", "allein bleibt es der Name");
        assert_eq!(disk_side("Edge of Disgrace side 2").as_deref(), Some("2"));
        assert_eq!(disk_side("forgotten worlds side a").as_deref(), Some("A"));
        assert_eq!(disk_side("Game Disk 1 Side B").as_deref(), Some("1/B"));
        assert_eq!(disk_side("spacegun s1").as_deref(), Some("1"));
        assert_eq!(disk_side("Turrican 2 - Side1").as_deref(), Some("1"));
        assert_eq!(disk_side("Turrican 2"), None);
        assert_eq!(disk_side("S1"), None);
        assert_eq!(clean_title("Space Gunner"), "Space Gunner");
        assert_ne!(normalize_title("SpaceGunner"), normalize_title("Space Gun"));
        assert_eq!(normalize_title("SpaceGun"), normalize_title("Space Gun"));
        assert!(names_match("Space Gun +12DFHG", "Space Gun"));
        assert!(names_match("spacegun v2d 26102026", "Space Gun"));
        assert!(!names_match("IK+", "IK"));
        assert!(names_match(
            "U.S.S. John Young +2DGW",
            "USS John Young : The Naval Warfare Simulator"
        ));
        assert!(
            !names_match("Rambo", "Rambo: First Blood Part II"),
            "Fortsetzung im Untertitel"
        );
        assert!(!names_match("Ultima", "Ultima IV: Quest of the Avatar"));
        assert!(best_name_match("Katakis", &list).is_none());
    }

    #[test]
    fn vorschlag_als_multipart() {
        let p = Proposal::Info {
            game_id: "36525".into(),
            typ: "description".into(),
            region: String::new(),
            langue: "de".into(),
            text: "Zeile 1\nZeile 2".into(),
            source: "Breadbin".into(),
        };
        let (fields, file) = p.form();
        let body = String::from_utf8(multipart(&fields, file, "XX")).unwrap();
        assert!(body.starts_with(
            "--XX\r\nContent-Disposition: form-data; name=\"gameid\"\r\n\r\n36525\r\n"
        ));
        assert!(body.contains("name=\"modiftexte\"\r\n\r\nZeile 1\nZeile 2\r\n"));
        assert!(body.ends_with("--XX--\r\n"));

        let m = Proposal::Media {
            game_id: "1".into(),
            typ: "box-2D".into(),
            region: "eu".into(),
            data: vec![1, 2, 3],
            file_name: "own_ab \"x.png".into(),
            source: String::new(),
        };
        let (fields, file) = m.form();
        let body = multipart(&fields, file, "XX");
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("name=\"modiftypenumsupport\"\r\n\r\n0\r\n"),
            "Verpackung braucht Datenträgernummer"
        );
        assert!(text.contains(
            "filename=\"own_abx.png\"\r\nContent-Type: image/png\r\n\r\n\u{1}\u{2}\u{3}\r\n"
        ));
        assert_eq!(
            plain_text("<html><body>Proposition <b>OK</b>\n</body></html>"),
            "Proposition OK"
        );
    }

    #[test]
    fn kommas_nur_ausserhalb_von_texten_entfernen() {
        assert_eq!(
            strip_trailing_commas(r#"{"a": [1, 2,], "b": "x,}",}"#),
            r#"{"a": [1, 2], "b": "x,}"}"#
        );
        // Textantworten statt JSON (Status 200) einordnen.
        assert!(matches!(
            check_status(200, "Le nombre maximum de threads est atteint"),
            Err(ApiError::Busy(_))
        ));
        assert!(matches!(check_status(200, ""), Err(ApiError::Busy(_))));
        assert!(matches!(
            check_status(
                200,
                "Faites du tri dans vos fichier roms et repassez demain !"
            ),
            Err(ApiError::Quota)
        ));
        assert!(matches!(
            check_status(200, "<html><body>API totalement fermée</body></html>"),
            Err(ApiError::Closed(_))
        ));
        assert!(
            matches!(check_status(200, "Irgendwas"), Err(ApiError::Http(200, t)) if t == "Irgendwas")
        );
        assert!(check_status(200, " {\"response\": {}}").is_ok());
        assert!(format!("{:#}", parse_json("<p>Kaputt</p>").unwrap_err()).contains("Kaputt"));
        // Treffer prüfen: passt eine gelieferte Prüfsumme zur Datei?
        let q = RomQuery {
            file_name: "warlock.prg".into(),
            title: "warlock".into(),
            size: 1,
            crc32: "0a1b2c3d".into(),
            md5: "ab".repeat(16),
            sha1: "cd".repeat(20),
        };
        let jeu = |rom: Value| serde_json::json!({ "rom": rom });
        assert!(rom_matches(
            &jeu(serde_json::json!({ "romsha1": "CD".repeat(20) })),
            &q
        ));
        assert!(
            rom_matches(&jeu(serde_json::json!({ "romcrc": "A1B2C3D" })), &q),
            "CRC ohne führende Null"
        );
        assert!(rom_matches(
            &serde_json::json!({ "roms": [{ "romcrc": "1" }, { "rommd5": "AB".repeat(16) }] }),
            &q
        ));
        assert!(
            !rom_matches(
                &jeu(serde_json::json!({ "romsha1": "EE".repeat(20), "romcrc": "12345678" })),
                &q
            ),
            "andere Datei"
        );
        assert!(
            !rom_matches(&serde_json::json!({ "noms": [] }), &q),
            "ohne Prüfsummen"
        );
        // Rohe Steuerzeichen in Texten (so von ScreenScraper geliefert), dazu ein abschließendes Komma.
        let raw = "{\n  \"synopsis\": \"Zeile 1\nZeile 2\tTab \u{1}x \\\"zitiert\\\"\",\n}";
        let v = parse_json(raw).unwrap();
        assert_eq!(v["synopsis"], "Zeile 1\nZeile 2\tTab \u{1}x \"zitiert\"");
    }

    #[test]
    fn fehlercodes() {
        assert_eq!(check_status(200, "{}"), Ok(()));
        assert_eq!(
            check_status(403, ""),
            Err(ApiError::DevLogin(String::new()))
        );
        let dev = "Erreur de login : Vérifier vos identifiants développeur !";
        assert_eq!(check_status(200, dev), Err(ApiError::DevLogin(dev.into())));
        assert_eq!(
            check_status(
                403,
                "Erreur de login : Vérifier vos identifiants utilisateur !"
            ),
            Err(ApiError::UserLogin)
        );
        assert_eq!(check_status(430, "quota"), Err(ApiError::Quota));
        assert!(matches!(
            check_status(429, "threads"),
            Err(ApiError::Busy(_))
        ));
        assert!(matches!(
            check_status(423, "fermé"),
            Err(ApiError::Closed(_))
        ));
        assert!(matches!(
            check_status(500, "x"),
            Err(ApiError::Http(500, _))
        ));
    }

    /// Echte Anfrage mit Platzhalterwerten: ScreenScraper meldet „identifiants utilisateurs“,
    /// die App muss daraus die abgelehnte Entwickler-Kennung machen. `cargo test -- --ignored`
    #[test]
    fn auswahllisten() {
        let genres: Value = serde_json::from_str(
            r#"{"serveurs": {"cpu1": "10"}, "genres": [
                {"id": 7, "nom_de": "Plattform", "nom_fr": "Plateforme", "parent": "0"},
                {"id": "2913", "nom_de": "Plattform / Jump 'n' Run Scrolling", "nom_fr": "Plateforme / Course Scrolling", "parent": "7"},
                {"id": "x"}]}"#,
        )
        .unwrap();
        let g = parse_choices(&genres);
        assert_eq!(g.len(), 2);
        assert_eq!(
            (
                g[0].id.as_str(),
                g[0].parent.as_str(),
                g[0].names["fr"].as_str()
            ),
            ("7", "", "Plateforme")
        );
        assert_eq!(g[1].parent, "7");
        let players: Value = serde_json::from_str(
            r#"{"ssuser": {"id": "x"}, "nbjoueurs": {"1": {"id": "1", "nom": "1", "parent": "0"}, "5": {"id": 5, "nom": "1-2", "parent": 0}}}"#,
        )
        .unwrap();
        let p = parse_choices(&players);
        assert_eq!(
            p.iter().map(|c| c.names[""].as_str()).collect::<Vec<_>>(),
            ["1", "1-2"]
        );
    }

    #[test]
    #[ignore = "braucht Netz"]
    fn falsche_entwicklerkennung_wird_erkannt() {
        let creds = Credentials {
            dev_id: "breadbin-test".into(),
            dev_password: "x".into(),
            user: "breadbin-test".into(),
            password: "x".into(),
        };
        let err = Client::new(creds, "Breadbin-test").user_info().unwrap_err();
        assert!(
            matches!(err.downcast_ref::<ApiError>(), Some(ApiError::DevLogin(_))),
            "{err}"
        );
    }
}
