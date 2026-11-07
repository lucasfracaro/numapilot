//! Configuração em /etc/numapilot.toml (regras manuais e parâmetros do modo automático).

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

pub const DEFAULT_PATH: &str = "/etc/numapilot.toml";

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub daemon: DaemonCfg,
    /// PCI -> apelido. Ex.: "0000:03:00.0" = "rx470"
    pub aliases: HashMap<String, String>,
    /// PCI -> nó NUMA, para quando o firmware não informa.
    pub gpu_node: HashMap<String, usize>,
    /// GPU padrão para jogos/gráficos no automático (apelido ou PCI). Vazio = a que tem monitor.
    pub graphics_gpu: Option<String>,
    /// Programas que, no automático, vão para GPU de computação (ROCm).
    pub compute_hints: Vec<String>,
    #[serde(rename = "rule")]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct DaemonCfg {
    /// Intervalo entre amostras (ms).
    pub tick_ms: u64,
    /// Só gerencia processos de usuários com UID >= este valor.
    pub min_uid: u32,
    /// A partir de quantos núcleos em uso um processo passa a ser posicionado.
    pub watch_cores: f64,
    /// Quantas amostras seguidas acima de `watch_cores` antes de posicionar.
    pub watch_ticks: u32,
    /// Fração dos CPUs do nó em uso que dispara a expansão para todos os sockets.
    pub promote_frac: f64,
    pub promote_ticks: u32,
    /// Abaixo desta fração por `demote_ticks` amostras, volta para um socket só.
    pub demote_frac: f64,
    pub demote_ticks: u32,
    /// Migrar a memória já alocada para o nó escolhido.
    pub migrate_memory: bool,
    /// Processos que o daemon nunca toca.
    pub ignore: Vec<String>,
    /// Interrupções de cada GPU atendidas pelo socket onde ela está ligada.
    pub gpu_irq: bool,
    /// O mesmo para discos NVMe e placas de rede.
    pub device_irq: bool,
}

impl Default for DaemonCfg {
    fn default() -> Self {
        DaemonCfg {
            tick_ms: 1000,
            min_uid: 1000,
            watch_cores: 0.5,
            watch_ticks: 3,
            promote_frac: 0.85,
            promote_ticks: 10,
            demote_frac: 0.30,
            demote_ticks: 120,
            migrate_memory: true,
            ignore: vec!["numapilot".into(), "systemd".into(), "dbus-*".into()],
            gpu_irq: true,
            device_irq: true,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Rule {
    pub name: Option<String>,
    /// Globs comparados com o nome do processo, o executável e os primeiros argumentos.
    #[serde(rename = "match")]
    pub patterns: Vec<String>,
    /// "auto" | "all" | "gpu" | "0" | "1" | "0,1" | "cpus:0-21"
    pub cpu: String,
    /// "auto" | "display" | "compute" | "compute:all" | "none" | apelido | PCI (lista com vírgula)
    pub gpu: String,
    /// "auto" | "graphics" | "compute": como usar a GPU (ex.: jogar numa MI50 = graphics)
    #[serde(rename = "use")]
    pub usage: String,
    /// "local" | "interleave" | "bind"
    pub mem: String,
    /// Enquanto este programa roda, o automático evita colocar outros processos no mesmo nó.
    pub reserve: bool,
}

impl Default for Rule {
    fn default() -> Self {
        Rule {
            name: None,
            patterns: vec![],
            cpu: "auto".into(),
            gpu: "auto".into(),
            usage: "auto".into(),
            mem: "local".into(),
            reserve: false,
        }
    }
}

impl Rule {
    pub fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| self.patterns.join("|"))
    }
    pub fn matches(&self, idents: &[String]) -> bool {
        self.patterns
            .iter()
            .any(|p| idents.iter().any(|i| crate::util::glob_match(p, i)))
    }
}

pub fn default_compute_hints() -> Vec<String> {
    [
        "llama-*",
        "llama.cpp*",
        "ollama",
        "koboldcpp*",
        "comfyui",
        "main.py",
        "vllm",
        "rocminfo",
        "hipcc",
        "rocm-smi",
        "amd-smi",
        "whisper*",
        "sd",
        "stable-diffusion*",
        "rvs",
        "TransferBench",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Config {
    pub fn path() -> PathBuf {
        std::env::var_os("NUMAPILOT_CONFIG")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_PATH))
    }

    pub fn load() -> Result<Config> {
        let p = Self::path();
        let mut cfg = match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str::<Config>(&s)
                .with_context(|| format!("erro ao ler {}", p.display()))?,
            Err(_) => Config::default(),
        };
        if cfg.compute_hints.is_empty() {
            cfg.compute_hints = default_compute_hints();
        }
        for r in &cfg.rules {
            CpuSpec::parse(&r.cpu)
                .with_context(|| format!("regra '{}': cpu inválido", r.label()))?;
            GpuSpec::parse(&r.gpu)
                .with_context(|| format!("regra '{}': gpu inválido", r.label()))?;
            MemSpec::parse(&r.mem)
                .with_context(|| format!("regra '{}': mem inválido", r.label()))?;
            crate::decide::Kind::parse_usage(&r.usage)
                .with_context(|| format!("regra '{}': use inválido", r.label()))?;
        }
        Ok(cfg)
    }

