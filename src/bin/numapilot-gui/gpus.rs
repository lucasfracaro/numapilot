//! Aba "Placas de vídeo": temperaturas, ventoinha (automática / curva / fixa) e potência.

use crate::App;
use eframe::egui::{self, Color32, RichText, Stroke};
use numapilot::ctl::Request;
use numapilot::gpuctl::{self, FanMode, GpuControl, Sensors, default_curve};
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
    for (label, v) in [
        ("borda", s.edge_c),
        ("hotspot", s.junction_c),
        ("memória", s.mem_c),
    ] {
        if let Some(t) = v {
            ui.label(label.to_string());
            ui.label(
                RichText::new(format!("{t:.0} °C"))
                    .strong()
                    .color(temp_color(t)),
            );
        }
    }
}

fn draw_curve(ui: &mut egui::Ui, c: &GpuControl, temp: Option<f64>) {
    let (resp, p) = ui.allocate_painter(egui::vec2(280.0, 120.0), egui::Sense::hover());
    let r = resp.rect;
    p.rect_stroke(
        r,
        4.0,
        Stroke::new(1.0, ui.visuals().weak_text_color()),
        egui::StrokeKind::Inside,
    );
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
        p.line_segment(
            [egui::pos2(x, r.top()), egui::pos2(x, r.bottom())],
            Stroke::new(1.0, temp_color(t)),
        );
        let pct = c.target_pct(Some(t)).unwrap_or(0.0);
        p.circle_filled(map(t, pct), 4.5, temp_color(t));
    }
    let small = egui::FontId::proportional(10.0);
    let weak = ui.visuals().weak_text_color();
    p.text(
        r.left_bottom() + egui::vec2(3.0, -2.0),
        egui::Align2::LEFT_BOTTOM,
        "20°C",
        small.clone(),
        weak,
    );
    p.text(
        r.right_bottom() + egui::vec2(-3.0, -2.0),
        egui::Align2::RIGHT_BOTTOM,
        "100°C",
        small.clone(),
        weak,
    );
    p.text(
        r.left_top() + egui::vec2(3.0, 2.0),
        egui::Align2::LEFT_TOP,
        "100%",
        small,
        weak,
    );
}

/// Gráfico do histórico: temperatura (0–100 °C) e potência (0–máx W) de uma GPU.
fn history_plot(ui: &mut egui::Ui, h: &History, idx: usize, secs: u64, max_w: f64) {
    let samples = h.since(secs);
    let (resp, p) = ui.allocate_painter(
        egui::vec2(ui.available_width().min(620.0), 124.0),
        egui::Sense::hover(),
    );
    let full = resp.rect;
    let r = egui::Rect::from_min_max(full.min, egui::pos2(full.right(), full.bottom() - 14.0));
    p.rect_stroke(
        r,
        4.0,
        Stroke::new(1.0, ui.visuals().weak_text_color()),
        egui::StrokeKind::Inside,
    );
    let small = egui::FontId::proportional(10.0);
    let weak = ui.visuals().weak_text_color();
    if samples.len() < 2 {
        p.text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            "sem histórico ainda (o serviço grava a cada 30 s)",
            small,
            weak,
        );
        return;
    }
    let t1 = samples.last().map(|s| s.t).unwrap_or(0) as f64;
    let t0 = t1 - secs as f64;
    let x =
        |t: u64| r.left() + (((t as f64 - t0) / secs as f64).clamp(0.0, 1.0) as f32) * r.width();
    let y = |v: f64, max: f64| r.bottom() - ((v / max).clamp(0.0, 1.0) as f32) * r.height();
    let temp: Vec<egui::Pos2> = samples
        .iter()
        .filter_map(|s| {
            s.g.get(idx)
                .map(|g| egui::pos2(x(s.t), y(g[0] as f64, 100.0)))
        })
        .collect();
    let pow: Vec<egui::Pos2> = samples
        .iter()
        .filter_map(|s| {
            s.g.get(idx)
                .map(|g| egui::pos2(x(s.t), y(g[1] as f64, max_w)))
        })
        .collect();
    let y90 = y(90.0, 100.0);
    p.line_segment(
        [egui::pos2(r.left(), y90), egui::pos2(r.right(), y90)],
        Stroke::new(1.0, Color32::from_rgb(190, 70, 60)),
    );
    p.add(egui::Shape::line(
        pow,
        Stroke::new(1.5, Color32::from_rgb(90, 140, 220)),
    ));
    p.add(egui::Shape::line(
        temp,
        Stroke::new(1.5, Color32::from_rgb(230, 120, 60)),
    ));
    let maxt = samples
        .iter()
        .filter_map(|s| s.g.get(idx).map(|g| g[0]))
        .fold(0.0f32, f32::max);
    let maxp = samples
        .iter()
        .filter_map(|s| s.g.get(idx).map(|g| g[1]))
        .fold(0.0f32, f32::max);
    p.text(
        full.left_bottom() + egui::vec2(2.0, 0.0),
        egui::Align2::LEFT_BOTTOM,
        format!("temperatura (máx {maxt:.0} °C)"),
        small.clone(),
        Color32::from_rgb(230, 120, 60),
    );
    p.text(
        full.center_bottom(),
        egui::Align2::CENTER_BOTTOM,
        format!("linha vermelha = 90 °C · últimas {} h", secs / 3600),
        small.clone(),
        weak,
    );
    p.text(
        full.right_bottom() + egui::vec2(-2.0, 0.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("potência (máx {maxp:.0} W)"),
        small,
        Color32::from_rgb(90, 140, 220),
    );
}

