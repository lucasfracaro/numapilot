//! "Adjust program" window: where on the hardware it runs (cores, memory, priority, GPU),
//! with ready-made profiles and compatibility warnings for games.

use crate::App;
use crate::map::core_map;
use eframe::egui::{self, RichText};
use numapilot::ctl::Request;
use numapilot::decide::{LaunchInput, decide_launch, game_warnings};
use numapilot::learned::Learned;
use numapilot::profiles::{Profile, Profiles};
use numapilot::util::{format_cpulist, parse_cpulist};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub struct Editor {
    pub key: String,
    pub label: String,
    pub main_pid: Option<i32>,
    pub current_cpus: String,
    pub mem: Vec<u64>,
    pub p: Profile,
    pub picking: bool,
    pub pick: BTreeSet<usize>,
    /// Menu shortcut numapilot made for this program (not for Steam games).
    pub shortcut: Option<std::path::PathBuf>,
    /// The open program was started directly, not through `numapilot run`: the card, what it
    /// sees and the upscaler of this adjustment do not reach it.
    pub direct: bool,
    /// Last real × shown measurement of the presentation block (read when the window opens).
    pub present_last: Option<numapilot::present::Measured>,
}

impl Editor {
    pub fn new(
        key: &str,
        label: &str,
        main_pid: Option<i32>,
        cpus: &str,
        mem: &[u64],
        p: Profile,
    ) -> Editor {
        let pick: BTreeSet<usize> = p
            .cpu_list
            .as_deref()
            .map(parse_cpulist)
            .unwrap_or_default()
            .into_iter()
            .collect();
        Editor {
            key: key.into(),
            label: label.into(),
            main_pid,
            direct: main_pid.is_some_and(|pid| {
                numapilot::procfs::env_var(pid, "NUMAPILOT_MODE").is_none()
                    && !numapilot::procfs::environ(pid).is_empty()
            }),
            current_cpus: cpus.into(),
            mem: mem.to_vec(),
            picking: !pick.is_empty(),
            pick,
            shortcut: if key.starts_with("steam:") {
                None
            } else {
                numapilot::desktop::own_shortcut(key)
            },
            present_last: numapilot::present::load_results().remove(key),
            p: Profile {
                label: label.into(),
                ..p
            },
        }
    }
}

const MEM_OPTS: &[(&str, &str, &str)] = &[
    (
        "auto",
        "Automatic",
        "numapilot decides (normally: memory of the cores' socket)",
    ),
    (
        "strict",
        "Socket only (strict)",
        "lowest latency; if the socket's memory fills up, the program suffers",
    ),
    (
        "prefer",
        "Prefer local",
        "uses the other socket's only when it runs out",
    ),
    (
        "interleave",
        "Interleave across sockets",
        "more total bandwidth; good for rendering/AI on the CPU (applies on reopen)",
    ),
    ("free", "Free", "does not move the memory"),
];

fn upscale_ui(app: &mut App, ui: &mut egui::Ui, ed: &mut Editor) {
    use numapilot::upscale;
    ui.label(RichText::new("Upscaling").strong());
    // card the game will run on → what works on it
    let gpu = app
        .topo
        .gpus
        .iter()
        .find(|g| g.alias == ed.p.gpu || g.pci == ed.p.gpu)
        .or_else(|| app.topo.gaming_gpu())
        .cloned();
    let rec = numapilot::recipes::load();
    let sug: Vec<&str> = gpu
        .as_ref()
        .and_then(|g| rec.arch_for(g))
        .map(|a| upscale::suggested(&a.upscale))
        .unwrap_or_default();
    if let (Some(g), Some(a)) = (gpu.as_ref(), gpu.as_ref().and_then(|g| rec.arch_for(g))) {
        ui.label(
            RichText::new(crate::tf!(
                "On the {} ({}), these work: {}",
                "Na {} ({}), funcionam: {}",
                g.alias,
                a.family,
                sug.join(", ")
            ))
            .small()
            .weak(),
        );
    }
    let mut u =
        ed.p.upscale
            .clone()
            .unwrap_or_else(|| numapilot::profiles::Upscale {
                mode: "nenhum".into(),
                ..Default::default()
            });
    let cur = upscale::MODES
        .iter()
        .find(|m| m.0 == u.mode)
        .map(|m| crate::i18n::tr(m.1))
        .unwrap_or(crate::i18n::tr("None"));
    egui::ComboBox::from_id_salt("ed-up")
        .selected_text(cur)
        .width(240.0)
        .show_ui(ui, |ui| {
            for (k, l, d) in upscale::MODES {
                let ok = *k == "nenhum" || sug.contains(k);
                let l = crate::i18n::tr(l);
                let label = if ok {
                    l.to_string()
                } else {
                    crate::tf!(
                        "{l} (not recommended for this card)",
                        "{l} (não indicado para esta placa)"
                    )
                };
                ui.selectable_value(&mut u.mode, k.to_string(), label)
                    .on_hover_text(crate::i18n::tr(d));
            }
        });
    if u.mode != "nenhum" {
        ui.label(
            RichText::new(
                upscale::MODES
                    .iter()
                    .find(|m| m.0 == u.mode)
                    .map(|m| crate::i18n::tr(m.2))
                    .unwrap_or(""),
            )
            .small()
            .weak(),
        );
        if u.mode != "optiscaler" {
            ui.label(
                RichText::new(crate::i18n::tr(
                    "Needs proton-cachyos or GE-Proton as the game's compatibility tool.",
                ))
                .small()
                .weak(),
            );
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut u.indicator, crate::i18n::tr("show the FSR 4/DLSS watermark (to check)"));
            if u.mode == "optiscaler" {
                ui.checkbox(&mut u.int8, crate::i18n::tr("force FSR 4 INT8 (experimental)"))
                    .on_hover_text(crate::i18n::tr("For cards without official FSR 4 (RDNA 2/3.5, Intel, NVIDIA). Cards without INT8 do not get FSR 4."));
            }
        });
    }
    if u.mode == "optiscaler" {
        ui.label(
            RichText::new(crate::i18n::tr(
                "Do not use OptiScaler in online games: the anti-cheat may ban you.",
            ))
            .small()
            .color(crate::theme::warn()),
        );
        if let Some(id) = ed.key.strip_prefix("steam:") {
            let dir = upscale::steam_game_root(id).and_then(|r| upscale::exe_dir(&r));
            ui.horizontal(|ui| {
                match dir.as_ref().and_then(|d| upscale::installed(d)) {
                    Some(i) => {
                        ui.label(RichText::new(crate::tf!("OptiScaler {} installed", "OptiScaler {} instalado", i.version)).color(crate::theme::ok()));
                        if ui.button(crate::i18n::tr("Remove from the game")).clicked() {
                            app.start_job("OptiScaler", vec![crate::jobs::sibling("numapilot"), "upscale".into(), "remove".into(), id.into()]);
                        }
                        if ui.button(crate::i18n::tr("Reinstall (new version)")).clicked() {
                            let mut v = vec![crate::jobs::sibling("numapilot"), "upscale".into(), "install".into(), id.into()];
                            if u.int8 {
                                v.push("--int8".into());
                            }
                            app.start_job("OptiScaler", v);
                        }
                    }
                    None if dir.is_some() => {
                        if ui.button(RichText::new(crate::i18n::tr("Install in the game")).strong()).on_hover_text(crate::i18n::tr("Downloads the newest stable version and copies it to the executable's folder, keeping what belongs to the game")).clicked() {
                            let mut v = vec![crate::jobs::sibling("numapilot"), "upscale".into(), "install".into(), id.into()];
                            if u.int8 {
                                v.push("--int8".into());
                            }
                            app.start_job("OptiScaler", v);
                        }
                    }
                    None => {
                        ui.label(RichText::new(crate::i18n::tr("Game folder not found in the Steam libraries.")).weak());
                    }
                }
            });
            if let Some(d) = dir {
                ui.label(
                    RichText::new(crate::tf!("Folder: {}", "Pasta: {}", d.display()))
                        .small()
                        .weak(),
                );
            }
        } else {
            ui.label(RichText::new(crate::i18n::tr("To install from the interface, open the game through Steam with numapilot (the profile is tied to the appid).")).small().weak());
        }
    }
    let new = if u.mode == "nenhum" && !u.indicator {
        None
    } else {
        Some(u)
    };
    if new != ed.p.upscale {
        ed.p.upscale = new;
    }
}

