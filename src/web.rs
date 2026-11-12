//! Painel web para o celular: temperaturas, uso, programas (com perfis de um toque) e atalhos
//! do laboratório/servidor de IA. Roda como usuário (`numapilot web`), protegido por um token.

use crate::config::Config;
use crate::ctl::{self, Request};
use crate::profiles::{Profile, Profiles};
use crate::topo::Topology;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 8577;

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("numapilot")
}

pub fn token_path() -> PathBuf {
    config_dir().join("web-token")
}

/// Lê (ou cria) o token de acesso.
pub fn token() -> std::io::Result<String> {
    if let Ok(t) = std::fs::read_to_string(token_path()) {
        let t = t.trim().to_string();
        if t.len() >= 16 {
            return Ok(t);
        }
    }
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    let t: String = b.iter().map(|x| format!("{x:02x}")).collect();
    std::fs::create_dir_all(config_dir())?;
    // já nasce legível só pelo dono (antes havia um instante com a permissão padrão)
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let _ = std::fs::remove_file(token_path());
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(token_path())?;
        f.write_all(t.as_bytes())?;
        let _ = std::fs::set_permissions(token_path(), std::fs::Permissions::from_mode(0o600));
    }
    Ok(t)
}

/// IP da máquina na rede local (o que o celular usa).
pub fn lan_ip() -> Option<String> {
    let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?; // não envia nada; só escolhe a interface
    Some(s.local_addr().ok()?.ip().to_string())
}

pub fn url(port: u16) -> Option<String> {
    Some(format!("http://{}:{port}/?t={}", lan_ip()?, token().ok()?))
}

fn gfx906() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("gfx906-rocm")))
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "gfx906-rocm".into())
}

