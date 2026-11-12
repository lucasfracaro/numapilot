//! "Map & measurements" tab: each socket's cores (usage and who is on each), memory,
//! remote allocations, frequency governor and the measured latency/bandwidth matrix.

use crate::App;
use crate::lab::gfx;
use eframe::egui::{self, Color32, RichText, Sense, Stroke, StrokeKind};
use numapilot::ctl::Request;
use numapilot::topo::Topology;
use std::collections::{BTreeMap, BTreeSet};

pub fn load_color(l: f32) -> Color32 {
    let l = l.clamp(0.0, 1.0);
    // dark green (free) → yellow → red (full)
    let (r, g, b) = if l < 0.5 {
        let t = l / 0.5;
        (40.0 + 180.0 * t, 70.0 + 110.0 * t, 50.0)
    } else {
        let t = (l - 0.5) / 0.5;
        (220.0 + 10.0 * t, 180.0 - 120.0 * t, 50.0)
    };
    Color32::from_rgb(r as u8, g as u8, b as u8)
}

/// Draws each socket's cores. If `selected` is given, clicking selects/unselects CPUs.
/// `who`: CPU → programs restricted to it (for the hover tooltip).
pub fn core_map(
    ui: &mut egui::Ui,
    topo: &Topology,
    loads: &BTreeMap<usize, f32>,
    mut selected: Option<&mut BTreeSet<usize>>,
    who: &BTreeMap<usize, Vec<String>>,
) {
    let accent = ui.visuals().selection.stroke.color;
    for n in &topo.nodes {
        let gpus: Vec<&str> = topo
            .gpus
            .iter()
            .filter(|g| g.numa_node == Some(n.id))
            .map(|g| g.alias.as_str())
            .collect();
        ui.label(
            RichText::new(crate::tf!(
                "Socket {} — {} cores / {} threads{}",
                "Socket {} — {} núcleos / {} threads{}",
                n.id,
                n.cores.len(),
                n.cpus.len(),
                if gpus.is_empty() {
                    String::new()
                } else {
                    format!(" — GPUs: {}", gpus.join(", "))
                }
            ))
            .strong(),
        );
        let per_row = 11usize;
        for row in n.cores.chunks(per_row) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for core in row {
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(36.0, 22.0), Sense::click());
                    let p = ui.painter();
                    let w = rect.width() / core.len().max(1) as f32;
                    for (k, &c) in core.iter().enumerate() {
                        let r = egui::Rect::from_min_size(
                            rect.min + egui::vec2(w * k as f32, 0.0),
                            egui::vec2(w - 1.0, rect.height()),
                        );
                        p.rect_filled(r, 3.0, load_color(loads.get(&c).copied().unwrap_or(0.0)));
                        if selected.as_ref().is_some_and(|s| s.contains(&c)) {
                            p.rect_stroke(
                                r.shrink(1.0),
                                3.0,
                                Stroke::new(2.0, accent),
                                StrokeKind::Inside,
                            );
                        }
                        if who.contains_key(&c) {
                            p.circle_filled(
                                r.right_top() + egui::vec2(-4.0, 4.0),
                                2.5,
                                Color32::WHITE,
                            );
                        }
                    }
                    let tip = core
                        .iter()
                        .map(|c| {
                            let mut s = format!(
                                "CPU {c}: {:.0}%",
                                loads.get(c).copied().unwrap_or(0.0) * 100.0
                            );
                            if let Some(ps) = who.get(c) {
                                s.push_str(&format!(" — {}", ps.join(", ")));
                            }
                            s
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let resp = resp.on_hover_text(tip);
                    if resp.clicked()
                        && let Some(sel) = selected.as_deref_mut()
                    {
                        // Click: toggles the whole core (both threads).
                        if core.iter().all(|c| sel.contains(c)) {
                            for c in core {
                                sel.remove(c);
                            }
                        } else {
                            sel.extend(core.iter().copied());
                        }
                    }
                    if resp.secondary_clicked()
                        && let Some(sel) = selected.as_deref_mut()
                    {
                        // Right-click: only the 1st thread (no hyper-threading on this core).
                        for c in core {
                            sel.remove(c);
                        }
                        sel.insert(core[0]);
                    }
                }
            });
        }
        ui.add_space(4.0);
    }
}

