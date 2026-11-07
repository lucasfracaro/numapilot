//! Notebook: tomada ou bateria, limite de carga, perfil de energia da plataforma (ACPI) e
//! se a placa de vídeo dedicada está dormindo.

use crate::util::{read_trim, read_u64, sys};
use std::fs;
use std::path::PathBuf;

fn supplies() -> Vec<PathBuf> {
    fs::read_dir(sys("class/power_supply"))
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default()
}

/// Baterias do próprio computador (não do mouse/fone: esses têm scope=Device).
fn batteries() -> Vec<PathBuf> {
    supplies()
        .into_iter()
        .filter(|p| {
            read_trim(p.join("type")).as_deref() == Some("Battery")
                && read_trim(p.join("scope")).as_deref() != Some("Device")
        })
        .collect()
}

pub fn is_laptop() -> bool {
    if !batteries().is_empty() {
        return true;
    }
    // Tipo de gabinete do DMI: 8 portátil, 9 laptop, 10 notebook, 14 sub-notebook, 31 conversível, 32 destacável
    read_trim(sys("class/dmi/id/chassis_type"))
        .and_then(|s| s.parse::<u32>().ok())
        .is_some_and(|t| [8, 9, 10, 14, 31, 32].contains(&t))
}

/// Ligado na tomada? (None = não dá para saber)
pub fn on_ac() -> Option<bool> {
    let mains: Vec<PathBuf> = supplies()
        .into_iter()
        .filter(|p| {
            matches!(
                read_trim(p.join("type")).as_deref(),
                Some("Mains") | Some("USB")
            ) && read_trim(p.join("scope")).as_deref() != Some("Device")
        })
        .collect();
    if mains.is_empty() {
        return None;
    }
    Some(
        mains
            .iter()
            .any(|p| read_trim(p.join("online")).as_deref() == Some("1")),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    pub name: String,
    pub dir: PathBuf,
    pub pct: Option<u64>,
    /// Charging | Discharging | Full | Not charging
    pub status: String,
    pub power_w: Option<f64>,
    /// Saúde: capacidade atual / de fábrica (%).
    pub health_pct: Option<f64>,
    /// Para de carregar em (%), se o notebook permitir.
    pub charge_limit: Option<u8>,
}

pub fn battery_info() -> Vec<Battery> {
    batteries()
        .into_iter()
        .map(|d| {
            let n = |f: &str| read_u64(d.join(f));
            let full = n("energy_full").or_else(|| n("charge_full"));
            let design = n("energy_full_design").or_else(|| n("charge_full_design"));
            let power = n("power_now").map(|v| v as f64 / 1e6).or_else(|| {
                // corrente (µA) × tensão (µV)
                Some(n("current_now")? as f64 * n("voltage_now")? as f64 / 1e12)
            });
            Battery {
                name: d
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default(),
                pct: n("capacity"),
                status: read_trim(d.join("status")).unwrap_or_default(),
                power_w: power.filter(|w| *w > 0.0),
                health_pct: match (full, design) {
                    (Some(f), Some(dz)) if dz > 0 => {
                        Some((f as f64 * 100.0 / dz as f64).min(100.0))
                    }
                    _ => None,
                },
                charge_limit: n("charge_control_end_threshold").map(|v| v as u8),
                dir: d,
            }
        })
        .collect()
}

pub fn set_charge_limit(pct: u8) -> Result<(), String> {
    let bats = battery_info();
    let with: Vec<&Battery> = bats.iter().filter(|b| b.charge_limit.is_some()).collect();
    if with.is_empty() {
        return Err("este notebook não permite limitar a carga pelo Linux".into());
    }
    if !(20..=100).contains(&pct) {
        return Err("limite de carga entre 20% e 100%".into());
    }
    for b in with {
        let p = b.dir.join("charge_control_end_threshold");
        fs::write(&p, pct.to_string()).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}

// ---------------- perfil da plataforma (ACPI) ----------------

pub fn platform_profile() -> Option<String> {
    read_trim(sys("firmware/acpi/platform_profile"))
}

pub fn platform_profiles() -> Vec<String> {
    read_trim(sys("firmware/acpi/platform_profile_choices"))
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

pub fn set_platform_profile(p: &str) -> Result<(), String> {
    if !platform_profiles().iter().any(|c| c == p) {
        return Err(format!("perfil {p} não existe neste notebook"));
    }
    fs::write(sys("firmware/acpi/platform_profile"), p).map_err(|e| e.to_string())
}

/// Perfil da plataforma que combina com um modo da máquina (o mais próximo disponível).
pub fn profile_for_mode(mode: &str) -> Option<String> {
    let ch = platform_profiles();
    let want: &[&str] = match mode {
        "jogo" | "ia" => &["performance", "balanced-performance"],
        "silencioso" => &["quiet", "low-power", "cool", "balanced-power"],
        _ => return None,
    };
    want.iter()
        .find(|w| ch.iter().any(|c| c == *w))
        .map(|s| s.to_string())
}

// ---------------- placa dedicada ----------------

/// Gerência de energia do PCI da placa: "auto" deixa dormir, "on" mantém ligada.
pub fn runtime_pm(pci: &str) -> Option<String> {
    read_trim(sys(&format!("bus/pci/devices/{pci}/power/control")))
}

pub fn set_runtime_pm(pci: &str, auto: bool) -> Result<(), String> {
    fs::write(
        sys(&format!("bus/pci/devices/{pci}/power/control")),
        if auto { "auto" } else { "on" },
    )
    .map_err(|e| e.to_string())
}

/// "dormindo (D3cold)", "ligada"...
pub fn dgpu_state(pci: &str) -> String {
    let rs =
        read_trim(sys(&format!("bus/pci/devices/{pci}/power/runtime_status"))).unwrap_or_default();
    let ps = read_trim(sys(&format!("bus/pci/devices/{pci}/power_state"))).unwrap_or_default();
    match rs.as_str() {
        "suspended" => format!(
            "dormindo{}",
            if ps.is_empty() {
                String::new()
            } else {
                format!(" ({ps})")
            }
        ),
        "active" => "ligada".into(),
        "" => "?".into(),
        o => o.into(),
    }
}

/// Serviço do Dynamic Boost da NVIDIA (passa potência da CPU para a GPU em notebooks).
pub fn has_nvidia_powerd() -> bool {
    [
        "/usr/lib/systemd/system/nvidia-powerd.service",
        "/etc/systemd/system/nvidia-powerd.service",
    ]
    .iter()
    .any(|p| std::path::Path::new(p).exists())
}

pub fn nvidia_powerd_enabled() -> bool {
    std::process::Command::new("systemctl")
        .args(["is-enabled", "nvidia-powerd"])
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn set_nvidia_powerd(on: bool) -> Result<(), String> {
    let st = std::process::Command::new("systemctl")
        .args([
            if on { "enable" } else { "disable" },
            "--now",
            "nvidia-powerd",
        ])
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err("systemctl falhou".into())
    }
}

/// Arquivos de dispositivo da placa: /dev/dri/cardN e renderDN, e /dev/nvidiaN na NVIDIA.
fn dev_nodes(pci: &str) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(sys(&format!("bus/pci/devices/{pci}/drm")))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with("card") || n.starts_with("renderD"))
                .map(|n| format!("/dev/dri/{n}"))
                .collect()
        })
        .unwrap_or_default();
    let info = fs::read_to_string(format!("/proc/driver/nvidia/gpus/{pci}/information"))
        .unwrap_or_default();
    if let Some(m) = info
        .lines()
        .find(|l| l.starts_with("Device Minor"))
        .and_then(|l| l.split(':').nth(1))
    {
        v.push(format!("/dev/nvidia{}", m.trim()));
    }
    v
}

