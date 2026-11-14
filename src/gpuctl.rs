//! Sensors and control of AMD GPUs through amdgpu's hwmon: temperatures, fan
//! (curve / fixed / automatic) and power limit. Reading: any user.
//! Writing: only the daemon (root), on request from the interface.

use crate::learned::state_dir;
use crate::util::{read_trim, sys};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;

/// Temperature at which the fan goes to 100% regardless of the curve.
pub const EMERGENCY_C: f64 = 95.0;

pub fn hwmon_dir(pci: &str) -> Option<PathBuf> {
    let base = sys(&format!("bus/pci/devices/{pci}/hwmon"));
    let mut v: Vec<PathBuf> = fs::read_dir(base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    v.sort();
    v.into_iter().next()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sensors {
    pub edge_c: Option<f64>,
    pub junction_c: Option<f64>,
    pub mem_c: Option<f64>,
    pub fan_rpm: Option<u64>,
    /// Current fan in % (from pwm1).
    pub fan_pct: Option<f64>,
    /// 1 = manual, 2 = automatic (firmware)
    pub fan_mode_raw: Option<u8>,
    pub fan_controllable: bool,
    pub power_w: Option<f64>,
    pub power_cap_w: Option<f64>,
    pub power_cap_min_w: Option<f64>,
    pub power_cap_max_w: Option<f64>,
    /// The fan is controlled by a curve in the firmware (RDNA3/RDNA4), not by pwm1.
    #[serde(default)]
    pub fw_fan_curve: bool,
    /// Current GPU core clock (MHz), from hwmon (freq1_input / sclk).
    #[serde(default)]
    pub clock_mhz: Option<f64>,
    /// GPU core voltage (mV) from the board's VRM (BC-250).
    #[serde(default)]
    pub voltage_mv: Option<f64>,
    /// Temperature of the GPU's voltage regulator (°C, BC-250).
    #[serde(default)]
    pub vrm_c: Option<f64>,
    /// Die temperature of an APU whose GPU sensor is unusable (k10temp Tctl: CPU and GPU
    /// share the chip). Shown as "APU".
    #[serde(default)]
    pub apu_c: Option<f64>,
}

impl Sensors {
    /// Temperature used for the curve: junction (hotspot) if present, otherwise edge.
    pub fn control_temp(&self) -> Option<f64> {
        self.junction_c.or(self.edge_c).or(self.apu_c)
    }
}

fn num(p: PathBuf) -> Option<f64> {
    read_trim(p)?.parse().ok()
}

/// Plausible GPU temperature (°C). Some firmwares (BC-250) send 0 or garbage.
fn plausible_temp(c: f64) -> Option<f64> {
    (c > 1.0 && c < 150.0).then_some(c)
}

/// Plausible GPU clock (MHz). On the BC-250 pp_dpm_sclk comes with garbage, freq1_input does not.
pub fn plausible_clock(mhz: f64) -> Option<f64> {
    (mhz.is_finite() && (50.0..4000.0).contains(&mhz)).then_some(mhz)
}

/// BC-250's GPU (Cyan Skillfish, 1002:13fe)?
fn is_bc250(pci: &str) -> bool {
    read_trim(sys(&format!("bus/pci/devices/{pci}/device"))).as_deref() == Some("0x13fe")
}

/// Plausible GPU power (W). No card goes over 1000 W; the BC-250 sends ~1470 W.
fn plausible_power(w: f64) -> Option<f64> {
    (w.is_finite() && (0.0..1000.0).contains(&w)).then_some(w)
}

pub fn read_sensors(pci: &str) -> Sensors {
    if crate::nvctl::is_nvidia(pci) {
        return crate::nvctl::sensors(pci);
    }
    let Some(h) = hwmon_dir(pci) else {
        return Sensors::default();
    };
    let mut s = Sensors::default();
    for i in 1..=4 {
        let label = read_trim(h.join(format!("temp{i}_label"))).unwrap_or_default();
        let Some(v) = num(h.join(format!("temp{i}_input"))) else {
            continue;
        };
        let Some(c) = plausible_temp(v / 1000.0) else {
            continue;
        };
        match label.as_str() {
            "edge" => s.edge_c = Some(c),
            "junction" => s.junction_c = Some(c),
            "mem" => s.mem_c = Some(c),
            _ if i == 1 => s.edge_c = Some(c),
            _ => {}
        }
    }
    s.fan_rpm = num(h.join("fan1_input")).map(|v| v as u64);
    let pwm_max = num(h.join("pwm1_max")).unwrap_or(255.0).max(1.0);
    s.fan_pct = num(h.join("pwm1")).map(|v| (v * 100.0 / pwm_max).round());
    s.fan_mode_raw = num(h.join("pwm1_enable")).map(|v| v as u8);
    s.fw_fan_curve = fan_curve_path(pci).exists();
    s.fan_controllable =
        !s.fw_fan_curve && h.join("pwm1").exists() && h.join("pwm1_enable").exists();
    let w = |f: &str| num(h.join(f)).map(|v| v / 1_000_000.0);
    s.power_w = w("power1_average")
        .or_else(|| w("power1_input"))
        .and_then(plausible_power);
    s.clock_mhz = (1..=2)
        .find(|i| read_trim(h.join(format!("freq{i}_label"))).is_none_or(|l| l == "sclk"))
        .and_then(|i| num(h.join(format!("freq{i}_input"))))
        .and_then(|hz| plausible_clock(hz / 1_000_000.0))
        // BC-250 (1002:13fe): the most any kernel offers is 2230 MHz; with the SMU table
        // read wrong, 3882 and 4764 MHz have shown up
        .filter(|&mhz| mhz <= 3000.0 || !is_bc250(pci));
    // BC-250: power and voltage from the VRM; temperature from the APU die when the SMU
    // table is unusable
    if read_trim(sys(&format!("bus/pci/devices/{pci}/device"))).as_deref() == Some("0x13fe") {
        if let Some(v) = crate::boards::vrm() {
            if s.power_w.is_none() {
                s.power_w = v.gpu_w.and_then(plausible_power);
            }
            s.voltage_mv = v.gpu_mv;
            s.vrm_c = v.gpu_vrm_c;
        }
        if s.edge_c.is_none() && s.junction_c.is_none() {
            s.apu_c = crate::cpuctl::package_temps().values().next().copied();
        }
    }
    s.power_cap_w = w("power1_cap");
    s.power_cap_min_w = w("power1_cap_min");
    s.power_cap_max_w = w("power1_cap_max");
    s
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FanMode {
    /// The card's firmware decides (default).
    Auto,
    /// Temperature → % curve controlled by the daemon.
    Curve,
    /// Fixed speed.
    Fixed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuControl {
    pub fan: FanMode,
    /// Points (°C, %) in ascending temperature order.
    pub curve: Vec<(f64, f64)>,
    pub fixed_pct: f64,
    /// Power limit in W (None = card default).
    pub power_cap_w: Option<f64>,
    /// Clock/voltage (OverDrive). None = factory default.
    pub od: Option<OdSettings>,
    /// NVIDIA: locked maximum clock (MHz). None = normal.
    pub clock_max_mhz: Option<u32>,
    /// NVIDIA: offset of the GPU's clock/voltage curve (MHz). With the maximum clock locked,
    /// a positive offset is the undervolt.
    pub nv_gpc_offset_mhz: Option<i32>,
    /// NVIDIA: memory clock offset (MHz).
    pub nv_mem_offset_mhz: Option<i32>,
    /// AMD: after 1 min idle, stays at the lowest clock/voltage level until used again.
    #[serde(default)]
    pub idle_save: bool,
}

pub fn default_curve() -> Vec<(f64, f64)> {
    vec![
        (40.0, 25.0),
        (60.0, 40.0),
        (75.0, 65.0),
        (85.0, 90.0),
        (95.0, 100.0),
    ]
}

impl Default for GpuControl {
    fn default() -> Self {
        GpuControl {
            fan: FanMode::Auto,
            curve: default_curve(),
            fixed_pct: 50.0,
            power_cap_w: None,
            od: None,
            clock_max_mhz: None,
            nv_gpc_offset_mhz: None,
            nv_mem_offset_mhz: None,
            idle_save: false,
        }
    }
}

impl GpuControl {
    pub fn validate(&mut self) -> Result<(), String> {
        if self.curve.len() < 2 {
            return Err(crate::i18n::tr("the curve needs at least 2 points").into());
        }
        self.curve
            .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for p in &mut self.curve {
            p.1 = p.1.clamp(0.0, 100.0);
        }
        self.fixed_pct = self.fixed_pct.clamp(0.0, 100.0);
        Ok(())
    }

    /// Fan % for a temperature (with the emergency rule).
    pub fn target_pct(&self, temp: Option<f64>) -> Option<f64> {
        let t = temp?;
        if t >= EMERGENCY_C {
            return Some(100.0);
        }
        match self.fan {
            FanMode::Auto => None,
            FanMode::Fixed => Some(self.fixed_pct),
            FanMode::Curve => Some(curve_pct(&self.curve, t)),
        }
    }
}

/// Linear interpolation on the curve (below the 1st point = 1st value; above the last = last).
pub fn curve_pct(curve: &[(f64, f64)], t: f64) -> f64 {
    if curve.is_empty() {
        return 100.0;
    }
    if t <= curve[0].0 {
        return curve[0].1;
    }
    for w in curve.windows(2) {
        let ((t0, p0), (t1, p1)) = (w[0], w[1]);
        if t <= t1 {
            if t1 <= t0 {
                return p1;
            }
            return p0 + (p1 - p0) * (t - t0) / (t1 - t0);
        }
    }
    curve[curve.len() - 1].1
}

#[derive(Debug, Default, Clone)]
pub struct GpuControls {
    pub map: BTreeMap<String, GpuControl>,
}

impl GpuControls {
    pub fn path() -> PathBuf {
        state_dir().join("gpu.json")
    }
    pub fn load() -> GpuControls {
        let map = fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        GpuControls { map }
    }
    pub fn save(&self) -> io::Result<()> {
        fs::create_dir_all(state_dir())?;
        let tmp = state_dir().join("gpu.json.tmp");
        fs::write(
            &tmp,
            serde_json::to_string_pretty(&self.map).unwrap_or_default(),
        )?;
        fs::rename(tmp, Self::path())
    }
    pub fn get(&self, pci: &str) -> GpuControl {
        self.map.get(pci).cloned().unwrap_or_default()
    }
}

// ---------- writing (daemon) ----------

pub fn set_fan_auto(pci: &str) -> io::Result<()> {
    let h = hwmon_dir(pci).ok_or_else(|| io::Error::other(crate::i18n::tr("no hwmon")))?;
    fs::write(h.join("pwm1_enable"), "2")
}

pub fn set_fan_pct(pci: &str, pct: f64) -> io::Result<()> {
    let h = hwmon_dir(pci).ok_or_else(|| io::Error::other(crate::i18n::tr("no hwmon")))?;
    let max = num(h.join("pwm1_max")).unwrap_or(255.0);
    if read_trim(h.join("pwm1_enable")).as_deref() != Some("1") {
        fs::write(h.join("pwm1_enable"), "1")?;
    }
    let v = (pct.clamp(0.0, 100.0) * max / 100.0).round() as u64;
    fs::write(h.join("pwm1"), v.to_string())
}

/// Power limit in W (within the card's minimum and maximum). None = back to the default.
pub fn set_power_cap(pci: &str, watts: Option<f64>) -> io::Result<f64> {
    if crate::nvctl::is_nvidia(pci) {
        return crate::nvctl::set_power_limit(pci, watts).map_err(io::Error::other);
    }
    let h = hwmon_dir(pci).ok_or_else(|| io::Error::other(crate::i18n::tr("no hwmon")))?;
    let w = |f: &str| num(h.join(f)).map(|v| v / 1_000_000.0);
    let target = match watts {
        Some(x) => x,
        None => w("power1_cap_default")
            .or_else(|| w("power1_cap_max"))
            .ok_or_else(|| io::Error::other(crate::i18n::tr("no default limit")))?,
    };
    let lo = w("power1_cap_min").unwrap_or(0.0);
    let hi = w("power1_cap_max").unwrap_or(target);
    let v = target.clamp(lo, hi);
    fs::write(
        h.join("power1_cap"),
        ((v * 1_000_000.0).round() as u64).to_string(),
    )?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_interpolation_and_emergency() {
        let c = default_curve();
        assert_eq!(curve_pct(&c, 30.0), 25.0);
        assert_eq!(curve_pct(&c, 50.0), 32.5);
        assert_eq!(curve_pct(&c, 85.0), 90.0);
        assert_eq!(curve_pct(&c, 120.0), 100.0);
        let mut g = GpuControl {
            fan: FanMode::Fixed,
            fixed_pct: 30.0,
            ..Default::default()
        };
        assert_eq!(g.target_pct(Some(60.0)), Some(30.0));
        assert_eq!(
            g.target_pct(Some(96.0)),
            Some(100.0),
            "emergência ignora o modo"
        );
        g.fan = FanMode::Auto;
        assert_eq!(g.target_pct(Some(60.0)), None);
        assert_eq!(g.target_pct(None), None);
        let mut bad = GpuControl {
            curve: vec![(70.0, 150.0), (30.0, 10.0)],
            ..Default::default()
        };
        bad.validate().unwrap();
        assert_eq!(bad.curve, vec![(30.0, 10.0), (70.0, 100.0)]);
    }
}

// ---------- clock and voltage (OverDrive) ----------

/// Tabela do pp_od_clk_voltage (Vega 20 / MI50).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Od {
    /// (index, MHz) — 0 = minimum, 1 = maximum
    pub sclk: Vec<(u32, u32)>,
    pub mclk: Vec<(u32, u32)>,
    /// (index, MHz, mV) — the 3 points of the voltage curve
    pub curve: Vec<(u32, u32, u32)>,
    pub range_sclk: Option<(u32, u32)>,
    pub range_mclk: Option<(u32, u32)>,
    /// per point: MHz and mV range
    pub range_curve_sclk: Vec<(u32, u32)>,
    pub range_curve_volt: Vec<(u32, u32)>,
    /// RDNA2/3/4: current voltage offset (mV) and allowed range.
    pub vddgfx_offset: Option<i32>,
    pub range_vo: Option<(i32, i32)>,
}

fn signed(s: &str) -> Vec<i32> {
    let mut v = Vec::new();
    let mut cur = String::new();
    for c in s.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() || (c == '-' && cur.is_empty()) {
            cur.push(c);
        } else {
            if let Ok(x) = cur.parse() {
                v.push(x);
            }
            cur.clear();
            if c == '-' {
                cur.push(c);
            }
        }
    }
    v
}

fn nums(s: &str) -> Vec<u32> {
    s.split(|c: char| !c.is_ascii_digit())
        .filter(|x| !x.is_empty())
        .filter_map(|x| x.parse().ok())
        .collect()
}

pub fn parse_od(s: &str) -> Od {
    let mut od = Od::default();
    let mut sec = "";
    for line in s.lines() {
        let l = line.trim();
        if l.ends_with(':') && !l.contains(' ') {
            sec = match l {
                "OD_SCLK:" => "s",
                "OD_MCLK:" => "m",
                "OD_VDDC_CURVE:" => "v",
                "OD_VDDGFX_OFFSET:" => "o",
                "OD_RANGE:" => "r",
                _ => "",
            };
            continue;
        }
        let n = nums(l);
        match sec {
            "s" if n.len() >= 2 => od.sclk.push((n[0], n[1])),
            "m" if n.len() >= 2 => od.mclk.push((n[0], n[1])),
            "v" if n.len() >= 3 => od.curve.push((n[0], n[1], n[2])),
            "o" => od.vddgfx_offset = signed(l).first().copied(),
            "r" => {
                if l.starts_with("SCLK:") && n.len() >= 2 {
                    od.range_sclk = Some((n[0], n[1]));
                } else if l.starts_with("VDDGFX_OFFSET:") {
                    let v = signed(l);
                    if v.len() >= 2 {
                        od.range_vo = Some((v[0], v[1]));
                    }
                } else if l.starts_with("MCLK:") && n.len() >= 2 {
                    od.range_mclk = Some((n[0], n[1]));
                } else if l.starts_with("VDDC_CURVE_SCLK[") && n.len() >= 3 {
                    od.range_curve_sclk.push((n[1], n[2]));
                } else if (l.starts_with("VDDC_CURVE_VOLT[") || l.starts_with("VDDC_CURVE_VOFF["))
                    && n.len() >= 3
                {
                    od.range_curve_volt.push((n[1], n[2]));
                }
            }
            _ => {}
        }
    }
    od
}

pub fn od_path(pci: &str) -> PathBuf {
    sys(&format!("bus/pci/devices/{pci}/pp_od_clk_voltage"))
}

/// None = OverDrive off (amdgpu.ppfeaturemask is missing the 0x4000 bit).
pub fn read_od(pci: &str) -> Option<Od> {
    let s = fs::read_to_string(od_path(pci)).ok()?;
    let od = parse_od(&s);
    if od.sclk.is_empty() && od.curve.is_empty() && od.vddgfx_offset.is_none() {
        None
    } else {
        Some(od)
    }
}

/// What the user chose (None/empty = keep the card default).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OdSettings {
    pub sclk_max: Option<u32>,
    pub mclk_max: Option<u32>,
    /// (MHz, mV) for each of the 3 points (Vega20 and RDNA1)
    pub curve: Vec<(u32, u32)>,
    /// RDNA2/3/4: voltage offset in mV (negative = undervolt)
    #[serde(default)]
    pub voltage_offset_mv: Option<i32>,
}

impl OdSettings {
    pub fn is_default(&self) -> bool {
        self.sclk_max.is_none()
            && self.mclk_max.is_none()
            && self.curve.is_empty()
            && self.voltage_offset_mv.is_none()
    }
}

/// Commands for pp_od_clk_voltage, validating the card's ranges.
pub fn od_commands(od: &Od, s: &OdSettings) -> Result<Vec<String>, String> {
    let mut v = Vec::new();
    let check = |val: u32, r: Option<(u32, u32)>, what: &str| -> Result<(), String> {
        match r {
            Some((lo, hi)) if val < lo || val > hi => {
                Err(format!("{what} {val} fora da faixa {lo}–{hi}"))
            }
            _ => Ok(()),
        }
    };
    if let Some(x) = s.sclk_max {
        check(x, od.range_sclk, "clock da GPU (MHz)")?;
        if !od.sclk.iter().any(|(i, _)| *i == 1) {
            return Err("esta placa não permite mudar o clock máximo da GPU".into());
        }
        v.push(format!("s 1 {x}"));
    }
    if let Some(o) = s.voltage_offset_mv {
        if od.vddgfx_offset.is_none() {
            return Err("esta placa não aceita deslocamento de voltagem (use a curva)".into());
        }
        if let Some((lo, hi)) = od.range_vo
            && (o < lo || o > hi)
        {
            return Err(format!("deslocamento {o} mV fora da faixa {lo}–{hi}"));
        }
        v.push(format!("vo {o}"));
    }
    if let Some(x) = s.mclk_max {
        check(x, od.range_mclk, "clock da memória (MHz)")?;
        v.push(format!("m 1 {x}"));
    }
    for (i, (mhz, mv)) in s.curve.iter().enumerate() {
        check(
            *mhz,
            od.range_curve_sclk.get(i).copied(),
            &format!("ponto {i} (MHz)"),
        )?;
        check(
            *mv,
            od.range_curve_volt.get(i).copied(),
            &format!("ponto {i} (mV)"),
        )?;
        v.push(format!("vc {i} {mhz} {mv}"));
    }
    v.push("c".into());
    Ok(v)
}

/// Applies (or, with `None`, goes back to the factory default).
pub fn apply_od(pci: &str, s: Option<&OdSettings>) -> Result<(), String> {
    let p = od_path(pci);
    let lvl = sys(&format!(
        "bus/pci/devices/{pci}/power_dpm_force_performance_level"
    ));
    let write = |cmd: &str| fs::write(&p, cmd).map_err(|e| format!("'{cmd}': {e}"));
    match s {
        None => {
            write("r")?;
            write("c")?;
        }
        Some(s) => {
            let od = read_od(pci).ok_or("OverDrive desligado (veja o diagnóstico)")?;
            let cmds = od_commands(&od, s)?;
            let _ = fs::write(&lvl, "manual");
            for c in &cmds {
                write(c)?;
            }
        }
    }
    let _ = fs::write(&lvl, "auto");
    Ok(())
}

// ---------- Resizable BAR ----------

/// (VRAM visible to the CPU, total VRAM) in bytes. If they are ~equal, Resizable BAR is active.
pub fn bar_info(pci: &str) -> Option<(u64, u64)> {
    let r = |f: &str| {
        read_trim(sys(&format!("bus/pci/devices/{pci}/{f}")))?
            .parse::<u64>()
            .ok()
    };
    Some((r("mem_info_vis_vram_total")?, r("mem_info_vram_total")?))
}

// ---------- Intel (i915) ----------

fn drm_card_dir(pci: &str) -> Option<PathBuf> {
    let d = sys(&format!("bus/pci/devices/{pci}/drm"));
    fs::read_dir(&d)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.starts_with("card") && !n.contains('-')
            })
        })
}

