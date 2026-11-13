//! Ultimate-Geräte ein- und ausschalten, Erreichbarkeit prüfen, im Netz suchen.
//!
//! - Einschalten: Wake-on-WiFi mit einem Magic Packet an die **WLAN-MAC** (die
//!   Ethernet-MAC weckt nicht). Geht beim C64 Ultimate und Ultimate 64 Elite II, wenn am
//!   Gerät „Wake On Wi-Fi“ eingeschaltet ist. Gesendet wird an den Netz-Broadcast,
//!   an 255.255.255.255 und direkt an die IP (das ausgeschaltete Gerät beantwortet ARP).
//! - Ausschalten: `machine:poweroff`, nur Ultimate 64 / C64 Ultimate. Bricht die
//!   Verbindung dabei ab, war es vermutlich erfolgreich; ein Timeout heißt „nicht erreicht“.

use crate::tr;
use anyhow::{Result, anyhow, bail};
use serde::Serialize;
use std::net::{Ipv4Addr, UdpSocket};
use std::time::{Duration, Instant};

/// Antwortet das Gerät auf `/v1/version`? (kurzer Timeout, für die Statusanzeige)
pub fn is_online(host: &str, password: &str) -> bool {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_millis(1500)))
        .build()
        .into();
    let mut req = agent.get(format!("http://{}/v1/version", host.trim()));
    if !password.is_empty() {
        req = req.header("X-Password", password);
    }
    let Ok(mut resp) = req.call() else {
        return false;
    };
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    looks_like_ultimate(&body)
}

/// Router-Weboberflächen antworten auf jeden Pfad mit 200 – echt ist nur JSON mit
/// `version` (Text) und `errors` (Liste).
pub fn looks_like_ultimate(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .is_ok_and(|v| v["version"].is_string() && v["errors"].is_array())
}

pub fn parse_mac(mac: &str) -> Result<[u8; 6]> {
    let parts: Vec<u8> = mac
        .split([':', '-'])
        .map(|p| u8::from_str_radix(p.trim(), 16))
        .collect::<Result<_, _>>()
        .map_err(|_| {
            anyhow!(tr!(
                "MAC-Adresse „{mac}“ ist ungültig",
                "MAC address “{mac}” is invalid"
            ))
        })?;
    parts.try_into().map_err(|_| {
        anyhow!(tr!(
            "MAC-Adresse „{mac}“ ist ungültig",
            "MAC address “{mac}” is invalid"
        ))
    })
}

pub fn magic_packet(mac: [u8; 6]) -> Vec<u8> {
    let mut p = vec![0xFF; 6];
    for _ in 0..16 {
        p.extend_from_slice(&mac);
    }
    p
}

/// Sendet das Magic Packet; wartet nicht auf das Gerät.
pub fn send_wake(host: &str, wifi_mac: &str) -> Result<()> {
    let packet = magic_packet(parse_mac(wifi_mac)?);
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.set_broadcast(true)?;
    let mut targets = vec!["255.255.255.255".to_string()];
    if let Ok(ip) = host.trim().parse::<Ipv4Addr>() {
        let o = ip.octets();
        targets.push(format!("{}.{}.{}.255", o[0], o[1], o[2]));
        targets.push(ip.to_string());
    }
    let mut sent = false;
    for t in &targets {
        for port in [9, 7] {
            sent |= sock.send_to(&packet, (t.as_str(), port)).is_ok();
        }
    }
    if !sent {
        bail!(tr!(
            "Weckpaket ließ sich nicht senden",
            "Could not send the wake-up packet"
        ));
    }
    Ok(())
}

