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
}

fn same() -> String {
    "same".into()
}

impl Default for Profile {
    fn default() -> Self {
        Profile { label: String::new(), cpu: auto(), gpu: auto(), usage: auto(), fg_gpu: same() }
    }
}

impl Profile {
    pub fn is_auto(&self) -> bool {
        self.cpu == "auto" && self.gpu == "auto" && self.usage == "auto" && self.fg_gpu == "same"
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
