//! Aba Programas: tabela no estilo de um monitor do sistema (colunas ordenáveis, linhas que
//! abrem para mostrar os processos e os controles do numapilot, Encerrar/Forçar parada).

use crate::{App, Group, fmt_bytes, theme};
use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Vec2, pos2, vec2};
use numapilot::procctl;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Col {
    Name,
    Pid,
    Cpu,
    Mem,
    Disk,
    Gpu,
    Vram,
    Card,
}

const COLS: &[(Col, &str, f32)] = &[
    (Col::Name, "Name", 0.0),
    (Col::Pid, "PID", 72.0),
    (Col::Cpu, "CPU", 70.0),
    (Col::Mem, "Memory", 92.0),
    (Col::Disk, "Disk", 84.0),
    (Col::Gpu, "GPU", 62.0),
    (Col::Vram, "GPU memory", 118.0),
    (Col::Card, "Card", 120.0),
];

const ROW_H: f32 = 30.0;
const HEAD_H: f32 = 38.0;

/// Estado da tabela (ordem, linhas abertas, seleção) e as medições por processo.
pub(crate) struct ProgTable {
    sort: Col,
    desc: bool,
    expanded: HashSet<String>,
    pub(crate) selected: Option<String>,
    pub(crate) filter: String,
    confirm_force: Option<String>,
    gpu_prev: HashMap<i32, (u64, Instant)>,
    /// Uso de GPU de cada processo (% de um motor), medido entre duas atualizações.
    gpu_pct: HashMap<i32, f32>,
    vram: HashMap<i32, u64>,
    pub(crate) pid_mem: HashMap<i32, u64>,
    io_prev: HashMap<i32, (u64, Instant)>,
    /// Leitura + escrita no disco de cada processo (bytes/s).
    disk_bps: HashMap<i32, f64>,
    /// Janela "Buscar app": programas instalados (lidos ao abrir a janela).
    pub(crate) finder: Option<Finder>,
    /// Uso de GPU de cada programa separado por placa (PCI), para conferir a escolha.
    by_pci: HashMap<String, HashMap<String, procctl::GpuUse>>,
    /// Por que o programa (processo principal) não pode ser encerrado; ausente = pode.
    blocked: HashMap<i32, &'static str>,
    /// Seções fechadas: 0 = Aplicativos, 1 = Sessão e sistema.
    closed: [bool; 2],
}

impl Default for ProgTable {
    fn default() -> Self {
        ProgTable {
            sort: Col::Cpu,
            desc: true,
            expanded: HashSet::new(),
            selected: None,
            filter: String::new(),
            confirm_force: None,
            gpu_prev: HashMap::new(),
            gpu_pct: HashMap::new(),
            vram: HashMap::new(),
            pid_mem: HashMap::new(),
            io_prev: HashMap::new(),
            disk_bps: HashMap::new(),
            finder: None,
            by_pci: HashMap::new(),
            blocked: HashMap::new(),
            closed: [false, false],
        }
    }
}

impl ProgTable {
    /// Mede memória e GPU de cada processo (chamado a cada atualização da interface).
    pub(crate) fn sample(&mut self, groups: &[Group]) {
        let now = Instant::now();
        let mut prev = HashMap::new();
        self.gpu_pct.clear();
        self.vram.clear();
        self.pid_mem.clear();
        self.disk_bps.clear();
        let mut io_prev = HashMap::new();
        self.blocked.clear();
        self.by_pci.clear();
        let me = unsafe { libc::getuid() };
        for g in groups {
            // só para quem tem uma placa escolhida (é o que dá para conferir)
            if g.gpus.len() == 1 {
                let mut by: HashMap<String, procctl::GpuUse> = HashMap::new();
                for &pid in &g.pids {
                    for (pci, u) in procctl::gpu_use_by_pci(pid) {
                        let e = by.entry(pci).or_default();
                        e.engine_ns += u.engine_ns;
                        e.vram += u.vram;
                    }
                }
                self.by_pci.insert(g.key.clone(), by);
            }
            if let Some(why) = procctl::kill_blocked(g.main_pid, me) {
                self.blocked.insert(g.main_pid, why);
            }
            for &pid in &g.pids {
                if let Some(b) = crate::pss_bytes(pid) {
                    self.pid_mem.insert(pid, b);
                }
                if let Some((r, w)) = numapilot::iostat::proc_io(pid) {
                    let tot = r + w;
                    if let Some((p, t)) = self.io_prev.get(&pid) {
                        let dt = now.duration_since(*t).as_secs_f64();
                        if dt > 0.05 {
                            self.disk_bps
                                .insert(pid, tot.saturating_sub(*p) as f64 / dt);
                        }
                    }
                    io_prev.insert(pid, (tot, now));
                }
                let Some(u) = procctl::gpu_use(pid) else {
                    continue;
                };
                self.vram.insert(pid, u.vram);
                if let Some((ns, t)) = self.gpu_prev.get(&pid) {
                    let dt = now.duration_since(*t).as_nanos() as f64;
                    if dt > 0.0 && u.engine_ns >= *ns {
                        let pct = ((u.engine_ns - ns) as f64 / dt * 100.0).clamp(0.0, 100.0);
                        self.gpu_pct.insert(pid, pct as f32);
                    }
                }
                prev.insert(pid, (u.engine_ns, now));
            }
        }
        self.gpu_prev = prev;
        self.io_prev = io_prev;
    }

