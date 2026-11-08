//! numapilot-gui: escolha gráfica de CPU (socket) e GPU por programa.
//! O daemon mapeia os programas abertos; aqui você troca o hardware de cada um.

mod bg;
mod editor;
mod gpus;
mod jobs;
mod lab;
mod llm;
mod machine;
mod map;
mod settings;
mod single;
mod stream;
mod theme;
mod tray;

use eframe::egui::{self, Color32, RichText};
use numapilot::config::Config;
use numapilot::ctl::{self, Request};
use numapilot::daemon::{self, Row};
use numapilot::decide::node_loads;
use numapilot::desktop;
use numapilot::gpuctl::{GpuControl, GpuControls};
use numapilot::labcfg::LabSettings;
use numapilot::procfs;
use numapilot::profiles::{Profile, Profiles};
use numapilot::topo::{GpuRole, Topology};
use std::collections::{BTreeMap, HashMap};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

const STEAM_OPTION: &str = "numapilot run -- %command%";

#[derive(Clone, Debug)]
pub(crate) struct Group {
    cpus: String,
    mem: Vec<u64>,
    pub(crate) label: String,
    key: String,
    exe: String,
    pids: Vec<i32>,
    main_pid: i32,
    uid: u32,
    cores: f64,
    nodes: Vec<usize>,
    gpus: Vec<String>,
    origin: String,
    steam: bool,
}

