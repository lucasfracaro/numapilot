//! Official NVIDIA driver on CachyOS: picks the right package for the card's generation,
//! installs it (through chwd, CachyOS's tool, or through pacman) and updates it.
//!
//! Turing (GTX 16 / RTX 20) onwards: NVIDIA open modules (current series).
//! Maxwell, Pascal and Volta: 580xx series (NVIDIA dropped these cards from the new drivers).
//! Updating = updating the whole system (on Arch a single package is not updated in isolation).

use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum Branch {
    /// Open modules, current series.
    Open,
    /// 580xx series (proprietary) for Maxwell/Pascal/Volta.
    Legacy580,
}

impl Branch {
    pub fn label(&self) -> &'static str {
        match self {
            Branch::Open => crate::i18n::tr("current (NVIDIA open modules)"),
            Branch::Legacy580 => crate::i18n::tr("580xx (last series for Maxwell/Pascal/Volta)"),
        }
    }
}

/// By PCI ID: Turing starts at 0x1e00 (TU102); before that, Volta/Pascal/Maxwell.
pub fn branch_for(device: u16) -> Branch {
    if device >= 0x1e00 {
        Branch::Open
    } else {
        Branch::Legacy580
    }
}

fn pacman_q(pkg: &str) -> Option<String> {
    let o = Command::new("pacman").args(["-Q", pkg]).output().ok()?;
    o.status.success().then(|| {
        String::from_utf8_lossy(&o.stdout)
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .to_string()
    })
}

fn pacman_si_version(pkg: &str) -> Option<String> {
    let o = Command::new("pacman").args(["-Si", pkg]).output().ok()?;
    if !o.status.success() {
        return None;
    }
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .find(|l| l.starts_with("Version") || l.starts_with("Versão"))
        .and_then(|l| l.split(':').nth(1).map(|s| s.trim().to_string()))
}

/// Installed kernel packages (linux-cachyos, linux-cachyos-lts, linux-cachyos-bore...).
pub fn kernels() -> Vec<String> {
    let Ok(o) = Command::new("pacman").args(["-Qq"]).output() else {
        return vec![];
    };
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter(|p| {
            (p.starts_with("linux-cachyos")
                || *p == "linux"
                || *p == "linux-lts"
                || *p == "linux-zen")
                && !p.ends_with("-headers")
                && !p.contains("-nvidia")
                && !p.contains("-zfs")
                && !p.contains("-dbg")
        })
        .map(String::from)
        .collect()
}

/// List of packages to install the driver.
pub fn packages(
    branch: &Branch,
    kernels: &[String],
    laptop: bool,
    available: &dyn Fn(&str) -> bool,
) -> Vec<String> {
    let mut v: Vec<String> = vec![];
    match branch {
        Branch::Open => {
            // Prebuilt module for each CachyOS kernel; otherwise DKMS with the headers.
            let mut dkms = false;
            for k in kernels {
                let m = format!("{k}-nvidia-open");
                if available(&m) {
                    v.push(m);
                } else {
                    dkms = true;
                    v.push(format!("{k}-headers"));
                }
            }
            if dkms || kernels.is_empty() {
                v.push("nvidia-open-dkms".into());
            }
            for p in [
                "nvidia-utils",
                "lib32-nvidia-utils",
                "nvidia-settings",
                "opencl-nvidia",
                "lib32-opencl-nvidia",
                "libva-nvidia-driver",
            ] {
                v.push(p.into());
            }
        }
        Branch::Legacy580 => {
            for k in kernels {
                v.push(format!("{k}-headers"));
            }
            for p in [
                "nvidia-580xx-dkms",
                "nvidia-580xx-utils",
                "lib32-nvidia-580xx-utils",
                "nvidia-580xx-settings",
                "opencl-nvidia-580xx",
            ] {
                v.push(p.into());
            }
        }
    }
    if laptop {
        v.push("nvidia-prime".into());
    }
    v.retain(|p| available(p));
    v
}

#[derive(Debug, Clone)]
pub struct Status {
    pub gpus: Vec<(String, String, u16)>,
    pub branch: Option<Branch>,
    pub loaded: Option<String>,
    pub installed: Option<String>,
    pub available: Option<String>,
    pub nouveau: bool,
    pub chwd: bool,
    pub needs_reboot: bool,
}

pub fn status(topo: &crate::topo::Topology) -> Status {
    let gpus: Vec<(String, String, u16)> = topo
        .gpus
        .iter()
        .filter(|g| g.is_nvidia())
        .map(|g| (g.pci.clone(), g.name.clone(), g.device))
        .collect();
    // The oldest card decides (there is a single driver for all of them).
    let branch = gpus
        .iter()
        .map(|g| branch_for(g.2))
        .min_by_key(|b| if *b == Branch::Legacy580 { 0 } else { 1 });
    let utils = ["nvidia-utils", "nvidia-580xx-utils"]
        .iter()
        .find_map(|p| pacman_q(p).map(|v| (p.to_string(), v)));
    let available = match &branch {
        Some(Branch::Legacy580) => pacman_si_version("nvidia-580xx-utils"),
        Some(Branch::Open) => pacman_si_version("nvidia-utils"),
        None => None,
    };
    let loaded = crate::nvctl::driver_version();
    let installed = utils.map(|u| u.1);
    let needs_reboot = match (&loaded, &installed) {
        (Some(l), Some(i)) => !i.starts_with(l.as_str()),
        _ => false,
    };
    Status {
        gpus,
        branch,
        loaded,
        installed,
        available,
        nouveau: std::path::Path::new("/sys/module/nouveau").exists(),
        chwd: crate::containers::which("chwd"),
        needs_reboot,
    }
}

