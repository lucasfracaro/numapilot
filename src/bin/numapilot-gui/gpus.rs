//! Aba "Placas de vídeo": temperaturas, ventoinha (automática / curva / fixa) e potência.

use crate::App;
use eframe::egui::{self, Color32, RichText, Stroke};
use numapilot::ctl::Request;
use numapilot::gpuctl::{self, default_curve, FanMode, GpuControl, Sensors};
use numapilot::history::History;

fn temp_color(t: f64) -> Color32 {
    if t >= 90.0 {
        Color32::from_rgb(230, 70, 60)
    } else if t >= 75.0 {
        Color32::from_rgb(230, 160, 40)
    } else {
        Color32::from_rgb(110, 190, 120)
    }
}

fn temps(ui: &mut egui::Ui, s: &Sensors) {
    for (label, v) in [("borda", s.edge_c), ("hotspot", s.junction_c), ("memória", s.mem_c)] {
        if let Some(t) = v {
            ui.label(label.to_string());
            ui.label(RichText::new(format!("{t:.0} °C")).strong().color(temp_color(t)));
        }
    }
}

fn draw_curve(ui: &mut egui::Ui, c: &GpuControl, temp: Option<f64>) {
    let (resp, p) = ui.allocate_painter(egui::vec2(280.0, 120.0), egui::Sense::hover());
    let r = resp.rect;
    p.rect_stroke(r, 4.0, Stroke::new(1.0, ui.visuals().weak_text_color()), egui::StrokeKind::Inside);
    let map = |t: f64, pct: f64| {
        egui::pos2(
            r.left() + ((t - 20.0) / 80.0).clamp(0.0, 1.0) as f32 * r.width(),
            r.bottom() - (pct / 100.0).clamp(0.0, 1.0) as f32 * r.height(),
        )
    };
    let mut pts = vec![map(20.0, c.curve.first().map(|x| x.1).unwrap_or(0.0))];
    pts.extend(c.curve.iter().map(|&(t, pct)| map(t, pct)));
    pts.push(map(100.0, c.curve.last().map(|x| x.1).unwrap_or(100.0)));
    let col = ui.visuals().selection.bg_fill;
    p.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
    for &(t, pct) in &c.curve {
        p.circle_filled(map(t, pct), 3.5, col);
    }
    if let Some(t) = temp {
        let x = map(t, 0.0).x;
        p.line_segment([egui::pos2(x, r.top()), egui::pos2(x, r.bottom())], Stroke::new(1.0, temp_color(t)));
        let pct = c.target_pct(Some(t)).unwrap_or(0.0);
        p.circle_filled(map(t, pct), 4.5, temp_color(t));
    }
    let small = egui::FontId::proportional(10.0);
    let weak = ui.visuals().weak_text_color();
    p.text(r.left_bottom() + egui::vec2(3.0, -2.0), egui::Align2::LEFT_BOTTOM, "20°C", small.clone(), weak);
    p.text(r.right_bottom() + egui::vec2(-3.0, -2.0), egui::Align2::RIGHT_BOTTOM, "100°C", small.clone(), weak);
    p.text(r.left_top() + egui::vec2(3.0, 2.0), egui::Align2::LEFT_TOP, "100%", small, weak);
}

