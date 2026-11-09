//! Aba Inicialização: tudo que abre quando o sistema liga, agrupado como na aba Programas, com
//! ligar/desligar, remover e adicionar (programa instalado, aberto agora, arquivo ou comando).

use crate::{App, fmt_bytes, theme};
use egui::RichText;
use numapilot::startup::{self, Action, Group, Item};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum AddMode {
    Installed,
    Running,
    File,
    Command,
}

/// Estado da aba.
pub(crate) struct StartupTab {
    items: Vec<Item>,
    at: Option<Instant>,
    filter: String,
    closed: [bool; 4],
    add: Option<AddMode>,
    query: String,
    found: Vec<(String, std::path::PathBuf)>,
    found_for: Option<String>,
    path: String,
    name: String,
    command: String,
    delay: u32,
}

impl Default for StartupTab {
    fn default() -> Self {
        StartupTab {
            items: vec![],
            at: None,
            filter: String::new(),
            // componentes da sessão fechados: quase ninguém mexe
            closed: [false, false, false, true],
            add: None,
            query: String::new(),
            found: vec![],
            found_for: None,
            path: String::new(),
            name: String::new(),
            command: String::new(),
            delay: 0,
        }
    }
}

impl StartupTab {
    pub(crate) fn invalidate(&mut self) {
        self.at = None;
    }
}

fn group_index(g: Group) -> usize {
    match g {
        Group::Login => 0,
        Group::UserService => 1,
        Group::SystemService => 2,
        Group::Session => 3,
    }
}

/// Escolher um arquivo pelo diálogo do desktop (kdialog ou zenity), se existir.
fn pick_file() -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let tries: [(&str, Vec<String>); 2] = [
        (
            "kdialog",
            vec![
                "--getopenfilename".into(),
                home.clone(),
                "*.AppImage *.sh *|Programas e scripts".into(),
            ],
        ),
        (
            "zenity",
            vec![
                "--file-selection".into(),
                "--title=Programa, AppImage ou script".into(),
            ],
        ),
    ];
    for (bin, args) in tries {
        if numapilot::containers::which(bin) {
            let o = std::process::Command::new(bin).args(&args).output().ok()?;
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            return (!s.is_empty()).then_some(s);
        }
    }
    None
}

