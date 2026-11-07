//! Vigia do lado do usuário (roda dentro do `numapilot web`): servidor de IA que caiu, outros PCs
//! que saíram da rede (avisos no celular) e o histórico de todos os PCs (7 dias, 1 ponto por
//! minuto) guardado neste PC.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    /// segundos desde 1970
    pub t: u64,
    pub name: String,
    pub online: bool,
    pub hot: Option<f64>,
    /// uso médio dos processadores (0-1)
    pub load: Option<f64>,
    pub power: Option<f64>,
}

pub fn history_path() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("numapilot/peers-history.jsonl")
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn read_history(since: u64) -> Vec<Point> {
    std::fs::read_to_string(history_path())
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Point>(l).ok())
        .filter(|p| p.t >= since)
        .collect()
}

/// Série de um PC para o gráfico: (minutos atrás, temperatura), em degraus de `step` segundos.
pub fn series(points: &[Point], name: &str, since: u64, step: u64) -> Vec<(u64, Option<f64>)> {
    let mut buckets: std::collections::BTreeMap<u64, f64> = Default::default();
    for p in points.iter().filter(|p| p.name == name && p.t >= since) {
        if let Some(h) = p.hot {
            let b = (p.t - since) / step;
            let e = buckets.entry(b).or_insert(h);
            *e = e.max(h);
        }
    }
    let n = (now().saturating_sub(since)) / step;
    (0..=n).map(|b| (b, buckets.get(&b).copied())).collect()
}

fn append(points: &[Point]) {
    let p = history_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
    {
        for x in points {
            let _ = writeln!(f, "{}", serde_json::to_string(x).unwrap_or_default());
        }
    }
}

fn trim(days: u64) {
    let keep = read_history(now().saturating_sub(days * 86400));
    let p = history_path();
    let tmp = p.with_extension("jsonl.tmp");
    let body: String = keep
        .iter()
        .map(|x| serde_json::to_string(x).unwrap_or_default() + "\n")
        .collect();
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(tmp, p);
    }
}

fn llm_state() -> Option<String> {
    let eng = crate::containers::engine()?;
    let name = crate::labcfg::LabSettings::load().get("LLM_NAME");
    let name = if name.is_empty() {
        "gfx906-llm".to_string()
    } else {
        name
    };
    let o = std::process::Command::new(eng)
        .args(["inspect", "-f", "{{.State.Status}}", &name])
        .output()
        .ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Laço do vigia (thread).
pub fn run() {
    let mut last_llm = llm_state();
    let mut offline: HashMap<String, u32> = HashMap::new();
    let mut tick: u64 = 0;
    trim(7);
    loop {
        std::thread::sleep(Duration::from_secs(60));
        tick += 1;
        // servidor de IA: estava rodando e parou sozinho (o "down" apaga o container: some)
        let st = llm_state();
        if last_llm.as_deref() == Some("running")
            && let Some(s) = st.as_deref()
            && s != "running"
        {
            crate::notify::event(
                "llm",
                "llm-down",
                "Servidor de IA parou",
                &format!(
                    "O container do llama.cpp está \"{s}\". Veja o log na aba Servidor de IA."
                ),
                4,
                Duration::from_secs(30 * 60),
            );
        }
        last_llm = st;
        // outros PCs
        let peers = crate::peers::summary();
        let t = now();
        let mut pts = vec![];
        for p in &peers {
            let name = p["name"].as_str().unwrap_or("").to_string();
            let online = p["online"].as_bool() == Some(true);
            let load = p["load"].as_array().and_then(|a| {
                let v: Vec<f64> = a.iter().filter_map(|x| x.as_f64()).collect();
                (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
            });
            pts.push(Point {
                t,
                name: name.clone(),
                online,
                hot: p["hot"].as_f64(),
                load,
                power: p["power"].as_f64(),
            });
            let c = offline.entry(name.clone()).or_default();
            if online {
                if *c >= 3 {
                    crate::notify::event(
                        "peers",
                        &format!("up-{name}"),
                        &format!("{name} voltou"),
                        &format!("{name} está ligado de novo."),
                        2,
                        Duration::from_secs(60),
                    );
                }
                *c = 0;
            } else {
                *c += 1;
                if *c == 3 {
                    crate::notify::event(
                        "peers",
                        &format!("down-{name}"),
                        &format!("{name} saiu da rede"),
                        &format!(
                            "{name} não responde há 3 minutos (desligou, travou ou perdeu a rede)."
                        ),
                        4,
                        Duration::from_secs(3600),
                    );
                }
            }
        }
        if !pts.is_empty() {
            append(&pts);
        }
        if tick.is_multiple_of(24 * 60) {
            trim(7);
        }
        // backup de cada PC uma vez por dia (o primeiro 10 min depois de ligar)
        if tick % (24 * 60) == 10 {
            for m in crate::peers::backup_all() {
                eprintln!("[numapilot] backup: {m}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets() {
        let base = now() - 3600;
        let pts = vec![
            Point {
                t: base + 10,
                name: "a".into(),
                online: true,
                hot: Some(50.0),
                load: None,
                power: None,
            },
            Point {
                t: base + 20,
                name: "a".into(),
                online: true,
                hot: Some(70.0),
                load: None,
                power: None,
            },
            Point {
                t: base + 700,
                name: "b".into(),
                online: true,
                hot: Some(90.0),
                load: None,
                power: None,
            },
        ];
        let s = series(&pts, "a", base, 300);
        assert_eq!(s[0].1, Some(70.0));
        assert_eq!(s[2].1, None);
        assert!(s.len() >= 12);
    }
}
