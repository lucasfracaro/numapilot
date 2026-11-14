//! Automatic-mode daemon: watches processes, puts each one on the best socket
//! and expands to both sockets when the program saturates the node.
//! Also serves the graphical interface through the control socket (per-program profiles).

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
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static STOP: AtomicBool = AtomicBool::new(false);

/// State of the machine's hardware controls (other than the GPUs).
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
    /// Cards under an undervolt trial: the good value that stays on disk.
    gpu_trial: HashMap<String, gpuctl::GpuControl>,
    health_at: Option<Instant>,
    started: Instant,
    hot_streak: HashMap<String, u32>,
    energy: crate::energy::Energy,
    energy_at: Instant,
    energy_saved: Instant,
    watts: f64,
    /// Idle GPU: since when, and the level it had before we lowered it.
    idle: HashMap<String, (Option<Instant>, Option<String>)>,
    /// Mode set by the automatic rules (program/schedule) and the manual mode to go back to.
    auto_mode: Option<String>,
    manual_mode: Option<String>,
    /// The user switched the mode by hand while a rule was in effect: do not reapply the same rule.
    auto_suppressed: Option<String>,
    /// Laptop NVIDIA (modes switched with the session open).
    dgpu: Option<crate::hybrid::State>,
    /// NVIDIA services stopped so the card can be removed (they come back with it).
    dgpu_stopped: Vec<String>,
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
            hot_streak: HashMap::new(),
            energy: crate::energy::Energy::load(),
            energy_at: Instant::now(),
            energy_saved: Instant::now(),
            watts: 0.0,
            idle: HashMap::new(),
            auto_mode: None,
            manual_mode: None,
            auto_suppressed: None,
            dgpu: crate::hybrid::pair(topo)
                .map(|(_, nv)| crate::hybrid::State {
                    pci: nv.pci.clone(),
                    alias: nv.alias.clone(),
                    name: nv.name.clone(),
                    functions: crate::vm::functions(&nv.pci),
                    bridge: crate::hybrid::parent_bridge(&nv.pci).unwrap_or_default(),
                    ..Default::default()
                })
                .or_else(crate::hybrid::load_state),
            dgpu_stopped: vec![],
        }
    }

    /// Laptop: mode switch when plugging/unplugging AC power.
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
        if self.auto_mode.is_some() {
            // an automatic rule is in charge now; applies when it ends
            self.manual_mode = Some(name);
            return;
        }
        if name == self.machine.mode {
            return;
        }
        let game_node = topo.gaming_gpu().and_then(|g| g.numa_node);
        let m = Machine::preset(&name, topo, game_node, &self.machine);
        let prev = std::mem::replace(&mut self.machine, m);
        let _ = self.machine.save();
        self.apply_machine(topo, Some(&prev));
        crate::nlog!(
            "{}: modo {name}",
            if ac == Some(true) {
                crate::i18n::tr("on AC power")
            } else {
                crate::i18n::tr("on battery")
            }
        );
    }

    /// The laptop firmware resets the Ryzen limits when switching AC/battery or profile:
    /// check from time to time and reapply what was asked.
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
                Ok(()) => crate::nlog!("limites do Ryzen reaplicados"),
                Err(e) => crate::nlog!("limites do Ryzen: {e}"),
            }
        }
        self.ryzen_info = info;
    }

    /// Automatic rules: mode by running program and by schedule.
    fn auto_mode_tick(&mut self, topo: &Topology, procs: &HashMap<i32, Tracked>) {
        let progs: Vec<String> = procs
            .values()
            .filter(|t| t.state == St::Placed)
            .filter_map(|t| t.profile.as_ref().and_then(|p| p.machine_mode.clone()))
            .collect();
        let want = machine::auto_mode(&progs, &self.machine.schedule, machine::local_minute());
        if want != self.auto_suppressed {
            self.auto_suppressed = None;
        } else if want.is_some() {
            return;
        }
        if want == self.auto_mode {
            return;
        }
        let target = match &want {
            Some(m) => {
                if self.auto_mode.is_none() {
                    self.manual_mode = Some(self.machine.mode.clone());
                }
                m.clone()
            }
            None => self.manual_mode.take().unwrap_or_else(|| "normal".into()),
        };
        self.auto_mode = want;
        if target == self.machine.mode {
            return;
        }
        let game_node = topo.gaming_gpu().and_then(|g| g.numa_node);
        let m = Machine::preset(&target, topo, game_node, &self.machine);
        let prev = std::mem::replace(&mut self.machine, m);
        let _ = self.machine.save();
        self.apply_machine(topo, Some(&prev));
        crate::nlog!(
            "modo {target} ({})",
            if self.auto_mode.is_some() {
                crate::i18n::tr("automatic rule")
            } else {
                crate::i18n::tr("end of the rule, back to the previous one")
            }
        );
    }

    /// Laptop NVIDIA modes: removes or brings back the card depending on the mode and AC power.
    fn dgpu_tick(&mut self) {
        let Some(mut st) = self.dgpu.clone() else {
            return;
        };
        let here = crate::hybrid::present(&st.pci);
        let mode = self.machine.dgpu_mode.clone();
        let want = match &mode {
            Some(m) => crate::hybrid::want_present(m, crate::notebook::on_ac()),
            // "leave as is": if we were the ones who removed it, bring it back
            None => here || st.removed,
        };
        st.mode = mode.clone().unwrap_or_default();
        st.want_present = want;
        if want && !here {
            match crate::hybrid::rescan_at(&st.bridge) {
                Ok(()) => {
                    std::thread::sleep(Duration::from_millis(1500));
                    for svc in self.dgpu_stopped.drain(..) {
                        let _ = Command::new("systemctl").args(["start", &svc]).status();
                    }
                    st.removed = false;
                    st.last_error = None;
                    crate::nlog!("NVIDIA de volta ({})", st.alias);
                }
                Err(e) => st.last_error = Some(e),
            }
        } else if !want && here {
            // The NVIDIA services hold the card: they stop with it and come back when it returns.
            let svcs = ["nvidia-powerd", "nvidia-persistenced"];
            let holders = crate::notebook::dgpu_holders(&st.pci);
            let users: Vec<String> = {
                let mut u: Vec<String> = holders
                    .iter()
                    .filter(|h| {
                        h.client
                            && !crate::notebook::SYSTEM_HOLDERS.contains(&h.name.as_str())
                            && !crate::notebook::COMPOSITORS.contains(&h.name.as_str())
                    })
                    .map(|h| h.name.clone())
                    .filter(|n| !svcs.contains(&n.as_str()))
                    .collect();
                u.sort();
                u.dedup();
                u
            };
            // The desktop (KWin) with the NVIDIA's video output open: removing the card from the system
            // under these conditions is not safe (the driver waits for the desktop to let go). The card
            // sleeps by itself with no programs using it; that is what counts.
            let display_open = holders.iter().any(|h| {
                (!h.client || crate::notebook::COMPOSITORS.contains(&h.name.as_str()))
                    && !crate::notebook::SYSTEM_HOLDERS.contains(&h.name.as_str())
            });
            st.sleep_only = users.is_empty() && display_open;
            if st.sleep_only {
                st.blocked_by.clear();
            } else if users.is_empty() {
                for svc in svcs {
                    let active = Command::new("systemctl")
                        .args(["is-active", "--quiet", svc])
                        .status()
                        .is_ok_and(|s| s.success());
                    if active
                        && Command::new("systemctl")
                            .args(["stop", svc])
                            .status()
                            .is_ok_and(|s| s.success())
                    {
                        self.dgpu_stopped.push(svc.to_string());
                    }
                }
                match crate::hybrid::remove(&st.functions) {
                    Ok(()) => {
                        st.removed = true;
                        st.blocked_by.clear();
                        st.last_error = None;
                        crate::nlog!("NVIDIA off ({}): integrated only", st.alias);
                    }
                    Err(e) => st.last_error = Some(e),
                }
            } else {
                if st.blocked_by != users {
                    crate::nlog!("NVIDIA in use, not turned off: {}", users.join(", "));
                }
                st.blocked_by = users;
            }
        } else {
            st.blocked_by.clear();
            st.sleep_only = false;
        }
        crate::hybrid::save_state(&st);
        self.dgpu = Some(st);
    }

    fn apply_machine(&mut self, topo: &Topology, prev: Option<&Machine>) -> Vec<String> {
        let errs = machine::apply(&self.machine, prev, &self.orig, topo, &base_governors());
        // The uncore of low-latency programs takes priority over the mode's.
        let cpus = std::mem::take(&mut self.lowlat.cpus);
        self.lowlat = LowLatency::default();
        self.lowlat.set(cpus, &self.machine, topo);
        for e in &errs {
            crate::nlog!("machine: {e}");
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
                            Err(e) => crate::nlog!("fan {}: {e}", f.id),
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

    /// Power profile of each GPU according to the programs using it now.
    fn gpu_profiles_tick(&mut self, topo: &Topology, procs: &HashMap<i32, Tracked>) {
        let mut want: BTreeMap<String, String> = BTreeMap::new();
        for t in procs.values() {
            let Some(p) = t.profile.as_ref().filter(|p| p.gpu_power != "auto") else {
                continue;
            };
            for a in &t.gpus {
                if let Some(g) = topo.gpus.iter().find(|g| g.alias == *a) {
                    // compute wins over 3d if two programs compete for the same card
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
                    Ok(()) => crate::nlog!("GPU {pci}: power profile {name}"),
                    Err(e) => crate::nlog!("GPU {pci}: power profile: {e}"),
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
            // Program without defined cores: does not disable power saving for the whole machine.
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
    /// Learning key (executable/script).
    key: String,
    /// Profile key (steam:ID or executable).
    pkey: String,
    label: String,
    exe: Option<String>,
    uid: u32,
    origin: String,
    state: St,
    nodes: Vec<usize>,
    /// Fixed decision (rule or profile): automatic mode neither expands nor shrinks it.
    fixed: bool,
    reserve: bool,
    last_ticks: u64,
    /// When `last_ticks` was read. Each process is read at a different moment of the tick, so
    /// its usage is divided by its own interval, not by the loop's.
    last_at: Instant,
    cores: f64,
    active: u32,
    hi: u32,
    lo: u32,
    gpus: Vec<String>,
    gpu_node: Option<usize>,
    reapply_at: Option<u64>,
    /// Exact CPUs (when the profile picks cores); empty = all of the nodes'.
    cpus: Vec<usize>,
    /// Profile applied to this process.
    profile: Option<Profile>,
    /// Memory per node (MB), measured through numa_maps.
    mem: Vec<u64>,
}

/// Row of /run/numapilot/state.json (read by `status` and by the graphical interface).
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
    /// The process's memory on each NUMA node (MB), index = node.
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

    /// Nodes matching a CPU specification (from a rule or profile). None = automatic.
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
            t.state = St::Ignored(crate::i18n::tr("no permission to change the affinity").into());
            return;
        }
        let all = self.topo.node_ids();
        if migrate && self.cfg.daemon.migrate_memory && nodes.len() < all.len() {
            let from: Vec<usize> = all.iter().copied().filter(|x| !nodes.contains(x)).collect();
            let to = nodes.clone();
            // Migration can take a while on large processes: runs in the background.
            std::thread::spawn(move || {
                let _ = procfs::migrate_pages(pid, &from, &to);
            });
        }
        crate::nlog!(
            "{} (pid {pid}) → node(s) {:?} [{}]: {why}",
            t.comm,
            nodes,
            format_cpulist(&cpus)
        );
        t.nodes = nodes;
        t.cpus = cpus;
        t.state = St::Placed;
        t.reapply_at = Some(tick + 2);
    }

    /// CPUs the profile asks for (None = leave on automatic).
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

    /// Priority and control group (memory, exclusive cores) of a program.
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
                cpu_quota_pct: p.cpu_max_pct.map(|c| c.round() as u32),
            };
            match cgroup::apply(&t.pkey, &t.label, pids, &limits) {
                Ok(name) => {
                    crate::nlog!("{} → {name} {:?}", t.comm, limits);
                    status_update(|s| s.cgroup_ok_count += 1);
                }
                Err(e) => {
                    crate::nlog!("{}: could not apply the control group: {e}", t.comm);
                    status_update(|s| s.cgroup_last_error = Some(e.clone()));
                }
            }
        }
    }

    /// Applies (live) a profile's CPU choice to a running process.
    fn apply_profile_live(&self, pid: i32, t: &mut Tracked, p: &Profile, tick: u64) {
        t.profile = if p.is_auto() { None } else { Some(p.clone()) };
        let cur = t.nodes.clone();
        match self.plan_cpus(p, &cur) {
            Some(cpus) => {
                t.fixed = true;
                t.origin = "perfil".into();
                self.apply_cpus(
                    pid,
                    t,
                    cpus,
                    true,
                    crate::i18n::tr("choice made in the interface"),
                    tick,
                );
            }
            None => {
                // Back to automatic: release it and let the daemon decide again.
                procfs::set_affinity_all(pid, &self.all_cpus);
                t.fixed = false;
                t.origin = "auto".into();
                t.state = St::Watching;
                t.active = 0;
                t.nodes.clear();
                t.cpus.clear();
                crate::nlog!(
                    "{} (pid {pid}) → automatic (choice made in the interface)",
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
            // Any local user may talk to the daemon; permission is checked per request (SO_PEERCRED).
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o666));
            let _ = l.set_nonblocking(true);
            Some(l)
        }
        Err(e) => {
            crate::nlog!("warning: no control socket ({}): {e}", p.display());
            None
        }
    }
}

