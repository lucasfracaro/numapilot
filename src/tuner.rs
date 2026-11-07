//! Otimizador de FPS: testa algumas configurações num jogo (uma por vez que ele é aberto),
//! mede com o MangoHud (FPS médio e 1% low) e mostra qual foi a melhor.

use crate::profiles::Profile;
use crate::topo::Topology;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Estado do usuário (o `numapilot run` roda como usuário, não como root).
pub fn user_state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("numapilot")
}

pub fn safe_key(key: &str) -> String {
    key.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn log_dir(key: &str) -> PathBuf {
    user_state_dir().join("mangologs").join(safe_key(key))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FpsResult {
    pub avg: f64,
    /// FPS do 1% de quadros mais lentos.
    pub low1: f64,
    pub low01: f64,
    pub frames: usize,
    pub seconds: f64,
}

impl FpsResult {
    /// Nota usada para escolher a melhor: valoriza o 1% low (travadinhas) mais que a média.
    pub fn score(&self) -> f64 {
        0.4 * self.avg + 0.6 * self.low1
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variant {
    pub label: String,
    pub profile: Profile,
    #[serde(default)]
    pub result: Option<FpsResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub key: String,
    pub label: String,
    pub variants: Vec<Variant>,
    pub next: usize,
    pub started: u64,
    /// Quanto tempo medir em cada abertura (s).
    #[serde(default = "dur")]
    pub seconds: u32,
}

fn dur() -> u32 {
    60
}

impl Session {
    pub fn current(&self) -> Option<&Variant> {
        self.variants.get(self.next)
    }
    pub fn done(&self) -> bool {
        self.next >= self.variants.len()
    }
    pub fn best(&self) -> Option<usize> {
        self.variants
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.result.as_ref().map(|r| (i, r.score())))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Tuner {
    pub sessions: BTreeMap<String, Session>,
}

impl Tuner {
    pub fn path() -> PathBuf {
        user_state_dir().join("tuner.json")
    }
    pub fn load() -> Tuner {
        std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(user_state_dir())?;
        let tmp = Self::path().with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).unwrap_or_default())?;
        std::fs::rename(tmp, Self::path())
    }
    pub fn start(&mut self, key: &str, label: &str, variants: Vec<Variant>) {
        let _ = std::fs::remove_dir_all(log_dir(key));
        self.sessions.insert(
            key.to_string(),
            Session {
                key: key.into(),
                label: label.into(),
                variants,
                next: 0,
                started: crate::history::now(),
                seconds: 60,
            },
        );
    }
}

/// Configurações a testar, a partir do perfil atual do programa.
pub fn variants(topo: &Topology, base: &Profile) -> Vec<Variant> {
    let mut b = base.clone();
    if b.usage == "auto" {
        b.usage = "graphics".into();
    }
    let gpu_node_cores = {
        let gn = topo
            .gpus
            .iter()
            .find(|g| g.alias == b.gpu || g.pci == b.gpu)
            .or_else(|| topo.display_gpu())
            .and_then(|g| g.numa_node);
        topo.nodes
            .iter()
            .find(|n| Some(n.id) == gn)
            .or(topo.nodes.first())
            .map(|n| n.physical_cores)
            .unwrap_or(8)
    };
    let mk = |label: String, cpu: &str, cores: Option<usize>, smt: bool, exclusive: bool| Variant {
        label,
        profile: Profile {
            cpu: cpu.into(),
            cores,
            smt,
            cpu_list: None,
            memory: "strict".into(),
            priority: "high".into(),
            exclusive,
            limit_visible: true,
            ..b.clone()
        },
        result: None,
    };
    let mut v = Vec::new();
    let p = gpu_node_cores;
    let c16 = p.min(16);
    v.push(mk(
        format!("{c16} núcleos sem HT (socket da GPU)"),
        "gpu",
        Some(c16),
        false,
        true,
    ));
    if p > 8 {
        v.push(mk(
            "8 núcleos sem HT (socket da GPU)".into(),
            "gpu",
            Some(8),
            false,
            true,
        ));
    }
    v.push(mk(
        format!("{} núcleos com HT ({} threads)", c16.min(8), c16.min(8) * 2),
        "gpu",
        Some(c16.min(8) * 2),
        true,
        true,
    ));
    if p > 16 {
        v.push(mk(
            format!("socket da GPU inteiro sem HT ({p} núcleos)"),
            "gpu",
            None,
            false,
            true,
        ));
    }
    if topo.nodes.len() > 1 {
        let mut w = mk(
            "os dois sockets sem HT (comparação)".into(),
            "all",
            Some(32.min(p * topo.nodes.len())),
            false,
            false,
        );
        w.profile.memory = "auto".into();
        v.push(w);
    }
    // GPUs alternativas (mesma CPU que a primeira configuração).
    if b.usage == "graphics" {
        for g in &topo.gpus {
            if g.alias == b.gpu || g.render.is_none() {
                continue;
            }
            let mut x = mk(
                format!("{c16} núcleos sem HT na {}", g.alias),
                "gpu",
                Some(c16),
                false,
                true,
            );
            x.profile.gpu = g.alias.clone();
            v.push(x);
        }
    }
    v
}

/// Variáveis para o MangoHud gravar um CSV com o tempo de cada quadro.
pub fn mangohud_env(key: &str, seconds: u32) -> Vec<(String, String)> {
    let dir = log_dir(key);
    let _ = std::fs::create_dir_all(&dir);
    let mine = format!(
        "autostart_log=10,log_duration={seconds},log_interval=0,output_folder={},log_versioning=0",
        dir.display()
    );
    let cfg = match std::env::var("MANGOHUD_CONFIG") {
        Ok(c) if !c.trim().is_empty() => format!("{c},{mine}"),
        _ => mine,
    };
    vec![
        ("MANGOHUD".into(), "1".into()),
        ("MANGOHUD_CONFIG".into(), cfg),
    ]
}

pub fn newest_csv(dir: &Path, since: u64) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            e.path().extension().is_some_and(|x| x == "csv")
                && !e.file_name().to_string_lossy().ends_with("_summary.csv")
        })
        .filter_map(|e| {
            let m = e.metadata().ok()?.modified().ok()?;
            let t = m.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
            (t >= since).then_some((t, e.path()))
        })
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
}