pub(crate) fn ui(app: &mut App, ui: &mut egui::Ui) {
    let p = theme::pal(ui);
    if app
        .startup
        .at
        .is_none_or(|t| t.elapsed() > Duration::from_secs(5))
    {
        app.startup.items = startup::list();
        app.startup.at = Some(Instant::now());
    }
    let items = app.startup.items.clone();
    let on = items.iter().filter(|i| i.enabled).count();

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new(format!("{on} itens abrem quando o sistema liga")).size(20.0).strong());
            ui.label(RichText::new("Desligar só vale para o próximo login ou boot: o que está aberto continua aberto.").weak());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(RichText::new("+ Adicionar").strong(), |ui| {
                for (m, t, h) in [
                    (AddMode::Installed, "Programa instalado…", "Procura nos programas do menu"),
                    (AddMode::Running, "Programa aberto agora…", "Um dos programas da aba Programas"),
                    (AddMode::File, "Arquivo (AppImage, script .sh, binário)…", "Escolhe o arquivo"),
                    (AddMode::Command, "Comando…", "Qualquer linha de comando"),
                ] {
                    if ui.button(t).on_hover_text(h).clicked() {
                        app.startup.add = Some(m);
                        app.startup.query.clear();
                        app.startup.found_for = None;
                        app.startup.path.clear();
                        app.startup.name.clear();
                        app.startup.command.clear();
                        app.startup.delay = 0;
                        ui.close();
                    }
                }
            });
            if ui.button("Atualizar").clicked() {
                app.startup.invalidate();
            }
            ui.add(egui::TextEdit::singleline(&mut app.startup.filter).hint_text("🔍 Buscar").desired_width(160.0));
        });
    });
    ui.add_space(6.0);

    let f = app.startup.filter.to_lowercase();
    let mut change: Option<(Item, bool)> = None;
    let mut remove: Option<Item> = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for g in [Group::Login, Group::UserService, Group::SystemService, Group::Session] {
            let list: Vec<&Item> = items
                .iter()
                .filter(|i| i.group == g)
                .filter(|i| f.is_empty() || i.name.to_lowercase().contains(&f) || i.detail.to_lowercase().contains(&f))
                .collect();
            if list.is_empty() {
                continue;
            }
            let gi = group_index(g);
            let closed = app.startup.closed[gi];
            let n_on = list.iter().filter(|i| i.enabled).count();
            let head = egui::Button::new(
                RichText::new(format!("{} {}  ({n_on} de {} ligados) · {}", if closed { "⏵" } else { "⏷" }, g.title(), list.len(), g.about()))
                    .strong(),
            )
            .fill(p.accent.gamma_multiply(0.18))
            .min_size(egui::vec2(ui.available_width(), 30.0));
            if ui.add(head).clicked() {
                app.startup.closed[gi] = !closed;
            }
            if closed {
                ui.add_space(4.0);
                continue;
            }
            egui::Grid::new(format!("startup-{gi}"))
                .striped(true)
                .num_columns(4)
                .spacing([16.0, 6.0])
                .min_col_width(60.0)
                .show(ui, |ui| {
                    for it in list {
                        // ligar/desligar
                        let mut v = it.enabled;
                        let resp = ui.add_enabled(it.protected.is_none() || !it.enabled, egui::Checkbox::new(&mut v, ""));
                        let resp = match it.protected {
                            Some(why) => resp.on_disabled_hover_text(format!("Não pode ser desligado: {why}")),
                            None => resp.on_hover_text(if it.enabled { "Desligar no próximo login/boot" } else { "Ligar no próximo login/boot" }),
                        };
                        if resp.changed() {
                            change = Some((it.clone(), v));
                        }
                        // nome e comando
                        ui.vertical(|ui| {
                            ui.set_max_width(520.0);
                            let name = if it.name.chars().count() > 70 { format!("{}…", it.name.chars().take(70).collect::<String>()) } else { it.name.clone() };
                            ui.label(RichText::new(name).strong().color(if it.enabled { p.text } else { p.weak }));
                            let d = if it.detail.chars().count() > 90 { format!("{}…", it.detail.chars().take(90).collect::<String>()) } else { it.detail.clone() };
                            ui.label(RichText::new(d).small().weak()).on_hover_text(format!("{}\n{}", it.detail, it.id));
                        });
                        // origem
                        ui.label(RichText::new(it.origin).small().color(p.weak));
                        // agora + ações
                        ui.horizontal(|ui| {
                            let now = if it.id.ends_with(".timer") {
                                if it.running { "agendado".to_string() } else { String::new() }
                            } else {
                                match (it.running, it.mem) {
                                    (true, Some(m)) if m >= 1048576 => format!("rodando · {}", fmt_bytes(m)),
                                    (true, _) => "rodando".to_string(),
                                    _ => String::new(),
                                }
                            };
                            if !now.is_empty() {
                                ui.label(RichText::new(now).small().color(p.ok));
                            }
                            if it.removable
                                && ui.small_button("Remover").on_hover_text("Apaga este atalho de inicialização (não desinstala o programa)").clicked()
                            {
                                remove = Some(it.clone());
                            }
                        });
                        ui.end_row();
                    }
                });
            ui.add_space(8.0);
        }
    });

    if let Some((it, v)) = change {
        match startup::set_enabled(&it, v) {
            Ok(Action::Done) => {
                numapilot::log::info(&format!(
                    "inicialização: {} {}",
                    it.id,
                    if v { "ligado" } else { "desligado" }
                ));
                app.toast(
                    format!(
                        "{}: {} no próximo login",
                        it.name,
                        if v { "liga" } else { "não abre mais" }
                    ),
                    true,
                );
            }
            Ok(Action::NeedsRoot(cmd)) => {
                app.start_job(
                    format!("{} {}", if v { "Ligar" } else { "Desligar" }, it.name),
                    cmd,
                );
            }
            Err(e) => app.toast(e, false),
        }
        app.startup.invalidate();
    }
    if let Some(it) = remove {
        match startup::remove(&it) {
            Ok(()) => app.toast(format!("{} removido da inicialização", it.name), true),
            Err(e) => app.toast(e, false),
        }
        app.startup.invalidate();
    }
    add_window(app, ui.ctx());
}

