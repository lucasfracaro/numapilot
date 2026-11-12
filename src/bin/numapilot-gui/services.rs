//! Aba Serviços: os serviços do systemd (sistema e usuário), com estado, se abrem no boot,
//! memória, e Iniciar/Parar/Reiniciar e o log de cada um.

use crate::{App, fmt_bytes, theme};
use egui::RichText;
use numapilot::services::{self, Service, Verb};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
enum Show {
    All,
    Running,
    Stopped,
    Failed,
}

pub(crate) struct ServicesTab {
    user: bool,
    show: Show,
    filter: String,
    list: Vec<Service>,
    at: Option<(Instant, bool)>,
    log: Option<(Service, String)>,
}

impl Default for ServicesTab {
    fn default() -> Self {
        ServicesTab {
            user: false,
            show: Show::Running,
            filter: String::new(),
            list: vec![],
            at: None,
            log: None,
        }
    }
}

pub(crate) fn ui(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::pal(ui);
    let t = &mut app.services;
    if t.at
        .is_none_or(|(i, u)| u != t.user || i.elapsed() > Duration::from_secs(5))
    {
        t.list = services::list(t.user);
        t.at = Some((Instant::now(), t.user));
    }
    let running = t.list.iter().filter(|s| s.running()).count();
    let failed = t.list.iter().filter(|s| s.failed()).count();
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(crate::tf!(
                    "{} serviços · {running} rodando",
                    "{} services · {running} running",
                    t.list.len()
                ))
                .size(20.0)
                .strong(),
            );
            ui.label(
                RichText::new(if t.user {
                    crate::i18n::tr("Do seu usuário: rodam a partir do login, sem pedir senha.")
                } else {
                    crate::i18n::tr(
                        "Do sistema: rodam desde o boot. Iniciar, parar e reiniciar pedem a senha.",
                    )
                })
                .weak(),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(crate::i18n::tr("Atualizar")).clicked() {
                t.at = None;
            }
            ui.add(
                egui::TextEdit::singleline(&mut t.filter)
                    .hint_text(crate::i18n::tr("🔍 Buscar"))
                    .desired_width(160.0),
            );
        });
    });
    ui.horizontal(|ui| {
        ui.selectable_value(&mut t.user, false, crate::i18n::tr("Do sistema"));
        ui.selectable_value(&mut t.user, true, crate::i18n::tr("Do usuário"));
        ui.separator();
        ui.selectable_value(&mut t.show, Show::Running, crate::i18n::tr("Rodando"));
        ui.selectable_value(&mut t.show, Show::Stopped, crate::i18n::tr("Parados"));
        let fl = if failed > 0 {
            crate::tf!("Com falha ({failed})", "Failed ({failed})")
        } else {
            crate::i18n::tr("Com falha").into()
        };
        ui.selectable_value(
            &mut t.show,
            Show::Failed,
            RichText::new(fl).color(if failed > 0 { p.err } else { p.text }),
        );
        ui.selectable_value(&mut t.show, Show::All, crate::i18n::tr("Todos"));
    });
    ui.label(
        RichText::new(crate::i18n::tr(
            "Para escolher o que abre no boot, use a aba Inicialização.",
        ))
        .small()
        .weak(),
    );
    ui.add_space(4.0);

    let f = t.filter.to_lowercase();
    let list: Vec<Service> = t
        .list
        .iter()
        .filter(|s| match t.show {
            Show::All => true,
            Show::Running => s.running(),
            Show::Stopped => !s.running() && !s.failed(),
            Show::Failed => s.failed(),
        })
        .filter(|s| {
            f.is_empty()
                || s.unit.to_lowercase().contains(&f)
                || s.description.to_lowercase().contains(&f)
        })
        .cloned()
        .collect();
    let mut act: Option<(Service, Verb)> = None;
    let mut want_log: Option<Service> = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if list.is_empty() {
                ui.label(RichText::new(crate::i18n::tr("Nenhum serviço nesse filtro.")).weak());
            }
            egui::Grid::new("services")
                .striped(true)
                .num_columns(5)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    for h in [
                        crate::i18n::tr("Serviço"),
                        crate::i18n::tr("Estado"),
                        crate::i18n::tr("No boot"),
                        crate::i18n::tr("Memória"),
                        "",
                    ] {
                        ui.label(RichText::new(h).small().color(p.weak));
                    }
                    ui.end_row();
                    for s in &list {
                        ui.vertical(|ui| {
                            ui.set_max_width(460.0);
                            let d = if s.description.chars().count() > 64 {
                                format!("{}…", s.description.chars().take(64).collect::<String>())
                            } else {
                                s.description.clone()
                            };
                            ui.label(RichText::new(d).strong());
                            ui.label(RichText::new(&s.unit).small().weak());
                        });
                        let c = if s.failed() {
                            p.err
                        } else if s.running() {
                            p.ok
                        } else {
                            p.weak
                        };
                        ui.label(RichText::new(s.state_text()).color(c));
                        ui.label(match s.at_boot() {
                            Some(true) => RichText::new(crate::i18n::tr("sim")),
                            Some(false) => RichText::new(crate::i18n::tr("não")).color(p.weak),
                            None => RichText::new("—").color(p.weak),
                        });
                        ui.label(
                            s.mem
                                .filter(|m| *m >= 1048576)
                                .map(fmt_bytes)
                                .unwrap_or_default(),
                        );
                        ui.horizontal(|ui| {
                            let why = s.protected.unwrap_or("");
                            if s.running() {
                                if ui
                                    .add_enabled(
                                        s.protected.is_none(),
                                        egui::Button::new(crate::i18n::tr("Parar")).small(),
                                    )
                                    .on_disabled_hover_text(why)
                                    .clicked()
                                {
                                    act = Some((s.clone(), Verb::Stop));
                                }
                                if ui
                                    .add_enabled(
                                        s.protected.is_none(),
                                        egui::Button::new(crate::i18n::tr("Reiniciar")).small(),
                                    )
                                    .on_disabled_hover_text(why)
                                    .clicked()
                                {
                                    act = Some((s.clone(), Verb::Restart));
                                }
                            } else if s.file_state != "masked"
                                && ui.small_button(crate::i18n::tr("Iniciar")).clicked()
                            {
                                act = Some((s.clone(), Verb::Start));
                            }
                            if ui.small_button("Log").clicked() {
                                want_log = Some(s.clone());
                            }
                        });
                        ui.end_row();
                    }
                });
        });

    if let Some((s, v)) = act {
        if s.user {
            match services::run_user(&s, v) {
                Ok(()) => app.toast(format!("{}: {}", s.unit, v.text().to_lowercase()), true),
                Err(e) => app.toast(e, false),
            }
        } else {
            match services::action_command(&s, v) {
                Ok(c) => app.start_job(format!("{} {}", v.text(), s.unit), c),
                Err(e) => app.toast(e, false),
            }
        }
        app.services.at = None;
    }
    if let Some(s) = want_log {
        let text = services::logs(&s, 200);
        app.services.log = Some((s, text));
    }
    log_window(app, ui.ctx());
}

fn log_window(app: &mut App, ctx: &egui::Context) {
    let Some((s, text)) = app.services.log.clone() else {
        return;
    };
    let mut open = true;
    let mut refresh = false;
    egui::Window::new(format!("Log: {}", s.unit))
        .open(&mut open)
        .default_size([760.0, 440.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button(crate::i18n::tr("Atualizar")).clicked() {
                    refresh = true;
                }
                if ui.button(crate::i18n::tr("Copiar")).clicked() {
                    ui.ctx().copy_text(text.clone());
                }
                ui.label(
                    RichText::new(crate::i18n::tr("últimas 200 linhas"))
                        .small()
                        .weak(),
                );
            });
            egui::ScrollArea::both()
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&text).monospace().small())
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
        });
    if refresh {
        app.services.log = Some((s.clone(), services::logs(&s, 200)));
    }
    if !open {
        app.services.log = None;
    }
}
