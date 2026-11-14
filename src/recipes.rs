//! Community recipes (recipes/default.toml built into the program, or a newer copy in
//! /var/lib/numapilot/recipes.toml): special boards, bad kernels, governors, per-processor
//! tips and what each GPU architecture can do (compute and upscaling).

use crate::topo::{Gpu, Topology};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const EMBEDDED: &str = include_str!("../recipes/default.toml");

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    pub id: String,
    pub name: String,
    pub gpu_ids: Vec<String>,
    pub gfx: Vec<String>,
    pub name_contains: Vec<String>,
    pub headless: bool,
    pub bad_kernels: Vec<(String, String)>,
    pub min_mesa: Option<String>,
    pub governor_services: Vec<String>,
    pub governor_configs: Vec<String>,
    pub governor_aur: Vec<String>,
    pub kernel_params: Vec<String>,
    pub cpu_oc_tools: Vec<String>,
    pub cpu_oc_url: Option<String>,
    pub docs: Option<String>,
    pub notes: Vec<String>,
    /// CPU cores from the factory and unlocked (BC-250: 6 and 8).
    pub cores: Option<(usize, usize)>,
    /// GPU CUs from the factory and unlocked (BC-250: 24 and 40).
    pub cus: Option<(u32, u32)>,
    /// Community programs made for this board.
    pub apps: Vec<CommunityApp>,
    /// No working video encoder (software streaming).
    pub no_video_encoder: bool,
}

/// Community program: numapilot shows, installs and updates it (AUR or git) and opens it; it
/// does not reimplement it. Always the maintainer's newest version: AUR `-git` packages are
/// rebuilt from the current code and git repositories are updated with `git pull`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommunityApp {
    pub name: String,
    /// Executables that indicate it is installed (the first that exists is the one opened).
    pub bins: Vec<String>,
    pub aur: Option<String>,
    pub url: Option<String>,
    pub about: String,
    /// The board's recommended one: shown first, highlighted.
    pub recommended: bool,
    /// Repository to install without a package: cloned into
    /// ~/.local/share/numapilot/comunidade/<name> and updated with `git pull`.
    pub git: Option<String>,
    /// Repository files that become commands in ~/.local/bin.
    pub scripts: Vec<String>,
    /// AUR packages it needs (installed first).
    pub aur_deps: Vec<String>,
    /// Command run in the repository folder after downloading or updating (build, install).
    pub setup: Option<String>,
    /// Folder under ~/.local/share of the program's own per-user installer (not a package).
    /// When it exists and the AUR package is not installed, "Update" downloads the newest code
    /// into a temporary folder and runs `local_setup` there, updating that copy in place
    /// instead of installing a second one through the AUR.
    pub local_dir: Option<String>,
    pub local_setup: Option<String>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

