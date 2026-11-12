//! Boards with community tweaks. Today: the BC-250's clock governor (reads and edits its
//! configuration, whatever the format, and restarts the service) and the CPU overclock tools
//! through the SMU.

use crate::recipes::Board;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct Service {
    pub name: String,
    pub active: bool,
    pub enabled: bool,
    pub installed: bool,
}

fn systemctl_ok(args: &[&str]) -> bool {
    Command::new("systemctl")
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn services(b: &Board) -> Vec<Service> {
    b.governor_services
        .iter()
        .map(|s| {
            let unit = format!("{s}.service");
            let installed = ["/usr/lib/systemd/system", "/etc/systemd/system"]
                .iter()
                .any(|d| std::path::Path::new(d).join(&unit).exists());
            Service {
                name: s.clone(),
                active: systemctl_ok(&["is-active", &unit]),
                enabled: systemctl_ok(&["is-enabled", &unit]),
                installed,
            }
        })
        .collect()
}

/// Configuration file of the governor in use (the first that exists).
pub fn governor_config(b: &Board) -> Option<String> {
    b.governor_configs
        .iter()
        .find(|p| std::path::Path::new(p).is_file())
        .cloned()
}

/// An editable numeric value of the configuration ("key" or "table.key").
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Num {
        key: String,
        value: f64,
        int: bool,
    },
    /// List of points [[MHz, mV], ...]
    Points {
        key: String,
        value: Vec<(f64, f64)>,
    },
}

fn walk(prefix: &str, t: &toml::Table, out: &mut Vec<Field>) {
    for (k, v) in t {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::Integer(i) => out.push(Field::Num {
                key,
                value: *i as f64,
                int: true,
            }),
            toml::Value::Float(f) => out.push(Field::Num {
                key,
                value: *f,
                int: false,
            }),
            toml::Value::Table(t) => walk(&key, t, out),
            toml::Value::Array(a) => {
                let pts: Option<Vec<(f64, f64)>> = a
                    .iter()
                    .map(|p| {
                        let arr = p.as_array()?;
                        let n = |x: &toml::Value| {
                            x.as_float().or_else(|| x.as_integer().map(|i| i as f64))
                        };
                        Some((n(arr.first()?)?, n(arr.get(1)?)?))
                    })
                    .collect();
                if let Some(p) = pts.filter(|p| !p.is_empty()) {
                    out.push(Field::Points { key, value: p });
                }
            }
            _ => {}
        }
    }
}

pub fn fields(text: &str) -> Result<Vec<Field>, String> {
    let t: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut v = vec![];
    walk("", &t, &mut v);
    Ok(v)
}

fn set_path(t: &mut toml::Table, key: &str, v: toml::Value) {
    let mut parts: Vec<&str> = key.split('.').collect();
    let last = parts.pop().unwrap_or(key);
    let mut cur = t;
    for p in parts {
        cur = match cur
            .entry(p.to_string())
            .or_insert(toml::Value::Table(Default::default()))
        {
            toml::Value::Table(x) => x,
            _ => return,
        };
    }
    cur.insert(last.to_string(), v);
}

/// Applies the edited fields to the original text (keeps keys we do not know).
pub fn apply_fields(text: &str, f: &[Field]) -> Result<String, String> {
    let mut t: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    for x in f {
        match x {
            Field::Num { key, value, int } => set_path(
                &mut t,
                key,
                if *int {
                    toml::Value::Integer(value.round() as i64)
                } else {
                    toml::Value::Float(*value)
                },
            ),
            Field::Points { key, value } => set_path(
                &mut t,
                key,
                toml::Value::Array(
                    value
                        .iter()
                        .map(|(a, b)| {
                            toml::Value::Array(vec![
                                toml::Value::Integer(*a as i64),
                                toml::Value::Integer(*b as i64),
                            ])
                        })
                        .collect(),
                ),
            ),
        }
    }
    toml::to_string_pretty(&t).map_err(|e| e.to_string())
}

