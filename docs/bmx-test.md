# BMX in Breadbin – test checklist (draft, untested preview)

As of 2026-11-23. Not part of release 0.9.4, only prepared (branch `bmx-preview`). Built without a device from the BMX
source code and the BMX developer's answers; code in `core/src/bmx.rs` (remaining assumptions marked `ANNAHME:` =
"assumption"), launch flow in `core/src/launch.rs` (`launch_bmx`). Deutsche Fassung: `bmx-test.de.md`.

Setup: BMX with networking, "Remote API" on, machine C64. Without the UI: `c64scan --probe <ip>`,
`c64scan <db> --launch <ip> /USER/disks/Breadbin <search>`, `c64scan <db> --screenshot <ip> shot.png [voll]`.

1. **Detection/password:** "Connect and check" detects BMX via `GET /state`; with a device password and none entered, a hint appears.
2. **Network search:** UDP 6464 (broadcast) plus HTTP sweep – "Search network" should find BMX with only the Remote API on.
3. **Launch folder:** suggested `/USER/disks/Breadbin` if `/storage` reports USER as mounted, otherwise `/SYS/disks/Breadbin`
   (cartridges to `…/carts/Breadbin`, tapes to `…/tapes/Breadbin`). Try a card with and without a USER partition.
4. **Launch** once each: PRG, P00, T64, D64, D71, D81, G64, TAP, CRT (also a 1 MB EasyFlash).
5. **Cartridge removal:** before every non-CRT launch Breadbin sends `media.cartridge.detach` – does the new game start?
6. **Insert without restart** (drive 8) during a game; back to side 1 (file is inserted → no new upload).
7. **Screenshot/automatic title picture:** Breadbin asks for `stage=emulator` first and repeats without it on 400. Is the
   crop to 320 × 200 right (also with black border, CRT effects, NTSC, menu open)?
8. **Fetch saved games** and **empty launch folder** (BMX cannot delete inserted files → is the message clear?).
9. **Free space:** before uploading Breadbin compares the file size with `free_bytes` from `/storage` – clear message when full?
10. **Scan** of a folder on the BMX (`bmx://<ip>/USER/disks/…`, pages of 16): duration with 1000+ files, non-ASCII names.
11. **Status polling** (`/state`) does not disturb a running game?

## Answered by the BMX developer (2026-11-23)
- `/bmx/api/v1` stays compatible; new endpoints may be added.
- UDP discovery runs whenever the Remote API or the developer API is on.
- Third-party programs may list, download and delete via `/files` and `/file` (media folders only).
- USER if mounted, otherwise SYS (always there; SD is an alias of SYS).
- `screenshot.ppm` is the whole composed display; `stage=emulator` is planned.

## Still open
- TAP/G64/CRT via `media.autostart`; cartridges from `carts/`, tapes from `tapes/` – to be checked on the device.
