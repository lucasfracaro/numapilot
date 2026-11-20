//! lsfg-vk (Lossless Scaling frame generation on Linux): what is installed and how to install the
//! rest. lsfg-vk is a Vulkan layer that needs the DLL of Lossless Scaling, bought and installed
//! through Steam (app 993090, branch "lsfg-vk"). numapilot never ships or downloads lsfg-vk
//! itself (its license since 2.0 is CC BY-NC-ND 4.0): it detects the pieces and hands the install
//! to the AUR helper in a terminal, or points to the official builds.
//!
//! Upstream facts (lsfg-vk.dev, 2.0): AUR packages `lsfg-vk`, `lsfg-vk-bin` (pre-built) and
//! `lsfg-vk-git`, not maintained by the author; manual install = a tarball from
//! builds.lsfg-vk.dev extracted into ~/.local; configurator `lsfg-vk-ui` (Qt 6) and
//! `lsfg-vk-cli healthcheck`; config `~/.config/lsfg-vk/conf.toml` (`LSFGVK_CONFIG` overrides),
//! created on the first Vulkan start; DLL path in `[global] dll` or `LSFGVK_DLL_PATH`.

use std::path::{Path, PathBuf};

/// Steam app id of Lossless Scaling.
pub const STEAM_APP: &str = "993090";
/// Steam branch that ships the files lsfg-vk reads.
pub const STEAM_BRANCH: &str = "lsfg-vk";
/// AUR packages, the recommended one first.
pub const AUR_PKGS: [&str; 3] = ["lsfg-vk", "lsfg-vk-bin", "lsfg-vk-git"];
pub const DOCS_URL: &str = "https://lsfg-vk.dev/docs/installation/";
pub const BUILDS_URL: &str = "https://builds.lsfg-vk.dev";
/// Installs (or opens the store page of) Lossless Scaling in the Steam client.
pub const STEAM_INSTALL_URL: &str = "steam://install/993090";
pub const STORE_URL: &str = "https://store.steampowered.com/app/993090/Lossless_Scaling/";

/// Shown before any install step: lsfg-vk is useless without Lossless Scaling.
pub const BEFORE: &str = "Before lsfg-vk: buy and install Lossless Scaling through Steam (app 993090) and switch it to the \"lsfg-vk\" branch (Properties → Betas). lsfg-vk uses its DLL and generates nothing without it.";

/// Lossless Scaling in the Steam libraries.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Lossless {
    pub dir: PathBuf,
    /// The DLL lsfg-vk loads (lsfg-vk.dll on the lsfg-vk branch, Lossless.dll before).
    pub dll: Option<PathBuf>,
    /// Steam branch in use (None = the default one).
    pub branch: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Status {
    /// Vulkan layer manifest found (system or user).
    pub layer: Option<PathBuf>,
    /// AUR/pacman package that provides it.
    pub package: Option<String>,
    pub cli: Option<PathBuf>,
    pub ui: Option<PathBuf>,
    pub lossless: Option<Lossless>,
    /// DLL set by hand (config `dll` or LSFGVK_DLL_PATH).
    pub dll_override: Option<PathBuf>,
    pub config: PathBuf,
    pub config_exists: bool,
    /// Arch-based system (pacman), for the AUR install.
    pub arch: bool,
    /// AUR helper found (paru or yay).
    pub helper: Option<String>,
}

impl Status {
    pub fn installed(&self) -> bool {
        self.layer.is_some()
    }
    /// A DLL lsfg-vk can load was found.
    pub fn has_dll(&self) -> bool {
        self.dll_override.as_ref().is_some_and(|p| p.is_file())
            || self.lossless.as_ref().is_some_and(|l| l.dll.is_some())
    }
    /// Lossless Scaling is installed but not on the lsfg-vk branch (and the new DLL is missing).
    pub fn wrong_branch(&self) -> bool {
        self.lossless.as_ref().is_some_and(|l| {
            l.branch.as_deref() != Some(STEAM_BRANCH)
                && !l.dll.as_ref().is_some_and(|d| d.ends_with("lsfg-vk.dll"))
        })
    }
    /// Everything frame generation needs is in place.
    pub fn ready(&self) -> bool {
        self.installed() && self.has_dll()
    }
    /// Command for the terminal that installs lsfg-vk from the AUR (None off Arch or without a helper).
    pub fn install_command(&self) -> Option<Vec<String>> {
        if !self.arch {
            return None;
        }
        let h = self.helper.clone()?;
        Some(vec![h, "-S".into(), "--needed".into(), AUR_PKGS[0].into()])
    }
}

