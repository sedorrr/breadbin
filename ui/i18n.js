// Sprache der Oberfläche: Deutsch oder Englisch, Standard nach Systemsprache.
//
// Texte stehen im Quelltext auf Deutsch; `t()` liefert die englische Fassung aus `EN`,
// wenn Englisch gewählt ist. `{name}` in einem Text wird durch `vars.name` ersetzt.
// Feste Texte in index.html übersetzt `translateDom()` beim Start (Textknoten, title,
// placeholder). Fehlt eine Übersetzung, bleibt der deutsche Text stehen.
"use strict";

const UI_LANGS = ["auto", "de", "en"];

function storedUiLang() {
  try {
    const v = localStorage.getItem("uiLang");
    return UI_LANGS.includes(v) ? v : "auto";
  } catch {
    return "auto";
  }
}

const LANG = (() => {
  const v = storedUiLang();
  if (v !== "auto") return v;
  const sys = (navigator.languages?.[0] ?? navigator.language ?? "en").toLowerCase();
  return sys.startsWith("de") ? "de" : "en";
})();
const LOCALE = LANG === "de" ? "de-DE" : "en-GB";

const EN = {
  // ------------------------------------------------ index.html
  "Teilen": "Share",
  "Selbst geschriebene Angaben und eigene Bilder als Datei weitergeben oder die Datei eines anderen einlesen. Von ScreenScraper Übernommenes bleibt draußen.":
    "Pass on the details you wrote yourself and your own pictures as a file, or read someone else's file. Anything taken from ScreenScraper is left out.",
  "Selbst geschriebene Angaben und eigene Bilder als Austauschdatei speichern":
    "Save the details you wrote yourself and your own pictures as an exchange file",
  "Austauschdatei speichern …": "Save exchange file …",
  "Austauschdatei eines anderen einlesen; eigene Angaben bleiben unangetastet":
    "Read someone else's exchange file; your own details stay untouched",
  "Austauschdatei einlesen …": "Read exchange file …",
  "Quellen": "Sources",
  "Ordner oder Netzlaufwerk hinzufügen": "Add a folder or network drive",
  "+ Ordner": "+ Folder",
  "Alle scannen": "Scan all",
  "Ultimate-Geräte": "Ultimate devices",
  "Ultimate 64, C64 Ultimate oder 1541 Ultimate II+ hinzufügen": "Add an Ultimate 64, C64 Ultimate or 1541 Ultimate II+",
  "+ Gerät": "+ Device",
  "Beschreibungen": "Descriptions",
  "Zugang zu ScreenScraper und Sprache einrichten": "Set up ScreenScraper access and app language",
  "Einstellungen": "Settings",
  "Für Trefferliste holen": "Fetch for result list",
  "Alle ohne Beschreibung holen": "Fetch all without description",
  "Sprache der Oberfläche": "App language",
  "Automatisch (Systemsprache)": "Automatic (system language)",
  "Wird nach dem Speichern sofort übernommen.": "Takes effect as soon as you save.",
  "Zusätzliche Beschreibungen, Genres und Bilder von ScreenScraper.fr. Dafür brauchst du ein kostenloses Benutzerkonto:":
    "Additional descriptions, genres and pictures from ScreenScraper.fr. You need a free account for that:",
  "dort registrieren": "sign up there",
  ". Das Passwort wird im Schlüsselbund deines Systems gespeichert.": ". The password is stored in your system's keychain.",
  "Benutzername": "User name",
  "Passwort": "Password",
  "Passwort anzeigen": "Show password",
  "Sprache der Beschreibungen": "Language of descriptions",
  "Deutsch": "German",
  "Englisch": "English",
  "Französisch": "French",
  "Spanisch": "Spanish",
  "Italienisch": "Italian",
  "Ohne Treffer über die Prüfsumme auch über den Namen suchen (findet mehr, kann aber danebenliegen)":
    "If the checksum finds nothing, also search by name (finds more, but can be wrong)",
  "Entwickler-Kennung": "Developer credentials",
  "Wird für diese App einmal bei ScreenScraper beantragt. In fertigen Paketen ist sie schon enthalten; dann erscheinen diese Felder nicht.":
    "Requested once for this app from ScreenScraper. Release packages already contain them; then these fields are hidden.",
  "Programmname bei der Beantragung:": "Program name used in the request:",
  "Zugang prüfen": "Test access",
  "Schließen": "Close",
  "Speichern": "Save",
  "Nur für die Trefferliste": "Only for the current list",
  "Einlegen": "Insert",
  "Anleitung": "Guide",
  "Über Breadbin": "About Breadbin",
  "Datenordner zeigen": "Show data folder",
  "Breadbin erfasst C64-Spiele aus Ordnern, Netzlaufwerken und von Ultimate-Geräten, zeigt, was wo liegt, holt Beschreibungen und Bilder und startet Spiele auf einer Ultimate 64, einem C64 Ultimate oder einem 1541 Ultimate II+. Der Name kommt vom Spitznamen des ersten C64-Gehäuses: „Brotkasten“.": "Breadbin catalogs C64 games from folders, network drives and Ultimate devices, shows what is stored where, fetches descriptions and pictures, and starts games on an Ultimate 64, a C64 Ultimate or a 1541 Ultimate II+. The name comes from the nickname of the original C64 case: the “breadbin”.",
  "Lizenz": "License",
  "– Projektseite, Neuigkeiten und weitere C64-Projekte": "– project page, news and more C64 projects",
  "Freie Software unter der": "Free software under the",
  "oder neuer. Kostenlos, ohne Werbung, ohne Datensammlung.": "or later. Free of charge, no ads, no data collection.",
  "Daten und Dank": "Data and thanks",
  "Beschreibungen und Bilder:": "Descriptions and pictures:",
  "(Creative Commons BY-NC-SA 4.0) – mit dem Konto des Nutzers abgerufen.": "(Creative Commons BY-NC-SA 4.0) – fetched with the user's own account.",
  "Links zu": "Links to",
  "und": "and",
  "– es werden keine Daten von dort übernommen.": "– no data is taken from there.",
  "Gebaut mit": "Built with",
  "und Rust; Datenbank SQLite (rusqlite), dazu ureq, zip, image, keyring und weitere freie Bibliotheken.": "and Rust; SQLite database (rusqlite), plus ureq, zip, image, keyring and other free libraries.",
  "Ultimate 64, C64 Ultimate und 1541 Ultimate II+ sind Produkte von Gideon's Logic; „Commodore 64“ ist eine Marke ihrer jeweiligen Inhaber. Breadbin ist mit keinem davon verbunden.": "Ultimate 64, C64 Ultimate and 1541 Ultimate II+ are products of Gideon's Logic; “Commodore 64” is a trademark of its respective owners. Breadbin is not affiliated with any of them.",
  "Disketten": "Disks",
  "Disk {s}": "Disk {s}",
  "Seite {s}": "Side {s}",
  "Diskettenreihe: {n} Disks bzw. Seiten": "Disk set: {n} disks or sides",
  "Ab dieser Disk starten – beendet, was gerade auf dem C64 läuft": "Launch from this disk – ends whatever is running on the C64",
  "In Laufwerk A einlegen, ohne Neustart – für den Diskwechsel im laufenden Spiel": "Insert into drive A without restarting – for swapping disks in a running game",
  "Starten beendet, was gerade auf dem C64 läuft, und startet dieses Spiel; Einlegen wechselt nur die Diskette in Laufwerk A.": "Launch ends whatever is running on the C64 and starts this game; Insert only swaps the disk in drive A.",
  "Lege „{title}“ in {name} ein …": "Inserting “{title}” into {name} …",
  "Rechner während Scan und Abruf nicht einschlafen lassen (Bildschirm darf aus)": "Keep the computer awake during scans and fetches (the display may turn off)",
  "Alle Einträge ohne Beschreibung bei ScreenScraper nachschlagen": "Look up all entries without a description on ScreenScraper",
  "{ok} mit Beschreibung · {nf} nicht gefunden · {open} offen": "{ok} with description · {nf} not found · {open} open",
  "Läuft: {done} von {total} – {found} gefunden": "Running: {done} of {total} – {found} found",
  "Suchen: Titel, Diskname, Datei im Verzeichnis …": "Search: title, disk name, file in directory …",
  "Alle Formate": "All formats",
  "Ansicht": "View",
  "Alle": "All",
  "★ Favoriten": "★ Favourites",
  "Zuletzt gestartet": "Recently launched",
  "Selbst geschrieben (teilbar)":
    "Written by you (shareable)",
  "Spiele mit selbst geschriebenen Angaben – sie kommen beim Teilen in die Austauschdatei":
    "Games with details you wrote yourself – they go into the exchange file when you share",
  "Nur Einträge aus dieser Quelle": "Only entries from this source",
  "Alle Quellen": "All sources",
  "mit Beschreibung": "with description",
  "mehrfach vorhanden": "duplicates",
  "Verschiedene Dateien desselben Spiels (D64, CRT, Cracks …) als eine Zeile zeigen – über die ScreenScraper-Spielnummer und den bereinigten Titel":
    "Show different files of the same game (D64, CRT, cracks …) as one row – matched by ScreenScraper game ID and cleaned-up title",
  "zusammenfassen": "group",
  "Alle Treffer der aktuellen Suche als CSV-Datei speichern (z. B. für Excel)": "Save all results of the current search as a CSV file (e.g. for Excel)",
  "Exportieren": "Export",
  "Favorit": "Favourite",
  "Titel": "Title",
  "Jahr": "Year",
  "Herausgeber": "Publisher",
  "Genre": "Genre",
  "Format": "Format",
  "Inhalt": "Contents",
  "Anzahl Speicherorte": "Number of locations",
  "Orte": "Places",
  "Bereit": "Ready",
  "Fehler anzeigen": "Show errors",
  "Abbrechen": "Cancel",
  "Ultimate-Gerät": "Ultimate device",
  "Ultimate 64, C64 Ultimate oder 1541 Ultimate II+ im Netzwerk. Nötig sind FTP und die REST-Schnittstelle („Web Remote Control Service“, ab Firmware 3.11).":
    "Ultimate 64, C64 Ultimate or 1541 Ultimate II+ on the network. FTP and the REST interface (“Web Remote Control Service”, firmware 3.11 or later) are required.",
  "Im Netz suchen": "Search network",
  "Adresse (IP oder Name)": "Address (IP or name)",
  "Netzwerk-Passwort (nur falls am Gerät gesetzt)": "Network password (only if set on the device)",
  "Verbinden und prüfen": "Connect and check",
  "Name": "Name",
  "Startordner auf dem Gerät": "Launch folder on the device",
  "Spiele, die nicht auf dem Gerät liegen, werden zum Starten dorthin kopiert.": "Games that are not on the device are copied there to launch them.",
  "Ordner auf dem Gerät als Quelle": "Device folder as a source",
  "Ordner": "Folder",
  "/USB0/Spiele": "/USB0/Games",
  "Der erste Scan lädt jede Datei einmal über das Netz (etwa 300 KB/s) – bei vollen Sticks dauert das Stunden. Spätere Scans lesen nur Geändertes.":
    "The first scan downloads every file once over the network (about 300 KB/s) – a full stick takes hours. Later scans only read what changed.",
  "Schnellscan: Dateien, die mit gleichem Namen und gleicher Größe schon in einer anderen Quelle liegen, von dort übernehmen statt herunterladen. Viel schneller, kann aber bei gleich großen, verschiedenen Dateien danebenliegen (etwa ein D64 mit Spielstand). Solche Einträge sind gekennzeichnet.":
    "Quick scan: take files that already exist with the same name and size in another source from there instead of downloading them. Much faster, but can be wrong for different files of the same size (e.g. a D64 with a saved game). Such entries are marked.",
  "Hinzufügen und scannen": "Add and scan",
  "Ziehen: Breite ändern · Doppelklick: zurücksetzen": "Drag: change width · Double-click: reset",

  // ------------------------------------------------ app.js
  "noch nie": "never",
  "„{name}“": "“{name}”",
  "{n} Fehler anzeigen": "Show {n} errors",
  "Fehler: {e}": "Error: {e}",
  "Fehler": "Error",
  "Noch keine Quellen. Füge einen Ordner mit Spielen hinzu – auch ein eingebundenes Netzlaufwerk.":
    "No sources yet. Add a folder with games – a mounted network drive works too.",
  "Gerät": "Device",
  "{n} Dateien · Scan: {date}": "{n} files · scanned: {date}",
  "Scannen": "Scan",
  "Während eines Scans gesperrt – unten in der Statusleiste lässt er sich abbrechen.": "Locked while a scan is running – you can cancel it in the status bar below.",
  "Wird gescannt … {read} neu{quick}, {unchanged} unverändert": "Scanning … {read} new{quick}, {unchanged} unchanged",
  "Ausschalten: Der nächste Scan lädt übernommene Dateien doch noch herunter und prüft sie.":
    "Turn off: the next scan downloads and checks the files taken over from other sources.",
  "Einschalten: Dateien mit gleichem Namen und gleicher Größe wie in anderen Quellen werden übernommen statt heruntergeladen.":
    "Turn on: files with the same name and size as in other sources are taken over instead of downloaded.",
  "Schnellscan: an": "Quick scan: on",
  "Schnellscan: aus": "Quick scan: off",
  "Entfernen": "Remove",
  "Bestand": "Collection",
  "Dateien": "Files",
  "Verschiedene Inhalte": "Distinct contents",
  "Ordner mit C64-Spielen wählen": "Choose a folder with C64 games",
  "„{label}“ aus der Datenbank entfernen? Die Dateien selbst bleiben unverändert.":
    "Remove “{label}” from the database? The files themselves are not touched.",
  "Quelle entfernen": "Remove source",
  "Scan startet …": "Starting scan …",
  ", {n} übernommen": ", {n} taken over",
  "Scanne {source}: {read} neu{quick}, {unchanged} unverändert – {current}": "Scanning {source}: {read} new{quick}, {unchanged} unchanged – {current}",
  "{n} neu gelesen": "{n} newly read",
  "{n} unverändert": "{n} unchanged",
  "{n} per Schnellscan übernommen": "{n} taken over by quick scan",
  "{n} entfernt": "{n} removed",
  "{n} Fehler": "{n} errors",
  "abgebrochen": "cancelled",
  "fertig": "done",
  "angehalten": "stopped",
  "{source}: {end} – {parts}": "{source}: {end} – {parts}",
  "Fehler beim Scannen von {source}": "Errors while scanning {source}",
  "Liste exportieren": "Export list",
  "{n} Einträge nach {path} exportiert.": "Exported {n} entries to {path}.",
  "mehr als {n} Treffer – Suche verfeinern": "more than {n} results – refine your search",
  "{n} Treffer": "{n} results",
  "Beschreibung vorhanden": "Description available",
  "{n} Fassungen": "{n} versions",
  "Keine Treffer.": "No results.",
  "Füge links einen Ordner hinzu, um deine Spiele zu erfassen.": "Add a folder on the left to catalogue your games.",
  "Verzeichnis": "Directory",
  "Größe": "Size",
  "Modulname": "Cartridge name",
  "Name im Abbild": "Image name",
  "Technik": "Technical",
  "Entwickler": "Developer",
  "Spieler": "Players",
  "Bewertung": "Rating",
  "{n} von 20": "{n} of 20",
  "Bei ScreenScraper nachschlagen": "Look up on ScreenScraper",
  "Bei ScreenScraper nicht gefunden ({date}).": "Not found on ScreenScraper ({date}).",
  "Erneut versuchen": "Try again",
  "Über den Namen zugeordnet – bitte prüfen, ob es das richtige Spiel ist.": "Matched by name – please check that it is the right game.",
  "Zuletzt gestartet: {date}": "Last launched: {date}",
  "1 Ort": "1 place",
  "{n} Orte": "{n} places",
  "Gespeichert an 1 Ort": "Stored in 1 place",
  "Gespeichert an {n} Orten": "Stored in {n} places",
  "Gleicher Name und gleiche Größe wie in einer anderen Quelle; der Inhalt wurde nicht heruntergeladen und geprüft.":
    "Same name and size as in another source; the contents were not downloaded and checked.",
  "Per Schnellscan übernommen, nicht geprüft": "Taken over by quick scan, not checked",
  "Im Ordner zeigen": "Show in folder",
  "Prüfsummen": "Checksums",
  "Angaben neu laden": "Reload details",
  "Anderes Spiel wählen": "Choose another game",
  "Spiel von Hand suchen": "Find game manually",
  "Zuordnung als falsch markiert ({date}).": "Match marked as wrong ({date}).",
  "Zurücknehmen": "Undo",
  "Fehler bei ScreenScraper melden": "Report error on ScreenScraper",
  "Von Hand zugeordnet.": "Matched manually.",
  "Falsches Spiel": "Wrong game",
  "Aus der Gruppe gelöst.": "Separated from its group.",
  "Wieder zusammenfassen": "Group again",
  "Beschreibung und Bilder von {n} Fassungen verwerfen? Spätere Abrufe holen sie nicht zurück.": "Discard description and pictures of {n} versions? Later fetches will not bring them back.",
  "Beschreibung und Bilder verwerfen? Spätere Abrufe holen sie nicht zurück.": "Discard description and pictures? Later fetches will not bring them back.",
  "Verwerfen": "Discard",
  "Lade Angaben zu Spiel {id} …": "Loading details for game {id} …",
  "Zugeordnet.": "Matched.",
  "Auch für die andere Fassung mit derselben Zuordnung": "Also for the other version with the same match",
  "Auch für die anderen {n} Fassungen mit derselben Zuordnung": "Also for the other {n} versions with the same match",
  "Nur diese Fassung": "Only this version",
  "Bildschirmfoto": "Screenshot",
  "Alle {n} Spiele mit selbst geschriebenen Angaben werden eingesandt.": "All {n} games with details you wrote yourself will be submitted.",
  "{n} neue oder geänderte Spiele werden eingesandt (von {all} mit selbst geschriebenen Angaben).": "{n} new or changed games will be submitted (of {all} with details you wrote yourself).",
  "Nichts Neues: Alle {all} Spiele mit selbst geschriebenen Angaben sind schon in dieser Fassung eingesandt.": "Nothing new: all {all} games with details you wrote yourself have already been submitted in this form.",
  "Auch schon eingesandte Spiele noch einmal senden": "Also send games that were already submitted",
  "Gehört eine Fassung nicht dazu, mit „Lösen“ abtrennen – sie steht dann allein in der Liste.": "If a version does not belong here, “Separate” it – it then gets its own row in the list.",
  "Diese Fassung ist ein anderes Spiel: aus der Gruppe lösen und allein anzeigen": "This version is a different game: separate it from the group and show it on its own",
  "Lösen": "Separate",
  "Fassung aus der Gruppe gelöst.": "Version separated from the group.",
  "In deiner Bibliothek": "In your library",
  "selbst geschrieben": "written yourself",
  "selbst bearbeitet": "edited yourself",
  "Gemeinschaft": "community",
  "Angaben von „{title}“ übernommen.": "Took over the details of “{title}”.",
  "Name (in deiner Bibliothek und bei ScreenScraper) oder Spielnummer bei ScreenScraper": "Name (in your library and on ScreenScraper) or game number on ScreenScraper",
  "Spielstand holen …": "Fetch saved game …",
  "Hat das Spiel auf seine Diskette im Startordner gespeichert, diese Diskette mit Spielstand auf den Rechner holen": "If the game saved to its disk in the launch folder, fetch that disk with the saved game to this computer",
  "Kein neuer Spielstand zu diesem Spiel im Startordner von {name} (oder schon geholt).": "No new saved game for this game in the launch folder of {name} (or already fetched).",
  "Alle Spielstände eines Geräts: ⋯ in der Gerätezeile → „Spielstände zurückholen …“.": "All saved games of a device: ⋯ in the device row → “Fetch saved games …”.",
  "Als Spielszene": "Use as in-game",
  "Als Titelbild": "Use as title screen",
  "Liegt dort schon ein Bild, tauschen beide den Platz.": "If there is already a picture there, the two swap places.",
  "Bild vom Bildschirm des Geräts als eigenes Titelbild oder eigene Spielszene speichern – nur mit Netzwerkkabel am Gerät": "Save what the device shows as your own title screen or in-game picture – only with a network cable on the device",
  "{name} liefert kein Bild – Bildschirmfotos gehen mit Ultimate 64 und C64 Ultimate.": "{name} provides no picture – screenshots work with Ultimate 64 and C64 Ultimate.",
  "Was gerade auf {name} zu sehen ist, als eigenes Bild speichern – als was?": "Save what {name} is showing as your own picture – as what?",
  "Die Ultimate schickt ihr Bild dafür kurz als Videostrom – nur über Ethernet, das Gerät muss per Netzwerkkabel verbunden sein. Andere Empfänger (etwa eine Jukebox) verlieren den Strom dabei.": "The Ultimate briefly sends its picture as a video stream – over Ethernet only, so the device needs a network cable. Other receivers (such as a jukebox) lose the stream meanwhile.",
  "Nehme das Bild von {name} auf …": "Capturing the picture from {name} …",
  "Bildschirmfoto gespeichert.": "Screenshot saved.",
  "Was gerade auf dem Gerät zu sehen ist, als dieses Bild speichern (320 × 200) – nur mit Netzwerkkabel am Gerät": "Save what the device is showing as this picture (320 × 200) – only with a network cable on the device",
  "Bildschirmfoto von": "Screenshot from",
  "lade …": "loading …",
  "Genres filtern …": "Filter genres …",
  "Jedes angehakte Genre wird einzeln vorgeschlagen.": "Each ticked genre is proposed separately.",
  "Nicht in der Liste von ScreenScraper: {list}": "Not in ScreenScraper's list: {list}",
  "Alle {n} Fassungen": "All {n} versions",
  "{n} Fassungen in dieser Gruppe haben dieselbe Zuordnung.": "{n} versions in this group have the same match.",
  "Suche bei ScreenScraper …": "Searching ScreenScraper …",
  "Keine Treffer. Anders schreiben oder die Spielnummer von screenscraper.fr eingeben.": "No results. Try another spelling or enter the game number from screenscraper.fr.",
  "Richtiges Spiel wählen": "Choose the right game",
  "Name oder Spielnummer bei ScreenScraper": "Name or ScreenScraper game number",
  "Suchen": "Search",
  "gleiche Spiele zusammenfassen": "group same games",
  "Einstellungen: Sprache, ScreenScraper-Zugang": "Settings: language, ScreenScraper account",
  "Alle Quellen nacheinander scannen": "Scan all sources one after another",
  "Zugang": "Account",
  "Zugang zu ScreenScraper einrichten": "Set up ScreenScraper account",
  "Beschreibungen und Bilder für alle Einträge der aktuellen Trefferliste holen": "Fetch descriptions and pictures for all entries in the current list",
  "Alle Einträge ohne Beschreibung nachschlagen": "Look up all entries without a description",
  "Schnellscan an": "Quick scan on",
  "Schnell": "Quick",
  "Schnellscan ausschalten": "Turn quick scan off",
  "Schnellscan einschalten": "Turn quick scan on",
  "Im Netz suchen:": "Search the web:",
  "Angaben bearbeiten": "Edit details",
  "Angaben selbst eintragen": "Enter details yourself",
  "Selbst geschrieben – wird beim Teilen mitgegeben.":
    "Written by you – included when you share.",
  "Titelbild": "Title screen",
  "Spielszene": "In-game",
  "Verpackung": "Box",
  "Gilt für alle {n} Fassungen.": "Applies to all {n} versions.",
  "kein Bild": "no picture",
  "Wählen …": "Choose …",
  "Bild wählen": "Choose picture",
  "Bilder": "Images",
  "gespeichert": "saved",
  "Eigene Angaben und Bilder löschen?": "Delete your details and pictures?",
  "Eigene Angaben": "Your details",
  "Löschen": "Delete",
  "Für Spiele, die ScreenScraper nicht kennt – etwa eigene Entwicklungen. Kein Abruf überschreibt sie.": "For games ScreenScraper doesn't know, such as your own productions. No fetch overwrites them.",
  "Beschreibung": "Description",
  "Eigene Angaben löschen": "Delete your details",
  "Angaben löschen": "Delete details",
  "Bei ScreenScraper vorschlagen": "Propose to ScreenScraper",
  "Mit anderen teilen":
    "Share with others",
  "Erscheinungsdatum": "Release date",
  "Vorschlag an ScreenScraper – Spiel #{id}": "Proposal to ScreenScraper – game #{id}",
  "Vorschlag an ScreenScraper": "Proposal to ScreenScraper",
  "Bei ScreenScraper: lade …": "On ScreenScraper: loading …",
  "Bei ScreenScraper: {v}": "On ScreenScraper: {v}",
  "Bei ScreenScraper: (leer)": "On ScreenScraper: (empty)",
  "eigenes Bild": "your picture",
  "ScreenScraper hat schon ein Bild dieser Art.": "ScreenScraper already has a picture of this kind.",
  "ScreenScraper hat noch kein Bild dieser Art.": "ScreenScraper has no picture of this kind yet.",
  "Bisher vorgeschlagen": "Proposed so far",
  "Nichts angehakt.": "Nothing ticked.",
  "{n} Vorschläge mit deinem ScreenScraper-Konto senden? Die Moderatoren prüfen sie, bevor sie erscheinen.": "Send {n} proposals with your ScreenScraper account? Moderators review them before they appear.",
  "Senden": "Send",
  "Senden …": "Send …",
  "Sende …": "Sending …",
  "gesendet": "sent",
  "System": "System",
  "Datei": "File",
  "Kopiert.": "Copied.",
  "Markiert – mit Cmd/Strg+C kopieren.": "Selected – copy with Cmd/Ctrl+C.",
  "Gesendet wird mit deinem ScreenScraper-Konto; die Moderatoren prüfen jeden Vorschlag, bevor er erscheint. Angehakt ist, was dort fehlt oder abweicht.": "Sent with your ScreenScraper account; moderators review every proposal before it appears. Ticked are the fields that are missing or different there.",
  "Region (für Name, Datum, Bilder)": "Region (for name, date, pictures)",
  "Welt": "World",
  "Europa": "Europe",
  "USA": "USA",
  "Deutschland": "Germany",
  "Großbritannien": "United Kingdom",
  "Frankreich": "France",
  "Japan": "Japan",
  "Sprache (für die Beschreibung)": "Language (for the description)",
  "Quelle (optional: Webseite, Anleitung, eigene Arbeit …)": "Source (optional: website, manual, own work …)",
  "ScreenScraper kennt diese Datei noch nicht. Ein Formular für neue Spiele gibt es dort nur für besonders vertraute Mitwirkende. Unbekannte Dateien nimmt ScreenScraper von selbst auf, sobald mehrere Mitglieder sie abfragen (Breadbin fragt beim Abruf mit); Mitwirkende ordnen sie dann einem Spiel zu. Nachhelfen kannst du im Forum von ScreenScraper – mit diesen Angaben:": "ScreenScraper doesn't know this file yet. Only especially trusted contributors get a form for new games there. ScreenScraper adds unknown files by itself once several members look them up (Breadbin does so when fetching); contributors then match them to a game. You can give it a nudge in the ScreenScraper forum – with these details:",
  "Kennt ScreenScraper das Spiel schon, nur diese Datei nicht: erst „Spiel von Hand suchen“, danach lassen sich Angaben und Bilder direkt aus Breadbin vorschlagen. Alle selbst eingetragenen, bei ScreenScraper unbekannten Spiele zeigt die Ansicht „Bei ScreenScraper unbekannt“.": "If ScreenScraper knows the game but not this file: use “Find game manually” first; then details and pictures can be proposed straight from Breadbin. The view “Unknown to ScreenScraper” lists all games you entered yourself that ScreenScraper does not know yet.",
  "Kopieren": "Copy",
  "Forum öffnen": "Open forum",
  "Angaben löschen ({n})": "Delete details ({n})",
  "Beschreibung und Bilder von {n} Fassungen löschen?": "Delete description and pictures of {n} versions?",
  "Beschreibung und Bilder löschen?": "Delete description and pictures?",
  "Die Einträge gelten danach als nicht nachgeschlagen; ein späterer Abruf kann sie neu holen. Damit das nicht passiert: „Falsches Spiel“.": "The entries then count as not looked up; a later fetch may bring the details back. To prevent that, use “Wrong game”.",
  "{n} Einträge markiert": "{n} entries selected",
  "{n} Fassungen insgesamt": "{n} versions in total",
  "Gemeinsam einem Spiel zuordnen": "Match all to one game",
  "Bei ScreenScraper nachschlagen ({n} ohne Beschreibung)": "Look up on ScreenScraper ({n} without description)",
  "Falsches Spiel ({n})": "Wrong game ({n})",
  "Markierung aufheben": "Clear selection",
  "Cmd- bzw. Strg-Klick markiert einzelne Zeilen, Shift-Klick einen Bereich.": "Cmd- or Ctrl-click selects single rows, Shift-click a range.",
  "Gilt für alle {n} markierten Fassungen.": "Applies to all {n} selected versions.",
  "Beschreibung und Bilder: ScreenScraper.fr (Stand {date})": "Description and pictures: ScreenScraper.fr (as of {date})",
  "Angemeldet als {user}": "Signed in as {user}",
  "Entwickler-Kennung fehlt – siehe Einstellungen.": "Developer credentials missing – see Settings.",
  "Kein Zugang eingerichtet (freiwillig).":
    "No access set up (optional).",
  "gespeichert – nur zum Ändern ausfüllen": "saved – fill in only to change",
  "Prüfe …": "Checking …",
  "Zugang in Ordnung. Heute {today} von {max} Abfragen genutzt, {threads} gleichzeitig erlaubt.":
    "Access OK. {today} of {max} requests used today, {threads} at a time allowed.",
  "Abruf startet …": "Starting fetch …",
  "ScreenScraper: {done} von {total} – {found} gefunden, {notfound} nicht gefunden – {current}":
    "ScreenScraper: {done} of {total} – {found} found, {notfound} not found – {current}",
  "{n} gefunden": "{n} found",
  "{n} nicht gefunden": "{n} not found",
  " (nichts offen)": " (nothing left to fetch)",
  "ScreenScraper {end}: {parts}{rest}": "ScreenScraper {end}: {parts}{rest}",
  "Fehler beim Abruf von ScreenScraper": "Errors while fetching from ScreenScraper",
  "prüfe …": "checking …",
  "Startordner: {dir}": "Launch folder: {dir}",
  "Einschalten": "Power on",
  "Ausschalten": "Power off",
  "Ordner als Quelle": "Folder as source",
  "Weitere Aktionen": "More actions",
  "Mehr": "More",
  "Bearbeiten": "Edit",
  "Dateien löschen, die Breadbin zum Starten in den Startordner kopiert hat": "Delete the files Breadbin copied to the launch folder to start games",
  "Startordner leeren": "Empty launch folder",
  "Noch kein Gerät. Mit „+ Gerät“ eine Ultimate im Netzwerk hinzufügen, um Spiele dort zu starten.":
    "No device yet. Use “+ Device” to add an Ultimate on your network and launch games on it.",
  "an": "on",
  "aus": "off",
  "unbekannt": "unknown",
  "Schalte {name} ein …": "Powering on {name} …",
  "{name} ist nach {secs} s eingeschaltet.": "{name} is on after {secs} s.",
  "Gerät bearbeiten": "Edit device",
  "Gerät hinzufügen": "Add device",
  "Suche im Netz … (einige Sekunden)": "Searching the network … (a few seconds)",
  ", Firmware {fw}": ", firmware {fw}",
  "(schon eingetragen)": "(already added)",
  "(auch {hosts})": "(also {hosts})",
  "(schon eingetragen als {host})": "(already added as {host})",
  "Kein eingeschaltetes Gerät gefunden. Geräte mit Netzwerk-Passwort antworten der Suche nicht.":
    "No powered-on device found. Devices with a network password do not answer the search.",
  "Bitte die Adresse eintragen.": "Please enter the address.",
  "Verbinde …": "Connecting …",
  "keine": "none",
  "Gefunden: {product}{fw}. Laufwerke: {drives}.": "Found: {product}{fw}. Drives: {drives}.",
  "Schalte {name} aus …": "Powering off {name} …",
  "{name} ist ausgeschaltet.": "{name} is off.",
  "{name}: Befehl angenommen, Gerät antwortet aber noch.": "{name}: command accepted, but the device still answers.",
  "Im Startordner {dir} auf „{name}“ die Dateien löschen, die Breadbin dort zum Starten abgelegt hat? Andere Dateien bleiben erhalten.":
    "Delete the files Breadbin placed in the launch folder {dir} on “{name}”? Other files are kept.",
  "{name}: {n} Dateien gelöscht{kept}.": "{name}: {n} files deleted{kept}.",
  ", {n} fremde Dateien behalten": ", {n} other files kept",
  "Gerät „{name}“ entfernen? Ordner-Quellen auf dem Gerät bleiben erhalten.": "Remove device “{name}”? Folder sources on the device are kept.",
  "Gerät entfernen": "Remove device",
  "Lese Ordner …": "Reading folder …",
  "Keine Unterordner.": "No subfolders.",
  "Bitte einen Ordner auf einem Laufwerk wählen, z. B. /USB0/Spiele.": "Please choose a folder on a drive, e.g. /USB0/Games.",
  "Bandabbilder (TAP) lassen sich noch nicht starten.": "Tape images (TAP) cannot be launched yet.",
  "Starten": "Launch",
  "Starten beendet, was gerade auf dem C64 läuft, und startet dieses Spiel.": "Launch ends whatever is running on the C64 and starts this game.",
  "{name} ist aus und lässt sich nicht aus der App einschalten.": "{name} is off and cannot be powered on from the app.",
  "Starte „{title}“ auf {name} …": "Launching “{title}” on {name} …",
  " (vorher hinkopiert)": " (copied there first)",
  // ------------------------------------------------ Teilen
  "Es gibt noch nichts zu teilen: Geteilt werden selbst geschriebene Angaben (Ansicht „Selbst geschrieben (teilbar)“). Bei Spielen von ScreenScraper geht das mit „Eigene Fassung schreiben“.":
    "Nothing to share yet: sharing covers details you wrote yourself (view “Written by you (shareable)”). For games from ScreenScraper, use “Write your own version”.",
  "Austauschdatei speichern": "Save exchange file",
  "Breadbin-Austauschdatei": "Breadbin exchange file",
  "Austauschdatei wird geschrieben …": "Writing exchange file …",
  "{n} Spiele in {path} gespeichert.": "{n} games saved to {path}.",
  "Austauschdatei einlesen": "Read exchange file",
  "Austauschdatei wird eingelesen …": "Reading exchange file …",
  "{n} von {total} Spielen übernommen, davon {lib} in deiner Sammlung.": "{n} of {total} games taken over, {lib} of them in your collection.",
  "{n} übergangen: Dafür gibt es schon eigene Angaben (oder von ScreenScraper, wenn es die bevorzugte Quelle ist).":
    "{n} skipped: you already have your own details (or ScreenScraper's, if it is the preferred source).",
  "{n} fehlerhaft:": "{n} faulty:",
  "Austauschdatei eingelesen": "Exchange file read",
  "Aus der Gemeinschaft (Austauschdatei) übernommen.":
    "Taken over from the community (exchange file).",
  "Auf Grundlage von ScreenScraper bearbeitet – wird nicht geteilt. Teilbar wird es mit „Eigene Fassung schreiben“.":
    "Edited on the basis of ScreenScraper – not shared. Use “Write your own version” to make it shareable.",
  "Mit leerem Formular beginnen – selbst Geschriebenes kann geteilt werden, von ScreenScraper Übernommenes nicht":
    "Start with an empty form – what you write yourself can be shared, anything taken from ScreenScraper cannot",
  "Eigene Fassung schreiben": "Write your own version",
  "Bevorzugte Quelle für Angaben": "Preferred source for details",
  "Gemeinschaft (Austauschdateien)": "Community (exchange files)",
  "ScreenScraper (freiwillig)": "ScreenScraper (optional)",
  "Selbst eingetragene Angaben gehen immer vor. Die bevorzugte Quelle ersetzt beim Einlesen bzw. Abrufen die andere; die andere füllt nur Lücken.":
    "Details you entered yourself always come first. The preferred source replaces the other when reading or fetching; the other only fills gaps.",
  "Kontakt:": "Contact:",
  "von ScreenScraper – wird nicht geteilt": "from ScreenScraper – not shared",
  "Bilder teilweise: ScreenScraper.fr – sie werden nicht geteilt.": "Some pictures: ScreenScraper.fr – they are not shared.",
  " · {n} gleichzeitig": " · {n} at once",
  "Gleichzeitige Abfragen": "Simultaneous requests",
  "Automatisch (laut Konto, höchstens 4)": "Automatic (per account, at most 4)",
  "Diese Zeile fasst {n} Fassungen zusammen. Angaben und Bilder für alle übernehmen oder nur für die gezeigte?":
    "This row groups {n} versions. Apply details and pictures to all of them or only to the one shown?",
  "Gemeinschaftsdatenbank": "Community database",
  "Den aktuellen Stand der Gemeinschaftsdatenbank herunterladen und einlesen (Adresse in den Einstellungen)":
    "Download and read the current community database (address in the settings)",
  "Gemeinschaftsdaten aktualisieren": "Update community data",
  "Selbst geschriebene Angaben im Aufbau der Gemeinschaftsdatenbank in einen Ordner schreiben, z. B. in die Arbeitskopie eines Forks – dann per Pull Request einreichen":
    "Write the details you wrote yourself into a folder in the community database layout, e.g. the working copy of a fork – then submit them as a pull request",
  "Beitrag vorbereiten …": "Prepare contribution …",
  "Adresse der Gemeinschaftsdaten": "Address of the community data",
  "Gemeinschaftsdaten von": "Community data from",
  "Beiträge einsenden an": "Send contributions to",
  "Breadbin-Server (8bitcavern.de)": "Breadbin server (8bitcavern.de)",
  "Eigener Server …": "Own server …",
  "Nicht einsenden": "Do not submit",
  "Adresse für Beiträge": "Address for contributions",
  "Gemeinschaftsdaten werden geladen …": "Loading community data …",
  "Gemeinschaftsdaten aktualisiert": "Community data updated",
  "Ordner für den Beitrag wählen (z. B. Arbeitskopie des Forks)": "Choose a folder for the contribution (e.g. the working copy of your fork)",
  "{n} Spiele für den Beitrag nach {path} geschrieben.": "{n} games written to {path} for the contribution.",
  "{n} Spiele liegen jetzt in {path} (Ordner games/ und images/).\n\nNächste Schritte: Liegt der Ordner in deinem Fork der Gemeinschaftsdatenbank, die Änderungen committen und einen Pull Request stellen. Die Prüfung läuft dort automatisch.":
    "{n} games are now in {path} (folders games/ and images/).\n\nNext steps: if the folder is in your fork of the community database, commit the changes and open a pull request. The check runs there automatically.",
  "Beitrag vorbereitet": "Contribution prepared",
  "Beitrag einsenden": "Submit contribution",
  "Name für die Namensnennung (freiwillig)": "Name for attribution (optional)",
  "Hinweis an die Prüfenden (freiwillig)": "Note to the reviewers (optional)",
  "Mit dem Einsenden stimmst du zu, dass deine Angaben und Bilder unter CC BY-SA 4.0 veröffentlicht werden. Bitte nur Selbsterstelltes.":
    "By submitting you agree that your details and pictures are published under CC BY-SA 4.0. Please only send what you created yourself.",
  "Einsenden": "Submit",
  "Selbst geschriebene Angaben direkt an den Server der Gemeinschaftsdatenbank schicken; dort werden sie geprüft":
    "Send the details you wrote yourself directly to the community database server, where they are reviewed",
  "Beitrag einsenden …": "Submit contribution …",
  "Adresse für Beiträge (leer = Einsenden aus)": "Address for contributions (empty = submitting off)",
  "Wird eingesandt …": "Submitting …",
  "Eingegangen (Nr. {id}, {n} Spiele). Danke! Nach der Prüfung erscheint er in den Gemeinschaftsdaten.":
    "Received (no. {id}, {n} games). Thank you! After review it will appear in the community data.",
  "Abgelehnt.": "Rejected.",
  "Eingegangen in {k} Beiträgen (Nr. {ids}, zusammen {n} Spiele). Danke! Nach der Prüfung erscheinen sie in den Gemeinschaftsdaten.":
    "Received in {k} contributions (no. {ids}, {n} games in total). Thank you! After review they will appear in the community data.",
  "Gemeinschaftsdaten beim Start selbst aktualisieren (höchstens einmal pro Woche)":
    "Update community data automatically at start (at most once a week)",
  "Beim Start nach neuen Breadbin-Fassungen sehen (8bitcavern.de)": "Check for new Breadbin versions at start (8bitcavern.de)",
  "Nach dem Starten ein Titelbild aufnehmen, wenn das Spiel noch keines hat":
    "After launching, take a title picture if the game has none yet",
  "Wartezeit bis zur Aufnahme in Sekunden": "Delay before taking it, in seconds",
  "Nur Ultimate 64 und C64 Ultimate mit Netzwerkkabel. Das Bild wird als eigenes Titelbild gespeichert und lässt sich in den Details ersetzen.":
    "Ultimate 64 and C64 Ultimate with a network cable only. The picture is saved as your own title picture and can be replaced in the details.",
  "Titelbild von {name} aufgenommen – in den Details lässt es sich ersetzen.":
    "Title picture taken from {name} – you can replace it in the details.",
  "Kein automatisches Titelbild: {e}": "No automatic title picture: {e}",
  "Zufallsspiel": "Random game",
  "Beschreibung (Deutsch)": "Description (German)",
  "Beschreibung (Englisch)": "Description (English)",
  "Eine Sprache genügt; mit beiden lesen alle Breadbin-Nutzer den Text in ihrer Sprache.": "One language is enough; with both, every Breadbin user reads the text in their language.",
  "Beschreibung nur auf Deutsch vorhanden.": "Description only available in German.",
  "Beschreibung nur auf Englisch vorhanden.": "Description only available in English.",
  "im Web ansehen": "view on the web",
  "Statistik und Suche der Gemeinschaftsdaten im Browser öffnen": "Open statistics and search of the community data in the browser",
  "Ein zufälliges Spiel aus den aktuellen Treffern zeigen (Taste Z)": "Show a random game from the current results (key Z)",
  "Breadbin {v} ist da": "Breadbin {v} is available",
  "Zum Herunterladen": "Go to download",
  "Gemeinschaftsdaten selbst aktualisiert: {n} übernommen.": "Community data updated automatically: {n} taken over.",
  ", {n} Dateien behalten (fremde oder mit Spielstand)": ", {n} files kept (other files or with saved games)",
  "Alle Einträge haben Angaben.": "All entries have details.",
  "Beschädigt oder unbekannt aufgebaut": "Damaged or unknown layout",
  "Davon {n} mehrfach in derselben Quelle ({size}) – Kopien auf anderen Datenträgern wie einem Gerätestick sind oft gewollt.": "{n} of them several times within the same source ({size}) – copies on other media such as a device stick are often intended.",
  "Doppelte Dateien": "Duplicate files",
  "Doppelte Dateien speichern": "Save duplicate files",
  "Keine doppelten Dateien.": "No duplicate files.",
  "Keine.": "None.",
  "Liste mit allen Orten speichern …": "Save list with all locations …",
  "Ohne Angaben": "Without details",
  "Zeigen": "Show",
  "{n} Abbilder ließen sich beim Scan nicht lesen – oft unvollständige Downloads oder seltene Sonderformate.": "{n} images could not be read during the scan – often incomplete downloads or rare special formats.",
  "{n} Einträge haben keine Beschreibung: nicht erkannt oder noch nicht nachgeschlagen. Hier helfen Gemeinschaftsdaten, ScreenScraper oder eigene Angaben.": "{n} entries have no description: not recognised or not looked up yet. Community data, ScreenScraper or your own details help here.",
  "{n} Inhalte liegen an mehreren Orten ({files} Dateien); die zusätzlichen Kopien belegen {size}.": "{n} contents are stored in several places ({files} files); the extra copies take up {size}.",
  "Aufräumen": "Clean up",
  "Aufräumen …": "Clean up …",
  "Doppelte Dateien, beschädigte Abbilder und Einträge ohne Angaben im Überblick": "Overview of duplicate files, damaged images and entries without details",
  "Breadbin löscht nichts selbst. Die Übersicht führt zu Listen, die du durchgehen oder als CSV speichern kannst.": "Breadbin deletes nothing by itself. The overview leads to lists you can go through or save as CSV.",
  "Einträge ohne Beschreibung: nicht erkannt oder noch nicht nachgeschlagen": "Entries without a description: not recognised or not looked up yet",
  "Beschädigt / unbekannt": "Damaged / unknown",
  "Abbilder, die beim Scan als beschädigt oder unbekannt aufgebaut erkannt wurden": "Images found to be damaged or of unknown layout during the scan",
  "Anlegen": "Create",
  "Auf Gerät kopieren": "Copy to device",
  "Auf Gerät kopieren …": "Copy to device …",
  "Aus dieser Liste entfernen": "Remove from this list",
  "Auswahl": "Selection",
  "Bibliothek gesichert: {n} Bilder, {size}.": "Library backed up: {n} pictures, {size}.",
  "Die jetzige Bibliothek wird durch die Sicherung ersetzt (Quellen, Geräte, eigene Angaben, Listen, Notizen, Einstellungen). Die bisherige Datenbank bleibt im Datenordner als „spiele.vor-wiederherstellung.sqlite“ liegen. Passwörter liegen im Schlüsselbund und bleiben, wie sie sind.": "The current library will be replaced by the backup (sources, devices, your own details, lists, notes, settings). The previous database stays in the data folder as “spiele.vor-wiederherstellung.sqlite”. Passwords are kept in the keychain and stay as they are.",
  "Disketten, die sich im Startordner seit dem Hinkopieren geändert haben (Spielstände), auf den Rechner holen": "Copy disks that changed in the launch folder since they were copied there (saved games) to this computer",
  "Durchgespielt": "Completed",
  "Eigene Bewertung": "Your rating",
  "Eigene Bewertung: {n} von 5": "Your rating: {n} of 5",
  "Eigene Listen": "Your lists",
  "Firmware {v}": "firmware {v}",
  "Für mich": "For me",
  "Im Startordner {dir} auf „{name}“ die Dateien löschen, die Breadbin dort zum Starten abgelegt hat? Andere Dateien bleiben erhalten, ebenso Disketten mit Spielstand, bis du sie zurückholst.": "Delete the files Breadbin put into the launch folder {dir} on “{name}”? Other files are kept, and so are disks with saved games until you fetch them.",
  "In Liste aufnehmen": "Add to list",
  "Ist der Ordner eine Quelle, erscheinen sie nach dem nächsten Scan als eigene Fassung.": "If the folder is a source, they appear as versions of their own after the next scan.",
  "Keine geänderten Disketten im Startordner (oder schon geholt).": "No changed disks in the launch folder (or already fetched).",
  "Keine neuen Spielstände.": "No new saved games.",
  "Kopiere {n} Spiele auf {name} …": "Copying {n} games to {name} …",
  "Listen verwalten …": "Manage lists …",
  "Liste anlegen …": "Create a list …",
  "Liste „{name}“ löschen? Die {n} Fassungen darin bleiben in der Bibliothek.": "Delete the list “{name}”? The {n} versions in it stay in the library.",
  "Name der neuen Liste": "Name of the new list",
  "Neue Liste …": "New list …",
  "Nicht kopiert:": "Not copied:",
  "Noch keine Listen.": "No lists yet.",
  "Notizen, Codes, Tipps …": "Notes, codes, hints …",
  "Ordner für Spielstände wählen": "Choose a folder for saved games",
  "Schon vorhanden:": "Already there:",
  "Sichere die Bibliothek …": "Backing up the library …",
  "Sicherung zurückgespielt ({n} Bilder). Breadbin lädt die Ansicht neu.": "Backup restored ({n} pictures). Breadbin reloads the view.",
  "Sicherung zurückspielen": "Restore backup",
  "Spiele die Sicherung zurück …": "Restoring the backup …",
  "Spielstände zurückholen": "Fetch saved games",
  "Spielstände zurückholen …": "Fetch saved games …",
  "Suche geänderte Disketten auf {name} …": "Looking for changed disks on {name} …",
  "Zurückspielen": "Restore",
  "{n} Disketten mit Spielstand geholt:": "{n} disks with saved games fetched:",
  "{n} Spiele in „{list}“ aufgenommen.": "{n} games added to “{list}”.",
  "{n} Spiele kopieren. Was schon auf dem Gerät liegt, wird nicht noch einmal kopiert.": "Copy {n} games. Whatever is already on the device is not copied again.",
  "{n} Spielstände geholt.": "{n} saved games fetched.",
  "{n} kopiert, {m} lagen schon auf dem Gerät.": "{n} copied, {m} were already on the device.",
  "{n} von 5": "{n} of 5",
  "Namen ändern sich beim Verlassen des Feldes. Löschen entfernt nur die Liste, nicht die Spiele.": "Names change when you leave the field. Deleting removes only the list, not the games.",
  "Fertig": "Done",
  "Ordner auf dem Gerät": "Folder on the device",
  "Die Dateien behalten ihren Namen. Im Menü der Ultimate findest du sie dann auch ohne Rechner.": "The files keep their names. You can then find them in the Ultimate menu without a computer.",
  "Bibliothek sichern …": "Back up library …",
  "Sicherung zurückspielen …": "Restore backup …",
  "Datenbank mit Quellen, Geräten, eigenen Angaben, Listen, Notizen und Einstellungen samt allen Bildern als eine ZIP-Datei speichern": "Save the database with sources, devices, your own details, lists, notes and settings together with all pictures as one ZIP file",
  "Eine Sicherung zurückspielen – ersetzt die jetzige Bibliothek": "Restore a backup – replaces the current library",
};

