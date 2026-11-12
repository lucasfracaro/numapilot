//! Turbo by awake cores (Intel): the processor raises the clock more when few cores are
//! awake. The table (how many cores → maximum frequency) comes from the
//! MSR_TURBO_RATIO_LIMIT registers, read by the service (root); the awake cores are estimated
//! from the time in deep idle states (cpuidle), which any user can read.

use crate::util::{read_trim, sys};
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// (up to N cores awake, MHz), in ascending order of N.
pub type Table = Vec<(u32, u32)>;

/// (Intel vendor?, family, model) from /proc/cpuinfo.
pub fn cpu_ids() -> (bool, u32, u32) {
    let t = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let get = |k: &str| {
        t.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split(':').nth(1))
            .map(|v| v.trim().to_string())
    };
    (
        get("vendor_id").is_some_and(|v| v == "GenuineIntel"),
        get("cpu family").and_then(|v| v.parse().ok()).unwrap_or(0),
        get("model\t").and_then(|v| v.parse().ok()).unwrap_or(0),
    )
}

fn read_msr(cpu: usize, reg: u64) -> Option<u64> {
    use std::os::unix::fs::FileExt;
    let f = std::fs::File::open(format!("/dev/cpu/{cpu}/msr")).ok()?;
    let mut b = [0u8; 8];
    f.read_exact_at(&mut b, reg).ok()?;
    Some(u64::from_le_bytes(b))
}

/// Builds the table from the three registers (0x1AD, 0x1AE, 0x1AF), depending on the model.
/// - Server Ivy Bridge/Haswell/Broadwell (E5/E7 v2–v4): each byte is the ratio for 1, 2, 3...
///   cores (0x1AD: 1–8, 0x1AE: 9–16, 0x1AF: 17–24; the last valid byte applies to the rest).
/// - Server Skylake onwards: 0x1AD holds 8 ratios and 0x1AE the core count of each group.
/// - Others (desktop/laptop): only 0x1AD, ratios for 1–8 cores.
pub fn decode(model: u32, m1ad: u64, m1ae: Option<u64>, m1af: Option<u64>, cores: u32) -> Table {
    let bytes = |v: u64| (0..8).map(move |i| ((v >> (i * 8)) & 0xff) as u32);
    let mut t: Table = vec![];
    let grouped = matches!(model, 0x55 | 0x6a | 0x6c | 0x8f | 0xcf | 0xad | 0xae);
    let server_bytes = matches!(model, 0x3e | 0x3f | 0x4f | 0x56);
    if grouped && let Some(g) = m1ae {
        for (r, n) in bytes(m1ad).zip(bytes(g)) {
            if r > 0 && n > 0 {
                t.push((n, r * 100));
            }
        }
    } else {
        let mut ratios: Vec<u32> = bytes(m1ad).collect();
        if server_bytes {
            if let Some(v) = m1ae {
                ratios.extend(bytes(v));
            }
            if let Some(v) = m1af {
                // bit 63 is a semaphore, not part of the ratio
                ratios.extend(bytes(v & !(1 << 63)));
            }
        }
        for (i, r) in ratios.into_iter().enumerate() {
            if r == 0 {
                break;
            }
            t.push((i as u32 + 1, r * 100));
        }
    }
    // merges consecutive steps with the same frequency and extends the last one to the core total
    let mut out: Table = vec![];
    for (n, mhz) in t {
        if n > cores && out.last().is_some_and(|l| l.0 >= cores) {
            break;
        }
        match out.last_mut() {
            Some(l) if l.1 == mhz => l.0 = n,
            _ => out.push((n, mhz)),
        }
    }
    if let Some(l) = out.last_mut()
        && l.0 < cores
    {
        l.0 = cores;
    }
    out
}

/// Table of each processor (package → table). Needs root; empty if not Intel or the MSRs
/// cannot be read. Read once.
pub fn tables(topo: &crate::topo::Topology) -> &'static BTreeMap<u32, Table> {
    static T: OnceLock<BTreeMap<u32, Table>> = OnceLock::new();
    T.get_or_init(|| {
        let mut m = BTreeMap::new();
        let (intel, fam, model) = cpu_ids();
        if !intel || fam != 6 || std::env::var_os("NUMAPILOT_SYSFS").is_some() {
            return m;
        }
        if !std::path::Path::new("/dev/cpu/0/msr").exists() {
            let _ = std::process::Command::new("modprobe").arg("msr").status();
        }
        for n in &topo.nodes {
            let Some(&cpu) = n.cpus.first() else {
                continue;
            };
            let Some(a) = read_msr(cpu, 0x1ad) else {
                continue;
            };
            let cores = n.physical_cores.max(n.cores.len()) as u32;
            let t = decode(model, a, read_msr(cpu, 0x1ae), read_msr(cpu, 0x1af), cores);
            if !t.is_empty() {
                m.insert(n.id as u32, t);
            }
        }
        m
    })
}

/// Maximum frequency with `awake` cores awake.
pub fn limit_for(t: &Table, awake: f32) -> Option<u32> {
    let n = awake.ceil().max(1.0) as u32;
    t.iter()
        .find(|(upto, _)| n <= *upto)
        .or(t.last())
        .map(|x| x.1)
}

