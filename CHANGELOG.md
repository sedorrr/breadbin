# Changelog / Änderungen

English first, German below each version. / Englisch zuerst, Deutsch jeweils darunter.

## 0.9.6 (2026-11-27)

- Before submitting, the dialog lists entries without description or picture and entries entered as games whose file
  name sounds like a demo, intro or music. These are hints only: click a title to edit it, “Send without these” keeps
  them for later, “Send anyway” sends everything. / Vor dem Einsenden listet der Dialog Einträge ohne Beschreibung oder
  Bild und Einträge, die als Spiel eingetragen sind, deren Dateiname aber nach Demo, Intro oder Musik klingt. Das sind
  nur Hinweise: Ein Klick auf einen Titel öffnet ihn zum Bearbeiten, „Ohne diese senden“ lässt sie für später liegen,
  „Trotzdem senden“ schickt alles.

## 0.9.5 (2026-11-27)

- Besides games, entries can now be demos, intros, music, diskmags, tools or graphics (field “Kind” in the details
  dialog, with group, party and place instead of genre and players). The list shows games by default; a new selector
  switches to all kinds or a single kind. Shared in the exchange format as `kind`, `group`, `party`, `place` – older
  versions show such entries as games. / Neben Spielen können Einträge jetzt Demos, Intros, Musik, Diskmags, Werkzeuge
  oder Grafiken sein (Feld „Art“ im Dialog „Angaben“, mit Gruppe, Party und Platz statt Genre und Spielerzahl). Die
  Liste zeigt standardmäßig Spiele; eine neue Auswahl schaltet auf alle oder eine bestimmte Art. Im Austauschformat als
  `kind`, `group`, `party`, `place` – ältere Fassungen zeigen solche Einträge als Spiel.
- Community entries show who added them and when, and who changed them last (“Added on … by …, last changed on …
  by …”). The server records this when it imports a contribution; the app never sends it. / Gemeinschaftseinträge
  zeigen, wann und von wem sie aufgenommen und zuletzt geändert wurden („Aufgenommen am … von …, zuletzt geändert am
  … von …“). Das trägt der Server beim Übernehmen eines Beitrags ein; die App schickt es nie mit.
- The library now carries a database version: a Breadbin version shows a message instead of opening (and possibly
  damaging) a library or backup from a newer version. Database and exchange format are documented in
  `docs/Datenbank.md` and `docs/Austauschformat.md`. / Die Bibliothek trägt jetzt eine Datenbank-Version: Eine
  Breadbin-Fassung zeigt eine Meldung, statt eine Bibliothek oder Sicherung aus einer neueren Fassung zu öffnen (und
  womöglich zu beschädigen). Datenbank und Austauschformat sind in `docs/Datenbank.md` und `docs/Austauschformat.md`
  beschrieben.
- VICE: “Insert” in the details swaps the disk in the running VICE (drive 8, no restart), and “Screenshot” takes the
  picture from VICE as title or in-game picture; the automatic title picture works with VICE too. Breadbin opens
  VICE's monitor on 127.0.0.1 for this. / VICE: „Einlegen“ in den Details wechselt die Diskette im laufenden VICE
  (Laufwerk 8, ohne Neustart), und „Bildschirmfoto“ nimmt das Bild aus VICE als Titelbild oder Spielszene; auch das
  automatische Titelbild geht mit VICE. Breadbin öffnet dafür den Monitor von VICE auf 127.0.0.1.
- New button ⤓ in each device row that is switched on: fetches saved games (disks a game changed in the launch
  folder) – before, this was hidden in the ⋯ menu. / Neuer Knopf ⤓ in jeder eingeschalteten Gerätezeile: holt die
  Spielstände (Disketten, die ein Spiel im Startordner verändert hat) – bisher nur im ⋯-Menü versteckt.
- New view “Most launched”; the details show how often a game was launched (devices and emulator). / Neue Ansicht
  „Am häufigsten gestartet“; die Details zeigen, wie oft ein Spiel gestartet wurde (Geräte und Emulator).
- New dialog Collection → “Statistics …”: size, share with description, favorites, ratings, launches, decades, most
  common genres and publishers. / Neuer Dialog Bestand → „Statistik …“: Größe, Anteil mit Beschreibung, Favoriten,
  Bewertungen, Starts, Jahrzehnte, häufigste Genres und Herausgeber.
- CSV export in English when the interface is English (column headers and technical details). / CSV-Export auf
  Englisch, wenn die Oberfläche englisch ist (Spaltenköpfe und technische Angaben).