/** Übersetzt einen deutschen Text (mit `{platzhaltern}`) in die gewählte Sprache. */
function t(de, vars = {}) {
  const text = LANG === "en" ? (EN[de] ?? de) : de;
  return text.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m));
}

/** Technische Angabe eines Inhalts (beim Scan auf Deutsch gespeichert, die Filter suchen danach)
 * in der Oberflächensprache. */
function tDetail(s) {
  if (!s || LANG === "de") return s ?? "";
  return s
    .replace(/: Datei beschädigt oder unbekannter Aufbau$/, ": file damaged or unknown layout")
    .replace(/^Ladeadresse /, "Load address ")
    .replace(/^Normales Modul,/, "Normal cartridge,")
    .replace(/^Modultyp (\d+),/, "Cartridge type $1,")
    .replace(/^GCR-Abbild, (\d+) Halbspuren$/, "GCR image, $1 half tracks")
    .replace(/^Band-Abbild Version /, "Tape image version ");
}

/** Genres der festen Liste (wie ScreenScraper sie deutsch liefert) für die englische Anzeige; Unbekanntes bleibt. */
const GENRE_EN = {
  "Plattform": "Platform", "Jump 'n' Run": "Run & Jump", "Shoot'em Up": "Shoot'em Up", "Horizontal": "Horizontal",
  "Vertikal": "Vertical", "Action": "Action", "Labyrinth": "Maze", "Abenteuer": "Adventure", "Breakout Spiele": "Breakout games",
  "Text": "Text", "Grafik": "Graphics", "Strategie": "Strategy", "Puzzle": "Puzzle", "Fallen": "Falling blocks",
  "Überlegung": "Thinking", "Rollenspiele": "Role-playing", "Rennen": "Racing", "Fahren": "Driving", "Simulation": "Simulation",
  "Sport": "Sports", "Shooter": "Shooter", "Beat'em Up": "Beat'em Up", "Kartenspiele": "Card games", "Brettspiele": "Board games",
  "Flipper": "Pinball", "Gelegenheitsspiel": "Casual", "Geschicklichkeit": "Skill", "Arcade": "Arcade", "Sammlung": "Compilation",
  "Lernspiel": "Educational", "Quiz": "Quiz", "Kampf": "Fighting", "Flug": "Flight", "Musik": "Music", "Verschiedenes": "Miscellaneous",
};
function tGenres(s) {
  if (!s || LANG === "de") return s ?? "";
  return s.split(", ").map((g) => g.split(" / ").map((p) => GENRE_EN[p.trim()] ?? p).join(" / ")).join(", ");
}