/// Resizable BAR: situação e o que fazer para ligar (com ou sem a opção na BIOS).
fn rebar_ui(ui: &mut egui::Ui, pci: &str) {
    let Some(b) = gpuctl::bar_status(pci) else {
        return;
    };
    let gb = |x: u64| {
        if x >= 1 << 30 {
            format!("{:.0} GB", x as f64 / (1u64 << 30) as f64)
        } else {
            format!("{} MB", x >> 20)
        }
    };
    if b.full() {
        ui.label(
            RichText::new(format!(
                "Resizable BAR: ativo ({} visíveis pela CPU)",
                gb(b.bar_bytes)
            ))
            .color(Color32::from_rgb(110, 190, 120)),
        );
        return;
    }
    let nv = numapilot::nvctl::is_nvidia(pci);
    let mut txt = format!(
        "Resizable BAR: inativo (a CPU vê {} da VRAM",
        gb(b.bar_bytes)
    );
    if let Some(v) = b.vram_bytes {
        txt.push_str(&format!(" de {}", gb(v)));
    }
    txt.push(')');
    ui.label(RichText::new(txt).color(Color32::from_rgb(230, 160, 40)));
    let advice = if !b.above_4g {
        "Primeiro passo, na BIOS: ligar \"Above 4G Decoding\" (às vezes chamado de \"64-bit PCI\" ou \"MMIO above 4G\"). Sem isso não dá."
    } else if nv {
        "Na NVIDIA o driver só usa o BAR grande se a BIOS ligar o Resizable BAR (e a placa for RTX 30 ou mais nova)."
    } else {
        "Placas AMD: o driver já tenta aumentar sozinho no boot, mesmo com a opção desligada na BIOS. Se não conseguiu, faltou espaço \
         na janela da ponte PCI: o parâmetro pci=realloc (Máquina → Parâmetros do kernel) pede ao Linux para reorganizar."
    };
    ui.label(RichText::new(advice).small().weak());
    if let Some(m) = b.max_bytes {
        ui.label(
            RichText::new(format!("A placa aceita até {}.", gb(m)))
                .small()
                .weak(),
        );
    }
}

