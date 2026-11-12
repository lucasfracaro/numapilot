//! History per program: how much processor and memory each one used over the last hours,
//! one point per minute, to pick limits with data. The service writes to
//! /run/numapilot/prog-history.json (gone when the PC reboots).

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::Mutex;

/// Points kept per program (one per minute): 6 hours.
pub const KEEP: usize = 360;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Point {
    /// seconds since 1970
    pub t: u64,
    /// cores in use: average and peak of the minute
    pub avg: f32,
    pub peak: f32,
    /// memory (MB) at the end of the minute
    pub mem_mb: u32,
}

#[derive(Default)]
struct Acc {
    sum: f64,
    n: u32,
    peak: f32,
    mem: u64,
}

#[derive(Default)]
struct State {
    acc: HashMap<String, Acc>,
    hist: BTreeMap<String, VecDeque<Point>>,
    minute: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

pub fn path() -> std::path::PathBuf {
    crate::learned::run_dir().join("prog-history.json")
}

/// One sample (the service calls it on every reading): (program key, cores, memory in MB),
/// one entry per process. Returns true when a minute closed (time to write).
pub fn sample<'a>(rows: impl Iterator<Item = (&'a str, f64, u64)>, now: u64) -> bool {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let st = g.get_or_insert_with(State::default);
    // adds up the processes of the same program in this sample
    let mut tick: HashMap<&str, (f64, u64)> = HashMap::new();
    for (k, c, m) in rows {
        if k.is_empty() {
            continue;
        }
        let e = tick.entry(k).or_default();
        e.0 += c;
        e.1 += m;
    }
    for (k, (c, m)) in tick {
        let a = st.acc.entry(k.to_string()).or_default();
        a.sum += c;
        a.n += 1;
        a.peak = a.peak.max(c as f32);
        a.mem = m;
    }
    let minute = now / 60;
    if st.minute == 0 {
        st.minute = minute;
    }
    if minute == st.minute {
        return false;
    }
    st.minute = minute;
    let acc = std::mem::take(&mut st.acc);
    for (k, a) in acc {
        if a.n == 0 {
            continue;
        }
        let v = st.hist.entry(k).or_default();
        if v.len() >= KEEP {
            v.pop_front();
        }
        v.push_back(Point {
            t: now,
            avg: (a.sum / a.n as f64) as f32,
            peak: a.peak,
            mem_mb: a.mem.min(u32::MAX as u64) as u32,
        });
    }
    // forgets whoever has not shown up for more than 6 hours
    let old = now.saturating_sub(KEEP as u64 * 60);
    st.hist.retain(|_, v| v.back().is_some_and(|p| p.t >= old));
    true
}

/// Writes the history (the service calls it when `sample` returns true).
pub fn save() {
    let g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(st) = g.as_ref()
        && let Ok(t) = serde_json::to_string(&st.hist)
    {
        let tmp = path().with_extension("tmp");
        if std::fs::write(&tmp, t).is_ok() {
            let _ = std::fs::rename(tmp, path());
        }
    }
}

/// History of one program (for the interface).
pub fn load(key: &str) -> Vec<Point> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|t| serde_json::from_str::<BTreeMap<String, Vec<Point>>>(&t).ok())
        .and_then(|mut m| m.remove(key))
        .unwrap_or_default()
}

/// Summary to decide limits.
#[derive(Debug, PartialEq)]
pub struct Summary {
    pub minutes: usize,
    pub avg: f32,
    pub peak: f32,
    pub mem_peak_mb: u32,
}

pub fn summary(p: &[Point]) -> Option<Summary> {
    if p.is_empty() {
        return None;
    }
    Some(Summary {
        minutes: p.len(),
        avg: p.iter().map(|x| x.avg).sum::<f32>() / p.len() as f32,
        peak: p.iter().map(|x| x.peak).fold(0.0, f32::max),
        mem_peak_mb: p.iter().map(|x| x.mem_mb).max().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minute_points() {
        *STATE.lock().unwrap() = None;
        let t0 = 6_000_000;
        // two processes of the same program add up
        assert!(!sample(
            [("a", 1.0, 100), ("a", 0.5, 50), ("b", 0.2, 10)].into_iter(),
            t0
        ));
        assert!(!sample([("a", 3.0, 300)].into_iter(), t0 + 1));
        assert!(sample([("a", 1.0, 100)].into_iter(), t0 + 60));
        let g = STATE.lock().unwrap();
        let h = &g.as_ref().unwrap().hist;
        let a = h["a"][0];
        assert_eq!(a.peak, 3.0);
        assert!((a.avg - (1.5 + 3.0 + 1.0) / 3.0).abs() < 0.01);
        assert_eq!(a.mem_mb, 100);
        assert_eq!(h["b"].len(), 1);
        let s = summary(&[
            a,
            Point {
                t: 0,
                avg: 1.0,
                peak: 5.0,
                mem_mb: 900,
            },
        ])
        .unwrap();
        assert_eq!((s.minutes, s.peak, s.mem_peak_mb), (2, 5.0, 900));
    }
}