/// Writes the governor's new configuration (with a backup) and restarts the service. Needs root.
pub fn governor_apply(
    b: &Board,
    new_file: &std::path::Path,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    if !crate::util::is_root() {
        return Err(
            crate::i18n::tr("needs root (the interface button asks for the password)").into(),
        );
    }
    let dest = governor_config(b).ok_or(crate::i18n::tr("no governor installed"))?;
    let text = std::fs::read_to_string(new_file).map_err(|e| e.to_string())?;
    toml::from_str::<toml::Table>(&text)
        .map_err(|e| crate::tf!("invalid configuration: {e}", "configuração inválida: {e}"))?;
    let bak = format!("{dest}.numapilot-bak-{}", crate::history::now());
    std::fs::copy(&dest, &bak).map_err(|e| format!("backup: {e}"))?;
    std::fs::write(&dest, text).map_err(|e| e.to_string())?;
    log(&crate::tf!(
        "Wrote {dest} (backup in {bak})",
        "Gravado {dest} (backup em {bak})"
    ));
    let svc = services(b)
        .into_iter()
        .find(|s| s.active || s.enabled)
        .map(|s| s.name)
        .ok_or(crate::i18n::tr("no governor service enabled"))?;
    let ok = Command::new("systemctl")
        .args(["restart", &format!("{svc}.service")])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        std::fs::copy(&bak, &dest).ok();
        return Err(crate::tf!(
            "{svc} did not restart with the new configuration: the previous one was restored",
            "{svc} não reiniciou com a configuração nova: voltei a anterior"
        ));
    }
    log(&crate::tf!("{svc} restarted", "{svc} reiniciado"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn governor_toml_roundtrip() {
        let src = "min_frequency = 1000\nmax_frequency = 2000\nmax_voltage = 950\n\n[timing]\ninterval_ms = 100\n\n[[unknown]]\nx = \"y\"\n\npoints = 5\n";
        let f = fields(src).unwrap();
        assert!(f.iter().any(|x| matches!(x, Field::Num { key, value, .. } if key == "max_frequency" && *value == 2000.0)));
        assert!(
            f.iter()
                .any(|x| matches!(x, Field::Num { key, .. } if key == "timing.interval_ms"))
        );
        let new = apply_fields(
            src,
            &[Field::Num {
                key: "max_frequency".into(),
                value: 2175.0,
                int: true,
            }],
        )
        .unwrap();
        assert!(new.contains("max_frequency = 2175"));
        assert!(new.contains("[[unknown]]"), "mantém o que não conhecemos");
        let pts = "safe_points = [[350, 700], [2000, 950]]\n";
        let f = fields(pts).unwrap();
        assert_eq!(
            f,
            vec![Field::Points {
                key: "safe_points".into(),
                value: vec![(350.0, 700.0), (2000.0, 950.0)]
            }]
        );
        let new = apply_fields(
            pts,
            &[Field::Points {
                key: "safe_points".into(),
                value: vec![(350.0, 700.0), (2175.0, 1050.0)],
            }],
        )
        .unwrap();
        assert!(new.contains("2175") && new.contains("1050"));
    }
}

/// Active physical cores (distinct package/core pairs).
pub fn active_cores() -> usize {
    let mut set = std::collections::BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir(crate::util::sys("devices/system/cpu")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if !n.starts_with("cpu") || !n[3..].chars().all(|c| c.is_ascii_digit()) || n.len() == 3
            {
                continue;
            }
            let t = e.path().join("topology");
            let rd = |f: &str| {
                std::fs::read_to_string(t.join(f))
                    .ok()
                    .map(|s| s.trim().to_string())
            };
            if let (Some(p), Some(c)) = (rd("physical_package_id"), rd("core_id")) {
                set.insert((p, c));
            }
        }
    }
    set.len()
}

/// How the amdgpu in use reads the SMU metrics table on a BC-250 with 8 cores.
/// linux-cachyos-bc250 has picked the table by core count since 2026-08-28
/// (`cs_eight_core_map` marks that code) and dropped `cs_legacy_8core_metrics` on 2026-09-17;
/// a kernel with neither reads the 6-core table. (Logic from BC250 Control Center,
/// MIT, https://github.com/movacx/bc250-control-center.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsDecode {
    /// Kernel reads the 8-core table: the SMU needs the "SMU Reporting Patch" in the BIOS.
    EightCore,
    /// `cs_legacy_8core_metrics=1` set: old option, removed from the kernel.
    Legacy,
    /// Regular kernel: reads the 6-core table.
    Stock,
}