/** Zahl im Format der Oberflächensprache. */
function nf(n) {
  return Number(n).toLocaleString(LOCALE);
}

/** Feste Texte in der Seite übersetzen; Leerraum um den Text bleibt erhalten. */
function translateDom(root = document.body) {
  document.documentElement.lang = LANG;
  if (LANG === "de") return;
  const lookup = (s) => {
    const key = s.replace(/\s+/g, " ").trim();
    return key && EN[key] !== undefined ? s.replace(/\S[\s\S]*\S|\S/, EN[key]) : null;
};
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
    acceptNode: (n) => (n.parentElement?.closest('script, style, code, [translate="no"]') ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
  });
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const v = lookup(n.nodeValue);
    if (v !== null) n.nodeValue = v;
  }
  root.querySelectorAll("[title], [placeholder]").forEach((el) => {
    for (const attr of ["title", "placeholder"]) {
      const v = el.hasAttribute(attr) ? lookup(el.getAttribute(attr)) : null;
      if (v !== null) el.setAttribute(attr, v);
    }
  });
}

/** Sprache merken. Gibt zurück, ob sie sich geändert hat (dann Seite neu laden). */
function setUiLang(v) {
  if (!UI_LANGS.includes(v) || v === storedUiLang()) return false;
  try { localStorage.setItem("uiLang", v); } catch { return false; }
  return true;
}