/// A sparkline of the last minute per thread (grouped by socket) and the processor
/// temperature.
fn thread_graphs(app: &App, ui: &mut egui::Ui, who: &BTreeMap<usize, Vec<String>>) {
    let p = crate::theme::pal(ui);
    let n_all = app.cpu_loads.len().max(1);
    let h = if n_all > 48 { 34.0 } else { 42.0 };
    let min_w = if n_all > 48 { 70.0 } else { 92.0 };
    let multi = app.topo.nodes.len() > 1;
    for n in &app.topo.nodes {
        let info = app.cpu_info(n);
        ui.label(
            RichText::new(format!(
                "{}{} — {} threads{}",
                app.cpu_name,
                if multi {
                    format!(" (socket {})", n.id)
                } else {
                    String::new()
                },
                n.cpus.len(),
                if info.is_empty() {
                    String::new()
                } else {
                    format!(" — {info}")
                }
            ))
            .size(12.5)
            .color(p.weak),
        );
        // turbo: how many cores are awake and how far the clock can go with that
        let pkg = n.id as u32;
        let deep = n
            .cpus
            .first()
            .and_then(|c| numapilot::turbo::deep_enabled(*c));
        if let Some(a) = app.awake.get(&pkg) {
            let total = n.physical_cores.max(n.cores.len());
            let mut txt = crate::tf!(
                "Turbo: ~{a:.1} of {total} cores awake",
                "Turbo: ~{a:.1} de {total} núcleos acordados"
            );
            let table = app.turbo_tables.get(&pkg);
            if let Some(mhz) = table.and_then(|t| numapilot::turbo::limit_for(t, *a)) {
                txt.push_str(&crate::tf!(
                    " → up to {:.1} GHz",
                    " → até {:.1} GHz",
                    mhz as f64 / 1000.0
                ));
            }
            let r = ui.label(RichText::new(txt).size(12.5).color(p.weak));
            match table {
                Some(t) => {
                    r.on_hover_text(crate::tf!(
                        "The fewer cores awake on this processor, the higher the clock of the ones in use.\n\
                         Cores awake → maximum clock:\n{}\n\
                         Programs with heavy AVX2 use a slightly lower table.", "Quanto menos núcleos acordados neste processador, mais alto o clock dos que estão em uso.\n\
                         Núcleos acordados → clock máximo:\n{}\n\
                         Programas com AVX2 pesado usam uma tabela um pouco mais baixa.",
                        numapilot::turbo::describe(t)
                    ));
                }
                None => {
                    r.on_hover_text(crate::i18n::tr(
                        "The turbo table comes from the numapilot service (Intel processors only).",
                    ));
                }
            }
            if deep == Some(false) {
                ui.label(
                    RichText::new(
                        crate::i18n::tr("The deep idle states (C3/C6) are off: every core counts as \
                         awake and turbo stays at the all-core value. Turn them on in Machine → Processor."),
                    )
                    .size(12.5)
                    .color(p.warn),
                );
            }
        }
        let mut cpus = n.cpus.clone();
        cpus.sort_unstable();
        // equal columns that fill the width; preferably a divisor of the thread count
        // (16 threads → 2 rows of 8)
        let gap = 6.0;
        // leave room for the scroll bar
        let avail = ui.available_width() - 12.0;
        let max_cols = (((avail + gap) / (min_w + gap)) as usize).clamp(1, cpus.len().max(1));
        let cols = (max_cols.div_ceil(2)..=max_cols)
            .rev()
            .find(|c| cpus.len() % c == 0)
            .unwrap_or(max_cols);
        let w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::TOP).with_main_wrap(true),
            |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                for c in cpus {
                    let l = app.cpu_loads.get(&c).copied().unwrap_or(0.0);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
                    let painter = ui.painter_at(rect.expand(1.0));
                    painter.rect(
                        rect,
                        6.0,
                        p.card,
                        Stroke::new(1.0, p.stroke),
                        StrokeKind::Inside,
                    );
                    let color = crate::theme::level_color(&p, l, p.accent);
                    if let Some(sp) = app.cpu_sparks.get(&c) {
                        let g = egui::Rect::from_min_max(
                            rect.left_top() + egui::vec2(2.0, 16.0),
                            rect.right_bottom() - egui::vec2(2.0, 2.0),
                        );
                        crate::theme::spark_in(&painter, g, sp, color);
                    }
                    let f = egui::FontId::proportional(10.5);
                    painter.text(
                        rect.left_top() + egui::vec2(5.0, 3.0),
                        egui::Align2::LEFT_TOP,
                        c.to_string(),
                        f.clone(),
                        p.weak,
                    );
                    painter.text(
                        rect.right_top() + egui::vec2(-5.0, 3.0),
                        egui::Align2::RIGHT_TOP,
                        format!("{:.0}%", l * 100.0),
                        f,
                        p.text,
                    );
                    if who.contains_key(&c) {
                        painter.circle_filled(
                            rect.left_bottom() + egui::vec2(6.0, -6.0),
                            2.5,
                            p.warn,
                        );
                    }
                    let core = n.cores.iter().position(|k| k.contains(&c));
                    let mut tip = format!("CPU {c}: {:.0}%", l * 100.0);
                    if let Some(k) = core {
                        tip.push_str(&crate::tf!(" · core {k}", " · núcleo {k}"));
                    }
                    if let Some(mhz) = app.cpu_freq.get(&c) {
                        tip.push_str(&format!(" · {:.2} GHz", *mhz as f64 / 1000.0));
                    }
                    if let Some(ps) = who.get(&c) {
                        tip.push_str(&crate::tf!(
                            "\npinned here: {}",
                            "\npreso aqui: {}",
                            ps.join(", ")
                        ));
                    }
                    resp.on_hover_text(tip);
                }
            },
        );
        ui.add_space(4.0);
    }
    // processor temperature (one chart per socket)
    if !app.temp_spark.is_empty() {
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            for (pkg, (sp, t)) in &app.temp_spark {
                let room = ui.available_width() - 12.0;
                let wfull = if app.temp_spark.len() > 1 {
                    (room / 2.0 - 8.0).max(200.0)
                } else {
                    room
                };
                let (rect, _) = ui.allocate_exact_size(egui::vec2(wfull, 46.0), Sense::hover());
                let painter = ui.painter_at(rect.expand(1.0));
                painter.rect(
                    rect,
                    6.0,
                    p.card,
                    Stroke::new(1.0, p.stroke),
                    StrokeKind::Inside,
                );
                let c = if *t >= 90.0 {
                    p.err
                } else if *t >= 80.0 {
                    p.warn
                } else {
                    Color32::from_rgb(0xf0, 0x7a, 0x4a)
                };
                let g = egui::Rect::from_min_max(
                    rect.left_top() + egui::vec2(2.0, 18.0),
                    rect.right_bottom() - egui::vec2(2.0, 2.0),
                );
                crate::theme::spark_in(&painter, g, sp, c);
                let label = if multi {
                    crate::tf!(
                        "Temperature socket {pkg} (last minute, 30–100 °C)",
                        "Temperatura socket {pkg} (último minuto, 30–100 °C)"
                    )
                } else {
                    crate::i18n::tr("Processor temperature (last minute, 30–100 °C)").into()
                };
                painter.text(
                    rect.left_top() + egui::vec2(6.0, 4.0),
                    egui::Align2::LEFT_TOP,
                    label,
                    egui::FontId::proportional(10.5),
                    p.weak,
                );
                painter.text(
                    rect.right_top() + egui::vec2(-6.0, 4.0),
                    egui::Align2::RIGHT_TOP,
                    format!("{t:.0} °C"),
                    egui::FontId::proportional(11.5),
                    p.text,
                );
            }
        });
    }
    ui.label(
        RichText::new(crate::i18n::tr("Hover over a chart to see the core and the frequency; yellow dot = some program is pinned to it."))
            .size(11.5)
            .color(p.weak),
    );
}

