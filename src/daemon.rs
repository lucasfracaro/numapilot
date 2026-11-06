//! Daemon do modo automático: observa processos, coloca cada um no melhor socket
//! e expande para os dois sockets quando o programa satura o nó.
//! Também atende a interface gráfica pelo socket de controle (perfis por programa).

use crate::config::{Config, CpuSpec, GpuSpec};
use crate::ctl::{self, Request, Response};
use crate::decide::{least_loaded_node, node_loads, pick_gpus, Kind};
use crate::learned::{run_dir, Learned};
use crate::procfs::{self, Stat};
use crate::profiles::{program_key, Profile, Profiles};
use crate::topo::Topology;
use crate::util::{format_cpulist, glob_match, is_root};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

#[derive(Debug, Clone, PartialEq)]
enum St {
    Ignored(String),
    Watching,
    Placed,
}

#[derive(Debug, Clone)]
struct Tracked {
    starttime: u64,
    comm: String,
    /// Chave de aprendizado (executável/script).
    key: String,
    /// Chave do perfil (steam:ID ou executável).
    pkey: String,
    label: String,
    exe: Option<String>,
    uid: u32,
    origin: String,
    state: St,
    nodes: Vec<usize>,
    /// Decisão fixa (regra ou perfil): o automático não expande nem reduz.
    fixed: bool,
    reserve: bool,
    last_ticks: u64,
    cores: f64,
    active: u32,
    hi: u32,
    lo: u32,
    gpus: Vec<String>,
    gpu_node: Option<usize>,
    reapply_at: Option<u64>,
}

/// Linha do /run/numapilot/state.json (lida por `status` e pela interface gráfica).
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(default)]
pub struct Row {
    pub pid: i32,
    pub ppid: i32,
    pub comm: String,
    pub key: String,
    pub label: String,
    pub exe: String,
    pub uid: u32,
    pub origin: String,
    pub state: String,
    pub nodes: String,
    pub cpus: String,
    pub cores: f64,
    pub gpus: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Pin {
    pub pid: i32,
    pub starttime: u64,
}

pub fn pins_path() -> std::path::PathBuf {
    run_dir().join("pins.json")
}

pub fn state_path() -> std::path::PathBuf {
    run_dir().join("state.json")
}

pub fn read_pins() -> HashSet<Pin> {
    std::fs::read_to_string(pins_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<Pin>>(&s).ok())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

pub fn read_state() -> Option<Vec<Row>> {
    let s = std::fs::read_to_string(state_path()).ok()?;
    serde_json::from_str(&s).ok()
}

struct Ctx<'a> {
    cfg: &'a Config,
    topo: &'a Topology,
    all_cpus: Vec<usize>,
    node_sets: Vec<Vec<usize>>,
}

impl Ctx<'_> {
    fn node_size(&self, id: usize) -> f64 {
        self.topo.node(id).map(|n| n.cpus.len()).unwrap_or(1).max(1) as f64
    }

    /// Nós correspondentes a uma especificação de CPU (de regra ou perfil). None = automático.
    fn nodes_for(&self, cpu: &CpuSpec, gpu: &GpuSpec, want: Option<Kind>, hint: bool) -> Option<Vec<usize>> {
        match cpu {
            CpuSpec::Auto => None,
            CpuSpec::All => Some(self.topo.node_ids()),
            CpuSpec::Nodes(n) => Some(n.iter().copied().filter(|x| self.topo.node(*x).is_some()).collect()),
            CpuSpec::Cpus(c) => Some(
                self.topo
                    .nodes
                    .iter()
                    .filter(|n| n.cpus.iter().any(|x| c.contains(x)))
                    .map(|n| n.id)
                    .collect(),
            ),
            CpuSpec::GpuNode => {
                let (g, _) = pick_gpus(self.topo, gpu, want, hint).ok()?;
                let mut n: Vec<usize> = g.iter().filter_map(|g| g.numa_node).collect();
                n.sort_unstable();
                n.dedup();
                if n.is_empty() { None } else { Some(n) }
            }
        }
        .filter(|v| !v.is_empty())
    }

    fn profile_nodes(&self, p: &Profile) -> Option<Vec<usize>> {
        let cpu = CpuSpec::parse(&p.cpu).ok()?;
        let gpu = GpuSpec::parse(&p.gpu).unwrap_or(GpuSpec::Auto);
        let want = Kind::parse_usage(&p.usage).ok().flatten();
        self.nodes_for(&cpu, &gpu, want, false)
    }

