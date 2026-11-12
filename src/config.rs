//! Configuration in /etc/numapilot.toml (manual rules and automatic-mode parameters).

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

pub const DEFAULT_PATH: &str = "/etc/numapilot.toml";

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub daemon: DaemonCfg,
    /// PCI -> alias. E.g.: "0000:03:00.0" = "rx470"
    pub aliases: HashMap<String, String>,
    /// PCI -> NUMA node, for when the firmware does not report it.
    pub gpu_node: HashMap<String, usize>,
    /// Default GPU for games/graphics on automatic (alias or PCI). Empty = the one with the monitor.
    pub graphics_gpu: Option<String>,
    /// Programs that, on automatic, go to a compute GPU (ROCm).
    pub compute_hints: Vec<String>,
    #[serde(rename = "rule")]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct DaemonCfg {
    /// Interval between samples (ms).
    pub tick_ms: u64,
    /// Only manages processes of users with UID >= this value.
    pub min_uid: u32,
    /// From how many cores in use a process starts being placed.
    pub watch_cores: f64,
    /// How many samples in a row above `watch_cores` before placing.
    pub watch_ticks: u32,
    /// Fraction of the node's CPUs in use that triggers expansion to all sockets.
    pub promote_frac: f64,
    pub promote_ticks: u32,
    /// Below this fraction for `demote_ticks` samples, goes back to a single socket.
    pub demote_frac: f64,
    pub demote_ticks: u32,
    /// Migrate the memory already allocated to the chosen node.
    pub migrate_memory: bool,
    /// Processes the daemon never touches.
    pub ignore: Vec<String>,
    /// Each GPU's interrupts handled by the socket it is connected to.
    pub gpu_irq: bool,
    /// The same for NVMe disks and network cards.
    pub device_irq: bool,
    /// Groups that may change the whole machine through the service (modes, fans, card clock and
    /// voltage, notifications). Per-program choices still work for any user, only on their own
    /// processes. `["*"]` allows every user.
    pub admin_groups: Vec<String>,
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
            admin_groups: vec!["wheel".into(), "sudo".into(), "numapilot".into()],
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct Rule {
    pub name: Option<String>,
    /// Globs matched against the process name, the executable and the first arguments.
    #[serde(rename = "match")]
    pub patterns: Vec<String>,
    /// "auto" | "all" | "gpu" | "0" | "1" | "0,1" | "cpus:0-21"
    pub cpu: String,
    /// "auto" | "display" | "compute" | "compute:all" | "none" | alias | PCI (comma-separated list)
    pub gpu: String,
    /// "auto" | "graphics" | "compute": how to use the GPU (e.g. gaming on an MI50 = graphics)
    #[serde(rename = "use")]
    pub usage: String,
    /// "local" | "interleave" | "bind"
    pub mem: String,
    /// While this program runs, automatic mode avoids placing other processes on the same node.
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
                .with_context(|| crate::tf!("error reading {}", "erro ao ler {}", p.display()))?,
            Err(_) => Config::default(),
        };
        if cfg.compute_hints.is_empty() {
            cfg.compute_hints = default_compute_hints();
        }
        for r in &cfg.rules {
            CpuSpec::parse(&r.cpu).with_context(|| {
                crate::tf!(
                    "rule '{}': invalid cpu",
                    "regra '{}': cpu inválido",
                    r.label()
                )
            })?;
            GpuSpec::parse(&r.gpu).with_context(|| {
                crate::tf!(
                    "rule '{}': invalid gpu",
                    "regra '{}': gpu inválido",
                    r.label()
                )
            })?;
            MemSpec::parse(&r.mem).with_context(|| {
                crate::tf!(
                    "rule '{}': invalid mem",
                    "regra '{}': mem inválido",
                    r.label()
                )
            })?;
            crate::decide::Kind::parse_usage(&r.usage).with_context(|| {
                crate::tf!(
                    "rule '{}': invalid use",
                    "regra '{}': use inválido",
                    r.label()
                )
            })?;
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
    /// Node of the chosen GPU(s).
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
                    bail!(
                        "{}",
                        crate::tf!("empty CPU list: {s}", "lista de CPUs vazia: {s}")
                    );
                }
                CpuSpec::Cpus(v)
            }
            _ => {
                let v: Result<Vec<usize>, _> =
                    s.split(',').map(|x| x.trim().parse::<usize>()).collect();
                match v {
                    Ok(v) if !v.is_empty() => CpuSpec::Nodes(v),
                    _ => {
                        bail!(
                            "{}",
                            crate::tf!(
                                "use auto, all, gpu, node number(s) or cpus:LIST (got '{s}')",
                                "use auto, all, gpu, número(s) de nó ou cpus:LISTA (recebido '{s}')"
                            )
                        )
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
    /// N least busy compute GPUs (0 = all)
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
            o => bail!(
                "{}",
                crate::tf!(
                    "mem must be local, interleave or bind (got '{o}')",
                    "mem deve ser local, interleave ou bind (recebido '{o}')"
                )
            ),
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