    fn mem(&self, pids: &[i32]) -> u64 {
        pids.iter().filter_map(|p| self.pid_mem.get(p)).sum()
    }
    fn disk(&self, pids: &[i32]) -> Option<f64> {
        let v: Vec<f64> = pids
            .iter()
            .filter_map(|p| self.disk_bps.get(p).copied())
            .collect();
        (!v.is_empty()).then(|| v.iter().sum())
    }
    fn gpu(&self, pids: &[i32]) -> Option<f32> {
        let v: Vec<f32> = pids
            .iter()
            .filter_map(|p| self.gpu_pct.get(p).copied())
            .collect();
        (!v.is_empty()).then(|| v.iter().sum::<f32>().min(100.0))
    }
    fn vram_of(&self, pids: &[i32]) -> Option<u64> {
        let v: Vec<u64> = pids
            .iter()
            .filter_map(|p| self.vram.get(p).copied())
            .collect();
        (!v.is_empty()).then(|| v.iter().sum())
    }
}

/// Valores de uma linha, já formatados.
struct Cells {
    pid: String,
    cpu: String,
    mem: String,
    disk: String,
    gpu: String,
    vram: String,
    card: String,
}

impl Cells {
    fn get(&self, c: Col) -> &str {
        match c {
            Col::Name => "",
            Col::Pid => &self.pid,
            Col::Cpu => &self.cpu,
            Col::Mem => &self.mem,
            Col::Disk => &self.disk,
            Col::Gpu => &self.gpu,
            Col::Vram => &self.vram,
            Col::Card => &self.card,
        }
    }
}

fn pct(v: f64) -> String {
    if v < 0.05 {
        "0%".into()
    } else if v < 10.0 {
        format!("{v:.1}%")
    } else {
        format!("{v:.0}%")
    }
}

/// Colunas visíveis e suas larguras (a do nome ocupa o que sobra).
fn layout(width: f32) -> Vec<(Col, &'static str, f32)> {
    let mut cols: Vec<(Col, &str, f32)> = COLS
        .iter()
        .map(|(c, n, w)| (*c, crate::i18n::tr(n), *w))
        .filter(|(c, _, _)| {
            !(width < 900.0 && *c == Col::Card
                || width < 760.0 && *c == Col::Vram
                || width < 640.0 && *c == Col::Disk
                || width < 560.0 && *c == Col::Pid)
        })
        .collect();
    let fixed: f32 = cols.iter().map(|c| c.2).sum();
    cols[0].2 = (width - fixed).max(160.0);
    cols
}