fn group_rows(rows: &[Row]) -> Vec<Group> {
    let mut map: BTreeMap<String, Vec<&Row>> = BTreeMap::new();
    for r in rows {
        map.entry(r.key.clone()).or_default().push(r);
    }
    let mut out: Vec<Group> = map
        .into_iter()
        .map(|(key, mut rs)| {
            rs.sort_by(|a, b| {
                b.cores
                    .partial_cmp(&a.cores)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let steam = key.starts_with("steam:");
            let pids: Vec<i32> = rs.iter().map(|r| r.pid).collect();
            let main_pid = rs
                .iter()
                .filter(|r| !pids.contains(&r.ppid))
                .map(|r| r.pid)
                .min()
                .unwrap_or(rs[0].pid);
            let label = if steam {
                rs.iter()
                    .find(|r| r.comm.to_lowercase().ends_with(".exe"))
                    .map(|r| r.comm.clone())
                    .unwrap_or_else(|| rs[0].label.clone())
            } else {
                rs.iter()
                    .find(|r| r.pid == main_pid)
                    .map(|r| r.label.clone())
                    .unwrap_or_default()
            };
            let mut nodes: Vec<usize> = rs
                .iter()
                .flat_map(|r| {
                    r.nodes
                        .split(',')
                        .filter_map(|n| n.parse().ok())
                        .collect::<Vec<_>>()
                })
                .collect();
            nodes.sort_unstable();
            nodes.dedup();
            let mut gpus: Vec<String> = rs
                .iter()
                .flat_map(|r| {
                    r.gpus
                        .split(',')
                        .filter(|g| !g.is_empty())
                        .map(String::from)
                })
                .collect();
            gpus.sort();
            gpus.dedup();
            let mut mem: Vec<u64> = vec![];
            for r in &rs {
                if mem.len() < r.mem.len() {
                    mem.resize(r.mem.len(), 0);
                }
                for (i, v) in r.mem.iter().enumerate() {
                    mem[i] += v;
                }
            }
            Group {
                cpus: rs
                    .iter()
                    .find(|r| r.pid == main_pid)
                    .map(|r| r.cpus.clone())
                    .unwrap_or_default(),
                mem,
                label,
                exe: rs
                    .iter()
                    .find(|r| r.pid == main_pid)
                    .map(|r| r.exe.clone())
                    .unwrap_or_default(),
                main_pid,
                uid: rs[0].uid,
                cores: rs.iter().map(|r| r.cores).sum(),
                origin: rs[0].origin.clone(),
                pids,
                nodes,
                gpus,
                steam,
                key,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.cores
            .partial_cmp(&a.cores)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Programs,
    Map,
    Profiles,
    Gpus,
    Machine,
    Lab,
    Llm,
    Stream,
    Settings,
}

#[derive(Clone, Default)]
pub(crate) struct Live {
    pub(crate) busy: Option<f32>,
    pub(crate) temp: Option<f64>,
    pub(crate) power: Option<f64>,
    pub(crate) vram: Option<(u64, u64)>,
}

/// Memória do sistema (em uso, total) em kB, pelo MemAvailable: o cache de arquivos não conta
/// como "em uso" (ele é liberado sozinho quando falta memória).
pub(crate) fn mem_system() -> (u64, u64) {
    let t = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let get = |k: &str| {
        t.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    };
    let total = get("MemTotal:");
    (total.saturating_sub(get("MemAvailable:")), total)
}

/// Abas: ícone, nome e o que cada uma faz.
const TABS: &[(Tab, &str, &str, &str)] = &[
    (
        Tab::Programs,
        "▶",
        "Programas",
        "O que está aberto agora e em qual processador e placa cada um roda",
    ),
    (
        Tab::Map,
        "🗺",
        "Mapa e medições",
        "Núcleos, memória por socket e medições de desempenho",
    ),
    (
        Tab::Profiles,
        "💾",
        "Escolhas salvas",
        "Escolhas por programa que valem toda vez que ele abrir",
    ),
    (
        Tab::Gpus,
        "🎮",
        "Placas de vídeo",
        "Ventoinha, potência, clock e undervolt de cada placa",
    ),
    (
        Tab::Machine,
        "🖥",
        "Máquina",
        "Modo de energia, processador, notebook e saúde do hardware",
    ),
    (
        Tab::Lab,
        "🔬",
        "Laboratório",
        "Containers com ROCm, Jupyter e IDEs",
    ),
    (
        Tab::Llm,
        "💬",
        "Servidor de IA",
        "Modelos locais com llama.cpp",
    ),
    (
        Tab::Stream,
        "📡",
        "Streaming",
        "Sunshine e Moonlight: jogar de outro aparelho",
    ),
    (
        Tab::Settings,
        "⚙",
        "Configurações",
        "Outros PCs, avisos, atualização e máquina virtual",
    ),
];

pub(crate) struct App {
    pub(crate) topo: Topology,
    pub(crate) cfg: Config,
    pub(crate) editor: Option<editor::Editor>,
    pub(crate) cpu_loads: BTreeMap<usize, f32>,
    /// Último minuto de cada thread e da temperatura do processador (aba Mapa).
    pub(crate) cpu_sparks: BTreeMap<usize, theme::Spark>,
    pub(crate) cpu_freq: BTreeMap<usize, u64>,
    pub(crate) temp_spark: BTreeMap<u32, (theme::Spark, f64)>,
    pub(crate) map_threads: bool,
    pub(crate) numa_rates: BTreeMap<usize, (f64, f64)>,
    prev_numa: BTreeMap<usize, (u64, u64)>,
    prev_numa_t: Instant,
    pub(crate) bench: Option<numapilot::bench::BenchResult>,
    pub(crate) bench_checked: Option<Instant>,
    pub(crate) bg: bg::Shared,
    pub(crate) job: Option<jobs::Job>,
    pub(crate) show_job: bool,
    pub(crate) has_gamescope: bool,
    pub(crate) has_mangohud: bool,
    pub(crate) gpu_saved: GpuControls,
    pub(crate) gpu_edit: HashMap<String, GpuControl>,
    pub(crate) pending_gpu: Option<(String, GpuControl)>,
    pub(crate) hist: Option<numapilot::history::History>,
    pub(crate) hist_at: Option<Instant>,
    pub(crate) hist_secs: u64,
    pub(crate) web_active: bool,
    pub(crate) mach: machine::MachineUi,
    pub(crate) nv_status: Option<(Instant, numapilot::driver::Status)>,
    pub(crate) board_edit: Option<(String, String, Vec<numapilot::boards::Field>)>,
    pub(crate) peer_link: String,
    pub(crate) notify_edit: Option<numapilot::notify::Notify>,
    pub(crate) vm_ui: crate::settings::VmUi,
    pub(crate) peer_name: String,
    pub(crate) web_checked: Option<Instant>,
    pub(crate) filter: String,
    pending_fix: Option<String>,
    pub(crate) llm_cfg: Option<LabSettings>,
    pub(crate) models: Option<Vec<(String, u64)>>,
    ctx: egui::Context,
    pub(crate) groups: Vec<Group>,
    profiles: Profiles,
    pub(crate) loads: HashMap<usize, f64>,
    prev_cpu: Vec<(usize, u64, u64)>,
    last: Instant,
    daemon_ok: bool,
    toast: Option<(String, bool, Instant)>,
    /// Leituras ao vivo por placa (lidas 1x por segundo, não a cada quadro).
    live: HashMap<String, Live>,
    /// Último minuto de CPU, memória e GPUs para os mini gráficos.
    sparks: HashMap<String, theme::Spark>,
    /// Memória do sistema: (em uso, total) em kB, sem contar o cache que o sistema libera sozinho.
    mem_sys: (u64, u64),
    tab: Tab,
    my_uid: u32,
    only_mine: bool,
    desktop_cache: HashMap<String, Option<PathBuf>>,
    bg_msgs: Arc<Mutex<Vec<(String, bool)>>>,
    pub(crate) cpu_opts: Vec<(String, String)>,
    pub(crate) gpu_opts: Vec<(String, String, String)>,
}

impl App {
    fn new(ctx: egui::Context) -> App {
        theme::install(&ctx);
        let cfg = Config::load().unwrap_or_default();
        let topo = Topology::load(&cfg);
        // Programas novos na inicialização também vão para a integrada (se ligado).
        if numapilot::autostart::enabled() {
            let _ = numapilot::autostart::apply(&topo);
        }
        let renders: Vec<String> = topo.gpus.iter().filter_map(|g| g.render.clone()).collect();
        let bg = bg::start(ctx.clone(), renders);
        let mut cpu_opts = vec![
            ("auto".to_string(), "Automático".to_string()),
            ("gpu".to_string(), "Socket da GPU escolhida".to_string()),
        ];
        for n in &topo.nodes {
            cpu_opts.push((n.id.to_string(), format!("Só o socket {}", n.id)));
        }
        if topo.nodes.len() > 1 {
            cpu_opts.push(("all".into(), "Os dois sockets".into()));
        }
        let mut gpu_opts = vec![(
            "auto".to_string(),
            "auto".to_string(),
            "Automático".to_string(),
        )];
        let sk = |n: Option<usize>| {
            n.map(|n| format!("socket {n}"))
                .unwrap_or_else(|| "socket ?".into())
        };
        for g in &topo.gpus {
            match g.role {
                GpuRole::Compute => {
                    let mon = if g.display_connected {
                        ", monitor próprio"
                    } else {
                        ""
                    };
                    gpu_opts.push((
                        g.alias.clone(),
                        "graphics".into(),
                        format!("{} — jogo/gráfico ({}{mon})", g.alias, sk(g.numa_node)),
                    ));
                    gpu_opts.push((
                        g.alias.clone(),
                        "compute".into(),
                        format!("{} — IA / ROCm ({})", g.alias, sk(g.numa_node)),
                    ));
                }
                _ => gpu_opts.push((
                    g.alias.clone(),
                    "graphics".into(),
                    format!("{} — vídeo ({})", g.alias, sk(g.numa_node)),
                )),
            }
        }
        gpu_opts.push(("none".into(), "auto".into(), "Nenhuma GPU".into()));
        let mut app = App {
            topo,
            cfg: cfg.clone(),
            editor: None,
            cpu_loads: BTreeMap::new(),
            cpu_sparks: BTreeMap::new(),
            cpu_freq: BTreeMap::new(),
            temp_spark: BTreeMap::new(),
            map_threads: true,
            numa_rates: BTreeMap::new(),
            prev_numa: BTreeMap::new(),
            prev_numa_t: Instant::now(),
            bench: numapilot::bench::load(),
            bench_checked: Some(Instant::now()),
            bg,
            job: None,
            show_job: false,
            has_gamescope: in_path("gamescope"),
            has_mangohud: in_path("mangohud"),
            gpu_saved: GpuControls::load(),
            gpu_edit: HashMap::new(),
            pending_gpu: None,
            hist: None,
            hist_at: None,
            hist_secs: 3600,
            web_active: false,
            mach: Default::default(),
            nv_status: None,
            board_edit: None,
            peer_link: String::new(),
            notify_edit: None,
            vm_ui: Default::default(),
            peer_name: String::new(),
            web_checked: None,
            filter: String::new(),
            pending_fix: None,
            llm_cfg: None,
            models: None,
            ctx,
            groups: vec![],
            profiles: Profiles::default(),
            loads: HashMap::new(),
            prev_cpu: procfs::cpu_times(),
            last: Instant::now() - Duration::from_secs(10),
            daemon_ok: false,
            toast: None,
            live: HashMap::new(),
            sparks: HashMap::new(),
            mem_sys: (0, 0),
            // NUMAPILOT_GUI_TAB=N abre direto na aba N (0 = Programas): para capturas de tela
            tab: std::env::var("NUMAPILOT_GUI_TAB")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|i| TABS.get(i))
                .map(|t| t.0)
                .unwrap_or(Tab::Programs),
            my_uid: unsafe { libc::getuid() },
            only_mine: true,
            desktop_cache: HashMap::new(),
            bg_msgs: Arc::new(Mutex::new(vec![])),
            cpu_opts,
            gpu_opts,
        };
        app.refresh();
        app
    }

    fn refresh(&mut self) {
        let cur = procfs::cpu_times();
        self.loads = node_loads(&self.topo, &self.prev_cpu, &cur);
        let prev: HashMap<usize, (u64, u64)> =
            self.prev_cpu.iter().map(|&(c, b, t)| (c, (b, t))).collect();
        self.cpu_loads = cur
            .iter()
            .filter_map(|&(c, b, t)| {
                let (pb, pt) = prev.get(&c)?;
                let dt = t.saturating_sub(*pt).max(1);
                Some((c, b.saturating_sub(*pb) as f32 / dt as f32))
            })
            .collect();
        for (c, l) in &self.cpu_loads {
            self.cpu_sparks.entry(*c).or_default().push(*l);
        }
        // frequência e temperatura só quando a aba Mapa está aberta (são muitos arquivos)
        if self.tab == Tab::Map {
            for c in self.cpu_loads.keys() {
                if let Some(k) = numapilot::util::read_trim(numapilot::util::sys(&format!(
                    "devices/system/cpu/cpu{c}/cpufreq/scaling_cur_freq"
                )))
                .and_then(|v| v.parse::<u64>().ok())
                {
                    self.cpu_freq.insert(*c, k / 1000);
                }
            }
        }
        for (pkg, t) in numapilot::cpuctl::package_temps() {
            let e = self.temp_spark.entry(pkg).or_default();
            // escala 30–100 °C
            e.0.push(((t - 30.0) / 70.0) as f32);
            e.1 = t;
        }
        // Alocações de memória no outro socket (numastat), por segundo.
        let dt = self.prev_numa_t.elapsed().as_secs_f64().max(0.1);
        self.prev_numa_t = Instant::now();
        for n in &self.topo.nodes {
            let st = procfs::numastat(n.id);
            let other = st.get("other_node").copied().unwrap_or(0);
            let hit = st.get("local_node").copied().unwrap_or(0) + other;
            if let Some((po, ph)) = self.prev_numa.get(&n.id) {
                let d_other = other.saturating_sub(*po) as f64;
                let d_all = hit.saturating_sub(*ph).max(1) as f64;
                self.numa_rates
                    .insert(n.id, (d_other / dt, d_other / d_all));
            }
            self.prev_numa.insert(n.id, (other, hit));
        }
        if self
            .bench_checked
            .is_none_or(|t| t.elapsed() > Duration::from_secs(10))
        {
            self.bench = numapilot::bench::load();
            self.bench_checked = Some(Instant::now());
        }
        self.prev_cpu = cur;
        // Leituras ao vivo (uma vez por segundo) e o último minuto para os mini gráficos.
        self.mem_sys = mem_system();
        let (mu, mt) = self.mem_sys;
        self.sparks
            .entry("mem".into())
            .or_default()
            .push(mu as f32 / mt.max(1) as f32);
        for n in &self.topo.nodes {
            let l = self.loads.get(&n.id).copied().unwrap_or(0.0) as f32;
            self.sparks
                .entry(format!("cpu{}", n.id))
                .or_default()
                .push(l);
        }
        for g in &self.topo.gpus {
            let s = numapilot::gpuctl::read_sensors(&g.pci);
            let l = Live {
                busy: g.busy_percent().map(|b| b as f32 / 100.0),
                temp: s.control_temp(),
                power: s.power_w,
                vram: g.vram_used_total(),
            };
            self.sparks
                .entry(format!("gpu{}", g.pci))
                .or_default()
                .push(l.busy.unwrap_or(0.0));
            self.live.insert(g.pci.clone(), l);
        }
        self.daemon_ok = std::fs::metadata(daemon::state_path())
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|d| d < Duration::from_secs(5));
        let rows: Vec<Row> = daemon::read_state()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| !self.only_mine || r.uid == self.my_uid)
            .collect();
        self.groups = group_rows(&rows);
        self.profiles = Profiles::load();
        self.last = Instant::now();
        let msgs: Vec<(String, bool)> = self
            .bg_msgs
            .lock()
            .map(|mut v| v.drain(..).collect())
            .unwrap_or_default();
        if let Some((m, ok)) = msgs.into_iter().last() {
            self.toast(m, ok);
        }
    }

    pub(crate) fn toast(&mut self, msg: impl Into<String>, ok: bool) {
        self.toast = Some((msg.into(), ok, Instant::now()));
    }

    /// Roda um comando em segundo plano e mostra a saída numa janelinha.
    pub(crate) fn start_job(&mut self, title: impl Into<String>, argv: Vec<String>) {
        if self.job.as_ref().is_some_and(|j| j.finished().is_none()) {
            self.toast("Espere o comando anterior terminar", false);
            self.show_job = true;
            return;
        }
        self.job = Some(jobs::run(title, argv, self.ctx.clone()));
        self.show_job = true;
    }

    fn job_window(&mut self, ctx: &egui::Context) {
        let Some(job) = self.job.clone() else { return };
        if !self.show_job {
            return;
        }
        let mut open = true;
        let status = match job.finished() {
            None => "rodando…",
            Some(true) => "concluído",
            Some(false) => "com erro",
        };
        egui::Window::new(format!("{} — {status}", job.title))
            .id(egui::Id::unique("numapilot-job"))
            .open(&mut open)
            .default_size([720.0, 360.0])
            .resizable(true)
            .show(ctx, |ui| {
                if job.finished().is_none() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("pode fechar esta janela; o comando continua em segundo plano");
                    });
                }
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(job.text()).monospace().small()).wrap(),
                        );
                    });
            });
        if !open {
            self.show_job = false;
        }
    }

    pub(crate) fn send(&mut self, req: Request) {
        match ctl::send(&req) {
            Ok(r) => self.toast(r.message, r.ok),
            Err(e) => self.toast(format!("{e:#}"), false),
        }
        self.last = Instant::now() - Duration::from_secs(10); // atualiza já
    }

    fn profile_of(&self, key: &str) -> Profile {
        self.profiles.get(key).cloned().unwrap_or_default()
    }

    fn desktop_for(&mut self, exe: &str) -> Option<PathBuf> {
        if exe.is_empty() {
            return None;
        }
        self.desktop_cache
            .entry(exe.to_string())
            .or_insert_with(|| desktop::find_for_exe(exe))
            .clone()
    }

    /// Menu lateral.
    fn nav(&mut self, ui: &mut egui::Ui) {
        let p = theme::pal(ui);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.label(
                RichText::new("numapilot")
                    .size(19.0)
                    .family(egui::FontFamily::Name(theme::BOLD.into()))
                    .color(p.text),
            );
        });
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.label(
                RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                    .size(11.5)
                    .color(p.weak),
            );
        });
        ui.add_space(14.0);
        for (t, icon, label, _) in TABS {
            if theme::nav_item(ui, icon, label, self.tab == *t).clicked() {
                self.tab = *t;
            }
            ui.add_space(2.0);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(6.0);
            let mut pref = ui.ctx().options(|o| o.theme_preference);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                for (v, l) in [
                    (egui::ThemePreference::System, "Sistema"),
                    (egui::ThemePreference::Light, "Claro"),
                    (egui::ThemePreference::Dark, "Escuro"),
                ] {
                    if ui
                        .selectable_label(pref == v, RichText::new(l).size(12.0))
                        .clicked()
                    {
                        pref = v;
                        ui.ctx().set_theme(v);
                        theme::save_theme(v);
                    }
                }
            });
            ui.label(RichText::new("Tema").size(11.5).color(p.weak));
        });
    }

    /// Topo: título da página, estado do serviço e os medidores ao vivo.
    fn header(&mut self, ui: &mut egui::Ui) {
        let p = theme::pal(ui);
        let (title, sub) = TABS
            .iter()
            .find(|t| t.0 == self.tab)
            .map(|t| (t.2, t.3))
            .unwrap_or(("", ""));
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(title)
                        .size(22.0)
                        .family(egui::FontFamily::Name(theme::BOLD.into())),
                );
                ui.label(RichText::new(sub).color(p.weak).size(13.0));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.daemon_ok {
                    theme::chip(ui, "Serviço ativo", p.ok);
                } else {
                    theme::chip(ui, "Serviço parado", p.err);
                }
                if self.job.is_some() && !self.show_job && ui.button("Último comando").clicked() {
                    self.show_job = true;
                }
            });
        });
        if !self.daemon_ok {
            ui.add_space(6.0);
            egui::Frame::new()
                .fill(p.err.gamma_multiply(0.12))
                .stroke(egui::Stroke::new(1.0, p.err.gamma_multiply(0.5)))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("O serviço do numapilot não está rodando: as escolhas não são aplicadas.")
                                .color(p.err),
                        );
                        if ui.button("Ativar o serviço").clicked() {
                            self.start_job(
                                "Ativar o serviço",
                                vec![
                                    "pkexec".into(),
                                    "systemctl".into(),
                                    "enable".into(),
                                    "--now".into(),
                                    "numapilot".into(),
                                ],
                            );
                        }
                    });
                });
        }
        ui.add_space(10.0);
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::TOP).with_main_wrap(true),
            |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                let multi = self.topo.nodes.len() > 1;
                for n in self.topo.nodes.clone() {
                    let l = self.loads.get(&n.id).copied().unwrap_or(0.0) as f32;
                    let name = if multi {
                        format!("CPU · socket {}", n.id)
                    } else {
                        "CPU".into()
                    };
                    let key = format!("cpu{}", n.id);
                    theme::metric(
                        ui,
                        &theme::Metric {
                            key: &key,
                            name: &name,
                            big: &format!("{:.0}%", l * 100.0),
                            small: &format!("{} núcleos", n.physical_cores.max(n.cores.len())),
                            frac: l,
                            color: p.accent,
                            spark: self.sparks.get(&key),
                            extra: None,
                        },
                    );
                }
                let (mu, mt) = self.mem_sys;
                if mt > 0 {
                    theme::metric(
                        ui,
                        &theme::Metric {
                            key: "mem",
                            name: "Memória",
                            big: &format!("{:.1} GB", mu as f64 / 1048576.0),
                            small: &format!(
                                "de {:.0} GB (sem contar o cache)",
                                mt as f64 / 1048576.0
                            ),
                            frac: mu as f32 / mt as f32,
                            color: Color32::from_rgb(0x9b, 0x7b, 0xff),
                            spark: self.sparks.get("mem"),
                            extra: None,
                        },
                    );
                }
                for g in self.topo.gpus.clone() {
                    let l = self.live.get(&g.pci).cloned().unwrap_or_default();
                    let mut info: Vec<String> = vec![];
                    if let Some(t) = l.temp {
                        info.push(format!("{t:.0} °C"));
                    }
                    if let Some(w) = l.power {
                        info.push(format!("{w:.0} W"));
                    }
                    let busy = l.busy.unwrap_or(0.0);
                    let key = format!("gpu{}", g.pci);
                    let vram = l.vram.map(|(u, t)| {
                        (
                            format!(
                                "VRAM  {:.1} / {:.0} GB",
                                u as f64 / 1073741824.0,
                                t as f64 / 1073741824.0
                            ),
                            u as f32 / t.max(1) as f32,
                        )
                    });
                    theme::metric(
                        ui,
                        &theme::Metric {
                            key: &key,
                            name: &g.alias,
                            big: &format!("{:.0}%", busy * 100.0),
                            small: &info.join(" · "),
                            frac: busy,
                            color: Color32::from_rgb(0x2d, 0xc4, 0xa8),
                            spark: self.sparks.get(&key),
                            extra: vram.as_ref().map(|(t, f)| (t.as_str(), *f)),
                        },
                    );
                }
            },
        );
    }

    fn programs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.checkbox(&mut self.only_mine, "Só os meus programas").changed() {
                self.last = Instant::now() - Duration::from_secs(10);
            }
            ui.label(
                RichText::new("CPU muda na hora. GPU vale quando o programa abre (Reabrir, atalho do menu ou opção da Steam).")
                    .weak(),
            );
        });
        ui.horizontal(|ui| {
            ui.label("Steam:");
            if ui
                .button("Aplicar em todos os jogos")
                .on_hover_text("Coloca \"numapilot run -- %command%\" nas opções de inicialização de todos os jogos instalados (mantém o que já existia). Feche a Steam antes.")
                .clicked()
            {
                self.start_job("Steam: aplicar em todos os jogos", vec![jobs::sibling("numapilot"), "steam".into(), "enable".into()]);
            }
            if ui.button("Remover de todos").clicked() {
                self.start_job("Steam: remover de todos os jogos", vec![jobs::sibling("numapilot"), "steam".into(), "disable".into()]);
            }
            if ui.button("Ver lista").clicked() {
                self.start_job("Steam: jogos", vec![jobs::sibling("numapilot"), "steam".into(), "list".into()]);
            }
        });
        ui.separator();
        if self.groups.is_empty() {
            ui.label("Nenhum programa sendo usado agora. Abra algo (um jogo, o Blender, o llama-server...) e ele aparece aqui.");
            return;
        }
        let groups = self.groups.clone();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("progs")
                .striped(true)
                .num_columns(6)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    for h in [
                        "Programa",
                        "Uso",
                        "Onde está agora",
                        "Processador",
                        "Placa de vídeo",
                        "",
                    ] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for g in &groups {
                        self.program_row(ui, g);
                        ui.end_row();
                    }
                });
        });
    }

    fn program_row(&mut self, ui: &mut egui::Ui, g: &Group) {
        let prof = self.profile_of(&g.key);
        // Programa
        ui.vertical(|ui| {
            ui.set_min_width(230.0);
            ui.label(RichText::new(&g.label).strong())
                .on_hover_text(format!("{}\nPIDs: {:?}\nchave: {}", g.exe, g.pids, g.key));
            let sub = if g.steam {
                format!("Steam · {}", g.key)
            } else {
                g.exe.clone()
            };
            ui.label(RichText::new(truncate(&sub, 42)).small().weak());
        });
        // Uso
        ui.label(format!("{:.1} núcleos", g.cores));
        // Onde está agora
        ui.vertical(|ui| {
            ui.set_min_width(170.0);
            let where_ = match g.nodes.len() {
                0 => "livre (todos)".to_string(),
                1 => format!("socket {}", g.nodes[0]),
                _ => "os dois sockets".to_string(),
            };
            let ncpu = numapilot::util::parse_cpulist(&g.cpus).len();
            let total = self.topo.all_cpus().len();
            ui.label(if ncpu > 0 && ncpu < total && !g.nodes.is_empty() {
                format!("{where_} · {ncpu} CPUs")
            } else {
                where_
            });
            let gpu = if g.gpus.is_empty() { "sem GPU".to_string() } else { format!("GPU {}", g.gpus.join(", ")) };
            ui.label(RichText::new(format!("{gpu} · {}", g.origin)).small().weak());
            let tot: u64 = g.mem.iter().sum();
            if tot >= 64 && self.topo.nodes.len() > 1 {
                let local: u64 = g.mem.iter().enumerate().filter(|(n, _)| g.nodes.contains(n)).map(|(_, v)| *v).sum();
                let pct = if g.nodes.is_empty() { 100.0 } else { local as f64 * 100.0 / tot as f64 };
                let txt = format!("memória {:.1} GB · {pct:.0}% local", tot as f64 / 1024.0);
                if pct < 80.0 {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(txt).small().color(Color32::from_rgb(230, 160, 40)))
                            .on_hover_text("Parte da memória está no outro socket: cada acesso a ela é mais lento.");
                        if ui.small_button("corrigir").clicked() {
                            self.pending_fix = Some(g.key.clone());
                        }
                    });
                } else {
                    ui.label(RichText::new(txt).small().weak());
                }
            }
        });
        // Processador
        let mut cpu = prof.cpu.clone();
        let cpu_text = self
            .cpu_opts
            .iter()
            .find(|(v, _)| *v == cpu)
            .map(|(_, t)| t.clone())
            .unwrap_or(cpu.clone());
        egui::ComboBox::from_id_salt(format!("cpu-{}", g.key))
            .selected_text(cpu_text)
            .width(190.0)
            .show_ui(ui, |ui| {
                for (v, t) in &self.cpu_opts {
                    ui.selectable_value(&mut cpu, v.clone(), t);
                }
            });
        // Placa de vídeo
        let cur = (prof.gpu.clone(), prof.usage.clone());
        let mut sel = cur.clone();
        let mut fg = prof.fg_gpu.clone();
        let gpu_text = self
            .gpu_opts
            .iter()
            .find(|(gg, u, _)| *gg == cur.0 && (*u == cur.1 || cur.0 == "auto" || cur.0 == "none"))
            .map(|(_, _, t)| t.clone())
            .unwrap_or_else(|| format!("{} ({})", cur.0, cur.1));
        ui.vertical(|ui| {
            egui::ComboBox::from_id_salt(format!("gpu-{}", g.key))
                .selected_text(gpu_text)
                .width(250.0)
                .show_ui(ui, |ui| {
                    for (gg, u, t) in &self.gpu_opts {
                        ui.selectable_value(&mut sel, (gg.clone(), u.clone()), t);
                    }
                });
            // Lossless Scaling (lsfg-vk): em qual GPU gerar os quadros.
            let fg_text = if fg == "same" {
                "Lossless Scaling: mesma GPU".to_string()
            } else {
                format!("Lossless Scaling: {fg} (dual GPU)")
            };
            egui::ComboBox::from_id_salt(format!("fg-{}", g.key))
                .selected_text(RichText::new(fg_text).small())
                .width(250.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut fg,
                        "same".to_string(),
                        "Mesma GPU do jogo (lsfg-vk oficial)",
                    );
                    for gg in &self.topo.gpus {
                        ui.selectable_value(
                            &mut fg,
                            gg.alias.clone(),
                            format!("{} — dual GPU (precisa de fork do lsfg-vk)", gg.alias),
                        );
                    }
                });
            if prof.gpu != "auto"
                && prof.gpu != "none"
                && !g.gpus.is_empty()
                && !g.gpus.contains(&prof.gpu)
            {
                ui.label(
                    RichText::new("reabra o programa para trocar a GPU")
                        .small()
                        .color(Color32::from_rgb(230, 160, 40)),
                );
            }
        });
        // Ações
        let mut action: Option<&str> = None;
        ui.horizontal(|ui| {
            if ui.button("Ajustar…").on_hover_text("Núcleos, memória, prioridade, GPU e perfis prontos").clicked() {
                action = Some("edit");
            }
            if g.steam {
                if ui.button("Copiar opção Steam").on_hover_text(format!(
                    "Cole nas Propriedades do jogo → Opções de inicialização:\n{STEAM_OPTION}\n(uma vez por jogo; depois a GPU escolhida aqui vale sempre)"
                )).clicked() {
                    action = Some("steam");
                }
            } else {
                let mine = g.uid == self.my_uid;
                if ui.add_enabled(mine, egui::Button::new("Reabrir"))
                    .on_hover_text("Fecha e abre de novo com a CPU e a GPU escolhidas")
                    .clicked()
                {
                    action = Some("relaunch");
                }
                if let Some(d) = self.desktop_for(&g.exe) {
                    let on = desktop::is_overridden(&d);
                    let txt = if on { "No menu: sim" } else { "Usar no menu" };
                    if ui.button(txt).on_hover_text(if on {
                        "O atalho do menu já abre pelo numapilot. Clique para desfazer."
                    } else {
                        "Faz o atalho do menu abrir este programa pelo numapilot, para a GPU escolhida valer sempre."
                    }).clicked() {
                        action = Some(if on { "menu-off" } else { "menu-on" });
                    }
                }
            }
        });

        if cpu != prof.cpu || sel != cur || fg != prof.fg_gpu {
            let profile = Profile {
                label: g.label.clone(),
                cpu,
                gpu: sel.0,
                usage: sel.1,
                fg_gpu: fg,
                ..prof.clone()
            };
            self.send(Request::SetProfile {
                key: g.key.clone(),
                profile,
                pid: Some(g.main_pid),
            });
        }
        match action {
            Some("edit") => {
                self.editor = Some(editor::Editor::new(
                    &g.key,
                    &g.label,
                    Some(g.main_pid),
                    &g.cpus,
                    &g.mem,
                    prof.clone(),
                ));
            }
            Some("steam") => {
                ui.ctx().copy_text(STEAM_OPTION.to_string());
                self.toast(
                    "Copiado! Cole em Steam → Propriedades do jogo → Opções de inicialização.",
                    true,
                );
            }
            Some("relaunch") => {
                relaunch(g, self.bg_msgs.clone());
                self.toast(format!("Reabrindo {}...", g.label), true);
            }
            Some("menu-on") => {
                if let Some(d) = self.desktop_for(&g.exe) {
                    match desktop::install_override(&d) {
                        Ok(p) => {
                            self.toast(format!("Atalho do menu ajustado ({})", p.display()), true)
                        }
                        Err(e) => self.toast(format!("Não consegui ajustar o atalho: {e}"), false),
                    }
                }
            }
            Some("menu-off") => {
                if let Some(d) = self.desktop_for(&g.exe) {
                    match desktop::remove_override(&d) {
                        Ok(()) => self.toast("Atalho do menu voltou ao original", true),
                        Err(e) => self.toast(format!("Não consegui desfazer: {e}"), false),
                    }
                }
            }
            _ => {}
        }
    }

    fn profiles_tab(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(
                "Programas com escolhas próprias. Os que não estão aqui ficam no automático.",
            )
            .weak(),
        );
        ui.separator();
        if self.profiles.map.is_empty() {
            ui.label("Nenhuma escolha salva: tudo no automático.");
            return;
        }
        let items: Vec<(String, Profile)> = self
            .profiles
            .map
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut delete = None;
        let mut edit: Option<(String, Profile)> = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("profiles")
                .striped(true)
                .num_columns(6)
                .spacing([14.0, 8.0])
                .show(ui, |ui| {
                    for h in [
                        "Programa",
                        "Processador",
                        "Placa de vídeo",
                        "Uso da GPU",
                        "Lossless Scaling",
                        "",
                    ] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (k, p) in &items {
                        ui.vertical(|ui| {
                            ui.set_min_width(260.0);
                            ui.label(
                                RichText::new(if p.label.is_empty() { k } else { &p.label })
                                    .strong(),
                            );
                            ui.label(RichText::new(truncate(k, 50)).small().weak());
                        });
                        let cpu_t = self
                            .cpu_opts
                            .iter()
                            .find(|(v, _)| *v == p.cpu)
                            .map(|(_, t)| t.clone());
                        ui.label(cpu_t.unwrap_or_else(|| p.cpu.clone()));
                        ui.label(if p.gpu == "auto" {
                            "Automático".to_string()
                        } else {
                            p.gpu.clone()
                        });
                        ui.label(match p.usage.as_str() {
                            "graphics" => "jogo / gráfico",
                            "compute" => "IA / ROCm",
                            _ => "automático",
                        });
                        ui.label(if p.fg_gpu == "same" {
                            "mesma GPU".to_string()
                        } else {
                            p.fg_gpu.clone()
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Ajustar…").clicked() {
                                edit = Some((k.clone(), p.clone()));
                            }
                            if ui.button("Voltar ao automático").clicked() {
                                delete = Some(k.clone());
                            }
                        });
                        ui.end_row();
                    }
                });
        });
        if let Some((k, p)) = edit {
            let label = if p.label.is_empty() {
                k.clone()
            } else {
                p.label.clone()
            };
            self.editor = Some(editor::Editor::new(&k, &label, None, "", &[], p));
        }
        if let Some(key) = delete {
            self.send(Request::DeleteProfile { key, pid: None });
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        single::handle(ui.ctx());
        if self.last.elapsed() > Duration::from_secs(1) {
            self.refresh();
        }
        ui.ctx().request_repaint_after(Duration::from_millis(1000));
        let p = theme::pal(ui);
        egui::Panel::left("nav")
            .exact_size(214.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(p.nav)
                    .inner_margin(egui::Margin::symmetric(12, 14)),
            )
            .show(ui, |ui| self.nav(ui));
        egui::Panel::top("top")
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin {
                left: 22,
                right: 22,
                top: 16,
                bottom: 10,
            }))
            .show(ui, |ui| self.header(ui));
        // Aviso flutuante (some sozinho)
        if let Some((m, ok, t)) = &self.toast
            && t.elapsed() < Duration::from_secs(8)
        {
            let c = if *ok { p.ok } else { p.err };
            egui::Area::new(ui.make_persistent_id("toast"))
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-20.0, -20.0))
                .order(egui::Order::Foreground)
                .interactable(false)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .inner_margin(egui::Margin::symmetric(14, 10))
                        .show(ui, |ui| {
                            ui.set_max_width(460.0);
                            ui.horizontal(|ui| {
                                let (r, _) = ui.allocate_exact_size(
                                    egui::vec2(8.0, 8.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(r.center(), 4.0, c);
                                ui.label(RichText::new(m).color(p.text));
                            });
                        });
                });
            ui.ctx().request_repaint_after(Duration::from_millis(500));
        }
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(p.bg).inner_margin(egui::Margin {
                left: 22,
                right: 22,
                top: 6,
                bottom: 16,
            }))
            .show(ui, |ui| match self.tab {
                Tab::Programs => self.programs(ui),
                Tab::Map => map::ui(self, ui),
                Tab::Profiles => self.profiles_tab(ui),
                Tab::Gpus => gpus::ui(self, ui),
                Tab::Machine => machine::ui(self, ui),
                Tab::Lab => lab::ui(self, ui),
                Tab::Llm => llm::ui(self, ui),
                Tab::Stream => stream::ui(self, ui),
                Tab::Settings => settings::ui(self, ui),
            });
        let ctx = ui.ctx().clone();
        self.job_window(&ctx);
        editor::window(self, &ctx);
        if let Some(key) = self.pending_fix.take() {
            self.send(Request::FixMemory { key });
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let tail: String = s
            .chars()
            .rev()
            .take(n - 1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("…{tail}")
    }
}

