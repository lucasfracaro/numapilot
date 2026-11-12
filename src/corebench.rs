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

/// Judges the measurements: `rounds[cpu]` = that core's round times.
pub fn judge(rounds: &BTreeMap<usize, Vec<f32>>, groups: &[Vec<usize>]) -> CoreRank {
    let mut ns = BTreeMap::new();
    let mut noises = vec![];
    for (c, v) in rounds {
        let mut s = v.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(best) = s.first() {
            ns.insert(*c, *best);
            if let Some(second) = s.get(1) {
                noises.push((second - best) / best * 100.0);
            }
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
    CoreRank {
        useful: spread_pct >= 2.0 && spread_pct >= noise_pct * 3.0,
        ns,
        spread_pct,
        noise_pct,
        at: crate::history::now(),
        cpu: crate::recipes::cpu_model(),
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
    let steps = (lines * 6).max(400_000);
    out(&crate::tf!(
        "Measuring {} cores, {} rounds, {} MB block (fits in the L3 cache)…",
        "Medindo {} núcleos, {} rodadas, bloco de {} MB (cabe no cache L3)…",
        cpus.len(),
        ROUNDS,
        bytes >> 20
    ));
    let before = crate::procfs::get_affinity(0);
    let mut rounds: BTreeMap<usize, Vec<f32>> = BTreeMap::new();
    for r in 0..ROUNDS {
        for &c in &cpus {
            if crate::procfs::set_affinity(0, &[c]).is_err() {
                continue;
            }
            // warm-up: brings the block into the L3 from this core and wakes the clock
            chase(&buf, lines);
            rounds.entry(c).or_default().push(chase(&buf, steps));
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

const ROUNDS: usize = 4;

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

/// A core's time for ordering (only when the measurement is reliable).
pub fn ns_of(first_cpu: usize) -> Option<f32> {
    let r = saved()?;
    if !r.useful {
        return None;
    }
    r.ns.get(&first_cpu).copied()
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
}