/// Gráfico do histórico: temperatura (0–100 °C) e potência (0–máx W) de uma GPU.
fn history_plot(ui: &mut egui::Ui, h: &History, idx: usize, secs: u64, max_w: f64) {
    let samples = h.since(secs);
    let (resp, p) = ui.allocate_painter(egui::vec2(ui.available_width().min(620.0), 124.0), egui::Sense::hover());
    let full = resp.rect;
    let r = egui::Rect::from_min_max(full.min, egui::pos2(full.right(), full.bottom() - 14.0));
    p.rect_stroke(r, 4.0, Stroke::new(1.0, ui.visuals().weak_text_color()), egui::StrokeKind::Inside);
    let small = egui::FontId::proportional(10.0);
    let weak = ui.visuals().weak_text_color();
    if samples.len() < 2 {
        p.text(r.center(), egui::Align2::CENTER_CENTER, "sem histórico ainda (o serviço grava a cada 30 s)", small, weak);
        return;
    }
    let t1 = samples.last().map(|s| s.t).unwrap_or(0) as f64;
    let t0 = t1 - secs as f64;
    let x = |t: u64| r.left() + (((t as f64 - t0) / secs as f64).clamp(0.0, 1.0) as f32) * r.width();
    let y = |v: f64, max: f64| r.bottom() - ((v / max).clamp(0.0, 1.0) as f32) * r.height();
    let temp: Vec<egui::Pos2> = samples.iter().filter_map(|s| s.g.get(idx).map(|g| egui::pos2(x(s.t), y(g[0] as f64, 100.0)))).collect();
    let pow: Vec<egui::Pos2> = samples.iter().filter_map(|s| s.g.get(idx).map(|g| egui::pos2(x(s.t), y(g[1] as f64, max_w)))).collect();
    let y90 = y(90.0, 100.0);
    p.line_segment([egui::pos2(r.left(), y90), egui::pos2(r.right(), y90)], Stroke::new(1.0, Color32::from_rgb(190, 70, 60)));
    p.add(egui::Shape::line(pow, Stroke::new(1.5, Color32::from_rgb(90, 140, 220))));
    p.add(egui::Shape::line(temp, Stroke::new(1.5, Color32::from_rgb(230, 120, 60))));
    let maxt = samples.iter().filter_map(|s| s.g.get(idx).map(|g| g[0])).fold(0.0f32, f32::max);
    let maxp = samples.iter().filter_map(|s| s.g.get(idx).map(|g| g[1])).fold(0.0f32, f32::max);
    p.text(full.left_bottom() + egui::vec2(2.0, 0.0), egui::Align2::LEFT_BOTTOM, format!("temperatura (máx {maxt:.0} °C)"), small.clone(), Color32::from_rgb(230, 120, 60));
    p.text(full.center_bottom(), egui::Align2::CENTER_BOTTOM, format!("linha vermelha = 90 °C · últimas {} h", secs / 3600), small.clone(), weak);
    p.text(full.right_bottom() + egui::vec2(-2.0, 0.0), egui::Align2::RIGHT_BOTTOM, format!("potência (máx {maxp:.0} W)"), small, Color32::from_rgb(90, 140, 220));
}