fn add_window(app: &mut App, ctx: &egui::Context) {
    let Some(mode) = app.startup.add else { return };
    let mut open = true;
    let mut done: Option<Result<std::path::PathBuf, String>> = None;
    let title = match mode {
        AddMode::Installed => "Abrir no login: programa instalado",
        AddMode::Running => "Abrir no login: programa aberto agora",
        AddMode::File => "Abrir no login: arquivo",
        AddMode::Command => "Abrir no login: comando",
    };
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .default_width(520.0)
        .show(ctx, |ui| {
            let s = &mut app.startup;
            match mode {
                AddMode::Installed => {
                    ui.add(egui::TextEdit::singleline(&mut s.query).hint_text("🔍 nome do programa (Steam, ZapZap...)").desired_width(f32::INFINITY));
                    if s.found_for.as_deref() != Some(s.query.as_str()) {
                        s.found = startup::search_apps(&s.query);
                        s.found_for = Some(s.query.clone());
                    }
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for (n, path) in s.found.iter().take(200) {
                            ui.horizontal(|ui| {
                                if ui.button("Adicionar").clicked() {
                                    done = Some(startup::add_desktop(path, s.delay));
                                }
                                ui.label(n);
                                ui.label(RichText::new(path.file_name().unwrap_or_default().to_string_lossy()).small().weak());
                            });
                        }
                    });
                }
                AddMode::Running => {
                    ui.label(RichText::new("Programas seus abertos agora (os mesmos da aba Programas):").weak());
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for g in app.groups.iter().filter(|g| g.uid == app.my_uid) {
                            ui.horizontal(|ui| {
                                if ui.button("Adicionar").clicked() {
                                    done = Some(startup::add_running(g.main_pid, &g.label));
                                }
                                ui.label(&g.label);
                                ui.label(RichText::new(&g.exe).small().weak());
                            });
                        }
                    });
                }
                AddMode::File => {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut s.path).hint_text("/caminho/do/programa.AppImage ou script.sh").desired_width(360.0));
                        if ui.button("Escolher…").clicked()
                            && let Some(f) = pick_file()
                        {
                            s.path = f;
                        }
                    });
                    ui.label(RichText::new("Se não for executável: o script roda pelo sh; AppImage e binário ficam executáveis.").small().weak());
                    if ui.add_enabled(!s.path.trim().is_empty(), egui::Button::new("Adicionar")).clicked() {
                        let path = s.path.trim().trim_start_matches("file://").to_string();
                        done = Some(startup::add_file(std::path::Path::new(&path), s.delay));
                    }
                }
                AddMode::Command => {
                    egui::Grid::new("add-cmd").num_columns(2).show(ui, |ui| {
                        ui.label("Nome");
                        ui.add(egui::TextEdit::singleline(&mut s.name).hint_text("Hermes agent").desired_width(360.0));
                        ui.end_row();
                        ui.label("Comando");
                        ui.add(egui::TextEdit::singleline(&mut s.command).hint_text("hermes-agent --serve").desired_width(360.0));
                        ui.end_row();
                    });
                    ui.label(RichText::new("Para IA nas GPUs do servidor, use numapilot run: numapilot run --gpu compute -- llama-server -m modelo.gguf").small().weak());
                    if ui.add_enabled(!s.name.trim().is_empty() && !s.command.trim().is_empty(), egui::Button::new("Adicionar")).clicked() {
                        done = Some(startup::add_command(s.name.trim(), s.command.trim(), s.delay));
                    }
                }
            }
            if mode != AddMode::Running {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Esperar antes de abrir:");
                    ui.add(egui::DragValue::new(&mut s.delay).range(0..=600).suffix(" s"));
                    ui.label(RichText::new("(útil para deixar a rede ou outro programa subir antes)").small().weak());
                });
            }
        });
    if let Some(r) = done {
        match r {
            Ok(p) => {
                numapilot::log::info(&format!("inicialização: adicionado {}", p.display()));
                app.toast("Adicionado: abre no próximo login", true);
                app.startup.add = None;
            }
            Err(e) => app.toast(e, false),
        }
        app.startup.invalidate();
    }
    if !open {
        app.startup.add = None;
    }
}
