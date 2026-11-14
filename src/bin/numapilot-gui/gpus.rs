//! "Graphics cards" tab: temperatures, fan (automatic / curve / fixed) and power.

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
        (crate::i18n::tr("edge"), s.edge_c),
        ("hotspot", s.junction_c),
        (crate::i18n::tr("memory"), s.mem_c),
        ("APU", s.apu_c),
        ("VRM", s.vrm_c),
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

/// History chart: temperature (0–100 °C) and power (0–max W) of one GPU.
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
            crate::i18n::tr("no history yet (the service records every 30 s)"),
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
        crate::tf!(
            "temperature (max {maxt:.0} °C)",
            "temperatura (máx {maxt:.0} °C)"
        ),
        small.clone(),
        Color32::from_rgb(230, 120, 60),
    );
    p.text(
        full.center_bottom(),
        egui::Align2::CENTER_BOTTOM,
        crate::tf!(
            "red line = 90 °C · last {} h",
            "linha vermelha = 90 °C · últimas {} h",
            secs / 3600
        ),
        small.clone(),
        weak,
    );
    p.text(
        full.right_bottom() + egui::vec2(-2.0, 0.0),
        egui::Align2::RIGHT_BOTTOM,
        crate::tf!("power (max {maxp:.0} W)", "potência (máx {maxp:.0} W)"),
        small,
        Color32::from_rgb(90, 140, 220),
    );
}

/// Resizable BAR: status and what to do to enable it (with or without the BIOS option).
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
            RichText::new(crate::tf!(
                "Resizable BAR: active ({} visible to the CPU)",
                "Resizable BAR: ativo ({} visíveis pela CPU)",
                gb(b.bar_bytes)
            ))
            .color(Color32::from_rgb(110, 190, 120)),
        );
        return;
    }
    let nv = numapilot::nvctl::is_nvidia(pci);
    let mut txt = crate::tf!(
        "Resizable BAR: inactive (the CPU sees {} of the VRAM",
        "Resizable BAR: inativo (a CPU vê {} da VRAM",
        gb(b.bar_bytes)
    );
    if let Some(v) = b.vram_bytes {
        txt.push_str(&crate::tf!(" of {}", " de {}", gb(v)));
    }
    txt.push(')');
    ui.label(RichText::new(txt).color(Color32::from_rgb(230, 160, 40)));
    let advice = if !b.above_4g {
        crate::i18n::tr(
            "First step, in the BIOS: enable \"Above 4G Decoding\" (sometimes called \"64-bit PCI\" or \"MMIO above 4G\"). It will not work without it.",
        )
    } else if nv {
        crate::i18n::tr(
            "On NVIDIA the driver only uses the large BAR if the BIOS enables Resizable BAR (and the card is an RTX 30 or newer).",
        )
    } else {
        crate::i18n::tr(
            "AMD cards: the driver already tries to grow it at boot, even with the BIOS option off. If it could not, there was no room \
         in the PCI bridge window: the pci=realloc parameter (Machine → Kernel parameters) asks Linux to rearrange.",
        )
    };
    ui.label(RichText::new(advice).small().weak());
    if let Some(m) = b.max_bytes {
        ui.label(
            RichText::new(crate::tf!(
                "The card accepts up to {}.",
                "A placa aceita até {}.",
                gb(m)
            ))
            .small()
            .weak(),
        );
    }
}

