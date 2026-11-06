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
    /// Clock/voltagem (OverDrive). None = padrão de fábrica.
    pub od: Option<OdSettings>,
}

pub fn default_curve() -> Vec<(f64, f64)> {
    vec![(40.0, 25.0), (60.0, 40.0), (75.0, 65.0), (85.0, 90.0), (95.0, 100.0)]
}

impl Default for GpuControl {
    fn default() -> Self {
        GpuControl { fan: FanMode::Auto, curve: default_curve(), fixed_pct: 50.0, power_cap_w: None, od: None }
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

// ---------- clock e voltagem (OverDrive) ----------

/// Tabela do pp_od_clk_voltage (Vega 20 / MI50).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Od {
    /// (índice, MHz) — 0 = mínimo, 1 = máximo
    pub sclk: Vec<(u32, u32)>,
    pub mclk: Vec<(u32, u32)>,
    /// (índice, MHz, mV) — os 3 pontos da curva de voltagem
    pub curve: Vec<(u32, u32, u32)>,
    pub range_sclk: Option<(u32, u32)>,
    pub range_mclk: Option<(u32, u32)>,
    /// por ponto: faixa de MHz e de mV
    pub range_curve_sclk: Vec<(u32, u32)>,
    pub range_curve_volt: Vec<(u32, u32)>,
}

fn nums(s: &str) -> Vec<u32> {
    s.split(|c: char| !c.is_ascii_digit()).filter(|x| !x.is_empty()).filter_map(|x| x.parse().ok()).collect()
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
            "r" => {
                if l.starts_with("SCLK:") && n.len() >= 2 {
                    od.range_sclk = Some((n[0], n[1]));
                } else if l.starts_with("MCLK:") && n.len() >= 2 {
                    od.range_mclk = Some((n[0], n[1]));
                } else if l.starts_with("VDDC_CURVE_SCLK[") && n.len() >= 3 {
                    od.range_curve_sclk.push((n[1], n[2]));
                } else if (l.starts_with("VDDC_CURVE_VOLT[") || l.starts_with("VDDC_CURVE_VOFF[")) && n.len() >= 3 {
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

/// None = OverDrive desligado (falta amdgpu.ppfeaturemask com o bit 0x4000).
pub fn read_od(pci: &str) -> Option<Od> {
    let s = fs::read_to_string(od_path(pci)).ok()?;
    let od = parse_od(&s);
    if od.sclk.is_empty() && od.curve.is_empty() { None } else { Some(od) }
}

/// O que o usuário escolheu (None/vazio = manter o padrão da placa).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct OdSettings {
    pub sclk_max: Option<u32>,
    pub mclk_max: Option<u32>,
    /// (MHz, mV) para cada um dos 3 pontos
    pub curve: Vec<(u32, u32)>,
}

impl OdSettings {
    pub fn is_default(&self) -> bool {
        self.sclk_max.is_none() && self.mclk_max.is_none() && self.curve.is_empty()
    }
}

/// Comandos para o pp_od_clk_voltage, validando as faixas da placa.
pub fn od_commands(od: &Od, s: &OdSettings) -> Result<Vec<String>, String> {
    let mut v = Vec::new();
    let check = |val: u32, r: Option<(u32, u32)>, what: &str| -> Result<(), String> {
        match r {
            Some((lo, hi)) if val < lo || val > hi => Err(format!("{what} {val} fora da faixa {lo}–{hi}")),
            _ => Ok(()),
        }
    };
    if let Some(x) = s.sclk_max {
        check(x, od.range_sclk, "clock da GPU (MHz)")?;
        v.push(format!("s 1 {x}"));
    }
    if let Some(x) = s.mclk_max {
        check(x, od.range_mclk, "clock da memória (MHz)")?;
        v.push(format!("m 1 {x}"));
    }
    for (i, (mhz, mv)) in s.curve.iter().enumerate() {
        check(*mhz, od.range_curve_sclk.get(i).copied(), &format!("ponto {i} (MHz)"))?;
        check(*mv, od.range_curve_volt.get(i).copied(), &format!("ponto {i} (mV)"))?;
        v.push(format!("vc {i} {mhz} {mv}"));
    }
    v.push("c".into());
    Ok(v)
}

/// Aplica (ou, com `None`, volta ao padrão de fábrica).
pub fn apply_od(pci: &str, s: Option<&OdSettings>) -> Result<(), String> {
    let p = od_path(pci);
    let lvl = sys(&format!("bus/pci/devices/{pci}/power_dpm_force_performance_level"));
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

/// (VRAM visível pela CPU, VRAM total) em bytes. Se forem ~iguais, o Resizable BAR está ativo.
pub fn bar_info(pci: &str) -> Option<(u64, u64)> {
    let r = |f: &str| read_trim(sys(&format!("bus/pci/devices/{pci}/{f}")))?.parse::<u64>().ok();
    Some((r("mem_info_vis_vram_total")?, r("mem_info_vram_total")?))
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
        let s = OdSettings { sclk_max: Some(1700), mclk_max: None, curve: vec![(808, 740), (1304, 800), (1700, 1000)] };
        assert_eq!(od_commands(&od, &s).unwrap(), vec!["s 1 1700", "vc 0 808 740", "vc 1 1304 800", "vc 2 1700 1000", "c"]);
        let bad = OdSettings { curve: vec![(808, 600)], ..Default::default() };
        assert!(od_commands(&od, &bad).unwrap_err().contains("fora da faixa"));
    }
}