/// Lê o CSV do MangoHud (linhas de sistema, depois o cabeçalho "fps,frametime,...").
pub fn parse_csv(text: &str) -> Option<FpsResult> {
    let mut lines = text.lines();
    let header = lines
        .by_ref()
        .find(|l| l.starts_with("fps,") || l.contains(",frametime"))?;
    let cols: Vec<&str> = header.split(',').map(str::trim).collect();
    let fi = cols.iter().position(|c| *c == "frametime")?;
    let mut ft: Vec<f64> = lines
        .filter_map(|l| l.split(',').nth(fi)?.trim().parse::<f64>().ok())
        .filter(|v| *v > 0.0 && *v < 5000.0)
        .collect();
    if ft.len() < 30 {
        return None;
    }
    let total_ms: f64 = ft.iter().sum();
    let avg = ft.len() as f64 * 1000.0 / total_ms;
    ft.sort_by(|a, b| b.total_cmp(a)); // mais lentos primeiro
    let low = |frac: f64| {
        let n = ((ft.len() as f64 * frac).ceil() as usize).max(1);
        let m: f64 = ft[..n].iter().sum::<f64>() / n as f64;
        1000.0 / m
    };
    Some(FpsResult {
        avg,
        low1: low(0.01),
        low01: low(0.001),
        frames: ft.len(),
        seconds: total_ms / 1000.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv() {
        let mut s = String::from(
            "os,cpu,gpu,ram,kernel,driver,cpuscheduler\nArch,Xeon,MI50,128,6.12,mesa,\nfps,frametime,cpu_load,gpu_load,elapsed\n",
        );
        for i in 0..990 {
            s.push_str(&format!("60,{:.3},10,90,{i}\n", 1000.0 / 60.0));
        }
        for i in 0..10 {
            s.push_str(&format!("20,50.0,10,90,{i}\n"));
        }
        let r = parse_csv(&s).unwrap();
        assert_eq!(r.frames, 1000);
        assert!((r.low1 - 20.0).abs() < 0.01, "{r:?}");
        assert!(r.avg > 55.0 && r.avg < 60.0, "{r:?}");
        assert!(parse_csv("lixo").is_none());
    }

    #[test]
    fn best_by_score() {
        let mut s = Session {
            key: "k".into(),
            label: "x".into(),
            variants: vec![],
            next: 0,
            started: 0,
            seconds: 60,
        };
        for (a, l) in [(70.0, 30.0), (62.0, 50.0), (80.0, 20.0)] {
            s.variants.push(Variant {
                label: String::new(),
                profile: Profile::default(),
                result: Some(FpsResult {
                    avg: a,
                    low1: l,
                    low01: l,
                    frames: 100,
                    seconds: 1.0,
                }),
            });
        }
        assert_eq!(s.best(), Some(1));
        assert_eq!(safe_key("steam:1245620"), "steam_1245620");
    }
}
