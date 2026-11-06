//! Perfis por programa escolhidos pela interface gráfica (ou por `numapilot profile`).
//! Ficam em /var/lib/numapilot/profiles.json (gravados pelo daemon, legíveis por todos).

use crate::learned::state_dir;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn auto() -> String {
    "auto".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    /// Nome amigável mostrado na interface (ex.: "Cyberpunk2077.exe").
    #[serde(default)]
    pub label: String,
    /// "auto" | "gpu" | "0" | "1" | "all" ...
    #[serde(default = "auto")]
    pub cpu: String,
    /// "auto" | "display" | apelido/PCI da GPU
    #[serde(default = "auto")]
    pub gpu: String,
    /// "auto" | "graphics" | "compute": como usar a GPU escolhida.
    #[serde(default = "auto", rename = "use")]
    pub usage: String,
    /// GPU do frame generation do Lossless Scaling (lsfg-vk): "same" = a mesma do jogo.
    /// Outra GPU só funciona com uma versão do lsfg-vk que suporte dual GPU.
    #[serde(default = "same")]
    pub fg_gpu: String,
    /// Quantos CPUs lógicos o programa pode usar (None = todos os dos sockets escolhidos).
    #[serde(default)]
    pub cores: Option<usize>,
    /// Pode usar os dois threads de cada núcleo (hyper-threading)?
    #[serde(default = "yes")]
    pub smt: bool,
    /// CPUs escolhidos à mão no mapa (ex.: "22-29,66-73"); tem prioridade sobre sockets/núcleos.
    #[serde(default)]
    pub cpu_list: Option<String>,
    /// Memória: "auto" | "strict" (só do(s) socket(s) dos núcleos) | "prefer" | "interleave" | "free"
    #[serde(default = "auto")]
    pub memory: String,
    /// Limite de memória em GB.
    #[serde(default)]
    pub mem_max_gb: Option<f64>,
    /// "low" | "normal" | "high"
    #[serde(default = "normal")]
    pub priority: String,
    /// Reserva os núcleos só para este programa enquanto ele roda.
    #[serde(default)]
    pub exclusive: bool,
    /// Faz o programa "enxergar" só os núcleos liberados (Proton/Wine, OpenMP, MKL, .NET, Java...).
    #[serde(default = "yes")]
    pub limit_visible: bool,
    /// Enquanto roda: núcleos dele sem os estados de economia profundos e uncore no máximo.
    #[serde(default)]
    pub low_latency: bool,
    /// Perfil de energia da GPU enquanto roda: "auto" (não mexe) | "3d" | "compute" | "vr" | "video" | "economia"
    #[serde(default = "auto")]
    pub gpu_power: String,
    /// Abre o jogo dentro do gamescope (resolução interna, FSR, limite de FPS).
    #[serde(default)]
    pub gamescope: Option<Gamescope>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Gamescope {
    /// Resolução em que o jogo renderiza (None = a da tela).
    #[serde(default)]
    pub render_w: Option<u32>,
    #[serde(default)]
    pub render_h: Option<u32>,
    /// Resolução de saída (None = a da tela).
    #[serde(default)]
    pub out_w: Option<u32>,
    #[serde(default)]
    pub out_h: Option<u32>,
    #[serde(default = "yes")]
    pub fullscreen: bool,
    /// Amplia com FSR 1 quando a resolução interna é menor.
    #[serde(default)]
    pub fsr: bool,
    #[serde(default)]
    pub fps_limit: Option<u32>,
}

impl Default for Gamescope {
    fn default() -> Self {
        Gamescope { render_w: None, render_h: None, out_w: None, out_h: None, fullscreen: true, fsr: false, fps_limit: None }
    }
}

impl Gamescope {
    /// Argumentos do gamescope (sem o "--" final).
    pub fn argv(&self) -> Vec<String> {
        let mut a = vec!["gamescope".to_string()];
        let mut push = |f: &str, v: Option<u32>| {
            if let Some(v) = v {
                a.push(f.into());
                a.push(v.to_string());
            }
        };
        push("-w", self.render_w);
        push("-h", self.render_h);
        push("-W", self.out_w);
        push("-H", self.out_h);
        push("-r", self.fps_limit);
        if self.fullscreen {
            a.push("-f".into());
        }
        if self.fsr {
            a.push("-F".into());
            a.push("fsr".into());
        }
        a
    }
}

fn same() -> String {
    "same".into()
}
fn yes() -> bool {
    true
}
fn normal() -> String {
    "normal".into()
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            label: String::new(),
            cpu: auto(),
            gpu: auto(),
            usage: auto(),
            fg_gpu: same(),
            cores: None,
            smt: true,
            cpu_list: None,
            memory: auto(),
            mem_max_gb: None,
            priority: normal(),
            exclusive: false,
            limit_visible: true,
            low_latency: false,
            gpu_power: auto(),
            gamescope: None,
        }
    }
}