/// OverDrive (clock and voltage) and Resizable BAR.
fn od_ui(ui: &mut egui::Ui, pci: &str, edit: &mut GpuControl, boot_job: &mut Option<bool>) {
    rebar_ui(ui, pci);
    let Some(od) = gpuctl::read_od(pci) else {
        let st = numapilot::bootcfg::overdrive_state();
        ui.horizontal_wrapped(|ui| {
            if st.on_next_boot {
                ui.label(RichText::new(crate::i18n::tr("Clock/voltage: OverDrive turns on when you reboot the computer.")).color(Color32::from_rgb(230, 160, 40)));
            } else {
                ui.label(RichText::new(crate::i18n::tr("Clock/voltage (undervolt/overclock): the driver's OverDrive is off.")).weak());
                if ui
                    .button(crate::i18n::tr("Enable OverDrive"))
                    .on_hover_text(
                        crate::i18n::tr("Writes the amdgpu driver option to /etc/modprobe.d and rebuilds the boot image. \
                         Works for any kernel, even after updates. Asks for the password; then just reboot."),
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
        ui.label(RichText::new(crate::i18n::tr("Clock and voltage:")).strong());
        let mut on = edit.od.is_some();
        if ui.checkbox(&mut on, crate::i18n::tr("customize")).changed() {
            edit.od = on.then(gpuctl::OdSettings::default);
        }
        ui.label(
            RichText::new(crate::tf!(
                "current: GPU up to {} MHz, memory {} MHz, curve {}",
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
            crate::i18n::tr("GPU max"),
        );
        ui.add_space(10.0);
        opt(
            ui,
            &mut s.mclk_max,
            od.mclk.iter().map(|x| x.1).max().unwrap_or(1000),
            od.range_mclk,
            crate::i18n::tr("memory max"),
        );
    });
    if let Some(cur) = od.vddgfx_offset {
        // RDNA2/3/4: undervolt by offset
        ui.horizontal(|ui| {
            let mut on = s.voltage_offset_mv.is_some();
            ui.checkbox(&mut on, crate::i18n::tr("offset the voltage"));
            let (lo, hi) = od.range_vo.unwrap_or((-200, 0));
            let mut v = s.voltage_offset_mv.unwrap_or(cur);
            ui.add_enabled(
                on,
                egui::Slider::new(&mut v, lo..=hi)
                    .suffix(" mV")
                    .step_by(5.0),
            );
            s.voltage_offset_mv = on.then_some(v);
            ui.label(
                RichText::new(crate::i18n::tr("negative = undervolt"))
                    .small()
                    .weak(),
            );
        });
    }
    if !od.curve.is_empty() {
        ui.horizontal(|ui| {
        let mut custom = !s.curve.is_empty();
        if ui.checkbox(&mut custom, crate::i18n::tr("voltage curve")).changed() {
            s.curve = if custom { od.curve.iter().map(|c| (c.1, c.2)).collect() } else { vec![] };
        }
        for mv in [25u32, 50] {
            if ui.small_button(crate::tf!("reduce {mv} mV", "reduzir {mv} mV")).on_hover_text(crate::i18n::tr("Undervolt: same frequency with less voltage (less heat and power). Test the stability.")).clicked() {
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
                        ui.label(crate::tf!("point {}", "ponto {}", i + 1));
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
    if od.vddgfx_offset.is_some() || !od.curve.is_empty() {
        auto_undervolt_button(ui, pci);
    }
    ui.label(
        RichText::new(crate::i18n::tr("If the card hangs or the game closes by itself, go back one step. On reboot the service reapplies; unchecking \"customize\" restores the default."))
            .small()
            .color(Color32::from_rgb(230, 160, 40)),
    );
}

/// Automatic undervolt button: opens a terminal with `numapilot undervolt` (shows the steps;
/// Ctrl+C stops and goes back to what it was).
fn auto_undervolt_button(ui: &mut egui::Ui, pci: &str) {
    ui.horizontal(|ui| {
        let have = numapilot::undervolt::stress_cmd().is_some();
        if ui
            .add_enabled(have, egui::Button::new(crate::i18n::tr("Find the undervolt automatically")))
            .on_hover_text(crate::i18n::tr("Tests step by step under load (1 min each) and saves the last stable one with 1 step of margin. Leave the PC idle; takes 10 to 30 min."))
            .clicked()
        {
            let _ = crate::jobs::in_terminal(&[crate::jobs::sibling_abs("numapilot"), "undervolt".into(), pci.to_string()]);
        }
        if !have {
            ui.label(RichText::new(crate::i18n::tr("needs vkmark: sudo pacman -S vkmark")).small().weak());
        }
    });
}

/// NVIDIA: lock the maximum clock (the driver does not allow changing the voltage on Linux).
fn nv_clock_ui(ui: &mut egui::Ui, pci: &str, edit: &mut GpuControl) {
    rebar_ui(ui, pci);
    let Some(g) = numapilot::nvctl::get(pci) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::i18n::tr("Maximum clock:")).strong());
        let mut on = edit.clock_max_mhz.is_some();
        let hi = g.clock_max_mhz.unwrap_or(2500.0) as u32;
        if ui.checkbox(&mut on, crate::i18n::tr("lock")).changed() {
            edit.clock_max_mhz = on.then_some((hi as f64 * 0.85) as u32);
        }
        if let Some(m) = edit.clock_max_mhz.as_mut() {
            ui.add(egui::Slider::new(m, 300..=hi).suffix(" MHz").step_by(15.0));
        } else {
            ui.label(
                RichText::new(crate::tf!(
                    "normal (up to {hi} MHz)",
                    "normal (até {hi} MHz)"
                ))
                .weak(),
            );
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Undervolt:").strong())
            .on_hover_text(crate::i18n::tr("Shifts the clock/voltage curve up: with the maximum clock locked, the card reaches it at a lower voltage. Go up 15 MHz at a time and test in a game; if it hangs or shows artifacts, go back one step."));
        let mut on = edit.nv_gpc_offset_mhz.is_some();
        if ui.checkbox(&mut on, crate::i18n::tr("offset the curve")).changed() {
            edit.nv_gpc_offset_mhz = on.then_some(75);
        }
        if let Some(v) = edit.nv_gpc_offset_mhz.as_mut() {
            ui.add(egui::Slider::new(v, -200..=300).suffix(" MHz").step_by(15.0));
        }
    });
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("Memory:"));
        let mut on = edit.nv_mem_offset_mhz.is_some();
        if ui.checkbox(&mut on, crate::i18n::tr("offset")).changed() {
            edit.nv_mem_offset_mhz = on.then_some(0);
        }
        if let Some(v) = edit.nv_mem_offset_mhz.as_mut() {
            ui.add(
                egui::Slider::new(v, -500..=1500)
                    .suffix(" MHz")
                    .step_by(50.0),
            );
        }
    });
    auto_undervolt_button(ui, pci);
    if edit.nv_gpc_offset_mhz.is_some_and(|v| v > 0) && edit.clock_max_mhz.is_none() {
        ui.label(
            RichText::new(crate::i18n::tr("Without locking the maximum clock, the offset becomes an overclock (more clock, same voltage). For undervolt, lock the clock too."))
                .small()
                .color(Color32::from_rgb(230, 160, 40)),
        );
    }
    ui.label(
        RichText::new(
            crate::i18n::tr("Undervolt on NVIDIA: lock the clock (e.g. 1800 MHz) and offset the curve (+100 to +200 MHz): same FPS using less power and heat. The \"Find automatically\" button tests the steps for you."),
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
            ui.label(RichText::new(crate::i18n::tr("NVIDIA driver")).strong());
            if let Some(b) = &st.branch {
                ui.label(RichText::new(crate::tf!("recommended for your card: {}", "indicado para a sua placa: {}", b.label())).weak());
            }
        });
        ui.label(crate::tf!(
            "Loaded: {}   Installed: {}   In the repository: {}", "Carregado: {}   Instalado: {}   No repositório: {}",
            st.loaded.clone().unwrap_or(crate::t2!("none", "nenhum").into()),
            st.installed.clone().unwrap_or(crate::i18n::tr("no").into()),
            st.available.clone().unwrap_or("?".into())
        ));
        if st.nouveau {
            ui.label(RichText::new(crate::i18n::tr("Using nouveau (the community's open driver): no performance for games.")).color(Color32::from_rgb(230, 160, 40)));
        }
        if st.needs_reboot {
            ui.label(RichText::new(crate::i18n::tr("The installed driver is newer than the loaded one: reboot.")).color(Color32::from_rgb(230, 160, 40)));
        }
        let newer = matches!((&st.installed, &st.available), (Some(i), Some(a)) if i != a);
        ui.horizontal(|ui| {
            let label = if st.installed.is_some() { crate::i18n::tr("Reinstall official driver") } else { crate::i18n::tr("Install official driver") };
            if ui
                .button(RichText::new(label).strong())
                .on_hover_text(crate::i18n::tr("Updates the system and installs the right driver for the card's generation (through CachyOS's chwd). Asks for the password; then reboot."))
                .clicked()
            {
                app.start_job(crate::i18n::tr("NVIDIA driver"), vec!["pkexec".into(), crate::jobs::sibling_abs("numapilot"), "driver".into(), "install".into()]);
                app.nv_status = None;
            }
            if st.installed.is_some()
                && ui
                    .add_enabled(newer, egui::Button::new(crate::i18n::tr("Update")))
                    .on_hover_text(crate::i18n::tr("On CachyOS the driver updates along with the system (pacman -Syu). Asks for the password."))
                    .clicked()
            {
                app.start_job(crate::i18n::tr("Updating system and driver"), vec!["pkexec".into(), crate::jobs::sibling_abs("numapilot"), "driver".into(), "update".into()]);
                app.nv_status = None;
            }
            if !newer && st.installed.is_some() {
                ui.label(RichText::new(crate::i18n::tr("up to date")).color(Color32::from_rgb(90, 180, 110)));
            }
        });
    });
    ui.add_space(4.0);
}

/// Architecture (what works for compute and upscaling) and the card's community tweaks.
fn board_ui(app: &mut App, ui: &mut egui::Ui, g: &numapilot::topo::Gpu) {
    let rec = numapilot::recipes::load();
    if let Some(a) = rec.arch_for(g) {
        let up: Vec<&str> = a
            .upscale
            .iter()
            .map(|u| match u.as_str() {
                "fsr4" => "FSR 4",
                "fsr4-rdna3" => crate::i18n::tr("FSR 4 (through vkd3d-proton)"),
                "fsr4-int8-experimental" => "FSR 4 INT8 (experimental)",
                "fsr3" => "FSR 3.1 (OptiScaler)",
                "xess" => "XeSS",
                "dlss" => "DLSS",
                "fsr1-gamescope" => "FSR 1 (gamescope)",
                "lsfg" => crate::i18n::tr("frame generation (lsfg-vk)"),
                o => o,
            })
            .collect();
        let compute = match a.compute.as_str() {
            "rocm" => crate::i18n::tr("Official ROCm"),
            "rocm-community" | "rocm-comunidade" => crate::i18n::tr("Community ROCm"),
            "vulkan" => "Vulkan (llama.cpp)",
            "cuda" => "CUDA",
            o => o,
        };
        ui.label(
            RichText::new(crate::tf!(
                "{} · AI: {compute} · upscaling: {}",
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
    egui::CollapsingHeader::new(RichText::new(crate::tf!("{}: community tweaks", "{}: ajustes da comunidade", b.name)).strong())
        .id_salt(format!("board-{}", g.pci))
        .default_open(!b.governor_services.is_empty())
        .show(ui, |ui| {
            first_setup_ui(app, ui, &b, g);
            for n in &b.notes {
                ui.label(RichText::new(format!("• {}", numapilot::i18n::tr_text(n))).small());
            }
            if !b.governor_services.is_empty() {
                let sv = numapilot::boards::services(&b);
                match sv.iter().find(|s| s.active) {
                    Some(s) => {
                        ui.label(RichText::new(crate::tf!("Clock governor: {} running", "Governor de clock: {} rodando", s.name)).color(Color32::from_rgb(110, 190, 120)));
                    }
                    None => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::i18n::tr("No governor running: the GPU stays at a fixed clock.")).color(Color32::from_rgb(230, 160, 40)));
                            if let Some(s) = sv.iter().find(|s| s.installed) {
                                if ui.button(crate::tf!("Enable {}", "Ligar {}", s.name)).clicked() {
                                    app.start_job(
                                        "Governor",
                                        vec!["pkexec".into(), "systemctl".into(), "enable".into(), "--now".into(), format!("{}.service", s.name)],
                                    );
                                }
                            } else if let Some(pkg) = b.governor_aur.first()
                                && ui.button(crate::i18n::tr("Install the governor (AUR)")).on_hover_text(crate::i18n::tr("Opens a terminal with paru/yay")).clicked()
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
                        ui.label(RichText::new(crate::tf!("Configuration: {path}", "Configuração: {path}")).small().weak());
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
                                .add_enabled(changed, egui::Button::new(crate::i18n::tr("Save and restart the governor")))
                                .on_hover_text(crate::i18n::tr("Backs up the configuration, writes the new one and restarts the service; if it does not come up, the previous one is restored. Asks for the password."))
                                .clicked()
                            {
                                save = numapilot::boards::apply_fields(orig, fields).ok();
                            }
                            if ui.add_enabled(changed, egui::Button::new(crate::i18n::tr("Undo"))).clicked() {
                                *fields = before.clone();
                            }
                        });
                        ui.label(
                            RichText::new(crate::i18n::tr("Raise the clock and the voltage little by little and test the stability (e.g. Unigine Superposition) at each step."))
                                .small()
                                .color(Color32::from_rgb(230, 160, 40)),
                        );
                        if let Some(newtext) = save {
                            let tmp = std::env::temp_dir().join("numapilot-governor.toml");
                            match std::fs::write(&tmp, newtext) {
                                Ok(()) => {
                                    app.start_job(
                                        crate::i18n::tr("GPU governor"),
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
            // ----- now: CPU cores and clock, GPU CUs and clock -----
            if b.cores.is_some() || b.cus.is_some() {
                ui.add_space(4.0);
                egui::Grid::new(format!("agora-{}", g.pci)).num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    if let Some((stock, full)) = b.cores {
                        let n = numapilot::boards::active_cores();
                        let mhz = numapilot::boards::cpu_mhz_now()
                            .map(|m| format!(" · {:.2} GHz", m as f64 / 1000.0))
                            .unwrap_or_default();
                        ui.label(RichText::new("CPU").strong());
                        ui.label(RichText::new(crate::tf!("{n} of {full} cores{mhz}", "{n} de {full} núcleos{mhz}")).color(if n >= full { ok } else { ui.visuals().text_color() }));
                        ui.end_row();
                        ui.label("");
                        let txt = match numapilot::boards::core_unlock_persistence(n, full, numapilot::boards::efi_core_unlock()) {
                            Some(p) => crate::tf!("unlocked: {p}", "desbloqueados: {p}"),
                            None => crate::tf!("factory: {stock}; can be unlocked with BC250 Control Center", "de fábrica: {stock}; dá para desbloquear pelo BC250 Control Center"),
                        };
                        ui.label(RichText::new(txt).small().weak());
                        ui.end_row();
                    }
                    if let Some((stock, full)) = b.cus {
                        let clock = gpuctl::read_sensors(&g.pci)
                            .clock_mhz
                            .map(|c| format!(" · {c:.0} MHz"))
                            .unwrap_or_default();
                        ui.label(RichText::new("GPU").strong());
                        match numapilot::boards::cu_now() {
                            Some((n, src)) => {
                                ui.label(RichText::new(crate::tf!("{n} of {full} CUs{clock}", "{n} de {full} CUs{clock}")).color(if n >= full { ok } else { ui.visuals().text_color() }));
                                ui.end_row();
                                ui.label("");
                                let mut txt = src.text().to_string();
                                if src == numapilot::boards::CuSource::Kernel {
                                    txt.push_str(" (bc250_cc_write_mode=3): não use o desbloqueio ao vivo junto");
                                } else if src == numapilot::boards::CuSource::Boot && n < full {
                                    txt.push_str(&crate::tf!(" (factory: {stock}); the live unlock shows up here after a reading in BC250 Control Center", " (de fábrica: {stock}); desbloqueio ao vivo aparece aqui depois de uma leitura no BC250 Control Center"));
                                }
                                ui.label(RichText::new(txt).small().weak());
                            }
                            None => {
                                ui.label(crate::tf!("CUs unknown{clock}", "CUs desconhecidos{clock}"));
                            }
                        }
                        ui.end_row();
                    }
                });
            }
            // ----- community programs: the recommended one first -----
            if !b.apps.is_empty() {
                ui.add_space(6.0);
                let helper = if numapilot::containers::which("paru") { "paru" } else { "yay" };
                let mut run: Option<(String, Vec<String>)> = None;
                let mut apps = b.apps.clone();
                apps.sort_by_key(|a| !a.recommended);
                let (rec, rest): (Vec<_>, Vec<_>) = apps.into_iter().partition(|a| a.recommended);
                for a in &rec {
                    egui::Frame::new()
                        .fill(ui.visuals().faint_bg_color)
                        .stroke(egui::Stroke::new(1.0, ok))
                        .corner_radius(6.0)
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("★ {}", a.name)).strong().size(15.0));
                                ui.label(RichText::new(crate::i18n::tr("recommended")).small().color(ok));
                            });
                            ui.label(RichText::new(numapilot::i18n::tr_text(&a.about)).small());
                            app_buttons(app, ui, a, helper, &mut run, true);
                        });
                }
                if !rest.is_empty() {
                    egui::CollapsingHeader::new(crate::tf!("Other community programs ({})", "Outros programas da comunidade ({})", rest.len()))
                        .id_salt(format!("outros-{}", g.pci))
                        .show(ui, |ui| {
                            ui.label(RichText::new(crate::i18n::tr("Installed from the maintainer's newest code (AUR -git or git); \"Update\" downloads it again.")).small().weak());
                            for a in &rest {
                                ui.add_space(4.0);
                                ui.label(RichText::new(&a.name).strong());
                                ui.label(RichText::new(numapilot::i18n::tr_text(&a.about)).small().weak());
                                app_buttons(app, ui, a, helper, &mut run, false);
                            }
                        });
                }
                if let Some((name, cmd)) = run {
                    numapilot::log::info(&crate::tf!("community program {name}: {}", "programa da comunidade {name}: {}", cmd.join(" ")));
                    if let Err(e) = crate::jobs::in_terminal(&cmd) {
                        app.toast(e, false);
                    }
                }
                ui.add_space(4.0);
            }
            if let Some(d) = &b.docs
                && ui.small_button(crate::i18n::tr("Community guide")).clicked()
            {
                let _ = crate::jobs::open_url(d);
            }
        });
}