/// (current, min, max, factory min, factory max) in MHz of the Intel integrated GPU.
pub fn intel_freq(pci: &str) -> Option<(u32, u32, u32, u32, u32)> {
    let c = drm_card_dir(pci)?;
    let r = |f: &str| read_trim(c.join(f)).and_then(|s| s.parse::<u32>().ok());
    Some((
        r("gt_cur_freq_mhz")?,
        r("gt_min_freq_mhz")?,
        r("gt_max_freq_mhz")?,
        r("gt_RPn_freq_mhz").unwrap_or(0),
        r("gt_RP0_freq_mhz").unwrap_or(0),
    ))
}

/// Intel GPU frequency ceiling (None = the factory one).
pub fn set_intel_max_freq(pci: &str, mhz: Option<u32>) -> Result<(), String> {
    let c = drm_card_dir(pci).ok_or("sem placa DRM")?;
    let (_, min, _, _, hw_max) = intel_freq(pci).ok_or("o driver não expõe as frequências")?;
    let v = mhz.unwrap_or(hw_max).clamp(min, hw_max.max(min));
    fs::write(c.join("gt_max_freq_mhz"), v.to_string()).map_err(|e| e.to_string())
}

// ---------- fan curve in the firmware (RDNA3/RDNA4: gpu_od/fan_ctrl) ----------