/// 'text' for the shell.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl CommunityApp {
    /// Path of the installed executable: in PATH or in ~/.local/bin (the interface started from
    /// the menu does not always have ~/.local/bin in PATH).
    pub fn installed_bin(&self) -> Option<String> {
        self.bins.iter().find_map(|b| {
            if crate::containers::which(b) {
                Some(b.clone())
            } else {
                let p = home().join(".local/bin").join(b);
                p.is_file().then(|| p.to_string_lossy().into_owned())
            }
        })
    }

    /// Folder of the git clone (`~/.local/share/numapilot/comunidade/<name>`).
    pub fn git_dir(&self) -> PathBuf {
        let slug: String = self
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        home().join(".local/share/numapilot/comunidade").join(slug)
    }

    /// Installed by numapilot through git, or through the AUR package. Cached for 10 s (the
    /// interface asks every frame and pacman -Q is slow).
    pub fn installed(&self) -> bool {
        use std::collections::HashMap;
        use std::sync::Mutex;
        use std::time::{Duration, Instant};
        static CACHE: Mutex<Option<HashMap<String, (Instant, bool)>>> = Mutex::new(None);
        let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        let m = c.get_or_insert_with(HashMap::new);
        if let Some((t, v)) = m.get(&self.name)
            && t.elapsed() < Duration::from_secs(10)
        {
            return *v;
        }
        let v = self.check_installed();
        m.insert(self.name.clone(), (Instant::now(), v));
        v
    }

    fn check_installed(&self) -> bool {
        self.installed_bin().is_some()
            || self.git.is_some() && self.git_dir().join(".git").is_dir()
            || self.aur.as_ref().is_some_and(|p| {
                std::process::Command::new("pacman")
                    .args(["-Q", p])
                    .output()
                    .is_ok_and(|o| o.status.success())
            })
    }

    /// Installed by the program's own per-user installer (in ~/.local/share/<local_dir>) and
    /// not by the AUR package.
    pub fn local_install(&self) -> bool {
        self.local_dir
            .as_ref()
            .is_some_and(|d| home().join(".local/share").join(d).is_dir())
            && !self.aur.as_ref().is_some_and(|p| {
                std::process::Command::new("pacman")
                    .args(["-Q", p])
                    .output()
                    .is_ok_and(|o| o.status.success())
            })
    }

    /// Update of a per-user install: newest code in a temporary folder + its own installer.
    pub fn local_update_command(&self) -> Option<Vec<String>> {
        if !self.local_install() {
            return None;
        }
        let url = self.git.as_ref()?;
        let setup = self.local_setup.as_ref()?;
        let sc = [
            "set -e".to_string(),
            "tmp=$(mktemp -d)".into(),
            "trap 'rm -rf \"$tmp\"' EXIT".into(),
            format!(
                "echo \"Updating {} (per-user install)\"",
                self.name.replace('"', "")
            ),
            format!("git clone --depth 1 {} \"$tmp/src\"", sh_quote(url)),
            "cd \"$tmp/src\"".into(),
            setup.clone(),
        ];
        Some(vec!["sh".into(), "-c".into(), sc.join("\n")])
    }

    /// Can it be installed/updated through numapilot?
    pub fn installable(&self) -> bool {
        self.aur.is_some() || self.git.is_some()
    }

    /// Command that installs or updates to the newest version (run in a terminal: asks for the
    /// password and shows progress). `helper` = paru or yay.
    pub fn install_command(&self, helper: &str) -> Option<Vec<String>> {
        if let Some(cmd) = self.local_update_command() {
            return Some(cmd);
        }
        if let Some(pkg) = &self.aur {
            // -git: paru/yay rebuilds from the current code
            let mut v = vec![helper.to_string(), "-S".into()];
            v.extend(self.aur_deps.iter().cloned());
            v.push(pkg.clone());
            return Some(v);
        }
        let url = self.git.as_ref()?;
        let d = self.git_dir();
        let d = sh_quote(&d.to_string_lossy());
        let mut sc = vec!["set -e".to_string()];
        if !self.aur_deps.is_empty() {
            let deps: Vec<String> = self.aur_deps.iter().map(|p| sh_quote(p)).collect();
            sc.push(format!("{helper} -S --needed {}", deps.join(" ")));
        }
        sc.push(format!(
            "if [ -d {d}/.git ]; then echo \"Atualizando {}\"; git -C {d} pull --ff-only; \
             else mkdir -p \"$(dirname {d})\"; git clone --depth 1 {} {d}; fi",
            self.name.replace('"', ""),
            sh_quote(url)
        ));
        if !self.scripts.is_empty() {
            sc.push("mkdir -p \"$HOME/.local/bin\"".into());
            for s in &self.scripts {
                let f = format!("{d}/{}", sh_quote(s));
                sc.push(format!(
                    "chmod +x {f} && ln -sf {f} \"$HOME/.local/bin/\"{}",
                    sh_quote(s)
                ));
            }
        }
        if let Some(setup) = &self.setup {
            sc.push(format!("cd {d} && {setup}"));
        }
        sc.push(crate::tf!(
            "echo; echo \"Done: {}\"",
            "echo; echo \"Pronto: {}\"",
            self.name.replace('"', "")
        ));
        Some(vec!["sh".into(), "-c".into(), sc.join("\n")])
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CpuRecipe {
    #[serde(rename = "match")]
    pub matches: String,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Arch {
    pub prefix: String,
    pub family: String,
    /// rocm | rocm-community | vulkan | cuda
    pub compute: String,
    pub upscale: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Recipes {
    pub version: u32,
    pub board: Vec<Board>,
    pub cpu: Vec<CpuRecipe>,
    pub arch: Vec<Arch>,
}

pub fn override_path() -> PathBuf {
    crate::learned::state_dir().join("recipes.toml")
}

pub fn parse(s: &str) -> Result<Recipes, String> {
    toml::from_str(s).map_err(|e| e.to_string())
}

/// The recipes in use: the downloaded copy if valid and not older than the built-in one.
pub fn load() -> Recipes {
    let base = parse(EMBEDDED).unwrap_or_default();
    match std::fs::read_to_string(override_path())
        .ok()
        .and_then(|s| parse(&s).ok())
    {
        Some(r) if r.version >= base.version => r,
        _ => base,
    }
}

fn gpu_key(g: &Gpu) -> String {
    format!("{:04x}:{:04x}", g.vendor, g.device)
}

impl Board {
    pub fn matches(&self, g: &Gpu) -> bool {
        self.gpu_ids
            .iter()
            .any(|i| i.eq_ignore_ascii_case(&gpu_key(g)))
            || g.gfx
                .as_ref()
                .is_some_and(|x| self.gfx.iter().any(|b| b == x))
            || self
                .name_contains
                .iter()
                .any(|n| g.name.to_lowercase().contains(n))
    }
}

impl Recipes {
    /// Special boards present in this PC.
    pub fn boards<'a>(&'a self, topo: &'a Topology) -> Vec<(&'a Board, &'a Gpu)> {
        let mut v = vec![];
        for g in &topo.gpus {
            if let Some(b) = self.board.iter().find(|b| b.matches(g)) {
                v.push((b, g));
            }
        }
        v
    }

    pub fn board_for(&self, g: &Gpu) -> Option<&Board> {
        self.board.iter().find(|b| b.matches(g))
    }

    /// GPU architecture (from ROCm's gfx, the vendor or the known board).
    pub fn arch_for(&self, g: &Gpu) -> Option<&Arch> {
        let key = if g.is_nvidia() {
            "nvidia".to_string()
        } else if g.vendor == 0x8086 {
            "intel".to_string()
        } else if let Some(x) = &g.gfx {
            x.clone()
        } else {
            self.board_for(g)?.gfx.first().cloned().unwrap_or_default()
        };
        // the most specific prefix wins (gfx1013 before gfx101)
        self.arch
            .iter()
            .filter(|a| key.starts_with(&a.prefix))
            .max_by_key(|a| a.prefix.len())
    }

    pub fn cpu_notes(&self, model: &str) -> Vec<String> {
        self.cpu
            .iter()
            .filter(|c| model.contains(&c.matches))
            .flat_map(|c| c.notes.clone())
            .collect()
    }
}

/// Processor name (first "model name" in /proc/cpuinfo).
pub fn cpu_model() -> String {
    if let Ok(m) = std::env::var("NUMAPILOT_CPU_MODEL") {
        return m;
    }
    std::fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .lines()
        .find_map(|l| {
            l.strip_prefix("model name")
                .and_then(|r| r.split(':').nth(1))
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_default()
}

/// "6.17.9-2-cachyos" → (6, 17, 9)
pub fn kernel_tuple(s: &str) -> Option<(u32, u32, u32)> {
    let mut it = s
        .split(|c: char| !c.is_ascii_digit())
        .filter(|x| !x.is_empty())
        .map(|x| x.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next().flatten().unwrap_or(0)))
}

pub fn kernel_in(ver: &str, ranges: &[(String, String)]) -> bool {
    let Some(k) = kernel_tuple(ver) else {
        return false;
    };
    ranges
        .iter()
        .any(|(a, b)| match (kernel_tuple(a), kernel_tuple(b)) {
            (Some(a), Some(b)) => k >= a && k <= b,
            _ => false,
        })
}

pub fn running_kernel() -> String {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Installed Mesa version (pacman), e.g. "25.3.1".
pub fn mesa_version() -> Option<String> {
    let o = std::process::Command::new("pacman")
        .args(["-Q", "mesa"])
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| {
            String::from_utf8_lossy(&o.stdout)
                .split_whitespace()
                .nth(1)
                .unwrap_or("")
                .to_string()
        })
        .map(|v| pacman_version(&v))
}

/// pacman package version without the epoch and pkgrel: "3:26.2.6-1.395" → "26.2.6".
pub fn pacman_version(v: &str) -> String {
    let v = v.rsplit_once(':').map_or(v, |(_, r)| r);
    v.split('-').next().unwrap_or("").to_string()
}

pub fn version_ge(a: &str, b: &str) -> bool {
    let p = |s: &str| {
        s.split('.')
            .map(|x| x.parse::<u32>().unwrap_or(0))
            .collect::<Vec<_>>()
    };
    p(a) >= p(b)
}

/// Downloads new recipes (the repository must be reachable) and writes the local copy.
pub fn update(url: &str) -> Result<u32, String> {
    let o = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "20", url])
        .output()
        .map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(crate::tf!(
            "could not download {url}",
            "não consegui baixar {url}"
        ));
    }
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    let r = parse(&s)?;
    if r.board.is_empty() && r.arch.is_empty() {
        return Err(crate::i18n::tr("empty recipes file").into());
    }
    std::fs::create_dir_all(crate::learned::state_dir()).map_err(|e| e.to_string())?;
    std::fs::write(override_path(), s).map_err(|e| e.to_string())?;
    Ok(r.version)
}

