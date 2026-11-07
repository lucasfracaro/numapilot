//! Outros PCs com o numapilot: cada um roda o painel web (`numapilot web`); aqui guardamos o
//! link de cada um para o painel do celular mostrar todas as máquinas juntas.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    pub name: String,
    /// http://ip:porta
    pub url: String,
    pub token: String,
}

impl Peer {
    pub fn link(&self) -> String {
        format!("{}/?t={}", self.url, self.token)
    }
}

fn path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("numapilot/peers.json")
}

pub fn load() -> Vec<Peer> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(v: &[Peer]) -> std::io::Result<()> {
    if let Some(d) = path().parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path(), serde_json::to_string_pretty(v).unwrap_or_default())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path(), std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// "http://192.168.0.10:8577/?t=abc" → (url base, token)
pub fn parse_link(link: &str) -> Option<(String, String)> {
    let l = link.trim();
    if !(l.starts_with("http://") || l.starts_with("https://")) {
        return None;
    }
    let (base, query) = l.split_once('?')?;
    let token = query
        .split('&')
        .find_map(|kv| kv.strip_prefix("t="))?
        .to_string();
    if token.len() < 16 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some((base.trim_end_matches('/').to_string(), token))
}

pub fn add(link: &str, name: Option<&str>) -> Result<Peer, String> {
    let (url, token) =
        parse_link(link).ok_or("link inválido: copie o link com ?t= do painel do outro PC")?;
    let name = name
        .map(String::from)
        .or_else(|| {
            url.split("//")
                .nth(1)
                .map(|h| h.split(':').next().unwrap_or(h).to_string())
        })
        .unwrap_or_else(|| url.clone());
    let mut v = load();
    v.retain(|p| p.url != url && p.name != name);
    let p = Peer { name, url, token };
    v.push(p.clone());
    save(&v).map_err(|e| e.to_string())?;
    Ok(p)
}

pub fn remove(name: &str) -> Result<(), String> {
    let mut v = load();
    let n = v.len();
    v.retain(|p| p.name != name);
    if v.len() == n {
        return Err(format!("PC {name} não está na lista"));
    }
    save(&v).map_err(|e| e.to_string())
}

/// Estado de outro PC (None se não respondeu em 2 s).
pub fn fetch(p: &Peer) -> Option<serde_json::Value> {
    let o = std::process::Command::new("curl")
        .args(["-fsS", "--noproxy", "*", "--max-time", "2", "-H"])
        .arg(format!("X-Token: {}", p.token))
        .arg(format!("{}/api/state", p.url))
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| serde_json::from_slice(&o.stdout).ok())
        .flatten()
}

/// Resumo de cada PC para o painel: nome, link, online, GPU mais quente, uso dos processadores.
pub fn summary() -> Vec<serde_json::Value> {
    let peers = load();
    let handles: Vec<_> = peers
        .into_iter()
        .map(|p| std::thread::spawn(move || (fetch(&p), p)))
        .collect();
    handles
        .into_iter()
        .filter_map(|h| h.join().ok())
        .map(|(st, p)| {
            let mut v =
                serde_json::json!({"name": p.name, "link": p.link(), "online": st.is_some()});
            if let Some(s) = st {
                let hot = s["gpus"]
                    .as_array()
                    .and_then(|g| g.iter().filter_map(|x| x["temp"].as_f64()).reduce(f64::max));
                let load: Vec<f64> = s["nodes"]
                    .as_array()
                    .map(|n| n.iter().filter_map(|x| x["load"].as_f64()).collect())
                    .unwrap_or_default();
                v["host"] = s["host"].clone();
                v["daemon"] = s["daemon"].clone();
                v["hot"] = serde_json::json!(hot);
                v["load"] = serde_json::json!(load);
                v["gpus"] = serde_json::json!(s["gpus"].as_array().map(|g| g.len()).unwrap_or(0));
                v["programs"] =
                    serde_json::json!(s["programs"].as_array().map(|g| g.len()).unwrap_or(0));
            }
            v
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        assert_eq!(
            parse_link("http://192.168.0.10:8577/?t=0123456789abcdef0123456789abcdef"),
            Some((
                "http://192.168.0.10:8577".into(),
                "0123456789abcdef0123456789abcdef".into()
            ))
        );
        assert!(parse_link("192.168.0.10:8577").is_none());
        assert!(parse_link("http://x/?t=curto").is_none());
    }
}
