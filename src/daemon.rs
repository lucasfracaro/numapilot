//! Daemon do modo automático: observa processos, coloca cada um no melhor socket
//! e expande para os dois sockets quando o programa satura o nó.
//! Também atende a interface gráfica pelo socket de controle (perfis por programa).

use crate::cgroup::{self, Limits};
use crate::config::{Config, CpuSpec, GpuSpec};
use crate::ctl::{self, Request, Response};
use crate::decide::{Kind, least_loaded_node, node_loads, pick_gpus};
use crate::fans::{self, BoardFan, BoardFans};
use crate::gpuctl::{self, FanMode, GpuControls};
use crate::history::{self, History, Sample, Status};
use crate::learned::{Learned, run_dir};
use crate::machine::{self, LowLatency, Machine, Orig};
use crate::procevents;
use crate::procfs::{self, Stat};
use crate::profiles::{Profile, Profiles, program_key};
use crate::topo::Topology;
use crate::util::{format_cpulist, glob_match, is_root};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static STOP: AtomicBool = AtomicBool::new(false);

/// Estado dos controles de hardware da máquina (fora das GPUs).
struct Hw {
    machine: Machine,
    orig: Orig,
    lowlat: LowLatency,
    board: BoardFans,
    board_fans: Vec<BoardFan>,
    board_orig: HashMap<String, Option<u8>>,
    board_manual: HashMap<String, f64>,
    gpu_prof: BTreeMap<String, String>,
    power: crate::cpuctl::PowerMeter,
    cpu_power: BTreeMap<u32, f64>,
    last_ac: Option<bool>,
    ryzen_info: BTreeMap<String, f64>,
    /// Placas em teste de undervolt: o valor bom que fica no disco.
    gpu_trial: HashMap<String, gpuctl::GpuControl>,
    health_at: Option<Instant>,
    started: Instant,
}