pub const DEFAULT_URL: &str =
    "https://raw.githubusercontent.com/lucasfracaro/numapilot/main/recipes/default.toml";

#[cfg(test)]
mod app_tests {
    use super::CommunityApp;

    #[test]
    fn instalar_pelo_git_ou_aur() {
        let a = super::CommunityApp {
            name: "bc250 cu live-manager".into(),
            git: Some("https://github.com/x/y".into()),
            scripts: vec!["y.sh".into()],
            aur_deps: vec!["umr".into()],
            ..Default::default()
        };
        let c = a.install_command("paru").unwrap();
        assert_eq!(c[0], "sh");
        let sc = &c[2];
        assert!(sc.contains("paru -S --needed 'umr'"));
        assert!(sc.contains("git -C") && sc.contains("pull --ff-only"));
        assert!(sc.contains("git clone --depth 1 'https://github.com/x/y'"));
        assert!(sc.contains("comunidade/bc250-cu-live-manager"));
        assert!(sc.contains("ln -sf"));
        let b = CommunityApp {
            aur: Some("bc250-control-center-git".into()),
            ..Default::default()
        };
        assert_eq!(
            b.install_command("yay").unwrap(),
            vec!["yay", "-S", "bc250-control-center-git"]
        );
        assert!(CommunityApp::default().install_command("paru").is_none());
    }
}

