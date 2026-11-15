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
    /// The card is in the system and asleep right now (runtime suspended).
    pub asleep: bool,
    /// Who has the card open while it is awake, as the service sees it: as root it also sees
    /// KWin (its /proc/PID/fd is hidden from the user because of cap_sys_nice) and system services.
    pub awake_by: Vec<String>,
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

// ---------------- desktop on the integrated card (KDE, chosen at login) ----------------
//
// KWin keeps a context with VRAM on the laptop's NVIDIA, so it never sleeps (Nitro 5: 26 h without
// suspending), and taking the card off the bus with the desktop holding it hangs the driver. The
// only way out is KWin starting without it: KWIN_DRM_DEVICES. A Plasma env script decides at
// each login: with a screen connected to an NVIDIA output (HDMI, or the laptop's own screen with
// the MUX on the NVIDIA) the desktop uses both cards as usual; otherwise only the integrated one.

/// Plasma script that limits KWin to the integrated card at login.
pub fn desktop_env_file() -> PathBuf {
    config_home().join("plasma-workspace/env/numapilot-kwin-igpu.sh")
}

/// The choice is on (the script exists).
pub fn desktop_integrated() -> bool {
    desktop_env_file().exists()
}

/// Integrated card and laptop NVIDIA PCI addresses, if this is such a laptop.
pub fn desktop_pair(topo: &Topology) -> Option<(String, String)> {
    let i = topo.gpus.iter().find(|g| g.is_integrated())?;
    let n = topo
        .gpus
        .iter()
        .find(|g| g.is_nvidia() && !g.is_integrated())?;
    Some((i.pci.clone(), n.pci.clone()))
}

/// The script. Cards are found by PCI address at login (cardN changes between boots); a
/// variable left in the systemd user manager by an earlier session (linger) is cleared when the
/// desktop should use both cards.
pub fn desktop_script(igpu_pci: &str, dgpu_pci: &str) -> String {
    format!(
        "# numapilot: the desktop (KWin) uses only the integrated card so the laptop's NVIDIA can\n\
         # sleep, unless a screen is connected to an NVIDIA output at login (HDMI...).\n\
         # Undo: numapilot hybrid desktop all (or Machine -> Laptop in numapilot)\n\
         _np_i=$(readlink -f /dev/dri/by-path/pci-{igpu_pci}-card 2>/dev/null)\n\
         _np_d=$(readlink -f /dev/dri/by-path/pci-{dgpu_pci}-card 2>/dev/null)\n\
         _np_busy=\n\
         if [ -n \"$_np_d\" ]; then\n\
         \x20   for _np_s in /sys/class/drm/\"${{_np_d##*/}}\"-*/status; do\n\
         \x20       [ \"$(cat \"$_np_s\" 2>/dev/null)\" = connected ] && _np_busy=1\n\
         \x20   done\n\
         fi\n\
         if [ -z \"$_np_busy\" ] && [ -n \"$_np_i\" ] && [ -e \"$_np_i\" ]; then\n\
         \x20   export KWIN_DRM_DEVICES=\"$_np_i\"\n\
         \x20   # without this KWin's EGL still loads the NVIDIA driver and holds the card\n\
         \x20   [ -f {MESA_EGL} ] && export __EGL_VENDOR_LIBRARY_FILENAMES={MESA_EGL}\n\
         else\n\
         \x20   unset KWIN_DRM_DEVICES __EGL_VENDOR_LIBRARY_FILENAMES\n\
         \x20   systemctl --user unset-environment KWIN_DRM_DEVICES __EGL_VENDOR_LIBRARY_FILENAMES 2>/dev/null\n\
         fi\n\
         unset _np_i _np_d _np_busy _np_s\n"
    )
}

/// Turns the choice on or off (user files, no password). Applies from the next login.
pub fn set_desktop_integrated(topo: &Topology, on: bool) -> Result<(), String> {
    let p = desktop_env_file();
    if !on {
        let _ = std::fs::remove_file(&p);
        // with linger the user manager outlives the session and would keep the variable
        let _ = std::process::Command::new("systemctl")
            .args([
                "--user",
                "unset-environment",
                "KWIN_DRM_DEVICES",
                "__EGL_VENDOR_LIBRARY_FILENAMES",
            ])
            .status();
        return Ok(());
    }
    if nix_is_root() {
        return Err(crate::i18n::tr("run it as your user, without sudo").into());
    }
    let (i, n) = desktop_pair(topo).ok_or_else(|| {
        crate::i18n::tr("only for a laptop with an integrated card and an NVIDIA").to_string()
    })?;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    std::fs::write(&p, desktop_script(&i, &n)).map_err(|e| format!("{}: {e}", p.display()))
}

fn nix_is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// KWIN_DRM_DEVICES set somewhere else (it would win or conflict): file and line.
pub fn kwin_drm_elsewhere() -> Option<String> {
    let mut files = vec![PathBuf::from("/etc/environment")];
    for d in [
        config_home().join("environment.d"),
        PathBuf::from("/etc/environment.d"),
        config_home().join("plasma-workspace/env"),
    ] {
        if let Ok(rd) = std::fs::read_dir(d) {
            files.extend(rd.flatten().map(|e| e.path()));
        }
    }
    files
        .into_iter()
        .filter(|f| *f != desktop_env_file())
        .find_map(|f| {
            let t = std::fs::read_to_string(&f).ok()?;
            t.lines()
                .map(str::trim)
                .find(|l| !l.starts_with('#') && l.contains("KWIN_DRM_DEVICES"))
                .map(|l| format!("{}: {l}", f.display()))
        })
}