pub fn metrics_decode(legacy: Option<&str>, eight_core_map: bool) -> MetricsDecode {
    if matches!(legacy, Some("Y" | "1")) {
        MetricsDecode::Legacy
    } else if eight_core_map {
        MetricsDecode::EightCore
    } else {
        MetricsDecode::Stock
    }
}

/// What to do when telemetry comes scrambled, for each way of reading the table.
pub fn mismatch_fix(d: MetricsDecode) -> &'static str {
    match d {
        MetricsDecode::EightCore => {
            "o kernel lê a tabela de 8 núcleos, mas a SMU grava a de 6: ligue o \"SMU Reporting \
             Patch\" no biosmod (MeiMeiDXE v3) ou volte para 6 núcleos"
        }
        MetricsDecode::Legacy => {
            "tire amdgpu.cs_legacy_8core_metrics=1 da linha do kernel (Máquina → Parâmetros do \
             kernel): o kernel atual escolhe a tabela sozinho"
        }
        MetricsDecode::Stock => {
            "este kernel lê a tabela de 6 núcleos: desligue o \"SMU Reporting Patch\" no BIOS ou \
             use o kernel linux-cachyos-bc250"
        }
    }
}

/// BC-250 with 8 cores and scrambled GPU temperature/power (0 °C, ~1470 W): the metrics table
/// the SMU writes does not match the one the kernel reads. The clocks are still right.
/// Returns the explanation when that is the case.
pub fn smu_table_mismatch(pci: &str) -> Option<String> {
    if active_cores() != 8 {
        return None;
    }
    let s = crate::gpuctl::read_sensors(pci);
    if s.edge_c.is_some() || s.power_w.is_some() {
        return None;
    }
    let param = |n: &str| crate::util::sys(&format!("module/amdgpu/parameters/{n}"));
    let legacy = crate::util::read_trim(param("cs_legacy_8core_metrics"));
    let d = metrics_decode(legacy.as_deref(), param("cs_eight_core_map").exists());
    Some(format!(
        "8 núcleos com a tabela de métricas da SMU diferente da que o kernel lê: temperatura e \
         potência da GPU vêm embaralhadas (os clocks estão certos). Para corrigir, {}. Com o \
         cyan-skillfish-governor rodando (fix-metrics), o MangoHud lê o gpu_metrics corrigido \
         por ele.",
        mismatch_fix(d)
    ))
}

/// BC-250: the amdgpu of linux-cachyos-bc250 with `bc250_cc_write_mode=3` clears, on load, the
/// bits that disable the shader arrays: all 40 CUs come from boot and the driver counts them
/// all. The live unlock (umr, bc250-cu-live-manager) then has nothing to do, and touching it
/// underneath the driver is risky.
pub fn cu_unlock_by_kernel() -> bool {
    crate::util::read_trim(crate::util::sys(
        "module/amdgpu/parameters/bc250_cc_write_mode",
    ))
    .is_some_and(|v| v == "3")
}

/// Where the EFI that unlocks the cores before boot (Hexxeh/bc250-efi-core-unlock) usually
/// lives.
const EFI_SHIM_PATHS: &[&str] = &[
    "/boot/EFI/BOOT/COREUNLOCK.EFI",
    "/boot/efi/EFI/BOOT/COREUNLOCK.EFI",
    "/efi/EFI/BOOT/COREUNLOCK.EFI",
    "/boot/EFI/BOOT/bc250-unlock.efi",
    "/boot/efi/EFI/BOOT/bc250-unlock.efi",
    "/efi/EFI/BOOT/bc250-unlock.efi",
];

/// Is the core-unlock EFI there (file or "CoreUnlock" entry in efibootmgr)?
pub fn efi_core_unlock() -> bool {
    EFI_SHIM_PATHS
        .iter()
        .any(|p| std::path::Path::new(p).is_file())
        || Command::new("efibootmgr")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .is_some_and(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .to_lowercase()
                    .contains("coreunlock")
            })
}

