//! Laptop with two cards (integrated + NVIDIA): NVIDIA modes switched with the session open.
//!
//! - **Always available**: the NVIDIA stays in the system; it sleeps by itself when nobody uses
//!   it and HDMI (wired to it) works at any time.
//! - **Off**: the service removes the NVIDIA from the system (as if it did not exist: zero
//!   power, nothing wakes it); KDE notices and carries on with the integrated GPU. No HDMI.
//! - **Automatic**: on AC power, always available; on battery, off.
//!
//! To bring it back, the service asks for a rescan of the PCI bus: the card returns, the driver
//! loads and KDE adds it by itself (without leaving the session). It only turns off if no
//! program is using the NVIDIA (otherwise it waits and shows who is using it).
//!
//! Xwayland (X11 programs) tends to open the NVIDIA even without using it; the wrapper in
//! /usr/local/bin/Xwayland makes it use only Mesa's EGL.

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

/// The card's cardN.
pub fn card(pci: &str) -> Option<String> {
    std::fs::read_dir(sys(&format!("bus/pci/devices/{pci}/drm")))
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .find(|n| n.starts_with("card"))
}

/// The card's video outputs. Reads the state kept by the driver (does not wake the card).
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

/// Laptop with integrated + dedicated NVIDIA present: (integrated, NVIDIA).
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

/// What the service knows about the NVIDIA (in /run/numapilot/dgpu.json).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub pci: String,
    pub alias: String,
    pub name: String,
    /// The card's PCI functions (GPU and audio).
    pub functions: Vec<String>,
    /// Removed from the system by numapilot.
    pub removed: bool,
    /// Requested mode and whether, by it, the card should be in the system now.
    pub mode: String,
    pub want_present: bool,
    /// Programs that prevent turning it off.
    pub blocked_by: Vec<String>,
    /// Only the video output is open (KWin): we do not remove the card (not safe with the desktop
    /// holding it), but it sleeps by itself.
    pub sleep_only: bool,
    pub last_error: Option<String>,
    /// PCI bridge above the card: bringing it back rescans only that, without touching the rest of
    /// the bus (Thunderbolt/USB4, NVMe...).
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

/// By the mode and the power source, should the NVIDIA be in the system?
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

/// Removes the card's functions from the system (root). Audio before the GPU.
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

/// PCI bridge above a device (e.g. 0000:00:01.1), if there is one.
pub fn parent_bridge(pci: &str) -> Option<String> {
    let real = std::fs::canonicalize(sys(&format!("bus/pci/devices/{pci}"))).ok()?;
    let parent = real.parent()?.file_name()?.to_string_lossy().to_string();
    // "0000:00:01.1" and not "pci0000:00"
    (parent.len() == 12 && parent.as_bytes()[4] == b':' && parent.as_bytes()[7] == b':')
        .then_some(parent)
}

/// Rescans only the card's bridge (or the whole bus, if it is not known): the card returns
/// and the driver loads (root).
pub fn rescan_at(bridge: &str) -> Result<(), String> {
    let b = sys(&format!("bus/pci/devices/{bridge}/rescan"));
    if !bridge.is_empty() && b.exists() {
        return std::fs::write(&b, "1").map_err(|e| format!("rescan {bridge}: {e}"));
    }
    rescan()
}

/// Rescans the whole bus (root).
pub fn rescan() -> Result<(), String> {
    std::fs::write(sys("bus/pci/rescan"), "1").map_err(|e| format!("rescan: {e}"))
}

/// Is the system shutting down or rebooting?
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

/// Installs/removes the Xwayland wrapper (root).
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

// ---------------- old version (choice at login) ----------------

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Files from the previous version (it decided only at login and locked KDE to the integrated
/// GPU): delete them.
pub fn remove_login_files() {
    for f in [
        config_home().join("plasma-workspace/env/numapilot-igpu.sh"),
        config_home().join("systemd/user-environment-generators/90-numapilot-igpu"),
        config_home().join("numapilot/hybrid-policy"),
    ] {
        let _ = std::fs::remove_file(f);
    }
}

/// EGL for a program that goes on the NVIDIA (overrides an inherited restriction).
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
