//! Bildschirmfoto von einer Ultimate (Ultimate 64, C64 Ultimate) über ihren Videostrom.
//!
//! Die Ultimate schickt das VIC-Bild per UDP: 780 Byte je Paket, 12 Byte Kopf
//! (`<HHHHBBH`: Folge, Bildnummer, Zeile mit Bit 15 = letztes Paket, Breite, Zeilen je Paket,
//! Bits je Punkt, Kodierung) und 4 Zeilen à 384 Punkte mit 4 Bit (unteres Halbbyte zuerst).
//! Die Werte sind VIC-Farbnummern. Die Ultimate streamt nur an ein Ziel: Ein Bildschirmfoto
//! nimmt anderen Empfängern den Strom weg.

use crate::launch::Rest;
use crate::tr;
use anyhow::{Result, bail};
use std::net::{IpAddr, UdpSocket};
use std::time::{Duration, Instant};

const WIDTH: usize = 384;
const HEADER: usize = 12;
const PACKET: usize = HEADER + WIDTH / 2 * 4;

/// Pepto-Palette (VIC-Farbnummer → RGB).
const PALETTE: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0xFF, 0xFF, 0xFF],
    [0x68, 0x37, 0x2B],
    [0x70, 0xA4, 0xB2],
    [0x6F, 0x3D, 0x86],
    [0x58, 0x8D, 0x43],
    [0x35, 0x28, 0x79],
    [0xB8, 0xC7, 0x6F],
    [0x6F, 0x4F, 0x25],
    [0x43, 0x39, 0x00],
    [0x9A, 0x67, 0x59],
    [0x44, 0x44, 0x44],
    [0x6C, 0x6C, 0x6C],
    [0x9A, 0xD2, 0x84],
    [0x6C, 0x5E, 0xB5],
    [0x95, 0x95, 0x95],
];

/// Ein Bild aus VIC-Farbnummern, 384 Punkte breit, 272 (PAL) oder 240 (NTSC) Zeilen hoch.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub height: usize,
    pub pixels: Vec<u8>,
}

/// Sammelt die Pakete eines Bildes; fertig, sobald ein Bild mit Endmarke lückenlos vorliegt.
#[derive(Default)]
pub struct Collector {
    frame: Option<u16>,
    rows: Vec<Option<[u8; WIDTH / 2]>>,
    last_line: Option<usize>,
}

impl Collector {
    /// Nimmt ein Paket auf; gibt das fertige Bild zurück, wenn es vollständig ist.
    pub fn push(&mut self, pkt: &[u8]) -> Option<Frame> {
        if pkt.len() != PACKET {
            return None;
        }
        let u16_at = |i: usize| u16::from_le_bytes([pkt[i], pkt[i + 1]]);
        let (frame, line_raw, width, lines, bits) = (
            u16_at(2),
            u16_at(4),
            u16_at(6) as usize,
            pkt[8] as usize,
            pkt[9],
        );
        if width != WIDTH || lines != 4 || bits != 4 {
            return None;
        }
        if self.frame != Some(frame) {
            *self = Collector {
                frame: Some(frame),
                ..Default::default()
            };
        }
        let line = (line_raw & 0x7FFF) as usize;
        if line + lines > 400 {
            return None;
        }
        if self.rows.len() < line + lines {
            self.rows.resize(line + lines, None);
        }
        for (i, row) in pkt[HEADER..]
            .as_chunks::<{ WIDTH / 2 }>()
            .0
            .iter()
            .enumerate()
        {
            self.rows[line + i] = Some(*row);
        }
        if line_raw & 0x8000 != 0 {
            self.last_line = Some(line + lines);
        }
        let height = self.last_line?;
        if self.rows.len() < height || self.rows[..height].iter().any(Option::is_none) {
            return None;
        }
        let mut pixels = Vec::with_capacity(WIDTH * height);
        for row in self.rows[..height].iter().flatten() {
            for b in row {
                pixels.push(b & 0x0F);
                pixels.push(b >> 4);
            }
        }
        Some(Frame { height, pixels })
    }
}

/// Schaltet den Videostrom ein, wartet auf ein vollständiges Bild und schaltet ihn wieder aus.
pub fn capture(host: &str, password: &str, timeout: Duration) -> Result<Frame> {
    let rest = Rest::new(host, password);
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    // Eigene Adresse in Richtung des Geräts (es wird nichts gesendet).
    let probe = UdpSocket::bind("0.0.0.0:0")?;
    probe.connect((host.trim(), 64))?;
    let IpAddr::V4(ip) = probe.local_addr()?.ip() else {
        bail!("keine IPv4-Adresse")
    };
    let target = format!("{ip}:{}", sock.local_addr()?.port());
    // Ein alter Strom blockiert sonst den Start.
    let _ = rest.video_stop();
    start_stream(&rest, &target)?;
    let result = receive(&sock, timeout);
    let _ = rest.video_stop();
    result
}

