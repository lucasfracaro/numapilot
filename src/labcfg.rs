//! User settings for the lab and the AI server
//! (~/.config/gfx906-lab/settings.env, also read by the gfx906-rocm script).

use std::collections::BTreeMap;
use std::path::PathBuf;

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn cfg_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("gfx906-lab")
}

pub fn state_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/state"))
        .join("gfx906-lab")
}

#[derive(Debug, Clone)]
pub struct LabSettings {
    pub values: BTreeMap<String, String>,
}

const DEFAULTS: &[(&str, &str)] = &[
    ("LAB_NAME", "gfx906-lab"),
    ("LAB_BIND", "127.0.0.1"),
    ("LAB_PORT_JUPYTER", "8888"),
    ("LAB_PORT_CODE", "8443"),
    ("LAB_PORT_SSH", "2222"),
    ("LLM_NAME", "gfx906-llm"),
    ("LLM_MODELS", "$HOME/models"),
    ("LLM_MODEL", ""),
    ("LLM_GPU", "compute:all"),
    ("LLM_CTX", "16384"),
    ("LLM_PORT", "8080"),
    ("LLM_ARGS", ""),
    ("LLM_AUTOSTART", "1"),
    ("LLM_IMAGE", ""),
    ("UPDATE_MODE", "auto"),
    ("REGRESS_PCT", "10"),
];

/// Reads KEY="value" (the format the script also reads). Ignores the rest.
pub fn parse_env(s: &str) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for l in s.lines() {
        let l = l.trim();
        if l.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            let k = k.trim();
            if k.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && !k.is_empty()
            {
                let v = v
                    .split(" #")
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_matches('"')
                    .to_string();
                m.insert(k.to_string(), v);
            }
        }
    }
    m
}

impl LabSettings {
    pub fn path() -> PathBuf {
        cfg_dir().join("settings.env")
    }

    pub fn load() -> LabSettings {
        let mut values: BTreeMap<String, String> = DEFAULTS
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        if let Ok(s) = std::fs::read_to_string("/etc/gfx906-rocm.conf") {
            values.extend(parse_env(&s));
        }
        if let Ok(s) = std::fs::read_to_string(Self::path()) {
            values.extend(parse_env(&s));
        }
        LabSettings { values }
    }

    pub fn get(&self, k: &str) -> String {
        let v = self.values.get(k).cloned().unwrap_or_default();
        v.replace("$HOME", &home().to_string_lossy())
    }

    pub fn set(&mut self, k: &str, v: &str) {
        self.values.insert(k.to_string(), v.to_string());
    }

    /// Writes only the keys the user changed through the interface.
    pub fn save_keys(&self, keys: &[&str]) -> std::io::Result<()> {
        let mut cur = std::fs::read_to_string(Self::path())
            .map(|s| parse_env(&s))
            .unwrap_or_default();
        for k in keys {
            cur.insert(
                k.to_string(),
                self.values.get(*k).cloned().unwrap_or_default(),
            );
        }
        std::fs::create_dir_all(cfg_dir())?;
        let mut out = String::from(
            "# Settings made by numapilot (graphical interface). Read by gfx906-rocm.\n",
        );
        for (k, v) in cur {
            out.push_str(&format!("{k}=\"{}\"\n", v.replace('"', "")));
        }
        std::fs::write(Self::path(), out)
    }