- Devices that get a new IP address from the router are found again: an Ultimate by its ID, other devices by their MAC
  address (also compared when there is an ID, in case it was changed). Breadbin moves the device together with the
  folders scanned on it and its password when you click “Search again” on an unreachable device or run “Search
  network” (it never searches on its own). / Geräte, die vom Router eine neue IP-Adresse bekommen, findet Breadbin
  wieder: den Ultimate über seine Kennung, andere Geräte über die MAC-Adresse (auch bei vorhandener Kennung, falls sie
  geändert wurde). Breadbin zieht das Gerät samt der darauf gescannten Ordner und dem Passwort um, wenn du am nicht
  erreichbaren Gerät „Neu suchen“ klickst oder „Im Netz suchen“ auslöst (von selbst sucht Breadbin nicht).
- Search: new setting “Start search: while typing / only on Enter” (helpful for very large collections or slow
  computers). The search now runs in the background on its own database connection, so typing no longer freezes the
  window, and a new search cancels the one still running. Searching descriptions and disk directories is faster
  (about half the time for short search terms with ~90,000 files). / Suche: neue Einstellung „Suche starten: beim
  Tippen / erst mit Enter“ (hilft bei sehr großen Sammlungen oder langsamen Rechnern). Die Suche läuft jetzt im
  Hintergrund über eine eigene Datenbankverbindung, das Fenster hängt beim Tippen nicht mehr, und eine neue Suche
  bricht die noch laufende ab. Die Suche in Beschreibungen und Disk-Verzeichnissen ist schneller (bei kurzen
  Suchwörtern und ~90 000 Dateien etwa die halbe Zeit).
- Games from collections in the GameBase64 folder style (`NAME_NNNNN_VV`, e.g. `LCP_04400_02.zip`): the GameBase64 button
  in the details opens the game's page on gb64.com directly instead of a search. Only a link – no GB64 data is read. /
  Spiele aus Sammlungen im GameBase64-Ordnerstil (`NAME_NNNNN_VV`, z. B. `LCP_04400_02.zip`): Der Knopf „GameBase64“ in
  den Details öffnet direkt die Seite des Spiels auf gb64.com statt einer Suche. Nur ein Link – Daten aus GB64 werden
  nicht gelesen.
- BMX (Bare Metal X on Raspberry Pi 4/5/400) as a fourth kind of device: upload and start all formats including TAP via
  the Remote API, disk swap without a restart, screenshots and automatic title pictures, saved games, copying, network
  search (UDP broadcast on all network interfaces, plus HTTP), optional password. Tested by the BMX developer under Linux
  (search, start, disk swap, screenshots). / BMX (Bare Metal X auf Raspberry Pi 4/5/400) als vierte Geräteart: alle
  Formate einschließlich TAP über die Remote API hochladen und starten, Diskwechsel ohne Neustart, Bildschirmfotos und
  automatisches Titelbild, Spielstände, Kopieren, Suche im Netz (UDP-Broadcast an alle Netzwerkverbindungen, dazu HTTP),
  Passwort möglich. Vom BMX-Entwickler unter Linux geprüft (Suche, Start, Diskwechsel, Bildschirmfotos).
- New button “Stop game” (■) next to each device that is switched on: resets the C64 without starting anything new –
  also ends a running cartridge on the Ultimate and BMX. / Neuer Knopf „Spiel beenden“ (■) an jedem eingeschalteten
  Gerät: setzt den C64 zurück, ohne etwas Neues zu starten – beendet auf Ultimate und BMX auch ein laufendes Modul.

## 0.9.4 (2026-11-23)

- After a scan, a message lists which files are new and which are missing since the last scan (up to 500 each per
  source). / Nach einem Scan nennt eine Meldung, welche Dateien seit dem letzten Scan neu sind und welche fehlen
  (je Quelle bis zu 500).
- Games that need a preloaded REU (e.g. Myst 64REU): if a `.reu` file with the same name lies next to the game, Breadbin
  loads it on start – in VICE and Denise, and on the Ultimate (REU and Command Interface are switched on for the
  session only, the earlier settings come back with the next start without a REU image). / Spiele, die ein vorgeladenes
  REU brauchen (z. B. Myst 64REU): Liegt neben dem Spiel eine `.reu` mit gleichem Namen, lädt Breadbin sie beim Start
  mit – in VICE und Denise sowie auf dem Ultimate (REU und Command Interface werden nur vorübergehend eingeschaltet, die
  vorherigen Einstellungen kommen beim nächsten Start ohne REU-Abbild zurück).
- The load address of PRG files is only shown when it differs from the usual $0801. / Die Ladeadresse von PRG-Dateien
  steht nur noch da, wenn sie vom üblichen $0801 abweicht.
- The list shows all results (more rows load while scrolling) instead of stopping at 1,000. / Die Liste zeigt alle
  Treffer (weitere Zeilen beim Blättern) statt bei 1 000 aufzuhören.
- Up to four own in-game pictures per game (shared as `more_images`, which older versions skip). / Bis zu vier eigene
  Spielszenen je Spiel (geteilt als `more_images`, das ältere Fassungen überspringen).