/// Startet den Videostrom. Die Ultimate lehnt Stream-Starts zeitweise mit „Network Host Resolve Error“
/// ab und nimmt sie nach einigen Sekunden wieder an – so lange erneut anfragen.
fn start_stream(rest: &Rest, target: &str) -> Result<()> {
    let mut tries = 0;
    loop {
        match rest.video_start(target) {
            Ok(()) => return Ok(()),
            Err(e) if format!("{e:#}").contains("Resolve") => {
                tries += 1;
                if tries == 6 {
                    bail!(tr!(
                        "Die Ultimate lehnt den Videostrom gerade ab („Network Host Resolve Error“). Später erneut versuchen; hilft das nicht, das Gerät aus- und wieder einschalten.",
                        "The Ultimate is refusing the video stream right now (“Network Host Resolve Error”). Try again later; if that does not help, switch the device off and on again."
                    ));
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            Err(e) => return Err(e),
        }
    }
}

fn receive(sock: &UdpSocket, timeout: Duration) -> Result<Frame> {
    sock.set_read_timeout(Some(Duration::from_millis(250)))?;
    let start = Instant::now();
    let mut buf = [0u8; 2048];
    let mut collector = Collector::default();
    // Das erste Bild kann mittendrin beginnen; der Sammler verwirft es von selbst.
    while start.elapsed() < timeout {
        match sock.recv(&mut buf) {
            Ok(n) => {
                if let Some(f) = collector.push(&buf[..n]) {
                    return Ok(f);
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(e.into()),
        }
    }
    bail!(tr!(
        "Kein vollständiges Bild vom Gerät empfangen. Die Ultimate schickt den Videostrom nur über Ethernet – ist sie per Netzwerkkabel verbunden? Sonst blockiert womöglich eine Firewall eingehende UDP-Pakete.",
        "No complete picture received from the device. The Ultimate sends the video stream over Ethernet only – is it connected with a network cable? Otherwise a firewall may be blocking incoming UDP packets."
    ))
}

/// Wandelt das Bild in ein PNG; `crop` schneidet den Rahmen ab (320 × 200, nur der Bildschirm).
pub fn to_png(frame: &Frame, crop: bool) -> Result<Vec<u8>> {
    // Sichtbarer Bildschirm im Strom: PAL ab (32, 35), NTSC ab (32, 20).
    let (x0, y0, w, h) = if crop {
        (32, if frame.height >= 272 { 35 } else { 20 }, 320, 200)
    } else {
        (0, 0, WIDTH, frame.height)
    };
    anyhow::ensure!(y0 + h <= frame.height, "Bild zu klein");
    let mut img = image::RgbImage::new(w as u32, h as u32);
    for (y, row) in img.rows_mut().enumerate() {
        for (x, px) in row.enumerate() {
            let c = frame.pixels[(y0 + y) * WIDTH + x0 + x] as usize;
            *px = image::Rgb(PALETTE[c & 15]);
        }
    }
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packet(frame: u16, line: u16, last: bool, color: u8) -> Vec<u8> {
        let mut p = Vec::with_capacity(PACKET);
        for v in [
            0u16,
            frame,
            line | if last { 0x8000 } else { 0 },
            WIDTH as u16,
        ] {
            p.extend_from_slice(&v.to_le_bytes());
        }
        p.extend_from_slice(&[4, 4, 0, 0]);
        p.extend(std::iter::repeat_n(color | (color << 4), WIDTH / 2 * 4));
        p
    }

    #[test]
    fn bild_wird_zusammengesetzt() {
        let mut c = Collector::default();
        // Bild 1 beginnt mittendrin und wird verworfen, Bild 2 ist vollständig (68 Pakete).
        assert!(c.push(&packet(1, 268, true, 1)).is_none());
        let mut done = None;
        for i in 0..68u16 {
            done = c.push(&packet(2, i * 4, i == 67, (i % 16) as u8));
        }
        let f = done.expect("vollständiges Bild");
        assert_eq!(f.height, 272);
        assert_eq!(f.pixels.len(), WIDTH * 272);
        assert_eq!(f.pixels[WIDTH * 4], 1, "zweites Paket hat Farbe 1");
        let png = to_png(&f, true).unwrap();
        let img = image::load_from_memory(&png).unwrap();
        assert_eq!((img.width(), img.height()), (320, 200));
        assert_eq!(
            to_png(&f, false)
                .map(|p| image::load_from_memory(&p).unwrap().height())
                .unwrap(),
            272
        );
    }

    #[test]
    fn luecke_verhindert_bild() {
        let mut c = Collector::default();
        for i in (0..68u16).filter(|&i| i != 10) {
            assert!(c.push(&packet(3, i * 4, i == 67, 0)).is_none());
        }
        assert!(
            c.push(&packet(3, 40, false, 0)).is_some(),
            "fehlendes Paket kommt nach"
        );
    }
}
