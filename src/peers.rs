//! Other PCs with numapilot: each one runs the web panel (`numapilot web`); here we keep each
//! one's link so the phone panel shows all the machines together.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    pub name: String,
    /// http://ip:port
    pub url: String,
    pub token: String,
    /// MAC of the network card with Wake-on-LAN (learned while the PC is on).
    #[serde(default)]
    pub mac: Option<String>,
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

/// "http://192.168.0.10:8577/?t=abc" → (base url, token)
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
    let (url, token) = parse_link(link).ok_or(crate::i18n::tr(
        "invalid link: copy the link with ?t= from the other PC's panel",
    ))?;
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
    let mut p = Peer {
        name,
        url,
        token,
        mac: None,
    };
    p.mac = fetch(&p).and_then(|s| wol_mac(&s));
    v.push(p.clone());
    save(&v).map_err(|e| e.to_string())?;
    Ok(p)
}

pub fn remove(name: &str) -> Result<(), String> {
    let mut v = load();
    let n = v.len();
    v.retain(|p| p.name != name);
    if v.len() == n {
        return Err(crate::tf!(
            "PC {name} is not in the list",
            "PC {name} não está na lista"
        ));
    }
    save(&v).map_err(|e| e.to_string())
}

/// State of another PC (None if it did not answer within 2 s).
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

fn get(p: &Peer, path: &str, out: Option<&std::path::Path>, secs: u32) -> Result<Vec<u8>, String> {
    let mut c = std::process::Command::new("curl");
    c.args([
        "-fsS",
        "--noproxy",
        "*",
        "--max-time",
        &secs.to_string(),
        "-H",
    ])
    .arg(format!("X-Token: {}", p.token));
    if let Some(o) = out {
        c.arg("-o").arg(o);
    }
    let o = c
        .arg(format!("{}{path}", p.url))
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if o.status.success() {
        Ok(o.stdout)
    } else {
        Err(format!(
            "{}: {}",
            p.name,
            String::from_utf8_lossy(&o.stderr).trim()
        ))
    }
}

/// Copies the program profiles from another PC (the ones this one lacks; with `all`, all).
/// Steam game profiles (steam:APPID) work on any PC.
pub fn pull_profiles(name: &str, all: bool) -> Result<String, String> {
    let p = load().into_iter().find(|p| p.name == name).ok_or_else(|| {
        crate::tf!(
            "PC {name} is not in the list",
            "PC {name} não está na lista"
        )
    })?;
    let body = get(&p, "/api/profiles", None, 10)?;
    let theirs: std::collections::BTreeMap<String, crate::profiles::Profile> =
        serde_json::from_slice(&body)
            .map_err(|e| crate::tf!("invalid response: {e}", "resposta inválida: {e}"))?;
    let mine = crate::profiles::Profiles::load();
    let mut n = 0;
    for (k, prof) in theirs {
        if !all && mine.get(&k).is_some() {
            continue;
        }
        let r = crate::ctl::send(&crate::ctl::Request::SetProfile {
            key: k.clone(),
            profile: prof,
            pid: None,
        })
        .map_err(|e| crate::tf!("numapilot service: {e}", "serviço numapilot: {e}"))?;
        if r.ok {
            n += 1;
        }
    }
    Ok(crate::tf!(
        "{n} profile(s) copied from {name}",
        "{n} perfil(s) copiado(s) de {name}"
    ))
}

pub fn backup_dir() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "/tmp".into())
        .join("numapilot-backups")
}

/// Stores the backup of each PC that is on in ~/numapilot-backups/NAME/YYYY-MM-DD.tar.gz (7 newest).
pub fn backup_all() -> Vec<String> {
    let mut msgs = vec![];
    for p in load() {
        let dir = backup_dir().join(&p.name);
        if std::fs::create_dir_all(&dir).is_err() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(backup_dir(), std::fs::Permissions::from_mode(0o700));
        }
        let f = dir.join(format!("{}.tar.gz", crate::energy::today()));
        match get(&p, "/api/backup", Some(&f), 120) {
            Ok(_) => msgs.push(format!("{}: {}", p.name, f.display())),
            Err(e) => {
                let _ = std::fs::remove_file(&f);
                msgs.push(e);
                continue;
            }
        }
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|x| x.extension().is_some_and(|e| e == "gz"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        while files.len() > 7 {
            let _ = std::fs::remove_file(files.remove(0));
        }
    }
    msgs
}

/// MAC with Wake-on-LAN enabled in the other PC's state.
fn wol_mac(s: &serde_json::Value) -> Option<String> {
    s["wol"]
        .as_array()?
        .iter()
        .find(|n| n["wol"].as_str() == Some("g"))
        .and_then(|n| n["mac"].as_str())
        .map(String::from)
}

/// Wakes the PC over the network (needs the MAC learned while it was on and WoL enabled on it).
pub fn wake(name: &str) -> Result<String, String> {
    let p = load().into_iter().find(|p| p.name == name).ok_or_else(|| {
        crate::tf!(
            "PC {name} is not in the list",
            "PC {name} não está na lista"
        )
    })?;
    let mac = p.mac.ok_or(crate::i18n::tr("I do not know its address yet: enable Wake-on-LAN there (Machine → Network) and open this panel once with it on"))?;
    let ip = p
        .url
        .split("//")
        .nth(1)
        .and_then(|h| h.split(':').next())
        .map(String::from);
    crate::wol::wake(&mac, ip.as_deref())?;
    Ok(crate::tf!(
        "wake packet for {name} sent ({mac})",
        "pacote para ligar {name} enviado ({mac})"
    ))
}

/// Summary of each PC for the panel: name, link, online, hottest GPU, processor usage.
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
                v["power"] = s["watts"].clone();
                v["kwh_today"] = s["kwh_today"].clone();
                v["version"] = s["version"].clone();
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
