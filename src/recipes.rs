//! Receitas da comunidade (recipes/default.toml embutido no programa, ou uma cópia mais nova em
//! /var/lib/numapilot/recipes.toml): placas especiais, kernels ruins, governors, dicas por
//! processador e o que cada arquitetura de GPU aguenta (computação e upscaling).

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
    /// Núcleos de CPU de fábrica e com desbloqueio (BC-250: 6 e 8).
    pub cores: Option<(usize, usize)>,
    /// CUs da GPU de fábrica e com desbloqueio (BC-250: 24 e 40).
    pub cus: Option<(u32, u32)>,
    /// Programas da comunidade feitos para esta placa.
    pub apps: Vec<CommunityApp>,
    /// Sem codificador de vídeo funcionando (streaming por software).
    pub no_video_encoder: bool,
}

/// Programa da comunidade: o numapilot mostra, instala e atualiza (AUR ou git) e abre; não
/// reimplementa. Sempre a versão mais nova de quem mantém: pacotes `-git` do AUR são
/// recompilados do código atual e os repositórios git são atualizados com `git pull`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommunityApp {
    pub name: String,
    /// Executáveis que indicam que está instalado (o primeiro que existir é o que abre).
    pub bins: Vec<String>,
    pub aur: Option<String>,
    pub url: Option<String>,
    pub about: String,
    /// O recomendado da placa: aparece primeiro, em destaque.
    pub recommended: bool,
    /// Repositório para instalar sem pacote: clonado em
    /// ~/.local/share/numapilot/comunidade/<nome> e atualizado com `git pull`.
    pub git: Option<String>,
    /// Arquivos do repositório que viram comandos em ~/.local/bin.
    pub scripts: Vec<String>,
    /// Pacotes do AUR de que ele precisa (instalados antes).
    pub aur_deps: Vec<String>,
    /// Comando rodado na pasta do repositório depois de baixar ou atualizar (compilar, instalar).
    pub setup: Option<String>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

/// 'texto' para o shell.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

impl CommunityApp {
    /// Caminho do executável instalado: no PATH ou em ~/.local/bin (a interface aberta pelo
    /// menu nem sempre tem o ~/.local/bin no PATH).
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

    /// Pasta do clone git (`~/.local/share/numapilot/comunidade/<nome>`).
    pub fn git_dir(&self) -> PathBuf {
        let slug: String = self
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        home().join(".local/share/numapilot/comunidade").join(slug)
    }

    /// Instalado pelo numapilot por git, ou pelo pacote do AUR. Guardado por 10 s (a interface
    /// pergunta a cada quadro e o pacman -Q é lento).
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

    /// Dá para instalar/atualizar pelo numapilot?
    pub fn installable(&self) -> bool {
        self.aur.is_some() || self.git.is_some()
    }

    /// Comando que instala ou atualiza para a versão mais nova (rodar num terminal: pede senha
    /// e mostra o progresso). `helper` = paru ou yay.
    pub fn install_command(&self, helper: &str) -> Option<Vec<String>> {
        if let Some(pkg) = &self.aur {
            // -git: o paru/yay compila de novo a partir do código atual
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
    /// rocm | rocm-comunidade | vulkan | cuda
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

/// As receitas em uso: a cópia baixada se for válida e não mais velha que a embutida.
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
    /// Placas especiais presentes neste PC.
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

    /// Arquitetura da GPU (pelo gfx do ROCm, pelo fabricante ou pela placa conhecida).
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
        // o prefixo mais específico ganha (gfx1013 antes de gfx101)
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

/// Nome do processador (primeira "model name" do /proc/cpuinfo).
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

/// Versão do Mesa instalada (pacman), ex.: "25.3.1".
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

/// Versão de pacote do pacman sem a época e o pkgrel: "3:26.2.6-1.395" → "26.2.6".
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

/// Baixa receitas novas (o repositório precisa estar acessível) e grava a cópia local.
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
        let a = CommunityApp {
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
            Some("rocm-comunidade")
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
    /// Roda de verdade a instalação e a atualização por git num HOME temporário (precisa de
    /// rede): `cargo test --release instalacao_git_de_verdade -- --ignored`.
    #[test]
    #[ignore]
    fn instalacao_git_de_verdade() {
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