#[cfg(test)]
mod tests {
    /// Every text of the built-in recipes (program descriptions and tips) is in English and
    /// has its Portuguese pair in the translation table.
    #[test]
    fn recipe_texts_have_a_portuguese_pair() {
        let r: super::Recipes = toml::from_str(super::EMBEDDED).unwrap();
        let mut texts: Vec<&str> = Vec::new();
        for b in &r.board {
            texts.extend(b.apps.iter().map(|a| a.about.as_str()));
            texts.extend(b.notes.iter().map(|n| n.as_str()));
        }
        for c in &r.cpu {
            texts.extend(c.notes.iter().map(|n| n.as_str()));
        }
        let missing: Vec<&str> = texts
            .into_iter()
            .filter(|t| !crate::i18n::has_pair(t))
            .collect();
        assert!(
            missing.is_empty(),
            "no Portuguese pair for:\n{}",
            missing.join("\n")
        );
    }

    use super::*;

    #[test]
    fn embedded_recipes() {
        let r = parse(EMBEDDED).unwrap();
        assert!(
            r.board
                .iter()
                .any(|b| b.id == "bc250" && b.bad_kernels.len() == 2)
        );
        assert!(kernel_in("6.17.9-2-cachyos", &r.board[0].bad_kernels));
        assert!(!kernel_in("6.18.1-1-cachyos", &r.board[0].bad_kernels));
        assert!(kernel_in("6.15.0-1-cachyos", &r.board[0].bad_kernels));
        assert!(version_ge("25.3.1", "25.1") && !version_ge("24.2.8", "25.1"));
        assert_eq!(pacman_version("3:26.2.6-1.395"), "26.2.6");
        assert_eq!(pacman_version("1:25.0.1-2"), "25.0.1");
        assert_eq!(pacman_version("24.2.8-1"), "24.2.8");
        assert!(version_ge(&pacman_version("3:26.2.6-1.395"), "25.1"));
        let mk = |gfx: Option<&str>, vendor: u16| Gpu {
            alias: "x".into(),
            pci: "0000:01:00.0".into(),
            vendor,
            device: 0x13fe,
            name: String::new(),
            numa_node: None,
            card: None,
            render: None,
            display_connected: false,
            gfx: gfx.map(String::from),
            rocr_id: None,
            role: crate::topo::GpuRole::Other,
        };
        let bc = mk(None, 0x1002);
        assert_eq!(r.board_for(&bc).map(|b| b.id.as_str()), Some("bc250"));
        assert_eq!(r.arch_for(&bc).map(|a| a.compute.as_str()), Some("vulkan"));
        assert_eq!(
            r.arch_for(&mk(Some("gfx1200"), 0x1002))
                .map(|a| a.family.as_str()),
            Some("RDNA 4")
        );
        assert_eq!(
            r.arch_for(&mk(Some("gfx906"), 0x1002))
                .map(|a| a.compute.as_str()),
            Some("rocm-community")
        );
        assert_eq!(
            r.arch_for(&mk(None, 0x10de)).map(|a| a.compute.as_str()),
            Some("cuda")
        );
        assert!(
            r.cpu_notes("Intel(R) Core(TM) i7-5775C CPU @ 3.30GHz")
                .len()
                == 2
        );
    }
}