/// Weckt das Gerät und wartet, bis es antwortet (höchstens `wait`).
pub fn wake_and_wait(
    host: &str,
    wifi_mac: &str,
    password: &str,
    wait: Duration,
) -> Result<Duration> {
    let start = Instant::now();
    send_wake(host, wifi_mac)?;
    let mut resent = false;
    while start.elapsed() < wait {
        if is_online(host, password) {
            return Ok(start.elapsed());
        }
        // Einmal nachlegen, falls das erste Paket verloren ging.
        if !resent && start.elapsed() > Duration::from_secs(8) {
            send_wake(host, wifi_mac)?;
            resent = true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    bail!(tr!(
        "Gerät {host} hat sich nach {} s nicht gemeldet. Ist am Gerät „Wake On Wi-Fi“ eingeschaltet \
         und hängt es am Strom?",
        "Device {host} did not answer after {} s. Is “Wake On Wi-Fi” enabled on the device \
         and is it plugged in?",
        wait.as_secs()
    ))
}

/// Kann sich dieses Gerät per REST ausschalten? (nur Ultimate-64-Familie)
pub fn can_power_off(product: &str) -> bool {
    let p = product.to_lowercase();
    (p.contains("ultimate 64") || p.contains("ultimate-64") || p.contains("c64 ultimate"))
        && !p.contains("ii+")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum PowerOff {
    /// Gerät ist aus (antwortet nicht mehr).
    Off,
    /// Befehl angenommen, Gerät antwortet aber noch.
    StillOnline,
}

pub fn power_off(host: &str, password: &str) -> Result<PowerOff> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(8)))
        .timeout_connect(Some(Duration::from_secs(4)))
        .build()
        .into();
    let mut req = agent.put(format!("http://{}/v1/machine:poweroff", host.trim()));
    if !password.is_empty() {
        req = req.header("X-Password", password);
    }
    match req.send_empty() {
        Ok(mut resp) => {
            let status = resp.status().as_u16();
            if !(200..300).contains(&status) {
                let body = resp.body_mut().read_to_string().unwrap_or_default();
                bail!(tr!(
                    "Gerät lehnt das Ausschalten ab ({status}): {}",
                    "Device refuses to power off ({status}): {}",
                    body.trim()
                ));
            }
        }
        // Abbruch der Verbindung = Gerät schaltet ab; Timeout beim Verbinden = nicht erreicht.
        Err(ureq::Error::Timeout(t)) if format!("{t:?}").contains("Connect") => {
            bail!(tr!(
                "Gerät {host} ist nicht erreichbar – vielleicht schon aus?",
                "Device {host} is not reachable – maybe already off?"
            ))
        }
        Err(_) => {}
    }
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(700));
        if !is_online(host, password) {
            return Ok(PowerOff::Off);
        }
    }
    Ok(PowerOff::StillOnline)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Found {
    pub host: String,
    pub product: String,
    pub hostname: String,
    pub firmware: String,
    pub unique_id: String,
    pub wifi_mac: String,
    #[serde(skip)]
    pub ethernet_mac: String,
    /// Weitere Adressen desselben Geräts (LAN und WLAN), beim Entdoppeln gesammelt.
    pub other_hosts: Vec<String>,
}

/// Eigene IPv4-Adresse im lokalen Netz (ohne Pakete zu senden).
pub fn local_ipv4() -> Option<Ipv4Addr> {
    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.connect("192.0.2.1:9").ok()?; // TEST-NET, es wird nichts gesendet
    match sock.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) if !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

fn info_json(host: &str) -> Option<serde_json::Value> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_millis(1500)))
        .build()
        .into();
    let body = agent
        .get(format!("http://{host}/v1/version"))
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    if !looks_like_ultimate(&body) {
        return None;
    }
    let info = agent
        .get(format!("http://{host}/v1/info"))
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    Some(serde_json::from_str(&info).unwrap_or(serde_json::Value::Null))
}

/// Sucht im eigenen /24-Netz nach Ultimate-Geräten (bis zu 64 gleichzeitige Anfragen).
/// Geräte mit Netzwerk-Passwort antworten nicht und werden nicht gefunden.
pub fn discover() -> Result<Vec<Found>> {
    let me = local_ipv4().ok_or_else(|| {
        anyhow!(tr!(
            "Eigene Netzwerkadresse nicht ermittelbar",
            "Cannot determine this computer's network address"
        ))
    })?;
    let o = me.octets();
    let hosts: Vec<String> = (1..=254u8)
        .filter(|&i| i != o[3])
        .map(|i| format!("{}.{}.{}.{i}", o[0], o[1], o[2]))
        .collect();
    let found = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|s| {
        for chunk in hosts.chunks(4) {
            let found = &found;
            s.spawn(move || {
                for h in chunk {
                    if let Some(v) = info_json(h) {
                        let t = |k: &str| v[k].as_str().unwrap_or("").to_string();
                        found.lock().unwrap().push(Found {
                            host: h.clone(),
                            product: Some(t("product"))
                                .filter(|p| !p.is_empty())
                                .unwrap_or_else(|| "Ultimate".into()),
                            hostname: t("hostname"),
                            firmware: t("firmware_version"),
                            unique_id: t("unique_id"),
                            wifi_mac: normalize_mac(&t("wifi_mac")).unwrap_or_default(),
                            ethernet_mac: normalize_mac(&t("ethernet_mac")).unwrap_or_default(),
                            other_hosts: Vec::new(),
                        });
                    }
                }
            });
        }
    });
    let mut list = found.into_inner().unwrap();
    let macs: std::collections::HashMap<String, String> = list
        .iter()
        .filter_map(|f| arp_mac(&f.host).map(|m| (f.host.clone(), m)))
        .collect();
    let lookup = |ip: &str| macs.get(ip).cloned();
    fill_wifi_macs(&mut list, lookup);
    dedup_prefer_lan(&mut list, lookup);
    Ok(list)
}