fn tuner_ui(app: &mut App, ui: &mut egui::Ui, ed: &mut Editor) {
    use numapilot::tuner::{self, Tuner};
    let mut t = Tuner::load();
    ui.label(RichText::new(crate::i18n::tr("Optimize FPS")).strong());
    let steam_id = ed.key.strip_prefix("steam:").map(String::from);
    let mut save = false;
    let mut apply_best = None;
    match t.sessions.get(&ed.key).cloned() {
        None => {
            ui.label(
                RichText::new(
                    crate::i18n::tr("Tests a few settings (cores, HT, socket, card), one each time you open the game, \
                     measures with MangoHud (average FPS and 1% low) and shows the best."),
                )
                .weak(),
            );
            if !app.has_mangohud {
                ui.label(
                    RichText::new(crate::i18n::tr("Needs MangoHud: sudo pacman -S mangohud"))
                        .color(crate::theme::warn()),
                );
            }
            if ui
                .button(crate::i18n::tr("Start the optimization"))
                .clicked()
            {
                let v = tuner::variants(&app.topo, &ed.p);
                t.start(&ed.key, &ed.label, v);
                save = true;
            }
        }
        Some(s) => {
            let best = s.best();
            egui::Grid::new("ed-tune")
                .striped(true)
                .num_columns(4)
                .spacing([12.0, 3.0])
                .show(ui, |ui| {
                    for h in [
                        "",
                        crate::i18n::tr("Setting"),
                        crate::i18n::tr("Average FPS"),
                        "1% low",
                    ] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (i, v) in s.variants.iter().enumerate() {
                        ui.label(if best == Some(i) {
                            crate::i18n::tr("best")
                        } else if i == s.next {
                            crate::i18n::tr("next")
                        } else {
                            ""
                        });
                        ui.label(&v.label);
                        match &v.result {
                            Some(r) => {
                                ui.label(format!("{:.1}", r.avg));
                                ui.label(format!("{:.1}", r.low1));
                            }
                            None => {
                                ui.label("—");
                                ui.label("—");
                            }
                        }
                        ui.end_row();
                    }
                });
            if let Some(v) = s.current() {
                ui.label(crate::tf!(
                    "Next test ({}/{}): {}. Open the game, go to a heavy scene and play for about {} s; then close it.", "Próximo teste ({}/{}): {}. Abra o jogo, vá para uma cena pesada e jogue uns {} s; depois feche.",
                    s.next + 1,
                    s.variants.len(),
                    v.label,
                    s.seconds + 10
                ));
            } else {
                ui.label(RichText::new(crate::i18n::tr("All tests finished.")).strong());
            }
            ui.horizontal(|ui| {
                if let Some(id) = &steam_id
                    && !s.done()
                    && ui.button(crate::i18n::tr("Open the game")).clicked()
                {
                    let _ = crate::jobs::spawn_detached(&[
                        "xdg-open".into(),
                        format!("steam://rungameid/{id}"),
                    ]);
                }
                if let Some(b) = best
                    && ui
                        .button(RichText::new(crate::i18n::tr("Use the best")).strong())
                        .clicked()
                {
                    apply_best = Some(s.variants[b].profile.clone());
                }
                if ui.button(crate::i18n::tr("Start over")).clicked() {
                    let v = tuner::variants(&app.topo, &ed.p);
                    t.start(&ed.key, &ed.label, v);
                    save = true;
                }
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    t.sessions.remove(&ed.key);
                    save = true;
                }
            });
            if steam_id.is_none() {
                ui.label(
                    RichText::new(crate::i18n::tr(
                        "Open it from the menu shortcut (numapilot run) for the test to count.",
                    ))
                    .small()
                    .weak(),
                );
            }
        }
    }
    if save && let Err(e) = t.save() {
        app.toast(crate::tf!("optimizer: {e}", "otimizador: {e}"), false);
    }
    if let Some(mut p) = apply_best {
        p.label = ed.label.clone();
        p.gamescope = ed.p.gamescope.clone();
        p.present = ed.p.present.clone();
        ed.p = p.clone();
        app.send(Request::SetProfile {
            key: ed.key.clone(),
            profile: p,
            pid: ed.main_pid,
        });
    }
}

