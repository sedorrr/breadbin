# Austauschformat „breadbin-share“, Version 1 / Exchange format “breadbin-share”, version 1

*English below.*

Dieses Format benutzen die Austauschdatei („Austauschdatei speichern …“), der Ordner aus „Beitrag vorbereiten …“,
das Daten-Repository der Gemeinschaftsdatenbank und die Datei `breadbin-db.zip`, die Breadbin beim Aktualisieren
lädt. Alle vier haben denselben Aufbau – eine Austauschdatei ist einfach dieser Ordner als ZIP.

## Aufbau

```
breadbin-share.json              Kopf
games/<ab>/<sha1>.json           je Inhalt (Spieldatei) ein Eintrag; <ab> = erste zwei Zeichen der SHA-1
images/<cd>/<sha1-des-bildes>.png   je Bild eine Datei, benannt nach der SHA-1 des Bildes (auch .jpg, .gif, .webp)
```

Schlüssel sind Prüfsummen, keine laufenden Nummern: Zwei Leute, die dieselbe Datei beschreiben, schreiben in
dieselbe Datei; gleiche Bilder liegen nur einmal da. Ein ZIP darf zusätzlich einen äußeren Ordner haben (so lädt
GitHub Repositorys herunter); Breadbin findet `breadbin-share.json` auch eine Ebene tiefer.

## Kopf `breadbin-share.json`

| Feld | Typ | Bedeutung |
| --- | --- | --- |
| `format` | Text | immer `breadbin-share` |
| `version` | Zahl | `1`; eine Breadbin-Fassung liest keine höhere Version |
| `created` | Zahl | Erstellungszeit (Unix-Sekunden); Breadbin vergleicht sie beim wöchentlichen Aktualisieren |
| `app` | Text | Erzeuger, z. B. `Breadbin/0.6.3` oder `breadbin-db` |
| `license` | Text | immer `CC-BY-SA-4.0` |
| `games` | Zahl | Anzahl der Einträge unter `games/` |

## Eintrag `games/<ab>/<sha1>.json`

| Feld | Pflicht | Bedeutung |
| --- | --- | --- |
| `version` | ja | `1` |
| `sha1` | ja | SHA-1 des Inhalts, 40 Hex-Zeichen klein; muss zum Dateinamen passen |
| `md5`, `crc32` | ja | weitere Prüfsummen des Inhalts (32 bzw. 8 Hex-Zeichen) |
| `size` | ja | Größe in Byte |
| `format` | ja | `PRG`, `CRT`, `D64`, `D71`, `D81`, `G64`, `T64`, `TAP`, `P00` |
| `files` | ja | bekannte Dateinamen ohne Ordner (zum Wiedererkennen) |
| `title` | ja | Titel |
| `year`, `publisher`, `developer`, `players` | nein | Text; `year` = Jahr des Originals |
| `genres` | nein | Liste, Genres aus der festen Liste der App (z. B. `Shoot'em Up / Horizontal`) |
| `synopsis` | nein | Beschreibung, Absätze durch Leerzeile: Deutsch bzw. die einzige vorhandene Sprache (so lesen es Fassungen vor 0.8.0) |
| `texts` | nein | Beschreibung je Sprache, `{"de": "…", "en": "…"}` (seit 0.8.0). Steht nur da, wenn es einen englischen Text gibt; dann ist `synopsis` = `texts.de`, ohne Deutsch = `texts.en`. Einlesen: `texts` vor `synopsis` |
| `images` | nein | Bildart → Pfad im Archiv; Arten: `sstitle` (Titelbild), `ss` (Spielszene), `box-2D` (Verpackung) |
| `screenscraper_id` | nein | Spielnummer bei ScreenScraper, nur zum Zusammenfassen von Fassungen |

Inhalt bezeichnet die Spieldatei selbst: Bei Disketten- und Modulabbildern die ganze Datei, bei Dateien in ZIP-Archiven
das ausgepackte Abbild. Mehrere Disketten eines Spiels sind mehrere Einträge mit denselben Angaben.