pub(crate) fn ui(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::pal(ui);
    let groups = app.groups.clone();
    let threads = app.topo.all_cpus().len().max(1) as f64;
    let (used_kb, total_kb) = crate::mem_system();
    let nproc: usize = groups.iter().map(|g| g.pids.len()).sum();

    // ----- barra de cima: contagem, busca, filtro, ações no selecionado -----
    let sel = app
        .progs
        .selected
        .as_ref()
        .and_then(|k| groups.iter().find(|g| &g.key == k))
        .cloned();
    let blocked = sel
        .as_ref()
        .and_then(|g| app.progs.blocked.get(&g.main_pid).copied());
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(format!("{} {}", groups.len(), crate::i18n::tr("programs running"))).size(20.0).strong());
            ui.label(RichText::new(crate::tf!("{nproc} processes", "{nproc} processos")).weak());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can = sel.is_some() && blocked.is_none();
            let why = blocked.unwrap_or(crate::i18n::tr("Select a program in the list"));
            let force = ui
                .add_enabled(can, egui::Button::new(crate::i18n::tr("⊗ Force stop")))
                .on_hover_text(crate::i18n::tr("Closes it right away (SIGKILL). Anything unsaved is lost."))
                .on_disabled_hover_text(why);
            let stop = ui
                .add_enabled(can, egui::Button::new(crate::i18n::tr("■ End")))
                .on_hover_text(crate::i18n::tr("Asks the program to close (SIGTERM), like closing the window"))
                .on_disabled_hover_text(why);
            let adjust = ui
                .add_enabled(sel.is_some(), egui::Button::new(crate::i18n::tr("Adjust…")))
                .on_hover_text(crate::i18n::tr("Cores, memory, priority, GPU and ready-made profiles"));
            if let Some(g) = &sel {
                if stop.clicked() {
                    kill(app, g, false);
                }
                if force.clicked() {
                    app.progs.confirm_force = Some(g.key.clone());
                }
                if adjust.clicked() {
                    let prof = app.profile_of(&g.key);
                    app.editor = Some(crate::editor::Editor::new(
                        &g.key,
                        &g.label,
                        Some(g.main_pid),
                        &g.cpus,
                        &g.mem,
                        prof,
                    ));
                }
            }
            ui.separator();
            if ui.button(crate::i18n::tr(if app.progs.expanded.is_empty() { crate::i18n::tr("Expand all") } else { crate::i18n::tr("Collapse all") })).clicked() {
                if app.progs.expanded.is_empty() {
                    app.progs.expanded = groups.iter().map(|g| g.key.clone()).collect();
                } else {
                    app.progs.expanded.clear();
                }
            }
            ui.menu_button("Steam ⏷", |ui| {
                if ui
                    .button(crate::i18n::tr("Apply to all games"))
                    .on_hover_text(crate::i18n::tr("Puts \"numapilot run -- %command%\" in the launch options of every installed game (keeps what was already there). Close Steam first."))
                    .clicked()
                {
                    app.start_job(crate::i18n::tr("Steam: apply to all games"), vec![crate::jobs::sibling("numapilot"), "steam".into(), "enable".into()]);
                    ui.close();
                }
                if ui.button(crate::i18n::tr("Remove from all")).clicked() {
                    app.start_job(crate::i18n::tr("Steam: remove from all games"), vec![crate::jobs::sibling("numapilot"), "steam".into(), "disable".into()]);
                    ui.close();
                }
                if ui.button(crate::i18n::tr("View list")).clicked() {
                    app.start_job(crate::i18n::tr("Steam: games"), vec![crate::jobs::sibling("numapilot"), "steam".into(), "list".into()]);
                    ui.close();
                }
            });
            if ui.checkbox(&mut app.only_mine, crate::i18n::tr("Only mine")).changed() {
                app.force_refresh();
            }
            ui.add(egui::TextEdit::singleline(&mut app.progs.filter).hint_text(crate::i18n::tr("🔍 Search")).desired_width(160.0));
        });
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(crate::i18n::tr("🔍 Find installed app…"))
            .on_hover_text(crate::i18n::tr("Lists the installed programs (menu, Flatpak, Snap, AppImage, Steam games) to choose processor, memory and card without having to open the program first."))
            .clicked()
        {
            app.progs.finder = Some(Finder::new());
        }
        ui.label(
            RichText::new(crate::i18n::tr("Click a row to select it; the arrow (or double-click) shows the processes and lets you choose processor and card. CPU changes right away; the GPU applies when the program starts."))
                .small()
                .weak(),
        );
    });
    finder_window(app, ui);
    ui.add_space(4.0);

    // ----- ordenar e filtrar -----
    let t = &app.progs;
    let f = t.filter.to_lowercase();
    let mut list: Vec<&Group> = groups
        .iter()
        .filter(|g| {
            f.is_empty()
                || g.label.to_lowercase().contains(&f)
                || g.exe.to_lowercase().contains(&f)
                || g.pids.iter().any(|p| p.to_string() == f)
        })
        .collect();
    let key = |g: &Group| -> f64 {
        match t.sort {
            Col::Pid => g.main_pid as f64,
            Col::Cpu => g.cores,
            Col::Mem => t.mem(&g.pids) as f64,
            Col::Disk => t.disk(&g.pids).unwrap_or(-1.0),
            Col::Gpu => t.gpu(&g.pids).unwrap_or(-1.0) as f64,
            Col::Vram => t.vram_of(&g.pids).map(|v| v as f64).unwrap_or(-1.0),
            _ => 0.0,
        }
    };
    list.sort_by(|a, b| {
        let o = match t.sort {
            Col::Name => a.label.to_lowercase().cmp(&b.label.to_lowercase()),
            Col::Card => a.gpus.join(",").cmp(&b.gpus.join(",")),
            _ => key(a)
                .partial_cmp(&key(b))
                .unwrap_or(std::cmp::Ordering::Equal),
        };
        let o = if t.desc { o.reverse() } else { o };
        o.then_with(|| a.label.cmp(&b.label))
    });

    let width = ui.available_width();
    let cols = layout(width);

    // ----- cabeçalho -----
    let tot_cpu = groups.iter().map(|g| g.cores).sum::<f64>() / threads * 100.0;
    let tot_mem = if total_kb > 0 {
        used_kb as f64 * 100.0 / total_kb as f64
    } else {
        0.0
    };
    let tot_gpu: f32 = groups
        .iter()
        .filter_map(|g| t.gpu(&g.pids))
        .sum::<f32>()
        .min(100.0);
    let tot_vram: u64 = groups.iter().filter_map(|g| t.vram_of(&g.pids)).sum();
    let (rect, _) = ui.allocate_exact_size(vec2(width, HEAD_H), Sense::hover());
    ui.painter().rect_filled(rect, 4.0, p.card);
    let mut x = rect.left();
    let mut clicked_col = None;
    for (i, &(c, title, w)) in cols.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(w, HEAD_H));
        let resp = ui.interact(cell, egui::Id::unique(("head", i)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(cell, 0.0, p.stroke);
        }
        if resp.clicked() {
            clicked_col = Some(c);
        }
        let arrow = if app.progs.sort == c {
            if app.progs.desc { " ▼" } else { " ▲" }
        } else {
            ""
        };
        let total = match c {
            Col::Cpu => pct(tot_cpu),
            Col::Mem => format!("{tot_mem:.0}%"),
            Col::Disk => numapilot::iostat::fmt_rate(
                app.disk_rates
                    .iter()
                    .map(|d| d.read_bps + d.write_bps)
                    .sum(),
            ),
            Col::Gpu => pct(tot_gpu as f64),
            Col::Vram if tot_vram > 0 => fmt_bytes(tot_vram),
            _ => String::new(),
        };
        let right = c != Col::Name && c != Col::Card;
        let (ax, align) = if right {
            (cell.right() - 8.0, Align2::RIGHT_CENTER)
        } else {
            (
                cell.left() + if c == Col::Name { 28.0 } else { 8.0 },
                Align2::LEFT_CENTER,
            )
        };
        let pt = ui.painter().with_clip_rect(cell);
        pt.text(
            pos2(ax, cell.top() + 13.0),
            align,
            format!("{title}{arrow}"),
            FontId::proportional(12.0),
            p.weak,
        );
        if !total.is_empty() {
            pt.text(
                pos2(ax, cell.top() + 27.0),
                align,
                total,
                FontId::proportional(11.0),
                p.weak,
            );
        }
        x += w;
    }
    if let Some(c) = clicked_col {
        let t = &mut app.progs;
        if t.sort == c {
            t.desc = !t.desc;
        } else {
            t.sort = c;
            // nome em ordem alfabética; números do maior para o menor
            t.desc = !matches!(c, Col::Name | Col::Card);
        }
    }

    if groups.is_empty() {
        ui.add_space(12.0);
        ui.label(crate::i18n::tr("No program in use right now. Open something (a game, Blender, llama-server...) and it shows up here."));
        return;
    }

    // ----- linhas -----
    // Aplicativos (dá para encerrar) e Sessão e sistema (protegidos), como num monitor do sistema
    let (apps, sys): (Vec<Group>, Vec<Group>) = list
        .into_iter()
        .cloned()
        .partition(|g| !app.progs.blocked.contains_key(&g.main_pid));
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (si, (title, list)) in [(crate::i18n::tr("Applications"), apps), (crate::i18n::tr("Session and system"), sys)]
                .into_iter()
                .enumerate()
            {
                if list.is_empty() {
                    continue;
                }
                let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_H + 4.0), Sense::hover());
                let resp = ui.interact(rect, egui::Id::unique(("sec", si)), Sense::click());
                let closed = app.progs.closed[si];
                ui.painter().rect_filled(
                    rect,
                    4.0,
                    if resp.hovered() {
                        p.stroke
                    } else {
                        p.accent.gamma_multiply(0.18)
                    },
                );
                ui.painter().text(
                    pos2(rect.left() + 13.0, rect.center().y),
                    Align2::CENTER_CENTER,
                    if closed { "⏵" } else { "⏷" },
                    FontId::proportional(13.0),
                    p.text,
                );
                ui.painter().text(
                    pos2(rect.left() + 28.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    format!("{title} ({})", list.len()),
                    FontId::proportional(14.0),
                    p.text,
                );
                if si == 1 {
                    ui.painter().text(
                        pos2(rect.right() - 8.0, rect.center().y),
                        Align2::RIGHT_CENTER,
                        crate::i18n::tr("cannot be ended from here"),
                        FontId::proportional(11.0),
                        p.weak,
                    );
                }
                if resp.clicked() {
                    app.progs.closed[si] = !closed;
                }
                if closed {
                    continue;
                }
                for (i, g) in list.iter().enumerate() {
                    let open = app.progs.expanded.contains(&g.key);
                    // o programa está na placa que foi escolhida para ele?
                    let wrong: Option<(String, String)> = g.gpus.first().and_then(|alias| {
                        let want = app.topo.gpus.iter().find(|x| &x.alias == alias)?;
                        let by = app.progs.by_pci.get(&g.key)?;
                        let got = procctl::wrong_gpu(by, &want.pci, want.is_nvidia())?;
                        let name = |x: &numapilot::topo::Gpu| format!("{} ({})", x.alias, x.short_name());
                        Some((
                            name(want),
                            app.topo
                                .gpus
                                .iter()
                                .find(|x| x.pci == got)
                                .map(name)
                                .unwrap_or(got),
                        ))
                    });
                    let t = &app.progs;
                    let cells = Cells {
                        pid: g.main_pid.to_string(),
                        cpu: pct(g.cores / threads * 100.0),
                        mem: fmt_mem(t.mem(&g.pids)),
                        disk: fmt_disk(t.disk(&g.pids)),
                        gpu: t
                            .gpu(&g.pids)
                            .map(|v| pct(v as f64))
                            .unwrap_or_else(|| "—".into()),
                        vram: t
                            .vram_of(&g.pids)
                            .map(fmt_mem)
                            .unwrap_or_else(|| "—".into()),
                        card: if g.gpus.is_empty() {
                            "—".into()
                        } else if wrong.is_some() {
                            format!("{} ≠", g.gpus.join(", "))
                        } else {
                            g.gpus.join(", ")
                        },
                    };
                    let sub = if g.pids.len() > 1 {
                        crate::tf!("{} processes", "{} processos", g.pids.len())
                    } else {
                        String::new()
                    };
                    let selected = app.progs.selected.as_deref() == Some(&g.key);
                    let r = row(
                        ui,
                        &p,
                        &cols,
                        0,
                        Some(open),
                        &g.label,
                        &sub,
                        &cells,
                        selected,
                        i % 2 == 1,
                        egui::Id::unique(("row", &g.key)),
                    );
                    if r.toggle {
                        if open {
                            app.progs.expanded.remove(&g.key);
                        } else {
                            app.progs.expanded.insert(g.key.clone());
                        }
                    }
                    if r.select {
                        app.progs.selected = Some(g.key.clone());
                    }
                    r.resp
                        .on_hover_text(format!("{}\nPIDs: {:?}", g.exe, g.pids));
                    if open {
                        // processos do programa
                        let mut procs = g.procs.clone();
                        procs.sort_by(|a, b| {
                            b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        for (pid, comm, cores) in &procs {
                            let t = &app.progs;
                            let pids = [*pid];
                            let c = Cells {
                                pid: pid.to_string(),
                                cpu: pct(cores / threads * 100.0),
                                mem: fmt_mem(t.mem(&pids)),
                                disk: fmt_disk(t.disk(&pids)),
                                gpu: t
                                    .gpu(&pids)
                                    .map(|v| pct(v as f64))
                                    .unwrap_or_else(|| "—".into()),
                                vram: t.vram_of(&pids).map(fmt_mem).unwrap_or_else(|| "—".into()),
                                card: String::new(),
                            };
                            row(
                                ui,
                                &p,
                                &cols,
                                1,
                                None,
                                comm,
                                "",
                                &c,
                                false,
                                false,
                                egui::Id::unique(("proc", pid)),
                            );
                        }
                        // controles do numapilot para o programa
                        egui::Frame::new()
                            .fill(p.card)
                            .inner_margin(egui::Margin {
                                left: 28,
                                right: 8,
                                top: 6,
                                bottom: 8,
                            })
                            .show(ui, |ui| {
                                ui.set_width(width - 36.0);
                                if let Some((want, got)) = &wrong {
                                    ui.label(
                                        RichText::new(crate::tf!(
                                            "Different card from the chosen one: asked for {want}, but it is using {got}. \
                                             The card choice only applies on start: use Reopen. If it persists, \
                                             check with \"Test this card's selection\" in the Graphics cards tab.", "Placa diferente da escolhida: pediu {want}, mas está usando {got}. \
                                             A escolha de placa só vale ao abrir: use Reabrir. Se continuar, \
                                             confira com \"Testar a escolha desta placa\" na aba Placas de vídeo."
                                        ))
                                        .color(p.warn),
                                    );
                                }
                                ui.horizontal_wrapped(|ui| app.program_row(ui, g));
                            });
                        ui.add_space(4.0);
                    }
                }
                ui.add_space(6.0);
            }
        });

    // ----- confirmar "Forçar parada" -----
    if let Some(k) = app.progs.confirm_force.clone() {
        let g = groups.iter().find(|g| g.key == k).cloned();
        let mut close = g.is_none();
        if let Some(g) = g {
            egui::Window::new(crate::i18n::tr("Force stop"))
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ui.ctx(), |ui| {
                    ui.label(crate::tf!(
                        "Close \"{}\" right away ({} process(es))?",
                        "Fechar \"{}\" na hora ({} processo(s))?",
                        g.label,
                        g.pids.len()
                    ));
                    ui.label(
                        RichText::new(crate::i18n::tr("Anything unsaved in it is lost."))
                            .color(p.warn),
                    );
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new(crate::i18n::tr("Force stop")).color(p.err))
                            .clicked()
                        {
                            kill(app, &g, true);
                            close = true;
                        }
                        if ui.button(crate::i18n::tr("Cancel")).clicked() {
                            close = true;
                        }
                    });
                });
        }
        if close {
            app.progs.confirm_force = None;
        }
    }
}

