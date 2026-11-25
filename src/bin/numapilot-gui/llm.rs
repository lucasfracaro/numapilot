//! "AI server" tab: llama.cpp on the MI50s with an OpenAI-compatible API.

use crate::App;
use crate::jobs::open_url;
use crate::lab::gfx;
use eframe::egui::{self, Color32, RichText};
use numapilot::labcfg::{LabSettings, LlmPreset, list_models};
use numapilot::topo::GpuRole;
use std::path::PathBuf;

const KEYS: &[&str] = &[
    "LLM_MODELS",
    "LLM_MODEL",
    "LLM_GPU",
    "LLM_CTX",
    "LLM_PORT",
    "LLM_ARGS",
    "LLM_AUTOSTART",
    "LLM_IMAGE",
];

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    if app.llm_cfg.is_none() {
        app.llm_cfg = Some(LabSettings::load());
    }
    let state = app.bg.lock().ok().map(|b| b.llm).unwrap_or(None);
    let mut cfg = app.llm_cfg.take().unwrap_or_else(LabSettings::load);
    let models_dir = PathBuf::from(cfg.get("LLM_MODELS"));
    if app.models.is_none() {
        app.models = Some(list_models(&models_dir));
    }

    ui.horizontal(|ui| {
        crate::theme::title(ui, crate::i18n::tr("AI server (llama.cpp)"));
        let (t, c) = match state {
            None => (crate::t2!("stopped", "parado"), Color32::GRAY),
            Some(false) => (
                crate::i18n::tr("loading the model..."),
                crate::theme::warn(),
            ),
            Some(true) => (crate::t2!("ready", "pronto"), crate::theme::ok()),
        };
        ui.label(RichText::new(t).color(c));
    });
    ui.label(
        RichText::new(crate::i18n::tr(
            "Runs .gguf models on the MI50s with an OpenAI-compatible API (works with Open WebUI, \
             Continue, scripts...). Each GPU uses the processors and memory of its own socket.",
        ))
        .weak(),
    );
    ui.separator();

    presets_ui(app, ui, &mut cfg, state, &models_dir);

    egui::Grid::new("llm")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(crate::i18n::tr("Models folder"));
            ui.horizontal(|ui| {
                let mut d = cfg.values.get("LLM_MODELS").cloned().unwrap_or_default();
                if ui
                    .add(egui::TextEdit::singleline(&mut d).desired_width(320.0))
                    .changed()
                {
                    cfg.set("LLM_MODELS", &d);
                    app.models = None;
                }
                if crate::theme::small(ui, crate::i18n::tr("open")).clicked() {
                    let _ = std::fs::create_dir_all(&models_dir);
                    let _ = open_url(&models_dir.to_string_lossy());
                }
                if crate::theme::small(ui, crate::i18n::tr("reload")).clicked() {
                    app.models = None;
                }
            });
            ui.end_row();

            ui.label(crate::i18n::tr("Model"));
            let models = app.models.clone().unwrap_or_default();
            let cur = cfg.get("LLM_MODEL");
            let shown = if cur.is_empty() {
                crate::i18n::tr("(choose)").to_string()
            } else {
                cur.clone()
            };
            egui::ComboBox::from_id_salt("llm-model")
                .selected_text(shown)
                .width(420.0)
                .show_ui(ui, |ui| {
                    if models.is_empty() {
                        ui.label(crate::tf!(
                            "no .gguf in {}",
                            "nenhum .gguf em {}",
                            models_dir.display()
                        ));
                    }
                    for (m, size) in &models {
                        let mut sel = cur.clone();
                        if ui
                            .selectable_value(
                                &mut sel,
                                m.clone(),
                                format!("{m}  ({:.1} GB)", *size as f64 / 1e9),
                            )
                            .clicked()
                        {
                            cfg.set("LLM_MODEL", m);
                        }
                    }
                });
            ui.end_row();

            ui.label(crate::i18n::tr("Cards"));
            let mut gpu = cfg.get("LLM_GPU");
            let mut opts: Vec<(String, String)> = vec![(
                "compute:all".into(),
                crate::i18n::tr("Both MI50s (split model)").into(),
            )];
            for g in app.topo.gpus.iter().filter(|g| g.role == GpuRole::Compute) {
                opts.push((
                    g.alias.clone(),
                    crate::tf!(
                        "Only the {} (socket {})",
                        "Só a {} (socket {})",
                        g.alias,
                        g.numa_node.map(|n| n.to_string()).unwrap_or("?".into())
                    ),
                ));
            }
            let txt = opts
                .iter()
                .find(|(v, _)| *v == gpu)
                .map(|(_, t)| t.clone())
                .unwrap_or(gpu.clone());
            egui::ComboBox::from_id_salt("llm-gpu")
                .selected_text(txt)
                .width(280.0)
                .show_ui(ui, |ui| {
                    for (v, t) in &opts {
                        ui.selectable_value(&mut gpu, v.clone(), t);
                    }
                });
            if gpu != cfg.get("LLM_GPU") {
                cfg.set("LLM_GPU", &gpu);
            }
            ui.end_row();

            ui.label(crate::i18n::tr("Context (tokens)"));
            let mut ctx: u32 = cfg.get("LLM_CTX").parse().unwrap_or(16384);
            if ui
                .add(
                    egui::DragValue::new(&mut ctx)
                        .range(2048..=262144)
                        .speed(1024),
                )
                .changed()
            {
                cfg.set("LLM_CTX", &ctx.to_string());
            }
            ui.end_row();

            ui.label(crate::i18n::tr("Port"));
            let mut port: u16 = cfg.get("LLM_PORT").parse().unwrap_or(8080);
            if ui
                .add(egui::DragValue::new(&mut port).range(1024..=65535))
                .changed()
            {
                cfg.set("LLM_PORT", &port.to_string());
            }
            ui.end_row();

            ui.label(crate::i18n::tr("Extra arguments"));
            let mut a = cfg.get("LLM_ARGS");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut a)
                        .desired_width(420.0)
                        .hint_text(crate::i18n::tr("e.g. --flash-attn on -ctk q8_0 -ctv q8_0")),
                )
                .changed()
            {
                cfg.set("LLM_ARGS", &a);
            }
            ui.end_row();

            ui.label(crate::i18n::tr("Image (engine)"));
            let mut im = cfg.get("LLM_IMAGE");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut im)
                        .desired_width(420.0)
                        .hint_text(crate::i18n::tr(
                            "empty = the default llama.cpp for these cards",
                        )),
                )
                .on_hover_text(crate::i18n::tr(
                    "Container image with llama-server (another llama.cpp build or fork). \
                     The default follows the cards: gfx906 build on the MI50s, CUDA on NVIDIA, \
                     ROCm or Vulkan on the others.",
                ))
                .changed()
            {
                cfg.set("LLM_IMAGE", im.trim());
            }
            ui.end_row();

            ui.label(crate::i18n::tr("After a reboot"));
            let mut auto = cfg.get("LLM_AUTOSTART") != "0";
            if ui
                .checkbox(
                    &mut auto,
                    crate::i18n::tr("start the AI server again by itself"),
                )
                .on_hover_text(crate::i18n::tr(
                    "Off: the server only runs when you start it here (or from a saved model). \
                     Applies the next time it starts.",
                ))
                .changed()
            {
                cfg.set("LLM_AUTOSTART", if auto { "1" } else { "0" });
            }
            ui.end_row();
        });

    let model = cfg.get("LLM_MODEL");
    if !model.is_empty() {
        let f = numapilot::labcfg::cfg_dir()
            .join("llm-tune")
            .join(format!("{}.env", model.trim_end_matches(".gguf")));
        if let Ok(t) = std::fs::read_to_string(&f) {
            let res = t
                .lines()
                .find_map(|l| l.strip_prefix("# geração "))
                .unwrap_or("");
            let args = t
                .lines()
                .find_map(|l| l.strip_prefix("LLM_TUNED_ARGS="))
                .unwrap_or("")
                .trim_matches('"');
            let one = t.lines().any(|l| l == "LLM_TUNED_ONE=1");
            ui.label(
                RichText::new(crate::tf!(
                    "Optimized: {}{args} ({res})",
                    "Otimizado: {}{args} ({res})",
                    if one { "1 GPU, " } else { "" }
                ))
                .color(crate::theme::ok()),
            );
        }
    }
    ui.add_space(6.0);
    let port = cfg.get("LLM_PORT");
    ui.horizontal_wrapped(|ui| {
        let has_model = !cfg.get("LLM_MODEL").is_empty();
        if state.is_none() {
            if ui
                .add_enabled(has_model, egui::Button::new(crate::i18n::tr("Start")))
                .clicked()
            {
                save(app, &cfg);
                app.start_job(crate::i18n::tr("Starting the AI server"), gfx(&["llm", "up"]));
            }
        } else {
            if ui.button(crate::i18n::tr("Stop")).clicked() {
                app.start_job(crate::i18n::tr("Stopping the AI server"), gfx(&["llm", "down"]));
            }
            if ui.button(crate::i18n::tr("Restart with these settings")).clicked() {
                save(app, &cfg);
                app.start_job(
                    crate::i18n::tr("Restarting the AI server"),
                    vec![
                        "sh".into(),
                        "-c".into(),
                        format!("'{0}' llm down; '{0}' llm up", gfx(&[])[0]),
                    ],
                );
            }
        }
        if ui.button(crate::i18n::tr("Save settings")).clicked() {
            save(app, &cfg);
        }
        if ui.button(crate::i18n::tr("Update llama.cpp")).clicked() {
            app.start_job(crate::i18n::tr("Updating llama.cpp"), gfx(&["llm", "update"]));
        }
        if ui
            .add_enabled(has_model && state.is_none(), egui::Button::new(crate::i18n::tr("Optimize (tokens/s)")))
            .on_hover_text(crate::i18n::tr("Tests 1 or all GPUs, layer or row split, flash attention and q8_0 cache with the chosen model and saves the fastest combination. With the server stopped; takes a few minutes."))
            .clicked()
        {
            save(app, &cfg);
            let m = cfg.get("LLM_MODEL");
            let mut c = gfx(&["llm", "tune"]);
            c.push(m);
            if let Err(e) = crate::jobs::in_terminal(&c) {
                app.toast(e, false);
            }
        }
        let eng = numapilot::containers::engine().unwrap_or_else(|| "podman".into());
        if ui
            .add_enabled(state.is_some(), egui::Button::new(crate::i18n::tr("View log")))
            .clicked()
        {
            app.start_job(
                crate::i18n::tr("AI server log"),
                vec![
                    eng,
                    "logs".into(),
                    "--tail".into(),
                    "200".into(),
                    cfg.get("LLM_NAME"),
                ],
            );
        }
    });
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut app.llm_preset_name)
                .desired_width(220.0)
                .hint_text(crate::i18n::tr("name of the saved model")),
        );
        let ok = !app.llm_preset_name.trim().is_empty() && !cfg.get("LLM_MODEL").is_empty();
        if ui
            .add_enabled(
                ok,
                egui::Button::new(crate::i18n::tr("Save as a saved model")),
            )
            .on_hover_text(crate::i18n::tr(
                "Saves this model, cards, context and extra arguments as a button above. \
                 A saved model with the same name is replaced.",
            ))
            .clicked()
        {
            let mut list = app
                .llm_presets
                .take()
                .unwrap_or_else(numapilot::labcfg::load_presets);
            let p = LlmPreset::from_settings(&app.llm_preset_name, &cfg);
            let name = p.name.clone();
            numapilot::labcfg::upsert_preset(&mut list, p);
            match numapilot::labcfg::save_presets(&list) {
                Ok(()) => app.toast(
                    crate::tf!("Saved model \"{name}\" saved", "Modelo \"{name}\" salvo"),
                    true,
                ),
                Err(e) => app.toast(
                    crate::tf!("Could not save: {e}", "Não consegui salvar: {e}"),
                    false,
                ),
            }
            app.llm_presets = Some(list);
        }
    });
    ui.add_space(6.0);
    ui.add_enabled_ui(state == Some(true), |ui| {
        ui.horizontal(|ui| {
            if ui.button(crate::i18n::tr("Open chat")).clicked() {
                let _ = open_url(&format!("http://127.0.0.1:{port}"));
            }
            let api = format!("http://127.0.0.1:{port}/v1");
            ui.label(RichText::new(format!("API: {api}")).monospace());
            if crate::theme::small(ui, crate::i18n::tr("copy")).clicked() {
                ui.ctx().copy_text(api);
            }
        });
    });
    app.llm_cfg = Some(cfg);
}