pub(crate) fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

fn which(bin: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    // the manual install goes to ~/.local/bin, which is not always in PATH
    dirs.push(home().join(".local/bin"));
    dirs.into_iter().map(|d| d.join(bin)).find(|p| p.is_file())
}

/// Folders where Vulkan looks for implicit layers.
fn layer_dirs() -> Vec<PathBuf> {
    let mut v = vec![
        PathBuf::from("/usr/share/vulkan/implicit_layer.d"),
        PathBuf::from("/usr/local/share/vulkan/implicit_layer.d"),
        PathBuf::from("/etc/vulkan/implicit_layer.d"),
    ];
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"));
    v.push(data.join("vulkan/implicit_layer.d"));
    v.push(home().join(".local/share/vulkan/implicit_layer.d"));
    v.dedup();
    v
}

/// The lsfg-vk layer manifest, if any.
pub fn layer_manifest() -> Option<PathBuf> {
    layer_dirs().into_iter().find_map(|d| {
        std::fs::read_dir(&d).ok()?.flatten().find_map(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            (n.contains("lsfg") && n.ends_with(".json")).then(|| e.path())
        })
    })
}

/// Config file: LSFGVK_CONFIG, else $XDG_CONFIG_HOME/lsfg-vk/conf.toml.
pub fn config_path() -> PathBuf {
    if let Some(p) = std::env::var_os("LSFGVK_CONFIG") {
        return PathBuf::from(p);
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("lsfg-vk/conf.toml")
}

/// `dll = "…"` of the config (first one, in any section).
pub fn config_dll(text: &str) -> Option<PathBuf> {
    text.lines().find_map(|l| {
        let l = l.trim();
        let rest = l
            .strip_prefix("dll")?
            .trim_start()
            .strip_prefix('=')?
            .trim();
        let v = rest.trim_matches(|c| c == '"' || c == '\'');
        (!v.is_empty()).then(|| PathBuf::from(v))
    })
}

/// Steam branch of an app manifest (`UserConfig`/`MountedConfig` → `BetaKey`; "public" = None).
pub fn branch_of(acf: &str) -> Option<String> {
    let m = crate::steam::parse(acf);
    let st = m.get("AppState")?;
    ["UserConfig", "MountedConfig"]
        .iter()
        .find_map(|k| st.get(k).and_then(|c| c.str("BetaKey")))
        .filter(|b| !b.is_empty() && *b != "public")
        .map(str::to_string)
}

/// The DLL in a Lossless Scaling folder (the lsfg-vk branch name first).
pub fn dll_in(dir: &Path) -> Option<PathBuf> {
    ["lsfg-vk.dll", "Lossless.dll"]
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
}

/// Lossless Scaling in the Steam libraries.
pub fn lossless() -> Option<Lossless> {
    for lib in crate::steam::libraries() {
        let Ok(s) = std::fs::read_to_string(lib.join(format!("appmanifest_{STEAM_APP}.acf")))
        else {
            continue;
        };
        let m = crate::steam::parse(&s);
        let dir = m
            .get("AppState")
            .and_then(|a| a.str("installdir"))
            .unwrap_or("Lossless Scaling");
        let dir = lib.join("common").join(dir);
        return Some(Lossless {
            dll: dll_in(&dir),
            branch: branch_of(&s),
            dir,
        });
    }
    None
}

fn package() -> Option<String> {
    which("pacman")?;
    AUR_PKGS.iter().find_map(|p| {
        std::process::Command::new("pacman")
            .args(["-Qq", p])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .filter(|s| s.success())
            .map(|_| p.to_string())
    })
}

/// Reads everything (pacman and the file system; a few ms).
pub fn status() -> Status {
    let config = config_path();
    let dll_override = std::env::var_os("LSFGVK_DLL_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            std::fs::read_to_string(&config)
                .ok()
                .and_then(|t| config_dll(&t))
        });
    Status {
        layer: layer_manifest(),
        package: package(),
        cli: which("lsfg-vk-cli"),
        ui: which("lsfg-vk-ui"),
        lossless: lossless(),
        dll_override,
        config_exists: config.is_file(),
        config,
        arch: Path::new("/etc/arch-release").exists() || which("pacman").is_some(),
        helper: ["paru", "yay"]
            .iter()
            .find(|h| which(h).is_some())
            .map(|h| h.to_string()),
    }
}

