//! Motherboard fans (case and processors): through the sensor chip (hwmon: nct67xx, it87, ...)
//! and, on server boards, through IPMI (reading; fan mode on Supermicro).

use crate::gpuctl::{FanMode, curve_pct};
use crate::learned::state_dir;
use crate::util::{read_trim, read_u64, sys};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

/// Chips that are not the motherboard's (GPUs, disks, ...).
const SKIP: &[&str] = &[
    "amdgpu",
    "nouveau",
    "nvidia",
    "radeon",
    "nvme",
    "drivetemp",
    "coretemp",
    "k10temp",
    "acpitz",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoardFan {
    /// Identifier stable across boots: "chip@device/pwmN".
    pub id: String,
    pub chip: String,
    #[serde(skip)]
    pub dir: PathBuf,
    pub idx: u32,
    pub label: String,
    pub rpm: Option<u64>,
    pub pct: Option<f64>,
    /// 1 = manual, 2+ = chip automatic, 0 = maximum
    pub enable: Option<u8>,
}

pub fn list() -> Vec<BoardFan> {
    let mut v = Vec::new();
    let Ok(rd) = fs::read_dir(sys("class/hwmon")) else {
        return v;
    };
    for e in rd.flatten() {
        let d = e.path();
        let chip = read_trim(d.join("name")).unwrap_or_default();
        if chip.is_empty() || SKIP.contains(&chip.as_str()) {
            continue;
        }
        let dev = fs::read_link(d.join("device"))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
            .unwrap_or_else(|| e.file_name().to_string_lossy().to_string());
        let Ok(files) = fs::read_dir(&d) else {
            continue;
        };
        let mut idxs: Vec<u32> = files
            .flatten()
            .filter_map(|f| {
                f.file_name()
                    .to_string_lossy()
                    .strip_prefix("pwm")
                    .and_then(|r| r.parse().ok())
            })
            .collect();
        idxs.sort_unstable();
        for i in idxs {
            let max = read_u64(d.join(format!("pwm{i}_max")))
                .unwrap_or(255)
                .max(1) as f64;
            v.push(BoardFan {
                id: format!("{chip}@{dev}/pwm{i}"),
                label: read_trim(d.join(format!("fan{i}_label")))
                    .unwrap_or_else(|| crate::tf!("{chip} fan {i}", "{chip} ventoinha {i}")),
                chip: chip.clone(),
                rpm: read_u64(d.join(format!("fan{i}_input"))),
                pct: read_u64(d.join(format!("pwm{i}"))).map(|p| (p as f64 * 100.0 / max).round()),
                enable: read_u64(d.join(format!("pwm{i}_enable"))).map(|x| x as u8),
                dir: d.clone(),
                idx: i,
            });
        }
    }
    v.sort_by(|a, b| a.id.cmp(&b.id));
    v
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoardFanControl {
    pub fan: FanMode,
    pub curve: Vec<(f64, f64)>,
    pub fixed_pct: f64,
    /// Temperature that drives the curve: "cpu" (hottest socket), "cpu0", "cpu1", "gpu" (hottest GPU), "max"
    pub source: String,
    pub label: String,
}

impl Default for BoardFanControl {
    fn default() -> Self {
        BoardFanControl {
            fan: FanMode::Auto,
            curve: vec![
                (35.0, 25.0),
                (50.0, 35.0),
                (65.0, 55.0),
                (75.0, 80.0),
                (85.0, 100.0),
            ],
            fixed_pct: 50.0,
            source: "cpu".into(),
            label: String::new(),
        }
    }
}

impl BoardFanControl {
    pub fn validate(&mut self) -> Result<(), String> {
        if !["cpu", "cpu0", "cpu1", "gpu", "max"].contains(&self.source.as_str()) {
            return Err(crate::tf!(
                "invalid temperature source: {}",
                "fonte de temperatura inválida: {}",
                self.source
            ));
        }
        if self.curve.len() < 2 {
            return Err(crate::i18n::tr("the curve needs at least 2 points").into());
        }
        self.curve.sort_by(|a, b| a.0.total_cmp(&b.0));
        self.fixed_pct = self.fixed_pct.clamp(0.0, 100.0);
        Ok(())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BoardFans {
    pub map: BTreeMap<String, BoardFanControl>,
}

impl BoardFans {
    pub fn path() -> PathBuf {
        state_dir().join("fans.json")
    }
    pub fn load() -> BoardFans {
        fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> std::io::Result<()> {
        fs::create_dir_all(state_dir())?;
        fs::write(
            Self::path(),
            serde_json::to_string_pretty(self).unwrap_or_default(),
        )
    }
}

/// Temperature of the chosen source.
pub fn source_temp(source: &str, topo: &crate::topo::Topology) -> Option<f64> {
    let cpu = crate::cpuctl::package_temps();
    let gpu = || {
        topo.gpus
            .iter()
            .filter_map(|g| crate::gpuctl::read_sensors(&g.pci).control_temp())
            .fold(None, |a: Option<f64>, t| Some(a.map_or(t, |a| a.max(t))))
    };
    let cmax = cpu
        .values()
        .copied()
        .fold(None, |a: Option<f64>, t| Some(a.map_or(t, |a| a.max(t))));
    match source {
        "cpu0" => cpu.get(&0).copied(),
        "cpu1" => cpu.get(&1).copied(),
        "gpu" => gpu(),
        "max" => match (cmax, gpu()) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        },
        _ => cmax,
    }
}

/// Above this (CPU) the controlled fans go to 100%, whatever the mode.
pub const EMERGENCY_CPU_C: f64 = 90.0;

pub fn set_pct(f: &BoardFan, pct: f64) -> Result<(), String> {
    let max = read_u64(f.dir.join(format!("pwm{}_max", f.idx))).unwrap_or(255) as f64;
    let en = f.dir.join(format!("pwm{}_enable", f.idx));
    if en.exists() && read_trim(&en).as_deref() != Some("1") {
        fs::write(&en, "1").map_err(|e| format!("{}: {e}", en.display()))?;
    }
    let p = f.dir.join(format!("pwm{}", f.idx));
    fs::write(
        &p,
        ((pct.clamp(0.0, 100.0) / 100.0 * max).round() as u64).to_string(),
    )
    .map_err(|e| format!("{}: {e}", p.display()))
}

/// Gives control back to the chip (the previous pwmN_enable value, or 2 = automatic).
pub fn set_auto(f: &BoardFan, orig: Option<u8>) -> Result<(), String> {
    let en = f.dir.join(format!("pwm{}_enable", f.idx));
    fs::write(&en, orig.filter(|&o| o != 1).unwrap_or(2).to_string())
        .map_err(|e| format!("{}: {e}", en.display()))
}

/// Wanted percentage right now (None = chip automatic).
pub fn target(c: &BoardFanControl, temp: Option<f64>, cpu_max: Option<f64>) -> Option<f64> {
    if cpu_max.is_some_and(|t| t >= EMERGENCY_CPU_C) && c.fan != FanMode::Auto {
        return Some(100.0);
    }
    match c.fan {
        FanMode::Auto => None,
        FanMode::Fixed => Some(c.fixed_pct),
        FanMode::Curve => Some(temp.map(|t| curve_pct(&c.curve, t)).unwrap_or(100.0)),
    }
}

// ---------------- IPMI ----------------

fn ipmitool(args: &[&str]) -> Option<String> {
    if !std::path::Path::new("/dev/ipmi0").exists() || !crate::containers::which("ipmitool") {
        return None;
    }
    let o = std::process::Command::new("ipmitool")
        .args(args)
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).to_string())
}

/// (name, RPM) of the fans seen by the BMC.
pub fn ipmi_fans() -> Vec<(String, Option<u64>)> {
    let Some(s) = ipmitool(&["sdr", "type", "Fan"]) else {
        return vec![];
    };
    parse_ipmi_fans(&s)
}

pub fn parse_ipmi_fans(s: &str) -> Vec<(String, Option<u64>)> {
    s.lines()
        .filter_map(|l| {
            let c: Vec<&str> = l.split('|').map(str::trim).collect();
            if c.len() < 5 {
                return None;
            }
            let rpm = c[4]
                .split_whitespace()
                .next()
                .and_then(|x| x.parse::<f64>().ok())
                .map(|x| x as u64);
            Some((c[0].to_string(), rpm))
        })
        .collect()
}

pub fn is_supermicro() -> bool {
    read_trim(sys("class/dmi/id/board_vendor"))
        .is_some_and(|v| v.to_lowercase().contains("supermicro"))
}

/// Fan mode on the Supermicro BMC (0 default, 1 maximum, 2 optimal, 4 heavy IO).
pub fn supermicro_mode() -> Option<u8> {
    if !is_supermicro() {
        return None;
    }
    let s = ipmitool(&["raw", "0x30", "0x45", "0x00"])?;
    u8::from_str_radix(s.trim(), 16).ok()
}

pub fn supermicro_set_mode(mode: u8) -> Result<(), String> {
    if !is_supermicro() {
        return Err(crate::i18n::tr("Supermicro boards only").into());
    }
    ipmitool(&["raw", "0x30", "0x45", "0x01", &format!("0x{mode:02x}")])
        .map(|_| ())
        .ok_or_else(|| crate::i18n::tr("ipmitool failed").into())
}

pub fn ipmi_mode_name(m: u8) -> &'static str {
    match m {
        0 => crate::i18n::tr("default"),
        1 => crate::i18n::tr("maximum"),
        2 => crate::i18n::tr("optimal (quiet)"),
        4 => "heavy IO",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipmi_parse_and_target() {
        let s = "FAN1             | 41h | ok  | 29.1 | 1500 RPM\nFAN2             | 42h | ns  | 29.2 | No Reading\n";
        assert_eq!(
            parse_ipmi_fans(s),
            vec![("FAN1".into(), Some(1500)), ("FAN2".into(), None)]
        );
        let mut c = BoardFanControl {
            fan: FanMode::Curve,
            ..Default::default()
        };
        assert!(c.validate().is_ok());
        assert_eq!(target(&c, Some(50.0), Some(50.0)), Some(35.0));
        assert_eq!(target(&c, Some(50.0), Some(92.0)), Some(100.0));
        c.fan = FanMode::Auto;
        assert_eq!(target(&c, Some(99.0), Some(99.0)), None);
    }
}