    fn apply(&self, pid: i32, t: &mut Tracked, nodes: Vec<usize>, migrate: bool, why: &str, tick: u64) {
        let cpus = self.topo.cpus_of(&nodes);
        let n = procfs::set_affinity_all(pid, &cpus);
        if n == 0 {
            t.state = St::Ignored("sem permissão para mudar a afinidade".into());
            return;
        }
        let all = self.topo.node_ids();
        if migrate && self.cfg.daemon.migrate_memory && nodes.len() < all.len() {
            let from: Vec<usize> = all.iter().copied().filter(|x| !nodes.contains(x)).collect();
            let to = nodes.clone();
            // Migração pode demorar em processos grandes: roda em segundo plano.
            std::thread::spawn(move || {
                let _ = procfs::migrate_pages(pid, &from, &to);
            });
        }
        eprintln!(
            "[numapilot] {} (pid {pid}) → nó(s) {:?} [{}]: {why}",
            t.comm,
            nodes,
            format_cpulist(&cpus)
        );
        t.nodes = nodes;
        t.state = St::Placed;
        t.reapply_at = Some(tick + 2);
    }

    /// Aplica (ao vivo) a escolha de CPU de um perfil num processo em execução.
    fn apply_profile_live(&self, pid: i32, t: &mut Tracked, p: &Profile, tick: u64) {
        match self.profile_nodes(p) {
            Some(nodes) => {
                t.fixed = true;
                t.origin = "perfil".into();
                self.apply(pid, t, nodes, true, "escolha feita na interface", tick);
            }
            None => {
                // Voltou para o automático: libera e deixa o daemon decidir de novo.
                procfs::set_affinity_all(pid, &self.all_cpus);
                t.fixed = false;
                t.origin = "auto".into();
                t.state = St::Watching;
                t.active = 0;
                t.nodes.clear();
                eprintln!("[numapilot] {} (pid {pid}) → automático (escolha feita na interface)", t.comm);
            }
        }
    }
}

fn same_set(a: &[usize], b: &[usize]) -> bool {
    a.len() == b.len() && a.iter().all(|x| b.contains(x))
}

fn bind_ctl() -> Option<UnixListener> {
    let p = ctl::sock_path();
    let _ = std::fs::remove_file(&p);
    match UnixListener::bind(&p) {
        Ok(l) => {
            // Qualquer usuário local pode falar com o daemon; a permissão é checada por pedido (SO_PEERCRED).
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o666));
            let _ = l.set_nonblocking(true);
            Some(l)
        }
        Err(e) => {
            eprintln!("[numapilot] aviso: sem socket de controle ({}): {e}", p.display());
            None
        }
    }
}