/// Whether the unlocked cores survive unplugging from the wall. Read-only, changes nothing.
pub fn core_unlock_persistence(cores: usize, full: usize, efi: bool) -> Option<&'static str> {
    if cores < full {
        return None;
    }
    Some(if efi {
        "ficam: o EFI de desbloqueio (bc250-efi-core-unlock) liga antes de cada boot"
    } else {
        "se vieram do BIOS (biosmod), ficam; se vieram do bc250-core-unlock, somem ao desligar \
         da tomada (reinício não perde) e é preciso rodar de novo"
    })
}

/// Where the CU count came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CuSource {
    /// Live reading from BC250 Control Center (Decky helper, this boot).
    ControlCenterLive,
    /// Last authorized reading in the BC250 Control Center app, taken this boot.
    ControlCenter,
    /// The kernel unlocked them at boot (bc250_cc_write_mode=3).
    Kernel,
    /// What the driver counted at boot (kernel log or KFD).
    Boot,
}

impl CuSource {
    pub fn text(self) -> &'static str {
        match self {
            CuSource::ControlCenterLive => "ao vivo, pelo BC250 Control Center",
            CuSource::ControlCenter => "última leitura do BC250 Control Center",
            CuSource::Kernel => "liberados pelo kernel no boot",
            CuSource::Boot => "no boot",
        }
    }
}

/// CUs active now, from the best source available. The live unlock (umr) does not change what
/// the driver counted at boot; BC250 Control Center reads the registers (with root) and leaves
/// the reading in files any user can read. Cached for 5 s.
pub fn cu_now() -> Option<(u32, CuSource)> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, Option<(u32, CuSource)>)>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((t, v)) = *c
        && t.elapsed() < Duration::from_secs(5)
    {
        return v;
    }
    let v = read_cu_now();
    *c = Some((Instant::now(), v));
    v
}

fn read_cu_now() -> Option<(u32, CuSource)> {
    let boot_id = crate::util::read_trim("/proc/sys/kernel/random/boot_id").unwrap_or_default();
    if let Some(n) = std::fs::read_to_string("/run/bc250-control-center/cu-live-state.json")
        .ok()
        .and_then(|t| parse_cc_runtime(&t, &boot_id))
    {
        return Some((n, CuSource::ControlCenterLive));
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| home.map(|h| h.join(".local/share")));
    if let Some(f) = data.map(|d| d.join("bc250-control-center/Data/cu_dashboard_live.txt"))
        && let Ok(m) = std::fs::metadata(&f)
        && m.modified()
            .ok()
            .zip(boot_time())
            .is_some_and(|(t, b)| t >= b)
        && let Some(n) = std::fs::read_to_string(&f)
            .ok()
            .and_then(|t| parse_cc_dashboard(&t))
    {
        return Some((n, CuSource::ControlCenter));
    }
    let n = active_cu()?;
    Some((
        n,
        if cu_unlock_by_kernel() {
            CuSource::Kernel
        } else {
            CuSource::Boot
        },
    ))
}

/// Boot time (btime from /proc/stat).
fn boot_time() -> Option<std::time::SystemTime> {
    let t = std::fs::read_to_string("/proc/stat").ok()?;
    let secs: u64 = t
        .lines()
        .find_map(|l| l.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs))
}

/// /run/bc250-control-center/cu-live-state.json (BC250 Control Center's Decky helper):
/// valid only if it is from this boot and the numbers match (2 CUs per WGP, 24 to 40).
pub fn parse_cc_runtime(text: &str, boot_id: &str) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    if boot_id.is_empty()
        || v.get("boot_id")?.as_str()? != boot_id
        || v.get("producer")?.as_str()? != "bc250-quick-access-helper"
        || v.get("cu_total_cus")?.as_u64()? != 40
    {
        return None;
    }
    let n = v.get("cu_active_cus")?.as_u64()?;
    let masks = v.get("cu_masks")?.as_array()?;
    let from_masks: u64 = masks
        .iter()
        .map(|m| m.as_u64().map(|m| u64::from(m.count_ones()) * 2))
        .sum::<Option<u64>>()?;
    (n == from_masks && (24..=40).contains(&n) && n % 2 == 0).then_some(n as u32)
}

