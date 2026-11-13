# Breadbin – C64 Game Library

*Deutsch: [README.de.md](README.de.md)*

A project by [8bitcavern.de](https://8bitcavern.de). Contact: breadbin@8bitcavern.de

Breadbin is a free desktop app for Windows, macOS and Linux that keeps track of your Commodore 64
games: it catalogs local folders, network drives and the USB sticks of Ultimate devices, shows what
is stored where, adds descriptions and pictures, and starts games directly on an Ultimate 64,
a C64 Ultimate or a 1541 Ultimate II+.

The app speaks English and German (following the system language, switchable in the settings) and
has a built-in manual (“?” button or F1).

## Features

**Catalog**
- Sources: local folders, mounted network drives and folders on Ultimate devices (read via FTP)
- Formats: PRG, P00, CRT, D64, D71, D81, G64, T64, TAP – also inside ZIP archives
- Disk directories (D64/D71/D81) and T64 contents are read and searchable; cartridge type for CRT
  (EasyFlash, Magic Desk, Ocean …), load address for PRG
- Title, year and publisher from file names (TOSEC patterns and variants)
- CRC32, MD5 and SHA-1 per file; identical files in several places show up once with all locations
- Rescans only read changed files (43,859 files: first run 80 s, afterwards 1.6 s); an unreachable
  network drive never deletes entries
- Quick scan for device sticks: files with the same name and size as in another source are taken
  over instead of downloaded
- Groups versions of the same game (D64, CRT, cracks …) into one row; filters, favourites,
  “recently started”, sortable columns, CSV export
- Your own lists, rating, “completed” and notes per game; back up and restore the whole library

**Descriptions and pictures**
- **Your own details come first:** title, year, publisher, developer, genre, players, description
  and your own pictures (title screen, in-game shot, box) – for one version or all versions of a game
- **Screenshot** from an Ultimate 64 / C64 Ultimate via its video stream (Ethernet only)
- **Community:** share the details you wrote yourself as an exchange file and read other people’s
  files – matched by checksum, no account needed. Shared details are licensed under CC BY-SA 4.0.
  “Update community data” loads the shared community database from 8bitcavern.de.
- **Privacy:** at most once a day the app checks 8bitcavern.de for a new version. The server only counts these
  requests per day and app version (plus package downloads); no IP addresses, IDs or cookies are stored.
- **ScreenScraper.fr (optional):** descriptions, genres, ratings and pictures, matched by checksum
  (with your own free account); matches that only fit by name are marked “please check”.
  Several requests at once if your account allows it.

**Ultimate devices**
- Start games: PRG and CRT directly, disk images are inserted and started with a small loader
  (`LOAD"*",8,1` + `RUN`), P00/T64 are converted; files from elsewhere are copied to the device first
- Insert a disk without restarting (disk swap during a game); fetch saved games from changed disks
- Copy marked games to a folder on the device, to find them in the Ultimate menu without a computer
- Device status, switch on via Wake-on-WiFi, switch off, search the network, browse device folders

## Download and first start

Packages are on the [releases page](../../releases): macOS (universal DMG for Apple Silicon and
Intel), Windows (MSI or EXE installer) and Linux (AppImage, deb, rpm).

Breadbin is not signed with a paid certificate, so the systems warn on the first start:

- **macOS:** the DMG contains `Zuerst lesen - Read me first.txt`. In short: close the message
  “Breadbin” not opened with **Done**, then (macOS 15 and later) open System Settings → Privacy &
  Security, scroll down and click **Open Anyway**; up to macOS 14, right-click the app → **Open**.
  Afterwards allow access to the **local network**, otherwise no Ultimate device can be reached.
- **Windows:** SmartScreen “Windows protected your PC” → **More info** → **Run anyway**.
- **Linux:** make the AppImage executable (`chmod +x Breadbin_*.AppImage`) or install the deb/rpm.

## Tested with

- C64 Ultimate (firmware 1.1.0) and Ultimate 64 Elite II (firmware 3.15a): starting PRG, T64, D64,
  D71, D81, G64 and CRT, disk swap, screenshots, scanning the USB stick
- Ultimate 64 Elite I (firmware 3.15a): starting PRG, D64, D71, D81, G64 and CRT
- macOS, Windows and Linux (AppImage on Fedora)
- Not tested yet: 1541 Ultimate II+. Tape images (TAP) are catalogued but cannot be started yet.

## License

Breadbin is free software under the GNU General Public License, version 3 or later
(`GPL-3.0-or-later`, see [LICENSE](LICENSE)). Details shared through Breadbin are licensed under
CC BY-SA 4.0. Descriptions and pictures from ScreenScraper.fr are subject to ScreenScraper’s own terms
and are not distributed with the app.

## For developers

| Folder | Contents |
| --- | --- |
| `core/` | Rust library without UI: formats, scanner, SQLite database, FTP, ScreenScraper, sharing, starting on Ultimate devices; `c64scan` for the command line |
| `core/c64/` | Loader for disk images (64tass source and built PRG) |
| `src-tauri/` | Tauri 2 app: commands for the UI, configuration, icons |
| `ui/` | User interface (HTML, CSS, JavaScript without a build step) |
| `packaging/` | Extra files for the packages (macOS “read me first”) |
| `docs/` | Guides (German) |

The database lives in the app’s data folder (macOS:
`~/Library/Application Support/de.dersedor.breadbin/spiele.sqlite`), pictures next to it in `media`.

Requirements: Rust (`rustup`) and the Tauri CLI (`cargo install tauri-cli --version "^2" --locked`);
on Linux also WebKitGTK 4.1 and the Tauri system packages.

```bash
cargo test
```

```bash
cd src-tauri && cargo tauri dev
```

```bash
cd src-tauri && cargo tauri build
```

Scanner without UI (also `ftp://<device>/USB0/folder`) and a device check without starting anything:

```bash
cargo run --release -p breadbin-core --bin c64scan -- test.sqlite /path/to/games
```

```bash
cargo run --release -p breadbin-core --bin c64scan -- --probe 192.0.2.64 /USB0/Breadbin
```

More `c64scan` commands (device search, wake, power off, start, screenshot, CSV) are listed at the top
of `core/src/bin/c64scan.rs`.

Packages are built by `.github/workflows/pakete.yml` (manually or for a `v*` tag, as a release draft).
The ScreenScraper developer credentials come from the repository secrets `SCREENSCRAPER_DEVID` and
`SCREENSCRAPER_DEVPASSWORD`; without them, users enter their own in the settings.

Rebuild the loader (only after changing `lader.asm`):

```bash
64tass --cbm-prg -a -o core/c64/lader.prg core/c64/lader.asm
```
