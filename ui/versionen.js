// Versionsverlauf für „Über Breadbin“ – kurz und stichpunktartig, ab 0.9.1 (Basis).
// Neue Fassung oben anfügen: { v, date, de: [...], en: [...] }.
const VERSIONS = [
  {
    v: "0.9.6", date: "2026-11-27",
    de: ["Vor dem Einsenden: Hinweise auf fehlende Beschreibung, Bild oder Art"],
    en: ["Before submitting: hints on missing description, picture or kind"],
  },
  {
    v: "0.9.5", date: "2026-11-27",
    de: ["BMX (Raspberry Pi 4/5/400) als Gerät", "Knopf „Spiel beenden“ an jedem Gerät",
      "Geräte mit neuer IP-Adresse wiederfinden („Neu suchen“)", "VICE: Diskwechsel und Bildschirmfoto",
      "Demos, Intros, Musik u. a. neben Spielen", "Suche erst mit Enter, schneller und ohne Hänger",
      "Statistik, Startzähler, Herkunft der Gemeinschaftsdaten"],
    en: ["BMX (Raspberry Pi 4/5/400) as a device", "“Stop game” button on every device",
      "Find devices again after an IP change (“Search again”)", "VICE: disk swap and screenshots",
      "Demos, intros, music and more besides games", "Search on Enter, faster and without freezes",
      "Statistics, launch counter, origin of community data"],
  },
  {
    v: "0.9.4", date: "2026-11-23",
    de: ["BMC64 (Raspberry Pi) als Gerät", "TeensyROM als Gerät (ungetestet)", "Mehrere Emulatoren, auch Denise",
      "REU-Abbild beim Start mitladen", "Scan meldet neue und fehlende Dateien", "Liste ohne 1000er-Grenze",
      "Bis zu vier Spielszenen, Emulator-Optionen je Spiel"],
    en: ["BMC64 (Raspberry Pi) as a device", "TeensyROM as a device (untested)", "Several emulators, Denise too",
      "Preload a REU image on start", "Scan reports new and missing files", "List without the 1,000 limit",
      "Up to four in-game pictures, emulator options per game"],
  },
  {
    v: "0.9.3", date: "2026-11-20",
    de: ["Neue Fassung: Prüfung alle 6 Stunden", "Knopf „Nach neuer Fassung suchen“", "Versionsverlauf hier im Dialog"],
    en: ["New version: checked every 6 hours", "“Check for a new version” button", "Version history in this dialog"],
  },
  {
    v: "0.9.2", date: "2026-11-20",
    de: ["Spiele im Emulator (VICE) starten", "Tragbare Windows-Fassung (ZIP)", "VICE-Suche erweitert", "Textkorrekturen"],
    en: ["Launch games in an emulator (VICE)", "Portable Windows version (ZIP)", "Wider VICE search", "Text fixes"],
  },
  {
    v: "0.9.1", date: "2026-11-13",
    de: ["Basis: öffentliche Beta", "Handbuch auch auf der Webseite"],
    en: ["Base: public beta", "Manual also on the website"],
  },
];