pub fn fan_curve_path(pci: &str) -> PathBuf {
    sys(&format!("bus/pci/devices/{pci}/gpu_od/fan_ctrl/fan_curve"))
}

/// Points (°C, %) and allowed ranges of the firmware curve.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FwFanCurve {
    pub points: Vec<(u32, u32)>,
    pub temp_range: Option<(u32, u32)>,
    pub pct_range: Option<(u32, u32)>,
}

pub fn parse_fan_curve(s: &str) -> FwFanCurve {
    let mut f = FwFanCurve::default();
    let mut range = false;
    for l in s.lines() {
        let l = l.trim();
        if l.starts_with("OD_RANGE") {
            range = true;
            continue;
        }
        let n = nums(l);
        if range {
            if l.contains("temp") && n.len() >= 2 {
                f.temp_range = Some((n[n.len() - 2], n[n.len() - 1]));
            } else if l.contains("speed") && n.len() >= 2 {
                f.pct_range = Some((n[n.len() - 2], n[n.len() - 1]));
            }
        } else if n.len() >= 3 && l.contains('C') {
            f.points.push((n[1], n[2]));
        }
    }
    f
}

pub fn read_fan_curve(pci: &str) -> Option<FwFanCurve> {
    let c = parse_fan_curve(&fs::read_to_string(fan_curve_path(pci)).ok()?);
    (!c.points.is_empty()).then_some(c)
}

