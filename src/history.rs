//! Histórico de temperatura, potência, ventoinha e uso (gravado pelo daemon a cada 30 s, 24 h).

use crate::learned::run_dir;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;

pub const MAX_SAMPLES: usize = 2880;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sample {
    /// segundos desde 1970
    pub t: u64,
    /// por GPU: [temperatura °C, potência W, ventoinha %, uso %]
    pub g: Vec<[f32; 4]>,
    /// uso de cada nó (0..1)
    pub n: Vec<f32>,
    /// potência de cada socket (W)
    #[serde(default)]
    pub p: Vec<f32>,
    /// temperatura de cada socket (°C)
    #[serde(default)]
    pub ct: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct History {
    pub gpus: Vec<String>,
    pub nodes: Vec<usize>,
    pub samples: VecDeque<Sample>,
}

impl History {
    pub fn path() -> PathBuf {
        run_dir().join("history.json")
    }
    pub fn load() -> Option<History> {
        serde_json::from_str(&std::fs::read_to_string(Self::path()).ok()?).ok()
    }
    pub fn push(&mut self, s: Sample) {
        self.samples.push_back(s);
        while self.samples.len() > MAX_SAMPLES {
            self.samples.pop_front();
        }
    }
    pub fn save(&self) -> std::io::Result<()> {
        let tmp = run_dir().join("history.json.tmp");
        std::fs::write(&tmp, serde_json::to_string(self).unwrap_or_default())?;
        std::fs::rename(tmp, Self::path())
    }
    /// Amostras dos últimos `secs` segundos.
    pub fn since(&self, secs: u64) -> Vec<&Sample> {
        let last = self.samples.back().map(|s| s.t).unwrap_or(0);
        self.samples.iter().filter(|s| s.t + secs >= last).collect()
    }
}

/// Estado do serviço para o diagnóstico (/run/numapilot/status.json).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Status {
    pub version: String,
    pub root: bool,
    pub proc_connector: bool,
    pub cgroup_last_error: Option<String>,
    pub cgroup_ok_count: u64,
    pub od_errors: Vec<String>,
    pub reserved: String,
    pub started: u64,
}

pub fn status_path() -> PathBuf {
    run_dir().join("status.json")
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}
