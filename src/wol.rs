//! Wake-on-LAN: ligar um PC desligado pela rede (ex.: a BC-250 como Steam Machine). Este PC
//! mostra as placas de rede com fio e liga o WoL nelas (o serviço reaplica a cada boot); o
//! painel do outro PC manda o "pacote mágico" para o endereço MAC guardado.

use crate::util::{read_trim, sys};
use std::net::UdpSocket;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Nic {
    pub iface: String,
    pub mac: String,
    /// "g" = acorda com pacote mágico; "d" = desligado; None = não deu para ler (sem ethtool/root).
    pub wol: Option<String>,
    pub supports: Option<String>,
    pub up: bool,
}

/// Placas de rede com fio de verdade (têm dispositivo; não são ponte, VPN ou wifi).
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

/// Liga o WoL (pacote mágico) na placa. Precisa de root (o serviço faz).
pub fn enable(iface: &str) -> Result<(), String> {
    if !iface
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    {
        return Err("nome de interface inválido".into());
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
    // NetworkManager: guarda na conexão para ele não desligar de novo.
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

/// Manda o pacote mágico para a rede local (broadcast, portas 9 e 7) e, se souber, para o
/// broadcast da sub-rede do IP do outro PC.
pub fn wake(mac: &str, ip_hint: Option<&str>) -> Result<(), String> {
    let m = parse_mac(mac).ok_or_else(|| format!("MAC inválido: {mac}"))?;
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
        Err("não consegui mandar o pacote (rede?)".into())
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
