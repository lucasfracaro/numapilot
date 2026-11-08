//! Aba "Mapa e medições": núcleos de cada socket (uso e quem está em cada um), memória,
//! alocações remotas, governador de frequência e a matriz de latência/banda medida.

use crate::App;
use crate::lab::gfx;
use eframe::egui::{self, Color32, RichText, Sense, Stroke, StrokeKind};
use numapilot::ctl::Request;
use numapilot::topo::Topology;
use std::collections::{BTreeMap, BTreeSet};

pub fn load_color(l: f32) -> Color32 {
    let l = l.clamp(0.0, 1.0);
    // verde-escuro (livre) → amarelo → vermelho (cheio)
    let (r, g, b) = if l < 0.5 {
        let t = l / 0.5;
        (40.0 + 180.0 * t, 70.0 + 110.0 * t, 50.0)
    } else {
        let t = (l - 0.5) / 0.5;
        (220.0 + 10.0 * t, 180.0 - 120.0 * t, 50.0)
    };
    Color32::from_rgb(r as u8, g as u8, b as u8)
}

/// Desenha os núcleos de cada socket. Se `selected` vier, clicar seleciona/desmarca CPUs.
/// `who`: CPU → programas restritos a ele (para a dica ao passar o mouse).
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
            RichText::new(format!(
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
                        // Clique: liga/desliga o núcleo inteiro (os dois threads).
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
                        // Botão direito: só o 1º thread (sem hyper-threading neste núcleo).
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

/// Um mini gráfico do último minuto por thread (agrupados por socket) e a temperatura do
/// processador.
fn thread_graphs(app: &App, ui: &mut egui::Ui, who: &BTreeMap<usize, Vec<String>>) {
    let p = crate::theme::pal(ui);
    let n_all = app.cpu_loads.len().max(1);
    let h = if n_all > 48 { 34.0 } else { 42.0 };
    let min_w = if n_all > 48 { 70.0 } else { 92.0 };
    let multi = app.topo.nodes.len() > 1;
    for n in &app.topo.nodes {
        if multi {
            ui.label(
                RichText::new(format!("Socket {} — {} threads", n.id, n.cpus.len()))
                    .size(12.5)
                    .color(p.weak),
            );
        }
        let mut cpus = n.cpus.clone();
        cpus.sort_unstable();
        // colunas iguais que enchem a largura; de preferência um divisor do nº de threads
        // (16 threads → 2 linhas de 8)
        let gap = 6.0;
        // desconta a barra de rolagem
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
                        tip.push_str(&format!(" · núcleo {k}"));
                    }
                    if let Some(mhz) = app.cpu_freq.get(&c) {
                        tip.push_str(&format!(" · {:.2} GHz", *mhz as f64 / 1000.0));
                    }
                    if let Some(ps) = who.get(&c) {
                        tip.push_str(&format!("\npreso aqui: {}", ps.join(", ")));
                    }
                    resp.on_hover_text(tip);
                }
            },
        );
        ui.add_space(4.0);
    }
    // temperatura do processador (um gráfico por socket)
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
                    format!("Temperatura socket {pkg} (último minuto, 30–100 °C)")
                } else {
                    "Temperatura do processador (último minuto, 30–100 °C)".into()
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
        RichText::new("Passe o mouse num gráfico para ver o núcleo e a frequência; ponto amarelo = algum programa está preso a ele.")
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
    // Quem está restrito a quais CPUs (programas com núcleos escolhidos).
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
            ui.selectable_value(&mut app.map_threads, true, "Gráfico por thread");
            ui.selectable_value(&mut app.map_threads, false, "Mapa de núcleos");
        });
        ui.add_space(4.0);
        if app.map_threads {
            thread_graphs(app, ui, &who);
        } else {
            ui.label(
                RichText::new(
                    "Cada quadrado é um núcleo físico (dividido nos 2 threads). Cor = uso; ponto branco = algum programa \
                     está preso a ele (passe o mouse para ver).",
                )
                .weak(),
            );
            core_map(ui, &app.topo, &app.cpu_loads, None, &who);
        }
        ui.separator();

        ui.heading("Memória e frequência por socket");
        let nodes = app.topo.nodes.clone();
        egui::Grid::new("sockets").num_columns(4).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
            ui.label(RichText::new("Socket").strong());
            ui.label(RichText::new("Memória em uso").strong());
            ui.label(RichText::new("Alocações no outro socket").strong())
                .on_hover_text("Páginas de memória que precisaram ser criadas no outro socket (numastat other_node/numa_miss), por segundo.");
            ui.label(RichText::new("Governador").strong())
                .on_hover_text("performance = frequência máxima sempre (jogos); schedutil/powersave = economiza energia.");
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
                let txt = format!("{:.0}/s ({:.1}% das alocações)", rate.0, rate.1 * 100.0);
                ui.label(if rate.1 > 0.05 { RichText::new(txt).color(Color32::from_rgb(230, 160, 40)) } else { RichText::new(txt) });
                let (cur, avail) = governors(n.cpus[0]);
                if avail.is_empty() {
                    ui.label(RichText::new("indisponível").weak());
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
            ui.label(RichText::new("Interrupções das GPUs: atendidas pelo socket de cada placa.").small().weak());
        } else {
            ui.label(
                RichText::new(
                    "O irqbalance está rodando e pode desfazer o ajuste das interrupções das GPUs: \
                     sudo systemctl disable --now irqbalance",
                )
                .small()
                .color(Color32::from_rgb(230, 160, 40)),
            );
        }
        ui.separator();

        ui.horizontal(|ui| {
            ui.heading("Latência e banda entre sockets (medidas)");
            if ui.button("Medir agora").on_hover_text("Leva ~10 s; o PC fica ocupado durante o teste").clicked() {
                let mut v = vec![crate::jobs::sibling("numapilot")];
                v.push("bench".into());
                app.start_job("Medindo latência e banda de memória", v);
                app.bench_checked = None;
            }
        });
        match &app.bench {
            None => {
                ui.label(RichText::new("Ainda não medido. Clique em \"Medir agora\".").weak());
            }
            Some(b) => {
                egui::Grid::new("bench").num_columns(b.nodes.len() + 1).spacing([18.0, 4.0]).striped(true).show(ui, |ui| {
                    ui.label(RichText::new("CPUs \\ memória").small());
                    for n in &b.nodes {
                        ui.label(RichText::new(format!("memória do socket {n}")).strong());
                    }
                    ui.end_row();
                    for (i, n) in b.nodes.iter().enumerate() {
                        ui.label(RichText::new(format!("CPUs do socket {n}")).strong());
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
                    ui.label(format!(
                        "Usar a memória do outro socket custa {extra:.0}% a mais de latência e {bw:.0}% menos banda. \
                         Por isso jogos rendem mais presos a um socket com a memória local."
                    ));
                }
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.small_button("Diagnóstico completo (gfx906-rocm doctor)").clicked() {
                app.start_job("Diagnóstico", gfx(&["doctor"]));
            }
        });
    });
}