/// Whether the desktop can let the card go: a compositor is running (between sessions a new one
/// may open the card while it is being removed, and the removal then hangs until a reboot) and,
/// if it is KWin, it was started without the NVIDIA (KWIN_DRM_DEVICES without its card and EGL
/// from Mesa only). Reads the compositor's environment, so only the service (root) can tell.
pub fn desktop_lets_go(nvidia_pci: &str) -> bool {
    let card = card(nvidia_pci).map(|c| format!("/dev/dri/{c}"));
    let mut any = false;
    let Ok(rd) = std::fs::read_dir("/proc") else {
        return false;
    };
    for e in rd.flatten() {
        let Some(comm) = read_trim(e.path().join("comm")) else {
            continue;
        };
        if !crate::notebook::COMPOSITORS.contains(&comm.as_str()) {
            continue;
        }
        any = true;
        if comm.starts_with("kwin") {
            let env = std::fs::read(e.path().join("environ")).unwrap_or_default();
            if !kwin_env_lets_go(&String::from_utf8_lossy(&env), card.as_deref()) {
                return false;
            }
        }
    }
    any
}

/// KWin's environment (NUL separated) keeps it off the NVIDIA card.
pub fn kwin_env_lets_go(environ: &str, nvidia_card: Option<&str>) -> bool {
    let get = |k: &str| {
        environ
            .split('\0')
            .find_map(|kv| kv.strip_prefix(k).and_then(|v| v.strip_prefix('=')))
    };
    let drm_ok = get("KWIN_DRM_DEVICES").is_some_and(|v| {
        !v.is_empty() && nvidia_card.is_none_or(|c| !v.split(':').any(|d| d == c))
    });
    let egl_ok = get("__EGL_VENDOR_LIBRARY_FILENAMES").is_some_and(|v| !v.contains("nvidia"));
    drm_ok && egl_ok
}

/// Asks KDE to show its log out dialog (the user confirms there).
pub fn prompt_logout() -> Result<(), String> {
    let ok = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--type=method_call",
            "--dest=org.kde.LogoutPrompt",
            "/LogoutPrompt",
            "org.kde.LogoutPrompt.promptLogout",
        ])
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        Ok(())
    } else {
        Err(crate::i18n::tr("could not open the log out dialog: log out from the menu").into())
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

#[cfg(test)]
mod desktop_tests {
    use super::*;

    #[test]
    fn login_script() {
        let s = desktop_script("0000:74:00.0", "0000:01:00.0");
        assert!(s.contains("/dev/dri/by-path/pci-0000:74:00.0-card"));
        assert!(s.contains("/dev/dri/by-path/pci-0000:01:00.0-card"));
        assert!(s.contains("export KWIN_DRM_DEVICES"));
        assert!(s.contains("systemctl --user unset-environment KWIN_DRM_DEVICES"));
        // KWin's EGL must not load the NVIDIA driver either (it held the card on the Nitro 5)
        assert!(s.contains(&format!("export __EGL_VENDOR_LIBRARY_FILENAMES={MESA_EGL}")));
        // Valid shell, and sourced with no card found (no PATH: readlink/systemctl missing, so
        // nothing on this machine is touched) it leaves KWIN_DRM_DEVICES unset and no helpers.
        let dir = std::env::temp_dir().join(format!("np-desk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("s.sh");
        std::fs::write(&f, &s).unwrap();
        let ok = std::process::Command::new("sh")
            .arg("-n")
            .arg(&f)
            .status()
            .unwrap();
        assert!(ok.success());
        let out = std::process::Command::new("/bin/sh")
            .env_clear()
            .env("PATH", "/nonexistent")
            .env("KWIN_DRM_DEVICES", "/dev/dri/card9")
            .arg("-c")
            .arg(format!(
                ". '{}'; echo \"k=${{KWIN_DRM_DEVICES-unset}} i=${{_np_i-unset}} b=${{_np_busy-unset}}\"",
                f.display()
            ))
            .output()
            .unwrap();
        let o = String::from_utf8_lossy(&out.stdout);
        assert_eq!(o.trim(), "k=unset i=unset b=unset", "{o}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod lets_go_tests {
    use super::kwin_env_lets_go;

    #[test]
    fn kwin_environment() {
        let env = |v: &[&str]| v.join("\0");
        let ok = env(&[
            "HOME=/home/u",
            "KWIN_DRM_DEVICES=/dev/dri/card2",
            "__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json",
        ]);
        assert!(kwin_env_lets_go(&ok, Some("/dev/dri/card1")));
        // the session of the Nitro 5 that hung: KWIN_DRM_DEVICES only, EGL still loads NVIDIA
        let no_egl = env(&["KWIN_DRM_DEVICES=/dev/dri/card2"]);
        assert!(!kwin_env_lets_go(&no_egl, Some("/dev/dri/card1")));
        // a normal session
        assert!(!kwin_env_lets_go(
            &env(&["HOME=/home/u"]),
            Some("/dev/dri/card1")
        ));
        // the NVIDIA card in the list
        let both = env(&[
            "KWIN_DRM_DEVICES=/dev/dri/card2:/dev/dri/card1",
            "__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json",
        ]);
        assert!(!kwin_env_lets_go(&both, Some("/dev/dri/card1")));
        let nv_egl = env(&[
            "KWIN_DRM_DEVICES=/dev/dri/card2",
            "__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/10_nvidia.json",
        ]);
        assert!(!kwin_env_lets_go(&nv_egl, Some("/dev/dri/card1")));
    }
}