/// Applies the fan mode through the firmware curve (fixed = flat curve).
pub fn apply_fw_fan(pci: &str, c: &GpuControl) -> Result<(), String> {
    match c.fan {
        FanMode::Auto => apply_fw_fan_curve(pci, None),
        FanMode::Curve => apply_fw_fan_curve(pci, Some(&c.curve)),
        FanMode::Fixed => {
            apply_fw_fan_curve(pci, Some(&[(0.0, c.fixed_pct), (100.0, c.fixed_pct)]))
        }
    }
}

/// Writes the user's curve (interpolated at the points the firmware has) and commits it.
pub fn apply_fw_fan_curve(pci: &str, curve: Option<&[(f64, f64)]>) -> Result<(), String> {
    let p = fan_curve_path(pci);
    let write = |cmd: &str| fs::write(&p, cmd).map_err(|e| format!("'{cmd}': {e}"));
    let Some(user) = curve else {
        write("r")?;
        return write("c");
    };
    let fw = read_fan_curve(pci).ok_or("a placa não tem curva de ventoinha no firmware")?;
    let (tlo, thi) = fw.temp_range.unwrap_or((25, 100));
    let (plo, phi) = fw.pct_range.unwrap_or((15, 100));
    let n = fw.points.len().max(2);
    for i in 0..n {
        let t = tlo + (thi - tlo) * i as u32 / (n as u32 - 1).max(1);
        let pct = (curve_pct(user, t as f64).round() as u32).clamp(plo, phi);
        write(&format!("{i} {t} {pct}"))?;
    }
    write("c")
}

