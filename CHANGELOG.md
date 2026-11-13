# Changelog / Änderungen

English first, German below each version. / Englisch zuerst, Deutsch jeweils darunter.

## 0.9.1 (2026-11-13)

- Built-in manual updated (server choice for community data, link to the public data repository, note on anonymous
  counting); the same manual is now also on the website: https://breadbin.8bitcavern.de/anleitung/
- German texts: correct grammatical gender for the Ultimate devices throughout.
- The result list no longer fails if it is drawn before the user's lists have loaded.
- Eingebautes Handbuch aktualisiert (Serverauswahl für Gemeinschaftsdaten, Link zum öffentlichen Daten-Repository,
  Hinweis zur anonymen Zählung); dasselbe Handbuch steht jetzt auch auf der Webseite:
  https://breadbin.8bitcavern.de/anleitung/
- Deutsche Texte: „der Ultimate“ überall richtig.
- Die Trefferliste bricht nicht mehr ab, wenn sie gezeichnet wird, bevor die eigenen Listen geladen sind.

## 0.9.0 (2026-11-13) – public beta / öffentliche Beta

- First public release: the source code is now on GitHub (GPL-3.0-or-later), the community data in its own public
  repository (`sedorrr/breadbin-db`, CC BY-SA 4.0).
- The website counts downloads and the app's daily update check anonymously (daily totals per version; no IP
  addresses, IDs or cookies are stored) – see the README.
- Much larger community data: more than 1,000 games with German and English descriptions and own screenshots.
- Erste öffentliche Fassung: Der Quelltext liegt jetzt auf GitHub (GPL-3.0-or-later), die Gemeinschaftsdaten in einem
  eigenen öffentlichen Repository (`sedorrr/breadbin-db`, CC BY-SA 4.0).
- Die Webseite zählt Downloads und die tägliche Update-Prüfung der App anonym (Tageszahlen je Version; keine
  IP-Adressen, Kennungen oder Cookies) – siehe README.
- Deutlich mehr Gemeinschaftsdaten: über 1 000 Spiele mit deutschen und englischen Beschreibungen und eigenen
  Bildschirmfotos.

## 0.8.2 (2026-11-11)

- Settings: community data and contributions each have a clear choice – Breadbin server (default), your own server
  with its own address, or (contributions only) “Do not submit”. ScreenScraper stays optional and separate.
- Contributions now go to `breadbindbsubmit.8bitcavern.de`; the previous address still counts as the default.
- macOS packages are signed with a fixed certificate (still not notarised).
- Einstellungen: Gemeinschaftsdaten und Beiträge haben je eine klare Auswahl – Breadbin-Server (Standard), eigener
  Server mit eigener Adresse oder (nur Beiträge) „Nicht einsenden“. ScreenScraper bleibt freiwillig und getrennt davon.
- Beiträge gehen jetzt an `breadbindbsubmit.8bitcavern.de`; die bisherige Adresse zählt weiter als Standard.
- macOS-Pakete sind mit einem festen Zertifikat signiert (weiterhin nicht notarisiert).

## 0.8.1 (2026-11-09)

- “Submit contribution …” splits large amounts into several contributions (at most 400 games and about 15 MB each);
  before, more than 500 changed games were rejected as “too many games in one contribution”. Each accepted part
  counts as submitted right away; if a later part fails, the next submission sends only the rest.
- „Beitrag einsenden …“ teilt große Mengen in mehrere Beiträge auf (höchstens 400 Spiele und etwa 15 MB je Beitrag);
  bisher wurden mehr als 500 geänderte Spiele als „zu viele Spiele in einem Beitrag“ abgelehnt. Jeder angenommene Teil
  gilt sofort als eingesandt; scheitert ein späterer Teil, schickt das nächste Einsenden nur den Rest.

## 0.8.0 (2026-11-09)

- Descriptions in German and English: “Enter details yourself” has a field per language; the app shows the text in the
  interface language (or the other one with a note), and submitting/sharing sends both (new optional field `texts` in
  the exchange format; older versions keep showing the German text). Genres are shown translated in the English
  interface.
- Beschreibungen auf Deutsch und Englisch: „Angaben selbst eintragen“ hat ein Feld je Sprache; die App zeigt den Text in
  der Sprache der Oberfläche (sonst den anderen mit Hinweis), Einsenden und Teilen schicken beide mit (neues freiwilliges
  Feld `texts` im Austauschformat; ältere Fassungen zeigen weiter den deutschen Text). Genres erscheinen in der
  englischen Oberfläche übersetzt.

## 0.7.0 (2026-11-09)

- Automatic title picture (⚙ Settings, off by default): after “Launch” the app waits a set time (25 s by default)
  and takes a screenshot as your own title picture, but only if the game has none yet (Ultimate 64 and C64 Ultimate
  with a network cable).