/// "1–2: 3,7 · 3: 3,5 · … · 22: 2,8 GHz"
pub fn describe(t: &Table) -> String {
    let mut prev = 0;
    let mut parts = vec![];
    for (n, mhz) in t {
        let range = if prev + 1 == *n {
            n.to_string()
        } else {
            format!("{}–{n}", prev + 1)
        };
        parts.push(format!("{range}: {:.1}", *mhz as f64 / 1000.0));
        prev = *n;
    }
    format!("{} GHz", parts.join(" · "))
}

fn deep_name(name: &str) -> bool {
    let n = name.to_uppercase();
    !(n == "POLL" || n.starts_with("C1") || n.starts_with("C0"))
}

/// Accumulated time (µs) of each CPU in deep states (everything beyond C1/C1E).
pub fn deep_time(cpus: &[usize]) -> BTreeMap<usize, u64> {
    let mut m = BTreeMap::new();
    for &c in cpus {
        let mut sum = 0u64;
        for i in 0..12 {
            let d = sys(&format!("devices/system/cpu/cpu{c}/cpuidle/state{i}"));
            let Some(name) = read_trim(d.join("name")) else {
                break;
            };
            if deep_name(&name) {
                sum += read_trim(d.join("time"))
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0);
            }
        }
        m.insert(c, sum);
    }
    m
}

/// Is there any deep state and is it enabled? None: no cpuidle (VM, BC-250).
pub fn deep_enabled(cpu: usize) -> Option<bool> {
    let mut any = None;
    for i in 0..12 {
        let d = sys(&format!("devices/system/cpu/cpu{cpu}/cpuidle/state{i}"));
        let Some(name) = read_trim(d.join("name")) else {
            break;
        };
        any = any.or(Some(false));
        if deep_name(&name) && read_trim(d.join("disable")).as_deref() != Some("1") {
            return Some(true);
        }
    }
    any
}

/// Cores awake on average over the interval: a core sleeps only when both of its threads
/// are in a deep state, so the thread that slept less is what counts.
pub fn awake_cores(
    prev: &BTreeMap<usize, u64>,
    cur: &BTreeMap<usize, u64>,
    dt_us: f64,
    cores: &[Vec<usize>],
) -> f32 {
    if dt_us <= 0.0 {
        return 0.0;
    }
    cores
        .iter()
        .map(|threads| {
            let deep = threads
                .iter()
                .map(|c| {
                    let d = cur
                        .get(c)
                        .copied()
                        .unwrap_or(0)
                        .saturating_sub(prev.get(c).copied().unwrap_or(0));
                    (d as f64 / dt_us).clamp(0.0, 1.0)
                })
                .fold(1.0f64, f64::min);
            (1.0 - deep) as f32
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn broadwell_ep() {
        // E5-2696 v4 (22 cores): 37 37 35 34 34 34 34 34 | 33 x8 | 32 32 31 30 29 28 ...
        let a = u64::from_le_bytes([37, 37, 35, 34, 34, 34, 34, 34]);
        let b = u64::from_le_bytes([33; 8]);
        let c = u64::from_le_bytes([32, 32, 31, 30, 29, 28, 28, 0x80]);
        let t = decode(0x4f, a, Some(b), Some(c), 22);
        assert_eq!(t.first(), Some(&(2, 3700)));
        assert_eq!(t.last(), Some(&(22, 2800)));
        assert_eq!(limit_for(&t, 1.2), Some(3700));
        assert_eq!(limit_for(&t, 3.0), Some(3500));
        assert_eq!(limit_for(&t, 12.4), Some(3300));
        assert_eq!(limit_for(&t, 22.0), Some(2800));
        assert!(describe(&t).starts_with("1–2: 3.7 · 3: 3.5 · 4–8: 3.4"));
    }

    #[test]
    fn skylake_x_groups_and_client() {
        let ratios = u64::from_le_bytes([40, 38, 36, 34, 32, 30, 30, 30]);
        let groups = u64::from_le_bytes([2, 4, 8, 12, 16, 20, 24, 28]);
        let t = decode(0x55, ratios, Some(groups), None, 28);
        assert_eq!(t.first(), Some(&(2, 4000)));
        assert_eq!(t.last(), Some(&(28, 3000)));
        // 4-core desktop: bytes beyond the 4th come repeated or zeroed
        let t = decode(
            0x9e,
            u64::from_le_bytes([45, 44, 43, 42, 0, 0, 0, 0]),
            None,
            None,
            4,
        );
        assert_eq!(t, vec![(1, 4500), (2, 4400), (3, 4300), (4, 4200)]);
    }

    #[test]
    fn awake() {
        let prev: BTreeMap<usize, u64> = [(0, 0), (1, 0), (2, 0), (3, 0)].into();
        // core A (cpus 0,2): one thread slept 90%, the other 10% → awake 90%
        // core B (cpus 1,3): both slept 100% → awake 0
        let cur: BTreeMap<usize, u64> =
            [(0, 900_000), (2, 100_000), (1, 1_000_000), (3, 1_000_000)].into();
        let a = awake_cores(&prev, &cur, 1_000_000.0, &[vec![0, 2], vec![1, 3]]);
        assert!((a - 0.9).abs() < 0.01);
        assert!(deep_name("C6") && deep_name("C3_ACPI") && !deep_name("C1E") && !deep_name("POLL"));
    }
}