    pub fn find_rule(&self, idents: &[String]) -> Option<&Rule> {
        self.rules.iter().find(|r| r.matches(idents))
    }

    pub fn is_compute_hint(&self, idents: &[String]) -> bool {
        self.compute_hints
            .iter()
            .any(|p| idents.iter().any(|i| crate::util::glob_match(p, i)))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CpuSpec {
    Auto,
    All,
    /// Nó da(s) GPU(s) escolhida(s).
    GpuNode,
    Nodes(Vec<usize>),
    Cpus(Vec<usize>),
}

impl CpuSpec {
    pub fn parse(s: &str) -> Result<CpuSpec> {
        let s = s.trim().to_lowercase();
        Ok(match s.as_str() {
            "" | "auto" => CpuSpec::Auto,
            "all" | "todos" | "both" => CpuSpec::All,
            "gpu" | "gpu-node" => CpuSpec::GpuNode,
            _ if s.starts_with("cpus:") => {
                let v = crate::util::parse_cpulist(&s[5..]);
                if v.is_empty() {
                    bail!("lista de CPUs vazia: {s}");
                }
                CpuSpec::Cpus(v)
            }
            _ => {
                let v: Result<Vec<usize>, _> =
                    s.split(',').map(|x| x.trim().parse::<usize>()).collect();
                match v {
                    Ok(v) if !v.is_empty() => CpuSpec::Nodes(v),
                    _ => {
                        bail!("use auto, all, gpu, número(s) de nó ou cpus:LISTA (recebido '{s}')")
                    }
                }
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum GpuSpec {
    Auto,
    None,
    Display,
    /// N GPUs de computação menos ocupadas (0 = todas)
    Compute(usize),
    Named(Vec<String>),
}

impl GpuSpec {
    pub fn parse(s: &str) -> Result<GpuSpec> {
        let s = s.trim().to_lowercase();
        Ok(match s.as_str() {
            "" | "auto" => GpuSpec::Auto,
            "none" | "nenhuma" => GpuSpec::None,
            "display" | "video" | "graphics" => GpuSpec::Display,
            "compute" => GpuSpec::Compute(1),
            "compute:all" => GpuSpec::Compute(0),
            _ if s.starts_with("compute:") => {
                GpuSpec::Compute(s[8..].parse().context("compute:N")?)
            }
            _ => GpuSpec::Named(
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect(),
            ),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MemSpec {
    Local,
    Interleave,
    Bind,
}

impl MemSpec {
    pub fn parse(s: &str) -> Result<MemSpec> {
        Ok(match s.trim().to_lowercase().as_str() {
            "" | "local" | "auto" => MemSpec::Local,
            "interleave" => MemSpec::Interleave,
            "bind" => MemSpec::Bind,
            o => bail!("mem deve ser local, interleave ou bind (recebido '{o}')"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_specs() {
        assert_eq!(CpuSpec::parse("all").unwrap(), CpuSpec::All);
        assert_eq!(CpuSpec::parse("1").unwrap(), CpuSpec::Nodes(vec![1]));
        assert_eq!(CpuSpec::parse("0,1").unwrap(), CpuSpec::Nodes(vec![0, 1]));
        assert_eq!(
            CpuSpec::parse("cpus:0-3").unwrap(),
            CpuSpec::Cpus(vec![0, 1, 2, 3])
        );
        assert!(CpuSpec::parse("abc").is_err());
        assert_eq!(GpuSpec::parse("compute:all").unwrap(), GpuSpec::Compute(0));
        assert_eq!(GpuSpec::parse("compute:2").unwrap(), GpuSpec::Compute(2));
        assert_eq!(
            GpuSpec::parse("rx470,mi50a").unwrap(),
            GpuSpec::Named(vec!["rx470".into(), "mi50a".into()])
        );
    }

    #[test]
    fn parse_example_config() {
        let s = include_str!("../packaging/numapilot.toml");
        let cfg: Config = toml::from_str(s).expect("exemplo de config precisa ser válido");
        assert!(!cfg.rules.is_empty());
        for r in &cfg.rules {
            CpuSpec::parse(&r.cpu).unwrap();
            GpuSpec::parse(&r.gpu).unwrap();
            MemSpec::parse(&r.mem).unwrap();
        }
    }
}