pub fn run(cfg: Config) -> Result<()> {
    if !is_root() {
        eprintln!("[numapilot] aviso: sem root só é possível mexer nos seus próprios processos.");
    }
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }
    let topo = Topology::load(&cfg);
    for w in topo.warnings() {
        eprintln!("[numapilot] aviso: {w}");
    }
    if topo.nodes.len() < 2 {
        eprintln!("[numapilot] só há 1 nó NUMA: o daemon apenas vai observar.");
    }
    let ctx = Ctx {
        cfg: &cfg,
        topo: &topo,
        all_cpus: topo.all_cpus(),
        node_sets: topo.nodes.iter().map(|n| n.cpus.clone()).collect(),
    };
    let _ = std::fs::create_dir_all(run_dir());
    let listener = bind_ctl();
    let mut learned = Learned::load();
    let mut profiles = Profiles::load();
    let tck = procfs::clk_tck();
    let me = std::process::id() as i32;
    let mut procs: HashMap<i32, Tracked> = HashMap::new();
    let mut prev_cpu = procfs::cpu_times();
    let mut prev_t = Instant::now();
    let mut last_save = Instant::now();
    let mut tick: u64 = 0;
    let tick_dur = Duration::from_millis(cfg.daemon.tick_ms);
    eprintln!(
        "[numapilot] daemon iniciado: {} nós, {} GPUs, amostra a cada {} ms",
        topo.nodes.len(),
        topo.gpus.len(),
        cfg.daemon.tick_ms
    );

    while !STOP.load(Ordering::SeqCst) {
        // Espera o próximo tick atendendo a interface gráfica nesse meio tempo.
        let wait_start = Instant::now();
        while wait_start.elapsed() < tick_dur && !STOP.load(Ordering::SeqCst) {
            if let Some(l) = &listener {
                while let Ok((s, _)) = l.accept() {
                    serve(s, &ctx, &mut procs, &mut profiles, tick);
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        tick += 1;
        let dt = prev_t.elapsed().as_secs_f64().max(0.001);
        prev_t = Instant::now();
        let cur_cpu = procfs::cpu_times();
        let loads = node_loads(&topo, &prev_cpu, &cur_cpu);
        prev_cpu = cur_cpu;
        let pins = read_pins();

        let pids = procfs::list_pids();
        let alive: HashSet<i32> = pids.iter().copied().collect();
        procs.retain(|p, _| alive.contains(p));
        let mut ppids: HashMap<i32, i32> = HashMap::new();

        for pid in pids {
            if pid == me {
                continue;
            }
            let Some(st) = procfs::read_stat(pid) else { continue };
            if st.is_kthread() {
                continue;
            }
            ppids.insert(pid, st.ppid);
            let reclassify = match procs.get(&pid) {
                None => true,
                // pid reaproveitado, ou exec() trocou o programa
                Some(t) => t.starttime != st.starttime || t.comm != st.comm,
            };
            if reclassify {
                let parent = procs.get(&st.ppid).cloned();
                let mut t = classify(&ctx, &learned, &profiles, pid, &st, parent.as_ref());
                if t.state == St::Placed {
                    let nodes = t.nodes.clone();
                    let why = t.origin.clone();
                    ctx.apply(pid, &mut t, nodes, true, &why, tick);
                } else if let (St::Watching, Some(n), true) =
                    (&t.state, learned.get(&t.key).and_then(|e| e.gpu_node), topo.nodes.len() > 1)
                {
                    // Já sabemos que este programa usa a GPU do nó n: começa lá (first touch local).
                    ctx.apply(pid, &mut t, vec![n], true, "aprendido: usa a GPU deste socket", tick);
                    t.origin = "aprendido".into();
                }
                procs.insert(pid, t);
                continue;
            }

            let reserved = reserved_nodes(&procs);
            let t = procs.get_mut(&pid).unwrap();
            if pins.contains(&Pin { pid, starttime: st.starttime }) {
                t.state = St::Ignored("pin manual (numapilot pin)".into());
                continue;
            }
            let delta = st.cpu_ticks.saturating_sub(t.last_ticks) as f64;
            t.last_ticks = st.cpu_ticks;
            t.cores = delta / tck / dt;
            // GPUs em uso (para a interface), a cada 5 amostras.
            let visible = !matches!(&t.state, St::Ignored(r) if !r.starts_with("manual"));
            if visible && tick.is_multiple_of(5) {
                let drm = procfs::open_drm(pid);
                let mut g: Vec<String> =
                    drm.iter().filter_map(|r| topo.gpu_by_render(r)).map(|g| g.alias.clone()).collect();
                g.sort();
                g.dedup();
                t.gpus = g;
            }
            if matches!(t.state, St::Ignored(_)) {
                continue;
            }
            step(&ctx, &mut learned, pid, t, &loads, &reserved, tick);
        }

        write_state(&ctx, &procs, &ppids, &profiles);
        if last_save.elapsed() > Duration::from_secs(30) {
            if let Err(e) = learned.save() {
                eprintln!("[numapilot] não consegui salvar o aprendizado: {e}");
            }
            last_save = Instant::now();
        }
    }

    // Ao parar, devolve todos os processos gerenciados para todos os CPUs.
    for (pid, t) in &procs {
        if t.state == St::Placed && !t.fixed {
            procfs::set_affinity_all(*pid, &ctx.all_cpus);
        }
    }
    let _ = learned.save();
    let _ = std::fs::remove_file(state_path());
    let _ = std::fs::remove_file(ctl::sock_path());
    eprintln!("[numapilot] daemon encerrado.");
    Ok(())
}

/// Atende um pedido da interface gráfica / CLI.
fn serve(s: UnixStream, ctx: &Ctx, procs: &mut HashMap<i32, Tracked>, profiles: &mut Profiles, tick: u64) {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
    let uid = ctl::peer_uid(&s);
    let mut line = String::new();
    let mut reader = BufReader::new(&s);
    let resp = match (reader.read_line(&mut line), uid) {
        (Ok(_), Some(uid)) => match serde_json::from_str::<Request>(line.trim()) {
            Ok(req) => handle(req, uid, ctx, procs, profiles, tick),
            Err(e) => Response::err(format!("pedido inválido: {e}")),
        },
        _ => Response::err("não consegui ler o pedido"),
    };
    let mut out = serde_json::to_string(&resp).unwrap_or_default();
    out.push('\n');
    let _ = (&s).write_all(out.as_bytes());
}

fn handle(
    req: Request,
    uid: u32,
    ctx: &Ctx,
    procs: &mut HashMap<i32, Tracked>,
    profiles: &mut Profiles,
    tick: u64,
) -> Response {
    let (key, profile, pid) = match req {
        Request::Ping => return Response::ok("pong"),
        Request::SetProfile { key, profile, pid } => (key, profile, pid),
        Request::DeleteProfile { key, pid } => (key, Profile::default(), pid),
    };
    if uid != 0 && uid < ctx.cfg.daemon.min_uid {
        return Response::err("sem permissão");
    }
    if let Err(e) = CpuSpec::parse(&profile.cpu)
        .and(GpuSpec::parse(&profile.gpu).map(|_| ()))
        .and(Kind::parse_usage(&profile.usage).map(|_| ()))
    {
        return Response::err(format!("perfil inválido: {e}"));
    }
    if let Some(p) = pid
        && uid != 0
        && procfs::uid(p).is_some_and(|owner| owner != uid)
    {
        return Response::err("esse processo é de outro usuário");
    }
    if profile.is_auto() {
        profiles.map.remove(&key);
    } else {
        profiles.map.insert(key.clone(), profile.clone());
    }
    if let Err(e) = profiles.save() {
        return Response::err(format!("não consegui salvar o perfil: {e}"));
    }
    // Aplica já a escolha de CPU em todos os processos desse programa.
    let mut n = 0;
    for (p, t) in procs.iter_mut() {
        let mine = uid == 0 || t.uid == uid;
        let matches_key = t.pkey == key || Some(*p) == pid;
        if mine && matches_key && !matches!(&t.state, St::Ignored(r) if !r.starts_with("manual")) {
            ctx.apply_profile_live(*p, t, &profile, tick);
            n += 1;
        }
    }
    Response::ok(format!("perfil salvo; {n} processo(s) ajustado(s) agora"))
}

fn reserved_nodes(procs: &HashMap<i32, Tracked>) -> Vec<usize> {
    procs
        .values()
        .filter(|t| t.reserve && t.state == St::Placed && t.nodes.len() == 1)
        .map(|t| t.nodes[0])
        .collect()
}

fn classify(
    ctx: &Ctx,
    learned: &Learned,
    profiles: &Profiles,
    pid: i32,
    st: &Stat,
    parent: Option<&Tracked>,
) -> Tracked {
    let exe = procfs::exe(pid);
    let argv = procfs::cmdline(pid);
    let idents = procfs::idents(&st.comm, exe.as_deref(), &argv);
    let key = procfs::learn_key(exe.as_deref(), &argv, &st.comm);
    let steam = procfs::env_var(pid, "SteamAppId");
    let pkey = program_key(steam.as_deref(), &key);
    let uid = procfs::uid(pid).unwrap_or(0);
    let mut t = Tracked {
        starttime: st.starttime,
        comm: st.comm.clone(),
        key: key.clone(),
        pkey: pkey.clone(),
        label: st.comm.clone(),
        exe: exe.clone(),
        uid,
        origin: "auto".into(),
        state: St::Watching,
        nodes: vec![],
        fixed: false,
        reserve: false,
        last_ticks: st.cpu_ticks,
        cores: 0.0,
        active: 0,
        hi: 0,
        lo: 0,
        gpus: vec![],
        gpu_node: None,
        reapply_at: None,
    };
    let ignore = |t: &mut Tracked, why: &str| t.state = St::Ignored(why.to_string());

    if uid < ctx.cfg.daemon.min_uid {
        ignore(&mut t, "processo do sistema");
        return t;
    }
    if ctx.cfg.daemon.ignore.iter().any(|p| idents.iter().any(|i| glob_match(p, i))) {
        ignore(&mut t, "na lista ignore");
        return t;
    }
    let mode = procfs::env_var(pid, "NUMAPILOT_MODE");
    let allowed = procfs::allowed_cpus(pid).unwrap_or_default();
    if mode.as_deref() == Some("manual") {
        ignore(&mut t, "manual (numapilot run)");
        t.origin = "manual".into();
        t.nodes = ctx
            .topo
            .nodes
            .iter()
            .filter(|n| n.cpus.iter().any(|c| allowed.contains(c)))
            .map(|n| n.id)
            .collect();
        return t;
    }
    let ours = same_set(&allowed, &ctx.all_cpus) || ctx.node_sets.iter().any(|s| same_set(s, &allowed));
    if !ours && mode.as_deref() != Some("auto") {
        ignore(&mut t, "afinidade definida por outro programa (taskset?)");
        return t;
    }

    // Perfil salvo pela interface gráfica tem prioridade sobre as regras.
    if let Some(p) = profiles.get(&pkey)
        && let Some(nodes) = ctx.profile_nodes(p)
    {
        t.origin = "perfil".into();
        t.nodes = nodes;
        t.fixed = true;
        t.state = St::Placed;
        return t;
    }

    if let Some(rule) = ctx.cfg.find_rule(&idents) {
        t.origin = format!("regra '{}'", rule.label());
        t.reserve = rule.reserve;
        let cpu = CpuSpec::parse(&rule.cpu).unwrap_or(CpuSpec::Auto);
        let gpu = GpuSpec::parse(&rule.gpu).unwrap_or(GpuSpec::Auto);
        let want = Kind::parse_usage(&rule.usage).ok().flatten();
        if let Some(nodes) = ctx.nodes_for(&cpu, &gpu, want, ctx.cfg.is_compute_hint(&idents)) {
            t.nodes = nodes;
            t.fixed = true;
            t.state = St::Placed;
        }
        return t;
    }

    // Filho de um processo com decisão fixa (ex.: jogo aberto pela Steam) herda a decisão.
    if let Some(p) = parent
        && p.fixed
        && p.state == St::Placed
        && same_set(&allowed, &ctx.topo.cpus_of(&p.nodes))
    {
        t.origin = format!("herdado de {}", p.comm);
        t.nodes = p.nodes.clone();
        t.fixed = true;
        t.state = St::Placed;
        return t;
    }

    if let Some(e) = learned.get(&key)
        && e.wide
        && ctx.topo.nodes.len() > 1
    {
        t.origin = "aprendido (2 sockets)".into();
        t.nodes = ctx.topo.node_ids();
        t.state = St::Placed;
    }
    t
}

fn step(
    ctx: &Ctx,
    learned: &mut Learned,
    pid: i32,
    t: &mut Tracked,
    loads: &HashMap<usize, f64>,
    reserved: &[usize],
    tick: u64,
) {
    let d = &ctx.cfg.daemon;
    let multi = ctx.topo.nodes.len() > 1;

    if t.reapply_at == Some(tick) {
        // Pega threads criadas durante a primeira aplicação.
        procfs::set_affinity_all(pid, &ctx.topo.cpus_of(&t.nodes));
        t.reapply_at = None;
    }

    match t.state {
        St::Ignored(_) => {}
        St::Watching => {
            t.active = if t.cores >= d.watch_cores { t.active + 1 } else { 0 };
            if !multi {
                return;
            }
            // Usa GPU? Então vai para o socket dessa GPU.
            if t.cores > 0.05 && tick.is_multiple_of(2) {
                let drm = procfs::open_drm(pid);
                let gpus: Vec<_> = drm.iter().filter_map(|r| ctx.topo.gpu_by_render(r)).collect();
                if !gpus.is_empty() {
                    t.gpus = gpus.iter().map(|g| g.alias.clone()).collect();
                    t.gpus.sort();
                    t.gpus.dedup();
                    let mut nodes: Vec<usize> = gpus.iter().filter_map(|g| g.numa_node).collect();
                    nodes.sort_unstable();
                    nodes.dedup();
                    if !nodes.is_empty() {
                        let why = format!("usa a GPU {}", t.gpus.join(","));
                        if nodes.len() == 1 {
                            t.gpu_node = Some(nodes[0]);
                            let n = nodes[0];
                            learned.update(&t.key, |e| {
                                e.gpu_node = Some(n);
                                e.runs += 1;
                            });
                        }
                        ctx.apply(pid, t, nodes, true, &why, tick);
                        return;
                    }
                }
            }
            if t.active >= d.watch_ticks {
                let n = least_loaded_node(ctx.topo, loads, reserved);
                let why = format!("{:.1} núcleos em uso; nó {n} é o menos ocupado", t.cores);
                learned.update(&t.key, |e| e.runs += 1);
                ctx.apply(pid, t, vec![n], true, &why, tick);
            }
        }
        St::Placed => {
            if t.fixed || !multi {
                return;
            }
            // Começou a usar uma GPU de outro socket depois de posicionado? Muda para lá.
            if t.nodes.len() == 1 && t.gpu_node.is_none() && t.cores > 0.05 && tick.is_multiple_of(10) {
                let drm = procfs::open_drm(pid);
                let gpus: Vec<_> = drm.iter().filter_map(|r| ctx.topo.gpu_by_render(r)).collect();
                let mut nodes: Vec<usize> = gpus.iter().filter_map(|g| g.numa_node).collect();
                nodes.sort_unstable();
                nodes.dedup();
                if nodes.len() == 1 {
                    let n = nodes[0];
                    t.gpus = gpus.iter().map(|g| g.alias.clone()).collect();
                    t.gpus.sort();
                    t.gpus.dedup();
                    t.gpu_node = Some(n);
                    learned.update(&t.key, |e| e.gpu_node = Some(n));
                    if t.nodes[0] != n {
                        let why = format!("passou a usar a GPU {}", t.gpus.join(","));
                        ctx.apply(pid, t, vec![n], true, &why, tick);
                        return;
                    }
                }
            }
            if t.nodes.len() == 1 {
                let frac = t.cores / ctx.node_size(t.nodes[0]);
                if learned.get(&t.key).is_none_or(|e| frac > e.peak_frac + 0.05) {
                    learned.update(&t.key, |e| e.peak_frac = frac);
                }
                t.hi = if frac >= d.promote_frac { t.hi + 1 } else { 0 };
                if t.hi >= d.promote_ticks {
                    let why = format!("saturou o nó ({:.0}% por {} amostras) → 2 sockets", frac * 100.0, t.hi);
                    learned.update(&t.key, |e| e.wide = true);
                    t.hi = 0;
                    t.lo = 0;
                    ctx.apply(pid, t, ctx.topo.node_ids(), false, &why, tick);
                }
            } else {
                let per_node = t.cores / ctx.node_size(t.nodes[0]);
                t.lo = if per_node < d.demote_frac { t.lo + 1 } else { 0 };
                if t.lo >= d.demote_ticks {
                    let n = t.gpu_node.unwrap_or_else(|| least_loaded_node(ctx.topo, loads, reserved));
                    let why = format!("uso baixo por {} amostras → volta para 1 socket", t.lo);
                    t.lo = 0;
                    ctx.apply(pid, t, vec![n], true, &why, tick);
                }
            }
        }
    }
}

fn write_state(ctx: &Ctx, procs: &HashMap<i32, Tracked>, ppids: &HashMap<i32, i32>, profiles: &Profiles) {
    let mut rows: Vec<Row> = procs
        .iter()
        .filter(|(_, t)| match &t.state {
            St::Placed => true,
            St::Watching => t.cores >= 0.1 || !t.gpus.is_empty() || profiles.get(&t.pkey).is_some(),
            St::Ignored(r) => r.starts_with("manual") || r.starts_with("pin"),
        })
        .map(|(pid, t)| Row {
            pid: *pid,
            ppid: ppids.get(pid).copied().unwrap_or(0),
            comm: t.comm.clone(),
            key: t.pkey.clone(),
            label: t.label.clone(),
            exe: t.exe.clone().unwrap_or_default(),
            uid: t.uid,
            origin: t.origin.clone(),
            state: match &t.state {
                St::Placed => {
                    if t.nodes.len() > 1 { "2 sockets".into() } else { "1 socket".into() }
                }
                St::Watching => "observando".into(),
                St::Ignored(r) => r.clone(),
            },
            nodes: t.nodes.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(","),
            cpus: format_cpulist(&ctx.topo.cpus_of(&t.nodes)),
            cores: (t.cores * 10.0).round() / 10.0,
            gpus: t.gpus.join(","),
        })
        .collect();
    rows.sort_by(|a, b| b.cores.partial_cmp(&a.cores).unwrap_or(std::cmp::Ordering::Equal));
    if let Ok(s) = serde_json::to_string(&rows) {
        let tmp = run_dir().join("state.json.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, state_path());
        }
    }
}
