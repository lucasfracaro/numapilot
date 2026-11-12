//! Aba Inicialização: tudo que abre quando o sistema liga, agrupado como na aba Programas, com
//! ligar/desligar, remover e adicionar (programa instalado, aberto agora, arquivo ou comando).

use crate::{App, fmt_bytes, theme};
use egui::RichText;
use numapilot::origin;
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
    /// Janela Detalhes: o item, de onde ele veio e o log (serviços).
    details: Option<(Item, origin::Trace, Option<String>)>,
    /// Janela Desinstalar: item, origem e "apagar também configurações e a pasta".
    uninstall: Option<(Item, origin::Trace, bool)>,
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
            details: None,
            uninstall: None,
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
            ui.label(
                RichText::new(crate::tf!(
                    "{on} items start when the system boots",
                    "{on} itens abrem quando o sistema liga"
                ))
                .size(20.0)
                .strong(),
            );
            ui.label(
                RichText::new(crate::i18n::tr(
                    "Turning off only applies to the next login or boot: what is open stays open.",
                ))
                .weak(),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button(RichText::new(crate::i18n::tr("+ Add")).strong(), |ui| {
                for (m, t, h) in [
                    (
                        AddMode::Installed,
                        crate::i18n::tr("Installed program…"),
                        crate::i18n::tr("Searches the menu programs"),
                    ),
                    (
                        AddMode::Running,
                        crate::i18n::tr("Program open now…"),
                        crate::i18n::tr("One of the programs in the Programs tab"),
                    ),
                    (
                        AddMode::File,
                        crate::i18n::tr("File (AppImage, .sh script, binary)…"),
                        crate::i18n::tr("Picks the file"),
                    ),
                    (
                        AddMode::Command,
                        crate::i18n::tr("Command…"),
                        crate::i18n::tr("Any command line"),
                    ),
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
            if ui.button(crate::i18n::tr("Update")).clicked() {
                app.startup.invalidate();
            }
            ui.add(
                egui::TextEdit::singleline(&mut app.startup.filter)
                    .hint_text(crate::i18n::tr("🔍 Search"))
                    .desired_width(160.0),
            );
        });
    });
    ui.add_space(6.0);

    let f = app.startup.filter.to_lowercase();
    let mut change: Option<(Item, bool)> = None;
    let mut remove: Option<Item> = None;
    let mut details: Option<Item> = None;
    let mut uninstall: Option<Item> = None;
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
                RichText::new(crate::tf!("{} {}  ({n_on} of {} enabled) · {}", "{} {}  ({n_on} de {} ligados) · {}", if closed { "⏵" } else { "⏷" }, g.title(), list.len(), g.about()))
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
                            Some(why) => resp.on_disabled_hover_text(crate::tf!("Cannot be turned off: {why}", "Não pode ser desligado: {why}")),
                            None => resp.on_hover_text(if it.enabled { crate::i18n::tr("Turn off at the next login/boot") } else { crate::i18n::tr("Turn on at the next login/boot") }),
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
                                if it.running { crate::i18n::tr("scheduled").to_string() } else { String::new() }
                            } else {
                                match (it.running, it.mem) {
                                    (true, Some(m)) if m >= 1048576 => crate::tf!("running · {}", "rodando · {}", fmt_bytes(m)),
                                    (true, _) => crate::i18n::tr("running").to_string(),
                                    _ => String::new(),
                                }
                            };
                            if !now.is_empty() {
                                ui.label(RichText::new(now).small().color(p.ok));
                            }
                            if it.id != "kde:session"
                                && ui.small_button(crate::i18n::tr("Details")).on_hover_text(crate::i18n::tr("Where it comes from: file, program, package, folders")).clicked()
                            {
                                details = Some(it.clone());
                            }
                            if it.removable
                                && ui.small_button(crate::i18n::tr("Remove")).on_hover_text(crate::i18n::tr("Deletes this startup shortcut (does not uninstall the program)")).clicked()
                            {
                                remove = Some(it.clone());
                            }
                            if it.protected.is_none()
                                && it.id != "kde:session"
                                && ui
                                    .small_button(crate::i18n::tr("Uninstall…"))
                                    .on_hover_text(crate::i18n::tr("Removes the program the right way for its source (package, Flatpak, npm, pipx, file...)"))
                                    .clicked()
                            {
                                uninstall = Some(it.clone());
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
                    "startup: {} {}",
                    it.id,
                    if v {
                        crate::i18n::tr("on")
                    } else {
                        crate::i18n::tr("off")
                    }
                ));
                app.toast(
                    crate::tf!(
                        "{}: {} at the next login",
                        "{}: {} no próximo login",
                        it.name,
                        if v {
                            crate::i18n::tr("turns on")
                        } else {
                            crate::i18n::tr("no longer starts")
                        }
                    ),
                    true,
                );
            }
            Ok(Action::NeedsRoot(cmd)) => {
                app.start_job(
                    format!(
                        "{} {}",
                        if v {
                            crate::i18n::tr("Turn on")
                        } else {
                            crate::i18n::tr("Turn off")
                        },
                        it.name
                    ),
                    cmd,
                );
            }
            Err(e) => app.toast(e, false),
        }
        app.startup.invalidate();
    }
    if let Some(it) = remove {
        match startup::remove(&it) {
            Ok(()) => app.toast(
                crate::tf!(
                    "{} removed from startup",
                    "{} removido da inicialização",
                    it.name
                ),
                true,
            ),
            Err(e) => app.toast(e, false),
        }
        app.startup.invalidate();
    }
    if let Some(it) = details {
        let t = origin::trace(&it.exec, it.file.as_deref());
        let log = unit_of(&it).map(|(u, user)| numapilot::services::unit_logs(u, user, 40));
        app.startup.details = Some((it, t, log));
    }
    if let Some(it) = uninstall {
        let t = origin::trace(&it.exec, it.file.as_deref());
        app.startup.uninstall = Some((it, t, false));
    }
    add_window(app, ui.ctx());
    details_window(app, ui.ctx());
    uninstall_window(app, ui.ctx());
}

