//! Controles do processador pelo sysfs: estados de economia (C-states), turbo, frequências,
//! viés de energia (EPB), frequência do "uncore" (L3 + controlador de memória) e limite de
//! potência por socket (RAPL).

use crate::util::{read_trim, read_u64, sys};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

fn w(p: PathBuf, v: &str) -> Result<(), String> {
    fs::write(&p, v).map_err(|e| format!("{}: {e}", p.display()))
}

// ---------------- C-states ----------------

#[derive(Debug, Clone, PartialEq)]
pub struct CState {
    pub index: usize,
    pub name: String,
    pub latency_us: u64,
    pub disabled: bool,
}

fn cstate_dir(cpu: usize, idx: usize) -> PathBuf {
    sys(&format!("devices/system/cpu/cpu{cpu}/cpuidle/state{idx}"))
}

pub fn cstates(cpu: usize) -> Vec<CState> {
    (0..16)
        .map_while(|i| {
            let d = cstate_dir(cpu, i);
            let name = read_trim(d.join("name"))?;
            Some(CState {
                index: i,
                name,
                latency_us: read_u64(d.join("latency")).unwrap_or(0),
                disabled: read_trim(d.join("disable")).as_deref() == Some("1"),
            })
        })
        .collect()
}

pub fn set_cstate_disabled(cpu: usize, idx: usize, off: bool) -> Result<(), String> {
    w(
        cstate_dir(cpu, idx).join("disable"),
        if off { "1" } else { "0" },
    )
}

/// Desliga, nos CPUs dados, os estados com latência de saída acima de `max_us`.
/// Devolve os (cpu, estado) que estavam ligados e foram desligados (para devolver depois).
pub fn limit_latency(cpus: &[usize], max_us: u64) -> Vec<(usize, usize)> {
    let mut changed = Vec::new();
    for &c in cpus {
        for s in cstates(c) {
            if s.index > 0
                && s.latency_us > max_us
                && !s.disabled
                && set_cstate_disabled(c, s.index, true).is_ok()
            {
                changed.push((c, s.index));
            }
        }
    }
    changed
}

pub fn restore_cstates(list: &[(usize, usize)]) {
    for &(c, i) in list {
        let _ = set_cstate_disabled(c, i, false);
    }
}

// ---------------- turbo ----------------

/// Turbo ligado? (intel_pstate/no_turbo ou cpufreq/boost)
pub fn turbo() -> Option<bool> {
    if let Some(v) = read_trim(sys("devices/system/cpu/intel_pstate/no_turbo")) {
        return Some(v == "0");
    }
    read_trim(sys("devices/system/cpu/cpufreq/boost")).map(|v| v == "1")
}

pub fn set_turbo(on: bool) -> Result<(), String> {
    let p = sys("devices/system/cpu/intel_pstate/no_turbo");
    if p.exists() {
        return w(p, if on { "0" } else { "1" });
    }
    let b = sys("devices/system/cpu/cpufreq/boost");
    if b.exists() {
        return w(b, if on { "1" } else { "0" });
    }
    Err("este sistema não expõe o controle do turbo".into())
}

// ---------------- frequência / governor / EPB ----------------