/// Ein Gerät mit LAN und WLAN antwortet unter zwei Adressen: nach unique_id entdoppeln. Behalten wird
/// die LAN-Adresse (schneller und zuverlässiger, auch für die Datenströme), sonst die niedrigere.
fn dedup_prefer_lan(list: &mut Vec<Found>, lookup: impl Fn(&str) -> Option<String>) {
    let last_octet = |f: &Found| {
        f.host
            .rsplit('.')
            .next()
            .and_then(|n| n.parse::<u8>().ok())
            .unwrap_or(0)
    };
    let is_wlan = |f: &Found| {
        !f.wifi_mac.is_empty() && lookup(&f.host).as_deref() == Some(f.wifi_mac.as_str())
    };
    list.sort_by_key(|f| (is_wlan(f), last_octet(f)));
    let mut kept: Vec<Found> = Vec::new();
    for f in list.drain(..) {
        match kept
            .iter_mut()
            .find(|k| !f.unique_id.is_empty() && k.unique_id == f.unique_id)
        {
            Some(k) => k.other_hosts.push(f.host),
            None => kept.push(f),
        }
    }
    *list = kept;
}

/// Lokal vergebene MAC (Bit 1 im ersten Byte). Die Ultimate-Firmware gibt ihrer Ethernet-Schnittstelle so eine
/// (`02:15:41:…`); WLAN-Module (Espressif) haben eine Herstellerkennung.
fn locally_administered(mac: &str) -> bool {
    u8::from_str_radix(mac.get(..2).unwrap_or("00"), 16).is_ok_and(|b| b & 0x02 != 0)
}