## Grenzen und Regeln

- Eintrag höchstens 256 KB, Bild höchstens 8 MB, Austauschdatei höchstens 512 MB, höchstens 200 000 Einträge.
- Bilder: PNG, JPEG, GIF oder WebP; der Inhalt muss zur Endung passen. Bildschirmfotos vom C64 am besten 320 × 200
  ohne Rahmen (so nimmt Breadbin sie über den Videostrom auf).
- Nur selbst Erstelltes: eigene Texte, eigene Bildschirmfotos, eigene Verpackungsfotos. Keine Texte oder Bilder aus
  ScreenScraper, MobyGames oder anderen Datenbanken. Breadbin exportiert ScreenScraper-Angaben nie, auch nicht
  bearbeitete.
- Lizenz: CC BY-SA 4.0. Wer einsendet, stimmt der Veröffentlichung unter dieser Lizenz zu.
- Einlesen überschreibt nie selbst geschriebene Angaben; Gemeinschaftsdaten gehen ScreenScraper vor, solange in den
  Einstellungen „Gemeinschaft“ die bevorzugte Quelle ist.
- Prüfwerkzeug: `tools/pruefen.py` im Daten-Repository (nur Python-Standardbibliothek).

## Änderungen am Format

Neue, freiwillige Felder dürfen in Version 1 dazukommen; ältere Fassungen überlesen sie. Wird ein Feld umgedeutet oder
Pflicht, steigt `version` – Breadbin lehnt dann Dateien mit höherer Version mit einem Hinweis ab, statt sie falsch zu
lesen.

---

# English

This format is used by the exchange file (“Save exchange file …”), the folder written by “Prepare contribution …”,
the community database repository and `breadbin-db.zip`, which Breadbin downloads when updating. All four share the
same layout – an exchange file is simply that folder as a ZIP.

**Layout:** `breadbin-share.json` (header); one entry per content (game file) at `games/<ab>/<sha1>.json`, where
`<ab>` are the first two characters of the SHA-1; one file per picture at `images/<cd>/<sha1 of the picture>.png`
(also .jpg, .gif, .webp). Keys are checksums, not running numbers. A ZIP may have one outer folder (as GitHub
downloads repositories).

**Header:** `format` = `breadbin-share`, `version` = `1` (newer versions are refused), `created` (Unix seconds,
compared by the weekly update), `app`, `license` = `CC-BY-SA-4.0`, `games` (number of entries).

**Entry:** required `version` (1), `sha1` (matches the file name), `md5`, `crc32`, `size`, `format`, `files` (known
file names without folders), `title`; optional `year` (year of the original), `publisher`, `developer`, `players`,
`genres` (list, from the app’s fixed genre list), `synopsis` (paragraphs separated by a blank line; German or the only
language available), `texts` (since 0.8.0: per-language descriptions `{"de", "en"}`, written only when an English text
exists; readers prefer `texts` over `synopsis`), `images`
(kind → path; kinds `sstitle` title screen, `ss` in-game, `box-2D` box), `screenscraper_id` (only for grouping
versions).

**Limits and rules:** entry ≤ 256 KB, picture ≤ 8 MB, exchange file ≤ 512 MB, ≤ 200,000 entries. Pictures PNG, JPEG,
GIF or WebP matching their extension; C64 screenshots ideally 320 × 200 without border. Only your own work – no texts
or pictures from ScreenScraper, MobyGames or other databases; Breadbin never exports ScreenScraper data. Licence
CC BY-SA 4.0. Importing never overwrites details you wrote yourself. Checker: `tools/pruefen.py` in the data
repository.

**Changes:** new optional fields may be added within version 1; older versions ignore them. If a field changes meaning
or becomes required, `version` goes up and older Breadbin versions refuse such files with a notice.
