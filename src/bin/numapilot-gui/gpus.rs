//! Aba "Placas de vídeo": temperaturas, ventoinha (automática / curva / fixa) e potência.

use crate::App;
use eframe::egui::{self, Color32, RichText, Stroke};
use numapilot::ctl::Request;
use numapilot::gpuctl::{self, default_curve, FanMode, GpuControl, Sensors};

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

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    ui.label(
        RichText::new(
            "Temperatura, ventoinha e potência de cada placa. Quem aplica é o serviço numapilot; \
             se ele parar, as ventoinhas voltam para o controle da própria placa. Acima de 95 °C a ventoinha vai a 100%.",
        )
        .weak(),
    );
    ui.separator();
    let gpus = app.topo.gpus.clone();
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
                if !s.fan_controllable && s.power_cap_max_w.is_none() {
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
    if let Some((pci, control)) = app.pending_gpu.take() {
        app.send(Request::SetGpuControl { pci, control });
        app.gpu_saved = gpuctl::GpuControls::load();
        app.gpu_edit.clear();
    }
}