pub fn run(cfg: Config) -> Result<()> {
    if !is_root() {
        crate::nlog!("warning: without root only your own processes can be moved.");
    }
    unsafe {
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
    }
    let topo = Topology::load(&cfg);
    for w in topo.warnings() {
        crate::nlog!("warning: {w}");
    }
    if topo.nodes.len() < 2 {
        crate::nlog!("there is only 1 NUMA node: the daemon will only watch.");
    }
    let ctx = Ctx {
        cfg: &cfg,
        topo: &topo,
        all_cpus: topo.all_cpus(),
        node_sets: topo.nodes.iter().map(|n| n.cpus.clone()).collect(),
    };
    let _ = std::fs::create_dir_all(run_dir());
    // Original values before any change (to restore later).
    let mut hw = Hw::new(&topo);
    let listener = bind_ctl();
    let mut learned = Learned::load();
    let mut profiles = Profiles::load();
    let mut gpuc = GpuControls::load();
    let mut fan_last: HashMap<String, f64> = HashMap::new();
    for (pci, c) in &gpuc.map {
        if let Some(w) = c.power_cap_w {
            match gpuctl::set_power_cap(pci, Some(w)) {
                Ok(v) => crate::nlog!("GPU {pci}: power limit {v:.0} W"),
                Err(e) => crate::nlog!("GPU {pci}: could not limit the power: {e}"),
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
        if errs.is_empty() {
            crate::nlog!("machine mode: {} applied", hw.machine.mode);
        } else {
            crate::nlog!(
                "machine mode: {} with {} error(s): {}",
                hw.machine.mode,
                errs.len(),
                errs.join("; ")
            );
        }
    }
    let reserved_flag = run_dir().join("reserved");
    if reserved_flag.exists() {
        // The previous daemon stopped with reserved cores: give everything back.
        let _ = cgroup::set_reserved(&ctx.all_cpus, &[]);
        let _ = std::fs::remove_file(&reserved_flag);
    }
    let mut reserved_now: Vec<usize> = vec![];
    if cfg.daemon.gpu_irq {
        gpu_irqs(&ctx);
    }
    if cfg.daemon.device_irq {
        let n = crate::irqs::apply(&topo);
        if n > 0 {
            crate::nlog!("{n} disk/network interrupts on the card's socket");
        }
    }
    for (pci, c) in &gpuc.map {
        if let Err(e) = nv_offsets(pci, c) {
            crate::nlog!("GPU {pci}: clock offset not applied: {e}");
        }
        if let Some(m) = c.clock_max_mhz
            && let Err(e) = if crate::nvctl::is_nvidia(pci) {
                crate::nvctl::set_clock_max(pci, Some(m))
            } else {
                gpuctl::set_intel_max_freq(pci, Some(m))
            }
        {
            crate::nlog!("GPU {pci}: maximum clock not applied: {e}");
        }
        if c.fan != FanMode::Auto
            && gpuctl::read_sensors(pci).fw_fan_curve
            && let Err(e) = gpuctl::apply_fw_fan(pci, c)
        {
            crate::nlog!("GPU {pci}: fan curve not applied: {e}");
        }
        if let Some(od) = &c.od
            && let Err(e) = gpuctl::apply_od(pci, Some(od))
        {
            crate::nlog!("GPU {pci}: clock/voltage not applied: {e}");
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
    crate::nlog!(
        "program detection: {}",
        if execs.is_some() {
            crate::i18n::tr("instant (proc connector)")
        } else {
            crate::i18n::tr("by scanning")
        }
    );
    let tck = procfs::clk_tck();
    let me = std::process::id() as i32;
    let mut procs: HashMap<i32, Tracked> = HashMap::new();
    let mut prev_cpu = procfs::cpu_times();
    let mut last_save = Instant::now();
    let mut tick: u64 = 0;
    let tick_dur = Duration::from_millis(cfg.daemon.tick_ms);
    crate::nlog!(
        "daemon started: {} nodes, {} GPUs, sampling every {} ms",
        topo.nodes.len(),
        topo.gpus.len(),
        cfg.daemon.tick_ms
    );

    while !STOP.load(Ordering::SeqCst) {
        // Waits for the next tick while serving the graphical interface in the meantime.
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
                // pid reused, or exec() replaced the program
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
                t.state = St::Ignored(crate::i18n::tr("manual pin (numapilot pin)").into());
                continue;
            }
            let delta = st.cpu_ticks.saturating_sub(t.last_ticks) as f64;
            t.last_ticks = st.cpu_ticks;
            let now = Instant::now();
            let own_dt = now.duration_since(t.last_at).as_secs_f64().max(0.001);
            t.last_at = now;
            t.cores = delta / tck / own_dt;
            // GPUs in use (for the interface), every 5 samples.
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
        // Idle GPU: low level until it is used again.
        for (pci, c) in &gpuc.map {
            let Some(g) = topo.gpus.iter().find(|g| &g.pci == pci) else {
                continue;
            };
            if g.vendor != 0x1002 {
                continue;
            }
            let lvl_path = crate::util::sys(&format!(
                "bus/pci/devices/{pci}/power_dpm_force_performance_level"
            ));
            let e = hw.idle.entry(pci.clone()).or_insert((None, None));
            if !c.idle_save {
                if let Some(prev) = e.1.take() {
                    let _ = std::fs::write(&lvl_path, &prev);
                }
                e.0 = None;
                continue;
            }
            let busy = g.busy_percent().unwrap_or(0);
            if busy > 0 {
                e.0 = None;
                if let Some(prev) = e.1.take() {
                    let _ = std::fs::write(&lvl_path, &prev);
                }
            } else {
                let since = *e.0.get_or_insert_with(Instant::now);
                if e.1.is_none() && since.elapsed() >= Duration::from_secs(60) {
                    let cur = crate::util::read_trim(&lvl_path).unwrap_or_else(|| "auto".into());
                    if cur != "low" && std::fs::write(&lvl_path, "low").is_ok() {
                        e.1 = Some(cur);
                    }
                }
            }
        }
        // The NVIDIA offset is lost when the laptop card sleeps and wakes:
        // check from time to time (only while it is awake, so as not to wake it).
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
                            crate::nlog!("GPU {pci}: deslocamento de clock reaplicado")
                        }
                        Err(e) => crate::nlog!("GPU {pci}: deslocamento de clock: {e}"),
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
        if tick.is_multiple_of(3) {
            hw.auto_mode_tick(&topo, &procs);
            hw.dgpu_tick();
        }
        // irqbalance undoes it from time to time: reapply every 5 min
        if tick.is_multiple_of(300) {
            if cfg.daemon.gpu_irq {
                gpu_irqs(&ctx);
            }
            if cfg.daemon.device_irq {
                crate::irqs::apply(&topo);
            }
        }
        // Hot GPU notification on the phone (3 readings in a row above the limit).
        if tick.is_multiple_of(5) {
            let n = crate::notify::Notify::load();
            if n.enabled && n.gpu_hot {
                for g in &topo.gpus {
                    if g.is_nvidia() && crate::nvctl::asleep(&g.pci) {
                        continue;
                    }
                    let s = gpuctl::read_sensors(&g.pci);
                    let st = hw.hot_streak.entry(g.pci.clone()).or_default();
                    match s.control_temp() {
                        Some(t) if t >= n.gpu_hot_c => {
                            *st += 1;
                            if *st == 3 {
                                crate::notify::event(
                                    "gpu_hot",
                                    &format!("hot-{}", g.pci),
                                    &crate::tf!("Hot GPU: {}", "GPU quente: {}", g.alias),
                                    &crate::tf!(
                                        "{} is at {t:.0} °C (fan {}, {}). Check the airflow or lower the power.",
                                        "{} está a {t:.0} °C (ventoinha {}, {}). Confira a ventilação ou reduza a potência.",
                                        g.alias,
                                        s.fan_pct.map(|p| format!("{p:.0}%")).unwrap_or("?".into()),
                                        s.power_w
                                            .map(|p| format!("{p:.0} W"))
                                            .unwrap_or("?".into())
                                    ),
                                    5,
                                    Duration::from_secs(30 * 60),
                                );
                            }
                        }
                        _ => *st = 0,
                    }
                }
            }
        }
        // Hardware health (SMART, ECC, battery) every 30 min, in the background.
        // (the first one 1 min after starting, so boot is not delayed)
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
                        crate::nlog!("health: {p}");
                        crate::notify::event(
                            "health",
                            &p,
                            crate::i18n::tr("Hardware problem"),
                            &p,
                            4,
                            Duration::from_secs(24 * 3600),
                        );
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
            // energy: integrates the power since the last sample
            let dt = hw.energy_at.elapsed().as_secs_f64();
            hw.energy_at = Instant::now();
            hw.watts =
                crate::energy::power_now(&hw.cpu_power, &topo, hw.machine.base_w.unwrap_or(0.0));
            hw.energy.add(hw.watts, dt);
            if hw.energy_saved.elapsed() >= Duration::from_secs(300) {
                hw.energy_saved = Instant::now();
                let _ = hw.energy.save();
            }
            let live = serde_json::json!({
                "power": hw.cpu_power,
                "turbo": crate::turbo::tables(&topo),
                "temp": crate::cpuctl::package_temps(),
                "lowlat": format_cpulist(&hw.lowlat.cpus),
                "gpu_profiles": hw.gpu_prof,
                "board_fans": hw.board_manual,
                "ryzen": hw.ryzen_info,
                "watts": hw.watts,
            });
            let _ = std::fs::write(run_dir().join("hw.json"), live.to_string());
        }
        // Exclusive cores: union of the programs with "exclusive" running now.
        let mut want: Vec<usize> = procs
            .values()
            .filter(|t| t.state == St::Placed && t.profile.as_ref().is_some_and(|p| p.exclusive))
            .flat_map(|t| t.cpus.iter().copied())
            .collect();
        // Maximum turbo: reserves the whole socket of the program's cores, so its other
        // cores sleep. Only with more than one processor, and never all sockets.
        if topo.nodes.len() > 1 {
            let mut socks: Vec<usize> = procs
                .values()
                .filter(|t| {
                    t.state == St::Placed && t.profile.as_ref().is_some_and(|p| p.turbo_max)
                })
                .flat_map(|t| topo.nodes_of(&t.cpus))
                .collect();
            socks.sort_unstable();
            socks.dedup();
            if socks.len() < topo.nodes.len() {
                want.extend(topo.cpus_of(&socks));
            }
        }
        want.sort_unstable();
        want.dedup();
        if want != reserved_now {
            match cgroup::set_reserved(&ctx.all_cpus, &want) {
                Ok(()) => {
                    if want.is_empty() {
                        let _ = std::fs::remove_file(&reserved_flag);
                        crate::nlog!("exclusive cores released");
                    } else {
                        let _ = std::fs::write(&reserved_flag, format_cpulist(&want));
                        crate::nlog!(
                            "cores {} reserved (the rest of the system does not use them)",
                            format_cpulist(&want)
                        );
                    }
                    reserved_now = want;
                }
                Err(e) => {
                    crate::nlog!("could not reserve cores: {e}");
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
                crate::nlog!("could not save what was learned: {e}");
            }
            last_save = Instant::now();
        }
    }

    // On stop, gives every managed process back to all CPUs.
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
    // Brings the NVIDIA back if we removed it. Not on shutdown/reboot: it comes back by itself
    // at the next boot, and loading the driver in the middle of shutdown can hang.
    if let Some(d) = hw.dgpu.as_ref().filter(|d| d.removed)
        && !crate::hybrid::system_stopping()
    {
        let _ = crate::hybrid::rescan_at(&d.bridge);
        std::thread::sleep(Duration::from_millis(1500));
        for svc in hw.dgpu_stopped.drain(..) {
            let _ = Command::new("systemctl").args(["start", &svc]).status();
        }
        let _ = std::fs::remove_file(crate::hybrid::state_path());
    }
    let _ = hw.energy.save();
    for (pci, (_, prev)) in &hw.idle {
        if let Some(p) = prev {
            let _ = std::fs::write(
                crate::util::sys(&format!(
                    "bus/pci/devices/{pci}/power_dpm_force_performance_level"
                )),
                p,
            );
        }
    }
    let _ = std::fs::remove_file(run_dir().join("hw.json"));
    // Fans go back to the board's control.
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
    crate::nlog!("daemon stopped.");
    // Everything is restored and saved. Leave right here, without running destructors or
    // atexit handlers: on the server the process once stayed alive after this line until
    // systemd killed it 10 s later (a helper thread still busy), and the restart waited for it.
    use std::io::Write;
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
    unsafe { libc::_exit(0) }
}

/// Who may change the whole machine (modes, fans, clock/voltage, notifications): root or a
/// user in one of the `daemon.admin_groups` groups (default: wheel, sudo, numapilot).
/// Returns the reason for the refusal.
fn admin_denied(uid: u32, ctx: &Ctx) -> Option<String> {
    let allowed = &ctx.cfg.daemon.admin_groups;
    if uid == 0 || allowed.iter().any(|g| g == "*") && uid >= ctx.cfg.daemon.min_uid {
        return None;
    }
    if uid >= ctx.cfg.daemon.min_uid
        && crate::util::groups_of_uid(uid)
            .iter()
            .any(|g| allowed.contains(g))
    {
        return None;
    }
    crate::nlog!(
        "request refused: user {uid} is not in {}",
        allowed.join(", ")
    );
    Some(crate::tf!(
        "no permission: only users in the groups {} change the whole machine (sudo usermod -aG wheel YOUR_USER, or admin_groups in /etc/numapilot.toml)",
        "sem permissão: só usuários dos grupos {} mudam a máquina inteira (sudo usermod -aG wheel SEU_USUARIO, ou admin_groups em /etc/numapilot.toml)",
        allowed.join(", ")
    ))
}

/// Handles a request from the graphical interface / CLI.
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
    let read = reader.read_line(&mut line);
    crate::i18n::follow_client(line.trim());
    let resp = match (read, uid) {
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
            Err(e) => Response::err(crate::tf!("invalid request: {e}", "pedido inválido: {e}")),
        },
        _ => Response::err(crate::i18n::tr("could not read the request")),
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
        Request::Adopt {
            pid,
            profile,
            key,
            label,
        } => {
            return handle_adopt(uid, ctx, procs, profiles, pid, profile, key, label);
        }
        Request::FixMemory { key } => return handle_fix_memory(uid, ctx, procs, &key),
        Request::SetGovernor { node, governor } => {
            return handle_governor(uid, ctx, node, &governor);
        }
        Request::SetCoreRank { rank } => {
            if let Some(e) = admin_denied(uid, ctx) {
                return Response::err(e);
            }
            let cpus = ctx.topo.all_cpus();
            if rank.ns.len() > 4096
                || rank.ns.keys().any(|c| !cpus.contains(c))
                || rank
                    .ns
                    .values()
                    .any(|v| !v.is_finite() || *v <= 0.0 || *v > 10_000.0)
            {
                return Response::err(crate::i18n::tr("invalid measurement"));
            }
            let p = crate::corebench::path();
            return match serde_json::to_string(&rank)
                .map_err(|e| e.to_string())
                .and_then(|t| std::fs::write(&p, t).map_err(|e| e.to_string()))
            {
                Ok(()) => {
                    crate::nlog!(
                        "cache latency per core: difference of {:.1}% (noise {:.1}%), {}",
                        rank.spread_pct,
                        rank.noise_pct,
                        if rank.useful {
                            crate::i18n::tr("in use for choosing cores")
                        } else {
                            crate::i18n::tr("too small to use")
                        }
                    );
                    Response::ok(crate::i18n::tr("measurement saved"))
                }
                Err(e) => Response::err(crate::tf!(
                    "could not save: {e}",
                    "não consegui salvar: {e}"
                )),
            };
        }
        Request::SetNotify { notify } => {
            if let Some(e) = admin_denied(uid, ctx) {
                return Response::err(e);
            }
            if let Err(e) = notify.validate() {
                return Response::err(e);
            }
            return match notify.save() {
                Ok(()) => Response::ok(if notify.enabled {
                    crate::i18n::tr("notifications on")
                } else {
                    crate::i18n::tr("notifications off")
                }),
                Err(e) => Response::err(crate::tf!(
                    "could not save: {e}",
                    "não consegui salvar: {e}"
                )),
            };
        }
        Request::SetGpuControl { .. }
        | Request::SetMachine { .. }
        | Request::SetBoardFan { .. } => {
            return Response::err(crate::i18n::tr("request in the wrong place"));
        }
        Request::SetProfile { key, profile, pid } => (key, profile, pid),
        Request::DeleteProfile { key, pid } => (key, Profile::default(), pid),
    };
    if uid != 0 && uid < ctx.cfg.daemon.min_uid {
        return Response::err(crate::i18n::tr("no permission"));
    }
    if let Err(e) = profile.validate() {
        return Response::err(e);
    }
    if let Err(e) = CpuSpec::parse(&profile.cpu)
        .and(GpuSpec::parse(&profile.gpu).map(|_| ()))
        .and(Kind::parse_usage(&profile.usage).map(|_| ()))
    {
        return Response::err(crate::tf!("invalid profile: {e}", "perfil inválido: {e}"));
    }
    if let Some(p) = pid
        && uid != 0
        && procfs::uid(p).is_some_and(|owner| owner != uid)
    {
        return Response::err(crate::i18n::tr("this process belongs to another user"));
    }
    if profile.is_auto() {
        profiles.map.remove(&key);
    } else {
        profiles.map.insert(key.clone(), profile.clone());
    }
    if let Err(e) = profiles.save() {
        return Response::err(crate::tf!(
            "could not save the profile: {e}",
            "não consegui salvar o perfil: {e}"
        ));
    }
    // Applies the CPU choice right away to every process of that program.
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
    // Priority and control group apply to the whole program.
    if let Some((t, prev)) = sample {
        ctx.apply_extras(&pids, &t, &profile, prev.as_ref());
    }
    Response::ok(crate::tf!(
        "profile saved; {n} process(es) adjusted now",
        "perfil salvo; {n} processo(s) ajustado(s) agora"
    ))
}

