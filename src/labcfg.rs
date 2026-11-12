//! Ajustes do usuário para o laboratório e o servidor de IA
//! (~/.config/gfx906-lab/settings.env, lido também pelo script gfx906-rocm).

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
    ("UPDATE_MODE", "auto"),
    ("REGRESS_PCT", "10"),
];

/// Lê KEY="valor" (o formato que o script também lê). Ignora o resto.
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

    /// Grava só as chaves que o usuário mexeu pela interface.
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

/// Modelos .gguf (só a 1ª parte dos arquivos divididos), com tamanho total em bytes.
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
