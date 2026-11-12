//! Disk and network: read/write speed and active time of each disk (/proc/diskstats),
//! download/upload of each network card (/sys/class/net) and disk per process (/proc/PID/io).

use std::collections::HashMap;
use std::time::Instant;

/// A whole disk (no partitions, loop, zram, ram).
#[derive(Debug, Clone, PartialEq)]
pub struct DiskCounters {
    pub name: String,
    pub read_bytes: u64,
    pub write_bytes: u64,
    /// Time with I/O in progress (ms): the difference gives the % active.
    pub io_ms: u64,
}

/// A real whole disk? (nvme0n1, sda, vda, mmcblk0 — not nvme0n1p1, sda1, loop0, zram0)
pub fn is_whole_disk(name: &str) -> bool {
    if ["loop", "ram", "zram", "dm-", "md", "sr", "fd"]
        .iter()
        .any(|p| name.starts_with(p))
    {
        return false;
    }
    if name.starts_with("nvme") || name.starts_with("mmcblk") {
        // nvme0n1 yes, nvme0n1p1 no; mmcblk0 yes, mmcblk0p1 no
        return !name.contains('p')
            || !name
                .rsplit('p')
                .next()
                .unwrap_or("")
                .chars()
                .all(|c| c.is_ascii_digit());
    }
    // sda, vda, xvda: no number at the end
    !name.ends_with(|c: char| c.is_ascii_digit())
}