/// "Render on one card, improve on the other": output card, upscaling model, frame generation,
/// minimum base FPS, and the last real × shown measurement.
fn present_ui(app: &mut App, ui: &mut egui::Ui, ed: &mut Editor) {
    use numapilot::present::{ENGINES, UPSCALERS};
    let mut on = ed.p.present.is_some();
    ui.horizontal(|ui| {
        ui.checkbox(
            &mut on,
            RichText::new(crate::i18n::tr("Presentation: upscaling and frame generation")).strong(),
        )
        .on_hover_text(crate::i18n::tr(
            "The game renders on the card above; the output card takes the finished image (one copy over PCIe), \
             upscales it, generates frames and shows it through its own video output. Opens inside gamescope.",
        ));
        if !app.has_gamescope {
            ui.label(
                RichText::new(crate::i18n::tr("(not installed: sudo pacman -S gamescope)"))
                    .small()
                    .color(crate::theme::warn()),
            );
        }
    });
    if on != ed.p.present.is_some() {
        ed.p.present = on.then(numapilot::profiles::Present::default);
        // the block runs on gamescope: its card choice (output card) lives in the gamescope block
        if on && ed.p.gamescope.is_none() {
            ed.p.gamescope = Some(numapilot::profiles::Gamescope::default());
        }
    }
    let game_gpu = app
        .topo
        .gpus
        .iter()
        .find(|g| g.matches(&ed.p.gpu))
        .or_else(|| app.topo.gaming_gpu())
        .cloned();
    let gs = ed.p.gamescope.clone().unwrap_or_default();
    // output card as `decide::resolve_out_gpu` picks it: "display" = the first card with a monitor
    let out_gpu = gs
        .output_gpu
        .as_deref()
        .and_then(|n| match n {
            "" | "same" | "auto" => None,
            "display" => app.topo.gpus.iter().find(|g| g.display_connected),
            n => app.topo.gpus.iter().find(|g| g.matches(n)),
        })
        .filter(|o| game_gpu.as_ref().is_none_or(|g| g.pci != o.pci))
        .cloned();
    let Some(pr) = ed.p.present.as_mut() else {
        return;
    };
    egui::Grid::new("ed-present")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.label(crate::i18n::tr("Output card"));
            ui.label(
                RichText::new(match &out_gpu {
                    Some(o) => o.alias.clone(),
                    None => crate::i18n::tr("the game's card").to_string(),
                })
                .strong(),
            )
            .on_hover_text(crate::i18n::tr(
                "Chosen in the gamescope block above (\"gamescope card\").",
            ));
            ui.end_row();

            ui.label(crate::i18n::tr("Upscaling model"));
            let name = |v: &str| {
                UPSCALERS
                    .iter()
                    .find(|u| u.0 == v)
                    .map(|u| crate::i18n::tr(u.1))
                    .unwrap_or("?")
            };
            egui::ComboBox::from_id_salt("ed-pr-up")
                .selected_text(name(&pr.upscaler))
                .show_ui(ui, |ui| {
                    for (v, l, ready) in UPSCALERS {
                        ui.add_enabled_ui(ready || pr.upscaler == v, |ui| {
                            ui.selectable_value(
                                &mut pr.upscaler,
                                v.to_string(),
                                crate::i18n::tr(l),
                            );
                        });
                    }
                });
            ui.end_row();

            ui.label(crate::i18n::tr("Frame generation"));
            ui.horizontal(|ui| {
                let mults = [
                    (1u8, numapilot::t2!("off", "desligada")),
                    (2, "2×"),
                    (3, "3×"),
                    (4, "4×"),
                ];
                let cur = mults
                    .iter()
                    .find(|m| m.0 == pr.multiplier)
                    .map(|m| m.1)
                    .unwrap_or("?");
                egui::ComboBox::from_id_salt("ed-pr-mult")
                    .selected_text(cur)
                    .show_ui(ui, |ui| {
                        for (k, l) in mults {
                            ui.selectable_value(&mut pr.multiplier, k, l);
                        }
                    });
                ui.add_enabled_ui(pr.multiplier > 1, |ui| {
                    let ename = ENGINES
                        .iter()
                        .find(|e| e.0 == pr.engine)
                        .map(|e| crate::i18n::tr(e.1))
                        .unwrap_or("?");
                    egui::ComboBox::from_id_salt("ed-pr-eng")
                        .selected_text(ename)
                        .show_ui(ui, |ui| {
                            for (v, l, ready) in ENGINES {
                                ui.add_enabled_ui(ready || pr.engine == v, |ui| {
                                    ui.selectable_value(
                                        &mut pr.engine,
                                        v.to_string(),
                                        crate::i18n::tr(l),
                                    );
                                });
                            }
                        });
                });
            });
            ui.end_row();

            ui.label(crate::i18n::tr("Minimum base FPS"));
            ui.horizontal(|ui| {
                ui.add_enabled(
                    pr.multiplier > 1,
                    egui::DragValue::new(&mut pr.min_fps)
                        .range(10..=240)
                        .suffix(" FPS"),
                );
                ui.label(
                    RichText::new(crate::i18n::tr(
                        "under this the generation turns off (it looks bad below ~40)",
                    ))
                    .small()
                    .weak(),
                );
            });
            ui.end_row();

            ui.label("");
            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut pr.measure,
                    crate::i18n::tr("Measure real × shown FPS (MangoHud)"),
                );
                if pr.measure && !app.has_mangohud {
                    ui.label(
                        RichText::new(crate::i18n::tr("Needs MangoHud: sudo pacman -S mangohud"))
                            .small()
                            .color(crate::theme::warn()),
                    );
                }
            });
            ui.end_row();
        });
    let want_lsfg = pr.multiplier > 1 && pr.engine == "lsfg";
    // What the launch will do on this machine (same plan as numapilot run).
    // same status the lsfg-vk panel shows (read in a thread, every 5 s)
    let lsfg = if want_lsfg {
        crate::bg::cached(
            "lsfg-status",
            std::time::Duration::from_secs(5),
            ui.ctx(),
            numapilot::lsfg::status,
        )
        .map(|s| numapilot::present::Lsfg::of(&s))
        .unwrap_or(numapilot::present::Lsfg::Ready)
    } else {
        numapilot::present::Lsfg::Ready
    };
    let plan = numapilot::present::plan(pr, &gs, out_gpu.as_ref(), lsfg);
    let route = match (&game_gpu, &plan.out_pci) {
        (Some(g), Some(o)) => {
            let out = app
                .topo
                .gpus
                .iter()
                .find(|x| x.pci == *o)
                .map(|x| x.alias.clone())
                .unwrap_or_else(|| o.clone());
            crate::tf!(
                "Renders on the {} → improves and shows on the {}",
                "Renderiza na {} → melhora e mostra na {}",
                g.alias,
                out
            )
        }
        (Some(g), None) => crate::tf!(
            "Renders, improves and shows on the {}",
            "Renderiza, melhora e mostra na {}",
            g.alias
        ),
        _ => crate::i18n::tr("Renders, improves and shows on the same card").to_string(),
    };
    ui.label(RichText::new(route).small());
    for w in &plan.warnings {
        ui.label(
            RichText::new(crate::tf!("Warning: {w}", "Atenção: {w}"))
                .small()
                .color(crate::theme::warn()),
        );
    }
    if want_lsfg {
        let fg_card = out_gpu
            .as_ref()
            .or(game_gpu.as_ref())
            .map(|g| g.alias.clone())
            .unwrap_or_else(|| "?".into());
        ui.label(
            RichText::new(crate::tf!(
                "Frame generation runs on the {} (the output card); the \"Lossless Scaling\" choice under Graphics card does not apply with this block on.",
                "A geração de quadros roda na {} (a placa de saída); a escolha \"Lossless Scaling\" em Placa de vídeo não vale com este bloco ligado.",
                fg_card
            ))
            .small(),
        );
        ui.label(
            RichText::new(crate::i18n::tr(
                "Lossless Scaling must be bought and installed through Steam first (Properties → Betas → \"lsfg-vk\"): lsfg-vk only uses its files.",
            ))
            .small()
            .color(crate::theme::warn()),
        );
        crate::lsfg::panel(app, ui, "editor", false);
    }
    match &ed.present_last {
        Some(m) => {
            ui.label(
                RichText::new(crate::tf!(
                    "Last session: {}",
                    "Última sessão: {}",
                    numapilot::present::summary(m)
                ))
                .strong(),
            );
            if !m.shown_measured {
                ui.label(
                    RichText::new(crate::i18n::tr(
                        "Real FPS is counted by MangoHud in the game; shown FPS is the real one times the \
                         multiplier while the base FPS stays above the minimum.",
                    ))
                    .small()
                    .weak(),
                );
            }
        }
        None => {
            ui.label(
                RichText::new(crate::i18n::tr(
                    "No measurement yet: open the game through numapilot run (menu shortcut or Steam).",
                ))
                .small()
                .weak(),
            );
        }
    }
}