/// "CUs active & routed : 40 / 40" in the bc250-cu-live-manager panel saved by
/// BC250 Control Center. Only valid if it appears once.
pub fn parse_cc_dashboard(text: &str) -> Option<u32> {
    let found: Vec<u32> = text
        .lines()
        .filter_map(|l| {
            let low = l.to_lowercase();
            let i = low.find("cus active")?;
            let rest = &low[i + "cus active".len()..];
            let rest = rest.trim_start().trim_start_matches('&').trim_start();
            let rest = rest
                .strip_prefix("routed")?
                .trim_start()
                .strip_prefix(':')?;
            let (n, total) = rest.split_once('/')?;
            (total.trim().starts_with("40")).then(|| n.trim().parse().ok())?
        })
        .collect();
    match found[..] {
        [n] if n <= 40 && n % 2 == 0 => Some(n),
        _ => None,
    }
}

/// Current CPU frequency (MHz): the highest among the cores (/proc/cpuinfo).
pub fn cpu_mhz_now() -> Option<u64> {
    crate::cpuctl::cur_mhz_all().values().copied().max()
}

/// GPU CUs active at boot, from the amdgpu message ("active_cu_number N"). Read once per
/// run; an unlock done after boot (live, through umr) does not show up here.
pub fn active_cu() -> Option<u32> {
    static CU: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *CU.get_or_init(read_active_cu)
}

fn read_active_cu() -> Option<u32> {
    Command::new("journalctl")
        .args([
            "-k",
            "-b",
            "0",
            "-o",
            "cat",
            "--no-pager",
            "--grep",
            "active_cu_number",
        ])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .filter(|s| s.contains("active_cu_number"))
        .or_else(|| {
            Command::new("dmesg")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        })
        .and_then(|out| parse_active_cu(&out))
        .or_else(kfd_cu)
}

/// CUs from the KFD topology (simd_count / simd_per_cu), readable by any user.
/// Useful when the boot message has already left the kernel buffer.
fn kfd_cu() -> Option<u32> {
    let base = crate::util::sys("class/kfd/kfd/topology/nodes");
    std::fs::read_dir(base)
        .ok()?
        .flatten()
        .filter_map(|e| parse_kfd_cu(&std::fs::read_to_string(e.path().join("properties")).ok()?))
        .max()
}

pub fn parse_kfd_cu(props: &str) -> Option<u32> {
    let get = |k: &str| {
        props.lines().find_map(|l| {
            let (n, v) = l.split_once(' ')?;
            (n == k).then(|| v.trim().parse::<u32>().ok())?
        })
    };
    let (simd, per) = (get("simd_count")?, get("simd_per_cu")?);
    (simd > 0 && per > 0).then(|| simd / per)
}

pub fn parse_active_cu(log: &str) -> Option<u32> {
    log.lines()
        .filter_map(|l| l.split("active_cu_number").nth(1))
        .filter_map(|r| {
            r.trim_start_matches([':', ' ', '='])
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        })
        .next_back()
}

#[cfg(test)]
mod unlock_tests {
    #[test]
    fn cu_log() {
        let l = "amdgpu 0000:01:00.0: amdgpu: SE 1, SH per SE 2, CU per SH 12, active_cu_number 24\nfoo\n";
        assert_eq!(super::parse_active_cu(l), Some(24));
        assert_eq!(super::parse_active_cu("nada"), None);
        let kfd = "cpu_cores_count 0\nsimd_count 48\nsimd_per_cu 2\n";
        assert_eq!(super::parse_kfd_cu(kfd), Some(24));
        assert_eq!(super::parse_kfd_cu("simd_count 0\nsimd_per_cu 0\n"), None);
    }

    #[test]
    fn control_center_cus() {
        let j = r#"{"schema":13,"producer":"bc250-quick-access-helper","boot_id":"abc","cu_active_cus":40,"cu_total_cus":40,"cu_masks":[31,31,31,31]}"#;
        assert_eq!(super::parse_cc_runtime(j, "abc"), Some(40));
        assert_eq!(super::parse_cc_runtime(j, "outro-boot"), None);
        let bad = j.replace("\"cu_active_cus\":40", "\"cu_active_cus\":38");
        assert_eq!(super::parse_cc_runtime(&bad, "abc"), None);
        let d = "Mode : live\n  CUs active & routed : 40 / 40\n  Service : enabled\n";
        assert_eq!(super::parse_cc_dashboard(d), Some(40));
        assert_eq!(
            super::parse_cc_dashboard("CUs active & routed: 24/40"),
            Some(24)
        );
        assert_eq!(super::parse_cc_dashboard("nada"), None);
        assert_eq!(
            super::parse_cc_dashboard("CUs active & routed: 24/40\nCUs active & routed: 40/40"),
            None
        );
    }