fn governors(cpu: usize) -> (String, Vec<String>) {
    let base = numapilot::util::sys(&format!("devices/system/cpu/cpu{cpu}/cpufreq"));
    let cur = numapilot::util::read_trim(base.join("scaling_governor")).unwrap_or_default();
    let avail = numapilot::util::read_trim(base.join("scaling_available_governors"))
        .map(|s| s.split_whitespace().map(String::from).collect())
        .unwrap_or_else(|| {
            if cur.is_empty() {
                vec![]
            } else {
                vec![cur.clone()]
            }
        });
    (cur, avail)
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    // Who is restricted to which CPUs (programs with chosen cores).
    let all = app.topo.all_cpus().len();
    let mut who: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for g in &app.groups {
        let cpus = numapilot::util::parse_cpulist(&g.cpus);
        if !cpus.is_empty() && cpus.len() < all {
            for c in cpus {
                who.entry(c).or_default().push(g.label.clone());
            }
        }
    }
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.selectable_value(&mut app.map_threads, true, crate::i18n::tr("Per-thread chart"));
            ui.selectable_value(&mut app.map_threads, false, crate::i18n::tr("Core map"));
        });
        ui.add_space(4.0);
        if app.map_threads {
            thread_graphs(app, ui, &who);
        } else {
            ui.label(
                RichText::new(
                    crate::i18n::tr("Each square is a physical core (split into its 2 threads). Color = usage; white dot = some program \
                     is pinned to it (hover to see)."),
                )
                .weak(),
            );
            core_map(ui, &app.topo, &app.cpu_loads, None, &who);
        }
        ui.separator();
        disks_and_net(app, ui);
        ui.separator();

        ui.heading(crate::i18n::tr("Memory and frequency per socket"));
        let nodes = app.topo.nodes.clone();
        egui::Grid::new("sockets").num_columns(4).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
            ui.label(RichText::new("Socket").strong());
            ui.label(RichText::new(crate::i18n::tr("Memory in use")).strong());
            ui.label(RichText::new(crate::i18n::tr("Allocations on the other socket")).strong())
                .on_hover_text(crate::i18n::tr("Memory pages that had to be created on the other socket (numastat other_node/numa_miss), per second."));
            ui.label(RichText::new(crate::i18n::tr("Governor")).strong())
                .on_hover_text(crate::i18n::tr("performance = always maximum frequency (games); schedutil/powersave = saves power."));
            ui.end_row();
            for n in &nodes {
                ui.label(format!("{}", n.id));
                let used_kb = n.mem_used_kb() as f32;
                let tot = n.mem_total_kb.max(1) as f32;
                let used = used_kb / tot;
                let p = crate::theme::pal(ui);
                ui.horizontal(|ui| {
                    let f = crate::theme::smooth(ui, &format!("node-mem-{}", n.id), used);
                    crate::theme::meter(ui, f, 160.0, crate::theme::level_color(&p, used, p.accent));
                    ui.label(format!("{:.1} / {:.0} GB", used_kb / 1048576.0, tot / 1048576.0));
                });
                let rate = app.numa_rates.get(&n.id).copied().unwrap_or((0.0, 0.0));
                let txt = crate::tf!("{:.0}/s ({:.1}% of allocations)", "{:.0}/s ({:.1}% das alocações)", rate.0, rate.1 * 100.0);
                ui.label(if rate.1 > 0.05 { RichText::new(txt).color(Color32::from_rgb(230, 160, 40)) } else { RichText::new(txt) });
                let (cur, avail) = governors(n.cpus[0]);
                if avail.is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("unavailable")).weak());
                } else {
                    let mut sel = cur.clone();
                    egui::ComboBox::from_id_salt(format!("gov-{}", n.id)).selected_text(&sel).show_ui(ui, |ui| {
                        for g in &avail {
                            ui.selectable_value(&mut sel, g.clone(), g);
                        }
                    });
                    if sel != cur {
                        app.send(Request::SetGovernor { node: n.id, governor: sel });
                    }
                }
                ui.end_row();
            }
        });
        if !numapilot::containers::which("irqbalance") || crate::jobs::quick(&["pgrep", "-x", "irqbalance"]).is_empty() {
            ui.label(RichText::new(crate::i18n::tr("GPU interrupts: handled by each card's socket.")).small().weak());
        } else {
            ui.label(
                RichText::new(
                    crate::i18n::tr("irqbalance is running and may undo the GPU interrupt tuning: \
                     sudo systemctl disable --now irqbalance"),
                )
                .small()
                .color(Color32::from_rgb(230, 160, 40)),
            );
        }
        ui.separator();

        ui.horizontal(|ui| {
            ui.heading(crate::i18n::tr("Cache latency per core"));
            if ui
                .button(crate::i18n::tr("Measure cores"))
                .on_hover_text(crate::i18n::tr("Measures L3 cache access from each core (a few seconds, with the PC idle). If the difference is larger than the noise, core choices, including maximum turbo, start preferring the fastest ones."))
                .clicked()
            {
                app.start_job(
                    crate::i18n::tr("Measuring cache latency per core"),
                    vec![crate::jobs::sibling("numapilot"), "bench-cores".into()],
                );
            }
        });
        match numapilot::corebench::saved().filter(|r| !r.ns.is_empty()) {
            Some(r) => {
                ui.label(
                    RichText::new(crate::tf!(
                        "Difference between the best and the worst core: {:.1}% (measurement noise {:.1}%). {}", "Diferença entre o melhor e o pior núcleo: {:.1}% (ruído da medição {:.1}%). {}",
                        r.spread_pct,
                        r.noise_pct,
                        if r.useful {
                            crate::i18n::tr("In use: programs with a chosen number of cores get the fastest ones.")
                        } else {
                            crate::i18n::tr("Too small to use as a criterion: the system order stays.")
                        }
                    ))
                    .weak(),
                );
            }
            None => {
                ui.label(RichText::new(crate::i18n::tr("Not measured yet.")).weak());
            }
        }
        ui.separator();

        ui.horizontal(|ui| {
            ui.heading(crate::i18n::tr("Latency and bandwidth between sockets (measured)"));
            if ui.button(crate::i18n::tr("Measure now")).on_hover_text(crate::i18n::tr("Takes ~10 s; the PC is busy during the test")).clicked() {
                let mut v = vec![crate::jobs::sibling("numapilot")];
                v.push("bench".into());
                app.start_job(crate::i18n::tr("Measuring memory latency and bandwidth"), v);
                app.bench_checked = None;
            }
        });
        match &app.bench {
            None => {
                ui.label(RichText::new(crate::i18n::tr("Not measured yet. Click \"Measure now\".")).weak());
            }
            Some(b) => {
                egui::Grid::new("bench").num_columns(b.nodes.len() + 1).spacing([18.0, 4.0]).striped(true).show(ui, |ui| {
                    ui.label(RichText::new(crate::i18n::tr("CPUs \\ memory")).small());
                    for n in &b.nodes {
                        ui.label(RichText::new(crate::tf!("memory of socket {n}", "memória do socket {n}")).strong());
                    }
                    ui.end_row();
                    for (i, n) in b.nodes.iter().enumerate() {
                        ui.label(RichText::new(crate::tf!("CPUs of socket {n}", "CPUs do socket {n}")).strong());
                        for j in 0..b.nodes.len() {
                            let lat = b.latency_ns[i][j];
                            let bw = b.bandwidth_gbs[i][j];
                            let t = format!("{lat:.0} ns · {bw:.1} GB/s");
                            ui.label(if i == j { RichText::new(t).color(Color32::from_rgb(110, 190, 120)) } else { RichText::new(t).color(Color32::from_rgb(230, 160, 40)) });
                        }
                        ui.end_row();
                    }
                });
                if b.nodes.len() >= 2 && b.latency_ns[0][0] > 0.0 {
                    let extra = (b.latency_ns[0][1] / b.latency_ns[0][0] - 1.0) * 100.0;
                    let bw = (1.0 - b.bandwidth_gbs[0][1] / b.bandwidth_gbs[0][0].max(0.001)) * 100.0;
                    ui.label(crate::tf!(
                        "Using the other socket's memory costs {extra:.0}% more latency and {bw:.0}% less bandwidth. \
                         That is why games do better pinned to one socket with local memory.", "Usar a memória do outro socket custa {extra:.0}% a mais de latência e {bw:.0}% menos banda. \
                         Por isso jogos rendem mais presos a um socket com a memória local."
                    ));
                }
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.small_button(crate::i18n::tr("Full diagnosis (gfx906-rocm doctor)")).clicked() {
                app.start_job(crate::i18n::tr("Diagnosis"), gfx(&["doctor"]));
            }
        });
    });
}

