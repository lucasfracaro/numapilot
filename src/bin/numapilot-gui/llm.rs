//! Aba "Servidor de IA": llama.cpp nas MI50 com API compatível com OpenAI.

use crate::App;
use crate::jobs::open_url;
use crate::lab::gfx;
use eframe::egui::{self, Color32, RichText};
use numapilot::labcfg::{LabSettings, list_models};
use numapilot::topo::GpuRole;
use std::path::PathBuf;

const KEYS: &[&str] = &[
    "LLM_MODELS",
    "LLM_MODEL",
    "LLM_GPU",
    "LLM_CTX",
    "LLM_PORT",
    "LLM_ARGS",
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
        ui.heading("Servidor de IA (llama.cpp)");
        let (t, c) = match state {
            None => ("parado", Color32::GRAY),
            Some(false) => ("carregando o modelo...", Color32::from_rgb(230, 160, 40)),
            Some(true) => ("pronto", Color32::from_rgb(90, 180, 110)),
        };
        ui.label(RichText::new(t).color(c));
    });
    ui.label(
        RichText::new(
            "Roda modelos .gguf nas MI50 com uma API compatível com a da OpenAI (serve para Open WebUI, \
             Continue, scripts...). Cada GPU usa os processadores e a memória do próprio socket.",
        )
        .weak(),
    );
    ui.separator();

    egui::Grid::new("llm")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label("Pasta dos modelos");
            ui.horizontal(|ui| {
                let mut d = cfg.values.get("LLM_MODELS").cloned().unwrap_or_default();
                if ui
                    .add(egui::TextEdit::singleline(&mut d).desired_width(320.0))
                    .changed()
                {
                    cfg.set("LLM_MODELS", &d);
                    app.models = None;
                }
                if ui.small_button("abrir").clicked() {
                    let _ = std::fs::create_dir_all(&models_dir);
                    let _ = open_url(&models_dir.to_string_lossy());
                }
                if ui.small_button("recarregar").clicked() {
                    app.models = None;
                }
            });
            ui.end_row();

            ui.label("Modelo");
            let models = app.models.clone().unwrap_or_default();
            let cur = cfg.get("LLM_MODEL");
            let shown = if cur.is_empty() {
                "(escolha)".to_string()
            } else {
                cur.clone()
            };
            egui::ComboBox::from_id_salt("llm-model")
                .selected_text(shown)
                .width(420.0)
                .show_ui(ui, |ui| {
                    if models.is_empty() {
                        ui.label(format!("nenhum .gguf em {}", models_dir.display()));
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

            ui.label("Placas");
            let mut gpu = cfg.get("LLM_GPU");
            let mut opts: Vec<(String, String)> = vec![(
                "compute:all".into(),
                "As duas MI50 (modelo dividido)".into(),
            )];
            for g in app.topo.gpus.iter().filter(|g| g.role == GpuRole::Compute) {
                opts.push((
                    g.alias.clone(),
                    format!(
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

            ui.label("Contexto (tokens)");
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

            ui.label("Porta");
            let mut port: u16 = cfg.get("LLM_PORT").parse().unwrap_or(8080);
            if ui
                .add(egui::DragValue::new(&mut port).range(1024..=65535))
                .changed()
            {
                cfg.set("LLM_PORT", &port.to_string());
            }
            ui.end_row();

            ui.label("Argumentos extras");
            let mut a = cfg.get("LLM_ARGS");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut a)
                        .desired_width(420.0)
                        .hint_text("ex.: --flash-attn on -ctk q8_0 -ctv q8_0"),
                )
                .changed()
            {
                cfg.set("LLM_ARGS", &a);
            }
            ui.end_row();
        });

    let model = cfg.get("LLM_MODEL");
    if !model.is_empty() {
        let f = numapilot::labcfg::cfg_dir()
            .join("llm-tune")
            .join(format!("{}.env", model.trim_end_matches(".gguf")));
        if let Ok(t) = std::fs::read_to_string(&f) {
            let res = t.lines().find_map(|l| l.strip_prefix("# geração ")).unwrap_or("");
            let args = t
                .lines()
                .find_map(|l| l.strip_prefix("LLM_TUNED_ARGS="))
                .unwrap_or("")
                .trim_matches('"');
            let one = t.lines().any(|l| l == "LLM_TUNED_ONE=1");
            ui.label(
                RichText::new(format!(
                    "Otimizado: {}{args} ({res})",
                    if one { "1 GPU, " } else { "" }
                ))
                .color(Color32::from_rgb(110, 190, 120)),
            );
        }
    }
    ui.add_space(6.0);
    let port = cfg.get("LLM_PORT");
    ui.horizontal_wrapped(|ui| {
        let has_model = !cfg.get("LLM_MODEL").is_empty();
        if state.is_none() {
            if ui
                .add_enabled(has_model, egui::Button::new("Iniciar"))
                .clicked()
            {
                save(app, &cfg);
                app.start_job("Iniciando o servidor de IA", gfx(&["llm", "up"]));
            }
        } else {
            if ui.button("Parar").clicked() {
                app.start_job("Parando o servidor de IA", gfx(&["llm", "down"]));
            }
            if ui.button("Reiniciar com estes ajustes").clicked() {
                save(app, &cfg);
                app.start_job(
                    "Reiniciando o servidor de IA",
                    vec![
                        "sh".into(),
                        "-c".into(),
                        format!("'{0}' llm down; '{0}' llm up", gfx(&[])[0]),
                    ],
                );
            }
        }
        if ui.button("Salvar ajustes").clicked() {
            save(app, &cfg);
        }
        if ui.button("Atualizar llama.cpp").clicked() {
            app.start_job("Atualizando o llama.cpp", gfx(&["llm", "update"]));
        }
        if ui
            .add_enabled(has_model && state.is_none(), egui::Button::new("Otimizar (tokens/s)"))
            .on_hover_text("Testa 1 ou todas as GPUs, divisão por camadas ou linhas, flash attention e cache q8_0 com o modelo escolhido e grava a combinação mais rápida. Com o servidor parado; leva alguns minutos.")
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
            .add_enabled(state.is_some(), egui::Button::new("Ver log"))
            .clicked()
        {
            app.start_job(
                "Log do servidor de IA",
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
    ui.add_space(6.0);
    ui.add_enabled_ui(state == Some(true), |ui| {
        ui.horizontal(|ui| {
            if ui.button("Abrir chat").clicked() {
                let _ = open_url(&format!("http://127.0.0.1:{port}"));
            }
            let api = format!("http://127.0.0.1:{port}/v1");
            ui.label(RichText::new(format!("API: {api}")).monospace());
            if ui.small_button("copiar").clicked() {
                ui.ctx().copy_text(api);
            }
        });
    });
    app.llm_cfg = Some(cfg);
}

fn save(app: &mut App, cfg: &LabSettings) {
    match cfg.save_keys(KEYS) {
        Ok(()) => app.toast("Ajustes do servidor de IA salvos", true),
        Err(e) => app.toast(format!("Não consegui salvar: {e}"), false),
    }
}