    #[test]
    fn smu_table() {
        use super::{MetricsDecode::*, metrics_decode};
        assert_eq!(metrics_decode(Some("Y"), true), Legacy);
        assert_eq!(metrics_decode(Some("N"), true), EightCore);
        assert_eq!(metrics_decode(None, true), EightCore);
        assert_eq!(metrics_decode(None, false), Stock);
        assert!(super::mismatch_fix(Stock).contains("desligue"));
        assert!(super::mismatch_fix(EightCore).contains("ligue"));
    }

    #[test]
    fn core_persistence() {
        assert_eq!(super::core_unlock_persistence(6, 8, false), None);
        assert!(
            super::core_unlock_persistence(8, 8, true)
                .unwrap()
                .contains("EFI")
        );
        assert!(
            super::core_unlock_persistence(8, 8, false)
                .unwrap()
                .contains("biosmod")
        );
    }
}

/// State of one first-setup step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum StepState {
    /// Pronto.
    Ok,
    /// Still to do (affects operation).
    Todo,
    /// Optional: it works without it, but worth knowing.
    Opcional,
}

/// What the interface can do for a step.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum StepAction {
    /// Enable an already installed service (asks for the password).
    EnableService(String),
    /// Install an AUR package in a terminal.
    InstallAur(String),
    /// Install a community program (by its name in the recipe).
    InstallApp(String),
    /// Open the GPU memory section (further down in the same tab).
    GpuMemory,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SetupStep {
    pub title: String,
    pub state: StepState,
    pub detail: String,
    pub action: Option<StepAction>,
}