/// One line per piece, for the CLI and the doctor.
pub fn report(s: &Status) -> Vec<(bool, String)> {
    let mut v = vec![];
    v.push(match (&s.layer, &s.package) {
        (Some(l), Some(p)) => (
            true,
            crate::tf!(
                "lsfg-vk layer: {} (package {p})",
                "camada do lsfg-vk: {} (pacote {p})",
                l.display()
            ),
        ),
        (Some(l), None) => (
            true,
            crate::tf!("lsfg-vk layer: {}", "camada do lsfg-vk: {}", l.display()),
        ),
        (None, _) => (
            false,
            crate::i18n::tr("lsfg-vk is not installed").to_string(),
        ),
    });
    v.push(match &s.lossless {
        Some(l) => match &l.dll {
            Some(d) => (
                true,
                crate::tf!(
                    "Lossless Scaling: {} (branch {})",
                    "Lossless Scaling: {} (ramo {})",
                    d.display(),
                    l.branch.as_deref().unwrap_or("public")
                ),
            ),
            None => (
                false,
                crate::tf!(
                    "Lossless Scaling in {} without the DLL (download not finished?)",
                    "Lossless Scaling em {} sem a DLL (download não terminou?)",
                    l.dir.display()
                ),
            ),
        },
        None if s.dll_override.is_some() => (
            true,
            crate::tf!(
                "Lossless Scaling DLL set by hand: {}",
                "DLL do Lossless Scaling indicada à mão: {}",
                s.dll_override.as_ref().unwrap().display()
            ),
        ),
        None => (
            false,
            crate::i18n::tr("Lossless Scaling is not installed through Steam (app 993090)")
                .to_string(),
        ),
    });
    if s.wrong_branch() {
        v.push((
            false,
            crate::i18n::tr(
                "Lossless Scaling is not on the \"lsfg-vk\" branch (Steam → Properties → Betas)",
            )
            .to_string(),
        ));
    }
    v.push((
        true,
        if s.config_exists {
            crate::tf!("config: {}", "configuração: {}", s.config.display())
        } else {
            crate::tf!(
                "config: {} (created on the first Vulkan game)",
                "configuração: {} (criada no primeiro jogo Vulkan)",
                s.config.display()
            )
        },
    ));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_dll_of_the_config() {
        let t = "[global]\nallow_fp16 = true\ndll = \"/games/LS/lsfg-vk.dll\"\n\n[[profile]]\nname = \"x\"\n";
        assert_eq!(config_dll(t), Some(PathBuf::from("/games/LS/lsfg-vk.dll")));
        assert_eq!(config_dll("[global]\ndll_x = 1\n"), None);
        assert_eq!(config_dll("dll = \"\"\n"), None);
    }

    #[test]
    fn reads_the_steam_branch() {
        let acf = r#""AppState"
{
	"appid"		"993090"
	"installdir"		"Lossless Scaling"
	"UserConfig"
	{
		"BetaKey"		"lsfg-vk"
	}
}"#;
        assert_eq!(branch_of(acf).as_deref(), Some("lsfg-vk"));
        let public = acf.replace("lsfg-vk\"\n", "public\"\n");
        assert_eq!(branch_of(&public), None);
        assert_eq!(branch_of("\"AppState\"\n{\n}\n"), None);
    }

    #[test]
    fn install_command_needs_arch_and_a_helper() {
        let mut s = Status {
            arch: true,
            helper: Some("paru".into()),
            ..Default::default()
        };
        assert_eq!(
            s.install_command().unwrap(),
            ["paru", "-S", "--needed", "lsfg-vk"]
        );
        s.helper = None;
        assert!(s.install_command().is_none());
        s.helper = Some("yay".into());
        s.arch = false;
        assert!(s.install_command().is_none());
    }

    #[test]
    fn branch_and_readiness() {
        let mut s = Status {
            layer: Some("/usr/share/vulkan/implicit_layer.d/lsfg.json".into()),
            lossless: Some(Lossless {
                dir: "/x".into(),
                dll: Some("/x/Lossless.dll".into()),
                branch: None,
            }),
            ..Default::default()
        };
        assert!(s.ready());
        assert!(s.wrong_branch());
        s.lossless.as_mut().unwrap().branch = Some("lsfg-vk".into());
        assert!(!s.wrong_branch());
        s.lossless = None;
        assert!(!s.ready());
        assert!(!s.wrong_branch());
    }
}