fn fmt_disk(v: Option<f64>) -> String {
    match v {
        None => "—".into(),
        Some(b) if b < 1024.0 => "0".into(),
        Some(b) => numapilot::iostat::fmt_rate(b),
    }
}

fn fmt_mem(b: u64) -> String {
    if b == 0 {
        "—".into()
    } else if b < 1048576 {
        format!("{} KB", b / 1024)
    } else {
        fmt_bytes(b)
    }
}

fn kill(app: &mut App, g: &Group, force: bool) {
    let (n, refused) = procctl::terminate(&g.pids, app.my_uid, force);
    numapilot::log::info(&format!(
        "{} {} (pids {:?}): {n} sinal(is), recusa: {refused:?}",
        if force {
            "forçar parada de"
        } else {
            "encerrar"
        },
        g.label,
        g.pids
    ));
    match (n, refused) {
        (0, Some(why)) => app.toast(
            crate::tf!("Did not end {}: {why}", "Não encerrei {}: {why}", g.label),
            false,
        ),
        (_, Some(why)) => app.toast(
            crate::tf!(
                "{}: {n} process(es) ended; skipped {why}",
                "{}: {n} processo(s) encerrado(s); pulei {why}",
                g.label
            ),
            true,
        ),
        _ => app.toast(
            format!(
                "{} {}",
                if force {
                    crate::i18n::tr("Forced stop:")
                } else {
                    crate::i18n::tr("Ending")
                },
                g.label
            ),
            true,
        ),
    }
    app.progs.selected = None;
    app.force_refresh();
}