- Extra emulator options per game, e.g. `-ntsc` or `-reu`. / Zusätzliche Emulator-Optionen je Spiel, etwa `-ntsc` oder
  `-reu`.
- The Denise emulator works as well: “Find emulator” finds it, and Breadbin passes just the file (Denise does not know
  VICE's `-autostart`). / Der Emulator Denise geht ebenfalls: „Emulator suchen“ findet ihn, und Breadbin übergibt nur
  die Datei (Denise kennt VICEs `-autostart` nicht).
- Several emulators can be set up (up to eight); each appears in the device selection of the details. Per-game options
  are kept separately for each kind of emulator. / Mehrere Emulatoren lassen sich einrichten (bis zu acht); jeder
  erscheint in der Geräteauswahl der Details. Optionen je Spiel gelten getrennt je Art Emulator.
- 1541 Ultimate II, II+ and II+L are recognised by their product name: screenshot and automatic title picture are hidden,
  the ⚠ hint says what works (still untested). / 1541 Ultimate II, II+ und II+L werden am Produktnamen erkannt:
  Bildschirmfoto und automatisches Titelbild sind ausgeblendet, das ⚠ nennt, was geht (weiter ungetestet).
- BMC64 on the Raspberry Pi as a second kind of device: upload and start games via its web interface (all formats
  including TAP), scan folders on the Pi, copy games, fetch saved games, network search, PIN. / BMC64 auf dem
  Raspberry Pi als zweite Geräteart: Spiele über dessen Weboberfläche hochladen und starten (alle Formate einschließlich
  TAP), Ordner auf dem Pi scannen, Spiele kopieren, Spielstände holen, Suche im Netz, PIN.
- TeensyROM as a third kind of device (**untested**, built from the protocol documentation without a real device):
  upload and start PRG, P00, T64, CRT and single-file disks over TCP port 2112, scan folders, copy games. /
  TeensyROM als dritte Geräteart (**ungetestet**, nach der Protokoll-Doku ohne echtes Gerät gebaut): PRG, P00, T64,
  CRT und Disketten mit einer Programmdatei über TCP-Port 2112 hochladen und starten, Ordner scannen, Spiele kopieren.

## 0.9.3 (2026-11-20)

- Short version history in the About dialog. / Kurzer Versionsverlauf im Dialog „Über Breadbin“.
- New version check at start every six hours at most (was once a day), plus "Check for a new version" in the About
  dialog. / Prüfung auf eine neue Fassung beim Start höchstens alle sechs Stunden (bisher einmal am Tag), dazu
  „Nach neuer Fassung suchen“ im Dialog „Über Breadbin“.

## 0.9.2 (2026-11-20)

- Launch games in an emulator on your computer (VICE `x64sc` by default, other emulators via their own command
  line): Settings → Emulator, then pick the emulator in the device selection of the details. Works with all formats
  including TAP, also from ZIP archives, network drives and an Ultimate's stick; saves stay in Breadbin's copy, disk
  sets get a VICE flip list.
- Spiele in einem Emulator auf dem Rechner starten (Standard VICE `x64sc`, andere Emulatoren über eine eigene
  Aufrufzeile): Einstellungen → Emulator, dann in den Details den Emulator in der Geräteauswahl wählen. Geht mit allen
  Formaten einschließlich TAP, auch aus ZIP-Archiven, von Netzlaufwerken und vom Stick eines Ultimate; Spielstände
  bleiben in Breadbins Kopie, Diskettenreihen bekommen eine VICE-Wechselliste.
- Portable Windows version (`Breadbin_…_x64-portable.zip`): with the folder `BreadbinData` next to `Breadbin.exe`,
  database, pictures, settings and the WebView data stay there; game folders on the stick follow a changed drive
  letter. / Tragbare Windows-Fassung (`Breadbin_…_x64-portable.zip`): Mit dem Ordner `BreadbinData` neben
  `Breadbin.exe` bleiben Datenbank, Bilder, Einstellungen und die WebView-Daten dort; Spieleordner auf dem Stick
  folgen einem geänderten Laufwerksbuchstaben.
- "Find VICE" also searches Downloads, Desktop, Documents and the root of every drive on Windows (VICE only comes as
  a ZIP there), and user Flatpak, Snap and `~/.local/bin` on Linux. / „VICE suchen“ sucht unter Windows auch in
  Downloads, Desktop, Dokumente und in der Wurzel aller Laufwerke (VICE gibt es dort nur als ZIP), unter Linux auch
  in Benutzer-Flatpak, Snap und `~/.local/bin`.
- German texts: remaining wrong gender for the Ultimate fixed. / Deutsche Texte: letzte „die Ultimate“ korrigiert.

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