pub fn state(topo: &Topology) -> Value {
    let gpus: Vec<Value> = topo
        .gpus
        .iter()
        .map(|g| {
            let s = crate::gpuctl::read_sensors(&g.pci);
            let t = s.control_temp();
            json!({
                "alias": g.alias, "name": g.name, "node": g.numa_node,
                "temp": t, "hot": t.is_some_and(|t| t >= 90.0),
                "power": s.power_w, "clock": s.clock_mhz, "cap": s.power_cap_w, "fan": s.fan_pct, "rpm": s.fan_rpm,
                "busy": g.busy_percent(),
                "vram": g.vram_used_total().map(|(u, t)| [u as f64 / 1073741824.0, t as f64 / 1073741824.0]),
            })
        })
        .collect();
    let loads = crate::decide::sample_node_loads(topo, std::time::Duration::from_millis(200));
    let nodes: Vec<Value> = topo
        .nodes
        .iter()
        .map(|n| {
            json!({"id": n.id, "cpus": n.cpus.len(), "load": loads.get(&n.id).copied().unwrap_or(0.0),
                   "mem_free_gb": n.mem_free_kb() as f64 / 1048576.0, "mem_total_gb": n.mem_total_kb as f64 / 1048576.0})
        })
        .collect();
    let profiles = Profiles::load();
    let mut progs: BTreeMap<String, (String, f64, String, String)> = BTreeMap::new();
    for r in crate::daemon::read_state().unwrap_or_default() {
        let e = progs
            .entry(r.key.clone())
            .or_insert_with(|| (r.label.clone(), 0.0, r.nodes.clone(), r.gpus.clone()));
        e.1 += r.cores;
        if e.3.is_empty() {
            e.3 = r.gpus.clone();
        }
    }
    let mut programs: Vec<Value> = progs
        .into_iter()
        .map(|(k, (label, cores, nodes, gpus))| {
            let p = profiles.get(&k);
            json!({"key": k, "label": label, "cores": cores, "nodes": nodes, "gpus": gpus,
                   "profile": p.map(describe).unwrap_or_else(|| crate::i18n::tr("automático").into())})
        })
        .collect();
    programs.sort_by(|a, b| {
        b["cores"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&a["cores"].as_f64().unwrap_or(0.0))
    });
    programs.truncate(20);
    json!({
        "host": std::fs::read_to_string("/etc/hostname").unwrap_or_default().trim(),
        "daemon": ctl::send(&Request::Ping).is_ok_and(|r| r.ok),
        "gpus": gpus, "nodes": nodes, "programs": programs,
        "wol": crate::wol::nics(),
        "watts": std::fs::read_to_string(crate::learned::run_dir().join("hw.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v["watts"].as_f64()),
        "kwh_today": crate::energy::Energy::load_today(),
        "price_kwh": crate::machine::Machine::load().price_kwh,
        "version": env!("CARGO_PKG_VERSION"),
        "alerts": crate::health::load()
            .map(|r| crate::health::problems(&r).into_iter().filter(|p| p.0 >= 2).map(|p| p.1).collect::<Vec<_>>())
            .unwrap_or_default(),
    })
}

fn describe(p: &Profile) -> String {
    if p.is_auto() {
        return crate::i18n::tr("automático").into();
    }
    let mut v = vec![];
    v.push(match p.cpu.as_str() {
        "auto" => "CPU auto".to_string(),
        "gpu" => crate::i18n::tr("socket da GPU").into(),
        "all" => crate::i18n::tr("dois sockets").into(),
        o => format!("socket {o}"),
    });
    if let Some(c) = p.cores {
        v.push(format!(
            "{c} CPUs{}",
            if p.smt {
                ""
            } else {
                crate::i18n::tr(" sem HT")
            }
        ));
    }
    if p.gpu != "auto" {
        v.push(p.gpu.clone());
    }
    if p.exclusive {
        v.push(crate::i18n::tr("exclusivo").into());
    }
    v.join(" · ")
}

fn act(body: &Value, topo: &Topology) -> Result<String, String> {
    let _ = topo;
    match body["action"].as_str().unwrap_or("") {
        "wake" => crate::peers::wake(body["name"].as_str().ok_or(crate::i18n::tr("falta name"))?),
        "launch" => {
            let id = body["appid"]
                .as_str()
                .ok_or(crate::i18n::tr("falta appid"))?;
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
                return Err(crate::i18n::tr("appid inválido").into());
            }
            let name = crate::steam::installed_games()
                .into_iter()
                .find(|g| g.0 == id)
                .map(|g| g.1)
                .ok_or(crate::i18n::tr("jogo não instalado aqui"))?;
            spawn_quiet(&["steam", &format!("steam://rungameid/{id}")])?;
            Ok(crate::tf!(
                "abrindo {name} na Steam",
                "opening {name} on Steam"
            ))
        }
        "sunshine" => sunshine_start(),
        "preset" => {
            let key = body["key"]
                .as_str()
                .ok_or(crate::i18n::tr("falta key"))?
                .to_string();
            let preset = body["preset"].as_str().unwrap_or("auto");
            let r = if preset == "auto" {
                ctl::send(&Request::DeleteProfile { key, pid: None })
            } else {
                let base = Profiles::load()
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| Profile {
                        label: body["label"].as_str().unwrap_or("").to_string(),
                        ..Default::default()
                    });
                let profile = Profile::preset(preset, &base);
                ctl::send(&Request::SetProfile {
                    key,
                    profile,
                    pid: None,
                })
            }
            .map_err(|e| format!("{e:#}"))?;
            if r.ok { Ok(r.message) } else { Err(r.message) }
        }
        a @ ("lab_up" | "lab_down" | "llm_up" | "llm_down") => {
            let (what, verb) = a.split_once('_').unwrap();
            std::process::Command::new(gfx906())
                .args([what, verb])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|e| e.to_string())?;
            Ok(format!(
                "{what}: {}",
                if verb == "up" {
                    crate::i18n::tr("ligando")
                } else {
                    crate::i18n::tr("desligando")
                }
            ))
        }
        o => Err(crate::tf!("ação desconhecida: {o}", "unknown action: {o}")),
    }
}