/// Row that opens a group of settings in the editor's table: its name in small capitals and a
/// rule across the second column.
fn group_row(ui: &mut egui::Ui, name: &str) {
    let p = crate::theme::pal(ui);
    ui.label(
        RichText::new(name.to_uppercase())
            .size(10.5)
            .family(egui::FontFamily::Name(crate::theme::BOLD.into()))
            .color(p.accent)
            .extra_letter_spacing(0.8),
    );
    let (r, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(60.0), 14.0),
        egui::Sense::hover(),
    );
    ui.painter()
        .hline(r.x_range(), r.center().y, egui::Stroke::new(1.0, p.stroke));
    ui.end_row();
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut ed) = app.editor.take() else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut fix = false;
    let mut steam_line: Option<Option<String>> = None;
    let mut shortcut_op: Option<bool> = None;
    egui::Window::new(crate::tf!("Adjust: {}", "Ajustar: {}", ed.label))
        .id(egui::Id::unique("numapilot-editor"))
        .open(&mut open)
        .default_size([820.0, 640.0])
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(RichText::new(&ed.key).small().weak());
                // Current situation
                let tot: u64 = ed.mem.iter().sum();
                let cur = parse_cpulist(&ed.current_cpus);
                let local_nodes = app.topo.nodes_of(&cur);
                let local: u64 = ed.mem.iter().enumerate().filter(|(n, _)| local_nodes.contains(n)).map(|(_, v)| *v).sum();
                ui.label(crate::tf!(
                    "Now: {} CPUs ({}) · memory {:.1} GB{}", "Agora: {} CPUs ({}) · memória {:.1} GB{}",
                    cur.len(),
                    if ed.current_cpus.is_empty() { crate::i18n::tr("all").to_string() } else { ed.current_cpus.clone() },
                    tot as f64 / 1024.0,
                    if tot > 0 { crate::tf!(", {:.0}% on the cores' socket", ", {:.0}% no socket dos núcleos", local as f64 * 100.0 / tot as f64) } else { String::new() }
                ));
                ui.add_space(4.0);

                // History: how much the program used over the last hours (the service keeps it)
                {
                    let id = ui.make_persistent_id(("prog-hist", &ed.key));
                    let pts: Vec<numapilot::proghist::Point> = ui.data_mut(|d| {
                        d.get_temp_mut_or_insert_with(id, || numapilot::proghist::load(&ed.key))
                            .clone()
                    });
                    if let Some(s) = numapilot::proghist::summary(&pts) {
                        let p = crate::theme::pal(ui);
                        ui.horizontal(|ui| {
                            let mut sp = crate::theme::Spark::default();
                            let peak = s.peak.max(0.1);
                            let step = (pts.len() as f32 / crate::theme::Spark::LEN as f32).max(1.0);
                            let mut i = 0.0f32;
                            while (i as usize) < pts.len() {
                                sp.push(pts[i as usize].peak / peak);
                                i += step;
                            }
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(140.0, 30.0), egui::Sense::hover());
                            crate::theme::spark_in(ui.painter(), rect, &sp, p.accent);
                            let h = s.minutes / 60;
                            let m = s.minutes % 60;
                            ui.label(
                                RichText::new(crate::tf!(
                                    "Recorded usage ({}{m} min): {:.1} cores on average, peak {:.1}; memory at peak {}", "Uso registrado ({}{m} min): em média {:.1} núcleos, pico {:.1}; memória no pico {}",
                                    if h > 0 { format!("{h} h ") } else { String::new() },
                                    s.avg,
                                    s.peak,
                                    crate::fmt_bytes(s.mem_peak_mb as u64 * 1048576)
                                ))
                                .small(),
                            )
                            .on_hover_text(crate::i18n::tr("From the last 6 hours the program was open, one point per minute. Use it to pick the memory and processor limits with headroom over the peak."));
                        });
                        ui.add_space(4.0);
                    }
                }
                // Ready-made profiles
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Ready-made profiles:")).strong());
                    for (id, name, tip) in [
                        ("jogo", crate::i18n::tr("Game"), crate::i18n::tr("GPU's socket, 16 physical cores, local memory only, high priority, reserved cores, the game sees only those cores")),
                        ("ia", crate::i18n::tr("AI"), crate::i18n::tr("compute GPU's socket, memory preferring local")),
                        ("render", crate::i18n::tr("Rendering"), crate::i18n::tr("both sockets, interleaved memory (maximum bandwidth)")),
                        ("turbo", crate::i18n::tr("Maximum turbo"), crate::i18n::tr("4 physical cores alone on one processor: the rest of the system moves to the other socket and the clock rises to the highest turbo step (needs 2 processors; check Map → Turbo)")),
                        ("video", crate::i18n::tr("Video editing"), crate::i18n::tr("the GPU's whole socket, with both threads of each core, memory preferring local")),
                        ("fundo", crate::i18n::tr("Background"), crate::i18n::tr("at most one processor core and low priority: for launchers, sync tools and the like")),
                        ("silencioso", crate::i18n::tr("Quiet"), crate::i18n::tr("4 cores and low priority: stays out of the way")),
                    ] {
                        if ui.button(name).on_hover_text(tip).clicked() {
                            ed.p = Profile::preset(id, &ed.p);
                            ed.picking = false;
                            ed.pick.clear();
                        }
                    }
                    if ui.button(crate::i18n::tr("All automatic")).clicked() {
                        ed.p = Profile { label: ed.label.clone(), ..Default::default() };
                        ed.picking = false;
                        ed.pick.clear();
                    }
                });
                ui.separator();

                egui::Grid::new("ed").num_columns(2).spacing([18.0, 10.0]).show(ui, |ui| {
                    group_row(ui, crate::i18n::tr("Processor"));
                    ui.label(RichText::new(crate::i18n::tr("Processors")).strong());
                    let txt = app.cpu_opts.iter().find(|(v, _)| *v == ed.p.cpu).map(|(_, t)| t.clone()).unwrap_or(ed.p.cpu.clone());
                    ui.add_enabled_ui(!ed.picking, |ui| {
                        egui::ComboBox::from_id_salt("ed-cpu").selected_text(txt).width(240.0).show_ui(ui, |ui| {
                            for (v, t) in &app.cpu_opts {
                                ui.selectable_value(&mut ed.p.cpu, v.clone(), t);
                            }
                        });
                    });
                    ui.end_row();

                    ui.label(RichText::new(crate::i18n::tr("Cores")).strong());
                    ui.vertical(|ui| {
                        ui.add_enabled_ui(!ed.picking, |ui| {
                            ui.horizontal(|ui| {
                                let mut lim = ed.p.cores.is_some();
                                if ui.checkbox(&mut lim, crate::i18n::tr("limit to")).changed() {
                                    // "4" should mean 4 physical cores: hyper-threading starts off
                                    // (with it on, 4 CPUs are 2 cores with both threads, half the speed)
                                    ed.p.cores = if lim { Some(4) } else { None };
                                    if lim {
                                        ed.p.smt = false;
                                    }
                                }
                                if let Some(c) = ed.p.cores.as_mut() {
                                    ui.add(egui::Slider::new(c, 1..=app.topo.all_cpus().len()).suffix(" CPUs"));
                                    let phys = if ed.p.smt { c.div_ceil(2) } else { *c };
                                    ui.label(
                                        RichText::new(if ed.p.smt {
                                            crate::tf!("= {phys} physical cores × 2 threads", "= {phys} núcleos físicos × 2 threads")
                                        } else {
                                            crate::tf!("= {phys} physical cores", "= {phys} núcleos físicos")
                                        })
                                        .small()
                                        .weak(),
                                    );
                                }
                            });
                            ui.checkbox(&mut ed.p.smt, crate::i18n::tr("use hyper-threading (2 threads per core)"))
                                .on_hover_text(crate::i18n::tr("Off: 1 thread per physical core. In games it usually gives more FPS and less stutter."));
                        });
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut ed.picking, crate::i18n::tr("pick the cores on the map")).changed() && ed.picking && ed.pick.is_empty() {
                                ed.pick.extend(cur.iter().copied());
                            }
                            if ed.picking {
                                ui.label(RichText::new(crate::tf!("{} CPUs selected", "{} CPUs selecionados", ed.pick.len())).weak());
                                if crate::theme::small(ui, crate::i18n::tr("clear")).clicked() {
                                    ed.pick.clear();
                                }
                            }
                        });
                    });
                    ui.end_row();

                    ui.label(RichText::new(crate::i18n::tr("The program sees")).strong());
                    ui.checkbox(&mut ed.p.limit_visible, crate::i18n::tr("only the allowed cores"))
                        .on_hover_text(
                            crate::i18n::tr("Proton/Wine games (WINE_CPU_TOPOLOGY), OpenMP, MKL, OpenBLAS, .NET, Java, Go...\n\
                             Keeps games and Easy Anti-Cheat from refusing to start for seeing too many cores. Applies on start.\n\
                             With strict memory on one socket, also a memory limit the size of that socket\n\
                             (programs that read their control group count only that RAM)."),
                        );
                    ui.end_row();

                    ui.label(RichText::new(crate::i18n::tr("Processor limit")).strong());
                    ui.horizontal(|ui| {
                        let mut lim = ed.p.cpu_max_pct.is_some();
                        if ui
                            .checkbox(&mut lim, crate::i18n::tr("Use at most"))
                            .on_hover_text(crate::i18n::tr("Usage cap adding up all cores: 50% = half a core, 200% = two cores. Good for background programs."))
                            .changed()
                        {
                            ed.p.cpu_max_pct = lim.then_some(100.0);
                        }
                        if let Some(c) = ed.p.cpu_max_pct.as_mut() {
                            ui.add(
                                egui::Slider::new(c, 5.0..=1600.0)
                                    .logarithmic(true)
                                    .suffix("%")
                                    .fixed_decimals(0),
                            );
                            ui.label(RichText::new(crate::tf!("= {:.1} core(s)", "= {:.1} núcleo(s)", *c / 100.0)).small().weak());
                            if numapilot::machine::sched_ext_active() {
                                let n = ((*c / 100.0).ceil() as usize).max(1);
                                ui.label(RichText::new(crate::tf!("applied as {n} core(s)", "aplicado como {n} núcleo(s)")).small().color(crate::theme::warn()))
                                    .on_hover_text(crate::i18n::tr("With a sched_ext scheduler running (Machine → Scheduler, e.g. LAVD) the kernel does not enforce a percentage limit, so numapilot limits the number of cores instead, rounded up."));
                            }
                        }
                    });
                    ui.end_row();

                    group_row(ui, crate::i18n::tr("Memory"));
                    ui.label(RichText::new(crate::i18n::tr("Memory")).strong());
                    let multi = app.topo.nodes.len() > 1;
                    ui.vertical(|ui| {
                        // With a single processor there is no "other socket's" memory.
                        for (v, t, d) in MEM_OPTS.iter().filter(|_| multi) {
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut ed.p.memory, v.to_string(), crate::i18n::tr(t));
                                ui.label(RichText::new(crate::i18n::tr(d)).small().weak());
                            });
                        }
                        if !multi {
                            ui.label(
                                RichText::new(crate::i18n::tr("This PC has a single processor: all memory is already the closest to it (the local memory options show up on PCs with 2 processors)."))
                                    .small()
                                    .weak(),
                            );
                        }
                        ui.horizontal(|ui| {
                            let mut lim = ed.p.mem_max_gb.is_some();
                            if ui
                                .checkbox(&mut lim, crate::i18n::tr("Limit the program's memory to"))
                                .on_hover_text(crate::i18n::tr("Past the limit, the system takes memory away from it (to swap) instead of freezing the whole PC."))
                                .changed()
                            {
                                ed.p.mem_max_gb = if lim { Some(16.0) } else { None };
                            }
                            let nodes_kb = app.topo.nodes.iter().map(|n| n.mem_total_kb).sum::<u64>();
                            let total_kb = if nodes_kb > 0 { nodes_kb } else { crate::mem_system().1 };
                            let max_gb = (total_kb as f64 / 1048576.0).max(1.0);
                            if let Some(g) = ed.p.mem_max_gb.as_mut() {
                                *g = g.min(max_gb);
                            }
                            let now = app.prog_mem.get(&ed.key).copied().unwrap_or(0);
                            if let Some(g) = ed.p.mem_max_gb.as_mut() {
                                ui.add(egui::Slider::new(g, 0.5..=max_gb).suffix(" GB").step_by(0.5));
                            }
                            if now > 0 {
                                ui.label(RichText::new(crate::tf!("now using {}", "agora usa {}", crate::fmt_bytes(now))).small().weak());
                            }
                        });
                    });
                    ui.end_row();

                    group_row(ui, crate::i18n::tr("Priority and clock"));
                    ui.label(RichText::new(crate::i18n::tr("Priority")).strong());
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut ed.p.priority, "low".to_string(), crate::i18n::tr("low"));
                        ui.radio_value(&mut ed.p.priority, "normal".to_string(), "normal");
                        ui.radio_value(&mut ed.p.priority, "high".to_string(), crate::i18n::tr("high"));
                        ui.separator();
                        ui.checkbox(&mut ed.p.exclusive, crate::i18n::tr("exclusive cores"))
                            .on_hover_text(crate::i18n::tr("While it runs, the rest of the system does not use these cores."));
                        if multi {
                            ui.checkbox(&mut ed.p.turbo_max, crate::i18n::tr("maximum turbo")).on_hover_text(
                                crate::i18n::tr("While it runs, the rest of the system leaves the whole processor it is on. \
                                 With the other cores asleep, its cores clock higher (see the table in Map → Turbo). \
                                 Works best with few cores (2 to 8) and the deep idle states enabled."),
                            );
                        }
                        ui.checkbox(&mut ed.p.low_latency, crate::i18n::tr("low latency")).on_hover_text(
                            crate::i18n::tr("While it runs: its cores do not enter the deep idle states, run at full clock (performance governor) and the socket's uncore \
                             stays at maximum (less micro-stutter). Tune it in Machine → Gaming latency."),
                        );
                    });
                    ui.end_row();

                    if multi && ed.p.turbo_max {
                        ui.label(RichText::new(crate::i18n::tr("Cores at turbo")).strong());
                        ui.vertical(|ui| {
                            let per_socket = app
                                .topo
                                .nodes
                                .iter()
                                .map(|n| n.physical_cores.max(n.cores.len()))
                                .min()
                                .unwrap_or(0);
                            let table = app.turbo_tables.values().next();
                            let ghz = |n: usize| {
                                table
                                    .and_then(|t| numapilot::turbo::limit_for(t, n as f32))
                                    .map(|m| crate::tf!(" · {:.1} GHz", " · {:.1} GHz", m as f64 / 1000.0))
                                    .unwrap_or_default()
                            };
                            // chosen physical cores (the profile counts threads when it uses both)
                            let phys = ed.p.cores.map(|c| if ed.p.smt { c / 2 } else { c });
                            ui.horizontal_wrapped(|ui| {
                                for n in [2usize, 4, 6, 8, 10, 12, 16] {
                                    if n > per_socket {
                                        continue;
                                    }
                                    let sel = phys == Some(n);
                                    if ui
                                        .selectable_label(sel, format!("{n}{}", ghz(n)))
                                        .on_hover_text(crate::tf!(
                                            "{n} physical cores (no core sharing) alone on the processor", "{n} núcleos físicos (sem dividir núcleo) sozinhos no processador"
                                        ))
                                        .clicked()
                                    {
                                        // with both threads, the profile's count is in threads
                                        ed.p.cores = Some(if ed.p.smt { n * 2 } else { n });
                                        ed.p.cpu_list = None;
                                        ed.picking = false;
                                        ed.pick.clear();
                                    }
                                }
                            });
                            let mut ht = ed.p.smt;
                            let label = match phys {
                                Some(n) => crate::tf!(
                                    "use both threads of each core ({n} cores, {} threads)", "usar os dois threads de cada núcleo ({n} núcleos, {} threads)",
                                    if ht { n * 2 } else { n }
                                ),
                                None => crate::i18n::tr("use both threads of each core").to_string(),
                            };
                            if ui
                                .checkbox(&mut ht, label)
                                .on_hover_text(
                                    crate::i18n::tr("Turbo counts awake cores, not threads: the maximum clock is the same either way. \
                                     Unchecked, each program thread gets a whole core (better for most games). \
                                     Checked, it does better in programs that use every thread (compiling, rendering)."),
                                )
                                .changed()
                            {
                                ed.p.smt = ht;
                                if let Some(n) = phys {
                                    ed.p.cores = Some(if ht { n * 2 } else { n });
                                }
                            }
                            let all = ghz(per_socket);
                            ui.label(
                                RichText::new(if table.is_some() {
                                    crate::tf!(
                                        "Maximum clock with this many cores awake (all {per_socket} awake{all}). \
                                         Games that use many cores do better with 6 or 8 than with 4.", "Clock máximo com esse número de núcleos acordados (todos os {per_socket} ligados{all}). \
                                         Jogos que usam muitos núcleos rendem mais com 6 ou 8 do que com 4."
                                    )
                                } else {
                                    crate::i18n::tr("The fewer cores, the higher the clock. This processor's table shows up with the service running (Intel only).").to_string()
                                })
                                .small()
                                .weak(),
                            );
                        });
                        ui.end_row();
                    }

                    ui.label(RichText::new(crate::i18n::tr("Machine mode")).strong())
                        .on_hover_text(crate::i18n::tr("While the program is open the machine stays in this mode (Machine tab) and goes back to the previous one when it closes."));
                    let modes = [(None, crate::i18n::tr("do not switch")), (Some("jogo"), crate::i18n::tr("Game")), (Some("ia"), crate::i18n::tr("AI")), (Some("silencioso"), crate::i18n::tr("Quiet"))];
                    let cur = modes.iter().find(|m| m.0 == ed.p.machine_mode.as_deref()).map(|m| m.1).unwrap_or("?");
                    egui::ComboBox::from_id_salt("ed-mmode").selected_text(cur).show_ui(ui, |ui| {
                        for (k, l) in modes {
                            ui.selectable_value(&mut ed.p.machine_mode, k.map(String::from), l);
                        }
                    });
                    ui.end_row();

                    group_row(ui, crate::i18n::tr("Graphics card"));
                    ui.label(RichText::new(crate::i18n::tr("Card")).strong());
                    ui.vertical(|ui| {
                        let cur = (ed.p.gpu.clone(), ed.p.usage.clone());
                        let mut sel = cur.clone();
                        let gtxt = app
                            .gpu_opts
                            .iter()
                            .find(|(g, u, _)| *g == cur.0 && (*u == cur.1 || cur.0 == "auto" || cur.0 == "none"))
                            .map(|(_, _, t)| t.clone())
                            .unwrap_or_else(|| format!("{} ({})", cur.0, cur.1));
                        egui::ComboBox::from_id_salt("ed-gpu").selected_text(gtxt).width(300.0).show_ui(ui, |ui| {
                            for (g, u, t) in &app.gpu_opts {
                                ui.selectable_value(&mut sel, (g.clone(), u.clone()), t);
                            }
                        });
                        if sel != cur {
                            ed.p.gpu = sel.0;
                            ed.p.usage = sel.1;
                        }
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(crate::i18n::tr("Card power profile:")).small());
                            let names = [
                                ("auto", crate::i18n::tr("leave as is")),
                                ("3d", crate::i18n::tr("3D fullscreen (game)")),
                                ("compute", crate::i18n::tr("compute (AI/ROCm)")),
                                ("vr", "VR"),
                                ("video", crate::i18n::tr("video")),
                                (crate::i18n::tr("power saving"), crate::i18n::tr("power saving")),
                            ];
                            let cur = names.iter().find(|n| n.0 == ed.p.gpu_power).map(|n| n.1).unwrap_or("?");
                            egui::ComboBox::from_id_salt("ed-gpow").selected_text(cur).show_ui(ui, |ui| {
                                for (k, l) in names {
                                    ui.selectable_value(&mut ed.p.gpu_power, k.to_string(), l);
                                }
                            });
                        });
                        let by_present = ed.p.present.as_ref().is_some_and(|pr| pr.multiplier > 1 && pr.engine == "lsfg");
                        ui.add_enabled_ui(!by_present, |ui| ui.horizontal(|ui| {
                            ui.label(RichText::new("Lossless Scaling:").small());
                            egui::ComboBox::from_id_salt("ed-fg").selected_text(if ed.p.fg_gpu == "same" { crate::i18n::tr("same GPU").to_string() } else { ed.p.fg_gpu.clone() }).show_ui(ui, |ui| {
                                ui.selectable_value(&mut ed.p.fg_gpu, "same".to_string(), crate::i18n::tr("same GPU as the game"));
                                for g in &app.topo.gpus {
                                    ui.selectable_value(&mut ed.p.fg_gpu, g.alias.clone(), format!("{} (dual GPU, fork)", g.alias));
                                }
                            });
                        }).response.on_disabled_hover_text(crate::i18n::tr("Set by the Presentation block: frame generation runs on its output card.")));
                    });
                    ui.end_row();
                });

                if ed.picking {
                    ui.separator();
                    ui.label(RichText::new(crate::i18n::tr("Click a core to use both threads; right-click = only 1 thread.")).weak());
                    let who = BTreeMap::new();
                    core_map(ui, &app.topo, &app.cpu_loads, Some(&mut ed.pick), &who);
                    ed.p.cpu_list = if ed.pick.is_empty() { None } else { Some(format_cpulist(&ed.pick.iter().copied().collect::<Vec<_>>())) };
                } else {
                    ed.p.cpu_list = None;
                }

                // gamescope
                ui.separator();
                let mut gs_on = ed.p.gamescope.is_some();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut gs_on, RichText::new(crate::i18n::tr("Open inside gamescope")).strong());
                    if !app.has_gamescope {
                        ui.label(RichText::new(crate::i18n::tr("(not installed: sudo pacman -S gamescope)")).small().color(crate::theme::warn()));
                    }
                });
                if gs_on != ed.p.gamescope.is_some() {
                    ed.p.gamescope = gs_on.then(numapilot::profiles::Gamescope::default);
                }
                if let Some(g) = ed.p.gamescope.as_mut() {
                    let opt_num = |ui: &mut egui::Ui, v: &mut Option<u32>, def: u32, max: u32| {
                        let mut on = v.is_some();
                        let mut x = v.unwrap_or(def);
                        ui.checkbox(&mut on, "");
                        ui.add_enabled(on, egui::DragValue::new(&mut x).range(1..=max));
                        *v = on.then_some(x);
                    };
                    egui::Grid::new("ed-gs").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                        ui.label(crate::i18n::tr("Game resolution"));
                        ui.horizontal(|ui| {
                            opt_num(ui, &mut g.render_w, 1920, 7680);
                            ui.label("x");
                            opt_num(ui, &mut g.render_h, 1080, 4320);
                            ui.label(RichText::new(crate::i18n::tr("unchecked = the display's")).small().weak());
                        });
                        ui.end_row();
                        ui.label(crate::i18n::tr("Output resolution"));
                        ui.horizontal(|ui| {
                            opt_num(ui, &mut g.out_w, 2560, 7680);
                            ui.label("x");
                            opt_num(ui, &mut g.out_h, 1440, 4320);
                        });
                        ui.end_row();
                        ui.label(crate::i18n::tr("FPS limit"));
                        ui.horizontal(|ui| opt_num(ui, &mut g.fps_limit, 60, 1000));
                        ui.end_row();
                        ui.label("");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut g.fullscreen, crate::i18n::tr("Fullscreen"));
                            ui.checkbox(&mut g.fsr, crate::i18n::tr("Upscale with FSR"));
                        });
                        ui.end_row();
                        if app.topo.gpus.len() > 1 {
                            ui.label(crate::i18n::tr("gamescope card"));
                            let shown = match g.output_gpu.as_deref() {
                                None => crate::i18n::tr("same as the game").to_string(),
                                Some("display") => crate::i18n::tr("the one with the monitor").to_string(),
                                Some(a) => app.topo.gpus.iter().find(|x| x.alias == a).map(|x| crate::gpu_place(x, app.topo.gpus.len() > 1)).unwrap_or_else(|| a.to_string()),
                            };
                            egui::ComboBox::from_id_salt("ed-gs-out").selected_text(shown).show_ui(ui, |ui| {
                                ui.selectable_value(&mut g.output_gpu, None, crate::i18n::tr("same as the game"));
                                ui.selectable_value(&mut g.output_gpu, Some("display".to_string()), crate::i18n::tr("the one with the monitor"));
                                for x in &app.topo.gpus {
                                    ui.selectable_value(&mut g.output_gpu, Some(x.alias.clone()), crate::gpu_place(x, app.topo.gpus.len() > 1));
                                }
                            });
                            ui.end_row();
                        }
                    });
                    ui.label(
                        RichText::new(if g.output_gpu.is_none() {
                            crate::i18n::tr("gamescope uses the same card chosen above. Good for running the game at a lower resolution and upscaling with FSR, or capping the FPS.")
                        } else {
                            crate::i18n::tr("The game renders on the card chosen above and gamescope (upscaling, FPS limit, image on the screen) runs on the other one: the frame crosses PCIe once. Cards with the same model are told apart by the PCI address.")
                        })
                        .small()
                        .weak(),
                    );
                }

                // Presentation: another card shows the image (upscaling, frame generation)
                ui.separator();
                present_ui(app, ui, &mut ed);

                // Upscaling
                ui.separator();
                upscale_ui(app, ui, &mut ed);

                // FPS optimizer
                ui.separator();
                tuner_ui(app, ui, &mut ed);

                // Preview: what will happen on start (same logic as numapilot run).
                ui.separator();
                let mut profs = Profiles::default();
                profs.map.insert(ed.key.clone(), ed.p.clone());
                let idents = vec![ed.label.clone()];
                let inp = LaunchInput { idents: &idents, learn_key: "", key: &ed.key, cpu: None, gpu: None, mem: None, usage: None, fg_gpu: None };
                match decide_launch(&app.cfg, &app.topo, &Learned::default(), &profs, &app.loads, &inp) {
                    Ok(d) => {
                        ui.label(RichText::new(crate::tf!(
                            "Will use {} CPUs ({}) on socket(s) {:?} · GPU {} · memory {}", "Vai usar {} CPUs ({}) no(s) socket(s) {:?} · GPU {} · memória {}",
                            d.cpus.len(),
                            format_cpulist(&d.cpus),
                            d.nodes,
                            d.gpus.first().map(|g| g.alias.as_str()).unwrap_or(crate::i18n::tr("none")),
                            match d.mem {
                                numapilot::config::MemSpec::Bind => crate::i18n::tr("socket only"),
                                numapilot::config::MemSpec::Interleave => "intercalada",
                                numapilot::config::MemSpec::Local => "local",
                            }
                        )).strong());
                        for w in game_warnings(&app.topo, &d) {
                            ui.label(RichText::new(crate::tf!("Warning: {w}", "Atenção: {w}")).color(crate::theme::warn()));
                        }
                    }
                    Err(e) => {
                        ui.label(RichText::new(crate::tf!("Invalid combination: {e}", "Combinação inválida: {e}")).color(crate::theme::err()));
                    }
                }
                ui.label(
                    RichText::new(
                        crate::i18n::tr("Apply right away: processors, cores, memory (except interleave), limit, priority and exclusive. \
                         Apply on reopen: graphics card, interleave and what the program sees."),
                    )
                    .small()
                    .weak(),
                );
                if ed.direct && ed.p.gpu != "auto" {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(if ed.key.starts_with("steam:") {
                            crate::i18n::tr(
                                "This game was opened without numapilot: the graphics card chosen here does not reach it. \
                                 Put the line from \"Copy the line for Steam\" in its launch options.",
                            )
                        } else {
                            crate::i18n::tr(
                                "This program was opened directly, not through numapilot: the graphics card chosen here does not reach it \
                                 (it goes to the system's default card). Open it from the menu shortcut numapilot makes.",
                            )
                        })
                        .color(crate::theme::warn()),
                    );
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new(crate::i18n::tr("Apply")).strong()).clicked() {
                        apply = true;
                    }
                    if ui.button(crate::i18n::tr("Bring the memory to the right socket")).clicked() {
                        fix = true;
                    }
                    // Steam game: the card only follows the profile when the game is opened
                    // through `numapilot run`, which goes in the game's launch options
                    if let Some(id) = ed.key.strip_prefix("steam:")
                        && ui
                            .button(crate::i18n::tr("Copy the line for Steam"))
                            .on_hover_text(crate::i18n::tr(
                                "Copies the launch options for this game with the card chosen here. Paste in Steam → game Properties → General → Launch options (what the game already has there is kept).",
                            ))
                            .clicked()
                    {
                        let gpu = app.topo.gpus.iter().find(|g| g.matches(&ed.p.gpu)).map(|g| g.alias.clone());
                        steam_line = Some(numapilot::steam::command_for_game(id, gpu.as_deref()));
                    }
                    // Anything else: a menu shortcut that opens it through `numapilot run`
                    if !ed.key.starts_with("steam:") {
                        if let Some(path) = ed.shortcut.clone() {
                            if ui
                                .button(crate::i18n::tr("Remove the menu shortcut"))
                                .on_hover_text(path.display().to_string())
                                .clicked()
                            {
                                shortcut_op = Some(false);
                            }
                        } else {
                            let cmd = numapilot::desktop::launch_command(&ed.key, ed.main_pid);
                            let b = ui
                                .add_enabled(cmd.is_some(), egui::Button::new(crate::i18n::tr("Create a menu shortcut")))
                                .on_hover_text(crate::i18n::tr(
                                    "Creates a shortcut in the menu that opens this program through numapilot, so the graphics card and everything in this adjustment apply from the start.",
                                ))
                                .on_disabled_hover_text(crate::i18n::tr(
                                    "Open the program once, so numapilot knows how to start it.",
                                ));
                            if b.clicked() {
                                shortcut_op = Some(true);
                            }
                        }
                    }
                });
            });
        });
    match shortcut_op {
        Some(true) => {
            let cmd = numapilot::desktop::launch_command(&ed.key, ed.main_pid).unwrap_or_default();
            match numapilot::desktop::create_shortcut(&ed.key, &ed.label, &cmd) {
                Ok(p) => {
                    ed.shortcut = Some(p);
                    let _ = std::process::Command::new("kbuildsycoca6")
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                    app.toast(
                        crate::tf!(
                            "Shortcut \"{} (numapilot)\" created in the menu",
                            "Atalho \"{} (numapilot)\" criado no menu",
                            ed.label
                        ),
                        true,
                    );
                }
                Err(e) => app.toast(
                    crate::tf!(
                        "Could not create the shortcut: {e}",
                        "Não consegui criar o atalho: {e}"
                    ),
                    false,
                ),
            }
        }
        Some(false) => match numapilot::desktop::remove_shortcut(&ed.key) {
            Ok(_) => {
                ed.shortcut = None;
                app.toast(crate::i18n::tr("Menu shortcut removed"), true);
            }
            Err(e) => app.toast(
                crate::tf!(
                    "Could not remove the shortcut: {e}",
                    "Não consegui remover o atalho: {e}"
                ),
                false,
            ),
        },
        None => {}
    }
    match steam_line {
        Some(Some(line)) => {
            ctx.copy_text(line.clone());
            app.toast(crate::tf!("Copied: {line}", "Copiado: {line}"), true);
        }
        Some(None) => app.toast(
            crate::i18n::tr("Steam's configuration was not found (open Steam once)."),
            false,
        ),
        None => {}
    }
    if apply {
        let p = ed.p.clone();
        app.send(Request::SetProfile {
            key: ed.key.clone(),
            profile: p,
            pid: ed.main_pid,
        });
    }
    if fix {
        app.send(Request::FixMemory {
            key: ed.key.clone(),
        });
    }
    if open {
        app.editor = Some(ed);
    }
}