/// OverDrive (clock e voltagem) e Resizable BAR.
fn od_ui(ui: &mut egui::Ui, pci: &str, edit: &mut GpuControl, boot_job: &mut Option<bool>) {
    match gpuctl::rebar_active(pci) {
        Some(true) => {
            ui.label(RichText::new("Resizable BAR: ativo").color(Color32::from_rgb(110, 190, 120)));
        }
        Some(false) => {
            ui.label(RichText::new("Resizable BAR: inativo (veja o diagnóstico em Configurações)").weak());
        }
        None => {}
    }
    let Some(od) = gpuctl::read_od(pci) else {
        let st = numapilot::bootcfg::overdrive_state();
        ui.horizontal_wrapped(|ui| {
            if st.on_next_boot {
                ui.label(RichText::new("Clock/voltagem: o OverDrive liga quando você reiniciar o computador.").color(Color32::from_rgb(230, 160, 40)));
            } else {
                ui.label(RichText::new("Clock/voltagem (undervolt/overclock): o OverDrive do driver está desligado.").weak());
                if ui
                    .button("Ativar OverDrive")
                    .on_hover_text(
                        "Grava a opção do driver amdgpu em /etc/modprobe.d e refaz a imagem de boot. \
                         Vale para qualquer kernel, inclusive depois de atualizar. Pede a senha; depois é só reiniciar.",
                    )
                    .clicked()
                {
                    *boot_job = Some(true);
                }
            }
        });
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Clock e voltagem:").strong());
        let mut on = edit.od.is_some();
        if ui.checkbox(&mut on, "personalizar").changed() {
            edit.od = on.then(gpuctl::OdSettings::default);
        }
        ui.label(
            RichText::new(format!(
                "atual: GPU até {} MHz, memória {} MHz, curva {}",
                od.sclk.iter().map(|x| x.1).max().unwrap_or(0),
                od.mclk.iter().map(|x| x.1).max().unwrap_or(0),
                od.curve.iter().map(|c| format!("{} MHz/{} mV", c.1, c.2)).collect::<Vec<_>>().join(", ")
            ))
            .small()
            .weak(),
        );
    });
    let Some(s) = edit.od.as_mut() else { return };
    let opt = |ui: &mut egui::Ui, v: &mut Option<u32>, cur: u32, range: Option<(u32, u32)>, label: &str| {
        let mut on = v.is_some();
        ui.checkbox(&mut on, label);
        let mut x = v.unwrap_or(cur);
        let (lo, hi) = range.unwrap_or((cur / 2, cur * 2));
        ui.add_enabled(on, egui::DragValue::new(&mut x).range(lo..=hi).suffix(" MHz"));
        *v = on.then_some(x);
    };
    ui.horizontal(|ui| {
        opt(ui, &mut s.sclk_max, od.sclk.iter().map(|x| x.1).max().unwrap_or(1801), od.range_sclk, "GPU máx.");
        ui.add_space(10.0);
        opt(ui, &mut s.mclk_max, od.mclk.iter().map(|x| x.1).max().unwrap_or(1000), od.range_mclk, "memória máx.");
    });
    ui.horizontal(|ui| {
        let mut custom = !s.curve.is_empty();
        if ui.checkbox(&mut custom, "curva de voltagem").changed() {
            s.curve = if custom { od.curve.iter().map(|c| (c.1, c.2)).collect() } else { vec![] };
        }
        for mv in [25u32, 50] {
            if ui.small_button(format!("reduzir {mv} mV")).on_hover_text("Undervolt: mesma frequência com menos voltagem (menos calor e consumo). Teste a estabilidade.").clicked() {
                s.curve = od
                    .curve
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        let lo = od.range_curve_volt.get(i).map(|r| r.0).unwrap_or(700);
                        (c.1, c.2.saturating_sub(mv).max(lo))
                    })
                    .collect();
            }
        }
    });
    if !s.curve.is_empty() {
        egui::Grid::new(format!("od-{pci}")).num_columns(3).show(ui, |ui| {
            for (i, pt) in s.curve.iter_mut().enumerate() {
                ui.label(format!("ponto {}", i + 1));
                let rs = od.range_curve_sclk.get(i).copied().unwrap_or((500, 2500));
                let rv = od.range_curve_volt.get(i).copied().unwrap_or((700, 1250));
                ui.add(egui::DragValue::new(&mut pt.0).range(rs.0..=rs.1).suffix(" MHz"));
                ui.add(egui::DragValue::new(&mut pt.1).range(rv.0..=rv.1).suffix(" mV"));
                ui.end_row();
            }
        });
    }
    ui.label(
        RichText::new("Se a placa travar ou o jogo fechar sozinho, volte um passo. Ao reiniciar o PC o serviço reaplica; desmarcar \"personalizar\" volta ao padrão.")
            .small()
            .color(Color32::from_rgb(230, 160, 40)),
    );
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    if app.hist_at.is_none_or(|t| t.elapsed().as_secs() >= 30) {
        app.hist = History::load();
        app.hist_at = Some(std::time::Instant::now());
    }
    ui.label(
        RichText::new(
            "Temperatura, ventoinha e potência de cada placa. Quem aplica é o serviço numapilot; \
             se ele parar, as ventoinhas voltam para o controle da própria placa. Acima de 95 °C a ventoinha vai a 100%.",
        )
        .weak(),
    );
    ui.horizontal(|ui| {
        ui.label("Histórico:");
        for (s, l) in [(3600u64, "1 h"), (6 * 3600, "6 h"), (24 * 3600, "24 h")] {
            ui.selectable_value(&mut app.hist_secs, s, l);
        }
    });
    ui.separator();
    let gpus = app.topo.gpus.clone();
    let hist = app.hist.clone();
    let mut boot_job: Option<bool> = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for g in &gpus {
            let s = gpuctl::read_sensors(&g.pci);
            let saved = app.gpu_saved.get(&g.pci);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.heading(&g.alias);
                    ui.label(
                        RichText::new(format!(
                            "{} · socket {} · {}",
                            g.name,
                            g.numa_node.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                            g.pci
                        ))
                        .weak(),
                    );
                });
                ui.horizontal_wrapped(|ui| {
                    temps(ui, &s);
                    ui.separator();
                    if let Some(rpm) = s.fan_rpm {
                        ui.label(format!("ventoinha {rpm} RPM"));
                    }
                    if let Some(p) = s.fan_pct {
                        ui.label(format!("({p:.0}%)"));
                    }
                    ui.separator();
                    if let Some(w) = s.power_w {
                        ui.label(format!("{w:.0} W"));
                    }
                    if let Some(c) = s.power_cap_w {
                        ui.label(RichText::new(format!("limite {c:.0} W")).weak());
                    }
                    ui.separator();
                    ui.label(format!("uso {}%", g.busy_percent().unwrap_or(0)));
                    if let Some((u, t)) = g.vram_used_total() {
                        ui.label(format!("VRAM {:.1}/{:.0} GB", u as f64 / 1073741824.0, t as f64 / 1073741824.0));
                    }
                });
                if let Some(h) = &hist
                    && let Some(idx) = h.gpus.iter().position(|a| *a == g.alias)
                {
                    history_plot(ui, h, idx, app.hist_secs, s.power_cap_max_w.unwrap_or(300.0));
                }
                if !s.fan_controllable && s.power_cap_max_w.is_none() && gpuctl::read_od(&g.pci).is_none() {
                    ui.label(RichText::new("O driver não expõe controles de ventoinha/potência para esta placa.").weak());
                    return;
                }
                let edit = app.gpu_edit.entry(g.pci.clone()).or_insert_with(|| saved.clone());
                ui.add_space(4.0);
                if s.fan_controllable {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Ventoinha:").strong());
                        ui.radio_value(&mut edit.fan, FanMode::Auto, "da placa (automática)");
                        ui.radio_value(&mut edit.fan, FanMode::Curve, "curva");
                        ui.radio_value(&mut edit.fan, FanMode::Fixed, "fixa");
                    });
                    match edit.fan {
                        FanMode::Fixed => {
                            ui.add(egui::Slider::new(&mut edit.fixed_pct, 0.0..=100.0).suffix("%").text("velocidade"));
                        }
                        FanMode::Curve => {
                            ui.horizontal(|ui| {
                                draw_curve(ui, edit, s.control_temp());
                                ui.vertical(|ui| {
                                    egui::Grid::new(format!("curve-{}", g.pci)).num_columns(3).show(ui, |ui| {
                                        let mut remove = None;
                                        let n = edit.curve.len();
                                        for (i, pt) in edit.curve.iter_mut().enumerate() {
                                            ui.add(egui::DragValue::new(&mut pt.0).range(20.0..=100.0).suffix(" °C"));
                                            ui.add(egui::DragValue::new(&mut pt.1).range(0.0..=100.0).suffix(" %"));
                                            if n > 2 && ui.small_button("x").clicked() {
                                                remove = Some(i);
                                            }
                                            ui.end_row();
                                        }
                                        if let Some(i) = remove {
                                            edit.curve.remove(i);
                                        }
                                    });
                                    ui.horizontal(|ui| {
                                        if ui.small_button("+ ponto").clicked() {
                                            let last = edit.curve.last().copied().unwrap_or((70.0, 60.0));
                                            edit.curve.push(((last.0 + 5.0).min(100.0), last.1));
                                        }
                                        if ui.small_button("curva padrão").clicked() {
                                            edit.curve = default_curve();
                                        }
                                    });
                                });
                            });
                        }
                        FanMode::Auto => {}
                    }
                }
                if let (Some(lo), Some(hi)) = (s.power_cap_min_w, s.power_cap_max_w) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Potência:").strong());
                        let mut on = edit.power_cap_w.is_some();
                        if ui.checkbox(&mut on, "limitar").changed() {
                            edit.power_cap_w = if on { Some(s.power_cap_w.unwrap_or(hi).round()) } else { None };
                        }
                        if let Some(w) = edit.power_cap_w.as_mut() {
                            ui.add(egui::Slider::new(w, lo.round()..=hi.round()).suffix(" W").step_by(5.0));
                        } else {
                            ui.label(RichText::new("padrão da placa").weak());
                        }
                    });
                }
                ui.add_space(4.0);
                od_ui(ui, &g.pci, edit, &mut boot_job);
                let changed = *edit != saved;
                let edit_c = edit.clone();
                ui.horizontal(|ui| {
                    if ui.add_enabled(changed, egui::Button::new("Aplicar")).clicked() {
                        app.pending_gpu = Some((g.pci.clone(), edit_c.clone()));
                    }
                    if ui.add_enabled(changed, egui::Button::new("Desfazer")).clicked() {
                        app.gpu_edit.insert(g.pci.clone(), saved.clone());
                    }
                });
            });
            ui.add_space(6.0);
        }
    });
    if let Some(on) = boot_job {
        app.start_job(
            if on { "Ativando o OverDrive" } else { "Desativando o OverDrive" },
            vec![
                "pkexec".into(),
                crate::jobs::sibling_abs("numapilot"),
                "boot".into(),
                "overdrive".into(),
                if on { "on" } else { "off" }.into(),
            ],
        );
    }
    if let Some((pci, control)) = app.pending_gpu.take() {
        app.send(Request::SetGpuControl { pci, control });
        app.gpu_saved = gpuctl::GpuControls::load();
        app.gpu_edit.clear();
    }
}