fn unit_of(it: &Item) -> Option<(&str, bool)> {
    it.id
        .strip_prefix("user:")
        .map(|u| (u, true))
        .or_else(|| it.id.strip_prefix("sys:").map(|u| (u, false)))
}

/// Uma linha "rótulo: caminho [Abrir pasta]".
fn path_row(ui: &mut egui::Ui, label: &str, p: &std::path::Path) {
    ui.label(RichText::new(label).strong());
    ui.horizontal(|ui| {
        ui.label(RichText::new(p.display().to_string()).monospace().small());
        let dir = if p.is_dir() {
            p.to_path_buf()
        } else {
            p.parent().map(|d| d.to_path_buf()).unwrap_or_default()
        };
        if ui.small_button(crate::i18n::tr("Open folder")).clicked() {
            let _ = crate::jobs::open_url(&dir.to_string_lossy());
        }
    });
    ui.end_row();
}

fn details_window(app: &mut App, ctx: &egui::Context) {
    let Some((it, t, log)) = app.startup.details.clone() else {
        return;
    };
    let mut open = true;
    let mut goto: Option<i32> = None;
    let mut uninstall = false;
    egui::Window::new(crate::tf!(
        "Where it comes from: {}",
        "De onde vem: {}",
        it.name
    ))
    .open(&mut open)
    .collapsible(false)
    .default_width(640.0)
    .show(ctx, |ui| {
        egui::Grid::new("trace")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                if let Some(f) = &t.file {
                    path_row(ui, crate::i18n::tr("Started by"), f);
                    if let Some(p) = &t.file_pkg {
                        ui.label("");
                        ui.label(
                            RichText::new(crate::tf!(
                                "file from package {p}",
                                "arquivo do pacote {p}"
                            ))
                            .small()
                            .weak(),
                        );
                        ui.end_row();
                    }
                }
                ui.label(RichText::new(crate::i18n::tr("Command")).strong());
                ui.label(RichText::new(&it.exec).monospace().small());
                ui.end_row();
                if let Some(e) = &t.exe {
                    path_row(ui, crate::i18n::tr("Program"), e);
                }
                ui.label(RichText::new(crate::i18n::tr("Source")).strong());
                ui.label(t.source.text());
                ui.end_row();
                if let Some(d) = &t.description {
                    ui.label(RichText::new(crate::i18n::tr("Description")).strong());
                    ui.label(d);
                    ui.end_row();
                }
                if let Some(sz) = t.size.filter(|s| *s > 0) {
                    ui.label(RichText::new(crate::i18n::tr("Size")).strong());
                    ui.label(fmt_bytes(sz));
                    ui.end_row();
                }
                if let Some(d) = &t.app_dir {
                    path_row(ui, crate::i18n::tr("Program folder"), d);
                }
                for c in &t.configs {
                    path_row(ui, crate::i18n::tr("Settings"), c);
                }
                ui.label(RichText::new(crate::i18n::tr("Now")).strong());
                ui.label(match (it.running, it.pids.first()) {
                    (true, Some(p)) => crate::tf!("running (PID {p})", "rodando (PID {p})"),
                    (true, None) => crate::i18n::tr("running").into(),
                    _ => crate::i18n::tr("not running").into(),
                });
                ui.end_row();
            });
        ui.horizontal(|ui| {
            if let Some(p) = it.pids.first()
                && ui
                    .button(crate::i18n::tr("View in the Programs tab"))
                    .clicked()
            {
                goto = Some(*p);
            }
            if it.protected.is_none() && ui.button(crate::i18n::tr("Uninstall…")).clicked() {
                uninstall = true;
            }
        });
        if let Some(l) = &log {
            ui.separator();
            ui.label(RichText::new(crate::i18n::tr("Last log lines")).strong());
            egui::ScrollArea::both()
                .max_height(200.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(l).monospace().small())
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
        }
    });
    if let Some(pid) = goto {
        app.progs.filter = pid.to_string();
        app.tab = crate::Tab::Programs;
        open = false;
    }
    if uninstall {
        app.startup.uninstall = Some((it.clone(), t.clone(), false));
        open = false;
    }
    if !open {
        app.startup.details = None;
    }
}

