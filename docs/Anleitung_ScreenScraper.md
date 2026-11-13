# ScreenScraper einrichten

Breadbin holt Beschreibungen, Genre, Entwickler, Bewertung und Bilder
(Spielszene, Titelbild, Verpackung) von [ScreenScraper.fr](https://www.screenscraper.fr).
Die Spiele werden über ihre Prüfsummen erkannt, also auch bei kryptischen Dateinamen.

Es braucht zwei Dinge:

| Was | Wer | Wofür |
| --- | --- | --- |
| Benutzerkonto | jeder Nutzer der App | Anmeldung, eigenes Tageskontingent |
| Entwickler-Kennung (`devid`, `devpassword`) | einmal für die App, vom Entwickler | erlaubt der App den Zugriff auf die API |

## 1. Benutzerkonto anlegen (jeder Nutzer)

1. <https://www.screenscraper.fr/membreinscription.php> öffnen.
2. Benutzername, E-Mail-Adresse und Passwort eintragen und abschicken.
3. Die Bestätigungs-E-Mail öffnen und das Konto freischalten.

Ein kostenloses Konto erlaubt eine Anfrage zur selben Zeit und nach Angaben anderer
Programme rund 20 000 Anfragen am Tag. Wer ScreenScraper mit Daten oder Geld unterstützt,
bekommt mehr gleichzeitige Anfragen – für die App ist das nicht nötig.

## 2. Entwickler-Kennung beantragen (einmal, vom Entwickler der App)

ScreenScraper vergibt die Kennung über das Forum. Die API darf nur in **kostenlosen**
Programmen genutzt werden; das trifft auf Breadbin zu.

1. Mit dem eigenen Konto bei ScreenScraper anmelden.
2. Das Entwickler-Forum öffnen:
   <https://www.screenscraper.fr/forumsujets.php?frub=12&numpage=0>
3. Ein neues Thema anlegen und die App vorstellen. Das Forum ist französisch; Englisch
   wird verstanden. Am besten beide Fassungen untereinander in einen Beitrag setzen.

   **Titel:**

   > Demande d'identifiants développeur pour « Breadbin » (bibliothèque de jeux C64 gratuite) /
   > Developer credentials for "Breadbin" (free C64 game library)

   **Text (Französisch):**

   > Bonjour,
   >
   > Je développe *Breadbin – C64 Game Library*, une application de bureau gratuite pour
   > Windows, macOS et Linux. Elle répertorie les jeux Commodore 64 d'un utilisateur (D64,
   > CRT, PRG, T64, TAP …) stockés dans des dossiers locaux, sur des lecteurs réseau et sur
   > des appareils Ultimate 64 / 1541 Ultimate II+, et permet de les lancer sur ces
   > appareils. L'application sera distribuée gratuitement.
   >
   > Utilisation prévue de l'API :
   > - uniquement le système Commodore 64 (systemeid 66)
   > - jeuInfos.php avec crc, md5, sha1 et romtaille
   > - ssuserInfos.php pour vérifier la connexion et le quota
   > - téléchargement des médias capture d'écran (ss), écran titre (sstitle) et boîte 2D (box-2D)
   >
   > Les requêtes sont strictement séquentielles (un seul thread), avec au moins 1,2 s
   > entre deux requêtes, médias compris. Les résultats et les images sont mis en cache
   > localement ; chaque image n'est téléchargée qu'une fois par jeu. Les erreurs de quota
   > (430/431) arrêtent le traitement. Chaque utilisateur se connecte avec son propre compte
   > ScreenScraper, et l'application indique ScreenScraper comme source des données.
   >
   > Nom du logiciel (softname) : `Breadbin` (envoyé sous la forme `Breadbin-<version>`)
   >
   > Pourriez-vous me fournir un devid et un devpassword ? Merci beaucoup pour votre
   > travail sur ScreenScraper !

   **Text (Englisch):**

   > Hello,
   >
   > I am developing *Breadbin – C64 Game Library*, a free desktop application for Windows,
   > macOS and Linux. It catalogs a user's Commodore 64 games (D64, CRT, PRG, T64, TAP …)
   > stored in local folders, on network drives and on Ultimate 64 / 1541 Ultimate II+
   > devices, and can start them on these devices. It will be distributed free of charge.
   >
   > Planned API usage:
   > - Commodore 64 system only (systemeid 66)
   > - jeuInfos.php with crc, md5, sha1 and romtaille
   > - ssuserInfos.php to check the login and quota
   > - downloads of the screenshot (ss), title screen (sstitle) and 2D box (box-2D) media
   >
   > Requests are strictly sequential (one thread) with at least 1.2 s between requests,
   > including media. Results and images are cached locally; each image is downloaded only
   > once per game. Quota errors (430/431) stop the run. Every user logs in with their own
   > ScreenScraper account, and the application credits ScreenScraper as the data source.
   >
   > Software name (softname): `Breadbin` (sent as `Breadbin-<version>`)
   >
   > Could you please provide a devid and devpassword? Thank you very much for your work
   > on ScreenScraper!

