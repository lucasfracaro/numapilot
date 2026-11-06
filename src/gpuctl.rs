//! Sensores e controle das GPUs AMD pelo hwmon do amdgpu: temperaturas, ventoinha
//! (curva / fixa / automática) e limite de potência. Leitura: qualquer usuário.
//! Escrita: só o daemon (root), pedido pela interface.

use crate::learned::state_dir;
use crate::util::{read_trim, sys};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::PathBuf;

/// Temperatura em que a ventoinha vai a 100% independentemente da curva.
pub const EMERGENCY_C: f64 = 95.0;

pub fn hwmon_dir(pci: &str) -> Option<PathBuf> {
    let base = sys(&format!("bus/pci/devices/{pci}/hwmon"));
    let mut v: Vec<PathBuf> = fs::read_dir(base).ok()?.flatten().map(|e| e.path()).collect();
    v.sort();
    v.into_iter().next()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Sensors {
    pub edge_c: Option<f64>,
    pub junction_c: Option<f64>,
    pub mem_c: Option<f64>,
    pub fan_rpm: Option<u64>,
    /// Ventoinha atual em % (do pwm1).
    pub fan_pct: Option<f64>,
    /// 1 = manual, 2 = automático (firmware)
    pub fan_mode_raw: Option<u8>,
    pub fan_controllable: bool,
    pub power_w: Option<f64>,
    pub power_cap_w: Option<f64>,
    pub power_cap_min_w: Option<f64>,
    pub power_cap_max_w: Option<f64>,
}

impl Sensors {
    /// Temperatura usada para a curva: junction (hotspot) se houver, senão edge.
    pub fn control_temp(&self) -> Option<f64> {
        self.junction_c.or(self.edge_c)
    }
}

fn num(p: PathBuf) -> Option<f64> {
    read_trim(p)?.parse().ok()
}

pub fn read_sensors(pci: &str) -> Sensors {
    let Some(h) = hwmon_dir(pci) else { return Sensors::default() };
    let mut s = Sensors::default();
    for i in 1..=4 {
        let label = read_trim(h.join(format!("temp{i}_label"))).unwrap_or_default();
        let Some(v) = num(h.join(format!("temp{i}_input"))) else { continue };
        let c = v / 1000.0;
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
    s.fan_controllable = h.join("pwm1").exists() && h.join("pwm1_enable").exists();
    let w = |f: &str| num(h.join(f)).map(|v| v / 1_000_000.0);
    s.power_w = w("power1_average").or_else(|| w("power1_input"));
    s.power_cap_w = w("power1_cap");
    s.power_cap_min_w = w("power1_cap_min");
    s.power_cap_max_w = w("power1_cap_max");
    s
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FanMode {
    /// O firmware da placa decide (padrão).
    Auto,
    /// Curva temperatura → % controlada pelo daemon.
    Curve,
    /// Velocidade fixa.
    Fixed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuControl {
    pub fan: FanMode,
    /// Pontos (°C, %) em ordem crescente de temperatura.
    pub curve: Vec<(f64, f64)>,
    pub fixed_pct: f64,
    /// Limite de potência em W (None = padrão da placa).
    pub power_cap_w: Option<f64>,
}

pub fn default_curve() -> Vec<(f64, f64)> {
    vec![(40.0, 25.0), (60.0, 40.0), (75.0, 65.0), (85.0, 90.0), (95.0, 100.0)]
}

impl Default for GpuControl {
    fn default() -> Self {
        GpuControl { fan: FanMode::Auto, curve: default_curve(), fixed_pct: 50.0, power_cap_w: None }
    }
}

impl GpuControl {
    pub fn validate(&mut self) -> Result<(), String> {
        if self.curve.len() < 2 {
            return Err("a curva precisa de pelo menos 2 pontos".into());
        }
        self.curve.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for p in &mut self.curve {
            p.1 = p.1.clamp(0.0, 100.0);
        }
        self.fixed_pct = self.fixed_pct.clamp(0.0, 100.0);
        Ok(())
    }

    /// % da ventoinha para uma temperatura (com a regra de emergência).
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

/// Interpolação linear na curva (abaixo do 1º ponto = 1º valor; acima do último = último).
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
        fs::write(&tmp, serde_json::to_string_pretty(&self.map).unwrap_or_default())?;
        fs::rename(tmp, Self::path())
    }
    pub fn get(&self, pci: &str) -> GpuControl {
        self.map.get(pci).cloned().unwrap_or_default()
    }
}

// ---------- escrita (daemon) ----------

pub fn set_fan_auto(pci: &str) -> io::Result<()> {
    let h = hwmon_dir(pci).ok_or_else(|| io::Error::other("sem hwmon"))?;
    fs::write(h.join("pwm1_enable"), "2")
}

pub fn set_fan_pct(pci: &str, pct: f64) -> io::Result<()> {
    let h = hwmon_dir(pci).ok_or_else(|| io::Error::other("sem hwmon"))?;
    let max = num(h.join("pwm1_max")).unwrap_or(255.0);
    if read_trim(h.join("pwm1_enable")).as_deref() != Some("1") {
        fs::write(h.join("pwm1_enable"), "1")?;
    }
    let v = (pct.clamp(0.0, 100.0) * max / 100.0).round() as u64;
    fs::write(h.join("pwm1"), v.to_string())
}

/// Limite de potência em W (respeitando o mínimo e o máximo da placa). None = volta ao padrão.
pub fn set_power_cap(pci: &str, watts: Option<f64>) -> io::Result<f64> {
    let h = hwmon_dir(pci).ok_or_else(|| io::Error::other("sem hwmon"))?;
    let w = |f: &str| num(h.join(f)).map(|v| v / 1_000_000.0);
    let target = match watts {
        Some(x) => x,
        None => w("power1_cap_default")
            .or_else(|| w("power1_cap_max"))
            .ok_or_else(|| io::Error::other("sem limite padrão"))?,
    };
    let lo = w("power1_cap_min").unwrap_or(0.0);
    let hi = w("power1_cap_max").unwrap_or(target);
    let v = target.clamp(lo, hi);
    fs::write(h.join("power1_cap"), ((v * 1_000_000.0).round() as u64).to_string())?;
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
        let mut g = GpuControl { fan: FanMode::Fixed, fixed_pct: 30.0, ..Default::default() };
        assert_eq!(g.target_pct(Some(60.0)), Some(30.0));
        assert_eq!(g.target_pct(Some(96.0)), Some(100.0), "emergência ignora o modo");
        g.fan = FanMode::Auto;
        assert_eq!(g.target_pct(Some(60.0)), None);
        assert_eq!(g.target_pct(None), None);
        let mut bad = GpuControl { curve: vec![(70.0, 150.0), (30.0, 10.0)], ..Default::default() };
        bad.validate().unwrap();
        assert_eq!(bad.curve, vec![(30.0, 10.0), (70.0, 100.0)]);
    }
}