fn uninstall_window(app: &mut App, ctx: &egui::Context) {
    let Some((it, t, mut configs)) = app.startup.uninstall.clone() else {
        return;
    };
    let p = theme::pal_ctx(ctx);
    let mut open = true;
    let mut go = false;
    let plan = origin::uninstall_plan(&t, unit_of(&it), configs);
    egui::Window::new(crate::tf!("Uninstall {}", "Desinstalar {}", it.name))
        .open(&mut open)
        .collapsible(false)
        .default_width(600.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.label(crate::tf!("Source: {}", "Origem: {}", t.source.text()));
            ui.add_space(4.0);
            if let Some(why) = &plan.blocked {
                ui.label(
                    RichText::new(crate::tf!(
                        "Cannot be uninstalled from here: {why}",
                        "Não dá para desinstalar por aqui: {why}"
                    ))
                    .color(p.err),
                );
                return;
            }
            ui.label(
                RichText::new(crate::i18n::tr(
                    "It will run, in a terminal (whatever needs the password asks there):",
                ))
                .strong(),
            );
            for s in &plan.steps {
                ui.label(format!("• {s}"));
            }
            // pacotes que saem junto (consulta ao pacman, sem senha)
            if let origin::Source::Pacman { pkg, .. } = &t.source {
                let key = egui::Id::unique(("rm-preview", pkg));
                let prev: Vec<String> = ui.data_mut(|d| {
                    d.get_temp_mut_or_insert_with(key, || origin::pacman_removal_preview(pkg))
                        .clone()
                });
                if prev.len() > 1 {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(crate::tf!(
                            "{} packages go away (the program and what only it used):",
                            "{} pacotes saem (o programa e o que só ele usava):",
                            prev.len()
                        ))
                        .strong(),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(110.0)
                        .id_salt("rm-prev")
                        .show(ui, |ui| {
                            for l in &prev {
                                ui.label(RichText::new(l).monospace().small());
                            }
                        });
                }
            }
            if !t.configs.is_empty() || t.app_dir.is_some() {
                ui.add_space(4.0);
                ui.checkbox(
                    &mut configs,
                    crate::i18n::tr("Also delete the settings and the program's folder"),
                );
                // sempre à vista: exatamente o que essa opção apaga
                for c in t.app_dir.iter().chain(t.configs.iter()) {
                    let txt = RichText::new(format!("    {}", c.display()))
                        .monospace()
                        .small();
                    ui.label(if configs {
                        txt.color(p.err)
                    } else {
                        txt.weak()
                    });
                }
            }
            ui.collapsing(crate::i18n::tr("Exact commands"), |ui| {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .id_salt("rm-script")
                    .show(ui, |ui| {
                        ui.label(RichText::new(&plan.script).monospace().small());
                    });
            });
            if it.running {
                ui.label(
                    RichText::new(crate::i18n::tr(
                        "The program is open: it will be closed first.",
                    ))
                    .small()
                    .color(p.warn),
                );
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new(crate::i18n::tr("Uninstall")).color(p.err))
                    .clicked()
                {
                    go = true;
                }
                if ui.button(crate::i18n::tr("Cancel")).clicked() {
                    go = false;
                    configs = false;
                    app.startup.uninstall = None;
                }
            });
        });
    if go {
        let (_, refused) = numapilot::procctl::terminate(&it.pids, app.my_uid, false);
        if let Some(r) = refused {
            numapilot::log::info(&format!("uninstall {}: did not close {r}", it.name));
        }
        let plan = origin::uninstall_plan(&t, unit_of(&it), configs);
        numapilot::log::info(&format!(
            "desinstalar {} ({}): {}\ncomandos:\n{}",
            it.name,
            t.source.text(),
            plan.steps.join(" | "),
            plan.script
        ));
        match crate::jobs::in_terminal(&["sh".into(), "-c".into(), plan.script]) {
            Ok(()) => app.toast(
                crate::tf!(
                    "Uninstalling {} in the terminal",
                    "Desinstalando {} no terminal",
                    it.name
                ),
                true,
            ),
            Err(e) => app.toast(e, false),
        }
        app.startup.invalidate();
        open = false;
    }
    if !open {
        app.startup.uninstall = None;
    } else if let Some(u) = app.startup.uninstall.as_mut() {
        u.2 = configs;
    }
}