/// Buttons of a community program: Open, Install or Update (in the terminal) and the page.
/// First setup of the board: what is already right and what is missing, with a button per step.
fn first_setup_ui(
    app: &mut App,
    ui: &mut egui::Ui,
    b: &numapilot::recipes::Board,
    g: &numapilot::topo::Gpu,
) {
    use numapilot::boards::{StepAction, StepState};
    // the readings call systemctl and read the kernel log: cached for 5 s
    let id = egui::Id::unique(("first-setup", &g.pci));
    let now = std::time::Instant::now();
    let cached: Option<(std::time::Instant, Vec<numapilot::boards::SetupStep>)> =
        ui.data(|d| d.get_temp(id));
    let steps = match cached {
        Some((t, s)) if now.duration_since(t).as_secs() < 5 => s,
        _ => {
            let s = numapilot::boards::first_setup(b, &g.pci);
            ui.data_mut(|d| d.insert_temp(id, (now, s.clone())));
            s
        }
    };
    if steps.is_empty() {
        return;
    }
    let p = crate::theme::pal(ui);
    let todo = steps.iter().any(|s| s.state == StepState::Todo);
    egui::CollapsingHeader::new(
        RichText::new(crate::tf!(
            "First setup: {}",
            "Primeira configuração: {}",
            numapilot::boards::setup_summary(&steps)
        ))
        .color(if todo { p.warn } else { p.ok }),
    )
    .id_salt(("first-setup-h", &g.pci))
    .default_open(todo)
    .show(ui, |ui| {
        let helper = if numapilot::containers::which("paru") {
            "paru"
        } else {
            "yay"
        };
        for s in &steps {
            ui.horizontal_wrapped(|ui| {
                let (tag, c) = match s.state {
                    StepState::Ok => (crate::t2!("ready", "pronto"), p.ok),
                    StepState::Todo => (crate::i18n::tr("missing"), p.warn),
                    StepState::Opcional => ("opcional", p.weak),
                };
                crate::theme::chip(ui, tag, c);
                ui.label(RichText::new(&s.title).strong());
                ui.label(RichText::new(&s.detail).small().weak());
                match &s.action {
                    Some(StepAction::EnableService(name)) => {
                        if ui
                            .small_button(crate::i18n::tr("Enable (asks for the password)"))
                            .clicked()
                        {
                            app.start_job(
                                "Governor",
                                vec![
                                    "pkexec".into(),
                                    "systemctl".into(),
                                    "enable".into(),
                                    "--now".into(),
                                    format!("{name}.service"),
                                ],
                            );
                        }
                    }
                    Some(StepAction::InstallAur(pkg)) => {
                        if ui.small_button(crate::i18n::tr("Install (AUR)")).clicked()
                            && let Err(e) =
                                crate::jobs::in_terminal(&[helper.into(), "-S".into(), pkg.clone()])
                        {
                            app.toast(e, false);
                        }
                    }
                    Some(StepAction::RebuildAur(name)) => {
                        if ui
                            .small_button(crate::i18n::tr("Rebuild (AUR)"))
                            .on_hover_text(crate::i18n::tr("Rebuilds the package against the current system libraries, in a terminal (may ask for the password)"))
                            .clicked()
                            && let Some(cmd) = numapilot::boards::broken_apps(b)
                                .into_iter()
                                .find(|a| &a.name == name)
                                .and_then(|a| a.fix_command(helper))
                            && let Err(e) = crate::jobs::in_terminal(&cmd)
                        {
                            app.toast(e, false);
                        }
                    }
                    Some(StepAction::InstallApp(name)) => {
                        if ui.small_button(crate::i18n::tr("Install")).clicked()
                            && let Some(cmd) = b
                                .apps
                                .iter()
                                .find(|a| &a.name == name)
                                .and_then(|a| a.install_command(helper))
                            && let Err(e) = crate::jobs::in_terminal(&cmd)
                        {
                            app.toast(e, false);
                        }
                    }
                    Some(StepAction::GpuMemory) => {
                        ui.label(
                            RichText::new(crate::i18n::tr("(section further down in this tab)"))
                                .small()
                                .weak(),
                        );
                    }
                    None => {}
                }
            });
        }
        ui.label(
            RichText::new(crate::i18n::tr(
                "On the command line: numapilot board setup",
            ))
            .small()
            .weak(),
        );
    });
    ui.add_space(4.0);
}