/// Métricas no formato do Prometheus (para o Grafana): GPUs, processadores e os outros PCs.
pub fn metrics(topo: &Topology) -> String {
    let host = std::fs::read_to_string("/etc/hostname")
        .unwrap_or_default()
        .trim()
        .to_string();
    let mut out = String::new();
    let mut line = |name: &str, labels: String, v: Option<f64>| {
        if let Some(v) = v {
            out.push_str(&format!(
                "numapilot_{name}{{host=\"{host}\"{labels}}} {v}\n"
            ));
        }
    };
    for g in &topo.gpus {
        let s = crate::gpuctl::read_sensors(&g.pci);
        let l = format!(",gpu=\"{}\"", g.alias);
        line("gpu_temp_celsius", l.clone(), s.control_temp());
        line("gpu_power_watts", l.clone(), s.power_w);
        line("gpu_fan_percent", l.clone(), s.fan_pct);
        line(
            "gpu_busy_percent",
            l.clone(),
            g.busy_percent().map(|b| b as f64),
        );
        line(
            "gpu_vram_used_bytes",
            l,
            g.vram_used_total().map(|v| v.0 as f64),
        );
    }
    let loads = crate::decide::sample_node_loads(topo, std::time::Duration::from_millis(200));
    for n in &topo.nodes {
        line(
            "cpu_load_ratio",
            format!(",node=\"{}\"", n.id),
            loads.get(&n.id).copied(),
        );
    }
    for (k, t) in crate::cpuctl::package_temps() {
        line("cpu_temp_celsius", format!(",package=\"{k}\""), Some(t));
    }
    if let Some(e) = crate::energy::Energy::load_today() {
        line("energy_today_kwh", String::new(), Some(e));
    }
    for p in crate::peers::summary() {
        let l = format!(",peer=\"{}\"", p["name"].as_str().unwrap_or(""));
        line(
            "peer_online",
            l.clone(),
            Some(if p["online"].as_bool() == Some(true) {
                1.0
            } else {
                0.0
            }),
        );
        line("peer_gpu_temp_max_celsius", l, p["hot"].as_f64());
    }
    out
}

/// Roda sem esperar e sem prender a saída (o painel continua respondendo).
fn spawn_quiet(cmd: &[&str]) -> Result<(), String> {
    std::process::Command::new(cmd[0])
        .args(&cmd[1..])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", cmd[0]))
}

/// Liga o Sunshine: serviço do usuário, Flatpak ou o executável.
fn sunshine_start() -> Result<String, String> {
    for unit in [
        "sunshine.service",
        "app-dev.lizardbyte.app.Sunshine.service",
    ] {
        let ok = std::process::Command::new("systemctl")
            .args(["--user", "start", unit])
            .output()
            .is_ok_and(|o| o.status.success());
        if ok {
            return Ok(crate::i18n::tr("Sunshine ligado").into());
        }
    }
    let flatpak = std::process::Command::new("flatpak")
        .args(["info", "dev.lizardbyte.app.Sunshine"])
        .output()
        .is_ok_and(|o| o.status.success());
    if flatpak {
        spawn_quiet(&["flatpak", "run", "dev.lizardbyte.app.Sunshine"])?;
        return Ok(crate::i18n::tr("Sunshine ligando (Flatpak)").into());
    }
    if crate::containers::which("sunshine") {
        spawn_quiet(&["sunshine"])?;
        return Ok(crate::i18n::tr("Sunshine ligando").into());
    }
    Err(crate::i18n::tr("Sunshine não instalado neste PC").into())
}

fn authorized(req: &tiny_http::Request, token: &str) -> bool {
    let hdr = req
        .headers()
        .iter()
        .find(|h| h.field.equiv("X-Token"))
        .map(|h| h.value.as_str().to_string());
    let q = req.url().split_once('?').map(|(_, q)| q).unwrap_or("");
    let qt = q
        .split('&')
        .find_map(|kv| kv.strip_prefix("t="))
        .map(String::from);
    let given = hdr.or(qt).unwrap_or_default();
    // comparação em tempo constante
    given.len() == token.len()
        && given
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |a, (x, y)| a | (x ^ y))
            == 0
}