fn cpufreq(cpu: usize, f: &str) -> PathBuf {
    sys(&format!("devices/system/cpu/cpu{cpu}/cpufreq/{f}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Freq {
    pub min_khz: u64,
    pub max_khz: u64,
    pub hw_min_khz: u64,
    pub hw_max_khz: u64,
    pub cur_khz: u64,
}

pub fn freq(cpu: usize) -> Option<Freq> {
    Some(Freq {
        min_khz: read_u64(cpufreq(cpu, "scaling_min_freq"))?,
        max_khz: read_u64(cpufreq(cpu, "scaling_max_freq"))?,
        hw_min_khz: read_u64(cpufreq(cpu, "cpuinfo_min_freq")).unwrap_or(0),
        hw_max_khz: read_u64(cpufreq(cpu, "cpuinfo_max_freq")).unwrap_or(0),
        cur_khz: read_u64(cpufreq(cpu, "scaling_cur_freq")).unwrap_or(0),
    })
}

/// Define mínimo e máximo (kHz). A ordem importa para não ficar mínimo > máximo no meio.
pub fn set_freq(cpus: &[usize], min_khz: u64, max_khz: u64) -> Result<(), String> {
    let mut err = None;
    for &c in cpus {
        let cur_max = read_u64(cpufreq(c, "scaling_max_freq")).unwrap_or(u64::MAX);
        let r = if min_khz > cur_max {
            w(cpufreq(c, "scaling_max_freq"), &max_khz.to_string())
                .and(w(cpufreq(c, "scaling_min_freq"), &min_khz.to_string()))
        } else {
            w(cpufreq(c, "scaling_min_freq"), &min_khz.to_string())
                .and(w(cpufreq(c, "scaling_max_freq"), &max_khz.to_string()))
        };
        if let Err(e) = r {
            err = Some(e);
        }
    }
    err.map_or(Ok(()), Err)
}

pub fn governor(cpu: usize) -> Option<String> {
    read_trim(cpufreq(cpu, "scaling_governor"))
}

pub fn governors_available(cpu: usize) -> Vec<String> {
    read_trim(cpufreq(cpu, "scaling_available_governors"))
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

pub fn set_governor(cpus: &[usize], gov: &str) -> Result<(), String> {
    let mut err = None;
    for &c in cpus {
        if let Err(e) = w(cpufreq(c, "scaling_governor"), gov) {
            err = Some(e);
        }
    }
    err.map_or(Ok(()), Err)
}

/// Governor "econômico" que ainda acompanha a carga: powersave no intel_pstate,
/// schedutil nos outros drivers (o powersave do acpi-cpufreq trava no mínimo).
pub fn eco_governor(cpu: usize) -> String {
    let driver = read_trim(cpufreq(cpu, "scaling_driver")).unwrap_or_default();
    let av = governors_available(cpu);
    if driver.starts_with("intel_pstate") || driver.starts_with("amd-pstate") {
        "powersave".into()
    } else if av.iter().any(|g| g == "schedutil") {
        "schedutil".into()
    } else {
        "ondemand".into()
    }
}

/// Energy Performance Bias: 0 = desempenho ... 15 = economia.
pub fn epb(cpu: usize) -> Option<u8> {
    read_trim(sys(&format!(
        "devices/system/cpu/cpu{cpu}/power/energy_perf_bias"
    )))?
    .parse()
    .ok()
}

pub fn set_epb(cpus: &[usize], v: u8) -> Result<(), String> {
    let mut err = None;
    for &c in cpus {
        if let Err(e) = w(
            sys(&format!("devices/system/cpu/cpu{c}/power/energy_perf_bias")),
            &v.to_string(),
        ) {
            err = Some(e);
        }
    }
    err.map_or(Ok(()), Err)
}

/// Energy Performance Preference (amd-pstate/intel_pstate em modo ativo):
/// performance, balance_performance, balance_power, power.
pub fn epp(cpu: usize) -> Option<String> {
    read_trim(cpufreq(cpu, "energy_performance_preference"))
}

pub fn epp_available(cpu: usize) -> Vec<String> {
    read_trim(cpufreq(cpu, "energy_performance_available_preferences"))
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

/// Grava o EPP. Com o governor performance o driver trava o EPP em performance: isso não é erro.
pub fn set_epp(cpus: &[usize], v: &str) -> Result<(), String> {
    let mut err = None;
    for &c in cpus {
        if governor(c).as_deref() == Some("performance") {
            continue;
        }
        if let Err(e) = w(cpufreq(c, "energy_performance_preference"), v) {
            err = Some(e);
        }
    }
    err.map_or(Ok(()), Err)
}

// ---------------- uncore ----------------

#[derive(Debug, Clone, PartialEq)]
pub struct Uncore {
    pub dir: PathBuf,
    pub package: u32,
    pub min_khz: u64,
    pub max_khz: u64,
    pub init_min_khz: u64,
    pub init_max_khz: u64,
    pub cur_khz: Option<u64>,
}

/// Zonas do driver intel_uncore_frequency (uma por socket).
pub fn uncores() -> Vec<Uncore> {
    let base = sys("devices/system/cpu/intel_uncore_frequency");
    let Ok(rd) = fs::read_dir(&base) else {
        return vec![];
    };
    let mut v: Vec<Uncore> = rd
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            // package_00_die_00 (driver antigo) — os "uncoreNN" novos têm package_id dentro
            let package = if let Some(rest) = n.strip_prefix("package_") {
                rest.split('_').next()?.parse().ok()?
            } else if n.starts_with("uncore") {
                read_u64(e.path().join("package_id"))? as u32
            } else {
                return None;
            };
            let d = e.path();
            Some(Uncore {
                package,
                min_khz: read_u64(d.join("min_freq_khz"))?,
                max_khz: read_u64(d.join("max_freq_khz"))?,
                init_min_khz: read_u64(d.join("initial_min_freq_khz")).unwrap_or(0),
                init_max_khz: read_u64(d.join("initial_max_freq_khz")).unwrap_or(0),
                cur_khz: read_u64(d.join("current_freq_khz")),
                dir: d,
            })
        })
        .collect();
    v.sort_by_key(|u| u.package);
    v.dedup_by_key(|u| u.package);
    v
}

pub fn set_uncore(package: u32, min_khz: u64, max_khz: u64) -> Result<(), String> {
    let u = uncores()
        .into_iter()
        .find(|u| u.package == package)
        .ok_or("uncore indisponível (modprobe intel_uncore_frequency)")?;
    if min_khz > u.max_khz {
        w(u.dir.join("max_freq_khz"), &max_khz.to_string())?;
        w(u.dir.join("min_freq_khz"), &min_khz.to_string())
    } else {
        w(u.dir.join("min_freq_khz"), &min_khz.to_string())?;
        w(u.dir.join("max_freq_khz"), &max_khz.to_string())
    }
}

/// Volta para os valores de fábrica.
pub fn reset_uncore(package: u32) -> Result<(), String> {
    let u = uncores()
        .into_iter()
        .find(|u| u.package == package)
        .ok_or("uncore indisponível")?;
    if u.init_max_khz == 0 {
        return Ok(());
    }
    set_uncore(package, u.init_min_khz, u.init_max_khz)
}

// ---------------- RAPL ----------------

#[derive(Debug, Clone, PartialEq)]
pub struct Rapl {
    pub dir: PathBuf,
    pub package: u32,
    /// Limite de longo prazo atual (W).
    pub limit_w: Option<f64>,
    /// Potência de projeto (TDP), quando o kernel mostra.
    pub max_w: Option<f64>,
    /// Limite de curto prazo (PL2), quando existe.
    pub limit2_w: Option<f64>,
}

pub fn rapl_zones() -> Vec<Rapl> {
    let base = sys("class/powercap");
    let Ok(rd) = fs::read_dir(&base) else {
        return vec![];
    };
    let mut v: Vec<Rapl> = rd
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            // só as zonas de pacote (intel-rapl:0, intel-rapl:1), não as subzonas (intel-rapl:0:0)
            let rest = n.strip_prefix("intel-rapl:")?;
            if rest.contains(':') {
                return None;
            }
            let d = e.path();
            let name = read_trim(d.join("name"))?;
            let package = name.strip_prefix("package-")?.parse().ok()?;
            let uw = |f: &str| read_u64(d.join(f)).map(|v| v as f64 / 1e6);
            Some(Rapl {
                package,
                limit_w: uw("constraint_0_power_limit_uw"),
                max_w: uw("constraint_0_max_power_uw"),
                limit2_w: uw("constraint_1_power_limit_uw"),
                dir: d,
            })
        })
        .collect();
    v.sort_by_key(|r| r.package);
    v
}