fn app_buttons(
    app: &mut App,
    ui: &mut egui::Ui,
    a: &numapilot::recipes::CommunityApp,
    helper: &str,
    run: &mut Option<(String, Vec<String>)>,
    big: bool,
) {
    ui.horizontal_wrapped(|ui| {
        let bin = a.installed_bin();
        let installed = bin.is_some() || a.installed();
        if let Some(bin) = &bin {
            let b = egui::Button::new(if big {
                crate::i18n::tr("⚙ Manage cores, CUs, frequencies and voltages").to_string()
            } else {
                crate::tf!("Open {}", "Abrir {}", a.name)
            });
            if ui.add(if big { b.min_size(egui::vec2(0.0, 28.0)) } else { b }).clicked()
                && let Err(e) = std::process::Command::new(bin).spawn()
            {
                app.toast(format!("{bin}: {e}"), false);
            }
        }
        if a.installable() {
            let (label, hover) = if installed {
                (crate::i18n::tr("Update"), crate::i18n::tr("Downloads and installs the maintainer's newest version (in a terminal; may ask for the password)"))
            } else if big {
                (crate::i18n::tr("Install BC250 Control Center"), crate::i18n::tr("Installs from the AUR (the author's -git package) in a terminal; may ask for the password. Afterwards this button becomes \"Manage cores, CUs, frequencies\""))
            } else if a.aur.is_some() {
                (crate::i18n::tr("Install"), crate::i18n::tr("Installs from the AUR (paru/yay) in a terminal; may ask for the password"))
            } else {
                (crate::i18n::tr("Install"), crate::i18n::tr("Downloads the code (git) into ~/.local/share/numapilot/comunidade and creates the commands in ~/.local/bin, in a terminal"))
            };
            let b = egui::Button::new(label);
            if ui.add(if big && !installed { b.min_size(egui::vec2(0.0, 28.0)) } else { b }).on_hover_text(hover).clicked()
                && let Some(cmd) = a.install_command(helper)
            {
                *run = Some((a.name.clone(), cmd));
            }
        }
        if let Some(u) = &a.url
            && ui.small_button(crate::i18n::tr("page")).clicked()
        {
            let _ = crate::jobs::open_url(u);
        }
        if !installed && !a.installable() {
            ui.label(RichText::new(crate::i18n::tr("manual install (see the page)")).small().weak());
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
            crate::i18n::tr("Temperature, fan and power of each card. The numapilot service applies them; \
             if it stops, the fans go back to the card's own control. Above 95 °C the fan goes to 100%."),
        )
        .weak(),
    );
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("History:"));
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
        let topo_n = gpus.len();
        let mut test_gpu: Option<String> = None;
        for g in &gpus {
            let s = gpuctl::read_sensors(&g.pci);
            let saved = app.gpu_saved.get(&g.pci);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.heading(g.short_name());
                    ui.label(
                        RichText::new(format!(
                            "{} · {} · socket {} · {}",
                            g.alias,
                            g.name,
                            g.numa_node.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                            g.pci
                        ))
                        .weak(),
                    );
                    if topo_n > 1
                        && ui
                            .small_button(crate::i18n::tr("Test this card's selection"))
                            .on_hover_text(crate::i18n::tr("Runs vulkaninfo and glxinfo the way a game would on this card and shows which card answered (Vulkan/DirectX and OpenGL)."))
                            .clicked()
                    {
                        test_gpu = Some(g.alias.clone());
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    temps(ui, &s);
                    ui.separator();
                    if let Some(rpm) = s.fan_rpm {
                        ui.label(crate::tf!("fan {rpm} RPM", "ventoinha {rpm} RPM"));
                    }
                    if let Some(p) = s.fan_pct {
                        ui.label(format!("({p:.0}%)"));
                    }
                    ui.separator();
                    if let Some(w) = s.power_w {
                        ui.label(format!("{w:.0} W"));
                    }
                    if let Some(c) = s.clock_mhz {
                        ui.label(format!("{c:.0} MHz"));
                    }
                    if let Some(mv) = s.voltage_mv {
                        ui.label(format!("{mv:.0} mV"));
                    }
                    if let Some(c) = s.power_cap_w {
                        ui.label(RichText::new(crate::tf!("limit {c:.0} W", "limite {c:.0} W")).weak());
                    }
                    ui.separator();
                    ui.label(crate::tf!("usage {}%", "uso {}%", g.busy_percent().unwrap_or(0)));
                    if let Some((u, t)) = g.vram_used_total() {
                        ui.label(format!("VRAM {:.1}/{:.0} GB", u as f64 / 1073741824.0, t as f64 / 1073741824.0));
                    }
                });
                board_ui(app, ui, g);
                apu_memory_ui(app, ui, g);
                ui.horizontal_wrapped(|ui| {
                    if let Some(l) = gpuctl::pcie_link(&g.pci) {
                        let txt = crate::tf!("PCIe Gen{} x{} (max Gen{} x{})", "PCIe Gen{} x{} (máx. Gen{} x{})", l.generation, l.width, l.max_gen, l.max_width);
                        if l.width < l.max_width {
                            ui.label(RichText::new(crate::tf!("{txt}: link narrower than possible; check the slot and the BIOS", "{txt}: link mais estreito que o possível; confira o slot e a BIOS")).color(Color32::from_rgb(230, 160, 40)));
                        } else {
                            ui.label(RichText::new(txt).weak()).on_hover_text(crate::i18n::tr("The generation drops when the card is idle (power saving); check it while in use."));
                        }
                    }
                    if let Some((list, Some(a))) = gpuctl::power_profiles(&g.pci)
                        && let Some((_, n)) = list.iter().find(|(i, _)| *i == a)
                    {
                        ui.label(RichText::new(crate::tf!("· power profile {n}", "· perfil de energia {n}")).weak())
                            .on_hover_text(crate::i18n::tr("Changes by itself depending on the program (Adjust… → Card power profile)."));
                    }
                });
                if let Some(h) = &hist
                    && let Some(idx) = h.gpus.iter().position(|a| *a == g.alias)
                {
                    history_plot(ui, h, idx, app.hist_secs, s.power_cap_max_w.unwrap_or(300.0));
                }
                if g.is_nvidia() {
                    if numapilot::nvctl::driver_version().is_none() {
                        ui.label(RichText::new(crate::i18n::tr("The official NVIDIA driver is not loaded: no sensors or controls.")).color(Color32::from_rgb(230, 160, 40)));
                        return;
                    }
                    if numapilot::nvctl::asleep(&g.pci) {
                        ui.label(
                            RichText::new(crate::i18n::tr("Asleep to save power: it wakes by itself when a program uses the card. The controls show up when it is on."))
                                .weak(),
                        );
                        return;
                    }
                }
                if !s.fan_controllable && !s.fw_fan_curve && s.power_cap_max_w.is_none() && gpuctl::read_od(&g.pci).is_none() && !g.is_nvidia() && gpuctl::intel_freq(&g.pci).is_none() {
                    ui.label(RichText::new(crate::i18n::tr("The driver does not expose fan/power controls for this card.")).weak());
                    return;
                }
                let edit = app.gpu_edit.entry(g.pci.clone()).or_insert_with(|| saved.clone());
                ui.add_space(4.0);
                if s.fan_controllable || s.fw_fan_curve {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::i18n::tr("Fan:")).strong());
                        if s.fw_fan_curve {
                            ui.label(RichText::new(crate::i18n::tr("(curve stored in the card's firmware)")).small().weak());
                        }
                        ui.radio_value(&mut edit.fan, FanMode::Auto, crate::i18n::tr("card's own (automatic)"));
                        ui.radio_value(&mut edit.fan, FanMode::Curve, crate::i18n::tr("curve"));
                        ui.radio_value(&mut edit.fan, FanMode::Fixed, crate::i18n::tr("fixed"));
                    });
                    match edit.fan {
                        FanMode::Fixed => {
                            ui.add(egui::Slider::new(&mut edit.fixed_pct, 0.0..=100.0).suffix("%").text(crate::i18n::tr("speed")));
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
                                        if ui.small_button(crate::i18n::tr("+ point")).clicked() {
                                            let last = edit.curve.last().copied().unwrap_or((70.0, 60.0));
                                            edit.curve.push(((last.0 + 5.0).min(100.0), last.1));
                                        }
                                        if ui.small_button(crate::i18n::tr("default curve")).clicked() {
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
                        ui.label(RichText::new(crate::i18n::tr("Power:")).strong());
                        let mut on = edit.power_cap_w.is_some();
                        if ui.checkbox(&mut on, crate::i18n::tr("limit")).changed() {
                            edit.power_cap_w = if on { Some(s.power_cap_w.unwrap_or(hi).round()) } else { None };
                        }
                        if let Some(w) = edit.power_cap_w.as_mut() {
                            ui.add(egui::Slider::new(w, lo.round()..=hi.round()).suffix(" W").step_by(5.0));
                        } else {
                            ui.label(RichText::new(crate::i18n::tr("card default")).weak());
                        }
                    });
                }
                ui.add_space(4.0);
                if g.is_nvidia() {
                    nv_clock_ui(ui, &g.pci, edit);
                } else if let Some((cur, _, _, lo, hi)) = gpuctl::intel_freq(&g.pci) {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::i18n::tr("Maximum frequency:")).strong());
                        let mut on = edit.clock_max_mhz.is_some();
                        if ui.checkbox(&mut on, crate::i18n::tr("limit")).changed() {
                            edit.clock_max_mhz = on.then_some(hi * 3 / 4);
                        }
                        if let Some(m) = edit.clock_max_mhz.as_mut() {
                            ui.add(egui::Slider::new(m, lo.max(100)..=hi).suffix(" MHz").step_by(50.0));
                        } else {
                            ui.label(RichText::new(crate::tf!("factory (up to {hi} MHz)", "de fábrica (até {hi} MHz)")).weak());
                        }
                        ui.label(RichText::new(crate::tf!("now {cur} MHz", "agora {cur} MHz")).weak());
                    });
                } else {
                    od_ui(ui, &g.pci, edit, &mut boot_job);
                }
                if g.vendor == 0x1002 && !g.is_integrated() {
                    ui.checkbox(&mut edit.idle_save, crate::i18n::tr("Power saving when idle"))
                        .on_hover_text(crate::i18n::tr("After 1 min idle, the card stays at the lowest clock and voltage level until it is used again. Helps on the MI50s, which draw a lot when idle; the first request afterwards takes a fraction of a second longer."));
                }
                let changed = *edit != saved;
                let edit_c = edit.clone();
                ui.horizontal(|ui| {
                    if ui.add_enabled(changed, egui::Button::new(crate::i18n::tr("Apply"))).clicked() {
                        app.pending_gpu = Some((g.pci.clone(), edit_c.clone()));
                    }
                    if ui.add_enabled(changed, egui::Button::new(crate::i18n::tr("Undo"))).clicked() {
                        app.gpu_edit.insert(g.pci.clone(), saved.clone());
                    }
                });
            });
            ui.add_space(6.0);
        }
        if let Some(alias) = test_gpu {
            app.start_job(
                crate::i18n::tr("Test the card selection"),
                vec![
                    crate::jobs::sibling_abs("numapilot"),
                    "gpu-test".into(),
                    alias,
                ],
            );
        }
    });
    if let Some(on) = boot_job {
        app.start_job(
            if on {
                crate::i18n::tr("Enabling OverDrive")
            } else {
                crate::i18n::tr("Disabling OverDrive")
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
        app.send(Request::SetGpuControl {
            pci,
            control,
            trial: false,
        });
        app.gpu_saved = gpuctl::GpuControls::load();
        app.gpu_edit.clear();
    }
}

/// Memory the integrated GPU (APU/BC-250) may use: the kernel default is half the RAM, which
/// limits large AI models. Writes to /etc/modprobe.d (ttm.pages_limit) and applies right away.
fn apu_memory_ui(app: &mut App, ui: &mut egui::Ui, g: &numapilot::topo::Gpu) {
    let bc = numapilot::recipes::load()
        .board_for(g)
        .is_some_and(|b| b.cus.is_some());
    // Only on the BC-250 or on a PC without a dedicated card (where the integrated GPU runs the AI).
    let only_igpu = !app.topo.gpus.iter().any(|o| !o.is_integrated());
    if g.vendor != 0x1002 || !(bc || g.is_integrated() && only_igpu) {
        return;
    }
    use numapilot::bootcfg;
    let total = bootcfg::mem_total_gib();
    if total < 4.0 {
        return;
    }
    let gib = |p: u64| p as f64 * 4096.0 / 1073741824.0;
    let now = bootcfg::ttm_now().map(gib);
    let saved = bootcfg::saved_ttm().map(gib);
    // BC250 Control Center applies its own limit at every boot: two tools on the same setting
    // would undo each other
    if saved.is_none()
        && let Some(cc) = bootcfg::cc_ttm().map(gib)
    {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(crate::i18n::tr("Memory for the GPU (AI):")).strong());
            ui.label(crate::tf!(
                "{:.1} GiB, set by BC250 Control Center (its bc250-memory-setup service applies it at every boot). Change it there so the two programs do not undo each other.",
                "{:.1} GiB, configurado pelo BC250 Control Center (o serviço bc250-memory-setup dele aplica a cada boot). Mude por ele, para os dois programas não desfazerem um ao outro.",
                cc
            ));
        });
        return;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::i18n::tr("Memory for the GPU (AI):")).strong())
            .on_hover_text(crate::i18n::tr("How much RAM the integrated GPU may use. The kernel default is half, which keeps large models from loading. Leave 1-2 GiB for the system."));
        let id = ui.scope_id().with(("apumem", &g.pci));
        let mut v: f64 = ui.data(|d| d.get_temp(id)).unwrap_or(saved.or(now).unwrap_or(total / 2.0).min(total - 1.0));
        ui.add(egui::Slider::new(&mut v, 2.0..=(total - 1.0).max(2.0)).suffix(" GiB").step_by(0.5).fixed_decimals(1));
        ui.data_mut(|d| d.insert_temp(id, v));
        if ui.button(crate::i18n::tr("Apply")).on_hover_text(crate::i18n::tr("Saves for the next boots and applies now (asks for the password)")).clicked() {
            app.start_job(
                crate::i18n::tr("GPU memory"),
                vec!["pkexec".into(), crate::jobs::sibling_abs("numapilot"), "boot".into(), "gpu-memory".into(), format!("{v:.1}")],
            );
        }
        if saved.is_some() && ui.small_button(crate::i18n::tr("default")).clicked() {
            app.start_job(
                crate::i18n::tr("GPU memory"),
                vec!["pkexec".into(), crate::jobs::sibling_abs("numapilot"), "boot".into(), "gpu-memory".into(), "padrao".into()],
            );
        }
        ui.label(
            RichText::new(crate::tf!(
                "now {} · at boot {}", "agora {} · no boot {}",
                now.map(|x| format!("{x:.1} GiB")).unwrap_or("?".into()),
                saved.map(|x| format!("{x:.1} GiB")).unwrap_or(crate::i18n::tr("default").into())
            ))
            .weak(),
        );
    });
    if bc {
        ui.label(
            RichText::new(crate::i18n::tr("BC-250: with 16 GB, 14 GiB lets models up to ~14B load (BIOS with UMA at 512 MB). The AI server uses Vulkan and already sets GGML_VK_PREFER_HOST_MEMORY (+5 to 12% in generation)."))
                .small()
                .weak(),
        );
    }
}
