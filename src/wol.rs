//! Wake-on-LAN: turning on a powered-off PC over the network (e.g. the BC-250 as a Steam
//! Machine). This PC shows the wired network cards and enables WoL on them (the service
//! reapplies it every boot); the other PC's panel sends the "magic packet" to the saved MAC.

use crate::util::{read_trim, sys};
use std::net::UdpSocket;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Nic {
    pub iface: String,
    pub mac: String,
    /// "g" = wakes on magic packet; "d" = off; None = could not be read (no ethtool/root).
    pub wol: Option<String>,
    pub supports: Option<String>,
    pub up: bool,
}

/// Real wired network cards (they have a device; not a bridge, VPN or wifi).
pub fn nics() -> Vec<Nic> {
    let Ok(rd) = std::fs::read_dir(sys("class/net")) else {
        return vec![];
    };
    let mut v: Vec<Nic> = rd
        .flatten()
        .filter_map(|e| {
            let iface = e.file_name().to_string_lossy().to_string();
            let d = e.path();
            if !d.join("device").exists()
                || d.join("wireless").exists()
                || d.join("phy80211").exists()
            {
                return None;
            }
            if read_trim(d.join("type")).as_deref() != Some("1") {
                return None;
            }
            let mac = read_trim(d.join("address"))?;
            let (wol, supports) = ethtool_wol(&iface);
            Some(Nic {
                up: read_trim(d.join("operstate")).as_deref() == Some("up"),
                iface,
                mac,
                wol,
                supports,
            })
        })
        .collect();
    v.sort_by(|a, b| b.up.cmp(&a.up).then(a.iface.cmp(&b.iface)));
    v
}

pub fn parse_ethtool(s: &str) -> (Option<String>, Option<String>) {
    let mut wol = None;
    let mut sup = None;
    for l in s.lines() {
        let l = l.trim();
        if let Some(v) = l.strip_prefix("Supports Wake-on:") {
            sup = Some(v.trim().to_string());
        } else if let Some(v) = l.strip_prefix("Wake-on:") {
            wol = Some(v.trim().to_string());
        }
    }
    (wol, sup)
}

fn ethtool_wol(iface: &str) -> (Option<String>, Option<String>) {
    Command::new("ethtool")
        .arg(iface)
        .output()
        .ok()
        .map(|o| parse_ethtool(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or((None, None))
}

/// Enables WoL (magic packet) on the card. Needs root (the service does it).
pub fn enable(iface: &str) -> Result<(), String> {
    if !iface
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        return Err(crate::i18n::tr("invalid interface name").into());
    }
    let o = Command::new("ethtool")
        .args(["-s", iface, "wol", "g"])
        .output()
        .map_err(|e| format!("ethtool: {e} (sudo pacman -S ethtool)"))?;
    if !o.status.success() {
        return Err(format!(
            "ethtool: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    // NetworkManager: stores it in the connection so it does not turn it off again.
    if let Ok(o) = Command::new("nmcli")
        .args(["-g", "NAME,DEVICE", "con", "show", "--active"])
        .output()
    {
        for l in String::from_utf8_lossy(&o.stdout).lines() {
            if let Some((name, dev)) = l.rsplit_once(':')
                && dev == iface
            {
                let _ = Command::new("nmcli")
                    .args([
                        "con",
                        "modify",
                        &name.replace("\\:", ":"),
                        "802-3-ethernet.wake-on-lan",
                        "magic",
                    ])
                    .output();
            }
        }
    }
    Ok(())
}

/// "aa:bb:cc:dd:ee:ff" -> bytes
pub fn parse_mac(s: &str) -> Option<[u8; 6]> {
    let p: Vec<u8> = s
        .split([':', '-'])
        .map(|x| u8::from_str_radix(x, 16))
        .collect::<Result<_, _>>()
        .ok()?;
    p.try_into().ok()
}

pub fn magic_packet(mac: [u8; 6]) -> Vec<u8> {
    let mut v = vec![0xFF; 6];
    for _ in 0..16 {
        v.extend_from_slice(&mac);
    }
    v
}

/// Sends the magic packet to the local network (broadcast, ports 9 and 7) and, if known, to the
/// broadcast of the other PC's IP subnet.
pub fn wake(mac: &str, ip_hint: Option<&str>) -> Result<(), String> {
    let m =
        parse_mac(mac).ok_or_else(|| crate::tf!("invalid MAC: {mac}", "MAC inválido: {mac}"))?;
    let pkt = magic_packet(m);
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    s.set_broadcast(true).map_err(|e| e.to_string())?;
    let mut targets = vec!["255.255.255.255".to_string()];
    if let Some(ip) = ip_hint
        && let Some((a, _)) = ip.rsplit_once('.')
        && ip.split('.').count() == 4
    {
        targets.push(format!("{a}.255"));
    }
    let mut sent = false;
    for t in &targets {
        for port in [9, 7] {
            sent |= s.send_to(&pkt, format!("{t}:{port}")).is_ok();
        }
    }
    if sent {
        Ok(())
    } else {
        Err(crate::i18n::tr("could not send the packet (network?)").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_and_ethtool() {
        let m = parse_mac("aa:BB:cc:00:11:22").unwrap();
        let p = magic_packet(m);
        assert_eq!(p.len(), 102);
        assert_eq!(&p[..6], &[0xFF; 6]);
        assert_eq!(&p[96..], &m);
        assert!(parse_mac("aa:bb").is_none());
        let (w, s) =
            parse_ethtool("Settings for enp3s0:\n\tSupports Wake-on: pumbg\n\tWake-on: d\n");
        assert_eq!((w.as_deref(), s.as_deref()), (Some("d"), Some("pumbg")));
    }
}