fn add_window(app: &mut App, ctx: &egui::Context) {
    let Some(mode) = app.startup.add else { return };
    let mut open = true;
    let mut done: Option<Result<std::path::PathBuf, String>> = None;
    let title = match mode {
        AddMode::Installed => crate::i18n::tr("Open at login: installed program"),
        AddMode::Running => crate::i18n::tr("Open at login: program open now"),
        AddMode::File => crate::i18n::tr("Open at login: file"),
        AddMode::Command => crate::i18n::tr("Open at login: command"),
    };
    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .default_width(520.0)
        .show(ctx, |ui| {
            let s = &mut app.startup;
            match mode {
                AddMode::Installed => {
                    ui.add(egui::TextEdit::singleline(&mut s.query).hint_text(crate::i18n::tr("🔍 program name (Steam, ZapZap...)")).desired_width(f32::INFINITY));
                    if s.found_for.as_deref() != Some(s.query.as_str()) {
                        s.found = startup::search_apps(&s.query);
                        s.found_for = Some(s.query.clone());
                    }
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for (n, path) in s.found.iter().take(200) {
                            ui.horizontal(|ui| {
                                if ui.button(crate::i18n::tr("Add")).clicked() {
                                    done = Some(startup::add_desktop(path, s.delay));
                                }
                                ui.label(n);
                                ui.label(RichText::new(path.file_name().unwrap_or_default().to_string_lossy()).small().weak());
                            });
                        }
                    });
                }
                AddMode::Running => {
                    ui.label(RichText::new(crate::i18n::tr("Your programs open now (the same as in the Programs tab):")).weak());
                    egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                        for g in app.groups.iter().filter(|g| g.uid == app.my_uid) {
                            ui.horizontal(|ui| {
                                if ui.button(crate::i18n::tr("Add")).clicked() {
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
                        ui.add(egui::TextEdit::singleline(&mut s.path).hint_text(crate::i18n::tr("/path/to/program.AppImage or script.sh")).desired_width(360.0));
                        if ui.button(crate::i18n::tr("Choose…")).clicked()
                            && let Some(f) = pick_file()
                        {
                            s.path = f;
                        }
                    });
                    ui.label(RichText::new(crate::i18n::tr("If it is not executable: the script runs through sh; AppImage and binary are made executable.")).small().weak());
                    if ui.add_enabled(!s.path.trim().is_empty(), egui::Button::new(crate::i18n::tr("Add"))).clicked() {
                        let path = s.path.trim().trim_start_matches("file://").to_string();
                        done = Some(startup::add_file(std::path::Path::new(&path), s.delay));
                    }
                }
                AddMode::Command => {
                    egui::Grid::new("add-cmd").num_columns(2).show(ui, |ui| {
                        ui.label(crate::i18n::tr("Name"));
                        ui.add(egui::TextEdit::singleline(&mut s.name).hint_text("Hermes agent").desired_width(360.0));
                        ui.end_row();
                        ui.label(crate::i18n::tr("Command"));
                        ui.add(egui::TextEdit::singleline(&mut s.command).hint_text("hermes-agent --serve").desired_width(360.0));
                        ui.end_row();
                    });
                    ui.label(RichText::new(crate::i18n::tr("For AI on the server's GPUs, use numapilot run: numapilot run --gpu compute -- llama-server -m modelo.gguf")).small().weak());
                    if ui.add_enabled(!s.name.trim().is_empty() && !s.command.trim().is_empty(), egui::Button::new(crate::i18n::tr("Add"))).clicked() {
                        done = Some(startup::add_command(s.name.trim(), s.command.trim(), s.delay));
                    }
                }
            }
            if mode != AddMode::Running {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(crate::i18n::tr("Wait before opening:"));
                    ui.add(egui::DragValue::new(&mut s.delay).range(0..=600).suffix(" s"));
                    ui.label(RichText::new(crate::i18n::tr("(useful to let the network or another program come up first)")).small().weak());
                });
            }
        });
    if let Some(r) = done {
        match r {
            Ok(p) => {
                numapilot::log::info(&format!("startup: added {}", p.display()));
                app.toast(crate::i18n::tr("Added: opens at the next login"), true);
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