struct RowResp {
    resp: egui::Response,
    toggle: bool,
    select: bool,
}

#[allow(clippy::too_many_arguments)]
fn row(
    ui: &mut egui::Ui,
    p: &theme::Pal,
    cols: &[(Col, &str, f32)],
    depth: u8,
    expander: Option<bool>,
    name: &str,
    sub: &str,
    cells: &Cells,
    selected: bool,
    stripe: bool,
    id: egui::Id,
) -> RowResp {
    let width: f32 = cols.iter().map(|c| c.2).sum();
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
    let resp = ui.interact(rect, id, Sense::click());
    let bg = if selected {
        p.accent.gamma_multiply(0.35)
    } else if resp.hovered() {
        p.stroke
    } else if stripe {
        p.card.gamma_multiply(0.6)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 3.0, bg);
    let mut toggle = false;
    let indent = depth as f32 * 22.0;
    if let Some(open) = expander {
        let arrow = Rect::from_min_size(pos2(rect.left() + indent, rect.top()), vec2(26.0, ROW_H));
        let hit = resp.clicked()
            && resp
                .interact_pointer_pos()
                .is_some_and(|q| arrow.contains(q));
        toggle = hit || resp.double_clicked();
        ui.painter().text(
            arrow.center(),
            Align2::CENTER_CENTER,
            if open { "⏷" } else { "⏵" },
            FontId::proportional(13.0),
            p.weak,
        );
    }
    let mut x = rect.left();
    for &(c, _, w) in cols {
        let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(w, ROW_H));
        let pt = ui.painter().with_clip_rect(cell.shrink2(vec2(4.0, 0.0)));
        if c == Col::Name {
            let tx = cell.left() + indent + 28.0;
            let color = if depth == 0 { p.text } else { p.weak };
            let r = pt.text(
                pos2(tx, cell.center().y),
                Align2::LEFT_CENTER,
                name,
                FontId::proportional(14.0),
                color,
            );
            if !sub.is_empty() {
                pt.text(
                    pos2(r.right() + 8.0, cell.center().y + 1.0),
                    Align2::LEFT_CENTER,
                    sub,
                    FontId::proportional(11.0),
                    p.weak,
                );
            }
        } else {
            let v = cells.get(c);
            let (ax, al) = if c == Col::Card {
                (cell.left() + 8.0, Align2::LEFT_CENTER)
            } else {
                (cell.right() - 8.0, Align2::RIGHT_CENTER)
            };
            pt.text(
                pos2(ax, cell.center().y),
                al,
                v,
                FontId::proportional(13.0),
                if v == "—" { p.weak } else { p.text },
            );
        }
        x += w;
    }
    let select = resp.clicked() && !toggle;
    RowResp {
        resp,
        toggle,
        select,
    }
}