/// Resizable BAR status of any card (AMD, NVIDIA, Intel), through PCI.
#[derive(Debug, Clone, PartialEq)]
pub struct BarStatus {
    /// Largest memory window of the card seen by the CPU (bytes).
    pub bar_bytes: u64,
    pub vram_bytes: Option<u64>,
    /// The window is above 4 GB (the BIOS has "Above 4G Decoding" enabled).
    pub above_4g: bool,
    /// Largest size the card accepts (Resizable BAR capability), if reported.
    pub max_bytes: Option<u64>,
}

impl BarStatus {
    pub fn full(&self) -> bool {
        match self.vram_bytes {
            Some(v) => self.bar_bytes * 10 >= v * 9,
            None => self.bar_bytes > (512 << 20),
        }
    }
}

/// Reads the "resource" file (start end flags per BAR) and the accepted sizes (resourceN_resize).
pub fn bar_status(pci: &str) -> Option<BarStatus> {
    let s = fs::read_to_string(sys(&format!("bus/pci/devices/{pci}/resource"))).ok()?;
    let hex = |x: &str| u64::from_str_radix(x.trim_start_matches("0x"), 16).ok();
    let mut best: Option<(usize, u64, u64)> = None;
    for (i, l) in s.lines().take(6).enumerate() {
        let f: Vec<&str> = l.split_whitespace().collect();
        let (Some(a), Some(b)) = (
            f.first().and_then(|x| hex(x)),
            f.get(1).and_then(|x| hex(x)),
        ) else {
            continue;
        };
        if b <= a {
            continue;
        }
        let size = b - a + 1;
        if best.is_none_or(|(_, _, s)| size > s) {
            best = Some((i, a, size));
        }
    }
    let (idx, start, size) = best?;
    let max_bytes = read_trim(sys(&format!("bus/pci/devices/{pci}/resource{idx}_resize")))
        .and_then(|m| u64::from_str_radix(m.trim_start_matches("0x"), 16).ok())
        .filter(|m| *m != 0)
        .map(|m| 1u64 << (20 + 63 - m.leading_zeros() as u64));
    let vram = bar_info(pci).map(|(_, t)| t).or_else(|| {
        crate::nvctl::is_nvidia(pci)
            .then(|| {
                crate::nvctl::get(pci)
                    .and_then(|g| g.mem_total_mb)
                    .map(|m| (m * 1048576.0) as u64)
            })
            .flatten()
    });
    Some(BarStatus {
        bar_bytes: size,
        vram_bytes: vram,
        above_4g: start > 0xffff_ffff,
        max_bytes,
    })
}

