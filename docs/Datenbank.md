# Datenbank der App / The app's database

*English below.*

Breadbin speichert die Bibliothek in einer SQLite-Datei `spiele.sqlite` im Datenordner der App (macOS:
`~/Library/Application Support/de.dersedor.breadbin/`, Windows: `%APPDATA%\de.dersedor.breadbin\`, Linux:
`~/.local/share/de.dersedor.breadbin/`, tragbare Windows-Fassung: `BreadbinData` neben der exe). Bilder liegen
daneben im Ordner `media/`, Kopien für den Emulator in `emulator/`.

Die Datenbank ist Arbeitsspeicher der App, keine Schnittstelle: Zum Weitergeben gibt es das Austauschformat
(`Austauschformat.md`), zum Sichern „Bibliothek sichern …“ (ZIP mit Datenbank und Bildern).

## Version des Aufbaus

- Die Version steht in `PRAGMA user_version` (`SCHEMA_VERSION` in `core/src/db.rs`). Datenbanken bis 0.9.4 haben 0;
  jede Fassung ab der Einführung (nach 0.9.4) hebt sie beim Öffnen auf die eigene Version.
- **Neue Spalten und Tabellen** kommen ohne neue Version dazu: Beim Öffnen baut Breadbin fehlende Spalten ein
  (`ALTER TABLE … ADD COLUMN` mit Vorgabewert) und legt fehlende Tabellen an. Ältere Fassungen kennen sie nicht
  und lassen sie in Ruhe.
- **Die Version steigt nur**, wenn eine ältere Fassung die Datenbank falsch lesen oder beim Schreiben beschädigen
  würde – etwa wenn eine Spalte eine andere Bedeutung bekommt oder eine Tabelle ersetzt wird.
- Eine Fassung öffnet keine Datenbank mit **höherer** Version: Sie zeigt beim Start eine Meldung („Bibliothek
  stammt aus einer neueren Fassung“) und beendet sich, statt Daten kaputtzuschreiben. Ebenso lehnt
  „Sicherung zurückspielen …“ eine Sicherung mit neuerer Datenbank ab.
- Die Version des Austauschformats (`breadbin-share`, Version 1) ist davon unabhängig.

## Tabellen (Version 1)

| Tabelle | Inhalt |
| --- | --- |
| `sources` | Quellen: Ordner, Netzlaufwerke, Gerätesticks (`kind`, `location`, Schnellscan) |
| `contents` | je Inhalt (Spieldatei, bei ZIP das ausgepackte Abbild) Prüfsummen, Größe, Format, Name im Abbild, technische Angabe (`detail`, deutsch gespeichert) |
| `entries` | Verzeichnis eines Abbilds (Dateinamen auf der Diskette) |
| `files` | Fundorte: Quelle, Pfad, ZIP-Mitglied, Titel/Jahr/Herausgeber aus dem Dateinamen, Ordner für Diskettenreihen |
| `metadata` | Angaben je Inhalt: `provider` (`screenscraper`, `own`, `own:edited`, `shared`), Status, Titel, Jahr, Genres, Beschreibung DE/EN, Art (`kind`, NULL = Spiel), Gruppe (`grp`), Party, Platz, Herkunft `history` (JSON, nur Gemeinschaftsangaben) … |
| `media` | Bilder je Inhalt (`sstitle`, `ss`, `ss2`–`ss4`, `box-2D`) → Datei in `media/` |
| `settings` | Einstellungen als Schlüssel/Wert |
| `favorites`, `notes` | Favoriten; Bewertung 0–5, „durchgespielt“, Notiz |
| `lists`, `list_items` | eigene Listen |
| `emu_game_options` | Emulator-Optionen je Spiel und Emulatorfamilie |
| `ungrouped` | vom Nutzer aus einer Gruppe gelöste Fassungen |
| `submitted` | Fingerabdruck des zuletzt Eingesandten je Inhalt |
| `proposals` | an ScreenScraper geschickte Vorschläge |
| `launches` | Starts (Gerätename bzw. „Emulator“, Zeit) |
| `devices` | Geräte: Adresse, Art (`ultimate`, `bmc64`, `teensyrom`, `bmx`), Startordner, MAC, Kennung |

Schlüssel für Spiele ist überall die SHA-1 des Inhalts, nicht eine laufende Nummer – so überstehen eigene Angaben,
Listen und Bewertungen einen Neuscan.

---

# English

Breadbin keeps the library in an SQLite file `spiele.sqlite` in the app's data folder (paths as above; the portable
Windows version uses `BreadbinData` next to the exe). Pictures are in `media/`, emulator copies in `emulator/`. The
database is the app's working store, not an interface: use the exchange format (`Austauschformat.md`) to share data
and “Back up library …” to save it.

**Schema version:** stored in `PRAGMA user_version` (`SCHEMA_VERSION` in `core/src/db.rs`); databases up to 0.9.4
have 0, every version since raises it to its own on opening. New columns and tables are added without a new version
(missing columns are added with a default, missing tables created; older versions leave them alone). The version
only goes up when an older Breadbin would misread or damage the database, e.g. a column changes meaning or a table is
replaced. A Breadbin version never opens a database with a higher version: it shows a message at start (“library
comes from a newer version”) and quits instead of corrupting data; “Restore backup …” refuses such backups too. The
exchange format (`breadbin-share`, version 1) is versioned separately.

**Tables (version 1):** see the table above – `sources`, `contents`, `entries`, `files`, `metadata`, `media`,
`settings`, `favorites`, `notes`, `lists`, `list_items`, `emu_game_options`, `ungrouped`, `submitted`, `proposals`,
`launches`, `devices`. Games are keyed by the SHA-1 of the content everywhere, not by running numbers, so your own
details, lists and ratings survive a rescan.
