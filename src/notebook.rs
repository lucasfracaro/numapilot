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
    // A bateria descarregando é a resposta mais confiável: um hub USB-C pode aparecer como
    // fonte "ligada" (power delivery) sem dar conta de carregar o notebook.
    let discharging = supplies().into_iter().any(|p| {
        read_trim(p.join("type")).as_deref() == Some("Battery")
            && read_trim(p.join("scope")).as_deref() != Some("Device")
            && read_trim(p.join("status")).as_deref() == Some("Discharging")
    });
    if discharging {
        return Some(false);
    }
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
        return Err(crate::i18n::tr("este notebook não permite limitar a carga pelo Linux").into());
    }
    if !(20..=100).contains(&pct) {
        return Err(crate::i18n::tr("limite de carga entre 20% e 100%").into());
    }
    for b in with {
        let p = b.dir.join("charge_control_end_threshold");
        fs::write(&p, pct.to_string()).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    Ok(())
}

// ---------------- perfil da plataforma (ACPI) ----------------

/// Quem fornece o perfil de energia (kernel 6.14+): /sys/class/platform-profile/*/
pub fn profile_handlers() -> Vec<(String, std::path::PathBuf)> {
    let mut v: Vec<(String, std::path::PathBuf)> = fs::read_dir(sys("class/platform-profile"))
        .map(|rd| {
            rd.flatten()
                .map(|e| {
                    let d = e.path();
                    (read_trim(d.join("name")).unwrap_or_default(), d)
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

pub fn platform_profile() -> Option<String> {
    read_trim(sys("firmware/acpi/platform_profile")).or_else(|| {
        profile_handlers()
            .first()
            .and_then(|(_, d)| read_trim(d.join("profile")))
    })
}

pub fn platform_profiles() -> Vec<String> {
    read_trim(sys("firmware/acpi/platform_profile_choices"))
        .or_else(|| {
            profile_handlers()
                .first()
                .and_then(|(_, d)| read_trim(d.join("choices")))
        })
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_default()
}

/// O power-profiles-daemon (o seletor de energia do KDE/GNOME) está cuidando do perfil?
pub fn ppd_active() -> bool {
    std::env::var_os("NUMAPILOT_SYSFS").is_none()
        && crate::containers::which("powerprofilesctl")
        && std::process::Command::new("systemctl")
            .args(["is-active", "--quiet", "power-profiles-daemon"])
            .status()
            .is_ok_and(|s| s.success())
}

/// Perfil do power-profiles-daemon equivalente ao perfil da plataforma.
pub fn ppd_profile_for(p: &str) -> &'static str {
    match p {
        "performance" | "balanced-performance" => "performance",
        "quiet" | "low-power" | "cool" | "balanced-power" => "power-saver",
        _ => "balanced",
    }
}

pub fn set_platform_profile(p: &str) -> Result<(), String> {
    if !platform_profiles().iter().any(|c| c == p) {
        return Err(crate::tf!(
            "perfil {p} não existe neste notebook",
            "profile {p} does not exist on this laptop"
        ));
    }
    // Com o power-profiles-daemon ligado, troca por ele: o seletor de energia do desktop fica
    // igual e ele não desfaz a troca. Depois acerta o perfil exato (ex.: quiet em vez de low-power).
    if ppd_active() {
        let _ = std::process::Command::new("powerprofilesctl")
            .args(["set", ppd_profile_for(p)])
            .status();
    }
    if platform_profile().as_deref() == Some(p) {
        return Ok(());
    }
    let legacy = sys("firmware/acpi/platform_profile");
    if std::path::Path::new(&legacy).exists() {
        return fs::write(legacy, p).map_err(|e| e.to_string());
    }
    // só a interface nova: escreve em cada fornecedor que aceita esse perfil
    let mut last = Err(crate::i18n::tr("nenhum fornecedor de perfil de energia").to_string());
    for (_, d) in profile_handlers() {
        if read_trim(d.join("choices")).is_some_and(|c| c.split_whitespace().any(|x| x == p)) {
            last = fs::write(d.join("profile"), p).map_err(|e| e.to_string());
        }
    }
    last
}

/// Por que não há controles da Acer / perfil de energia: módulos, bloqueios e DKMS.
pub fn acer_diagnose() -> Vec<String> {
    let mut v = vec![];
    let loaded = |m: &str| std::path::Path::new(&format!("/sys/module/{m}")).exists();
    let acer = loaded("acer_wmi");
    let linuwu = loaded("linuwu_sense");
    v.push(crate::tf!(
        "módulos: acer_wmi {} · linuwu_sense {} · amd_pmf {}",
        "modules: acer_wmi {} · linuwu_sense {} · amd_pmf {}",
        if acer {
            crate::i18n::tr("carregado")
        } else {
            crate::i18n::tr("não carregado")
        },
        if linuwu {
            crate::i18n::tr("carregado")
        } else {
            crate::i18n::tr("não carregado")
        },
        if loaded("amd_pmf") {
            crate::i18n::tr("carregado")
        } else {
            crate::i18n::tr("não carregado")
        }
    ));
    if let Some(p) = read_trim("/sys/module/acer_wmi/parameters/predator_v4") {
        v.push(format!("acer_wmi predator_v4 = {p}"));
    }
    let h = profile_handlers();
    v.push(if h.is_empty() {
        crate::i18n::tr(
            "nenhum fornecedor de perfil de energia (/sys/class/platform-profile vazio)",
        )
        .into()
    } else {
        crate::tf!(
            "perfil de energia por: {}",
            "power profile from: {}",
            h.iter()
                .map(|(n, _)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    });
    // bloqueios em /etc/modprobe.d
    if let Ok(rd) = fs::read_dir("/etc/modprobe.d") {
        for e in rd.flatten() {
            let t = fs::read_to_string(e.path()).unwrap_or_default();
            for l in t.lines().map(str::trim) {
                if !l.starts_with('#') && l.contains("acer_wmi") {
                    v.push(format!("{}: {l}", e.path().display()));
                }
            }
        }
    }
    // Linuwu-Sense instalado pelo DKMS para este kernel?
    let kernel = crate::recipes::running_kernel();
    if let Ok(o) = std::process::Command::new("dkms").arg("status").output() {
        let t = String::from_utf8_lossy(&o.stdout);
        let lines: Vec<&str> = t
            .lines()
            .filter(|l| l.to_lowercase().contains("linuwu"))
            .collect();
        if lines.is_empty() {
            v.push(crate::i18n::tr("Linuwu-Sense não aparece no dkms status").into());
        } else {
            for l in &lines {
                v.push(format!("dkms: {l}"));
            }
            if !lines
                .iter()
                .any(|l| l.contains(&kernel) && l.contains("installed"))
            {
                v.push(crate::tf!(
                    "o Linuwu-Sense não está compilado para o kernel em uso ({kernel}): \
                     sudo dkms autoinstall e reinicie (ou veja se o projeto já suporta este kernel)", "Linuwu-Sense is not built for the running kernel ({kernel}): \
                     sudo dkms autoinstall and reboot (or check whether the project already supports this kernel)"
                ));
            }
        }
    }
    if !acer && !linuwu {
        v.push(
            crate::i18n::tr("sem acer_wmi nem linuwu_sense carregados: o notebook fica sem perfil de energia e sem \
             controles da Acer")
                .into(),
        );
    }
    v
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
/// A placa está dormindo? (`dgpu_state` responde no idioma em uso)
pub fn dgpu_asleep(pci: &str) -> bool {
    let s = dgpu_state(pci);
    s.starts_with("dormindo") || s.starts_with("asleep")
}

pub fn dgpu_state(pci: &str) -> String {
    let rs =
        read_trim(sys(&format!("bus/pci/devices/{pci}/power/runtime_status"))).unwrap_or_default();
    let ps = read_trim(sys(&format!("bus/pci/devices/{pci}/power_state"))).unwrap_or_default();
    match rs.as_str() {
        "suspended" => crate::tf!(
            "dormindo{}",
            "asleep{}",
            if ps.is_empty() {
                String::new()
            } else {
                format!(" ({ps})")
            }
        ),
        "active" => crate::i18n::tr("ligada").into(),
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
        Err(crate::i18n::tr("systemctl falhou").into())
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

/// Quem abriu a placa e como: `client` = usa a placa de verdade (/dev/nvidiaN ou o nó de
/// render: CUDA, NVENC, NVML, OpenGL/Vulkan), o que a impede de dormir; senão só abriu a saída
/// de vídeo (cardN), como o KWin e o logind fazem com todas as placas.
#[derive(Debug, Clone, PartialEq)]
pub struct Holder {
    pub pid: u32,
    pub name: String,
    pub client: bool,
}

/// Processos do sistema que só guardam uma cópia do acesso (não usam a placa).
pub const SYSTEM_HOLDERS: &[&str] = &["systemd", "systemd-logind"];

/// O desktop (compositor) abre todas as placas para poder ligar telas nelas. Parado, isso não
/// impede a NVIDIA de dormir no modo "fine-grained" do driver; não conta como uso.
pub const COMPOSITORS: &[&str] = &[
    "kwin_wayland",
    "kwin_x11",
    "gnome-shell",
    "Hyprland",
    "sway",
    "Xorg",
    "mutter",
];

pub fn dgpu_holders(pci: &str) -> Vec<Holder> {
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
        let mut any = false;
        let mut client = false;
        for f in fds.flatten() {
            let Ok(t) = fs::read_link(f.path()) else {
                continue;
            };
            let t = t.to_string_lossy().to_string();
            if nodes.contains(&t) {
                any = true;
                if !t.starts_with("/dev/dri/card") {
                    client = true;
                    break;
                }
            }
        }
        if any {
            let name = read_trim(e.path().join("comm")).unwrap_or_default();
            out.push(Holder { pid, name, client });
        }
    }
    out
}

/// Programas que usam a placa de verdade e não deixam ela dormir (sem os do sistema).
pub fn dgpu_users(pci: &str) -> Vec<(u32, String)> {
    dgpu_holders(pci)
        .into_iter()
        .filter(|h| {
            h.client
                && !SYSTEM_HOLDERS.contains(&h.name.as_str())
                && !COMPOSITORS.contains(&h.name.as_str())
        })
        .map(|h| (h.pid, h.name))
        .collect()
}

/// Dica para os programas que costumam prender a NVIDIA do notebook.
pub fn holder_hint(name: &str) -> Option<&'static str> {
    match name {
        "lact" | "lactd" => Some(crate::i18n::tr(
            "LACT: o serviço dele lê a NVIDIA o tempo todo (NVML). Se o numapilot já faz o que você usa nele: sudo systemctl disable --now lactd",
        )),
        "sunshine" => Some(crate::i18n::tr(
            "Sunshine: codifica na NVIDIA (NVENC). Escolha VA-API na aba Streaming (usa a 680M) ou tire o Sunshine da inicialização",
        )),
        "Xwayland" => Some(crate::i18n::tr(
            "Xwayland: sudo numapilot hybrid xwayland on e entre de novo na sessão",
        )),
        "nvidia-powerd" | "nvidia-persistenced" => Some(crate::i18n::tr(
            "serviço da NVIDIA: o numapilot para e liga junto com a placa",
        )),
        _ => None,
    }
}

/// Ventoinhas do notebook (controlador do fabricante: acer, asus, dell_smm, thinkpad...): RPM.
pub fn ec_fans() -> Vec<u64> {
    let Ok(rd) = fs::read_dir(sys("class/hwmon")) else {
        return vec![];
    };
    let mut v = vec![];
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        let dev = fs::canonicalize(d.join("device"))
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        if !dev.contains("/platform/") {
            continue;
        }
        for i in 1..=4 {
            if let Some(r) = read_u64(d.join(format!("fan{i}_input"))) {
                v.push(r);
            }
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usb_c_dock_on_battery() {
        let root = std::env::temp_dir().join(format!("np-ac-{}", std::process::id()));
        let w = |p: &str, v: &str| {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, v).unwrap();
        };
        w("class/power_supply/ACAD/type", "Mains");
        w("class/power_supply/ACAD/online", "0");
        w("class/power_supply/ucsi-source-psy-USBC000:001/type", "USB");
        w("class/power_supply/ucsi-source-psy-USBC000:001/online", "1");
        w("class/power_supply/BAT1/type", "Battery");
        w("class/power_supply/BAT1/status", "Discharging");
        let _g = crate::util::test_sysfs_lock(&root);
        assert_eq!(on_ac(), Some(false));
        std::fs::write(root.join("class/power_supply/BAT1/status"), "Charging").unwrap();
        assert_eq!(on_ac(), Some(true));
        let _ = std::fs::remove_dir_all(root);
    }

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