/// First setup of a special board (BC-250): what is already right and what is missing,
/// in order. Read-only; the interface acts (or the user, through the commands shown).
pub fn first_setup(b: &Board, pci: &str) -> Vec<SetupStep> {
    let mut v = vec![];
    let mut add = |title: &str, state: StepState, detail: String, action: Option<StepAction>| {
        v.push(SetupStep {
            title: title.to_string(),
            state,
            detail,
            action,
        })
    };
    // 1. kernel and Mesa
    let k = crate::recipes::running_kernel();
    if !b.bad_kernels.is_empty() {
        if crate::recipes::kernel_in(&k, &b.bad_kernels) {
            add(
                "Kernel",
                StepState::Todo,
                format!(
                    "{k} é de uma faixa que quebra a GPU desta placa: atualize o sistema (sudo pacman -Syu) e reinicie"
                ),
                None,
            );
        } else {
            add("Kernel", StepState::Ok, k.clone(), None);
        }
    }
    if let Some(min) = &b.min_mesa {
        match crate::recipes::mesa_version() {
            Some(m) if crate::recipes::version_ge(&m, min) => add("Mesa", StepState::Ok, m, None),
            Some(m) => add(
                "Mesa",
                StepState::Todo,
                format!("{m}: precisa de {min} ou mais novo (sudo pacman -Syu)"),
                None,
            ),
            None => add(
                "Mesa",
                StepState::Todo,
                "não encontrei o Mesa instalado".into(),
                None,
            ),
        }
    }
    // 2. kernel parameters
    let cmd = crate::cmdline::running();
    for p in &b.kernel_params {
        if cmd.split_whitespace().any(|t| t == p) {
            add(
                &format!("Parâmetro {p}"),
                StepState::Ok,
                "ligado no boot".into(),
                None,
            );
        } else {
            add(
                &format!("Parâmetro {p}"),
                StepState::Opcional,
                "recomendado pela comunidade; ligue em Máquina → Parâmetros do kernel (pede a senha e vale depois de reiniciar)".into(),
                None,
            );
        }
    }
    // 3. clock governor
    if !b.governor_services.is_empty() {
        let sv = services(b);
        if let Some(s) = sv.iter().find(|s| s.active) {
            add(
                "Governor de clock da GPU",
                StepState::Ok,
                format!(
                    "{} rodando{}",
                    s.name,
                    if s.enabled {
                        ""
                    } else {
                        " (mas não liga sozinho no boot)"
                    }
                ),
                None,
            );
        } else if let Some(s) = sv.iter().find(|s| s.installed) {
            add(
                "Governor de clock da GPU",
                StepState::Todo,
                format!(
                    "{} instalado, mas parado: sem ele a GPU fica com clock fixo",
                    s.name
                ),
                Some(StepAction::EnableService(s.name.clone())),
            );
        } else {
            add(
                "Governor de clock da GPU",
                StepState::Todo,
                "não instalado: sem ele a GPU fica com clock fixo (baixo desempenho ou consumo alto parado)".into(),
                b.governor_aur.first().cloned().map(StepAction::InstallAur),
            );
        }
    }
    // 4. telemetria
    match smu_table_mismatch(pci) {
        None => add(
            "Sensores (temperatura e potência)",
            StepState::Ok,
            "a tabela da SMU bate com o número de núcleos".into(),
            None,
        ),
        Some(why) => add(
            "Sensores (temperatura e potência)",
            StepState::Todo,
            why,
            None,
        ),
    }
    // 5. cores and CUs
    if let Some((stock, full)) = b.cores {
        let n = active_cores();
        if n >= full {
            let keep = core_unlock_persistence(n, full, efi_core_unlock())
                .map(|s| format!(" ({s})"))
                .unwrap_or_default();
            add(
                "Núcleos do processador",
                StepState::Ok,
                format!("{n} de {full}{keep}"),
                None,
            );
        } else {
            add(
                "Núcleos do processador",
                StepState::Opcional,
                format!(
                    "{n} ativos (de fábrica: {stock}); dá para liberar {full} com os programas da comunidade abaixo"
                ),
                None,
            );
        }
    }
    if let Some((stock, full)) = b.cus {
        match cu_now() {
            Some((n, src)) if n >= full => add(
                "Unidades de computação da GPU (CUs)",
                StepState::Ok,
                format!("{n} de {full} ({})", src.text()),
                None,
            ),
            Some((n, src)) => add(
                "Unidades de computação da GPU (CUs)",
                StepState::Opcional,
                format!(
                    "{n} ativas ({}; de fábrica: {stock}); dá para liberar {full} com os programas da comunidade",
                    src.text()
                ),
                None,
            ),
            None => add(
                "Unidades de computação da GPU (CUs)",
                StepState::Opcional,
                "não consegui ler quantas estão ativas".into(),
                None,
            ),
        }
    }
    // 6. memory for the GPU
    if b.cus.is_some() {
        let gib = |p: u64| p as f64 * 4096.0 / 1073741824.0;
        match crate::bootcfg::saved_ttm().or_else(crate::bootcfg::ttm_now) {
            Some(p) if crate::bootcfg::saved_ttm().is_some() => add(
                "Memória para a GPU (IA e jogos pesados)",
                StepState::Ok,
                format!("limite de {:.0} GB configurado", gib(p)),
                None,
            ),
            _ => add(
                "Memória para a GPU (IA e jogos pesados)",
                StepState::Opcional,
                "no padrão do kernel; para modelos de IA maiores, aumente em \"Memória da GPU integrada\"".into(),
                Some(StepAction::GpuMemory),
            ),
        }
    }
    // 7. recommended community programs
    for a in b.apps.iter().filter(|a| a.recommended) {
        if a.installed() {
            add(&a.name, StepState::Ok, "instalado".into(), None);
        } else {
            add(
                &a.name,
                StepState::Opcional,
                a.about.clone(),
                a.installable()
                    .then(|| StepAction::InstallApp(a.name.clone())),
            );
        }
    }
    v
}

/// "3 of 5 ready, 1 to do"
pub fn setup_summary(steps: &[SetupStep]) -> String {
    let n = |s: StepState| steps.iter().filter(|x| x.state == s).count();
    let todo = n(StepState::Todo);
    format!(
        "{} de {} prontos{}{}",
        n(StepState::Ok),
        steps.len(),
        if todo > 0 {
            format!(", {todo} por fazer")
        } else {
            String::new()
        },
        if n(StepState::Opcional) > 0 {
            format!(", {} opcionais", n(StepState::Opcional))
        } else {
            String::new()
        }
    )
}