/// Two-series chart (last minute) on the same scale: a = filled, b = line.
fn dual_graph(
    ui: &mut egui::Ui,
    a: &std::collections::VecDeque<f32>,
    b: &std::collections::VecDeque<f32>,
    ca: Color32,
    cb: Color32,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(260.0, 46.0), Sense::hover());
    let pal = crate::theme::pal(ui);
    ui.painter().rect_filled(rect, 4.0, pal.track);
    let max = a.iter().chain(b.iter()).cloned().fold(1024.0f32, f32::max);
    let n = (crate::theme::Spark::LEN - 1) as f32;
    let pts = |q: &std::collections::VecDeque<f32>| -> Vec<egui::Pos2> {
        let len = q.len();
        q.iter()
            .enumerate()
            .map(|(i, v)| {
                egui::pos2(
                    rect.right() - (len - 1 - i) as f32 * rect.width() / n,
                    rect.bottom() - (v / max) * (rect.height() - 2.0) - 1.0,
                )
            })
            .collect()
    };
    for (q, c, w) in [(a, ca, 2.0), (b, cb, 1.5)] {
        let p = pts(q);
        if p.len() >= 2 {
            ui.painter().add(egui::Shape::line(p, Stroke::new(w, c)));
        }
    }
}

/// Disks (read, write, % active) and network (download, upload), with the last minute.
fn disks_and_net(app: &App, ui: &mut egui::Ui) {
    use numapilot::iostat::fmt_rate;
    let read = Color32::from_rgb(0x5b, 0x8c, 0xff);
    let write = Color32::from_rgb(0xf0, 0xa6, 0x3a);
    let down = Color32::from_rgb(0x3e, 0xcf, 0x8e);
    let up = Color32::from_rgb(0xc0, 0x7c, 0xf0);
    ui.heading(crate::i18n::tr("Disks and network"));
    if app.disk_rates.is_empty() && app.net_rates.is_empty() {
        ui.label(RichText::new(crate::i18n::tr("Measuring...")).weak());
        return;
    }
    egui::Grid::new("io")
        .num_columns(3)
        .spacing([18.0, 8.0])
        .min_col_width(170.0)
        .striped(true)
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            let empty = std::collections::VecDeque::new();
            for d in &app.disk_rates {
                ui.vertical(|ui| {
                    ui.label(RichText::new(crate::tf!("Disk {}", "Disco {}", d.name)).strong());
                    if !d.model.is_empty() {
                        ui.label(RichText::new(&d.model).small().weak());
                    }
                });
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(crate::tf!("read {}", "leitura {}", fmt_rate(d.read_bps)))
                            .color(read),
                    );
                    ui.label(
                        RichText::new(crate::tf!("write {}", "escrita {}", fmt_rate(d.write_bps)))
                            .color(write),
                    );
                    ui.label(
                        RichText::new(crate::tf!(
                            "active {:.0}% of the time",
                            "ativo {:.0}% do tempo",
                            d.busy_pct
                        ))
                        .small()
                        .weak(),
                    );
                });
                let h = app.io_hist.get(&format!("disk:{}", d.name));
                dual_graph(
                    ui,
                    h.map_or(&empty, |h| &h.0),
                    h.map_or(&empty, |h| &h.1),
                    read,
                    write,
                );
                ui.end_row();
            }
            for n in &app.net_rates {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} {}",
                            if n.wifi {
                                "Wi-Fi"
                            } else {
                                crate::i18n::tr("Network")
                            },
                            n.name
                        ))
                        .strong(),
                    );
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("↓ {}", fmt_rate(n.rx_bps))).color(down));
                    ui.label(RichText::new(format!("↑ {}", fmt_rate(n.tx_bps))).color(up));
                });
                let h = app.io_hist.get(&format!("net:{}", n.name));
                dual_graph(
                    ui,
                    h.map_or(&empty, |h| &h.0),
                    h.map_or(&empty, |h| &h.1),
                    down,
                    up,
                );
                ui.end_row();
            }
        });
    ui.label(
        RichText::new(crate::i18n::tr(
            "Charts: last minute, on the same scale (solid line = read/download).",
        ))
        .small()
        .weak(),
    );
}
