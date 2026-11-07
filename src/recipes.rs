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
}

/// Programa da comunidade: o numapilot mostra, instala (AUR) e abre; não reimplementa.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommunityApp {
    pub name: String,
    /// Executáveis que indicam que está instalado (o primeiro que existir é o que abre).
    pub bins: Vec<String>,
    pub aur: Option<String>,
    pub url: Option<String>,
    pub about: String,
}

impl CommunityApp {
    pub fn installed_bin(&self) -> Option<&str> {
        self.bins
            .iter()
            .find(|b| crate::containers::which(b))
            .map(|s| s.as_str())
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
    o.status.success().then(|| {
        String::from_utf8_lossy(&o.stdout)
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .split('-')
            .next()
            .unwrap_or("")
            .trim_start_matches("1:")
            .to_string()
    })
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
        return Err(format!("não consegui baixar {url}"));
    }
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    let r = parse(&s)?;
    if r.board.is_empty() && r.arch.is_empty() {
        return Err("arquivo de receitas vazio".into());
    }
    std::fs::create_dir_all(crate::learned::state_dir()).map_err(|e| e.to_string())?;
    std::fs::write(override_path(), s).map_err(|e| e.to_string())?;
    Ok(r.version)
}

pub const DEFAULT_URL: &str =
    "https://raw.githubusercontent.com/lucasfracaro/numapilot/main/recipes/default.toml";

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