/// Saved models: one button each that starts llama.cpp with that model and its settings.
fn presets_ui(
    app: &mut App,
    ui: &mut egui::Ui,
    cfg: &mut LabSettings,
    state: Option<bool>,
    models_dir: &std::path::Path,
) {
    if app.llm_presets.is_none() {
        app.llm_presets = Some(numapilot::labcfg::load_presets());
    }
    let list = app.llm_presets.clone().unwrap_or_default();
    ui.label(RichText::new(crate::i18n::tr("Saved models")).strong());
    if list.is_empty() {
        ui.label(
            RichText::new(crate::i18n::tr(
                "None yet: choose the model and settings below, give it a name and click \
                 \"Save as a saved model\".",
            ))
            .weak(),
        );
    }
    let running_model = cfg.get("LLM_MODEL");
    let mut remove = None;
    egui::Grid::new("llm-presets")
        .num_columns(3)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            for (i, p) in list.iter().enumerate() {
                let exists = models_dir.join(&p.model).is_file();
                let btn = ui
                    .add_enabled(
                        state.is_none() && exists,
                        egui::Button::new(format!("▶ {}", p.name)),
                    )
                    .on_hover_text(crate::i18n::tr("Start the AI server with this model"))
                    .on_disabled_hover_text(if exists {
                        crate::i18n::tr("Stop the AI server first")
                    } else {
                        crate::i18n::tr("The model file is not in the models folder")
                    });
                if btn.clicked() {
                    p.apply(cfg);
                    save(app, cfg);
                    app.start_job(
                        crate::tf!("Starting {}", "Iniciando {}", p.name),
                        gfx(&["llm", "up"]),
                    );
                }
                let gpu = if p.gpu == "compute:all" || p.gpu.is_empty() {
                    crate::i18n::tr("all cards").to_string()
                } else {
                    p.gpu.clone()
                };
                let mut info = format!("{}  ·  {}k  ·  {gpu}", p.model, p.ctx / 1024);
                if !p.args.is_empty() {
                    info.push_str(&format!("  ·  {}", p.args));
                }
                if !p.image.is_empty() {
                    info.push_str(&format!("  ·  {}", p.image));
                }
                let mut t = RichText::new(info).weak();
                if !exists {
                    t = t.color(crate::theme::warn());
                }
                ui.label(t);
                ui.horizontal(|ui| {
                    if state.is_some() && p.model == running_model {
                        ui.label(
                            RichText::new(crate::t2!("in use", "em uso")).color(crate::theme::ok()),
                        );
                    }
                    if crate::theme::small(ui, crate::i18n::tr("edit")).clicked() {
                        p.apply(cfg);
                        app.llm_preset_name = p.name.clone();
                    }
                    if crate::theme::small(ui, crate::i18n::tr("delete")).clicked() {
                        remove = Some(i);
                    }
                });
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        let mut list = list;
        let gone = list.remove(i);
        if let Err(e) = numapilot::labcfg::save_presets(&list) {
            app.toast(
                crate::tf!("Could not save: {e}", "Não consegui salvar: {e}"),
                false,
            );
        } else {
            app.toast(
                crate::tf!("Removed \"{}\"", "\"{}\" removido", gone.name),
                true,
            );
        }
        app.llm_presets = Some(list);
    }
    ui.separator();
}

fn save(app: &mut App, cfg: &LabSettings) {
    match cfg.save_keys(KEYS) {
        Ok(()) => app.toast(crate::i18n::tr("AI server settings saved"), true),
        Err(e) => app.toast(
            crate::tf!("Could not save: {e}", "Não consegui salvar: {e}"),
            false,
        ),
    }
}