pub fn set_rapl_limit(package: u32, watts: f64) -> Result<(), String> {
    let z = rapl_zones()
        .into_iter()
        .find(|r| r.package == package)
        .ok_or("RAPL indisponível neste socket")?;
    if watts < 20.0 {
        return Err("limite muito baixo (mínimo 20 W)".into());
    }
    w(
        z.dir.join("constraint_0_power_limit_uw"),
        &((watts * 1e6) as u64).to_string(),
    )
}

/// Limite de curto prazo (PL2: o pico que o processador pode puxar por alguns segundos).
pub fn set_rapl_limit2(package: u32, watts: f64) -> Result<(), String> {
    let z = rapl_zones()
        .into_iter()
        .find(|r| r.package == package)
        .ok_or("RAPL indisponível neste socket")?;
    if watts < 20.0 {
        return Err("limite muito baixo (mínimo 20 W)".into());
    }
    w(
        z.dir.join("constraint_1_power_limit_uw"),
        &((watts * 1e6) as u64).to_string(),
    )
}

/// Mede a potência de cada socket pela diferença do contador de energia (precisa de root).
#[derive(Default)]
pub struct PowerMeter {
    last: BTreeMap<u32, (u64, Instant)>,
}

impl PowerMeter {
    pub fn sample(&mut self) -> BTreeMap<u32, f64> {
        let mut out = BTreeMap::new();
        for z in rapl_zones() {
            let Some(e) = read_u64(z.dir.join("energy_uj")) else {
                continue;
            };
            let max = read_u64(z.dir.join("max_energy_range_uj")).unwrap_or(u64::MAX);
            let now = Instant::now();
            if let Some((pe, pt)) = self.last.get(&z.package) {
                let de = if e >= *pe { e - pe } else { max - pe + e };
                let dt = now.duration_since(*pt).as_secs_f64();
                if dt > 0.05 {
                    out.insert(z.package, de as f64 / 1e6 / dt);
                }
            }
            self.last.insert(z.package, (e, now));
        }
        out
    }
}

