//! Energy use: the service adds up the power of the processors (RAPL) and the graphics cards
//! (plus a fixed value for the rest of the PC, if you enter one) and keeps each day's kWh.
//! With the kWh price, it shows the cost for the day and the month.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Energy {
    /// "YYYY-MM-DD" -> kWh
    pub days: BTreeMap<String, f64>,
}

pub fn path() -> std::path::PathBuf {
    crate::learned::state_dir().join("energy.json")
}

/// Local date "YYYY-MM-DD".
pub fn today() -> String {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!(
        "{:04}-{:02}-{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday
    )
}

impl Energy {
    pub fn load() -> Energy {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn load_today() -> Option<f64> {
        Self::load().days.get(&today()).copied()
    }

    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(crate::learned::state_dir())?;
        let tmp = path().with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string(self).unwrap_or_default())?;
        std::fs::rename(tmp, path())
    }

    /// Adds `watts` for `secs` to today; keeps only the last 400 days.
    pub fn add(&mut self, watts: f64, secs: f64) {
        if !(watts.is_finite() && watts > 0.0 && secs > 0.0 && secs < 3600.0) {
            return;
        }
        *self.days.entry(today()).or_default() += watts * secs / 3.6e6;
        while self.days.len() > 400 {
            let k = self.days.keys().next().cloned().unwrap_or_default();
            self.days.remove(&k);
        }
    }

    /// kWh of the month ("YYYY-MM").
    pub fn month(&self, ym: &str) -> f64 {
        self.days
            .iter()
            .filter(|(d, _)| d.starts_with(ym))
            .map(|(_, v)| v)
            .sum()
    }

    /// Average per day over the last `n` complete days.
    pub fn avg_days(&self, n: usize) -> Option<f64> {
        let t = today();
        let v: Vec<f64> = self
            .days
            .iter()
            .rev()
            .filter(|(d, _)| **d != t)
            .take(n)
            .map(|(_, v)| *v)
            .collect();
        (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
    }
}

/// Power right now (W): processors + graphics cards + the fixed part for the rest of the PC.
pub fn power_now(cpu: &BTreeMap<u32, f64>, topo: &crate::topo::Topology, base_w: f64) -> f64 {
    let gpus: f64 = topo
        .gpus
        .iter()
        .filter(|g| !(g.is_nvidia() && crate::nvctl::asleep(&g.pci)))
        .filter_map(|g| crate::gpuctl::read_sensors(&g.pci).power_w)
        .sum();
    cpu.values().sum::<f64>() + gpus + base_w.max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums() {
        let mut e = Energy::default();
        e.add(300.0, 3600.0 - 1.0);
        e.add(300.0, 1.0);
        let d = today();
        assert!((e.days[&d] - 0.3).abs() < 1e-9);
        assert!((e.month(&d[..7]) - 0.3).abs() < 1e-9);
        e.add(f64::NAN, 1.0);
        assert_eq!(e.days.len(), 1);
        assert_eq!(d.len(), 10);
    }
}
