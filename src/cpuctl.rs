//! Processor controls through sysfs: idle states (C-states), turbo, frequencies, energy bias
//! (EPB), "uncore" frequency (L3 + memory controller) and power limit per socket (RAPL).

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

/// Disables, on the given CPUs, the states with exit latency above `max_us`.
/// Returns the (cpu, state) pairs that were enabled and got disabled (to restore later).
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

/// Turbo on? (intel_pstate/no_turbo or cpufreq/boost)
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
    Err(crate::i18n::tr("this system does not expose turbo control").into())
}

// ---------------- frequency / governor / EPB ----------------

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

/// Current MHz of each logical CPU: cpufreq (scaling_cur_freq) or, without it (BC-250), the
/// "cpu MHz" from /proc/cpuinfo, which the kernel measures through APERF/MPERF.
pub fn cur_mhz_all() -> std::collections::BTreeMap<usize, u64> {
    let mut m = std::collections::BTreeMap::new();
    let txt = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    m.extend(parse_cpuinfo_mhz(&txt));
    for (c, v) in m.iter_mut() {
        if let Some(k) = read_u64(cpufreq(*c, "scaling_cur_freq")) {
            *v = k / 1000;
        }
    }
    m
}

pub fn parse_cpuinfo_mhz(txt: &str) -> Vec<(usize, u64)> {
    let mut v = vec![];
    let mut cpu = None;
    for l in txt.lines() {
        let Some((k, val)) = l.split_once(':') else {
            continue;
        };
        match k.trim() {
            "processor" => cpu = val.trim().parse::<usize>().ok(),
            "cpu MHz" => {
                if let (Some(c), Ok(f)) = (cpu, val.trim().parse::<f64>()) {
                    v.push((c, f.round() as u64));
                }
            }
            _ => {}
        }
    }
    v
}

/// Sets minimum and maximum (kHz). The order matters so minimum > maximum never happens midway.
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

/// "Power-saving" governor that still follows the load: powersave on intel_pstate,
/// schedutil on the other drivers (acpi-cpufreq's powersave locks at the minimum).
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

/// Energy Performance Bias: 0 = performance ... 15 = power saving.
pub fn epb(cpu: usize) -> Option<u8> {
    read_trim(sys(&format!(
        "devices/system/cpu/cpu{cpu}/power/energy_perf_bias"
    )))?
    .parse()
    .ok()
}

static EPB_UNSUPPORTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Does the processor accept changing EPB? (on AMD the file exists but the write is denied)
pub fn epb_supported() -> bool {
    !EPB_UNSUPPORTED.load(std::sync::atomic::Ordering::Relaxed)
        && std::path::Path::new(&sys("devices/system/cpu/cpu0/power/energy_perf_bias")).exists()
}

pub fn set_epb(cpus: &[usize], v: u8) -> Result<(), String> {
    if !epb_supported() {
        return Ok(());
    }
    let mut err = None;
    for &c in cpus {
        if let Err(e) = w(
            sys(&format!("devices/system/cpu/cpu{c}/power/energy_perf_bias")),
            &v.to_string(),
        ) {
            // denied even for root: this processor does not allow changing it; not an error
            if e.contains("Permission denied") || e.contains("os error 13") {
                EPB_UNSUPPORTED.store(true, std::sync::atomic::Ordering::Relaxed);
                return Ok(());
            }
            err = Some(e);
        }
    }
    err.map_or(Ok(()), Err)
}

/// Energy Performance Preference (amd-pstate/intel_pstate in active mode):
/// performance, balance_performance, balance_power, power.
pub fn epp(cpu: usize) -> Option<String> {
    read_trim(cpufreq(cpu, "energy_performance_preference"))
}

pub fn epp_available(cpu: usize) -> Vec<String> {
    read_trim(cpufreq(cpu, "energy_performance_available_preferences"))
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

/// Writes EPP. With the performance governor the driver locks EPP to performance: not an error.
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

/// Zones of the intel_uncore_frequency driver (one per socket).
pub fn uncores() -> Vec<Uncore> {
    let base = sys("devices/system/cpu/intel_uncore_frequency");
    let Ok(rd) = fs::read_dir(&base) else {
        return vec![];
    };
    let mut v: Vec<Uncore> = rd
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            // package_00_die_00 (old driver) — the new "uncoreNN" ones have package_id inside
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
        .ok_or(crate::i18n::tr(
            "uncore unavailable (modprobe intel_uncore_frequency)",
        ))?;
    if min_khz > u.max_khz {
        w(u.dir.join("max_freq_khz"), &max_khz.to_string())?;
        w(u.dir.join("min_freq_khz"), &min_khz.to_string())
    } else {
        w(u.dir.join("min_freq_khz"), &min_khz.to_string())?;
        w(u.dir.join("max_freq_khz"), &max_khz.to_string())
    }
}

/// Back to the factory values.
pub fn reset_uncore(package: u32) -> Result<(), String> {
    let u = uncores()
        .into_iter()
        .find(|u| u.package == package)
        .ok_or(crate::i18n::tr("uncore unavailable"))?;
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
    /// Current long-term limit (W).
    pub limit_w: Option<f64>,
    /// Design power (TDP), when the kernel shows it.
    pub max_w: Option<f64>,
    /// Short-term limit (PL2), when it exists.
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
            // only the package zones (intel-rapl:0, intel-rapl:1), not the subzones (intel-rapl:0:0)
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
        .ok_or(crate::i18n::tr("RAPL unavailable on this socket"))?;
    if watts < 20.0 {
        return Err(crate::i18n::tr("limit too low (minimum 20 W)").into());
    }
    w(
        z.dir.join("constraint_0_power_limit_uw"),
        &((watts * 1e6) as u64).to_string(),
    )
}

/// Short-term limit (PL2: the peak the processor may draw for a few seconds).
pub fn set_rapl_limit2(package: u32, watts: f64) -> Result<(), String> {
    let z = rapl_zones()
        .into_iter()
        .find(|r| r.package == package)
        .ok_or(crate::i18n::tr("RAPL unavailable on this socket"))?;
    if watts < 20.0 {
        return Err(crate::i18n::tr("limit too low (minimum 20 W)").into());
    }
    w(
        z.dir.join("constraint_1_power_limit_uw"),
        &((watts * 1e6) as u64).to_string(),
    )
}

/// Measures each socket's power from the energy counter difference (needs root).
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
        // BC-250: no RAPL, but the VRM reports the CPU rail
        if out.is_empty()
            && let Some(w) = crate::boards::vrm().and_then(|v| v.cpu_w)
        {
            out.insert(0, w);
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
    // AMD (k10temp/zenpower): one per socket, in bus order; Tctl/Tdie
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
    #[test]
    fn cpuinfo_mhz() {
        let t = "processor\t: 0\ncpu MHz\t\t: 3493.236\n\nprocessor\t: 1\ncpu MHz\t\t: 1200.0\n";
        assert_eq!(super::parse_cpuinfo_mhz(t), vec![(0, 3493), (1, 1200)]);
    }

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

        // The functions read NUMAPILOT_SYSFS; this test runs alone on its own path.
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