impl Profile {
    pub fn is_auto(&self) -> bool {
        let d = Profile { label: self.label.clone(), ..Default::default() };
        *self == d
    }

    /// Algo na parte de CPU/memória que não é automático?
    pub fn has_cpu_choice(&self) -> bool {
        self.cpu != "auto" || self.cores.is_some() || !self.smt || self.cpu_list.is_some()
    }

    /// Precisa de um grupo de controle (cgroup) do systemd para valer.
    pub fn needs_scope(&self) -> bool {
        self.memory == "strict" || self.mem_max_gb.is_some() || self.exclusive
    }

    /// Precisa do serviço (root) para aplicar: cgroup ou prioridade alta.
    pub fn needs_daemon_hw(&self) -> bool {
        self.low_latency || self.gpu_power != "auto"
    }

    pub fn needs_daemon(&self) -> bool {
        self.needs_scope() || self.priority == "high"
    }

    pub fn validate(&self) -> Result<(), String> {
        if !["auto", "strict", "prefer", "interleave", "free"].contains(&self.memory.as_str()) {
            return Err(format!("política de memória inválida: {}", self.memory));
        }
        if !["low", "normal", "high"].contains(&self.priority.as_str()) {
            return Err(format!("prioridade inválida: {}", self.priority));
        }
        if !["auto", "3d", "compute", "vr", "video", "economia"].contains(&self.gpu_power.as_str()) {
            return Err(format!("perfil de energia da GPU inválido: {}", self.gpu_power));
        }
        if self.cores == Some(0) {
            return Err("núcleos: pelo menos 1".into());
        }
        if let Some(m) = self.mem_max_gb
            && m < 0.25
        {
            return Err("limite de memória muito baixo (mínimo 0,25 GB)".into());
        }
        if let Some(l) = &self.cpu_list
            && crate::util::parse_cpulist(l).is_empty()
        {
            return Err(format!("lista de CPUs inválida: {l}"));
        }
        Ok(())
    }

    /// Perfis prontos de um clique.
    pub fn preset(name: &str, base: &Profile) -> Profile {
        let mut p = Profile { label: base.label.clone(), gpu: base.gpu.clone(), usage: base.usage.clone(), fg_gpu: base.fg_gpu.clone(), gamescope: base.gamescope.clone(), ..Default::default() };
        match name {
            // Jogo: socket da GPU, núcleos físicos (sem dividir núcleo), memória só local,
            // prioridade alta e núcleos reservados.
            "jogo" => {
                p.cpu = "gpu".into();
                p.cores = Some(16);
                p.smt = false;
                p.memory = "strict".into();
                p.priority = "high".into();
                p.exclusive = true;
                p.low_latency = true;
                p.gpu_power = "3d".into();
                if p.usage == "auto" {
                    p.usage = "graphics".into();
                }
            }
            // IA: socket(s) da(s) GPU(s) de computação, memória preferindo a local.
            "ia" => {
                p.cpu = "gpu".into();
                p.memory = "prefer".into();
                p.gpu_power = "compute".into();
                if p.usage == "auto" {
                    p.usage = "compute".into();
                }
            }
            // Renderização/compilação: tudo, memória intercalada (banda dos dois sockets).
            "render" => {
                p.cpu = "all".into();
                p.memory = "interleave".into();
            }
            // Silencioso: poucos núcleos, prioridade baixa (não atrapalha o resto).
            "silencioso" => {
                p.cores = Some(4);
                p.priority = "low".into();
            }
            _ => {}
        }
        p
    }
}

#[derive(Debug, Default, Clone)]
pub struct Profiles {
    pub map: BTreeMap<String, Profile>,
}

impl Profiles {
    pub fn path() -> PathBuf {
        state_dir().join("profiles.json")
    }

    pub fn load() -> Profiles {
        let map = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Profiles { map }
    }

    pub fn get(&self, key: &str) -> Option<&Profile> {
        self.map.get(key)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = state_dir();
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join("profiles.json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&self.map).unwrap_or_default())?;
        std::fs::rename(tmp, Self::path())
    }
}

/// Chave do programa: jogos da Steam usam o AppId (todos os processos do jogo
/// ficam juntos); o resto usa a chave de aprendizado (executável / script).
pub fn program_key(steam_appid: Option<&str>, learn_key: &str) -> String {
    match steam_appid {
        Some(id) if !id.is_empty() && id != "0" => format!("steam:{id}"),
        _ => learn_key.to_string(),
    }
}
