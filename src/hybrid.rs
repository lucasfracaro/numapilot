//! Notebook com duas placas (integrada + NVIDIA): modos da NVIDIA trocados com a sessão aberta.
//!
//! - **Sempre disponível**: a NVIDIA fica no sistema; dorme sozinha quando ninguém usa e o HDMI
//!   (ligado nela) funciona a qualquer hora.
//! - **Desligada**: o serviço tira a NVIDIA do sistema (como se ela não existisse: zero consumo,
//!   nada a acorda); o KDE percebe e segue só na integrada. O HDMI não funciona.
//! - **Automático**: na tomada, sempre disponível; na bateria, desligada.
//!
//! Para devolver, o serviço pede uma nova leitura do barramento PCI: a placa volta, o driver
//! carrega e o KDE a adiciona sozinho (sem sair da sessão). Só desliga se nenhum programa estiver
//! usando a NVIDIA (senão espera e mostra quem está usando).
//!
//! O Xwayland (programas X11) costuma abrir a NVIDIA mesmo sem usar; o wrapper em
//! /usr/local/bin/Xwayland faz ele usar só o EGL do Mesa.

use crate::topo::{Gpu, Topology};
use crate::util::{read_trim, sys};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MESA_EGL: &str = "/usr/share/glvnd/egl_vendor.d/50_mesa.json";
pub const NVIDIA_EGL: &str = "/usr/share/glvnd/egl_vendor.d/10_nvidia.json";
pub const MODES: &[(&str, &str)] = &[
    ("auto", "Automatic (AC power: available, battery: off)"),
    ("hibrido", "Always available"),
    ("integrada", "Off (integrated only)"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Output {
    /// "HDMI-A-1", "DP-9", "eDP-2"...
    pub name: String,
    pub connected: bool,
    pub enabled: bool,
}

/// cardN da placa.
pub fn card(pci: &str) -> Option<String> {
    std::fs::read_dir(sys(&format!("bus/pci/devices/{pci}/drm")))
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .find(|n| n.starts_with("card"))
}

/// Saídas de vídeo da placa. Lê o estado guardado pelo driver (não acorda a placa).
pub fn outputs(pci: &str) -> Vec<Output> {
    let Some(c) = card(pci) else { return vec![] };
    let Ok(rd) = std::fs::read_dir(sys("class/drm")) else {
        return vec![];
    };
    let mut v: Vec<Output> = rd
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            let name = n.strip_prefix(&format!("{c}-"))?.to_string();
            if name.starts_with("Writeback") {
                return None;
            }
            Some(Output {
                connected: read_trim(e.path().join("status")).as_deref() == Some("connected"),
                enabled: read_trim(e.path().join("enabled")).as_deref() == Some("enabled"),
                name,
            })
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// Notebook com integrada + NVIDIA dedicada presente: (integrada, NVIDIA).
pub fn pair(topo: &Topology) -> Option<(&Gpu, &Gpu)> {
    if !crate::notebook::is_laptop() {
        return None;
    }
    let igpu = topo.gpus.iter().find(|g| g.is_integrated())?;
    let nv = topo
        .gpus
        .iter()
        .find(|g| g.is_nvidia() && !g.is_integrated())?;
    Some((igpu, nv))
}

/// O que o serviço sabe da NVIDIA (em /run/numapilot/dgpu.json).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub pci: String,
    pub alias: String,
    pub name: String,
    /// Funções PCI da placa (GPU e áudio).
    pub functions: Vec<String>,
    /// Tirada do sistema pelo numapilot.
    pub removed: bool,
    /// Modo pedido e se, por ele, a placa deveria estar no sistema agora.
    pub mode: String,
    pub want_present: bool,
    /// Programas que impedem desligar.
    pub blocked_by: Vec<String>,
    /// Só a saída de vídeo está aberta (KWin): não tiramos a placa (não é seguro com o desktop
    /// segurando), mas ela dorme sozinha.
    pub sleep_only: bool,
    pub last_error: Option<String>,
    /// Ponte PCI acima da placa: a volta lê só ela de novo, sem mexer no resto do barramento
    /// (Thunderbolt/USB4, NVMe...).
    pub bridge: String,
}

pub fn state_path() -> PathBuf {
    crate::learned::run_dir().join("dgpu.json")
}

pub fn load_state() -> Option<State> {
    serde_json::from_str(&std::fs::read_to_string(state_path()).ok()?).ok()
}

pub fn save_state(s: &State) {
    let _ = std::fs::write(state_path(), serde_json::to_string(s).unwrap_or_default());
}

/// Pelo modo e pela fonte de energia, a NVIDIA deve estar no sistema?
pub fn want_present(mode: &str, on_ac: Option<bool>) -> bool {
    match mode {
        "integrada" => false,
        "auto" => on_ac != Some(false),
        _ => true,
    }
}

pub fn present(pci: &str) -> bool {
    sys(&format!("bus/pci/devices/{pci}")).exists()
}

/// Tira as funções da placa do sistema (root). Áudio antes da GPU.
pub fn remove(functions: &[String]) -> Result<(), String> {
    let mut f = functions.to_vec();
    f.sort();
    f.reverse();
    for p in f {
        let path = sys(&format!("bus/pci/devices/{p}/remove"));
        if path.exists() {
            std::fs::write(&path, "1").map_err(|e| format!("{p}: {e}"))?;
        }
    }
    Ok(())
}

/// Ponte PCI acima de um dispositivo (ex.: 0000:00:01.1), se houver.
pub fn parent_bridge(pci: &str) -> Option<String> {
    let real = std::fs::canonicalize(sys(&format!("bus/pci/devices/{pci}"))).ok()?;
    let parent = real.parent()?.file_name()?.to_string_lossy().to_string();
    // "0000:00:01.1" e não "pci0000:00"
    (parent.len() == 12 && parent.as_bytes()[4] == b':' && parent.as_bytes()[7] == b':')
        .then_some(parent)
}

/// Lê de novo só a ponte da placa (ou o barramento todo, se não souber qual): a placa volta
/// e o driver carrega (root).
pub fn rescan_at(bridge: &str) -> Result<(), String> {
    let b = sys(&format!("bus/pci/devices/{bridge}/rescan"));
    if !bridge.is_empty() && b.exists() {
        return std::fs::write(&b, "1").map_err(|e| format!("rescan {bridge}: {e}"));
    }
    rescan()
}

/// Lê o barramento todo de novo (root).
pub fn rescan() -> Result<(), String> {
    std::fs::write(sys("bus/pci/rescan"), "1").map_err(|e| format!("rescan: {e}"))
}

/// O sistema está desligando ou reiniciando?
pub fn system_stopping() -> bool {
    std::process::Command::new("systemctl")
        .arg("is-system-running")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "stopping")
}

// ---------------- Xwayland ----------------

pub fn xwayland_wrapper() -> PathBuf {
    std::env::var_os("NUMAPILOT_XWAYLAND_WRAPPER")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/local/bin/Xwayland"))
}

pub fn xwayland_wrapped() -> bool {
    std::fs::read_to_string(xwayland_wrapper()).is_ok_and(|s| s.contains("numapilot"))
}

/// Instala/remove o wrapper do Xwayland (root).
pub fn set_xwayland_wrapper(on: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let p = xwayland_wrapper();
    if !on {
        if xwayland_wrapped() {
            std::fs::remove_file(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        return Ok(());
    }
    if p.exists() && !xwayland_wrapped() {
        return Err(crate::tf!(
            "{} already exists and is not numapilot's",
            "{} já existe e não é do numapilot",
            p.display()
        ));
    }
    let real = ["/usr/bin/Xwayland", "/usr/lib/xorg/Xwayland"]
        .into_iter()
        .find(|r| std::path::Path::new(r).exists())
        .ok_or(crate::i18n::tr("Xwayland not found in /usr/bin"))?;
    let body = format!(
        "#!/bin/sh\n# numapilot: Xwayland uses only Mesa's EGL and does not hold the laptop's NVIDIA\n\
         # (X11 games set to the NVIDIA still go to it). To remove: numapilot hybrid xwayland off\n\
         [ -f {MESA_EGL} ] && export __EGL_VENDOR_LIBRARY_FILENAMES={MESA_EGL}\nexec {real} \"$@\"\n"
    );
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, body).map_err(|e| format!("{}: {e}", p.display()))?;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())
}

// ---------------- versão antiga (escolha no login) ----------------

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Arquivos da versão anterior (decidia só no login e travava o KDE na integrada): apaga.
pub fn remove_login_files() {
    for f in [
        config_home().join("plasma-workspace/env/numapilot-igpu.sh"),
        config_home().join("systemd/user-environment-generators/90-numapilot-igpu"),
        config_home().join("numapilot/hybrid-policy"),
    ] {
        let _ = std::fs::remove_file(f);
    }
}

/// EGL para um programa que vai na NVIDIA (passa por cima de uma restrição herdada).
pub fn nvidia_egl() -> Option<String> {
    let have: Vec<&str> = [NVIDIA_EGL, MESA_EGL]
        .into_iter()
        .filter(|p| std::path::Path::new(p).exists())
        .collect();
    (!have.is_empty()).then(|| have.join(":"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn modes_and_remove() {
        assert!(!super::want_present("auto", Some(false)));
        assert!(super::want_present("auto", Some(true)));
        assert!(super::want_present("hibrido", Some(false)));
        assert!(!super::want_present("integrada", Some(true)));
        let root = std::env::temp_dir().join(format!("np-hy-{}", std::process::id()));
        let w = |p: &str, v: &str| {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, v).unwrap();
        };
        w("bus/pci/devices/0000:01:00.0/drm/card1/dev", "x");
        w("bus/pci/devices/0000:01:00.0/remove", "");
        w("bus/pci/devices/0000:01:00.1/remove", "");
        w("class/drm/card1-HDMI-A-1/status", "connected");
        w("bus/pci/rescan", "");
        let _g = crate::util::test_sysfs_lock(&root);
        assert!(super::outputs("0000:01:00.0")[0].connected);
        super::remove(&["0000:01:00.0".into(), "0000:01:00.1".into()]).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("bus/pci/devices/0000:01:00.1/remove")).unwrap(),
            "1"
        );
        super::rescan().unwrap();
        let _ = std::fs::remove_dir_all(root);
    }
}
