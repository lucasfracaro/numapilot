//! Notebook com duas placas (integrada + NVIDIA): "NVIDIA só quando precisar".
//!
//! No login, se nenhuma tela estiver ligada numa saída da NVIDIA (no Nitro 5: o HDMI), o desktop
//! sobe só na integrada: o KWin usa apenas ela (KWIN_DRM_DEVICES) e o Xwayland e os programas
//! comuns usam o EGL do Mesa (__EGL_VENDOR_LIBRARY_FILENAMES). Assim nada abre a NVIDIA à toa e ela
//! dorme. Jogos e programas escolhidos para a NVIDIA continuam indo para ela (o numapilot passa o
//! EGL/Vulkan da NVIDIA para eles). Com uma tela na saída da NVIDIA no login, nada muda: ela fica
//! ligada para gerar a imagem.
//!
//! Funciona por dois arquivos lidos a cada login: o do Plasma (~/.config/plasma-workspace/env) e
//! um gerador de ambiente do systemd do usuário (outros desktops).

use crate::topo::{Gpu, Topology};
use crate::util::{read_trim, sys};
use std::path::PathBuf;

pub const MESA_EGL: &str = "/usr/share/glvnd/egl_vendor.d/50_mesa.json";
pub const NVIDIA_EGL: &str = "/usr/share/glvnd/egl_vendor.d/10_nvidia.json";

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

/// Como root: manda cada saída da placa se conferir de novo ("detect"). A placa acorda por um
/// instante e o estado fica certo mesmo quando o notebook não avisa que um cabo foi ligado.
pub fn probe_root(pci: &str) -> usize {
    let Some(c) = card(pci) else { return 0 };
    let Ok(rd) = std::fs::read_dir(sys("class/drm")) else {
        return 0;
    };
    let mut n = 0;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with(&format!("{c}-"))
            && !name.contains("Writeback")
            && std::fs::write(e.path().join("status"), "detect").is_ok()
        {
            n += 1;
        }
    }
    n
}

/// Confere as saídas da placa de verdade (pelo serviço, que é root; sem ele, pelo que o driver
/// já sabe) e diz se há tela ligada.
pub fn probe_connected(pci: &str) -> bool {
    if crate::util::is_root() {
        probe_root(pci);
    } else {
        let _ = crate::ctl::send(&crate::ctl::Request::ProbeOutputs {
            pci: pci.to_string(),
        });
    }
    outputs(pci).iter().any(|o| o.connected)
}

/// Quando restringir: "sempre" ou só se o login for na "bateria" (na tomada, desktop nas duas
/// placas e o HDMI funciona a qualquer hora).
pub fn policy_file() -> PathBuf {
    config_home().join("numapilot/hybrid-policy")
}

pub fn policy() -> String {
    read_trim(policy_file())
        .filter(|p| p == "bateria")
        .unwrap_or_else(|| "sempre".into())
}

pub fn set_policy(p: &str) -> Result<(), String> {
    if p != "sempre" && p != "bateria" {
        return Err("use sempre ou bateria".into());
    }
    let f = policy_file();
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&f, p).map_err(|e| e.to_string())
}

/// Notebook com integrada + NVIDIA dedicada: (integrada, NVIDIA).
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

/// Ambiente do login no modo "NVIDIA só quando precisar" (vazio = não restringir).
pub fn session_env(topo: &Topology) -> Vec<(String, String)> {
    let Some((igpu, nv)) = pair(topo) else {
        return vec![];
    };
    if policy() == "bateria" && crate::notebook::on_ac() != Some(false) {
        return vec![];
    }
    if probe_connected(&nv.pci) {
        return vec![];
    }
    let Some(c) = card(&igpu.pci) else {
        return vec![];
    };
    let mut v = vec![("KWIN_DRM_DEVICES".to_string(), format!("/dev/dri/{c}"))];
    if std::path::Path::new(MESA_EGL).exists() {
        v.push(("__EGL_VENDOR_LIBRARY_FILENAMES".into(), MESA_EGL.into()));
    }
    v.push(("NUMAPILOT_IGPU_SESSION".into(), "1".into()));
    v
}

/// Esta sessão subiu no modo restrito?
pub fn session_restricted() -> bool {
    std::env::var_os("NUMAPILOT_IGPU_SESSION").is_some()
}

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn plasma_env_file() -> PathBuf {
    config_home().join("plasma-workspace/env/numapilot-igpu.sh")
}

pub fn generator_file() -> PathBuf {
    config_home().join("systemd/user-environment-generators/90-numapilot-igpu")
}

pub fn enabled() -> bool {
    plasma_env_file().exists() || generator_file().exists()
}

/// Liga/desliga o modo (arquivos do usuário; vale a partir do próximo login).
pub fn set_enabled(on: bool, bin: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let files = [plasma_env_file(), generator_file()];
    if !on {
        for f in files {
            if f.exists() {
                std::fs::remove_file(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            }
        }
        return Ok(());
    }
    let bin = bin.replace('\'', "");
    let plasma = format!(
        "# numapilot: NVIDIA só quando precisar (desktop na integrada se nada estiver ligado na NVIDIA)\n\
         if [ -x '{bin}' ]; then\n    eval \"$('{bin}' hybrid env --export)\"\nfi\n"
    );
    let generator = format!(
        "#!/bin/sh\n# numapilot: NVIDIA só quando precisar\n[ -x '{bin}' ] && exec '{bin}' hybrid env\nexit 0\n"
    );
    for (f, body, mode) in [(&files[0], plasma, 0o644), (&files[1], generator, 0o755)] {
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(f, body).map_err(|e| format!("{}: {e}", f.display()))?;
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(mode))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// EGL para um programa que vai na NVIDIA numa sessão restrita.
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
    fn outputs_and_env() {
        let root = std::env::temp_dir().join(format!("np-hy-{}", std::process::id()));
        let w = |p: &str, v: &str| {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, v).unwrap();
        };
        w("bus/pci/devices/0000:01:00.0/drm/card1/dev", "x");
        w("class/drm/card1-HDMI-A-1/status", "disconnected");
        w("class/drm/card1-eDP-1/status", "disconnected");
        w("class/drm/card2-DP-9/status", "connected");
        w("class/drm/card2-DP-9/enabled", "enabled");
        let _g = crate::util::test_sysfs_lock(&root);
        let o = super::outputs("0000:01:00.0");
        assert_eq!(o.len(), 2);
        assert!(o.iter().all(|x| !x.connected));
        assert_eq!(super::card("0000:01:00.0").as_deref(), Some("card1"));
        let _ = std::fs::remove_dir_all(root);
    }
}