    pub fn token(&self) -> String {
        std::fs::read_to_string(cfg_dir().join("token"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    }

    pub fn url_host(&self, key: &str) -> String {
        let h = self.get(key);
        if h == "0.0.0.0" {
            "127.0.0.1".into()
        } else {
            h
        }
    }
}

/// A saved AI model: one button in the AI server tab that starts llama.cpp with these settings
/// (~/.config/gfx906-lab/llm-presets.toml).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LlmPreset {
    pub name: String,
    pub model: String,
    #[serde(default)]
    pub gpu: String,
    #[serde(default)]
    pub ctx: u32,
    #[serde(default)]
    pub args: String,
    /// llama.cpp container image for this model (another build or fork); empty = the default one.
    #[serde(default)]
    pub image: String,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct PresetFile {
    #[serde(default, rename = "preset")]
    presets: Vec<LlmPreset>,
}

impl LlmPreset {
    /// The model, cards, context and extra arguments currently in the form.
    pub fn from_settings(name: &str, cfg: &LabSettings) -> LlmPreset {
        LlmPreset {
            name: name.trim().to_string(),
            model: cfg.get("LLM_MODEL"),
            gpu: cfg.get("LLM_GPU"),
            ctx: cfg.get("LLM_CTX").parse().unwrap_or(16384),
            args: cfg.get("LLM_ARGS"),
            image: cfg.get("LLM_IMAGE"),
        }
    }

    /// Puts the preset into the settings (the script reads them on `llm up`).
    pub fn apply(&self, cfg: &mut LabSettings) {
        cfg.set("LLM_MODEL", &self.model);
        if !self.gpu.is_empty() {
            cfg.set("LLM_GPU", &self.gpu);
        }
        if self.ctx > 0 {
            cfg.set("LLM_CTX", &self.ctx.to_string());
        }
        cfg.set("LLM_ARGS", &self.args);
        cfg.set("LLM_IMAGE", &self.image);
    }
}

pub fn presets_path() -> PathBuf {
    cfg_dir().join("llm-presets.toml")
}

pub fn presets_from_str(s: &str) -> Vec<LlmPreset> {
    toml::from_str::<PresetFile>(s)
        .map(|f| f.presets)
        .unwrap_or_default()
        .into_iter()
        .filter(|p| !p.name.is_empty() && !p.model.is_empty())
        .collect()
}

pub fn presets_to_string(list: &[LlmPreset]) -> String {
    let f = PresetFile {
        presets: list.to_vec(),
    };
    format!(
        "# Saved AI models (numapilot, AI server tab).\n{}",
        toml::to_string(&f).unwrap_or_default()
    )
}

pub fn load_presets() -> Vec<LlmPreset> {
    std::fs::read_to_string(presets_path())
        .map(|s| presets_from_str(&s))
        .unwrap_or_default()
}

pub fn save_presets(list: &[LlmPreset]) -> std::io::Result<()> {
    std::fs::create_dir_all(cfg_dir())?;
    std::fs::write(presets_path(), presets_to_string(list))
}

/// Adds the preset, or replaces the one with the same name (ignoring case).
pub fn upsert_preset(list: &mut Vec<LlmPreset>, p: LlmPreset) {
    match list
        .iter_mut()
        .find(|x| x.name.eq_ignore_ascii_case(&p.name))
    {
        Some(x) => *x = p,
        None => list.push(p),
    }
}

/// .gguf models (only the 1st part of split files), with total size in bytes.
pub fn list_models(dir: &std::path::Path) -> Vec<(String, u64)> {
    fn walk(
        base: &std::path::Path,
        d: &std::path::Path,
        out: &mut Vec<(String, u64)>,
        depth: usize,
    ) {
        if depth > 4 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(d) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(base, &p, out, depth + 1);
            } else if p.extension().and_then(|x| x.to_str()) == Some("gguf") {
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if let Some(i) = name.find("-of-")
                    && name.get(i.saturating_sub(5)..i) != Some("00001")
                {
                    continue;
                }
                let size = p.metadata().map(|m| m.len()).unwrap_or(0);
                let rel = p
                    .strip_prefix(base)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .to_string();
                out.push((rel, size));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out, 0);
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_parsing() {
        let m = parse_env(
            "# x\nLLM_PORT=8081\nLLM_ARGS=\"--flash-attn on\"  # comentário\nfoo=bar\nLAB_BIND=\"0.0.0.0\"\n",
        );
        assert_eq!(m.get("LLM_PORT").map(String::as_str), Some("8081"));
        assert_eq!(
            m.get("LLM_ARGS").map(String::as_str),
            Some("--flash-attn on")
        );
        assert_eq!(m.get("LAB_BIND").map(String::as_str), Some("0.0.0.0"));
        assert!(!m.contains_key("foo"));
    }

    #[test]
    fn presets_round_trip() {
        let mut l = Vec::new();
        upsert_preset(
            &mut l,
            LlmPreset {
                name: "Qwen 256k".into(),
                model: "q/a.gguf".into(),
                gpu: "compute:all".into(),
                ctx: 262144,
                args: "--flash-attn on".into(),
                image: "docker.io/x/llama:1".into(),
            },
        );
        upsert_preset(
            &mut l,
            LlmPreset {
                name: "small".into(),
                model: "b.gguf".into(),
                ..Default::default()
            },
        );
        upsert_preset(
            &mut l,
            LlmPreset {
                name: "qwen 256K".into(),
                model: "q/a.gguf".into(),
                ctx: 131072,
                ..Default::default()
            },
        );
        assert_eq!(l.len(), 2);
        let back = presets_from_str(&presets_to_string(&l));
        assert_eq!(back, l);
        assert_eq!(back[0].ctx, 131072);
        assert!(presets_from_str("not toml [").is_empty());
        let mut cfg = LabSettings {
            values: BTreeMap::new(),
        };
        back[1].apply(&mut cfg);
        assert_eq!(cfg.get("LLM_MODEL"), "b.gguf");
        assert!(!cfg.values.contains_key("LLM_CTX"));
        let p = LlmPreset::from_settings(" x ", &cfg);
        assert_eq!((p.name.as_str(), p.ctx), ("x", 16384));
    }

    #[test]
    fn models_skip_shards() {
        let d = std::env::temp_dir().join("numapilot-models-test");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("q")).unwrap();
        for f in [
            "a.gguf",
            "q/big-00001-of-00003.gguf",
            "q/big-00002-of-00003.gguf",
            "q/readme.txt",
        ] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        let m: Vec<String> = list_models(&d).into_iter().map(|x| x.0).collect();
        assert_eq!(m, vec!["a.gguf", "q/big-00001-of-00003.gguf"]);
    }
}