/// OverDrive (clock e voltagem) e Resizable BAR.
fn od_ui(ui: &mut egui::Ui, pci: &str, edit: &mut GpuControl, boot_job: &mut Option<bool>) {
    rebar_ui(ui, pci);
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
                od.curve
                    .iter()
                    .map(|c| format!("{} MHz/{} mV", c.1, c.2))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .small()
            .weak(),
        );
    });
    let Some(s) = edit.od.as_mut() else { return };
    let opt = |ui: &mut egui::Ui,
               v: &mut Option<u32>,
               cur: u32,
               range: Option<(u32, u32)>,
               label: &str| {
        let mut on = v.is_some();
        ui.checkbox(&mut on, label);
        let mut x = v.unwrap_or(cur);
        let (lo, hi) = range.unwrap_or((cur / 2, cur * 2));
        ui.add_enabled(
            on,
            egui::DragValue::new(&mut x).range(lo..=hi).suffix(" MHz"),
        );
        *v = on.then_some(x);
    };
    ui.horizontal(|ui| {
        opt(
            ui,
            &mut s.sclk_max,
            od.sclk.iter().map(|x| x.1).max().unwrap_or(1801),
            od.range_sclk,
            "GPU máx.",
        );
        ui.add_space(10.0);
        opt(
            ui,
            &mut s.mclk_max,
            od.mclk.iter().map(|x| x.1).max().unwrap_or(1000),
            od.range_mclk,
            "memória máx.",
        );
    });
    if let Some(cur) = od.vddgfx_offset {
        // RDNA2/3/4: undervolt por deslocamento
        ui.horizontal(|ui| {
            let mut on = s.voltage_offset_mv.is_some();
            ui.checkbox(&mut on, "deslocar a voltagem");
            let (lo, hi) = od.range_vo.unwrap_or((-200, 0));
            let mut v = s.voltage_offset_mv.unwrap_or(cur);
            ui.add_enabled(
                on,
                egui::Slider::new(&mut v, lo..=hi)
                    .suffix(" mV")
                    .step_by(5.0),
            );
            s.voltage_offset_mv = on.then_some(v);
            ui.label(RichText::new("negativo = undervolt").small().weak());
        });
    }
    if !od.curve.is_empty() {
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
            egui::Grid::new(format!("od-{pci}"))
                .num_columns(3)
                .show(ui, |ui| {
                    for (i, pt) in s.curve.iter_mut().enumerate() {
                        ui.label(format!("ponto {}", i + 1));
                        let rs = od.range_curve_sclk.get(i).copied().unwrap_or((500, 2500));
                        let rv = od.range_curve_volt.get(i).copied().unwrap_or((700, 1250));
                        ui.add(
                            egui::DragValue::new(&mut pt.0)
                                .range(rs.0..=rs.1)
                                .suffix(" MHz"),
                        );
                        ui.add(
                            egui::DragValue::new(&mut pt.1)
                                .range(rv.0..=rv.1)
                                .suffix(" mV"),
                        );
                        ui.end_row();
                    }
                });
        }
    }
    ui.label(
        RichText::new("Se a placa travar ou o jogo fechar sozinho, volte um passo. Ao reiniciar o PC o serviço reaplica; desmarcar \"personalizar\" volta ao padrão.")
            .small()
            .color(Color32::from_rgb(230, 160, 40)),
    );
}

/// NVIDIA: travar o clock máximo (o driver não deixa mexer na voltagem pelo Linux).
fn nv_clock_ui(ui: &mut egui::Ui, pci: &str, edit: &mut GpuControl) {
    rebar_ui(ui, pci);
    let Some(g) = numapilot::nvctl::get(pci) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new("Clock máximo:").strong());
        let mut on = edit.clock_max_mhz.is_some();
        let hi = g.clock_max_mhz.unwrap_or(2500.0) as u32;
        if ui.checkbox(&mut on, "travar").changed() {
            edit.clock_max_mhz = on.then_some((hi as f64 * 0.85) as u32);
        }
        if let Some(m) = edit.clock_max_mhz.as_mut() {
            ui.add(egui::Slider::new(m, 300..=hi).suffix(" MHz").step_by(15.0));
        } else {
            ui.label(RichText::new(format!("normal (até {hi} MHz)")).weak());
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Undervolt:").strong())
            .on_hover_text("Desloca a curva clock/voltagem para cima: com o clock máximo travado, a placa chega nele com voltagem menor. Suba de 15 em 15 MHz e teste em jogo; se travar ou der artefato, volte um passo.");
        let mut on = edit.nv_gpc_offset_mhz.is_some();
        if ui.checkbox(&mut on, "deslocar a curva").changed() {
            edit.nv_gpc_offset_mhz = on.then_some(75);
        }
        if let Some(v) = edit.nv_gpc_offset_mhz.as_mut() {
            ui.add(egui::Slider::new(v, -200..=300).suffix(" MHz").step_by(15.0));
        }
    });
    ui.horizontal(|ui| {
        ui.label("Memória:");
        let mut on = edit.nv_mem_offset_mhz.is_some();
        if ui.checkbox(&mut on, "deslocar").changed() {
            edit.nv_mem_offset_mhz = on.then_some(0);
        }
        if let Some(v) = edit.nv_mem_offset_mhz.as_mut() {
            ui.add(egui::Slider::new(v, -500..=1500).suffix(" MHz").step_by(50.0));
        }
    });
    if edit.nv_gpc_offset_mhz.is_some_and(|v| v > 0) && edit.clock_max_mhz.is_none() {
        ui.label(
            RichText::new("Sem travar o clock máximo, o deslocamento vira overclock (mais clock, mesma voltagem). Para undervolt, trave o clock também.")
                .small()
                .color(Color32::from_rgb(230, 160, 40)),
        );
    }
    ui.label(
        RichText::new(
            "Undervolt na NVIDIA: trave o clock (ex.: 1800 MHz) e desloque a curva (+100 a +200 MHz): mesmo FPS gastando e esquentando menos. O botão \"Achar sozinho\" testa os passos por você.",
        )
        .small()
        .weak(),
    );
}