pub fn rebar_active(pci: &str) -> Option<bool> {
    bar_info(pci).map(|(vis, tot)| tot > 0 && vis * 10 >= tot * 9)
}

#[cfg(test)]
mod od_tests {
    use super::*;

    const SAMPLE: &str = "OD_SCLK:\n0:        808Mhz\n1:       1801Mhz\nOD_MCLK:\n1:       1000Mhz\nOD_VDDC_CURVE:\n0:        808Mhz        721mV\n1:       1304Mhz        822mV\n2:       1801Mhz       1078mV\nOD_RANGE:\nSCLK:     808Mhz       2200Mhz\nMCLK:     800Mhz       1200Mhz\nVDDC_CURVE_SCLK[0]:     808Mhz       2200Mhz\nVDDC_CURVE_VOLT[0]:     738mV        1218mV\nVDDC_CURVE_SCLK[1]:     808Mhz       2200Mhz\nVDDC_CURVE_VOLT[1]:     738mV        1218mV\nVDDC_CURVE_SCLK[2]:     808Mhz       2200Mhz\nVDDC_CURVE_VOLT[2]:     738mV        1218mV\n";

    #[test]
    fn parse_and_commands() {
        let od = parse_od(SAMPLE);
        assert_eq!(od.sclk, vec![(0, 808), (1, 1801)]);
        assert_eq!(od.mclk, vec![(1, 1000)]);
        assert_eq!(od.curve[2], (2, 1801, 1078));
        assert_eq!(od.range_sclk, Some((808, 2200)));
        assert_eq!(od.range_curve_volt.len(), 3);
        let s = OdSettings {
            sclk_max: Some(1700),
            mclk_max: None,
            curve: vec![(808, 740), (1304, 800), (1700, 1000)],
            voltage_offset_mv: None,
        };
        assert_eq!(
            od_commands(&od, &s).unwrap(),
            vec![
                "s 1 1700",
                "vc 0 808 740",
                "vc 1 1304 800",
                "vc 2 1700 1000",
                "c"
            ]
        );
        let bad = OdSettings {
            curve: vec![(808, 600)],
            ..Default::default()
        };
        assert!(
            od_commands(&od, &bad)
                .unwrap_err()
                .contains("fora da faixa")
        );
    }
}

// ---------- power profile (pp_power_profile_mode) ----------

/// (index, name) of the card's profiles and the active index.
/// Profiles (index, name) and the active index.
pub type PowerProfiles = (Vec<(u32, String)>, Option<u32>);

pub fn power_profiles(pci: &str) -> Option<PowerProfiles> {
    let s =
        fs::read_to_string(sys(&format!("bus/pci/devices/{pci}/pp_power_profile_mode"))).ok()?;
    Some(parse_power_profiles(&s))
}

