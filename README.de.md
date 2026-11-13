# Breadbin – C64 Game Library

*English: [README.md](README.md)*

Ein Projekt von [8bitcavern.de](https://8bitcavern.de). Kontakt: breadbin@8bitcavern.de

Breadbin ist eine kostenlose Desktop-App für Windows, macOS und Linux, die den Überblick über deine
Commodore-64-Spiele behält: Sie erfasst lokale Ordner, Netzlaufwerke und die USB-Sticks von
Ultimate-Geräten, zeigt, was wo liegt, ergänzt Beschreibungen und Bilder und startet Spiele direkt auf
einer Ultimate 64, einem C64 Ultimate oder einem 1541 Ultimate II+.

Die Oberfläche gibt es auf Deutsch und Englisch (nach Systemsprache, umschaltbar in den Einstellungen),
eine Anleitung ist eingebaut (Knopf „?“ oder F1).

## Funktionen

**Sammlung erfassen**
- Quellen: lokale Ordner, eingebundene Netzlaufwerke und Ordner auf Ultimate-Geräten (per FTP gelesen)
- Formate: PRG, P00, CRT, D64, D71, D81, G64, T64, TAP – auch in ZIP-Archiven
- Disk-Verzeichnisse (D64/D71/D81) und T64-Inhalte werden ausgelesen und sind durchsuchbar; Modultyp bei
  CRT (EasyFlash, Magic Desk, Ocean …), Ladeadresse bei PRG
- Titel, Jahr und Herausgeber aus Dateinamen (TOSEC-Muster und Varianten)
- CRC32, MD5 und SHA-1 je Datei; gleiche Dateien an mehreren Orten erscheinen einmal mit allen Fundorten
- Ein zweiter Scan liest nur Geändertes (43 859 Dateien: erster Lauf 80 s, danach 1,6 s); ein nicht
  erreichbares Netzlaufwerk löscht keine Einträge
- Schnellscan für Gerätesticks: Dateien mit gleichem Namen und gleicher Größe wie in einer anderen Quelle
  werden übernommen statt heruntergeladen
- Fassungen desselben Spiels (D64, CRT, Cracks …) als eine Zeile; Filter, Favoriten, „Zuletzt gestartet“,
  sortierbare Spalten, Ausfuhr als CSV
- Eigene Listen, Bewertung, „durchgespielt“ und Notizen je Spiel; ganze Bibliothek sichern und zurückspielen

**Beschreibungen und Bilder**
- **Eigene Angaben gehen vor:** Titel, Jahr, Herausgeber, Entwickler, Genre, Spieler, Beschreibung und
  eigene Bilder (Titelbild, Spielszene, Verpackung) – für eine oder alle Fassungen eines Spiels
- **Bildschirmfoto** von einer Ultimate 64 / einem C64 Ultimate über den Videostrom (nur mit Netzwerkkabel)
- **Gemeinschaft:** selbst geschriebene Angaben als Austauschdatei teilen und die anderer einlesen –
  zugeordnet über die Prüfsumme, ohne Konto. Geteilte Angaben stehen unter CC BY-SA 4.0.
  „Gemeinschaftsdaten aktualisieren“ lädt die gemeinsame Datenbank von 8bitcavern.de.
- **Datenschutz:** Höchstens einmal am Tag fragt die App bei 8bitcavern.de nach einer neuen Fassung. Der Server zählt
  diese Abrufe nur je Tag und App-Version (dazu die Downloads der Pakete); IP-Adressen, Kennungen oder Cookies werden
  nicht gespeichert.
- **ScreenScraper.fr (freiwillig):** Beschreibungen, Genres, Bewertung und Bilder, erkannt über die
  Prüfsumme (mit eigenem kostenlosem Konto); Treffer, die nur über den Namen passen, sind mit „bitte
  prüfen“ gekennzeichnet. Mehrere Abfragen gleichzeitig, wenn das Konto es erlaubt.

**Ultimate-Geräte**
- Spiele starten: PRG und CRT direkt, Disk-Abbilder einlegen und mit einem kleinen Lader starten
  (`LOAD"*",8,1` + `RUN`), P00/T64 werden umgewandelt; Dateien von anderswo werden vorher aufs Gerät kopiert
- Diskette einlegen ohne Neustart (Diskwechsel im laufenden Spiel); Spielstände von geänderten Disketten zurückholen
- Markierte Spiele in einen Ordner auf dem Gerät kopieren – dann findet man sie auch ohne Rechner im Menü
- Gerätestatus, Einschalten per Wake-on-WiFi, Ausschalten, Suche im Netz, Ordnerauswahl auf dem Gerät

## Herunterladen und erster Start

Die Pakete liegen auf der [Release-Seite](../../releases): macOS (Universal-DMG für Apple Silicon und
Intel), Windows (MSI- oder EXE-Installer) und Linux (AppImage, deb, rpm).

Breadbin ist nicht mit einem kostenpflichtigen Zertifikat signiert, deshalb warnen die Systeme beim
ersten Start:

- **macOS:** Das DMG enthält `Zuerst lesen - Read me first.txt`. Kurz: Meldung „Breadbin“ nicht geöffnet
  mit **Fertig** schließen, dann (ab macOS 15) Systemeinstellungen → Datenschutz & Sicherheit → ganz unten
  **Dennoch öffnen**; bis macOS 14 Rechtsklick auf die App → **Öffnen**. Danach den Zugriff aufs **lokale
  Netzwerk** erlauben, sonst sind keine Ultimate-Geräte erreichbar.
- **Windows:** SmartScreen „Der Computer wurde durch Windows geschützt“ → **Weitere Informationen** →
  **Trotzdem ausführen**.
- **Linux:** AppImage ausführbar machen (`chmod +x Breadbin_*.AppImage`) oder deb/rpm installieren.

## Getestet mit

- C64 Ultimate (Firmware 1.1.0) und Ultimate 64 Elite II (Firmware 3.15a): Start von PRG, T64, D64, D71,
  D81, G64 und CRT, Diskwechsel, Bildschirmfotos, Scan des USB-Sticks
- Ultimate 64 Elite I (Firmware 3.15a): Start von PRG, D64, D71, D81, G64 und CRT
- macOS, Windows und Linux (AppImage unter Fedora)
- Noch nicht getestet: 1541 Ultimate II+. Bandabbilder (TAP) werden erfasst, lassen sich aber noch nicht starten.

## Lizenz

Breadbin ist freie Software unter der GNU General Public License, Version 3 oder später
(`GPL-3.0-or-later`, siehe [LICENSE](LICENSE)). Über Breadbin geteilte Angaben stehen unter CC BY-SA 4.0.
Beschreibungen und Bilder von ScreenScraper.fr unterliegen dessen eigenen Bedingungen und werden nicht mit
der App verteilt.

## Für Entwickler

| Ordner | Inhalt |
| --- | --- |
| `core/` | Rust-Bibliothek ohne Oberfläche: Formate, Scanner, SQLite-Datenbank, FTP, ScreenScraper, Teilen, Start auf Ultimate-Geräten; `c64scan` für die Kommandozeile |
| `core/c64/` | Lader für Disk-Abbilder (64tass-Quelltext und fertiges PRG) |
| `src-tauri/` | Tauri-2-App: Befehle für die Oberfläche, Konfiguration, Symbole |
| `ui/` | Oberfläche (HTML, CSS, JavaScript ohne Build-Schritt) |
| `packaging/` | Zusatzdateien für die Pakete („Zuerst lesen“ für macOS) |
| `docs/` | Anleitungen, z. B. [ScreenScraper einrichten](docs/Anleitung_ScreenScraper.md) |

Die Datenbank liegt im Datenordner der App (macOS:
`~/Library/Application Support/de.dersedor.breadbin/spiele.sqlite`), die Bilder daneben im Ordner `media`.

Voraussetzung: Rust (`rustup`) und die Tauri-CLI (`cargo install tauri-cli --version "^2" --locked`);
unter Linux zusätzlich WebKitGTK 4.1 und die Tauri-Systempakete.

```bash
cargo test
```

```bash
cd src-tauri && cargo tauri dev
```

```bash
cd src-tauri && cargo tauri build
```

Scanner ohne Oberfläche (auch `ftp://<gerät>/USB0/Ordner`) und Geräteprüfung ohne Start:

```bash
cargo run --release -p breadbin-core --bin c64scan -- test.sqlite /Pfad/zu/Spielen
```

```bash
cargo run --release -p breadbin-core --bin c64scan -- --probe 192.0.2.64 /USB0/Breadbin
```

Weitere Befehle von `c64scan` (Gerätesuche, Wecken, Ausschalten, Start, Bildschirmfoto, CSV) stehen oben
in `core/src/bin/c64scan.rs`.

Die Pakete baut `.github/workflows/pakete.yml` (von Hand oder bei einem Tag `v*` als Release-Entwurf).
Die Entwickler-Kennung für ScreenScraper kommt aus den Repository-Secrets `SCREENSCRAPER_DEVID` und
`SCREENSCRAPER_DEVPASSWORD`; ohne sie tragen Nutzer eine eigene in den Einstellungen ein.

Lader neu bauen (nur nach Änderungen an `lader.asm`):

```bash
64tass --cbm-prg -a -o core/c64/lader.prg core/c64/lader.asm
```