/// Fecha o programa e abre de novo pelo `numapilot run` (vale o perfil novo).
fn relaunch(g: &Group, msgs: Arc<Mutex<Vec<(String, bool)>>>) {
    let argv = procfs::cmdline(g.main_pid);
    let cwd = std::fs::read_link(format!("/proc/{}/cwd", g.main_pid))
        .unwrap_or_else(|_| PathBuf::from("/"));
    const DROP: &[&str] = &[
        "DRI_PRIME",
        "MESA_VK_DEVICE_SELECT",
        "ROCR_VISIBLE_DEVICES",
        "LSFGVK_GPU",
    ];
    let env: Vec<(String, String)> = procfs::environ(g.main_pid)
        .into_iter()
        .filter(|(k, _)| !DROP.contains(&k.as_str()) && !k.starts_with("NUMAPILOT_"))
        .collect();
    let pids = g.pids.clone();
    let label = g.label.clone();
    std::thread::spawn(move || {
        let push = |m: String, ok: bool| {
            if let Ok(mut v) = msgs.lock() {
                v.push((m, ok));
            }
        };
        if argv.is_empty() {
            push(
                format!("Não consegui ler a linha de comando de {label}"),
                false,
            );
            return;
        }
        for p in &pids {
            unsafe { libc::kill(*p, libc::SIGTERM) };
        }
        let t = Instant::now();
        while pids.iter().any(|p| procfs::alive(*p)) && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(200));
        }
        if pids.iter().any(|p| procfs::alive(*p)) {
            push(
                format!("{label} não fechou; feche manualmente e abra de novo"),
                false,
            );
            return;
        }
        let mut c = std::process::Command::new(desktop::numapilot_bin());
        c.arg("run")
            .arg("--")
            .args(&argv)
            .env_clear()
            .envs(env)
            .current_dir(cwd);
        c.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        unsafe {
            c.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        match c.spawn() {
            Ok(_) => push(format!("{label} reaberto com a nova escolha"), true),
            Err(e) => push(format!("Não consegui reabrir {label}: {e}"), false),
        }
    });
}

fn main() -> eframe::Result {
    if std::env::args().any(|a| a == "--tray") {
        if let Err(e) = tray::main_loop() {
            eprintln!("numapilot: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    // Já tem uma janela aberta: ela vem para frente e esta não abre outra.
    if single::show_existing() {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("numapilot — processador e placa de vídeo por programa")
            .with_app_id("numapilot")
            .with_inner_size([1340.0, 780.0])
            .with_min_inner_size([800.0, 400.0]),
        ..Default::default()
    };
    let r = eframe::run_native(
        "numapilot",
        options,
        Box::new(|cc| {
            single::listen(cc.egui_ctx.clone());
            Ok(Box::new(App::new(cc.egui_ctx.clone())))
        }),
    );
    single::cleanup();
    r
}

pub(crate) fn in_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
}
