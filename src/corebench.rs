//! Cache latency per core: measures, on each physical core, the time of an access that lands in
//! the L3 cache (outside the core's own L1/L2), and builds an order from fastest to slowest.
//! On processors with many cores the position on the L3 ring changes this time a little; on
//! others (AMD preferred cores, Intel P/E) the difference is larger. The order is only used for
//! choosing cores when the measured difference is larger than the measurement's own noise.

use crate::topo::Topology;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CoreRank {
    /// first CPU of the core → nanoseconds per access (lower = better)
    #[serde(deserialize_with = "usize_keys")]
    pub ns: BTreeMap<usize, f32>,
    /// difference between the best and the worst core, in % (the largest value among the processors)
    pub spread_pct: f32,
    /// how much the same measurement varies by itself, in %
    pub noise_pct: f32,
    /// is the difference reliable enough to order the cores?
    pub useful: bool,
    pub at: u64,
    /// model of the measured processor (if it changes, the measurement no longer holds)
    #[serde(default)]
    pub cpu: String,
    /// how much two independent halves of the measurement agree on the order (Spearman, -1..1);
    /// an order that does not repeat is noise, not a property of the cores
    #[serde(default)]
    pub agreement: f32,
}

/// JSON object keys are text ("0"). Inside an internally tagged enum (the control socket's
/// requests) serde buffers the map and no longer turns them into numbers, so the service
/// rejected `numapilot bench-cores`: parse the keys here.
fn usize_keys<'de, D: serde::Deserializer<'de>>(d: D) -> Result<BTreeMap<usize, f32>, D::Error> {
    let m: BTreeMap<String, f32> = Deserialize::deserialize(d)?;
    m.into_iter()
        .map(|(k, v)| {
            k.parse::<usize>()
                .map(|k| (k, v))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

pub fn path() -> PathBuf {
    crate::learned::state_dir().join("core-rank.json")
}

fn cache_bytes(cpu: usize, index: usize) -> Option<usize> {
    let s = crate::util::read_trim(crate::util::sys(&format!(
        "devices/system/cpu/cpu{cpu}/cache/index{index}/size"
    )))?;
    let n: usize = s.trim_end_matches(['K', 'M']).parse().ok()?;
    Some(if s.ends_with('M') { n << 20 } else { n << 10 })
}

/// Test size: much larger than the core's L2, fitting comfortably in the L3.
pub fn test_bytes(l2: Option<usize>, l3: Option<usize>) -> usize {
    let l2 = l2.unwrap_or(512 << 10);
    let l3 = l3.unwrap_or(8 << 20);
    (l3 / 4).clamp(l2 * 4, 8 << 20).min(l3.max(l2 * 4))
}

/// A random pointer chase through the vector (each position points to another, in a single
/// cycle): the processor cannot guess the next address, so every step costs one access.
fn chase(buf: &[u32], steps: usize) -> f32 {
    let mut i = 0u32;
    let t = Instant::now();
    for _ in 0..steps {
        // each element takes one cache line (16 × u32 = 64 bytes)
        i = buf[(i as usize) << 4];
    }
    let ns = t.elapsed().as_nanos() as f32 / steps as f32;
    std::hint::black_box(i);
    ns
}

/// Single random cycle (Sattolo) with `lines` cache lines.
fn make_cycle(lines: usize) -> Vec<u32> {
    let mut perm: Vec<u32> = (0..lines as u32).collect();
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for i in (1..lines).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let j = (x % i as u64) as usize;
        perm.swap(i, j);
    }
    let mut buf = vec![0u32; lines << 4];
    for (i, p) in perm.iter().enumerate() {
        buf[i << 4] = *p;
    }
    buf
}

/// Judges the measurements: `rounds[cpu]` = that core's round times. Besides the difference
/// being larger than the noise, the order must repeat: the even and the odd rounds are judged
/// apart and must agree (on the Nitro 5 an 8.8% "difference" came from load on some cores and
/// the fastest core changed at every measurement).
pub fn judge(rounds: &BTreeMap<usize, Vec<f32>>, groups: &[Vec<usize>]) -> CoreRank {
    let best = |v: &[f32]| v.iter().cloned().fold(f32::MAX, f32::min);
    let mut ns = BTreeMap::new();
    let mut noises = vec![];
    let mut half_a = BTreeMap::new();
    let mut half_b = BTreeMap::new();
    for (c, v) in rounds {
        let mut s = v.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(b) = s.first() {
            ns.insert(*c, *b);
            if let Some(second) = s.get(1) {
                noises.push((second - b) / b * 100.0);
            }
        }
        let even: Vec<f32> = v.iter().step_by(2).copied().collect();
        let odd: Vec<f32> = v.iter().skip(1).step_by(2).copied().collect();
        if !even.is_empty() && !odd.is_empty() {
            half_a.insert(*c, best(&even));
            half_b.insert(*c, best(&odd));
        }
    }
    noises.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise_pct = noises.get(noises.len() / 2).copied().unwrap_or(100.0);
    // difference within each processor (between processors does not matter here)
    let spread_pct = groups
        .iter()
        .filter_map(|g| {
            let v: Vec<f32> = g.iter().filter_map(|c| ns.get(c).copied()).collect();
            let min = v.iter().cloned().fold(f32::MAX, f32::min);
            let max = v.iter().cloned().fold(0.0, f32::max);
            (v.len() > 1 && min > 0.0).then(|| (max - min) / min * 100.0)
        })
        .fold(0.0, f32::max);
    // the worst agreement among the processors with at least 3 cores
    let agreement = groups
        .iter()
        .filter_map(|g| {
            let a: Vec<f32> = g.iter().filter_map(|c| half_a.get(c).copied()).collect();
            let b: Vec<f32> = g.iter().filter_map(|c| half_b.get(c).copied()).collect();
            (a.len() >= 3 && a.len() == b.len()).then(|| spearman(&a, &b))
        })
        .fold(1.0, f32::min);
    CoreRank {
        useful: spread_pct >= 2.0 && spread_pct >= noise_pct * 3.0 && agreement >= 0.8,
        ns,
        spread_pct,
        noise_pct,
        at: crate::history::now(),
        cpu: crate::recipes::cpu_model(),
        agreement,
    }
}

/// Rank correlation of two series (1 = same order, 0 = unrelated, -1 = reversed).
fn spearman(a: &[f32], b: &[f32]) -> f32 {
    let rank = |v: &[f32]| -> Vec<f32> {
        let mut idx: Vec<usize> = (0..v.len()).collect();
        idx.sort_by(|&x, &y| v[x].partial_cmp(&v[y]).unwrap_or(std::cmp::Ordering::Equal));
        let mut r = vec![0.0; v.len()];
        for (pos, &i) in idx.iter().enumerate() {
            r[i] = pos as f32;
        }
        r
    };
    let (ra, rb) = (rank(a), rank(b));
    let n = a.len() as f32;
    let d2: f32 = ra.iter().zip(&rb).map(|(x, y)| (x - y) * (x - y)).sum();
    1.0 - 6.0 * d2 / (n * (n * n - 1.0))
}

/// Fraction of the processor in use over `ms` (0..1), from /proc/stat.
pub fn system_busy(ms: u64) -> f32 {
    let a = crate::procfs::cpu_times();
    std::thread::sleep(std::time::Duration::from_millis(ms));
    let b = crate::procfs::cpu_times();
    let (busy, total) = b.iter().zip(&a).fold((0u64, 0u64), |(x, y), (n, p)| {
        (x + n.1.saturating_sub(p.1), y + n.2.saturating_sub(p.2))
    });
    if total == 0 {
        0.0
    } else {
        busy as f32 / total as f32
    }
}

/// Measures every physical core (one thread of each), in interleaved rounds so warm-up and
/// turbo affect all of them equally. Takes a few seconds; the PC should be idle.
pub fn measure(topo: &Topology, out: &mut dyn FnMut(&str)) -> Result<CoreRank, String> {
    let groups: Vec<Vec<usize>> = topo
        .nodes
        .iter()
        .map(|n| n.cores.iter().filter_map(|c| c.first().copied()).collect())
        .collect();
    let cpus: Vec<usize> = groups.iter().flatten().copied().collect();
    if cpus.len() < 2 {
        return Err(crate::i18n::tr("there is only one core: nothing to compare").into());
    }
    let bytes = test_bytes(cache_bytes(cpus[0], 2), cache_bytes(cpus[0], 3));
    let lines = bytes / 64;
    let buf = make_cycle(lines);
    let steps = lines.max(50_000);
    out(&crate::tf!(
        "Measuring {} cores, {} rounds, {} MB block (fits in the L3 cache)…",
        "Medindo {} núcleos, {} rodadas, bloco de {} MB (cabe no cache L3)…",
        cpus.len(),
        ROUNDS,
        bytes >> 20
    ));
    // The clock must be the same on every core: with a governor that follows the load
    // (schedutil + scx_lavd on the 2× Xeon) each core was caught at 1.2 or at 2.2 GHz at random,
    // the same core went from 23 to 42 ns between rounds and the order never repeated (27%).
    // With the clock held: 16% between cores, noise 0.1%, order repeats 98%.
    let hold = ClockHold::take(&cpus);
    if !hold.0 {
        out(crate::i18n::tr(
            "Could not hold the clock at full speed (service or performance governor missing): the result may vary with the clock.",
        ));
    }
    let before = crate::procfs::get_affinity(0);
    let mut rounds: BTreeMap<usize, Vec<f32>> = BTreeMap::new();
    for r in 0..ROUNDS {
        for &c in &cpus {
            if crate::procfs::set_affinity(0, &[c]).is_err() {
                continue;
            }
            // warm-up: brings the block into the L3 from this core and gives the clock time
            // to rise (a core coming out of deep idle starts slow)
            let t = Instant::now();
            while t.elapsed().as_millis() < WARM_MS {
                chase(&buf, lines / 4);
            }
            // the best of several short passes: an interruption spoils one pass, not the round
            let ns = (0..PASSES)
                .map(|_| chase(&buf, steps))
                .fold(f32::MAX, f32::min);
            rounds.entry(c).or_default().push(ns);
        }
        out(&crate::tf!(
            "  round {} of {ROUNDS}",
            "  rodada {} de {ROUNDS}",
            r + 1
        ));
    }
    if let Some(b) = before {
        let _ = crate::procfs::set_affinity(0, &b);
    }
    if rounds.len() < 2 {
        return Err(
            crate::i18n::tr("could not pin the test to the cores (sched_setaffinity)").into(),
        );
    }
    Ok(judge(&rounds, &groups))
}

const ROUNDS: usize = 8;
const WARM_MS: u128 = 30;
const PASSES: usize = 8;

/// Full clock while measuring: asked from the service (or done here when this is the service),
/// given back when dropped. The service ends it by itself if this process dies.
struct ClockHold(bool);

impl ClockHold {
    fn take(cpus: &[usize]) -> Self {
        if cfg!(test) || std::env::var_os("NUMAPILOT_SYSFS").is_some() {
            return Self(false);
        }
        let all_fast = || {
            cpus.iter()
                .all(|&c| crate::cpuctl::governor(c).as_deref() == Some("performance"))
        };
        if all_fast() {
            return Self(true);
        }
        if crate::util::is_root() {
            crate::cpuctl::hold_full_clock(cpus, 180);
        } else {
            let _ = crate::ctl::send(&crate::ctl::Request::FullClock { secs: 180 });
        }
        let ok = all_fast();
        if ok {
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        Self(ok)
    }
}

impl Drop for ClockHold {
    fn drop(&mut self) {
        if cfg!(test) || std::env::var_os("NUMAPILOT_SYSFS").is_some() {
            return;
        }
        if crate::util::is_root() {
            crate::cpuctl::release_full_clock();
        } else {
            let _ = crate::ctl::send(&crate::ctl::Request::FullClock { secs: 0 });
        }
    }
}

/// Saved order (if it exists, is from this processor and the difference is reliable). Rereads
/// when the file changes.
pub fn saved() -> Option<CoreRank> {
    static CACHE: Mutex<Option<(std::time::SystemTime, Option<CoreRank>)>> = Mutex::new(None);
    if std::env::var_os("NUMAPILOT_SYSFS").is_some() {
        return None;
    }
    let p = path();
    let mtime = std::fs::metadata(&p).ok()?.modified().ok()?;
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((m, r)) = g.as_ref()
        && *m == mtime
    {
        return r.clone();
    }
    let r: Option<CoreRank> = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(|r: &CoreRank| r.cpu == crate::recipes::cpu_model());
    *g = Some((mtime, r.clone()));
    r
}

/// No measurement of this processor yet (a deleted one counts as measured: the user chose the
/// system order). Only on PCs with more than one physical core.
pub fn needs_measure(topo: &Topology) -> bool {
    if std::env::var_os("NUMAPILOT_SYSFS").is_some() {
        return false;
    }
    let cores: usize = topo.nodes.iter().map(|n| n.cores.len()).sum();
    cores >= 2
        && std::fs::read_to_string(path())
            .ok()
            .and_then(|t| serde_json::from_str::<CoreRank>(&t).ok())
            .is_none_or(|r| r.cpu != crate::recipes::cpu_model())
}

/// Measures and stores the order through the service (users in its admin groups).
pub fn measure_and_store(topo: &Topology, out: &mut dyn FnMut(&str)) -> Result<CoreRank, String> {
    // with the PC busy the busy cores look slow: better no order than a wrong one
    let busy = system_busy(500);
    if busy > 0.15 {
        return Err(crate::tf!(
            "the PC is busy ({:.0}% in use): it measures another time",
            "o PC está ocupado ({:.0}% em uso): mede outra vez",
            busy * 100.0
        ));
    }
    let r = measure(topo, out)?;
    match crate::ctl::send(&crate::ctl::Request::SetCoreRank { rank: r.clone() }) {
        Ok(resp) if resp.ok => Ok(r),
        Ok(resp) => Err(resp.message),
        Err(e) => Err(e.to_string()),
    }
}

/// One line about a measurement ("difference 8.8%, noise 1.2%: in use").
pub fn summary(r: &CoreRank) -> String {
    crate::tf!(
        "difference between cores {:.1}%, noise {:.1}%, order repeats {:.0}%: {}",
        "diferença entre núcleos {:.1}%, ruído {:.1}%, ordem se repete {:.0}%: {}",
        r.spread_pct,
        r.noise_pct,
        r.agreement.max(0.0) * 100.0,
        if r.useful {
            crate::t2!(
                "the fastest cores come first",
                "os núcleos mais rápidos vêm primeiro"
            )
        } else {
            crate::t2!(
                "not reliable, system order",
                "não confiável, ordem do sistema"
            )
        }
    )
}

/// The firmware's preferred-core ranking of a CPU (higher = better): AMD
/// `amd_pstate_prefcore_ranking`, otherwise ACPI CPPC `highest_perf` (Intel Turbo Boost Max 3.0,
/// hybrid processors). None when the firmware does not rank the cores.
fn firmware_rank_of(cpu: usize) -> Option<u32> {
    let d = crate::util::sys(&format!("devices/system/cpu/cpu{cpu}"));
    crate::util::read_trim(d.join("cpufreq/amd_pstate_prefcore_ranking"))
        .or_else(|| crate::util::read_trim(d.join("acpi_cppc/highest_perf")))
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
}

/// Preferred cores by the firmware, only when it really ranks them (not all the same value).
pub fn firmware_ranking(topo: &Topology) -> Option<BTreeMap<usize, u32>> {
    let firsts: Vec<usize> = topo
        .nodes
        .iter()
        .flat_map(|n| n.cores.iter().filter_map(|c| c.first().copied()))
        .collect();
    let m: BTreeMap<usize, u32> = firsts
        .iter()
        .map(|c| Some((*c, firmware_rank_of(*c)?)))
        .collect::<Option<_>>()?;
    let lo = m.values().min()?;
    let hi = m.values().max()?;
    (m.len() >= 2 && lo != hi).then_some(m)
}

/// Where the core order comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderSource {
    /// the cache latency measured here, when it is reliable (repeats and beats the noise)
    Cache,
    /// the firmware's preferred cores, when the cache latency makes no measurable difference
    Firmware,
    /// neither: the system order
    System,
}

pub fn order_source() -> OrderSource {
    if saved().is_some_and(|r| r.useful) {
        OrderSource::Cache
    } else if firmware_rank_of(0).is_some() && firmware_differs() {
        OrderSource::Firmware
    } else {
        OrderSource::System
    }
}

fn firmware_differs() -> bool {
    let Ok(rd) = std::fs::read_dir(crate::util::sys("devices/system/cpu")) else {
        return false;
    };
    let mut seen = None;
    for e in rd.flatten() {
        let Some(c) = e
            .file_name()
            .to_str()
            .and_then(|n| n.strip_prefix("cpu"))
            .and_then(|n| n.parse::<usize>().ok())
        else {
            continue;
        };
        if let Some(v) = firmware_rank_of(c) {
            match seen {
                None => seen = Some(v),
                Some(s) if s != v => return true,
                _ => {}
            }
        }
    }
    false
}

/// A core's cost for ordering (lower = better). The cache latency measured here when it matters
/// (it repeats and beats the noise, as on the 2x Xeon's ring); otherwise the firmware's
/// preferred cores (on a single-CCX Ryzen every core sees the L3 the same and the measured
/// "order" was noise); otherwise nothing (system order).
pub fn ns_of(first_cpu: usize) -> Option<f32> {
    match order_source() {
        OrderSource::Cache => saved()?.ns.get(&first_cpu).copied(),
        // higher rank = better: as a cost, 1000/rank (5.10 for 196, 5.24 for 191: 2.6% apart)
        OrderSource::Firmware => firmware_rank_of(first_cpu).map(|r| 1000.0 / r as f32),
        OrderSource::System => None,
    }
}

/// Orders cores (each with its threads) from fastest to slowest; without a reliable
/// measurement the order stays as is. Differences under 1% do not change the order.
pub fn sort_units(units: &mut [Vec<usize>], ns: &dyn Fn(usize) -> Option<f32>) {
    let key = |u: &Vec<usize>| u.first().and_then(|c| ns(*c));
    if units.iter().any(|u| key(u).is_none()) {
        return;
    }
    let best = units.iter().filter_map(key).fold(f32::MAX, f32::min);
    // 1% steps over the best: practically equal cores keep the original order
    units.sort_by_key(|u| ((key(u).unwrap_or(best) / best - 1.0) * 100.0) as i32);
}

#[cfg(test)]
mod tests {
    #[test]
    fn rank_through_the_control_socket() {
        let mut r = super::CoreRank::default();
        r.ns.insert(6, 10.5);
        let req = crate::ctl::Request::SetCoreRank { rank: r.clone() };
        let back: crate::ctl::Request =
            serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        match back {
            crate::ctl::Request::SetCoreRank { rank } => assert_eq!(rank, r),
            _ => panic!(),
        }
    }

    use super::*;

    #[test]
    fn judge_and_sort() {
        let mut r = BTreeMap::new();
        r.insert(0, vec![10.0, 10.1, 10.3]);
        r.insert(1, vec![10.9, 10.8, 11.0]);
        r.insert(2, vec![10.05, 10.2, 10.1]);
        let j = judge(&r, &[vec![0, 1, 2]]);
        assert!(j.useful, "{j:?}");
        assert!((j.spread_pct - 8.0).abs() < 0.2);
        // large noise: not trusted
        let mut n = BTreeMap::new();
        n.insert(0, vec![10.0, 11.0]);
        n.insert(1, vec![10.3, 11.5]);
        assert!(!judge(&n, &[vec![0, 1]]).useful);
        // a clear spread, but the even and odd rounds disagree on the order: noise
        let mut f = BTreeMap::new();
        f.insert(0, vec![10.0, 11.0, 10.0, 11.0]);
        f.insert(1, vec![10.5, 10.5, 10.5, 10.5]);
        f.insert(2, vec![11.0, 10.0, 11.0, 10.0]);
        let jf = judge(&f, &[vec![0, 1, 2]]);
        assert!(jf.agreement < 0.0 && !jf.useful, "{jf:?}");

        let ns = |c: usize| j.ns.get(&c).copied();
        let mut u = vec![vec![1, 5], vec![0, 4], vec![2, 6]];
        sort_units(&mut u, &ns);
        // 0 (10.0) and 2 (10.05) tie in the 1% step and keep the order; 1 (10.8) goes last
        assert_eq!(u, vec![vec![0, 4], vec![2, 6], vec![1, 5]]);
        let mut v = vec![vec![1], vec![9]];
        sort_units(&mut v, &ns);
        assert_eq!(v, vec![vec![1], vec![9]]);
        assert_eq!(test_bytes(Some(256 << 10), Some(55 << 20)), 8 << 20);
        assert_eq!(test_bytes(Some(512 << 10), Some(4 << 20)), 2 << 20);
    }

    #[test]
    fn chase_runs() {
        let b = make_cycle(1024);
        // single cycle: after 1024 steps it is back at the start
        let mut i = 0u32;
        for _ in 0..1024 {
            i = b[(i as usize) << 4];
        }
        assert_eq!(i, 0);
        assert!(chase(&b, 10_000) > 0.0);
    }

    #[test]
    fn firmware_preferred_cores_when_the_cache_does_not_matter() {
        // Nitro 5 (Ryzen 7 6800H): amd_pstate_prefcore_ranking per core, no cache order
        let root = std::env::temp_dir().join(format!("np-prefcore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (cpu, rank) in [(0, 186), (2, 171), (4, 191), (6, 196)] {
            let d = root.join(format!("devices/system/cpu/cpu{cpu}/cpufreq"));
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("amd_pstate_prefcore_ranking"), format!("{rank}\n")).unwrap();
        }
        let _g = crate::util::test_sysfs_lock(&root);
        // saved() is off under NUMAPILOT_SYSFS: no cache order, so the firmware's
        assert_eq!(order_source(), OrderSource::Firmware);
        let mut u = vec![vec![0], vec![2], vec![4], vec![6]];
        sort_units(&mut u, &ns_of);
        assert_eq!(u, vec![vec![6], vec![4], vec![0], vec![2]]);
        // all equal (a Xeon's highest_perf): no firmware order
        for cpu in [0, 2, 4, 6] {
            std::fs::write(
                root.join(format!(
                    "devices/system/cpu/cpu{cpu}/cpufreq/amd_pstate_prefcore_ranking"
                )),
                "186\n",
            )
            .unwrap();
        }
        assert_eq!(order_source(), OrderSource::System);
        let _ = std::fs::remove_dir_all(root);
    }
}