- “Random game” button (or key Z): shows a random game from the current results.
- “view on the web” next to “Community database”: opens statistics and search of the community data.
- Community data now comes from https://breadbindb.8bitcavern.de (same data; the old address keeps working for older versions).
- Titelbild automatisch (⚙ Einstellungen, Standard aus): Nach „Starten“ wartet die App eine einstellbare Zeit
  (Standard 25 s) und nimmt ein Bildschirmfoto als eigenes Titelbild auf, aber nur, wenn das Spiel noch keines hat
  (Ultimate 64 und C64 Ultimate mit Netzwerkkabel).
- Knopf „Zufallsspiel“ (oder Taste Z): zeigt ein zufälliges Spiel aus den aktuellen Treffern.
- Gemeinschaftsdaten kommen jetzt von https://breadbindb.8bitcavern.de (gleiche Daten; die alte Adresse bleibt für ältere Fassungen erreichbar).
- „im Web ansehen“ neben „Gemeinschaftsdatenbank“: öffnet Statistik und Suche der Gemeinschaftsdaten.

## 0.6.3 (2026-11-08)

- Disk launcher: games with an autostart loader (loaded with ,8,1 into the cassette buffer, started via the CLALL
  vector, e.g. “Abyssal Karma”) now start; before they stopped at READY.
- Disk-Start: Spiele mit Autostart-Lader (mit ,8,1 in den Kassettenpuffer geladen, Start über den CLALL-Vektor, z. B.
  „Abyssal Karma“) starten jetzt; bisher blieben sie bei READY stehen.

## 0.6.2 (2026-11-07)

- “Separate” on every version in the details’ versions list: a version that was grouped with a different game only
  because of its name gets its own row again (before, only the version currently shown could be separated).
- Technical details (damaged, load address, cartridge type …) appear in English in the English interface.
- “Submit contribution …” sends only games that are new or changed since the last submission (checkbox: send all
  again); after sending, the dialog only offers “OK”.
- „Lösen“ an jeder Fassung in der Fassungsliste der Details: Eine Fassung, die nur wegen des Namens bei einem fremden
  Spiel gelandet ist, steht wieder allein (bisher ließ sich nur die gerade gezeigte Fassung lösen).
- Technikangaben (beschädigt, Ladeadresse, Modultyp …) erscheinen in der englischen Oberfläche auf Englisch.
- „Beitrag einsenden …“ schickt nur Spiele, die seit dem letzten Einsenden neu sind oder sich geändert haben (Haken:
  alles noch einmal senden); danach bietet der Dialog nur noch „OK“.

## 0.6.1 (2026-11-06)

- “Fetch saved game …” in the details of a disk game (next to “Launch”) fetches the saved games of just that game;
  “Fetch saved games …” in the device row still fetches all of them.
- “Match all to one game” / “Choose another game” also finds entries of your own library (written yourself,
  community, already matched), even with spelling variants; details and pictures are taken over and the versions
  then share one row. Works without a ScreenScraper account too.
- „Spielstand holen …“ in den Details eines Diskettenspiels (neben „Starten“) holt die Spielstände nur dieses Spiels;
  „Spielstände zurückholen …“ in der Gerätezeile holt weiterhin alle.
- „Gemeinsam einem Spiel zuordnen“ / „Anderes Spiel wählen“ findet auch Einträge der eigenen Bibliothek (selbst
  geschrieben, Gemeinschaft, schon zugeordnet), auch bei Schreibvarianten; Angaben und Bilder werden übernommen, die
  Fassungen stehen danach in einer Zeile. Geht auch ohne ScreenScraper-Konto.

## 0.6.0 (2026-11-06)

- Notice in the status bar when a newer Breadbin version is available (checked at most once a day on
  8bitcavern.de, can be switched off). Community data is updated automatically at start, at most once a week
  and only when there is a new state (can be switched off).
- “Submit contribution …” sends to the contribution server on 8bitcavern.de by default (no GitHub account needed).
- Clean-up overview (Collection → “Clean up …”): duplicate files with the space they take, damaged images, entries
  without details; new views “Without details” and “Damaged / unknown”; duplicates as CSV with all locations.
- Your own lists, rating (1–5), “Completed” and notes per game (“For me” in the details, lists in the view
  selector). Copy marked games to a folder on a device (games already on it are skipped). Fetch saved games:
  disks in the launch folder that changed since they were copied; “Empty launch folder” keeps them. Back up the
  whole library as one ZIP and restore it. Device row shows the firmware and a ⚠ with hints (old firmware,
  JiffyDOS and tapes).
- Pictures can be reassigned in “Edit details”: “Use as in-game” / “Use as title screen” moves a picture saved as
  the wrong kind; if there is already one there, the two swap places.