#[cfg(test)]
mod install_real {
    #[test]
    fn local_update_only_for_user_installs() {
        let a = super::CommunityApp {
            name: "X".into(),
            git: Some("https://example.invalid/x".into()),
            local_dir: Some("numapilot-test-surely-missing-dir".into()),
            local_setup: Some("true".into()),
            ..Default::default()
        };
        assert!(a.local_update_command().is_none());
    }

    /// Really runs the git install and update in a temporary HOME (needs network):
    /// `cargo test --release real_git_install -- --ignored`.
    #[test]
    #[ignore]
    fn real_git_install() {
        let home = std::env::temp_dir().join(format!("np-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        unsafe { std::env::set_var("HOME", &home) };
        let rc = super::load();
        let a = rc
            .board
            .iter()
            .flat_map(|b| &b.apps)
            .find(|a| a.name == "bc250-core-unlock")
            .unwrap()
            .clone();
        for _ in 0..2 {
            let c = a.install_command("paru").unwrap();
            let st = std::process::Command::new(&c[0])
                .args(&c[1..])
                .status()
                .unwrap();
            assert!(st.success());
        }
        assert!(a.git_dir().join(".git").is_dir());
        assert!(home.join(".local/bin/bc250-unlock-cores.py").is_file());
        assert!(
            a.installed_bin()
                .unwrap()
                .ends_with("bc250-unlock-cores.py")
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