pub fn parse_power_profiles(s: &str) -> PowerProfiles {
    let mut v = Vec::new();
    let mut active = None;
    for l in s.lines() {
        let mut it = l.split_whitespace();
        let (Some(a), Some(b)) = (it.next(), it.next()) else {
            continue;
        };
        let Ok(i) = a.parse::<u32>() else { continue };
        let star = b.contains('*');
        let name = b
            .trim_end_matches(':')
            .trim_end_matches('*')
            .trim_end_matches(':')
            .to_string();
        if !name.chars().any(|c| c.is_ascii_uppercase())
            || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            continue;
        }
        if star {
            active = Some(i);
        }
        if !v.iter().any(|(j, _)| *j == i) {
            v.push((i, name));
        }
    }
    (v, active)
}

/// Short name used in numapilot's profiles → driver name.
pub fn profile_driver_name(short: &str) -> Option<&'static str> {
    Some(match short {
        "3d" => "3D_FULL_SCREEN",
        "compute" => "COMPUTE",
        "vr" => "VR",
        "video" => "VIDEO",
        "economia" => "POWER_SAVING",
        "padrao" => "BOOTUP_DEFAULT",
        _ => return None,
    })
}

/// Picks a profile by its short name ("3d", "compute", "padrao"...).
pub fn set_power_profile(pci: &str, short: &str) -> Result<(), String> {
    let want = profile_driver_name(short).ok_or("perfil desconhecido")?;
    let (list, active) = power_profiles(pci).ok_or("a placa não tem pp_power_profile_mode")?;
    let (idx, _) = list
        .iter()
        .find(|(_, n)| n == want)
        .ok_or(format!("a placa não tem o perfil {want}"))?;
    if active == Some(*idx) {
        return Ok(());
    }
    let lvl = sys(&format!(
        "bus/pci/devices/{pci}/power_dpm_force_performance_level"
    ));
    // Outside the default the driver requires the "manual" level; on the default, back to "auto".
    if want != "BOOTUP_DEFAULT" {
        let _ = fs::write(&lvl, "manual");
    }
    let r = fs::write(
        sys(&format!("bus/pci/devices/{pci}/pp_power_profile_mode")),
        idx.to_string(),
    )
    .map_err(|e| e.to_string());
    if want == "BOOTUP_DEFAULT" {
        let _ = fs::write(&lvl, "auto");
    }
    r
}

// ---------- link PCIe ----------

#[derive(Debug, Clone, PartialEq)]
pub struct PcieLink {
    /// Current generation and width of the slowest stretch between the card and the processor.
    pub generation: u32,
    pub width: u32,
    /// The most the slot/card would negotiate.
    pub max_gen: u32,
    pub max_width: u32,
}

fn gen_of(speed: &str) -> Option<u32> {
    let gt: f64 = speed.split_whitespace().next()?.parse().ok()?;
    Some(match gt {
        x if x < 3.0 => 1,
        x if x < 6.0 => 2,
        x if x < 10.0 => 3,
        x if x < 20.0 => 4,
        x if x < 40.0 => 5,
        _ => 6,
    })
}