fn nv_driver_panel(app: &mut App, ui: &mut egui::Ui) {
    if !app.topo.gpus.iter().any(|g| g.is_nvidia()) {
        return;
    }
    if app
        .nv_status
        .as_ref()
        .is_none_or(|(t, _)| t.elapsed().as_secs() >= 60)
    {
        app.nv_status = Some((
            std::time::Instant::now(),
            numapilot::driver::status(&app.topo),
        ));
    }
    let Some((_, st)) = app.nv_status.clone() else {
        return;
    };
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("Driver NVIDIA").strong());
            if let Some(b) = &st.branch {
                ui.label(RichText::new(format!("indicado para a sua placa: {}", b.label())).weak());
            }
        });
        ui.label(format!(
            "Carregado: {}   Instalado: {}   No repositório: {}",
            st.loaded.clone().unwrap_or("nenhum".into()),
            st.installed.clone().unwrap_or("não".into()),
            st.available.clone().unwrap_or("?".into())
        ));
        if st.nouveau {
            ui.label(RichText::new("Usando o nouveau (driver aberto da comunidade): sem desempenho para jogos.").color(Color32::from_rgb(230, 160, 40)));
        }
        if st.needs_reboot {
            ui.label(RichText::new("O driver instalado é mais novo que o carregado: reinicie.").color(Color32::from_rgb(230, 160, 40)));
        }
        let newer = matches!((&st.installed, &st.available), (Some(i), Some(a)) if i != a);
        ui.horizontal(|ui| {
            let label = if st.installed.is_some() { "Reinstalar driver oficial" } else { "Instalar driver oficial" };
            if ui
                .button(RichText::new(label).strong())
                .on_hover_text("Atualiza o sistema e instala o driver certo para a geração da placa (pelo chwd do CachyOS). Pede a senha; depois reinicie.")
                .clicked()
            {
                app.start_job("Driver NVIDIA", vec!["pkexec".into(), crate::jobs::sibling_abs("numapilot"), "driver".into(), "install".into()]);
                app.nv_status = None;
            }
            if st.installed.is_some()
                && ui
                    .add_enabled(newer, egui::Button::new("Atualizar"))
                    .on_hover_text("No CachyOS o driver atualiza junto com o sistema (pacman -Syu). Pede a senha.")
                    .clicked()
            {
                app.start_job("Atualizando sistema e driver", vec!["pkexec".into(), crate::jobs::sibling_abs("numapilot"), "driver".into(), "update".into()]);
                app.nv_status = None;
            }
            if !newer && st.installed.is_some() {
                ui.label(RichText::new("em dia").color(Color32::from_rgb(90, 180, 110)));
            }
        });
    });
    ui.add_space(4.0);
}