fn respond(req: tiny_http::Request, code: u16, ctype: &str, body: String) {
    let h = tiny_http::Header::from_bytes(&b"Content-Type"[..], ctype.as_bytes()).unwrap();
    let nc = tiny_http::Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).unwrap();
    let _ = req.respond(
        tiny_http::Response::from_string(body)
            .with_status_code(code)
            .with_header(h)
            .with_header(nc),
    );
}

pub fn serve(cfg: &Config, bind: &str, port: u16) -> anyhow::Result<()> {
    let token = token()?;
    let topo = Topology::load(cfg);
    let server = tiny_http::Server::http(format!("{bind}:{port}"))
        .map_err(|e| anyhow::anyhow!("{}", crate::tf!("porta {port}: {e}", "port {port}: {e}")))?;
    match url(port) {
        Some(u) => eprintln!("{}", crate::tf!("Painel: {u}", "Panel: {u}")),
        None => eprintln!(
            "{}",
            crate::tf!(
                "Painel na porta {port} (token em {})",
                "Panel on port {port} (token in {})",
                token_path().display()
            )
        ),
    }
    std::thread::spawn(crate::watch::run);
    let mut bad_auth = 0u32;
    for mut req in server.incoming_requests() {
        let path = req.url().split('?').next().unwrap_or("/").to_string();
        if bad_auth > 0 && path.starts_with("/api/") && authorized(&req, &token) {
            bad_auth = 0;
        }
        match (req.method(), path.as_str()) {
            (tiny_http::Method::Get, "/") => {
                respond(req, 200, "text/html; charset=utf-8", PAGE.to_string())
            }
            (_, p) if (p.starts_with("/api/") || p == "/metrics") && !authorized(&req, &token) => {
                // segura quem fica tentando adivinhar o token: depois de 10 erros seguidos,
                // cada erro novo espera 2 s (um acerto zera)
                bad_auth += 1;
                if bad_auth > 10 {
                    if bad_auth == 11 {
                        crate::log::warn(crate::i18n::tr(
                            "painel web: muitas tentativas com token errado",
                        ));
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
                respond(
                    req,
                    401,
                    "application/json",
                    r#"{"error":"token inválido"}"#.into(),
                )
            }
            (tiny_http::Method::Get, "/api/state") => {
                respond(req, 200, "application/json", state(&topo).to_string())
            }
            (tiny_http::Method::Get, "/api/peers") => {
                let since = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
                    .saturating_sub(24 * 3600);
                let hist = crate::watch::read_history(since);
                let mut v = crate::peers::summary();
                for p in &mut v {
                    let name = p["name"].as_str().unwrap_or("").to_string();
                    p["hist"] = json!(
                        crate::watch::series(&hist, &name, since, 900)
                            .into_iter()
                            .map(|x| x.1)
                            .collect::<Vec<_>>()
                    );
                }
                respond(req, 200, "application/json", Value::Array(v).to_string())
            }
            (tiny_http::Method::Get, "/api/profiles") => {
                let v = serde_json::to_string(&crate::profiles::Profiles::load().map)
                    .unwrap_or_default();
                respond(req, 200, "application/json", v)
            }
            (tiny_http::Method::Get, "/api/backup") => {
                let f = std::env::temp_dir().join(format!(
                    "numapilot-web-backup-{}.tar.gz",
                    std::process::id()
                ));
                match crate::backup::create(Some(&f))
                    .and_then(|p| std::fs::read(&p).map_err(|e| e.to_string()))
                {
                    Ok(bytes) => {
                        let _ = std::fs::remove_file(&f);
                        let h = tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/gzip"[..],
                        )
                        .unwrap();
                        let _ = req.respond(tiny_http::Response::from_data(bytes).with_header(h));
                    }
                    Err(e) => respond(
                        req,
                        500,
                        "application/json",
                        json!({"error": e}).to_string(),
                    ),
                }
            }
            (tiny_http::Method::Get, "/api/games") => {
                let v: Vec<Value> = crate::steam::installed_games()
                    .into_iter()
                    .map(|(id, name)| json!({"appid": id, "name": name}))
                    .collect();
                respond(req, 200, "application/json", Value::Array(v).to_string())
            }
            (tiny_http::Method::Get, "/metrics") => {
                respond(req, 200, "text/plain; version=0.0.4", metrics(&topo))
            }
            (tiny_http::Method::Post, "/api/action") => {
                let mut b = String::new();
                let _ = req.as_reader().take(64 * 1024).read_to_string(&mut b);
                let body: Value = serde_json::from_str(&b).unwrap_or(Value::Null);
                let (code, v) = match act(&body, &topo) {
                    Ok(m) => (200, json!({"ok": true, "message": m})),
                    Err(m) => (400, json!({"ok": false, "message": m})),
                };
                respond(req, code, "application/json", v.to_string())
            }
            _ => respond(
                req,
                404,
                "text/plain",
                crate::i18n::tr("não encontrado").into(),
            ),
        }
    }
    Ok(())
}

const PAGE: &str = r##"<!DOCTYPE html>
<html lang="pt-BR"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<meta name="theme-color" content="#16181d"><title>numapilot</title>
<style>
:root{--bg:#16181d;--card:#20232b;--fg:#e8e9ec;--mut:#9095a1;--ok:#5fbf78;--warn:#e6a23c;--bad:#e5534b;--acc:#4f8ff0}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:15px/1.4 system-ui,-apple-system,sans-serif;padding:12px 12px 40px}
h1{font-size:20px;margin:4px 0 12px;display:flex;justify-content:space-between;align-items:center}
h2{font-size:13px;text-transform:uppercase;letter-spacing:.06em;color:var(--mut);margin:18px 2px 8px}
.card{background:var(--card);border-radius:12px;padding:12px;margin-bottom:10px}
.row{display:flex;justify-content:space-between;align-items:baseline;gap:8px}
.big{font-size:26px;font-weight:600}.mut{color:var(--mut);font-size:13px}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:10px}
.bar{height:6px;background:#2d313b;border-radius:3px;overflow:hidden;margin-top:6px}.bar i{display:block;height:100%;background:var(--acc)}
button{background:#2d313b;color:var(--fg);border:0;border-radius:8px;padding:9px 11px;font-size:14px;margin:4px 4px 0 0}
button:active{background:var(--acc)}.hot{color:var(--bad)}.warm{color:var(--warn)}.cool{color:var(--ok)}
#msg{position:fixed;left:12px;right:12px;bottom:12px;background:#2d313b;border-radius:10px;padding:10px;display:none}
.dot{width:9px;height:9px;border-radius:50%;display:inline-block;margin-right:6px}
</style></head><body>
<h1><span>numapilot <span class="mut" id="host"></span></span><span class="mut" id="svc"></span></h1>
<div id="err" class="card hot" style="display:none"></div>
<div id="alerts" class="card warm" style="display:none"></div>
<h2>Placas de vídeo</h2><div class="grid" id="gpus"></div>
<h2>Processadores</h2><div class="grid" id="nodes"></div>
<h2>Programas</h2><div id="progs"></div>
<div id="peersbox" style="display:none"><h2>Meus PCs</h2><div class="grid" id="peers"></div></div>
<div id="gamesbox" style="display:none"><h2>Jogos</h2><div class="card">
<div class="row"><input id="gq" placeholder="procurar" oninput="drawGames()" style="flex:1;padding:6px;border-radius:6px;border:1px solid #444;background:#111;color:var(--fg)">
<button onclick="act({action:'sunshine'})">Ligar Sunshine</button></div><div id="games"></div></div></div>
<h2>Atalhos</h2><div class="card">
<div class="row"><span>Laboratório (Jupyter/VS Code)</span></div>
<button onclick="act({action:'lab_up'})">Ligar</button><button onclick="act({action:'lab_down'})">Desligar</button>
<div class="row" style="margin-top:10px"><span>Servidor de IA</span></div>
<button onclick="act({action:'llm_up'})">Iniciar</button><button onclick="act({action:'llm_down'})">Parar</button>
</div>
<div id="msg"></div>
<script>
const q=new URLSearchParams(location.search);let T=q.get('t')||localStorage.getItem('npt')||'';
if(q.get('t')){try{localStorage.setItem('npt',T)}catch(e){};history.replaceState(null,'','/')}
const esc=s=>String(s??'').replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]));
const tc=t=>t==null?'':t>=90?'hot':t>=75?'warm':'cool';
function toast(m){const e=document.getElementById('msg');e.textContent=m;e.style.display='block';clearTimeout(window._t);window._t=setTimeout(()=>e.style.display='none',3000)}
async function act(b){try{const r=await fetch('/api/action',{method:'POST',headers:{'X-Token':T,'Content-Type':'application/json'},body:JSON.stringify(b)});const j=await r.json();toast(j.message);setTimeout(load,800)}catch(e){toast('erro: '+e)}}
const PRE=[['jogo','Jogo'],['ia','IA'],['render','Render'],['silencioso','Silencioso'],['auto','Auto']];
async function load(){
 let s;try{const r=await fetch('/api/state',{headers:{'X-Token':T}});if(r.status==401){document.getElementById('err').style.display='block';document.getElementById('err').textContent='Token inválido: abra o link com ?t= mostrado no numapilot (Configurações → Painel no celular).';return}s=await r.json()}catch(e){return}
 document.getElementById('err').style.display='none';
 window.ME=s.version;
 const al=document.getElementById('alerts');al.style.display=(s.alerts||[]).length?'block':'none';al.innerHTML=(s.alerts||[]).map(a=>'⚠ '+esc(a)).join('<br>');
 document.getElementById('host').textContent=s.host+(s.watts!=null?' · '+Math.round(s.watts)+' W':'')+(s.kwh_today!=null?' · hoje '+s.kwh_today.toFixed(2)+' kWh'+(s.price_kwh?' (R$ '+(s.kwh_today*s.price_kwh).toFixed(2)+')':''):'');
 document.getElementById('svc').innerHTML=`<span class="dot" style="background:${s.daemon?'var(--ok)':'var(--bad)'}"></span>${s.daemon?'serviço ok':'serviço parado'}`;
 document.getElementById('gpus').innerHTML=s.gpus.map(g=>`<div class="card"><div class="row"><b>${esc(g.alias)}</b><span class="mut">socket ${g.node??'?'}</span></div>
  <div class="big ${tc(g.temp)}">${g.temp!=null?Math.round(g.temp)+' °C':'—'}</div>
  <div class="mut">${g.power!=null?Math.round(g.power)+' W':''}${g.cap!=null?' / '+Math.round(g.cap)+' W':''} · ventoinha ${g.fan!=null?Math.round(g.fan)+'%':'—'}</div>
  <div class="mut">uso ${g.busy??0}%${g.vram?` · VRAM ${g.vram[0].toFixed(1)}/${g.vram[1].toFixed(0)} GB`:''}</div>
  <div class="bar"><i style="width:${g.busy??0}%"></i></div></div>`).join('');
 document.getElementById('nodes').innerHTML=s.nodes.map(n=>`<div class="card"><div class="row"><b>Socket ${n.id}</b><span class="mut">${n.cpus} threads</span></div>
  <div class="big">${Math.round(n.load*100)}%</div><div class="mut">RAM livre ${n.mem_free_gb.toFixed(0)} de ${n.mem_total_gb.toFixed(0)} GB</div>
  <div class="bar"><i style="width:${Math.round(n.load*100)}%"></i></div></div>`).join('');
 document.getElementById('progs').innerHTML=s.programs.length?s.programs.map(p=>`<div class="card"><div class="row"><b>${esc(p.label)}</b><span class="mut">${p.cores.toFixed(1)} núcleos</span></div>
  <div class="mut">socket ${esc(p.nodes||'todos')}${p.gpus?' · '+esc(p.gpus):''} · ${esc(p.profile)}</div>
  ${PRE.map(([k,l])=>`<button onclick='act({action:"preset",key:${JSON.stringify(p.key)},label:${JSON.stringify(p.label)},preset:"${k}"})'>${l}</button>`).join('')}</div>`).join('')
  :'<div class="card mut">Nenhum programa sendo usado agora.</div>';
}
function spark(h){if(!h||!h.filter(v=>v!=null).length)return'';const w=200,ht=36,mx=100,mn=20,n=h.length;let d='',pen=false;
 h.forEach((v,i)=>{if(v==null){pen=false;return}const x=(i/(n-1||1))*w,y=ht-((Math.min(Math.max(v,mn),mx)-mn)/(mx-mn))*ht;d+=(pen?'L':'M')+x.toFixed(1)+' '+y.toFixed(1)+' ';pen=true});
 return `<svg width="${w}" height="${ht}" style="display:block;margin:4px 0"><path d="${d}" fill="none" stroke="var(--acc)" stroke-width="1.5"/></svg><div class="mut">temperatura máx., últimas 24 h</div>`}