fn run(cmd: &[&str], log: &mut dyn FnMut(&str)) -> Result<(), String> {
    log(&format!("$ {}", cmd.join(" ")));
    let st = Command::new(cmd[0])
        .args(&cmd[1..])
        .status()
        .map_err(|e| format!("{}: {e}", cmd[0]))?;
    if st.success() {
        Ok(())
    } else {
        Err(crate::tf!(
            "{} finished with an error ({st})",
            "{} terminou com erro ({st})",
            cmd[0]
        ))
    }
}

fn repo_has(p: &str) -> bool {
    Command::new("pacman")
        .args(["-Si", p])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Installs the right driver. Needs root.
pub fn install(topo: &crate::topo::Topology, log: &mut dyn FnMut(&str)) -> Result<(), String> {
    if !crate::util::is_root() {
        return Err(
            crate::i18n::tr("needs root (the interface button asks for the password)").into(),
        );
    }
    let st = status(topo);
    let branch = st.branch.ok_or(crate::i18n::tr("no NVIDIA card found"))?;
    log(&crate::tf!(
        "Card(s): {}",
        "Placa(s): {}",
        st.gpus
            .iter()
            .map(|g| g.1.clone())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    log(&crate::tf!(
        "Recommended driver: {}",
        "Driver indicado: {}",
        branch.label()
    ));
    // System up to date first (on Arch, installing against an outdated base breaks things).
    run(&["pacman", "-Syu", "--noconfirm"], log)?;
    if st.chwd && branch == Branch::Open {
        // CachyOS's own tool picks and configures the profile for the hardware.
        log(crate::i18n::tr("Using chwd (CachyOS hardware detection)."));
        if run(&["chwd", "-a", "-f"], log).is_ok() {
            log(crate::i18n::tr(
                "Done. Reboot the computer to load the new driver.",
            ));
            return Ok(());
        }
        log(crate::i18n::tr(
            "chwd failed; installing the packages directly.",
        ));
    }
    let pk = packages(&branch, &kernels(), crate::notebook::is_laptop(), &repo_has);
    if pk.is_empty() {
        return Err(
            crate::i18n::tr("could not find the driver packages in the repositories").into(),
        );
    }
    let mut cmd: Vec<&str> = vec!["pacman", "-S", "--needed", "--noconfirm"];
    cmd.extend(pk.iter().map(String::as_str));
    // Switching series (current ⇄ 580xx) causes a conflict; pacman asks and --noconfirm does not
    // answer "yes" to removals, so we remove the other series first.
    let other: &[&str] = match branch {
        Branch::Open => &[
            "nvidia-580xx-dkms",
            "nvidia-580xx-utils",
            "lib32-nvidia-580xx-utils",
            "nvidia-580xx-settings",
            "opencl-nvidia-580xx",
        ],
        Branch::Legacy580 => &[
            "nvidia-open-dkms",
            "nvidia-utils",
            "lib32-nvidia-utils",
            "nvidia-settings",
            "opencl-nvidia",
            "lib32-opencl-nvidia",
        ],
    };
    let mut rm: Vec<String> = other
        .iter()
        .filter(|p| pacman_q(p).is_some())
        .map(|p| p.to_string())
        .collect();
    if branch == Branch::Legacy580 {
        for k in kernels() {
            let m = format!("{k}-nvidia-open");
            if pacman_q(&m).is_some() {
                rm.push(m);
            }
        }
    }
    if !rm.is_empty() {
        let mut c: Vec<&str> = vec!["pacman", "-Rdd", "--noconfirm"];
        c.extend(rm.iter().map(String::as_str));
        run(&c, log)?;
    }
    run(&cmd, log)?;
    log(crate::i18n::tr(
        "Done. Reboot the computer to load the new driver.",
    ));
    Ok(())
}

/// Updates the driver along with the system. Needs root.
pub fn update(log: &mut dyn FnMut(&str)) -> Result<(), String> {
    if !crate::util::is_root() {
        return Err(
            crate::i18n::tr("needs root (the interface button asks for the password)").into(),
        );
    }
    log(crate::i18n::tr(
        "On Arch/CachyOS the driver is updated along with the system (updating only it would break the dependencies).",
    ));
    run(&["pacman", "-Syu", "--noconfirm"], log)?;
    log(crate::i18n::tr(
        "Done. If the driver version changed, reboot.",
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branches_and_packages() {
        assert_eq!(branch_for(0x2860), Branch::Open, "RTX 4060 notebook");
        assert_eq!(branch_for(0x1f95), Branch::Open, "GTX 1650 Ti (Turing)");
        assert_eq!(
            branch_for(0x1c8d),
            Branch::Legacy580,
            "GTX 1050 notebook (Pascal)"
        );
        let ks = vec!["linux-cachyos".to_string(), "linux-cachyos-lts".to_string()];
        let avail = |p: &str| p != "linux-cachyos-lts-nvidia-open";
        let p = packages(&Branch::Open, &ks, true, &avail);
        assert!(p.contains(&"linux-cachyos-nvidia-open".to_string()));
        assert!(
            p.contains(&"linux-cachyos-lts-headers".to_string())
                && p.contains(&"nvidia-open-dkms".to_string()),
            "{p:?}"
        );
        assert!(p.contains(&"nvidia-prime".to_string()));
        let p = packages(&Branch::Legacy580, &ks, false, &|_| true);
        assert!(
            p.contains(&"nvidia-580xx-dkms".to_string())
                && !p.iter().any(|x| x.contains("nvidia-open"))
        );
    }
}