4. Auf die Antwort warten. Die Kennung kommt meist als private Nachricht im Forum.

**Die Kennung ist vertraulich:** nicht ins Git-Repository schreiben, nicht öffentlich
weitergeben.

## 3. In der App eintragen

1. App starten, links bei **Beschreibungen** auf **Einstellungen** klicken.
2. **Benutzername** und **Passwort** des ScreenScraper-Kontos eintragen.
3. **Sprache der Beschreibungen** wählen. Fehlt eine Beschreibung in dieser Sprache,
   nimmt die App Englisch.
4. Nur solange die Kennung nicht fest eingebaut ist: im Feld **Entwickler-Kennung**
   `devid` und `devpassword` eintragen.
5. **Zugang prüfen** – bei Erfolg steht dort, wie viele Abfragen heute schon genutzt sind.
6. **Speichern**.

Passwörter speichert die App im Schlüsselbund des Systems (macOS-Schlüsselbund,
Windows-Anmeldeinformationsverwaltung, Secret Service unter Linux), nicht in einer Datei.
macOS fragt beim ersten Zugriff, ob die App den Schlüsselbund benutzen darf: **Immer
erlauben** wählen. Bei nicht signierten Testfassungen kann die Frage nach jedem neuen
Bau wiederkommen.

## 4. Beschreibungen holen

- **Ein Spiel:** in der Liste anklicken, rechts **Bei ScreenScraper nachschlagen**.
  Gut zum Ausprobieren.
- **Trefferliste:** erst suchen oder filtern, dann links **Für Trefferliste holen**
  (bis zu 1 000 Einträge; schon nachgeschlagene werden übersprungen).
- **Alles:** **Alle ohne Beschreibung holen**. Bei großen Sammlungen dauert das: je Datei
  mindestens 1,2 s, dazu die Bilder (je Spiel nur einmal). Ist das Tageskontingent
  erreicht, hält die App an und macht beim nächsten Mal dort weiter.

Nicht gefundene Dateien merkt sich die App und fragt sie nicht noch einmal ab; in den
Details lässt sich mit **Erneut versuchen** gezielt nachfragen. Mit dem Punkt hinter dem
Titel sind Einträge mit Beschreibung markiert.

## 5. Kennung fest in die App einbauen (für fertige Pakete)

Damit Nutzer nur noch ihr eigenes Konto eintragen müssen, wird die Entwickler-Kennung
beim Bauen über Umgebungsvariablen eingebracht:

```bash
SCREENSCRAPER_DEVID=… SCREENSCRAPER_DEVPASSWORD=… cargo tauri build
```

Für die Pakete über GitHub Actions kommen die beiden Werte als Repository-Secrets
gleichen Namens hinein. Im Programm stehen sie dann im Klartext; jeder, der sucht, kann
sie herauslesen. Andere Programme (Skyscraper, ES-DE) verschleiern sie nur ebenfalls.