/// MAC als `AA:BB:CC:DD:EE:FF`; nimmt auch `a:b:…` (macOS-ARP) und `aa-bb-…` (Windows).
pub fn normalize_mac(s: &str) -> Option<String> {
    let parts: Vec<&str> = s.trim().split([':', '-']).collect();
    if parts.len() != 6
        || parts
            .iter()
            .any(|p| p.is_empty() || p.len() > 2 || !p.chars().all(|c| c.is_ascii_hexdigit()))
    {
        return None;
    }
    Some(
        parts
            .iter()
            .map(|p| format!("{:0>2}", p.to_uppercase()))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

/// Sucht in ARP-Ausgaben (macOS/BSD `arp -n`, Linux `arp -n` oder `ip neigh`, Windows `arp -a`)
/// die MAC zur IP.
fn parse_arp(text: &str, ip: &str) -> Option<String> {
    text.lines()
        .filter(|l| {
            l.split(|c: char| c.is_whitespace() || c == '(' || c == ')')
                .any(|t| t == ip)
        })
        .find_map(|l| l.split_whitespace().find_map(normalize_mac))
}

/// MAC einer IP aus dem ARP-Speicher des Rechners (vorher muss mit der IP gesprochen worden sein).
pub fn arp_mac(ip: &str) -> Option<String> {
    let run = |prog: &str, args: &[&str]| -> Option<String> {
        let mut cmd = std::process::Command::new(prog);
        cmd.args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: kein Konsolenfenster
        }
        let out = cmd.output().ok()?;
        parse_arp(&String::from_utf8_lossy(&out.stdout), ip)
    };
    if cfg!(windows) {
        run("arp", &["-a", ip])
    } else {
        run("arp", &["-n", ip]).or_else(|| run("ip", &["neigh", "show", ip]))
    }
}

/// Der C64 Ultimate (FW 1.1.0) meldet keine `wifi_mac`. Antwortet ein Gerät unter zwei
/// Adressen (LAN und WLAN, gleiche `unique_id`), ist die MAC der Adresse, die nicht die
/// Ethernet-MAC hat, die WLAN-MAC.
fn fill_wifi_macs(list: &mut [Found], lookup: impl Fn(&str) -> Option<String>) {
    let ids: std::collections::HashSet<String> = list
        .iter()
        .filter(|f| f.wifi_mac.is_empty() && !f.unique_id.is_empty())
        .map(|f| f.unique_id.clone())
        .collect();
    for id in ids {
        let wifi = list
            .iter()
            .filter(|f| f.unique_id == id)
            // Ohne gemeldete LAN-MAC (C64 Ultimate, FW 1.1.0) gilt die MAC mit Herstellerkennung als WLAN.
            .filter_map(|f| {
                lookup(&f.host).filter(|m| {
                    if f.ethernet_mac.is_empty() {
                        !locally_administered(m)
                    } else {
                        *m != f.ethernet_mac
                    }
                })
            })
            .next();
        if let Some(mac) = wifi {
            list.iter_mut()
                .filter(|f| f.unique_id == id)
                .for_each(|f| f.wifi_mac = mac.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arp_ausgaben_und_wlan_mac() {
        assert_eq!(
            normalize_mac("4:d4:c4:4e:42:af").as_deref(),
            Some("04:D4:C4:4E:42:AF")
        );
        assert_eq!(
            normalize_mac("98-88-e0-02-34-56").as_deref(),
            Some("98:88:E0:02:34:56")
        );
        assert_eq!(normalize_mac("(incomplete)"), None);
        let mac = "? (192.0.2.246) at 98:88:e0:2:34:57 on en0 ifscope [ethernet]\n? (192.0.2.24) at 1:2:3:4:5:6 on en0";
        assert_eq!(
            parse_arp(mac, "192.0.2.246").as_deref(),
            Some("98:88:E0:02:34:57")
        );
        assert_eq!(parse_arp(mac, "192.0.2.2"), None, "nur exakte IP");
        let linux = "Address HWtype HWaddress Flags Mask Iface\n192.0.2.246 ether 98:88:e0:02:34:57 C eth0";
        assert_eq!(
            parse_arp(linux, "192.0.2.246").as_deref(),
            Some("98:88:E0:02:34:57")
        );
        let win = "Schnittstelle: 192.0.2.81 --- 0x5\n  192.0.2.246         98-88-e0-02-34-57     dynamisch";
        assert_eq!(
            parse_arp(win, "192.0.2.246").as_deref(),
            Some("98:88:E0:02:34:57")
        );

        let f = |host: &str| Found {
            host: host.into(),
            product: "C64 Ultimate".into(),
            hostname: String::new(),
            firmware: String::new(),
            unique_id: "U1".into(),
            wifi_mac: String::new(),
            ethernet_mac: "02:15:41:00:00:01".into(),
            other_hosts: Vec::new(),
        };
        let mut list = vec![f("192.0.2.103"), f("192.0.2.246")];
        fill_wifi_macs(&mut list, |ip| {
            Some(
                if ip.ends_with(".103") {
                    "02:15:41:00:00:01"
                } else {
                    "98:88:E0:00:00:02"
                }
                .into(),
            )
        });
        assert!(list.iter().all(|x| x.wifi_mac == "98:88:E0:00:00:02"));
        // Nur LAN erreichbar: WLAN-MAC bleibt unbekannt.
        let mut list = vec![f("192.0.2.103")];
        fill_wifi_macs(&mut list, |_| Some("02:15:41:00:00:01".into()));
        assert_eq!(list[0].wifi_mac, "");

        // C64 Ultimate (FW 1.1.0) meldet keine MACs: WLAN .103 (Espressif), LAN .246 (02:15:41:…).
        let arp = |ip: &str| {
            Some(
                if ip.ends_with(".103") {
                    "3C:DC:75:AB:CD:EF"
                } else {
                    "02:15:41:6E:00:D2"
                }
                .into(),
            )
        };
        let c64u = |host: &str| Found {
            ethernet_mac: String::new(),
            ..f(host)
        };
        let mut list = vec![c64u("192.0.2.103"), c64u("192.0.2.246")];
        fill_wifi_macs(&mut list, arp);
        assert!(list.iter().all(|x| x.wifi_mac == "3C:DC:75:AB:CD:EF"));
        dedup_prefer_lan(&mut list, arp);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].host, "192.0.2.246", "LAN-Adresse bevorzugt");
        assert_eq!(list[0].other_hosts, ["192.0.2.103"]);
        // Nur über LAN erreichbar und keine MACs gemeldet: nichts raten.
        let mut list = vec![c64u("192.0.2.246")];
        fill_wifi_macs(&mut list, arp);
        assert_eq!(list[0].wifi_mac, "");
        // Ohne WLAN-Kenntnis: niedrigere Adresse.
        let mut list = vec![f("192.0.2.200"), f("192.0.2.20")];
        dedup_prefer_lan(&mut list, |_| None);
        assert_eq!(list[0].host, "192.0.2.20");
    }

    #[test]
    fn magic_packet_aufbau() {
        let mac = parse_mac("98:88:E0:02:34:56").unwrap();
        assert_eq!(mac, [0x98, 0x88, 0xE0, 0x02, 0x34, 0x56]);
        let p = magic_packet(mac);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xFF; 6]);
        assert_eq!(&p[96..], &mac);
        assert!(parse_mac("98:88:E0").is_err());
        assert!(parse_mac("zz:88:E0:02:34:56").is_err());
    }

    #[test]
    fn geraete_erkennen() {
        assert!(looks_like_ultimate(
            r#"{ "version" : "0.1", "errors" : [ ] }"#
        ));
        assert!(!looks_like_ultimate("<html>Router</html>"));
        assert!(!looks_like_ultimate(r#"{"version": 1}"#));
        assert!(can_power_off("Ultimate 64-II"));
        assert!(can_power_off("C64 Ultimate"));
        assert!(!can_power_off("Ultimate-II+"));
        assert!(!can_power_off("1541 Ultimate-II+"));
    }
}