/// Classifies a new process and applies the decision right away (rule, profile or learning).
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
        // We already know this program uses the GPU on node n: start there (local first touch).
        ctx.apply(
            pid,
            &mut t,
            vec![n],
            true,
            crate::i18n::tr("learned: uses this socket's GPU"),
            tick,
        );
        t.origin = "aprendido".into();
    }
    procs.insert(pid, t);
}

/// Applies a GPU's fan/power according to the saved configuration.
fn gpu_tick(pci: &str, c: &gpuctl::GpuControl, last: &mut HashMap<String, f64>) {
    let s = gpuctl::read_sensors(pci);
    if !s.fan_controllable {
        return;
    }
    match c.target_pct(s.control_temp()) {
        Some(pct) => {
            let prev = last.get(pci).copied();
            // Goes up right away; goes down only with a 3% difference (keeps the fan from "hunting").
            if prev.is_none_or(|p| pct > p || p - pct >= 3.0 || s.fan_mode_raw != Some(1)) {
                if let Err(e) = gpuctl::set_fan_pct(pci, pct) {
                    crate::nlog!("fan {pci}: {e}");
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

/// `numapilot run` asks, before exec, for what only root can do (cgroup, high priority).
#[allow(clippy::too_many_arguments)]
fn handle_adopt(
    uid: u32,
    ctx: &Ctx,
    procs: &mut HashMap<i32, Tracked>,
    profiles: &Profiles,
    pid: i32,
    given: Option<Profile>,
    key: Option<String>,
    label: Option<String>,
) -> Response {
    if uid != 0 && procfs::uid(pid) != Some(uid) {
        return Response::err(crate::i18n::tr("this process belongs to another user"));
    }
    let Some(st) = procfs::read_stat(pid) else {
        return Response::err(crate::i18n::tr("process does not exist"));
    };
    let learned = Learned::default();
    let mut t = classify(ctx, &learned, profiles, pid, &st, None);
    // `numapilot run` before exec: the profile and the control group are those of the program
    // about to start, not numapilot's own. That way it is born inside the group.
    if let Some(k) = key.filter(|k| !k.is_empty() && k.len() < 4096) {
        t.pkey = k;
        if let Some(l) = label {
            t.label = l.chars().take(64).collect();
        }
    }
    let Some(p) = given.or_else(|| profiles.get(&t.pkey).cloned()) else {
        return Response::ok(crate::i18n::tr("no profile"));
    };
    if let Err(e) = p.validate() {
        return Response::err(e);
    }
    t.profile = Some(p.clone());
    t.cpus = procfs::allowed_cpus(pid).unwrap_or_default();
    t.nodes = ctx.topo.nodes_of(&t.cpus);
    ctx.apply_extras(&[pid], &t, &p, None);
    procs.insert(pid, t);
    Response::ok(crate::i18n::tr("profile applied"))
}

/// Brings back the memory left on the wrong socket.
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
        Response::ok(crate::i18n::tr(
            "nothing to move (the program uses all sockets)",
        ))
    } else {
        Response::ok(crate::tf!(
            "moving the memory of {n} process(es) to the right socket",
            "movendo a memória de {n} processo(s) para o socket certo"
        ))
    }
}

pub fn governors_path() -> std::path::PathBuf {
    crate::learned::state_dir().join("governors.json")
}

fn write_governor(ctx: &Ctx, node: usize, gov: &str) -> Result<usize, String> {
    let n = ctx
        .topo
        .node(node)
        .ok_or(crate::i18n::tr("node does not exist"))?;
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
        Err(crate::i18n::tr("could not change the governor (cpufreq unavailable?)").into())
    } else {
        Ok(ok)
    }
}

fn handle_governor(uid: u32, ctx: &Ctx, node: usize, gov: &str) -> Response {
    if let Some(e) = admin_denied(uid, ctx) {
        return Response::err(e);
    }
    if !gov.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || gov.is_empty() {
        return Response::err(crate::i18n::tr("invalid governor"));
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
            Response::ok(crate::tf!(
                "socket {node}: {gov} on {n} CPUs",
                "socket {node}: {gov} em {n} CPUs"
            ))
        }
        Err(e) => Response::err(e),
    }
}

/// Each GPU's interrupts handled by the CPUs of its socket.
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
    if let Some(e) = admin_denied(uid, ctx) {
        return Response::err(e);
    }
    if !ctx.topo.gpus.iter().any(|g| g.pci == pci) {
        return Response::err(crate::tf!(
            "GPU {pci} does not exist",
            "GPU {pci} não existe"
        ));
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
            Err(e) => return Response::err(crate::tf!("fan: {e}", "ventoinha: {e}")),
        }
    } else if control.fan != FanMode::Auto && !s.fan_controllable {
        return Response::err(crate::i18n::tr(
            "this card does not allow fan control (no pwm1 in hwmon)",
        ));
    }
    let mut msg = Vec::new();
    if control.power_cap_w != old.power_cap_w {
        match gpuctl::set_power_cap(&pci, control.power_cap_w) {
            Ok(v) => msg.push(crate::tf!("power {v:.0} W", "potência {v:.0} W")),
            Err(e) => {
                return Response::err(crate::tf!(
                    "could not change the power: {e}",
                    "não consegui mudar a potência: {e}"
                ));
            }
        }
    }
    if control.clock_max_mhz != old.clock_max_mhz {
        let r = if crate::nvctl::is_nvidia(&pci) {
            crate::nvctl::set_clock_max(&pci, control.clock_max_mhz)
        } else if gpuctl::intel_freq(&pci).is_some() {
            gpuctl::set_intel_max_freq(&pci, control.clock_max_mhz)
        } else {
            return Response::err(crate::i18n::tr(
                "locking the clock here is for NVIDIA and Intel; on AMD use clock/voltage",
            ));
        };
        match r {
            Ok(()) => msg.push(match control.clock_max_mhz {
                Some(m) => crate::tf!("clock up to {m} MHz", "clock até {m} MHz"),
                None => crate::i18n::tr("normal clock").into(),
            }),
            Err(e) => return Response::err(format!("clock: {e}")),
        }
    }
    if (control.nv_gpc_offset_mhz, control.nv_mem_offset_mhz)
        != (old.nv_gpc_offset_mhz, old.nv_mem_offset_mhz)
    {
        if !crate::nvctl::is_nvidia(&pci) {
            return Response::err(crate::i18n::tr("clock offset here is NVIDIA only"));
        }
        let mut c = control.clone();
        // what was removed goes back to 0
        if old.nv_gpc_offset_mhz.is_some() && c.nv_gpc_offset_mhz.is_none() {
            c.nv_gpc_offset_mhz = Some(0);
        }
        if old.nv_mem_offset_mhz.is_some() && c.nv_mem_offset_mhz.is_none() {
            c.nv_mem_offset_mhz = Some(0);
        }
        match nv_offsets(&pci, &c) {
            Ok(()) => msg.push(crate::tf!(
                "offset GPU {:+} / memory {:+} MHz",
                "deslocamento GPU {:+} / memória {:+} MHz",
                control.nv_gpc_offset_mhz.unwrap_or(0),
                control.nv_mem_offset_mhz.unwrap_or(0)
            )),
            Err(e) => {
                return Response::err(crate::tf!(
                    "clock offset: {e}",
                    "deslocamento de clock: {e}"
                ));
            }
        }
    }
    if control.od != old.od {
        let od = control.od.as_ref().filter(|o| !o.is_default());
        match gpuctl::apply_od(&pci, od) {
            Ok(()) => msg.push(if od.is_some() {
                crate::i18n::tr("clock/voltage applied").into()
            } else {
                crate::i18n::tr("factory clock/voltage").into()
            }),
            Err(e) => {
                return Response::err(crate::tf!("clock/voltage: {e}", "clock/voltagem: {e}"));
            }
        }
        if od.is_none() {
            control.od = None;
        }
    }
    if control.fan == FanMode::Auto && old.fan != FanMode::Auto {
        let _ = gpuctl::set_fan_auto(&pci);
        fan_last.remove(&pci);
        msg.push(crate::i18n::tr("automatic fan").into());
    } else if control.fan != FanMode::Auto {
        fan_last.remove(&pci); // forces a reapply on the next tick
        msg.push(match control.fan {
            FanMode::Fixed => crate::tf!(
                "fan fixed at {:.0}%",
                "ventoinha fixa em {:.0}%",
                control.fixed_pct
            ),
            _ => crate::i18n::tr("fan on the curve").to_string(),
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
    // On disk, the cards under trial keep the last good value.
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
        return Response::err(crate::tf!(
            "could not save: {e}",
            "não consegui salvar: {e}"
        ));
    }
    if !msg.is_empty() {
        crate::nlog!("GPU {pci}: {}", msg.join(", "));
    }
    Response::ok(if msg.is_empty() {
        crate::i18n::tr("nothing changed").to_string()
    } else {
        msg.join(", ")
    })
}

fn handle_machine(uid: u32, ctx: &Ctx, hw: &mut Hw, m: Machine) -> Response {
    if let Some(e) = admin_denied(uid, ctx) {
        return Response::err(e);
    }
    if let Err(e) = m.validate() {
        return Response::err(e);
    }
    let prev = std::mem::replace(&mut hw.machine, m);
    if let Err(e) = hw.machine.save() {
        return Response::err(crate::tf!(
            "could not save: {e}",
            "não consegui salvar: {e}"
        ));
    }
    // Mode switched by hand while an automatic rule was in effect: respected until the rule changes.
    if hw.auto_mode.is_some() && prev.mode != hw.machine.mode {
        hw.auto_suppressed = hw.auto_mode.take();
        hw.manual_mode = None;
    }
    let errs = hw.apply_machine(ctx.topo, Some(&prev));
    crate::nlog!("machine mode: {}", hw.machine.mode);
    if errs.is_empty() {
        Response::ok(crate::tf!(
            "mode {} applied",
            "modo {} aplicado",
            hw.machine.mode
        ))
    } else {
        Response::ok(crate::tf!(
            "applied with warnings: {}",
            "aplicado com avisos: {}",
            errs.join("; ")
        ))
    }
}

fn handle_board_fan(
    uid: u32,
    ctx: &Ctx,
    hw: &mut Hw,
    id: String,
    mut c: fans::BoardFanControl,
) -> Response {
    if let Some(e) = admin_denied(uid, ctx) {
        return Response::err(e);
    }
    if !hw.board_fans.iter().any(|f| f.id == id) {
        return Response::err(crate::tf!(
            "fan {id} does not exist",
            "ventoinha {id} não existe"
        ));
    }
    if let Err(e) = c.validate() {
        return Response::err(e);
    }
    let msg = match c.fan {
        FanMode::Auto => crate::i18n::tr("automatic (board chip)").to_string(),
        FanMode::Fixed => crate::tf!("fixed at {:.0}%", "fixa em {:.0}%", c.fixed_pct),
        FanMode::Curve => crate::tf!("on the curve ({})", "pela curva ({})", c.source),
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
        return Response::err(crate::tf!(
            "could not save: {e}",
            "não consegui salvar: {e}"
        ));
    }
    crate::nlog!("fan {id}: {msg}");
    Response::ok(crate::tf!("fan {msg}", "ventoinha {msg}"))
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
        last_at: Instant::now(),
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
        ignore(&mut t, crate::i18n::tr("system process"));
        return t;
    }
    if ctx
        .cfg
        .daemon
        .ignore
        .iter()
        .any(|p| idents.iter().any(|i| glob_match(p, i)))
    {
        ignore(&mut t, crate::i18n::tr("on the ignore list"));
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
        ignore(
            &mut t,
            crate::i18n::tr("affinity set by another program (taskset?)"),
        );
        return t;
    }

    // A profile saved by the graphical interface takes priority over the rules.
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
        t.origin = crate::tf!("rule '{}'", "regra '{}'", rule.label());
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

    // A child of a process with a fixed decision (e.g. game started by Steam) inherits it.
    if let Some(p) = parent
        && p.fixed
        && p.state == St::Placed
        && same_set(&allowed, &ctx.topo.cpus_of(&p.nodes))
    {
        t.origin = crate::tf!("inherited from {}", "herdado de {}", p.comm);
        t.nodes = p.nodes.clone();
        t.fixed = true;
        t.state = St::Placed;
        return t;
    }

    if let Some(e) = learned.get(&key)
        && e.wide
        && ctx.topo.nodes.len() > 1
    {
        t.origin = crate::i18n::tr("learned (2 sockets)").into();
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
        // Catches threads created during the first application.
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
            // Uses a GPU? Then it goes to that GPU's socket.
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
                        let why = crate::tf!("uses GPU {}", "usa a GPU {}", t.gpus.join(","));
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
                let why = crate::tf!(
                    "{:.1} cores in use; node {n} is the least busy",
                    "{:.1} núcleos em uso; nó {n} é o menos ocupado",
                    t.cores
                );
                learned.update(&t.key, |e| e.runs += 1);
                ctx.apply(pid, t, vec![n], true, &why, tick);
            }
        }
        St::Placed => {
            if t.fixed || !multi {
                return;
            }
            // Started using a GPU on another socket after being placed? Move there.
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
                        let why = crate::tf!(
                            "started using GPU {}",
                            "passou a usar a GPU {}",
                            t.gpus.join(",")
                        );
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
                    let why = crate::tf!(
                        "saturated the node ({:.0}% for {} samples) → 2 sockets",
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
                    let why = crate::tf!(
                        "low usage for {} samples → back to 1 socket",
                        "uso baixo por {} amostras → volta para 1 socket",
                        t.lo
                    );
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
                St::Watching => crate::i18n::tr("watching").into(),
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
    // history per program (one point per minute)
    if crate::proghist::sample(
        rows.iter()
            .map(|r| (r.key.as_str(), r.cores, r.mem.iter().sum::<u64>())),
        crate::history::now(),
    ) {
        crate::proghist::save();
    }
    if let Ok(s) = serde_json::to_string(&rows) {
        let tmp = run_dir().join("state.json.tmp");
        if std::fs::write(&tmp, s).is_ok() {
            let _ = std::fs::rename(&tmp, state_path());
        }
    }
}

/// Applies the NVIDIA clock offsets (within the range the card accepts).
fn nv_offsets(pci: &str, c: &gpuctl::GpuControl) -> Result<(), String> {
    if c.nv_gpc_offset_mhz.is_none() && c.nv_mem_offset_mhz.is_none() {
        return Ok(());
    }
    if crate::nvctl::asleep(pci) {
        return Err(crate::i18n::tr("the card is asleep; it will be applied when it wakes").into());
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