async function peers(){try{const r=await fetch('/api/peers',{headers:{'X-Token':T}});if(!r.ok)return;const p=await r.json();
 document.getElementById('peersbox').style.display=p.length?'block':'none';
 document.getElementById('peers').innerHTML=p.map(x=>`<div class="card"><div class="row"><b>${esc(x.name)}</b><span class="dot" style="background:${x.online?(x.daemon?'var(--ok)':'var(--warn)'):'var(--bad)'}"></span></div>
  ${x.online?`<div class="mut">${esc(x.host||'')} · ${x.gpus} GPU · ${x.programs} programas</div>
  <div class="big ${tc(x.hot)}">${x.hot!=null?Math.round(x.hot)+' °C':'—'}</div>
  ${spark(x.hist)}
  ${x.version&&window.ME&&x.version!=window.ME?`<div class="mut warm">numapilot ${esc(x.version)} (este: ${esc(window.ME)}): atualize em Configurações</div>`:''}
  <div class="mut">CPU ${(x.load||[]).map(l=>Math.round(l*100)+'%').join(' · ')}</div>
  <a href="${esc(x.link)}"><button>Abrir</button></a>`:`<div class="mut">desligado ou fora da rede</div>${x.can_wake?`<button onclick='act({action:"wake",name:${JSON.stringify(x.name)}})'>Ligar</button>`:''}`}</div>`).join('')}catch(e){}}
let G=[];async function games(){try{const r=await fetch('/api/games',{headers:{'X-Token':T}});if(!r.ok)return;G=await r.json();document.getElementById('gamesbox').style.display=G.length?'block':'none';drawGames()}catch(e){}}
function drawGames(){const q=(document.getElementById('gq').value||'').toLowerCase();document.getElementById('games').innerHTML=G.filter(g=>g.name.toLowerCase().includes(q)).slice(0,30).map(g=>`<div class="row" style="margin-top:6px"><span>${esc(g.name)}</span><button onclick='act({action:"launch",appid:${JSON.stringify(g.appid)}})'>Abrir</button></div>`).join('')}
load();setInterval(load,3000);peers();setInterval(peers,15000);games();
</script></body></html>
"##;

#[cfg(test)]
mod tests {
    #[test]
    fn page_is_self_contained() {
        assert!(super::PAGE.starts_with("<!DOCTYPE html>"));
        assert!(!super::PAGE.contains("http://") && !super::PAGE.contains("https://"));
    }
}