/// Programas com a placa aberta (o que a impede de dormir). Sem root, só aparecem os do
/// próprio usuário, que é o caso comum (navegador, Steam, jogo).
pub fn dgpu_users(pci: &str) -> Vec<(u32, String)> {
    let nodes = dev_nodes(pci);
    if nodes.is_empty() {
        return vec![];
    }
    let mut out = vec![];
    let Ok(rd) = fs::read_dir("/proc") else {
        return out;
    };
    for e in rd.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(fds) = fs::read_dir(e.path().join("fd")) else {
            continue;
        };
        let uses = fds.flatten().any(|f| {
            fs::read_link(f.path())
                .map(|t| nodes.iter().any(|n| t.as_os_str() == n.as_str()))
                .unwrap_or(false)
        });
        if uses {
            let name = read_trim(e.path().join("comm")).unwrap_or_default();
            out.push((pid, name));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_laptop() {
        let root = std::env::temp_dir().join("numapilot-notebook-test");
        let _ = fs::remove_dir_all(&root);
        let w = |p: &str, v: &str| {
            let f = root.join(p);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, v).unwrap();
        };
        w("class/power_supply/BAT0/type", "Battery");
        w("class/power_supply/BAT0/capacity", "83");
        w("class/power_supply/BAT0/status", "Discharging");
        w("class/power_supply/BAT0/energy_full", "45000000");
        w("class/power_supply/BAT0/energy_full_design", "50000000");
        w("class/power_supply/BAT0/power_now", "12500000");
        w(
            "class/power_supply/BAT0/charge_control_end_threshold",
            "100",
        );
        w("class/power_supply/ADP1/type", "Mains");
        w("class/power_supply/ADP1/online", "0");
        w("class/power_supply/hidpp_battery_0/type", "Battery");
        w("class/power_supply/hidpp_battery_0/scope", "Device");
        w("firmware/acpi/platform_profile", "balanced");
        w(
            "firmware/acpi/platform_profile_choices",
            "low-power balanced performance",
        );
        let _g = crate::util::test_sysfs_lock(&root);
        assert!(is_laptop());
        assert_eq!(on_ac(), Some(false));
        let b = battery_info();
        assert_eq!(b.len(), 1, "a bateria do mouse não conta");
        assert_eq!(b[0].pct, Some(83));
        assert_eq!(b[0].health_pct, Some(90.0));
        assert_eq!(b[0].power_w, Some(12.5));
        set_charge_limit(80).unwrap();
        assert_eq!(battery_info()[0].charge_limit, Some(80));
        assert_eq!(profile_for_mode("silencioso").as_deref(), Some("low-power"));
        assert_eq!(profile_for_mode("jogo").as_deref(), Some("performance"));
        set_platform_profile("performance").unwrap();
        assert_eq!(platform_profile().as_deref(), Some("performance"));
        assert!(set_platform_profile("turbo").is_err());
    }
}
