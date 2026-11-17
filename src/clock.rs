//! The clock a program is really getting: the frequency of the CPUs its threads ran on during
//! a short sample and the clock of the card it is rendering on (`numapilot clock`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

/// (CPU time in ticks, last CPU) of a thread, from /proc/PID/task/TID/stat.
pub fn parse_task_stat(txt: &str) -> Option<(u64, usize)> {
    // the name may hold spaces and parentheses: the fields start after the last ')'
    let rest = &txt[txt.rfind(')')? + 1..];
    let f: Vec<&str> = rest.split_whitespace().collect();
    let utime: u64 = f.get(11)?.parse().ok()?;
    let stime: u64 = f.get(12)?.parse().ok()?;
    let cpu: usize = f.get(36)?.parse().ok()?;
    Some((utime + stime, cpu))
}

fn threads(pids: &[i32]) -> HashMap<(i32, i32), (u64, usize)> {
    let mut m = HashMap::new();
    for &pid in pids {
        let Ok(dir) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
            continue;
        };
        for e in dir.flatten() {
            let Some(tid) = e.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else {
                continue;
            };
            if let Some(v) = std::fs::read_to_string(e.path().join("stat"))
                .ok()
                .and_then(|t| parse_task_stat(&t))
            {
                m.insert((pid, tid), v);
            }
        }
    }
    m
}

/// What one card is doing for the program.
#[derive(Debug, Clone, Default)]
pub struct CardClock {
    pub alias: String,
    pub pci: String,
    /// Share of the sample this program kept the card's engines busy (%).
    pub use_pct: f64,
    pub clock_mhz: Option<f64>,
    pub busy_pct: Option<u64>,
    pub power_w: Option<f64>,
    pub temp_c: Option<f64>,
    /// power_dpm_force_performance_level (AMD): auto, low, high, manual.
    pub level: Option<String>,
    /// Active line of pp_power_profile_mode (AMD).
    pub profile: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ProgClock {
    /// CPUs the program may use.
    pub allowed: Vec<usize>,
    /// CPUs its threads ran on during the sample, with the MHz of each.
    pub ran_on: BTreeMap<usize, u64>,
    /// CPU time used during the sample, in cores.
    pub cores: f64,
    pub governors: BTreeSet<String>,
    pub cards: Vec<CardClock>,
}

impl ProgClock {
    /// (average, lowest, highest) MHz of the CPUs it ran on.
    pub fn mhz(&self) -> Option<(u64, u64, u64)> {
        let v: Vec<u64> = self.ran_on.values().copied().collect();
        let (min, max) = (v.iter().min()?, v.iter().max()?);
        Some((v.iter().sum::<u64>() / v.len() as u64, *min, *max))
    }
}

fn amd_profile(pci: &str) -> Option<String> {
    let t = std::fs::read_to_string(crate::util::sys(&format!(
        "bus/pci/devices/{pci}/pp_power_profile_mode"
    )))
    .ok()?;
    t.lines().find(|l| l.contains('*')).and_then(|l| {
        l.split_whitespace()
            .nth(1)
            .map(|w| w.trim_end_matches([':', '*']).to_string())
    })
}

/// Samples the processes of one program for `ms` milliseconds.
pub fn sample(topo: &crate::topo::Topology, pids: &[i32], ms: u64) -> ProgClock {
    let t0 = threads(pids);
    let g0: Vec<_> = pids
        .iter()
        .map(|&p| crate::procctl::gpu_use_by_pci(p))
        .collect();
    let start = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(ms));
    // the frequency first: reading the rest wakes cores up
    let mhz = crate::cpuctl::cur_mhz_all();
    let t1 = threads(pids);
    let dt = start.elapsed().as_secs_f64().max(0.001);
    let mut out = ProgClock::default();
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64;
    let mut ticks = 0u64;
    for (k, (now, cpu)) in &t1 {
        let d = now.saturating_sub(t0.get(k).map_or(*now, |v| v.0));
        if d > 0 {
            ticks += d;
            if let Some(m) = mhz.get(cpu) {
                out.ran_on.insert(*cpu, *m);
            }
        }
    }
    out.cores = ticks as f64 / hz / dt;
    let mut allowed = BTreeSet::new();
    for &p in pids {
        allowed.extend(crate::procfs::get_affinity(p).unwrap_or_default());
    }
    out.allowed = allowed.into_iter().collect();
    let look: Vec<usize> = if out.ran_on.is_empty() {
        out.allowed.clone()
    } else {
        out.ran_on.keys().copied().collect()
    };
    out.governors = look
        .iter()
        .filter_map(|&c| crate::cpuctl::governor(c))
        .collect();
    // cards: engine time this program used on each one during the sample
    let mut used: BTreeMap<String, u64> = BTreeMap::new();
    for (i, &p) in pids.iter().enumerate() {
        for (pci, u) in crate::procctl::gpu_use_by_pci(p) {
            let before = g0[i].get(&pci).map_or(u.engine_ns, |b| b.engine_ns);
            *used.entry(pci).or_default() += u.engine_ns.saturating_sub(before);
        }
    }
    for (pci, ns) in used {
        let Some(g) = topo.gpus.iter().find(|g| g.pci == pci) else {
            continue;
        };
        let s = crate::gpuctl::read_sensors(&pci);
        out.cards.push(CardClock {
            alias: g.alias.clone(),
            use_pct: ns as f64 / 1e9 / dt * 100.0,
            clock_mhz: s.clock_mhz,
            busy_pct: g.busy_percent(),
            power_w: s.power_w,
            temp_c: s.junction_c.or(s.edge_c),
            level: crate::util::read_trim(crate::util::sys(&format!(
                "bus/pci/devices/{pci}/power_dpm_force_performance_level"
            ))),
            profile: amd_profile(&pci),
            pci,
        });
    }
    out.cards.sort_by(|a, b| {
        b.use_pct
            .partial_cmp(&a.use_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_stat_with_spaces_in_the_name() {
        let t = "4242 (Main Thread) x) R 1 2 3 4 5 6 7 8 9 10 150 50 0 0 20 0 9 0 100 200 300 \
                 18446744073709551615 1 1 0 0 0 0 0 0 0 0 0 0 17 13 0 0 0 0 0";
        assert_eq!(parse_task_stat(t), Some((200, 13)));
        assert_eq!(parse_task_stat("garbage"), None);
    }
}