/// Arquitetura (o que serve para computação e upscaling) e ajustes da comunidade da placa.
fn board_ui(app: &mut App, ui: &mut egui::Ui, g: &numapilot::topo::Gpu) {
    let rec = numapilot::recipes::load();
    if let Some(a) = rec.arch_for(g) {
        let up: Vec<&str> = a
            .upscale
            .iter()
            .map(|u| match u.as_str() {
                "fsr4" => "FSR 4",
                "fsr4-rdna3" => "FSR 4 (via vkd3d-proton)",
                "fsr4-int8-experimental" => "FSR 4 INT8 (experimental)",
                "fsr3" => "FSR 3.1 (OptiScaler)",
                "xess" => "XeSS",
                "dlss" => "DLSS",
                "fsr1-gamescope" => "FSR 1 (gamescope)",
                "lsfg" => "geração de quadros (lsfg-vk)",
                o => o,
            })
            .collect();
        let compute = match a.compute.as_str() {
            "rocm" => "ROCm oficial",
            "rocm-comunidade" => "ROCm da comunidade",
            "vulkan" => "Vulkan (llama.cpp)",
            "cuda" => "CUDA",
            o => o,
        };
        ui.label(
            RichText::new(format!(
                "{} · IA: {compute} · upscaling: {}",
                a.family,
                up.join(", ")
            ))
            .small()
            .weak(),
        );
    }
    let Some(b) = rec.board_for(g).cloned() else {
        return;
    };
    if b.notes.is_empty() && b.governor_services.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(RichText::new(format!("{}: ajustes da comunidade", b.name)).strong())
        .id_salt(format!("board-{}", g.pci))
        .default_open(!b.governor_services.is_empty())
        .show(ui, |ui| {
            for n in &b.notes {
                ui.label(RichText::new(format!("• {n}")).small());
            }
            if !b.governor_services.is_empty() {
                let sv = numapilot::boards::services(&b);
                match sv.iter().find(|s| s.active) {
                    Some(s) => {
                        ui.label(RichText::new(format!("Governor de clock: {} rodando", s.name)).color(Color32::from_rgb(110, 190, 120)));
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Nenhum governor rodando: a GPU fica com clock fixo.").color(Color32::from_rgb(230, 160, 40)));
                            if let Some(s) = sv.iter().find(|s| s.installed) {
                                if ui.button(format!("Ligar {}", s.name)).clicked() {
                                    app.start_job(
                                        "Governor",
                                        vec!["pkexec".into(), "systemctl".into(), "enable".into(), "--now".into(), format!("{}.service", s.name)],
                                    );
                                }
                            } else if let Some(pkg) = b.governor_aur.first()
                                && ui.button("Instalar o governor (AUR)").on_hover_text("Abre um terminal com o paru/yay").clicked()
                            {
                                let helper = if numapilot::containers::which("paru") { "paru" } else { "yay" };
                                if let Err(e) = crate::jobs::in_terminal(&[helper.into(), "-S".into(), pkg.clone()]) {
                                    app.toast(e, false);
                                }
                            }
                        });
                    }
                }
                if let Some(path) = numapilot::boards::governor_config(&b) {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    if app.board_edit.as_ref().is_none_or(|(p, t, _)| *p != path || *t != text) {
                        app.board_edit = numapilot::boards::fields(&text).ok().map(|f| (path.clone(), text.clone(), f));
                    }
                    if let Some((_, orig, fields)) = app.board_edit.as_mut() {
                        ui.label(RichText::new(format!("Configuração: {path}")).small().weak());
                        let before = numapilot::boards::fields(orig).unwrap_or_default();
                        egui::Grid::new(format!("gov-{}", g.pci)).num_columns(2).show(ui, |ui| {
                            for f in fields.iter_mut() {
                                match f {
                                    numapilot::boards::Field::Num { key, value, int } => {
                                        ui.label(key.as_str());
                                        let unit = if key.contains("freq") { " MHz" } else if key.contains("volt") { " mV" } else { "" };
                                        let mut d = egui::DragValue::new(value).suffix(unit);
                                        if *int {
                                            d = d.fixed_decimals(0).speed(5.0);
                                        }
                                        ui.add(d);
                                    }
                                    numapilot::boards::Field::Points { key, value } => {
                                        ui.label(key.as_str());
                                        ui.horizontal_wrapped(|ui| {
                                            for p in value.iter_mut() {
                                                ui.add(egui::DragValue::new(&mut p.0).fixed_decimals(0).suffix(" MHz").speed(5.0));
                                                ui.add(egui::DragValue::new(&mut p.1).fixed_decimals(0).suffix(" mV").speed(5.0));
                                                ui.add_space(8.0);
                                            }
                                        });
                                    }
                                }
                                ui.end_row();
                            }
                        });
                        let changed = *fields != before;
                        let mut save = None;
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(changed, egui::Button::new("Salvar e reiniciar o governor"))
                                .on_hover_text("Faz backup da configuração, grava a nova e reinicia o serviço; se ele não subir, volta a anterior. Pede a senha.")
                                .clicked()
                            {
                                save = numapilot::boards::apply_fields(orig, fields).ok();
                            }
                            if ui.add_enabled(changed, egui::Button::new("Desfazer")).clicked() {
                                *fields = before.clone();
                            }
                        });
                        ui.label(
                            RichText::new("Suba o clock e a voltagem aos poucos e teste a estabilidade (ex.: Unigine Superposition) a cada passo.")
                                .small()
                                .color(Color32::from_rgb(230, 160, 40)),
                        );
                        if let Some(newtext) = save {
                            let tmp = std::env::temp_dir().join("numapilot-governor.toml");
                            match std::fs::write(&tmp, newtext) {
                                Ok(()) => {
                                    app.start_job(
                                        "Governor da GPU",
                                        vec![
                                            "pkexec".into(),
                                            crate::jobs::sibling_abs("numapilot"),
                                            "board".into(),
                                            "governor-apply".into(),
                                            tmp.to_string_lossy().to_string(),
                                        ],
                                    );
                                    app.board_edit = None;
                                }
                                Err(e) => app.toast(e.to_string(), false),
                            }
                        }
                    }
                }
            }
            let ok = Color32::from_rgb(110, 190, 120);
            if let Some((stock, full)) = b.cores {
                let n = numapilot::boards::active_cores();
                ui.label(if n >= full {
                    RichText::new(format!("CPU: {n} de {full} núcleos ativos (desbloqueados)")).color(ok)
                } else {
                    RichText::new(format!("CPU: {n} de {full} núcleos ativos (de fábrica: {stock}); dá para desbloquear pelos programas abaixo"))
                });
            }
            if let Some((stock, full)) = b.cus
                && let Some(n) = numapilot::boards::active_cu()
            {
                ui.label(if n >= full {
                    RichText::new(format!("GPU: {n} de {full} CUs ativos no boot (desbloqueados)")).color(ok)
                } else {
                    RichText::new(format!("GPU: {n} de {full} CUs ativos no boot (de fábrica: {stock}); o desbloqueio ao vivo não aparece aqui"))
                });
            }
            if !b.apps.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new("Programas da comunidade").strong());
                for a in &b.apps {
                    ui.horizontal(|ui| {
                        match a.installed_bin() {
                            Some(bin) => {
                                if ui.button(format!("Abrir {}", a.name)).clicked() {
                                    let bin = bin.to_string();
                                    if let Err(e) = std::process::Command::new(&bin).spawn() {
                                        app.toast(format!("{bin}: {e}"), false);
                                    }
                                }
                            }
                            None => {
                                ui.label(RichText::new(&a.name).strong());
                                if let Some(pkg) = &a.aur
                                    && ui.small_button("instalar (AUR)").on_hover_text("Abre um terminal com o paru/yay").clicked()
                                {
                                    let helper = if numapilot::containers::which("paru") { "paru" } else { "yay" };
                                    if let Err(e) = crate::jobs::in_terminal(&[helper.into(), "-S".into(), pkg.clone()]) {
                                        app.toast(e, false);
                                    }
                                }
                            }
                        }
                        if let Some(u) = &a.url
                            && ui.small_button("página").clicked()
                        {
                            let _ = crate::jobs::open_url(u);
                        }
                    });
                    ui.label(RichText::new(&a.about).small().weak());
                }
                ui.add_space(4.0);
            }
            if !b.cpu_oc_tools.is_empty() {
                let have = b.cpu_oc_tools.iter().all(|t| numapilot::containers::which(t));
                ui.horizontal(|ui| {
                    ui.label(if have {
                        "Overclock/undervolt da CPU pela SMU: ferramentas instaladas (bc250-detect / bc250-apply)."
                    } else {
                        "Overclock/undervolt da CPU pela SMU: instale o bc250_smu_oc."
                    });
                    if let Some(u) = &b.cpu_oc_url
                        && ui.small_button("abrir").clicked()
                    {
                        let _ = crate::jobs::open_url(u);
                    }
                });
            }
            if let Some(d) = &b.docs
                && ui.small_button("Guia da comunidade").clicked()
            {
                let _ = crate::jobs::open_url(d);
            }
        });
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
    nv_driver_panel(app, ui);
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
                board_ui(app, ui, g);
                ui.horizontal_wrapped(|ui| {
                    if let Some(l) = gpuctl::pcie_link(&g.pci) {
                        let txt = format!("PCIe Gen{} x{} (máx. Gen{} x{})", l.generation, l.width, l.max_gen, l.max_width);
                        if l.width < l.max_width {
                            ui.label(RichText::new(format!("{txt}: link mais estreito que o possível; confira o slot e a BIOS")).color(Color32::from_rgb(230, 160, 40)));
                        } else {
                            ui.label(RichText::new(txt).weak()).on_hover_text("A geração cai quando a placa está parada (economia); confira com ela em uso.");
                        }
                    }
                    if let Some((list, Some(a))) = gpuctl::power_profiles(&g.pci)
                        && let Some((_, n)) = list.iter().find(|(i, _)| *i == a)
                    {
                        ui.label(RichText::new(format!("· perfil de energia {n}")).weak())
                            .on_hover_text("Muda sozinho conforme o programa (Ajustar… → Perfil de energia da placa).");
                    }
                });
                if let Some(h) = &hist
                    && let Some(idx) = h.gpus.iter().position(|a| *a == g.alias)
                {
                    history_plot(ui, h, idx, app.hist_secs, s.power_cap_max_w.unwrap_or(300.0));
                }
                if g.is_nvidia() {
                    if numapilot::nvctl::driver_version().is_none() {
                        ui.label(RichText::new("O driver oficial da NVIDIA não está carregado: sem sensores nem controles.").color(Color32::from_rgb(230, 160, 40)));
                        return;
                    }
                    if numapilot::nvctl::asleep(&g.pci) {
                        ui.label(
                            RichText::new("Dormindo para economizar energia: acorda sozinha quando um programa usa a placa. Os controles aparecem com ela ligada.")
                                .weak(),
                        );
                        return;
                    }
                }
                if !s.fan_controllable && !s.fw_fan_curve && s.power_cap_max_w.is_none() && gpuctl::read_od(&g.pci).is_none() && !g.is_nvidia() && gpuctl::intel_freq(&g.pci).is_none() {
                    ui.label(RichText::new("O driver não expõe controles de ventoinha/potência para esta placa.").weak());
                    return;
                }
                let edit = app.gpu_edit.entry(g.pci.clone()).or_insert_with(|| saved.clone());
                ui.add_space(4.0);
                if s.fan_controllable || s.fw_fan_curve {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Ventoinha:").strong());
                        if s.fw_fan_curve {
                            ui.label(RichText::new("(curva gravada no firmware da placa)").small().weak());
                        }
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
                if g.is_nvidia() {
                    nv_clock_ui(ui, &g.pci, edit);
                } else if let Some((cur, _, _, lo, hi)) = gpuctl::intel_freq(&g.pci) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Frequência máxima:").strong());
                        let mut on = edit.clock_max_mhz.is_some();
                        if ui.checkbox(&mut on, "limitar").changed() {
                            edit.clock_max_mhz = on.then_some(hi * 3 / 4);
                        }
                        if let Some(m) = edit.clock_max_mhz.as_mut() {
                            ui.add(egui::Slider::new(m, lo.max(100)..=hi).suffix(" MHz").step_by(50.0));
                        } else {
                            ui.label(RichText::new(format!("de fábrica (até {hi} MHz)")).weak());
                        }
                        ui.label(RichText::new(format!("agora {cur} MHz")).weak());
                    });
                } else {
                    od_ui(ui, &g.pci, edit, &mut boot_job);
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
    if let Some(on) = boot_job {
        app.start_job(
            if on {
                "Ativando o OverDrive"
            } else {
                "Desativando o OverDrive"
            },
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
        app.send(Request::SetGpuControl { pci, control, trial: false });
        app.gpu_saved = gpuctl::GpuControls::load();
        app.gpu_edit.clear();
    }
}