/// /proc/diskstats → whole disks (512-byte sectors).
pub fn parse_diskstats(text: &str) -> Vec<DiskCounters> {
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 13 || !is_whole_disk(f[2]) {
                return None;
            }
            let n = |i: usize| f[i].parse::<u64>().unwrap_or(0);
            Some(DiskCounters {
                name: f[2].to_string(),
                read_bytes: n(5) * 512,
                write_bytes: n(9) * 512,
                io_ms: n(12),
            })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetCounters {
    pub name: String,
    pub rx: u64,
    pub tx: u64,
    pub wifi: bool,
}

/// Network card worth showing: physical (has `device`) or VPN (wg*, tailscale*, tun*).
pub fn is_shown_iface(name: &str, has_device: bool) -> bool {
    name != "lo"
        && (has_device
            || ["wg", "tailscale", "tun", "zt"]
                .iter()
                .any(|p| name.starts_with(p)))
}

fn read_net() -> Vec<NetCounters> {
    let base = crate::util::sys("class/net");
    let mut v: Vec<NetCounters> = std::fs::read_dir(&base)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let p = e.path();
            if !is_shown_iface(&name, p.join("device").exists()) {
                return None;
            }
            let n = |f: &str| crate::util::read_u64(p.join("statistics").join(f)).unwrap_or(0);
            Some(NetCounters {
                rx: n("rx_bytes"),
                tx: n("tx_bytes"),
                wifi: p.join("wireless").exists() || p.join("phy80211").exists(),
                name,
            })
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// Bytes read and written to disk by a process (yours; another user's needs root).
pub fn proc_io(pid: i32) -> Option<(u64, u64)> {
    let t = std::fs::read_to_string(format!("/proc/{pid}/io")).ok()?;
    let get = |k: &str| {
        t.lines()
            .find_map(|l| l.strip_prefix(k))
            .and_then(|v| v.trim().parse::<u64>().ok())
    };
    Some((get("read_bytes:")?, get("write_bytes:")?))
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiskRate {
    pub name: String,
    pub model: String,
    pub read_bps: f64,
    pub write_bps: f64,
    /// % of the time with I/O in progress.
    pub busy_pct: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NetRate {
    pub name: String,
    pub wifi: bool,
    pub rx_bps: f64,
    pub tx_bps: f64,
}

/// Measures the speeds between two readings.
#[derive(Default)]
pub struct IoMonitor {
    last: Option<Instant>,
    disks: HashMap<String, DiskCounters>,
    net: HashMap<String, NetCounters>,
    models: HashMap<String, String>,
}

impl IoMonitor {
    pub fn sample(&mut self) -> (Vec<DiskRate>, Vec<NetRate>) {
        let now = Instant::now();
        let dt = self
            .last
            .map(|t| now.duration_since(t).as_secs_f64())
            .filter(|d| *d > 0.05);
        self.last = Some(now);
        let disks =
            parse_diskstats(&std::fs::read_to_string("/proc/diskstats").unwrap_or_default());
        let net = read_net();
        let mut dr = vec![];
        for d in &disks {
            let model = self
                .models
                .entry(d.name.clone())
                .or_insert_with(|| {
                    crate::util::read_trim(crate::util::sys(&format!(
                        "block/{}/device/model",
                        d.name
                    )))
                    .unwrap_or_default()
                })
                .clone();
            if let (Some(dt), Some(p)) = (dt, self.disks.get(&d.name)) {
                dr.push(DiskRate {
                    name: d.name.clone(),
                    model,
                    read_bps: d.read_bytes.saturating_sub(p.read_bytes) as f64 / dt,
                    write_bps: d.write_bytes.saturating_sub(p.write_bytes) as f64 / dt,
                    busy_pct: (d.io_ms.saturating_sub(p.io_ms) as f64 / (dt * 10.0)).min(100.0),
                });
            }
        }
        let mut nr = vec![];
        for n in &net {
            if let (Some(dt), Some(p)) = (dt, self.net.get(&n.name)) {
                nr.push(NetRate {
                    name: n.name.clone(),
                    wifi: n.wifi,
                    rx_bps: n.rx.saturating_sub(p.rx) as f64 / dt,
                    tx_bps: n.tx.saturating_sub(p.tx) as f64 / dt,
                });
            }
        }
        self.disks = disks.into_iter().map(|d| (d.name.clone(), d)).collect();
        self.net = net.into_iter().map(|n| (n.name.clone(), n)).collect();
        (dr, nr)
    }
}

/// "12,3 MB/s", "850 KB/s", "0 B/s"
pub fn fmt_rate(bps: f64) -> String {
    if bps >= 1e9 {
        format!("{:.1} GB/s", bps / 1e9)
    } else if bps >= 1e6 {
        format!("{:.1} MB/s", bps / 1e6)
    } else if bps >= 1e3 {
        format!("{:.0} KB/s", bps / 1e3)
    } else {
        format!("{bps:.0} B/s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_discos_inteiros() {
        for d in ["sda", "sdb", "nvme0n1", "vda", "mmcblk0", "nvme1n1"] {
            assert!(is_whole_disk(d), "{d}");
        }
        for d in [
            "sda1",
            "nvme0n1p2",
            "loop0",
            "zram0",
            "dm-0",
            "mmcblk0p1",
            "sr0",
        ] {
            assert!(!is_whole_disk(d), "{d}");
        }
    }

    #[test]
    fn diskstats() {
        let t = "   8       0 sda 1853 35 1617072 26229 7143 2610 32024648 358646 0 100755 389187 0 0 0 0 114 4312\n   8       1 sda1 1782 35 1605256 26024 7143 2610 32024648 358646 0 100584 384670 0 0 0 0 0 0\n";
        let v = parse_diskstats(t);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].read_bytes, 1617072 * 512);
        assert_eq!(v[0].write_bytes, 32024648 * 512);
        assert_eq!(v[0].io_ms, 100755);
    }

    #[test]
    fn network_cards() {
        assert!(is_shown_iface("enp3s0", true));
        assert!(is_shown_iface("wg0", false));
        assert!(!is_shown_iface("lo", true));
        assert!(!is_shown_iface("docker0", false));
        assert!(!is_shown_iface("veth12ab", false));
        assert_eq!(fmt_rate(1_500_000.0), "1.5 MB/s");
        assert_eq!(fmt_rate(900.0), "900 B/s");
    }

    #[test]
    fn own_process_io() {
        assert!(proc_io(std::process::id() as i32).is_some());
    }
}