/// Walks the card's PCI path up to the root (the MI50s have an internal PCIe switch: the link
/// that matters is the slot's) and returns the most limited stretch.
pub fn pcie_link(pci: &str) -> Option<PcieLink> {
    let dev = fs::canonicalize(sys(&format!("bus/pci/devices/{pci}"))).ok()?;
    let mut best: Option<PcieLink> = None;
    let mut p = Some(dev.as_path());
    while let Some(d) = p {
        let name = d.file_name()?.to_string_lossy().to_string();
        if !name.contains(':') || name.starts_with("pci") {
            break;
        }
        let r = |f: &str| read_trim(d.join(f));
        if let (Some(cs), Some(cw), Some(ms), Some(mw)) = (
            r("current_link_speed"),
            r("current_link_width"),
            r("max_link_speed"),
            r("max_link_width"),
        ) {
            let l = PcieLink {
                generation: gen_of(&cs).unwrap_or(0),
                width: cw.parse().unwrap_or(0),
                max_gen: gen_of(&ms).unwrap_or(0),
                max_width: mw.parse().unwrap_or(0),
            };
            if l.width > 0 {
                best = Some(match best {
                    None => l,
                    Some(b) => PcieLink {
                        generation: b.generation.min(l.generation),
                        width: b.width.min(l.width),
                        max_gen: b.max_gen.min(l.max_gen),
                        max_width: b.max_width.min(l.max_width),
                    },
                });
            }
        }
        p = d.parent();
    }
    best
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn rdna_offset_and_fw_curve() {
        let od = parse_od(
            "OD_SCLK:\n0: 500Mhz\n1: 2700Mhz\nOD_MCLK:\n0: 97Mhz\n1: 1250MHz\nOD_VDDGFX_OFFSET:\n0mV\nOD_RANGE:\nSCLK:     500Mhz       4000Mhz\nMCLK:     674Mhz       1500Mhz\nVDDGFX_OFFSET:    -200mv          0mv\n",
        );
        assert_eq!(od.vddgfx_offset, Some(0));
        assert_eq!(od.range_vo, Some((-200, 0)));
        let cmds = od_commands(
            &od,
            &OdSettings {
                voltage_offset_mv: Some(-60),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(cmds, vec!["vo -60", "c"]);
        assert!(
            od_commands(
                &od,
                &OdSettings {
                    voltage_offset_mv: Some(-300),
                    ..Default::default()
                }
            )
            .is_err()
        );
        let fc = parse_fan_curve(
            "OD_FAN_CURVE:\n0: 0C 0%\n1: 45C 15%\n2: 50C 30%\n3: 55C 70%\n4: 65C 100%\nOD_RANGE:\nFAN_CURVE(hotspot temp): 25C 100C\nFAN_CURVE(fan speed): 15% 100%\n",
        );
        assert_eq!(fc.points.len(), 5);
        assert_eq!(fc.temp_range, Some((25, 100)));
        assert_eq!(fc.pct_range, Some((15, 100)));
    }

    #[test]
    fn bar_from_resource() {
        let root = std::env::temp_dir().join("numapilot-bar-test");
        let _ = std::fs::remove_dir_all(&root);
        let d = root.join("bus/pci/devices/0000:03:00.0");
        std::fs::create_dir_all(&d).unwrap();
        // BAR0 256 MB above 4 GB, BAR2 2 MB, BAR5 512 KB
        std::fs::write(
            d.join("resource"),
            "0x000000bff0000000 0x000000bfffffffff 0x000000000014220c\n0x0000000000000000 0x0000000000000000 0x0000000000000000\n0x000000bfe0000000 0x000000bfe01fffff 0x000000000014220c\n0x0000000000000000 0x0000000000000000 0x0000000000000000\n0x0000000000000000 0x0000000000000000 0x0000000000000000\n0x00000000fbe00000 0x00000000fbe7ffff 0x0000000000040200\n",
        )
        .unwrap();
        std::fs::write(d.join("resource0_resize"), "0000000000003f00").unwrap();
        std::fs::write(d.join("mem_info_vram_total"), "8589934592").unwrap();
        std::fs::write(d.join("mem_info_vis_vram_total"), "268435456").unwrap();
        let _g = crate::util::test_sysfs_lock(&root);
        let b = bar_status("0000:03:00.0").unwrap();
        assert_eq!(b.bar_bytes, 256 << 20);
        assert!(b.above_4g);
        assert_eq!(b.max_bytes, Some(8 << 30), "bit 13 = 8 GB");
        assert!(!b.full());
    }

    #[test]
    fn garbage_clock_only_on_bc250() {
        let root = std::env::temp_dir().join("numapilot-bc250-clock-test");
        let _ = std::fs::remove_dir_all(&root);
        for (pci, dev) in [("0000:01:00.0", "0x13fe"), ("0000:03:00.0", "0x7590")] {
            let d = root.join(format!("bus/pci/devices/{pci}"));
            std::fs::create_dir_all(d.join("hwmon/hwmon0")).unwrap();
            std::fs::write(d.join("device"), dev).unwrap();
            std::fs::write(d.join("hwmon/hwmon0/freq1_label"), "sclk").unwrap();
            std::fs::write(d.join("hwmon/hwmon0/freq1_input"), "3882000000").unwrap();
        }
        let _g = crate::util::test_sysfs_lock(&root);
        // BC-250: SMU table read wrong, 3882 MHz is garbage
        assert_eq!(read_sensors("0000:01:00.0").clock_mhz, None);
        // another card with the same value is not affected
        assert_eq!(read_sensors("0000:03:00.0").clock_mhz, Some(3882.0));
        std::fs::write(
            root.join("bus/pci/devices/0000:01:00.0/hwmon/hwmon0/freq1_input"),
            "1500000000",
        )
        .unwrap();
        assert_eq!(read_sensors("0000:01:00.0").clock_mhz, Some(1500.0));
    }

    #[test]
    fn vega20_profiles() {
        let s = "NUM        MODE_NAME     BUSY_SET_POINT      FPS     USE_RLC_BUSY    MIN_ACTIVE_LEVEL\n  0 BOOTUP_DEFAULT*:             0        0               0                0\n  1 3D_FULL_SCREEN :            70       60               1                3\n  2   POWER_SAVING :            90       60               0                0\n  3          VIDEO :            70       60               0                0\n  4             VR :            70       90               0                0\n  5        COMPUTE :            30       60               0                6\n  6         CUSTOM :             0        0               0                0\n";
        let (v, a) = parse_power_profiles(s);
        assert_eq!(a, Some(0));
        assert_eq!(v.len(), 7);
        assert_eq!(v[5], (5, "COMPUTE".to_string()));
        // format of the newer cards (one line per profile + table)
        let n = "PROFILE_INDEX(NAME) CLOCK_TYPE(NAME) FPS MinActiveFreqType\n 0 BOOTUP_DEFAULT :\n 1 3D_FULL_SCREEN*:\n                    0(       GFXCLK)       1\n";
        let (v, a) = parse_power_profiles(n);
        assert_eq!(a, Some(1));
        assert_eq!(v.len(), 2);
        assert_eq!(gen_of("8.0 GT/s PCIe"), Some(3));
        assert_eq!(gen_of("2.5 GT/s PCIe"), Some(1));
        assert_eq!(gen_of("16.0 GT/s PCIe"), Some(4));
    }
}