fn base_governors() -> BTreeMap<usize, String> {
    std::fs::read_to_string(governors_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

impl Hw {
    fn new(topo: &Topology) -> Hw {
        let board_fans = fans::list();
        let board_orig = board_fans
            .iter()
            .map(|f| (f.id.clone(), f.enable))
            .collect();
        Hw {
            machine: Machine::load(),
            orig: Orig::capture(topo),
            lowlat: LowLatency::default(),
            board: BoardFans::load(),
            board_fans,
            board_orig,
            board_manual: HashMap::new(),
            gpu_prof: BTreeMap::new(),
            power: Default::default(),
            cpu_power: BTreeMap::new(),
            last_ac: crate::notebook::on_ac(),
            ryzen_info: BTreeMap::new(),
            gpu_trial: HashMap::new(),
            health_at: None,
            started: Instant::now(),
        }
    }

    /// Notebook: troca de modo ao ligar/desligar da tomada.
    fn power_source_tick(&mut self, topo: &Topology) {
        let ac = crate::notebook::on_ac();
        if ac.is_none() || ac == self.last_ac {
            return;
        }
        self.last_ac = ac;
        let target = if ac == Some(true) {
            self.machine.ac_mode.clone()
        } else {
            self.machine.battery_mode.clone()
        };
        let Some(name) = target else { return };
        if name == self.machine.mode {
            return;
        }
        let game_node = topo.gaming_gpu().and_then(|g| g.numa_node);
        let m = Machine::preset(&name, topo, game_node, &self.machine);
        let prev = std::mem::replace(&mut self.machine, m);
        let _ = self.machine.save();
        self.apply_machine(topo, Some(&prev));
        eprintln!(
            "[numapilot] {}: modo {name}",
            if ac == Some(true) {
                "na tomada"
            } else {
                "na bateria"
            }
        );
    }

    /// O firmware do notebook volta os limites do Ryzen ao trocar tomada/bateria ou perfil:
    /// confere de tempos em tempos e reaplica o que foi pedido.
    fn ryzen_tick(&mut self) {
        if !(crate::ryzen::available() && crate::ryzen::applicable()) {
            return;
        }
        let Ok(info) = crate::ryzen::info() else {
            return;
        };
        if let Some(w) = self.machine.ryzen.as_ref().filter(|l| !l.is_empty())
            && w.differs(&crate::ryzen::limits_from(&info))
        {
            match crate::ryzen::apply(w) {
                Ok(()) => eprintln!("[numapilot] limites do Ryzen reaplicados"),
                Err(e) => eprintln!("[numapilot] limites do Ryzen: {e}"),
            }
        }
        self.ryzen_info = info;
    }

    fn apply_machine(&mut self, topo: &Topology, prev: Option<&Machine>) -> Vec<String> {
        let errs = machine::apply(&self.machine, prev, &self.orig, topo, &base_governors());
        // O uncore dos programas de baixa latência tem prioridade sobre o do modo.
        let cpus = std::mem::take(&mut self.lowlat.cpus);
        self.lowlat = LowLatency::default();
        self.lowlat.set(cpus, &self.machine, topo);
        for e in &errs {
            eprintln!("[numapilot] máquina: {e}");
        }
        errs
    }

    fn board_tick(&mut self, topo: &Topology) {
        if self.board.map.is_empty() && self.board_manual.is_empty() {
            return;
        }
        let cpu_max = crate::cpuctl::package_temps()
            .values()
            .copied()
            .fold(None, |a: Option<f64>, t| Some(a.map_or(t, |a| a.max(t))));
        for f in &self.board_fans {
            let c = self.board.map.get(&f.id).cloned().unwrap_or_default();
            match fans::target(&c, fans::source_temp(&c.source, topo), cpu_max) {
                Some(pct) => {
                    let prev = self.board_manual.get(&f.id).copied();
                    if prev.is_none_or(|p| pct > p || p - pct >= 3.0) {
                        match fans::set_pct(f, pct) {
                            Ok(()) => {
                                self.board_manual.insert(f.id.clone(), pct);
                            }
                            Err(e) => eprintln!("[numapilot] ventoinha {}: {e}", f.id),
                        }
                    }
                }
                None => {
                    if self.board_manual.remove(&f.id).is_some() {
                        let _ = fans::set_auto(f, self.board_orig.get(&f.id).copied().flatten());
                    }
                }
            }
        }
    }

    /// Perfil de energia de cada GPU conforme os programas que a usam agora.
    fn gpu_profiles_tick(&mut self, topo: &Topology, procs: &HashMap<i32, Tracked>) {
        let mut want: BTreeMap<String, String> = BTreeMap::new();
        for t in procs.values() {
            let Some(p) = t.profile.as_ref().filter(|p| p.gpu_power != "auto") else {
                continue;
            };
            for a in &t.gpus {
                if let Some(g) = topo.gpus.iter().find(|g| g.alias == *a) {
                    // compute ganha de 3d se dois programas disputarem a mesma placa
                    let e = want
                        .entry(g.pci.clone())
                        .or_insert_with(|| p.gpu_power.clone());
                    if p.gpu_power == "compute" {
                        *e = p.gpu_power.clone();
                    }
                }
            }
        }
        for (pci, prof) in &want {
            let name = gpuctl::profile_driver_name(prof).unwrap_or("");
            let active = gpuctl::power_profiles(pci).and_then(|(l, a)| {
                a.and_then(|a| l.into_iter().find(|(i, _)| *i == a).map(|x| x.1))
            });
            if active.as_deref() != Some(name) {
                match gpuctl::set_power_profile(pci, prof) {
                    Ok(()) => eprintln!("[numapilot] GPU {pci}: perfil de energia {name}"),
                    Err(e) => eprintln!("[numapilot] GPU {pci}: perfil de energia: {e}"),
                }
            }
        }
        for pci in self.gpu_prof.keys() {
            if !want.contains_key(pci) {
                let _ = gpuctl::set_power_profile(pci, "padrao");
            }
        }
        self.gpu_prof = want;
    }

    fn lowlat_tick(&mut self, topo: &Topology, procs: &HashMap<i32, Tracked>) {
        let mut want: Vec<usize> = procs
            .iter()
            .filter(|(_, t)| {
                t.profile.as_ref().is_some_and(|p| p.low_latency)
                    && (t.state == St::Placed || t.origin == "manual")
            })
            .flat_map(|(pid, t)| {
                if t.cpus.is_empty() {
                    procfs::allowed_cpus(*pid).unwrap_or_default()
                } else {
                    t.cpus.clone()
                }
            })
            .collect();
        want.sort_unstable();
        want.dedup();
        if want.len() == topo.all_cpus().len() {
            // Programa sem núcleos definidos: não desliga a economia da máquina inteira.
            want.clear();
        }
        self.lowlat.set(want, &self.machine, topo);
    }

    fn restore(&mut self, topo: &Topology) {
        self.lowlat.set(vec![], &self.machine, topo);
        let prev = std::mem::take(&mut self.machine);
        let normal = Machine {
            game_latency_us: prev.game_latency_us,
            ..Default::default()
        };
        let _ = machine::apply(&normal, Some(&prev), &self.orig, topo, &base_governors());
        for f in &self.board_fans {
            if self.board_manual.contains_key(&f.id) {
                let _ = fans::set_auto(f, self.board_orig.get(&f.id).copied().flatten());
            }
        }
        for pci in self.gpu_prof.keys() {
            let _ = gpuctl::set_power_profile(pci, "padrao");
        }
    }
}
static STATUS: std::sync::Mutex<Option<Status>> = std::sync::Mutex::new(None);

fn status_update(f: impl FnOnce(&mut Status)) {
    if let Ok(mut g) = STATUS.lock() {
        let st = g.get_or_insert_with(Status::default);
        f(st);
        let _ = std::fs::write(
            history::status_path(),
            serde_json::to_string_pretty(st).unwrap_or_default(),
        );
    }
}

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
    /// CPUs exatos (quando o perfil escolhe núcleos); vazio = todos os dos nós.
    cpus: Vec<usize>,
    /// Perfil aplicado a este processo.
    profile: Option<Profile>,
    /// Memória por nó (MB), medida pelo numa_maps.
    mem: Vec<u64>,
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
    /// Memória do processo em cada nó NUMA (MB), índice = nó.
    pub mem: Vec<u64>,
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
    fn nodes_for(
        &self,
        cpu: &CpuSpec,
        gpu: &GpuSpec,
        want: Option<Kind>,
        hint: bool,
    ) -> Option<Vec<usize>> {
        match cpu {
            CpuSpec::Auto => None,
            CpuSpec::All => Some(self.topo.node_ids()),
            CpuSpec::Nodes(n) => Some(
                n.iter()
                    .copied()
                    .filter(|x| self.topo.node(*x).is_some())
                    .collect(),
            ),
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

    fn apply(
        &self,
        pid: i32,
        t: &mut Tracked,
        nodes: Vec<usize>,
        migrate: bool,
        why: &str,
        tick: u64,
    ) {
        let cpus = self.topo.cpus_of(&nodes);
        self.apply_cpus(pid, t, cpus, migrate, why, tick);
    }

    fn apply_cpus(
        &self,
        pid: i32,
        t: &mut Tracked,
        cpus: Vec<usize>,
        migrate: bool,
        why: &str,
        tick: u64,
    ) {
        let nodes = self.topo.nodes_of(&cpus);
        let migrate = migrate && t.profile.as_ref().is_none_or(|p| p.memory != "free");
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
        t.cpus = cpus;
        t.state = St::Placed;
        t.reapply_at = Some(tick + 2);
    }

    /// CPUs que o perfil pede (None = deixar no automático).
    fn plan_cpus(&self, p: &Profile, current_nodes: &[usize]) -> Option<Vec<usize>> {
        if let Some(l) = &p.cpu_list {
            let v: Vec<usize> = crate::util::parse_cpulist(l)
                .into_iter()
                .filter(|c| self.all_cpus.contains(c))
                .collect();
            if !v.is_empty() {
                return Some(v);
            }
        }
        if !p.has_cpu_choice() {
            return None;
        }
        let nodes = self.profile_nodes(p).unwrap_or_else(|| {
            if current_nodes.is_empty() {
                self.topo.node_ids()
            } else {
                current_nodes.to_vec()
            }
        });
        let v = self.topo.select_cpus(&nodes, p.cores, p.smt, &[]);
        if v.is_empty() { None } else { Some(v) }
    }

    /// Prioridade e grupo de controle (memória, núcleos exclusivos) de um programa.
    fn apply_extras(&self, pids: &[i32], t: &Tracked, p: &Profile, prev: Option<&Profile>) {
        let prev_prio = prev.map(|x| x.priority.as_str()).unwrap_or("normal");
        if p.priority != "normal" || prev_prio != "normal" {
            let (nice, io) = procfs::priority_values(&p.priority);
            for &pid in pids {
                procfs::set_nice_all(pid, nice);
                procfs::set_ioprio_all(pid, io);
            }
        }
        let scope = cgroup::scope_name(&t.pkey);
        if p.needs_scope() || cgroup::unit_active(&scope) {
            let cpus = if t.cpus.is_empty() {
                self.topo.cpus_of(&t.nodes)
            } else {
                t.cpus.clone()
            };
            let limits = Limits {
                cpus: (p.has_cpu_choice() && !cpus.is_empty()).then(|| cpus.clone()),
                mem_nodes: (p.memory == "strict" && !cpus.is_empty())
                    .then(|| self.topo.nodes_of(&cpus)),
                mem_max_bytes: p.mem_max_gb.map(|g| (g * 1073741824.0) as u64),
            };
            match cgroup::apply(&t.pkey, &t.label, pids, &limits) {
                Ok(name) => {
                    eprintln!("[numapilot] {} → {name} {:?}", t.comm, limits);
                    status_update(|s| s.cgroup_ok_count += 1);
                }
                Err(e) => {
                    eprintln!(
                        "[numapilot] {}: não consegui aplicar o grupo de controle: {e}",
                        t.comm
                    );
                    status_update(|s| s.cgroup_last_error = Some(e.clone()));
                }
            }
        }
    }

    /// Aplica (ao vivo) a escolha de CPU de um perfil num processo em execução.
    fn apply_profile_live(&self, pid: i32, t: &mut Tracked, p: &Profile, tick: u64) {
        t.profile = if p.is_auto() { None } else { Some(p.clone()) };
        let cur = t.nodes.clone();
        match self.plan_cpus(p, &cur) {
            Some(cpus) => {
                t.fixed = true;
                t.origin = "perfil".into();
                self.apply_cpus(pid, t, cpus, true, "escolha feita na interface", tick);
            }
            None => {
                // Voltou para o automático: libera e deixa o daemon decidir de novo.
                procfs::set_affinity_all(pid, &self.all_cpus);
                t.fixed = false;
                t.origin = "auto".into();
                t.state = St::Watching;
                t.active = 0;
                t.nodes.clear();
                t.cpus.clear();
                eprintln!(
                    "[numapilot] {} (pid {pid}) → automático (escolha feita na interface)",
                    t.comm
                );
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
            eprintln!(
                "[numapilot] aviso: sem socket de controle ({}): {e}",
                p.display()
            );
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
    // Valores originais antes de qualquer mudança (para devolver depois).
    let mut hw = Hw::new(&topo);
    let listener = bind_ctl();
    let mut learned = Learned::load();
    let mut profiles = Profiles::load();
    let mut gpuc = GpuControls::load();
    let mut fan_last: HashMap<String, f64> = HashMap::new();
    for (pci, c) in &gpuc.map {
        if let Some(w) = c.power_cap_w {
            match gpuctl::set_power_cap(pci, Some(w)) {
                Ok(v) => eprintln!("[numapilot] GPU {pci}: limite de potência {v:.0} W"),
                Err(e) => eprintln!("[numapilot] GPU {pci}: não consegui limitar a potência: {e}"),
            }
        }
    }
    if let Ok(s) = std::fs::read_to_string(governors_path())
        && let Ok(m) = serde_json::from_str::<BTreeMap<usize, String>>(&s)
    {
        for (node, gov) in m {
            let _ = write_governor(&ctx, node, &gov);
        }
    }
    if hw.machine != Machine::default() {
        let errs = hw.apply_machine(&topo, None);
        eprintln!(
            "[numapilot] modo da máquina: {} ({} erro(s))",
            hw.machine.mode,
            errs.len()
        );
    }
    let reserved_flag = run_dir().join("reserved");
    if reserved_flag.exists() {
        // O daemon anterior parou com núcleos reservados: devolve tudo.
        let _ = cgroup::set_reserved(&ctx.all_cpus, &[]);
        let _ = std::fs::remove_file(&reserved_flag);
    }
    let mut reserved_now: Vec<usize> = vec![];
    if cfg.daemon.gpu_irq {
        gpu_irqs(&ctx);
    }
    for (pci, c) in &gpuc.map {
        if let Err(e) = nv_offsets(pci, c) {
            eprintln!("[numapilot] GPU {pci}: deslocamento de clock não aplicado: {e}");
        }
        if let Some(m) = c.clock_max_mhz
            && let Err(e) = if crate::nvctl::is_nvidia(pci) {
                crate::nvctl::set_clock_max(pci, Some(m))
            } else {
                gpuctl::set_intel_max_freq(pci, Some(m))
            }
        {
            eprintln!("[numapilot] GPU {pci}: clock máximo não aplicado: {e}");
        }
        if c.fan != FanMode::Auto
            && gpuctl::read_sensors(pci).fw_fan_curve
            && let Err(e) = gpuctl::apply_fw_fan(pci, c)
        {
            eprintln!("[numapilot] GPU {pci}: curva da ventoinha não aplicada: {e}");
        }
        if let Some(od) = &c.od
            && let Err(e) = gpuctl::apply_od(pci, Some(od))
        {
            eprintln!("[numapilot] GPU {pci}: clock/voltagem não aplicados: {e}");
            status_update(|s| s.od_errors.push(format!("{pci}: {e}")));
        }
    }
    let mut hist = History::load().unwrap_or_default();
    hist.gpus = topo.gpus.iter().map(|g| g.alias.clone()).collect();
    hist.nodes = topo.node_ids();
    let mut last_hist = Instant::now() - Duration::from_secs(60);
    let execs = procevents::start();
    status_update(|s| {
        s.version = env!("CARGO_PKG_VERSION").into();
        s.root = is_root();
        s.proc_connector = execs.is_some();
        s.started = history::now();
    });
    eprintln!(
        "[numapilot] detecção de programas: {}",
        if execs.is_some() {
            "instantânea (proc connector)"
        } else {
            "por varredura"
        }
    );
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
                    serve(
                        s,
                        &ctx,
                        &mut procs,
                        &mut profiles,
                        &mut gpuc,
                        &mut fan_last,
                        &mut hw,
                        tick,
                    );
                }
            }
            if let Some(rx) = &execs {
                while let Ok(pid) = rx.try_recv() {
                    if pid == me {
                        continue;
                    }
                    let Some(st) = procfs::read_stat(pid) else {
                        continue;
                    };
                    let known = procs
                        .get(&pid)
                        .is_some_and(|t| t.starttime == st.starttime && t.comm == st.comm);
                    if !st.is_kthread() && !known {
                        admit(&ctx, &learned, &profiles, &mut procs, pid, &st, tick);
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(25));
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
            let Some(st) = procfs::read_stat(pid) else {
                continue;
            };
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
                admit(&ctx, &learned, &profiles, &mut procs, pid, &st, tick);
                continue;
            }

            let reserved = reserved_nodes(&procs);
            let t = procs.get_mut(&pid).unwrap();
            if pins.contains(&Pin {
                pid,
                starttime: st.starttime,
            }) {
                t.state = St::Ignored("pin manual (numapilot pin)".into());
                continue;
            }
            let delta = st.cpu_ticks.saturating_sub(t.last_ticks) as f64;
            t.last_ticks = st.cpu_ticks;
            t.cores = delta / tck / dt;
            // GPUs em uso (para a interface), a cada 5 amostras.
            let visible = !matches!(&t.state, St::Ignored(r) if !r.starts_with("manual"));
            if visible && tick.is_multiple_of(10) && (t.state == St::Placed || t.cores >= 0.05) {
                let m = procfs::mem_by_node(pid);
                let max = topo.nodes.iter().map(|n| n.id).max().unwrap_or(0);
                t.mem = (0..=max)
                    .map(|n| m.get(&n).copied().unwrap_or(0) / 1048576)
                    .collect();
            }
            if visible && tick.is_multiple_of(5) {
                let drm = procfs::open_drm(pid);
                let mut g: Vec<String> = drm
                    .iter()
                    .filter_map(|r| topo.gpu_by_render(r))
                    .map(|g| g.alias.clone())
                    .collect();
                g.sort();
                g.dedup();
                t.gpus = g;
            }
            if matches!(t.state, St::Ignored(_)) {
                continue;
            }
            step(&ctx, &mut learned, pid, t, &loads, &reserved, tick);
        }

        for (pci, c) in &gpuc.map {
            gpu_tick(pci, c, &mut fan_last);
        }
        // O deslocamento da NVIDIA se perde quando a placa do notebook dorme e acorda:
        // confere de tempos em tempos (só com ela acordada, para não acordá-la).
        if tick.is_multiple_of(10) {
            for (pci, c) in &gpuc.map {
                if (c.nv_gpc_offset_mhz.is_some() || c.nv_mem_offset_mhz.is_some())
                    && crate::nvctl::is_nvidia(pci)
                    && !crate::nvctl::asleep(pci)
                    && let Ok(o) = crate::nvml::offsets(pci)
                    && (c.nv_gpc_offset_mhz.is_some_and(|v| {
                        Some(crate::nvml::clamp(v, o.gpc_range, 300)) != o.gpc_mhz
                    }) || c.nv_mem_offset_mhz.is_some_and(|v| {
                        Some(crate::nvml::clamp(v, o.mem_range, 1500)) != o.mem_mhz
                    }))
                {
                    match nv_offsets(pci, c) {
                        Ok(()) => {
                            eprintln!("[numapilot] GPU {pci}: deslocamento de clock reaplicado")
                        }
                        Err(e) => eprintln!("[numapilot] GPU {pci}: deslocamento de clock: {e}"),
                    }
                }
            }
        }
        hw.board_tick(&topo);
        if tick.is_multiple_of(3) {
            hw.power_source_tick(&topo);
        }
        hw.lowlat_tick(&topo, &procs);
        if tick.is_multiple_of(5) {
            hw.gpu_profiles_tick(&topo, &procs);
        }
        if tick.is_multiple_of(15) {
            hw.ryzen_tick();
        }
        // Saúde do hardware (SMART, ECC, bateria) a cada 30 min, em segundo plano.
        // (a primeira 1 min depois de iniciar, para não atrasar o boot)
        let due = match hw.health_at {
            None => hw.started.elapsed() >= Duration::from_secs(60),
            Some(t) => t.elapsed() >= Duration::from_secs(30 * 60),
        };
        if due {
            hw.health_at = Some(Instant::now());
            let t = topo.clone();
            std::thread::spawn(move || {
                let r = crate::health::collect(&t);
                for (lvl, p) in crate::health::problems(&r) {
                    if lvl >= 2 {
                        eprintln!("[numapilot] saúde: {p}");
                    }
                }
                let tmp = crate::health::path().with_extension("json.tmp");
                if std::fs::write(&tmp, serde_json::to_string(&r).unwrap_or_default()).is_ok() {
                    let _ = std::fs::rename(tmp, crate::health::path());
                }
            });
        }
        if tick.is_multiple_of(2) {
            let p = hw.power.sample();
            if !p.is_empty() {
                hw.cpu_power = p;
            }
            let live = serde_json::json!({
                "power": hw.cpu_power,
                "temp": crate::cpuctl::package_temps(),
                "lowlat": format_cpulist(&hw.lowlat.cpus),
                "gpu_profiles": hw.gpu_prof,
                "board_fans": hw.board_manual,
                "ryzen": hw.ryzen_info,
            });
            let _ = std::fs::write(run_dir().join("hw.json"), live.to_string());
        }
        // Núcleos exclusivos: união dos programas com "exclusivo" rodando agora.
        let mut want: Vec<usize> = procs
            .values()
            .filter(|t| t.state == St::Placed && t.profile.as_ref().is_some_and(|p| p.exclusive))
            .flat_map(|t| t.cpus.iter().copied())
            .collect();
        want.sort_unstable();
        want.dedup();
        if want != reserved_now {
            match cgroup::set_reserved(&ctx.all_cpus, &want) {
                Ok(()) => {
                    if want.is_empty() {
                        let _ = std::fs::remove_file(&reserved_flag);
                        eprintln!("[numapilot] núcleos exclusivos liberados");
                    } else {
                        let _ = std::fs::write(&reserved_flag, format_cpulist(&want));
                        eprintln!(
                            "[numapilot] núcleos {} reservados (o resto do sistema não usa)",
                            format_cpulist(&want)
                        );
                    }
                    reserved_now = want;
                }
                Err(e) => {
                    eprintln!("[numapilot] não consegui reservar núcleos: {e}");
                    reserved_now = want;
                }
            }
        }
        if cfg.daemon.gpu_irq && tick.is_multiple_of(60) {
            gpu_irqs(&ctx);
        }
        if last_hist.elapsed() >= Duration::from_secs(30) {
            last_hist = Instant::now();
            let g = topo
                .gpus
                .iter()
                .map(|g| {
                    let s = gpuctl::read_sensors(&g.pci);
                    [
                        s.control_temp().unwrap_or(0.0) as f32,
                        s.power_w.unwrap_or(0.0) as f32,
                        s.fan_pct.unwrap_or(0.0) as f32,
                        g.busy_percent().unwrap_or(0) as f32,
                    ]
                })
                .collect();
            let n = topo
                .nodes
                .iter()
                .map(|n| loads.get(&n.id).copied().unwrap_or(0.0) as f32)
                .collect();
            let temps = crate::cpuctl::package_temps();
            let pkg =
                |n: &crate::topo::Node| n.sockets.iter().next().copied().unwrap_or(n.id as u32);
            let p = topo
                .nodes
                .iter()
                .map(|n| hw.cpu_power.get(&pkg(n)).copied().unwrap_or(0.0) as f32)
                .collect();
            let ct = topo
                .nodes
                .iter()
                .map(|n| temps.get(&pkg(n)).copied().unwrap_or(0.0) as f32)
                .collect();
            hist.push(Sample {
                t: history::now(),
                g,
                n,
                p,
                ct,
            });
            let _ = hist.save();
            let r = format_cpulist(&reserved_now);
            status_update(|s| s.reserved = r);
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
    if !reserved_now.is_empty() {
        let _ = cgroup::set_reserved(&ctx.all_cpus, &[]);
        let _ = std::fs::remove_file(&reserved_flag);
    }
    hw.restore(&topo);
    let _ = std::fs::remove_file(run_dir().join("hw.json"));
    // Ventoinhas voltam para o controle da placa.
    for pci in fan_last.keys() {
        let _ = gpuctl::set_fan_auto(pci);
    }
    for (pci, c) in &gpuc.map {
        if c.fan != FanMode::Auto && gpuctl::read_sensors(pci).fw_fan_curve {
            let _ = gpuctl::apply_fw_fan_curve(pci, None);
        }
        if c.nv_gpc_offset_mhz.is_some_and(|v| v != 0) {
            let _ = crate::nvml::set_gpc_offset(pci, 0);
        }
        if c.nv_mem_offset_mhz.is_some_and(|v| v != 0) {
            let _ = crate::nvml::set_mem_offset(pci, 0);
        }
    }
    let _ = learned.save();
    let _ = std::fs::remove_file(state_path());
    let _ = std::fs::remove_file(ctl::sock_path());
    eprintln!("[numapilot] daemon encerrado.");
    Ok(())
}

/// Atende um pedido da interface gráfica / CLI.
#[allow(clippy::too_many_arguments)]
fn serve(
    s: UnixStream,
    ctx: &Ctx,
    procs: &mut HashMap<i32, Tracked>,
    profiles: &mut Profiles,
    gpuc: &mut GpuControls,
    fan_last: &mut HashMap<String, f64>,
    hw: &mut Hw,
    tick: u64,
) {
    let _ = s.set_nonblocking(false);
    let _ = s.set_read_timeout(Some(Duration::from_secs(1)));
    let uid = ctl::peer_uid(&s);
    let mut line = String::new();
    let mut reader = BufReader::new(&s);
    let resp = match (reader.read_line(&mut line), uid) {
        (Ok(_), Some(uid)) => match serde_json::from_str::<Request>(line.trim()) {
            Ok(Request::SetGpuControl {
                pci,
                control,
                trial,
            }) => handle_gpu(
                uid,
                ctx,
                gpuc,
                fan_last,
                &mut hw.gpu_trial,
                pci,
                control,
                trial,
            ),
            Ok(Request::SetMachine { machine }) => handle_machine(uid, ctx, hw, machine),
            Ok(Request::SetBoardFan { id, control }) => handle_board_fan(uid, ctx, hw, id, control),
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
        Request::Adopt { pid, profile } => {
            return handle_adopt(uid, ctx, procs, profiles, pid, profile);
        }
        Request::FixMemory { key } => return handle_fix_memory(uid, ctx, procs, &key),
        Request::SetGovernor { node, governor } => {
            return handle_governor(uid, ctx, node, &governor);
        }
        Request::SetGpuControl { .. }
        | Request::SetMachine { .. }
        | Request::SetBoardFan { .. } => {
            return Response::err("pedido no lugar errado");
        }
        Request::SetProfile { key, profile, pid } => (key, profile, pid),
        Request::DeleteProfile { key, pid } => (key, Profile::default(), pid),
    };
    if uid != 0 && uid < ctx.cfg.daemon.min_uid {
        return Response::err("sem permissão");
    }
    if let Err(e) = profile.validate() {
        return Response::err(e);
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
    let mut pids = Vec::new();
    let mut sample: Option<(Tracked, Option<Profile>)> = None;
    for (p, t) in procs.iter_mut() {
        let mine = uid == 0 || t.uid == uid;
        let matches_key = t.pkey == key || Some(*p) == pid;
        if mine && matches_key && !matches!(&t.state, St::Ignored(r) if !r.starts_with("manual")) {
            let prev = t.profile.clone();
            ctx.apply_profile_live(*p, t, &profile, tick);
            pids.push(*p);
            if sample.is_none() {
                sample = Some((t.clone(), prev));
            }
            n += 1;
        }
    }
    // Prioridade e grupo de controle valem para o programa inteiro.
    if let Some((t, prev)) = sample {
        ctx.apply_extras(&pids, &t, &profile, prev.as_ref());
    }
    Response::ok(format!("perfil salvo; {n} processo(s) ajustado(s) agora"))
}

/// Classifica um processo novo e já aplica a decisão (regra, perfil ou aprendizado).
fn admit(
    ctx: &Ctx,
    learned: &Learned,
    profiles: &Profiles,
    procs: &mut HashMap<i32, Tracked>,
    pid: i32,
    st: &Stat,
    tick: u64,
) {
    let parent = procs.get(&st.ppid).cloned();
    let mut t = classify(ctx, learned, profiles, pid, st, parent.as_ref());
    if t.state == St::Placed {
        let cpus = if t.cpus.is_empty() {
            ctx.topo.cpus_of(&t.nodes)
        } else {
            t.cpus.clone()
        };
        let why = t.origin.clone();
        ctx.apply_cpus(pid, &mut t, cpus, true, &why, tick);
    }
    if !matches!(&t.state, St::Ignored(r) if !r.starts_with("manual"))
        && let Some(p) = t.profile.clone()
        && (p.needs_daemon() || p.priority != "normal")
    {
        ctx.apply_extras(&[pid], &t, &p, None);
    } else if let (St::Watching, Some(n), true) = (
        &t.state,
        learned.get(&t.key).and_then(|e| e.gpu_node),
        ctx.topo.nodes.len() > 1,
    ) {
        // Já sabemos que este programa usa a GPU do nó n: começa lá (first touch local).
        ctx.apply(
            pid,
            &mut t,
            vec![n],
            true,
            "aprendido: usa a GPU deste socket",
            tick,
        );
        t.origin = "aprendido".into();
    }
    procs.insert(pid, t);
}

/// Aplica ventoinha/potência de uma GPU conforme a configuração salva.
fn gpu_tick(pci: &str, c: &gpuctl::GpuControl, last: &mut HashMap<String, f64>) {
    let s = gpuctl::read_sensors(pci);
    if !s.fan_controllable {
        return;
    }
    match c.target_pct(s.control_temp()) {
        Some(pct) => {
            let prev = last.get(pci).copied();
            // Sobe na hora; desce só com 3% de diferença (evita a ventoinha "caçando").
            if prev.is_none_or(|p| pct > p || p - pct >= 3.0 || s.fan_mode_raw != Some(1)) {
                if let Err(e) = gpuctl::set_fan_pct(pci, pct) {
                    eprintln!("[numapilot] ventoinha {pci}: {e}");
                } else {
                    last.insert(pci.to_string(), pct);
                }
            }
        }
        None => {
            if c.fan == FanMode::Auto && last.remove(pci).is_some() {
                let _ = gpuctl::set_fan_auto(pci);
            }
        }
    }
}

/// O `numapilot run` pede, antes do exec, o que só o root faz (cgroup, prioridade alta).
fn handle_adopt(
    uid: u32,
    ctx: &Ctx,
    procs: &mut HashMap<i32, Tracked>,
    profiles: &Profiles,
    pid: i32,
    given: Option<Profile>,
) -> Response {
    if uid != 0 && procfs::uid(pid) != Some(uid) {
        return Response::err("esse processo é de outro usuário");
    }
    let Some(st) = procfs::read_stat(pid) else {
        return Response::err("processo não existe");
    };
    let learned = Learned::default();
    let mut t = classify(ctx, &learned, profiles, pid, &st, None);
    let Some(p) = given.or_else(|| profiles.get(&t.pkey).cloned()) else {
        return Response::ok("sem perfil");
    };
    if let Err(e) = p.validate() {
        return Response::err(e);
    }
    t.profile = Some(p.clone());
    t.cpus = procfs::allowed_cpus(pid).unwrap_or_default();
    t.nodes = ctx.topo.nodes_of(&t.cpus);
    ctx.apply_extras(&[pid], &t, &p, None);
    procs.insert(pid, t);
    Response::ok("perfil aplicado")
}

/// Traz de volta a memória que ficou no socket errado.
fn handle_fix_memory(uid: u32, ctx: &Ctx, procs: &HashMap<i32, Tracked>, key: &str) -> Response {
    let all = ctx.topo.node_ids();
    let mut n = 0;
    for (pid, t) in procs
        .iter()
        .filter(|(_, t)| t.pkey == key && (uid == 0 || t.uid == uid))
    {
        let cpus = if t.cpus.is_empty() {
            procfs::allowed_cpus(*pid).unwrap_or_default()
        } else {
            t.cpus.clone()
        };
        let to = ctx.topo.nodes_of(&cpus);
        if to.is_empty() || to.len() == all.len() {
            continue;
        }
        let from: Vec<usize> = all.iter().copied().filter(|x| !to.contains(x)).collect();
        let pid = *pid;
        std::thread::spawn(move || {
            let _ = procfs::migrate_pages(pid, &from, &to);
        });
        n += 1;
    }
    if n == 0 {
        Response::ok("nada a mover (o programa usa todos os sockets)")
    } else {
        Response::ok(format!(
            "movendo a memória de {n} processo(s) para o socket certo"
        ))
    }
}

pub fn governors_path() -> std::path::PathBuf {
    crate::learned::state_dir().join("governors.json")
}

fn write_governor(ctx: &Ctx, node: usize, gov: &str) -> Result<usize, String> {
    let n = ctx.topo.node(node).ok_or("nó não existe")?;
    let mut ok = 0;
    for c in &n.cpus {
        let p = crate::util::sys(&format!(
            "devices/system/cpu/cpu{c}/cpufreq/scaling_governor"
        ));
        if std::fs::write(&p, gov).is_ok() {
            ok += 1;
        }
    }
    if ok == 0 {
        Err("não consegui mudar o governador (cpufreq indisponível?)".into())
    } else {
        Ok(ok)
    }
}

fn handle_governor(uid: u32, ctx: &Ctx, node: usize, gov: &str) -> Response {
    if uid != 0 && uid < ctx.cfg.daemon.min_uid {
        return Response::err("sem permissão");
    }
    if !gov.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || gov.is_empty() {
        return Response::err("governador inválido");
    }
    match write_governor(ctx, node, gov) {
        Ok(n) => {
            let mut m: BTreeMap<usize, String> = std::fs::read_to_string(governors_path())
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default();
            m.insert(node, gov.to_string());
            let _ = std::fs::create_dir_all(crate::learned::state_dir());
            let _ = std::fs::write(
                governors_path(),
                serde_json::to_string(&m).unwrap_or_default(),
            );
            Response::ok(format!("socket {node}: {gov} em {n} CPUs"))
        }
        Err(e) => Response::err(e),
    }
}

/// Interrupções de cada GPU atendidas pelos CPUs do socket dela.
fn gpu_irqs(ctx: &Ctx) {
    for g in &ctx.topo.gpus {
        let Some(n) = g.numa_node else { continue };
        let cpus = format_cpulist(&ctx.topo.cpus_of(&[n]));
        let dir = crate::util::sys(&format!("bus/pci/devices/{}/msi_irqs", g.pci));
        let irqs: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        for irq in irqs {
            let _ = std::fs::write(format!("{}/{irq}/smp_affinity_list", irq_root()), &cpus);
        }
    }
}

fn irq_root() -> String {
    std::env::var("NUMAPILOT_IRQ_ROOT").unwrap_or_else(|_| "/proc/irq".into())
}

#[allow(clippy::too_many_arguments)]
fn handle_gpu(
    uid: u32,
    ctx: &Ctx,
    gpuc: &mut GpuControls,
    fan_last: &mut HashMap<String, f64>,
    trials: &mut HashMap<String, gpuctl::GpuControl>,
    pci: String,
    mut control: gpuctl::GpuControl,
    trial: bool,
) -> Response {
    if uid != 0 && uid < ctx.cfg.daemon.min_uid {
        return Response::err("sem permissão");
    }
    if !ctx.topo.gpus.iter().any(|g| g.pci == pci) {
        return Response::err(format!("GPU {pci} não existe"));
    }
    if let Err(e) = control.validate() {
        return Response::err(e);
    }
    let s = gpuctl::read_sensors(&pci);
    let old = gpuc.get(&pci);
    if s.fw_fan_curve
        && (control.fan != old.fan
            || control.curve != old.curve
            || control.fixed_pct != old.fixed_pct)
    {
        match gpuctl::apply_fw_fan(&pci, &control) {
            Ok(()) => {}
            Err(e) => return Response::err(format!("ventoinha: {e}")),
        }
    } else if control.fan != FanMode::Auto && !s.fan_controllable {
        return Response::err("esta placa não permite controlar a ventoinha (sem pwm1 no hwmon)");
    }
    let mut msg = Vec::new();
    if control.power_cap_w != old.power_cap_w {
        match gpuctl::set_power_cap(&pci, control.power_cap_w) {
            Ok(v) => msg.push(format!("potência {v:.0} W")),
            Err(e) => return Response::err(format!("não consegui mudar a potência: {e}")),
        }
    }
    if control.clock_max_mhz != old.clock_max_mhz {
        let r = if crate::nvctl::is_nvidia(&pci) {
            crate::nvctl::set_clock_max(&pci, control.clock_max_mhz)
        } else if gpuctl::intel_freq(&pci).is_some() {
            gpuctl::set_intel_max_freq(&pci, control.clock_max_mhz)
        } else {
            return Response::err(
                "travar o clock por aqui é para NVIDIA e Intel; nas AMD use clock/voltagem",
            );
        };
        match r {
            Ok(()) => msg.push(match control.clock_max_mhz {
                Some(m) => format!("clock até {m} MHz"),
                None => "clock normal".into(),
            }),
            Err(e) => return Response::err(format!("clock: {e}")),
        }
    }
    if (control.nv_gpc_offset_mhz, control.nv_mem_offset_mhz)
        != (old.nv_gpc_offset_mhz, old.nv_mem_offset_mhz)
    {
        if !crate::nvctl::is_nvidia(&pci) {
            return Response::err("deslocamento de clock por aqui é só para NVIDIA");
        }
        let mut c = control.clone();
        // o que foi tirado volta a 0
        if old.nv_gpc_offset_mhz.is_some() && c.nv_gpc_offset_mhz.is_none() {
            c.nv_gpc_offset_mhz = Some(0);
        }
        if old.nv_mem_offset_mhz.is_some() && c.nv_mem_offset_mhz.is_none() {
            c.nv_mem_offset_mhz = Some(0);
        }
        match nv_offsets(&pci, &c) {
            Ok(()) => msg.push(format!(
                "deslocamento GPU {:+} / memória {:+} MHz",
                control.nv_gpc_offset_mhz.unwrap_or(0),
                control.nv_mem_offset_mhz.unwrap_or(0)
            )),
            Err(e) => return Response::err(format!("deslocamento de clock: {e}")),
        }
    }
    if control.od != old.od {
        let od = control.od.as_ref().filter(|o| !o.is_default());
        match gpuctl::apply_od(&pci, od) {
            Ok(()) => msg.push(if od.is_some() {
                "clock/voltagem aplicados".into()
            } else {
                "clock/voltagem de fábrica".into()
            }),
            Err(e) => return Response::err(format!("clock/voltagem: {e}")),
        }
        if od.is_none() {
            control.od = None;
        }
    }
    if control.fan == FanMode::Auto && old.fan != FanMode::Auto {
        let _ = gpuctl::set_fan_auto(&pci);
        fan_last.remove(&pci);
        msg.push("ventoinha automática".into());
    } else if control.fan != FanMode::Auto {
        fan_last.remove(&pci); // força reaplicar no próximo tick
        msg.push(match control.fan {
            FanMode::Fixed => format!("ventoinha fixa em {:.0}%", control.fixed_pct),
            _ => "ventoinha pela curva".to_string(),
        });
    }
    if trial {
        trials.entry(pci.clone()).or_insert_with(|| old.clone());
    } else {
        trials.remove(&pci);
    }
    if control == gpuctl::GpuControl::default() {
        gpuc.map.remove(&pci);
    } else {
        gpuc.map.insert(pci.clone(), control);
    }
    // No disco, as placas em teste ficam com o último valor bom.
    let mut disk = GpuControls {
        map: gpuc.map.clone(),
    };
    for (p, good) in trials.iter() {
        if *good == gpuctl::GpuControl::default() {
            disk.map.remove(p);
        } else {
            disk.map.insert(p.clone(), good.clone());
        }
    }
    if let Err(e) = disk.save() {
        return Response::err(format!("não consegui salvar: {e}"));
    }
    eprintln!("[numapilot] GPU {pci}: {}", msg.join(", "));
    Response::ok(if msg.is_empty() {
        "nada mudou".to_string()
    } else {
        msg.join(", ")
    })
}

fn handle_machine(uid: u32, ctx: &Ctx, hw: &mut Hw, m: Machine) -> Response {
    if uid != 0 && uid < ctx.cfg.daemon.min_uid {
        return Response::err("sem permissão");
    }
    if let Err(e) = m.validate() {
        return Response::err(e);
    }
    let prev = std::mem::replace(&mut hw.machine, m);
    if let Err(e) = hw.machine.save() {
        return Response::err(format!("não consegui salvar: {e}"));
    }
    let errs = hw.apply_machine(ctx.topo, Some(&prev));
    eprintln!("[numapilot] modo da máquina: {}", hw.machine.mode);
    if errs.is_empty() {
        Response::ok(format!("modo {} aplicado", hw.machine.mode))
    } else {
        Response::ok(format!("aplicado com avisos: {}", errs.join("; ")))
    }
}

fn handle_board_fan(
    uid: u32,
    ctx: &Ctx,
    hw: &mut Hw,
    id: String,
    mut c: fans::BoardFanControl,
) -> Response {
    if uid != 0 && uid < ctx.cfg.daemon.min_uid {
        return Response::err("sem permissão");
    }
    if !hw.board_fans.iter().any(|f| f.id == id) {
        return Response::err(format!("ventoinha {id} não existe"));
    }
    if let Err(e) = c.validate() {
        return Response::err(e);
    }
    let msg = match c.fan {
        FanMode::Auto => "automática (chip da placa)".to_string(),
        FanMode::Fixed => format!("fixa em {:.0}%", c.fixed_pct),
        FanMode::Curve => format!("pela curva ({})", c.source),
    };
    if c.fan == FanMode::Auto && c.label.is_empty() {
        hw.board.map.remove(&id);
    } else {
        hw.board.map.insert(id.clone(), c);
    }
    hw.board_manual.remove(&id);
    if let Some(f) = hw.board_fans.iter().find(|f| f.id == id)
        && hw.board.map.get(&id).is_none_or(|c| c.fan == FanMode::Auto)
    {
        let _ = fans::set_auto(f, hw.board_orig.get(&id).copied().flatten());
    }
    if let Err(e) = hw.board.save() {
        return Response::err(format!("não consegui salvar: {e}"));
    }
    eprintln!("[numapilot] ventoinha {id}: {msg}");
    Response::ok(format!("ventoinha {msg}"))
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
        cpus: vec![],
        profile: None,
        mem: vec![],
    };
    let ignore = |t: &mut Tracked, why: &str| t.state = St::Ignored(why.to_string());

    if uid < ctx.cfg.daemon.min_uid {
        ignore(&mut t, "processo do sistema");
        return t;
    }
    if ctx
        .cfg
        .daemon
        .ignore
        .iter()
        .any(|p| idents.iter().any(|i| glob_match(p, i)))
    {
        ignore(&mut t, "na lista ignore");
        return t;
    }
    let mode = procfs::env_var(pid, "NUMAPILOT_MODE");
    let allowed = procfs::allowed_cpus(pid).unwrap_or_default();
    if mode.as_deref() == Some("manual") {
        ignore(&mut t, "manual (numapilot run)");
        t.origin = "manual".into();
        t.profile = profiles.get(&pkey).cloned();
        t.cpus = allowed.clone();
        t.nodes = ctx
            .topo
            .nodes
            .iter()
            .filter(|n| n.cpus.iter().any(|c| allowed.contains(c)))
            .map(|n| n.id)
            .collect();
        return t;
    }
    let ours =
        same_set(&allowed, &ctx.all_cpus) || ctx.node_sets.iter().any(|s| same_set(s, &allowed));
    if !ours && mode.as_deref() != Some("auto") {
        ignore(&mut t, "afinidade definida por outro programa (taskset?)");
        return t;
    }

    // Perfil salvo pela interface gráfica tem prioridade sobre as regras.
    if let Some(p) = profiles.get(&pkey).filter(|p| !p.is_auto()) {
        t.profile = Some(p.clone());
        if let Some(cpus) = ctx.plan_cpus(p, &[]) {
            t.origin = "perfil".into();
            t.nodes = ctx.topo.nodes_of(&cpus);
            t.cpus = cpus;
            t.fixed = true;
            t.state = St::Placed;
            return t;
        }
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
        let cpus = if t.cpus.is_empty() {
            ctx.topo.cpus_of(&t.nodes)
        } else {
            t.cpus.clone()
        };
        procfs::set_affinity_all(pid, &cpus);
        t.reapply_at = None;
    }

    match t.state {
        St::Ignored(_) => {}
        St::Watching => {
            t.active = if t.cores >= d.watch_cores {
                t.active + 1
            } else {
                0
            };
            if !multi {
                return;
            }
            // Usa GPU? Então vai para o socket dessa GPU.
            if t.cores > 0.05 && tick.is_multiple_of(2) {
                let drm = procfs::open_drm(pid);
                let gpus: Vec<_> = drm
                    .iter()
                    .filter_map(|r| ctx.topo.gpu_by_render(r))
                    .collect();
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
            if t.nodes.len() == 1
                && t.gpu_node.is_none()
                && t.cores > 0.05
                && tick.is_multiple_of(10)
            {
                let drm = procfs::open_drm(pid);
                let gpus: Vec<_> = drm
                    .iter()
                    .filter_map(|r| ctx.topo.gpu_by_render(r))
                    .collect();
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
                if learned
                    .get(&t.key)
                    .is_none_or(|e| frac > e.peak_frac + 0.05)
                {
                    learned.update(&t.key, |e| e.peak_frac = frac);
                }
                t.hi = if frac >= d.promote_frac { t.hi + 1 } else { 0 };
                if t.hi >= d.promote_ticks {
                    let why = format!(
                        "saturou o nó ({:.0}% por {} amostras) → 2 sockets",
                        frac * 100.0,
                        t.hi
                    );
                    learned.update(&t.key, |e| e.wide = true);
                    t.hi = 0;
                    t.lo = 0;
                    ctx.apply(pid, t, ctx.topo.node_ids(), false, &why, tick);
                }
            } else {
                let per_node = t.cores / ctx.node_size(t.nodes[0]);
                t.lo = if per_node < d.demote_frac {
                    t.lo + 1
                } else {
                    0
                };
                if t.lo >= d.demote_ticks {
                    let n = t
                        .gpu_node
                        .unwrap_or_else(|| least_loaded_node(ctx.topo, loads, reserved));
                    let why = format!("uso baixo por {} amostras → volta para 1 socket", t.lo);
                    t.lo = 0;
                    ctx.apply(pid, t, vec![n], true, &why, tick);
                }
            }
        }
    }
}

fn write_state(
    ctx: &Ctx,
    procs: &HashMap<i32, Tracked>,
    ppids: &HashMap<i32, i32>,
    profiles: &Profiles,
) {
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
                    if t.nodes.len() > 1 {
                        "2 sockets".into()
                    } else {
                        "1 socket".into()
                    }
                }
                St::Watching => "observando".into(),
                St::Ignored(r) => r.clone(),
            },
            nodes: t
                .nodes
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(","),
            cpus: format_cpulist(&if t.cpus.is_empty() {
                ctx.topo.cpus_of(&t.nodes)
            } else {
                t.cpus.clone()
            }),
            mem: t.mem.clone(),
            cores: (t.cores * 10.0).round() / 10.0,
            gpus: t.gpus.join(","),
        })
        .collect();
    rows.sort_by(|a, b| {
        b.cores
            .partial_cmp(&a.cores)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let Ok(s) = serde_json::to_string(&rows) {
        let tmp = run_dir().join("state.json.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, state_path());
        }
    }
}

/// Aplica os deslocamentos de clock da NVIDIA (dentro da faixa que a placa aceita).
fn nv_offsets(pci: &str, c: &gpuctl::GpuControl) -> Result<(), String> {
    if c.nv_gpc_offset_mhz.is_none() && c.nv_mem_offset_mhz.is_none() {
        return Ok(());
    }
    if crate::nvctl::asleep(pci) {
        return Err("a placa está dormindo; aplico quando ela acordar".into());
    }
    let o = crate::nvml::offsets(pci)?;
    if let Some(v) = c.nv_gpc_offset_mhz {
        crate::nvml::set_gpc_offset(pci, crate::nvml::clamp(v, o.gpc_range, 300))?;
    }
    if let Some(v) = c.nv_mem_offset_mhz {
        crate::nvml::set_mem_offset(pci, crate::nvml::clamp(v, o.mem_range, 1500))?;
    }
    Ok(())
}