- “Manage lists …” is also the last entry of the view selector; it can create lists too, and deleting a list asks first.
- „Beitrag einsenden …“ schickt standardmäßig an den Annahme-Server auf 8bitcavern.de (ohne GitHub-Konto).
- Aufräum-Übersicht (Bestand → „Aufräumen …“): doppelte Dateien samt belegtem Platz, beschädigte Abbilder, Einträge
  ohne Angaben; neue Ansichten „Ohne Angaben“ und „Beschädigt / unbekannt“; doppelte Dateien als CSV mit allen Orten.
- Eigene Listen, Bewertung (1–5), „Durchgespielt“ und Notizen je Spiel („Für mich“ in den Details, Listen in der
  Ansichtsauswahl). Markierte Spiele in einen Ordner auf einem Gerät kopieren (was dort schon liegt, wird
  übersprungen). Spielstände zurückholen: Disketten im Startordner, die sich seit dem Hinkopieren geändert haben;
  „Startordner leeren“ lässt sie liegen. Ganze Bibliothek als ZIP sichern und zurückspielen. Geräte zeigen die
  Firmware und ein ⚠ mit Hinweisen (alte Firmware, JiffyDOS und Bänder).
- Bilder lassen sich in „Angaben bearbeiten“ umordnen: „Als Spielszene“ / „Als Titelbild“ verschiebt ein falsch
  eingeordnetes Bild; liegt dort schon eins, tauschen beide den Platz.
- „Listen verwalten …“ steht auch als letzter Eintrag in der Ansichtsauswahl, legt dort auch Listen an; Löschen fragt nach.
- Hinweis in der Statuszeile, wenn eine neuere Breadbin-Fassung da ist (höchstens einmal am Tag auf 8bitcavern.de
  geprüft, abschaltbar). Gemeinschaftsdaten werden beim Start selbst aktualisiert, höchstens einmal pro Woche und
  nur bei neuem Stand (abschaltbar).

## 0.5.7 (2026-11-03)

- Community database: “Update community data”, “Prepare contribution …” and “Submit contribution …”;
  reading also accepts a repository ZIP with an outer folder and an unpacked data folder. The data comes
  from 8bitcavern.de (address adjustable in the settings).
- Gemeinschaftsdatenbank: „Gemeinschaftsdaten aktualisieren“, „Beitrag vorbereiten …“ und „Beitrag
  einsenden …“; Einlesen versteht auch ein Repository-ZIP mit äußerem Ordner und einen ausgepackten Datenordner. Die Daten
  kommen von 8bitcavern.de (Adresse in den Einstellungen änderbar).

## 0.5.6 (2026-11-02)

- Several ScreenScraper requests at once (as many as the account allows, at most 4); setting “Simultaneous
  requests”. Name search only from 3 characters; a failing name search no longer stops the fetch. Text answers
  from ScreenScraper are recognised; “Show errors” during and after a fetch. Edit details for all versions of a
  row at once. macOS DMG with “Read me first”. English README.
- Mehrere Abfragen bei ScreenScraper gleichzeitig (so viele wie das Konto erlaubt, höchstens 4); Einstellung
  „Gleichzeitige Abfragen“. Namenssuche erst ab 3 Zeichen; eine gescheiterte Namenssuche hält den Abruf nicht mehr
  an. Textantworten von ScreenScraper werden erkannt; „Fehler anzeigen“ während und nach dem Abruf. Angaben für alle
  Fassungen einer Zeile auf einmal. macOS-DMG mit „Zuerst lesen“. README auf Englisch.

## 0.5.5 (2026-11-02)

- Sharing via exchange files; community first, ScreenScraper optional; “Write your own version”; matches
  without a fitting checksum marked “please check”. Fixed: crash at start with titles ending in an umlaut, fetch
  stopping on control characters in ScreenScraper texts. Contact address and https links.
- Teilen über Austauschdateien; Gemeinschaft vor ScreenScraper, ScreenScraper freiwillig; „Eigene Fassung
  schreiben“; Treffer ohne passende Prüfsumme als „bitte prüfen“ gekennzeichnet. Behoben: Absturz beim Start bei
  Titeln mit Umlaut am Ende, Abbruch bei Steuerzeichen in ScreenScraper-Texten. Kontaktadresse und https-Links.

## 0.5.4 (2026-11-02)

- Screenshot from the Ultimate (retry on “Network Host Resolve Error”), device search finds the C64 Ultimate’s
  Wi-Fi address and prefers the cable, view “Unknown to ScreenScraper”.
- Bildschirmfoto von der Ultimate (Wiederholung bei „Network Host Resolve Error“), Gerätesuche findet die
  WLAN-Adresse des C64 Ultimate und bevorzugt das Kabel, Ansicht „Bei ScreenScraper unbekannt“.
