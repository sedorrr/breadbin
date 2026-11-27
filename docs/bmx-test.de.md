# BMX in Breadbin – Prüfliste (Entwurf, ungetestete Vorschau)

Stand 2026-11-23. Nicht in Release 0.9.4, nur vorbereitet (Branch `bmx-preview`). Gebaut ohne Gerät nach dem
BMX-Quelltext und den Antworten des BMX-Entwicklers; Code in `core/src/bmx.rs` (verbliebene Annahmen als `ANNAHME:`
markiert), Start-Ablauf in `core/src/launch.rs` (`launch_bmx`). English version: `bmx-test.md`.

Vorbereitung: BMX mit Netzwerk, „Remote API“ an, Maschine C64. Ohne Oberfläche: `c64scan --probe <ip>`,
`c64scan <db> --launch <ip> /USER/disks/Breadbin <suchtext>`, `c64scan <db> --screenshot <ip> bild.png [voll]`.

1. **Erkennung/Passwort:** „Verbinden und prüfen“ erkennt BMX über `GET /state`; mit Passwort am Gerät kommt ohne Eingabe ein Hinweis.
2. **Netzsuche:** UDP 6464 (Broadcast) und HTTP-Suche – „Im Netz suchen“ sollte BMX schon mit eingeschalteter Remote API finden.
3. **Startordner:** Vorschlag `/USER/disks/Breadbin`, wenn `/storage` USER als eingebunden meldet, sonst `/SYS/disks/Breadbin`
   (Module nach `…/carts/Breadbin`, Bänder nach `…/tapes/Breadbin`). Mit und ohne USER-Partition probieren.
4. **Starten** je einmal: PRG, P00, T64, D64, D71, D81, G64, TAP, CRT (auch ein 1-MB-EasyFlash).
5. **Modul abziehen:** Vor jedem Nicht-CRT-Start `media.cartridge.detach` – startet danach das neue Spiel?
6. **Einlegen ohne Neustart** (Laufwerk 8) mitten im Spiel; zurück zu Seite 1 (Datei ist eingelegt → kein neuer Upload).
7. **Bildschirmfoto/automatisches Titelbild:** Breadbin fragt zuerst mit `stage=emulator` und bei 400 noch einmal ohne.
   Sitzt der Zuschnitt auf 320 × 200 (auch mit schwarzem Rahmen, CRT-Effekten, NTSC, offenem Menü)?
8. **Spielstände holen** und **Startordner leeren** (eingelegte Dateien kann BMX nicht löschen → Meldung verständlich?).
9. **Freier Platz:** Vor dem Hochladen vergleicht Breadbin die Dateigröße mit `free_bytes` aus `/storage` – Meldung verständlich, wenn voll?
10. **Scan** eines Ordners auf dem BMX (`bmx://<ip>/USER/disks/…`, Seiten zu 16): Dauer bei 1000+ Dateien, Umlaute.
11. **Statusabfrage** (`/state`) stört ein laufendes Spiel nicht?

## Vom BMX-Entwickler beantwortet (2026-11-23)
- `/bmx/api/v1` bleibt kompatibel; neue Endpunkte können dazukommen.
- Die UDP-Suche läuft, sobald Remote API oder Developer-API an ist.
- Fremdprogramme dürfen über `/files` und `/file` auflisten, herunterladen und löschen (nur Medienordner).
- USER, wenn eingebunden, sonst SYS (gibt es immer; SD ist ein anderer Name für SYS).
- `screenshot.ppm` ist das ganze zusammengesetzte Anzeigebild; `stage=emulator` ist geplant.

## Noch offen
- TAP/G64/CRT per `media.autostart`; Module aus `carts/`, Bänder aus `tapes/` – am Gerät zu prüfen.
