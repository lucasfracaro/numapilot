//! Informações atualizadas em segundo plano (containers, servidor de IA, Sunshine, atualizações),
//! para a interface nunca travar esperando um comando.

use crate::jobs::quick;
use numapilot::containers::{self, Apps, Container};
use numapilot::labcfg::{self, LabSettings};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default, Clone)]
pub struct Sunshine {
    /// "sistema" | "flatpak" | None (não instalado)
    pub install: Option<String>,
    pub unit: Option<String>,
    pub active: bool,
    pub conf: Option<PathBuf>,
}

#[derive(Default)]
pub struct Bg {
    pub containers: Option<Result<Vec<Container>, String>>,
    pub apps: HashMap<String, Apps>,
    pub inspecting: HashSet<String>,
    /// None = servidor parado; Some(false) = carregando; Some(true) = pronto
    pub llm: Option<bool>,
    pub sunshine: Sunshine,
    /// render node → codecs de codificação (VA-API)
    pub vaapi: HashMap<String, String>,
    pub autoupdate: bool,
    pub last_update: Option<serde_json::Value>,
    pub daemon_ok: bool,
}

pub type Shared = Arc<Mutex<Bg>>;

fn http_ok(port: &str) -> Option<bool> {
    let addr = format!("127.0.0.1:{port}").parse().ok()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(300)).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(800))).ok()?;
    s.write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n").ok()?;
    let mut b = String::new();
    let _ = s.read_to_string(&mut b);
    Some(b.contains("\"ok\""))
}

pub fn sunshine_info() -> Sunshine {
    let mut s = Sunshine::default();
    if containers::which("sunshine") {
        s.install = Some("sistema".into());
    } else if containers::which("flatpak")
        && std::process::Command::new("flatpak")
            .args(["info", "dev.lizardbyte.app.Sunshine"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|x| x.success())
    {
        s.install = Some("flatpak".into());
    }
    for u in ["sunshine.service", "app-dev.lizardbyte.app.Sunshine.service"] {
        let out = quick(&["systemctl", "--user", "show", "-p", "LoadState,ActiveState", u]);
        if out.contains("LoadState=loaded") {
            s.unit = Some(u.to_string());
            s.active = out.contains("ActiveState=active");
            break;
        }
    }
    if s.unit.is_none() {
        s.active = !quick(&["pgrep", "-x", "sunshine"]).is_empty();
    }
    let h = labcfg::home();
    let a = h.join(".config/sunshine/sunshine.conf");
    let b = h.join(".var/app/dev.lizardbyte.app.Sunshine/config/sunshine/sunshine.conf");
    s.conf = Some(if s.install.as_deref() == Some("flatpak") { b } else { a });
    s
}

/// Codecs que a GPU consegue codificar (para streaming), via `vainfo`.
pub fn vaapi_encoders(render: &str) -> String {
    if !containers::which("vainfo") {
        return "instale libva-utils para verificar".into();
    }
    let out = std::process::Command::new("vainfo")
        .args(["--display", "drm", "--device", &format!("/dev/dri/{render}")])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let mut c = Vec::new();
    for (key, name) in [("H264", "H.264"), ("HEVC", "HEVC"), ("AV1", "AV1")] {
        if out.lines().any(|l| l.contains(key) && l.contains("VAEntrypointEncSlice")) {
            c.push(name);
        }
    }
    if c.is_empty() { "sem codificação de vídeo".into() } else { c.join(", ") }
}

pub fn start(ctx: eframe::egui::Context, renders: Vec<String>) -> Shared {
    let shared: Shared = Arc::new(Mutex::new(Bg::default()));
    let sh = shared.clone();
    std::thread::spawn(move || {
        let mut first = true;
        let mut last_vaapi = Instant::now();
        loop {
            let st = LabSettings::load();
            let list = containers::list(&st.get("LAB_NAME"), &st.get("LLM_NAME"));
            let llm_running = list
                .as_ref()
                .map(|v| v.iter().any(|c| c.name == st.get("LLM_NAME") && c.running))
                .unwrap_or(false);
            let llm = if llm_running { Some(http_ok(&st.get("LLM_PORT")).unwrap_or(false)) } else { None };
            let sun = sunshine_info();
            let auto = quick(&["systemctl", "--user", "is-enabled", "gfx906-update.timer"]) == "enabled";
            let last = std::fs::read_to_string(labcfg::state_dir().join("last-update.json"))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());
            let daemon_ok = std::fs::metadata(numapilot::daemon::state_path())
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
                .is_some_and(|d| d < Duration::from_secs(5));
            let vaapi = if first || last_vaapi.elapsed() > Duration::from_secs(600) {
                last_vaapi = Instant::now();
                Some(renders.iter().map(|r| (r.clone(), vaapi_encoders(r))).collect::<HashMap<_, _>>())
            } else {
                None
            };
            if let Ok(mut b) = sh.lock() {
                b.containers = Some(list);
                b.llm = llm;
                b.sunshine = sun;
                b.autoupdate = auto;
                b.last_update = last;
                b.daemon_ok = daemon_ok;
                if let Some(v) = vaapi {
                    b.vaapi = v;
                }
            }
            first = false;
            ctx.request_repaint();
            std::thread::sleep(Duration::from_secs(3));
        }
    });
    shared
}

/// Descobre os apps de um container (em segundo plano).
pub fn inspect(shared: &Shared, c: Container, ctx: eframe::egui::Context) {
    if let Ok(mut b) = shared.lock()
        && !b.inspecting.insert(c.id.clone()) {
            return;
        }
    let sh = shared.clone();
    std::thread::spawn(move || {
        let apps = containers::inspect_apps(&c);
        if let Ok(mut b) = sh.lock() {
            b.inspecting.remove(&c.id);
            b.apps.insert(c.id.clone(), apps);
        }
        ctx.request_repaint();
    });
}
