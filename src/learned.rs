//! Memory of what automatic mode learned about each program.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn state_dir() -> PathBuf {
    std::env::var_os("NUMAPILOT_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/var/lib/numapilot"))
}

pub fn run_dir() -> PathBuf {
    std::env::var_os("NUMAPILOT_RUN_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/numapilot"))
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Entry {
    /// Got more performance using both sockets.
    pub wide: bool,
    /// Largest fraction of a node ever used.
    pub peak_frac: f64,
    pub runs: u32,
    /// Node of the GPU the program used.
    pub gpu_node: Option<usize>,
    pub last_seen: u64,
}

#[derive(Debug, Default)]
pub struct Learned {
    pub map: BTreeMap<String, Entry>,
    pub dirty: bool,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

impl Learned {
    pub fn path() -> PathBuf {
        state_dir().join("learned.json")
    }

    pub fn load() -> Learned {
        let map = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Learned { map, dirty: false }
    }

    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.map.get(key)
    }

    pub fn update(&mut self, key: &str, f: impl FnOnce(&mut Entry)) {
        let e = self.map.entry(key.to_string()).or_default();
        f(e);
        e.last_seen = now();
        self.dirty = true;
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let dir = state_dir();
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join("learned.json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(&self.map).unwrap_or_default(),
        )?;
        std::fs::rename(tmp, Self::path())?;
        self.dirty = false;
        Ok(())
    }
}