/// Janela de busca de programas instalados.
pub(crate) struct Finder {
    apps: Vec<numapilot::apps::App>,
    query: String,
    source: Option<&'static str>,
    focus: bool,
}

impl Finder {
    fn new() -> Self {
        Finder {
            apps: numapilot::apps::list(),
            query: String::new(),
            source: None,
            focus: true,
        }
    }
}

fn finder_window(app: &mut App, ui: &mut egui::Ui) {
    let Some(mut f) = app.progs.finder.take() else {
        return;
    };
    let p = theme::pal(ui);
    let mut open = true;
    let mut pick: Option<numapilot::apps::App> = None;
    let mut menu_fix: Option<std::path::PathBuf> = None;
    egui::Window::new(crate::i18n::tr("Find installed app"))
        .open(&mut open)
        .collapsible(false)
        .default_size([640.0, 520.0])
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut f.query)
                        .hint_text(crate::i18n::tr("🔍 Program name, executable or source"))
                        .desired_width(360.0),
                );
                if f.focus {
                    r.request_focus();
                    f.focus = false;
                }
                if ui.button(crate::i18n::tr("Reload")).clicked() {
                    f.apps = numapilot::apps::list();
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let mut sources: Vec<&'static str> = f.apps.iter().map(|a| a.source).collect();
                sources.sort_unstable();
                sources.dedup();
                if ui.selectable_label(f.source.is_none(), crate::tf!("All ({})", "Todos ({})", f.apps.len())).clicked() {
                    f.source = None;
                }
                for s in sources {
                    let n = f.apps.iter().filter(|a| a.source == s).count();
                    if ui.selectable_label(f.source == Some(s), format!("{s} ({n})")).clicked() {
                        f.source = Some(s);
                    }
                }
            });
            ui.label(
                RichText::new(
                    crate::i18n::tr("The choice is saved and applies when the program starts. Processor and memory apply on any \
                     start; for the graphics card, open it from the adjusted menu (option in Adjust… itself) or with Reopen."),
                )
                .small()
                .weak(),
            );
            ui.separator();
            let shown: Vec<&numapilot::apps::App> = f
                .apps
                .iter()
                .filter(|a| f.source.is_none_or(|s| a.source == s))
                .filter(|a| numapilot::apps::matches(a, &f.query))
                .collect();
            if shown.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("No program found with this search.")).weak());
            }
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, 44.0, shown.len(), |ui, range| {
                for a in &shown[range] {
                    let saved = app.profiles.map.contains_key(&a.key);
                    let running = app.groups.iter().any(|g| g.key == a.key);
                    ui.horizontal(|ui| {
                        ui.set_height(40.0);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&a.name).family(egui::FontFamily::Name(theme::BOLD.into())));
                                ui.label(RichText::new(a.source).small().color(p.accent));
                                if saved {
                                    ui.label(RichText::new(crate::i18n::tr("saved choice")).small().color(p.ok));
                                }
                                if running {
                                    ui.label(RichText::new(crate::i18n::tr("open now")).small().color(p.weak));
                                }
                            });
                            ui.label(RichText::new(&a.detail).small().weak());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(8.0);
                            if ui.button(crate::i18n::tr("Adjust…")).clicked() {
                                pick = Some((*a).clone());
                            }
                            // a placa de vídeo só vale se o programa abrir pelo numapilot
                            if let Some(d) = &a.desktop {
                                if numapilot::desktop::is_overridden(d) {
                                    ui.label(RichText::new(crate::i18n::tr("menu adjusted")).small().color(p.ok))
                                        .on_hover_text(crate::i18n::tr("Opened from the menu, this program already goes through numapilot (the card choice applies)."));
                                } else if saved
                                    && ui
                                        .small_button(crate::i18n::tr("Adjust the menu"))
                                        .on_hover_text(crate::i18n::tr("Makes the menu shortcut open this program through numapilot, so the graphics card choice applies. Can be undone in Saved choices."))
                                        .clicked()
                                {
                                    menu_fix = Some(d.clone());
                                }
                            }
                        });
                    });
                    ui.separator();
                }
            });
        });
    if let Some(d) = menu_fix {
        match numapilot::desktop::install_override(&d) {
            Ok(pth) => app.toast(
                crate::tf!(
                    "Menu shortcut adjusted ({})",
                    "Atalho do menu ajustado ({})",
                    pth.display()
                ),
                true,
            ),
            Err(e) => app.toast(
                crate::tf!(
                    "Could not adjust the shortcut: {e}",
                    "Não consegui ajustar o atalho: {e}"
                ),
                false,
            ),
        }
    }
    if let Some(a) = pick {
        // se estiver aberto, usa os dados do programa em execução (vale na hora)
        let g = app.groups.iter().find(|g| g.key == a.key).cloned();
        let prof = app.profile_of(&a.key);
        app.editor = Some(match g {
            Some(g) => crate::editor::Editor::new(
                &g.key,
                &g.label,
                Some(g.main_pid),
                &g.cpus,
                &g.mem,
                prof,
            ),
            None => crate::editor::Editor::new(&a.key, &a.name, None, "", &[], prof),
        });
        numapilot::log::info(&format!(
            "buscar app: {} ({}) → {}",
            a.name, a.source, a.key
        ));
        open = false;
    }
    if open {
        app.progs.finder = Some(f);
    }
}