/// Temperatura do pacote (coretemp "Package id N").
pub fn package_temps() -> BTreeMap<u32, f64> {
    let mut out = BTreeMap::new();
    let Ok(rd) = fs::read_dir(sys("class/hwmon")) else {
        return out;
    };
    // AMD (k10temp/zenpower): um por socket, na ordem do barramento; Tctl/Tdie
    let mut amd: Vec<(String, f64)> = vec![];
    let entries: Vec<_> = rd.flatten().collect();
    for e in &entries {
        let d = e.path();
        if matches!(
            read_trim(d.join("name")).as_deref(),
            Some("k10temp") | Some("zenpower")
        ) {
            let dev = fs::canonicalize(d.join("device"))
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            for i in 1..=4 {
                let l = read_trim(d.join(format!("temp{i}_label"))).unwrap_or_default();
                if (l == "Tctl" || l == "Tdie")
                    && let Some(v) = read_u64(d.join(format!("temp{i}_input")))
                {
                    amd.push((dev.clone(), v as f64 / 1000.0));
                    break;
                }
            }
        }
    }
    amd.sort_by(|a, b| a.0.cmp(&b.0));
    for (i, (_, t)) in amd.into_iter().enumerate() {
        out.insert(i as u32, t);
    }
    for e in entries {
        let d = e.path();
        if read_trim(d.join("name")).as_deref() != Some("coretemp") {
            continue;
        }
        for i in 1..=4 {
            let Some(l) = read_trim(d.join(format!("temp{i}_label"))) else {
                continue;
            };
            if let Some(p) = l
                .strip_prefix("Package id ")
                .and_then(|x| x.trim().parse().ok())
                && let Some(v) = read_u64(d.join(format!("temp{i}_input")))
            {
                out.insert(p, v as f64 / 1000.0);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_cpu_controls() {
        let root = std::env::temp_dir().join("numapilot-cpuctl-test");
        let _ = fs::remove_dir_all(&root);
        let mk = |p: &str, v: &str| {
            let f = root.join(p);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, v).unwrap();
        };
        for (i, (n, l)) in [("POLL", 0), ("C1", 2), ("C1E", 10), ("C6", 133)]
            .iter()
            .enumerate()
        {
            mk(&format!("devices/system/cpu/cpu0/cpuidle/state{i}/name"), n);
            mk(
                &format!("devices/system/cpu/cpu0/cpuidle/state{i}/latency"),
                &l.to_string(),
            );
            mk(
                &format!("devices/system/cpu/cpu0/cpuidle/state{i}/disable"),
                "0",
            );
        }
        mk("devices/system/cpu/intel_pstate/no_turbo", "0");
        mk(
            "devices/system/cpu/intel_uncore_frequency/package_01_die_00/min_freq_khz",
            "1200000",
        );
        mk(
            "devices/system/cpu/intel_uncore_frequency/package_01_die_00/max_freq_khz",
            "2800000",
        );
        mk(
            "devices/system/cpu/intel_uncore_frequency/package_01_die_00/initial_min_freq_khz",
            "1200000",
        );
        mk(
            "devices/system/cpu/intel_uncore_frequency/package_01_die_00/initial_max_freq_khz",
            "2800000",
        );
        mk("class/powercap/intel-rapl:1/name", "package-1");
        mk(
            "class/powercap/intel-rapl:1/constraint_0_power_limit_uw",
            "150000000",
        );
        mk(
            "class/powercap/intel-rapl:1/constraint_0_max_power_uw",
            "150000000",
        );
        mk("class/powercap/intel-rapl:1:0/name", "dram");
        mk("class/hwmon/hwmon3/name", "coretemp");
        mk("class/hwmon/hwmon3/temp1_label", "Package id 1");
        mk("class/hwmon/hwmon3/temp1_input", "54000");

        // As funções leem NUMAPILOT_SYSFS; este teste roda sozinho no seu próprio caminho.
        let _g = crate::util::test_sysfs_lock(&root);
        let ch = limit_latency(&[0], 2);
        assert_eq!(ch, vec![(0, 2), (0, 3)]);
        assert!(cstates(0)[3].disabled);
        restore_cstates(&ch);
        assert!(!cstates(0)[3].disabled);
        assert_eq!(turbo(), Some(true));
        set_turbo(false).unwrap();
        assert_eq!(turbo(), Some(false));
        let u = uncores();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].package, 1);
        set_uncore(1, 2800000, 2800000).unwrap();
        assert_eq!(uncores()[0].min_khz, 2800000);
        reset_uncore(1).unwrap();
        assert_eq!(uncores()[0].min_khz, 1200000);
        let r = rapl_zones();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].limit_w, Some(150.0));
        set_rapl_limit(1, 90.0).unwrap();
        assert_eq!(rapl_zones()[0].limit_w, Some(90.0));
        assert_eq!(package_temps().get(&1), Some(&54.0));
    }
}
