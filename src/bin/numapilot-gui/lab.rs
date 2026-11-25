//! "Lab and containers" tab: turns the lab on/off, opens the IDEs and shows
//! the containers with the apps installed in each one.

use crate::App;
use crate::jobs::{home, open_url, sibling, spawn_detached};
use eframe::egui::{self, Color32, RichText};
use numapilot::containers::{self, Container, Kind};
use numapilot::labcfg::LabSettings;

// ---------- actions (also used by the tray icon) ----------

pub fn jupyter_url(st: &LabSettings) -> String {
    format!(
        "http://{}:{}/lab?token={}",
        st.url_host("LAB_BIND"),
        st.get("LAB_PORT_JUPYTER"),
        st.token()
    )
}

pub fn code_url(st: &LabSettings) -> String {
    format!(
        "http://{}:{}",
        st.url_host("LAB_BIND"),
        st.get("LAB_PORT_CODE")
    )
}

/// Garante a entrada "gfx906-lab" no ~/.ssh/config (VS Code Remote-SSH, PyCharm).
pub fn ensure_ssh_config(st: &LabSettings) -> std::io::Result<bool> {
    let p = home().join(".ssh/config");
    let cur = std::fs::read_to_string(&p).unwrap_or_default();
    if cur.lines().any(|l| l.trim() == "Host gfx906-lab") {
        return Ok(false);
    }
    std::fs::create_dir_all(home().join(".ssh"))?;
    let block = format!(
        "\n# added by numapilot (MI50 lab)\nHost gfx906-lab\n    HostName 127.0.0.1\n    Port {}\n    User root\n    StrictHostKeyChecking accept-new\n",
        st.get("LAB_PORT_SSH")
    );
    std::fs::write(&p, cur + &block)?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
    Ok(true)
}

pub fn open_code_app(st: &LabSettings) -> Result<(), String> {
    let bin = ["code", "codium", "code-oss"]
        .into_iter()
        .find(|b| containers::which(b))
        .ok_or(crate::i18n::tr("VS Code is not installed on the PC"))?;
    ensure_ssh_config(st).map_err(|e| e.to_string())?;
    spawn_detached(&[
        bin.into(),
        "--remote".into(),
        "ssh-remote+gfx906-lab".into(),
        "/workspace".into(),
    ])
    .map_err(|e| e.to_string())
}

pub fn open_terminal(container: &str) -> Result<(), String> {
    open_terminal_cmd(container, "bash")
}

/// Opens a host terminal running `cmd` inside the container (terminal apps: htop, vim...).
pub fn open_terminal_cmd(container: &str, cmd: &str) -> Result<(), String> {
    let eng = containers::engine().ok_or(crate::i18n::tr("no podman/docker"))?;
    let inner = vec![
        eng,
        "exec".to_string(),
        "-it".to_string(),
        container.to_string(),
        "sh".to_string(),
        "-c".to_string(),
        cmd.to_string(),
    ];
    let term: Vec<String> = if containers::which("konsole") {
        vec!["konsole".into(), "-e".into()]
    } else if containers::which("gnome-terminal") {
        vec!["gnome-terminal".into(), "--".into()]
    } else if containers::which("kitty") {
        vec!["kitty".into()]
    } else if containers::which("alacritty") {
        vec!["alacritty".into(), "-e".into()]
    } else if containers::which("xterm") {
        vec!["xterm".into(), "-e".into()]
    } else {
        return Err(crate::i18n::tr(
            "no terminal found (konsole, gnome-terminal, kitty, alacritty, xterm)",
        )
        .into());
    };
    spawn_detached(&[term, inner].concat()).map_err(|e| e.to_string())
}

pub fn gfx(args: &[&str]) -> Vec<String> {
    let mut v = vec![sibling("gfx906-rocm")];
    v.extend(args.iter().map(|s| s.to_string()));
    v
}

// ---------- automation (n8n + Nod) ----------

use numapilot::automation::{self, State};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Last status of the automation stack, read in a thread (it runs docker/podman commands).
static AUTO: Mutex<Option<(Instant, Option<automation::Status>, bool)>> = Mutex::new(None);

fn auto_status(ctx: &egui::Context) -> Option<automation::Status> {
    let mut g = AUTO.lock().ok()?;
    let (at, st, busy) = g.get_or_insert((Instant::now() - Duration::from_secs(60), None, false));
    if !*busy && at.elapsed() > Duration::from_secs(4) {
        *busy = true;
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let s = automation::status();
            if let Ok(mut g) = AUTO.lock() {
                *g = Some((Instant::now(), Some(s), false));
            }
            ctx.request_repaint();
        });
    }
    st.clone()
}

fn state_badge(ui: &mut egui::Ui, name: &str, s: Option<State>) {
    let (t, c) = match s {
        Some(State::Running) => (crate::i18n::tr("running"), crate::theme::ok()),
        Some(State::Stopped) => (crate::t2!("stopped", "parado"), Color32::GRAY),
        Some(State::Missing) => (crate::i18n::tr("not installed"), Color32::GRAY),
        Some(State::NoEngine) => (crate::i18n::tr("unavailable"), crate::theme::err()),
        None => ("…", Color32::GRAY),
    };
    ui.label(RichText::new(name).strong());
    ui.label(RichText::new(t).color(c));
}

fn automation_ui(app: &mut App, ui: &mut egui::Ui) {
    let st = auto_status(ui.ctx());
    let n8n = st.as_ref().map(|s| s.n8n);
    let nod = st.as_ref().map(|s| s.nod);
    let any_running = n8n == Some(State::Running) || nod == Some(State::Running);
    let installed = matches!(n8n, Some(State::Running | State::Stopped));
    let cli = |a: &str| vec![sibling("numapilot"), "automation".into(), a.into()];
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            crate::theme::title(ui, crate::i18n::tr("Automation: n8n + Nod"));
            state_badge(ui, "n8n", n8n);
            state_badge(ui, "Nod", nod);
        });
        ui.label(RichText::new(crate::i18n::tr("n8n (workflows) and Nod (n8n's AI agent, which builds and tests n8n workflows for you) in containers. They only run when you turn them on here: nothing starts at boot.")).weak());
        if let Some(n) = st.as_ref().and_then(|s| s.nod_note.as_ref()) {
            ui.label(RichText::new(n).small().color(crate::theme::err()));
        }
        ui.horizontal_wrapped(|ui| {
            if any_running {
                if ui.button(crate::i18n::tr("Turn off")).clicked() {
                    app.start_job(crate::i18n::tr("Turning n8n and Nod off"), cli("down"));
                    invalidate();
                }
            } else if ui
                .button(if installed { crate::i18n::tr("Turn on") } else { crate::i18n::tr("Create and turn on (slow the 1st time)") })
                .clicked()
            {
                app.start_job(crate::i18n::tr("Turning n8n and Nod on"), cli("up"));
                invalidate();
            }
            if ui.add_enabled(installed, egui::Button::new(crate::i18n::tr("Update")))
                .on_hover_text(crate::i18n::tr("New n8n image and Nod's latest code (workflows and Nod's data are kept)"))
                .clicked()
            {
                app.start_job(crate::i18n::tr("Updating n8n and Nod"), cli("update"));
                invalidate();
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(crate::i18n::tr("Open:")).strong());
            if ui.add_enabled(n8n == Some(State::Running), egui::Button::new("n8n")).clicked() {
                let _ = open_url(&automation::n8n_url());
            }
            let token = automation::nod_token();
            let b = ui.add_enabled(nod == Some(State::Running), egui::Button::new("Nod"));
            let b = match &token {
                Some(t) => b.on_hover_text(crate::tf!("access token: {}", "token de acesso: {}", t)),
                None => b,
            };
            if b.clicked() {
                let port = st.as_ref().map(|s| s.nod_port).unwrap_or(automation::NOD_PORT);
                if let Some(t) = token {
                    ui.ctx().copy_text(t);
                    app.toast(crate::i18n::tr("Nod access token copied (paste it on the login screen)"), true);
                }
                let _ = open_url(&automation::nod_url(port));
            }
        });
        if any_running {
            let host = st
                .as_ref()
                .and_then(|s| s.engine.as_deref())
                .map(automation::host_from_container)
                .unwrap_or("host.docker.internal");
            let llm = LabSettings::load().get("LLM_PORT");
            ui.label(
                RichText::new(crate::tf!(
                    "In Nod: Apps → n8n → address http://n8n:5678, \"Use an access token instead\" with the token from n8n (Settings → Instance-level MCP). Local AI in n8n or Nod: http://{host}:{llm}/v1 (the AI server must listen beyond 127.0.0.1).",
                    "No Nod: Apps → n8n → endereço http://n8n:5678, \"Use an access token instead\" com o token do n8n (Settings → Instance-level MCP). IA local no n8n ou no Nod: http://{host}:{llm}/v1 (o servidor de IA precisa escutar além do 127.0.0.1)."
                ))
                .small()
                .weak(),
            );
        }
    });
}

/// Reads the status again soon after a button (the job changes it).
fn invalidate() {
    if let Ok(mut g) = AUTO.lock()
        && let Some((at, _, _)) = g.as_mut()
    {
        *at = Instant::now() - Duration::from_secs(60);
    }
}

// ---------- interface ----------

fn status_badge(ui: &mut egui::Ui, running: bool) {
    let (t, c) = if running {
        (crate::i18n::tr("running"), crate::theme::ok())
    } else {
        (crate::t2!("stopped", "parado"), Color32::GRAY)
    };
    ui.label(RichText::new(t).color(c));
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let st = LabSettings::load();
    let (list, apps, inspecting) = match app.bg.lock() {
        Ok(b) => (b.containers.clone(), b.apps.clone(), b.inspecting.clone()),
        Err(_) => (None, Default::default(), Default::default()),
    };
    let containers: Vec<Container> = match &list {
        Some(Ok(v)) => v.clone(),
        Some(Err(e)) => {
            ui.label(
                RichText::new(crate::tf!(
                    "Containers unavailable: {e}",
                    "Containers indisponíveis: {e}"
                ))
                .color(crate::theme::err()),
            );
            ui.label(crate::i18n::tr("Install podman:  sudo pacman -S podman"));
            vec![]
        }
        None => {
            ui.spinner();
            vec![]
        }
    };
    let lab = containers.iter().find(|c| c.kind == Kind::Lab).cloned();
    let lab_running = lab.as_ref().is_some_and(|c| c.running);

    egui::ScrollArea::vertical().show(ui, |ui| {
        // ---- lab ----
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                crate::theme::title(ui, crate::i18n::tr("Data and AI lab"));
                status_badge(ui, lab_running);
                if let Some(c) = &lab {
                    ui.label(RichText::new(&c.image).weak().small());
                } else {
                    ui.label(RichText::new(crate::i18n::tr("not created yet")).weak());
                }
            });
            ui.label(RichText::new(crate::i18n::tr("PyTorch on the MI50s + pandas, scikit-learn, transformers... Your files stay in ~/lab.")).weak());
            ui.horizontal_wrapped(|ui| {
                if lab_running {
                    if ui.button(crate::i18n::tr("Turn off")).clicked() {
                        app.start_job(crate::i18n::tr("Turning the lab off"), gfx(&["lab", "down"]));
                    }
                } else if ui.button(if lab.is_some() { crate::i18n::tr("Turn on") } else { crate::i18n::tr("Create and turn on (slow the 1st time)") }).clicked() {
                    app.start_job(crate::i18n::tr("Turning the lab on"), gfx(&["lab", "up"]));
                }
                if ui.add_enabled(lab_running, egui::Button::new(crate::i18n::tr("Test GPUs"))).clicked() {
                    app.start_job(crate::i18n::tr("GPU test in the lab"), gfx(&["lab", "test"]));
                }
                if ui.button(crate::i18n::tr("Update")).on_hover_text(crate::i18n::tr("New PyTorch/ROCm base; tests on the GPUs and reverts by itself if it fails")).clicked() {
                    app.start_job(crate::i18n::tr("Updating the lab"), gfx(&["lab", "update"]));
                }
                if ui.button(crate::i18n::tr("Update Python packages")).clicked() {
                    app.start_job(crate::i18n::tr("Rebuilding the lab"), gfx(&["lab", "update", "--force"]));
                }
                if ui.button(crate::i18n::tr("Roll back version")).clicked() {
                    app.start_job(crate::i18n::tr("Rolling back to the previous image"), gfx(&["lab", "rollback"]));
                }
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(crate::i18n::tr("Open:")).strong());
                ui.add_enabled_ui(lab_running, |ui| {
                    if ui.button("JupyterLab").clicked() {
                        let _ = open_url(&jupyter_url(&st));
                    }
                    if ui.button(crate::i18n::tr("VS Code (browser)")).on_hover_text(crate::tf!("password: {}", "senha: {}", st.token())).clicked() {
                        ui.ctx().copy_text(st.token());
                        let _ = open_url(&code_url(&st));
                        app.toast(crate::i18n::tr("VS Code password copied (paste it on the login screen)"), true);
                    }
                    if ui.button(crate::i18n::tr("VS Code (PC app)")).on_hover_text(crate::i18n::tr("Needs the Remote - SSH extension")).clicked() {
                        match open_code_app(&st) {
                            Ok(()) => app.toast(crate::i18n::tr("Opening VS Code connected to the lab"), true),
                            Err(e) => app.toast(e, false),
                        }
                    }
                    // PyCharm is optional in the image (LAB_PYCHARM): without it the button
                    // offers to add it instead of running a command that can only fail.
                    if st.get("LAB_PYCHARM").trim().starts_with('1') {
                        if ui.button("PyCharm").on_hover_text(crate::i18n::tr("Opens the PyCharm installed in the lab")).clicked() {
                            app.start_job(crate::i18n::tr("Opening PyCharm"), gfx(&["lab", "pycharm"]));
                        }
                    } else if ui
                        .button(crate::i18n::tr("PyCharm (install)"))
                        .on_hover_text(crate::i18n::tr("PyCharm is not in the lab yet. Click to add it: the lab image is rebuilt (a large download, takes several minutes) and the lab restarts."))
                        .clicked()
                    {
                        let mut s = st.clone();
                        s.set("LAB_PYCHARM", "1");
                        match s.save_keys(&["LAB_PYCHARM"]) {
                            Ok(()) => app.start_job(crate::i18n::tr("Installing PyCharm in the lab"), gfx(&["lab", "update", "--force"])),
                            Err(e) => app.toast(e.to_string(), false),
                        }
                    }
                    if ui.button("Terminal").clicked()
                        && let Err(e) = open_terminal(&st.get("LAB_NAME"))
                    {
                        app.toast(e, false);
                    }
                });
                if ui.button(crate::i18n::tr("~/lab folder")).clicked() {
                    let _ = open_url(&home().join("lab").to_string_lossy());
                }
            });
            if lab_running {
                ui.label(
                    RichText::new(crate::tf!(
                        "SSH: ssh gfx906-lab (port {}) · interpreter: /usr/bin/python3", "SSH: ssh gfx906-lab (porta {}) · interpretador: /usr/bin/python3",
                        st.get("LAB_PORT_SSH")
                    ))
                    .small()
                    .weak(),
                );
            }
        });
        ui.add_space(8.0);
        automation_ui(app, ui);
        ui.add_space(8.0);

        // ---- containers ----
        ui.horizontal(|ui| {
            crate::theme::title(ui, "Containers");
            ui.label(RichText::new(crate::i18n::tr("click \"View apps\" to list what is installed in each one")).weak());
        });
        if containers.is_empty() && matches!(list, Some(Ok(_))) {
            ui.label(crate::i18n::tr("No containers yet."));
        }
        for c in &containers {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&c.name).strong());
                    status_badge(ui, c.running);
                    let kind = match c.kind {
                        Kind::Lab => crate::i18n::tr("lab"),
                        Kind::Llm => crate::t2!("AI server", "servidor de IA"),
                        Kind::Distrobox => "distrobox",
                        Kind::Other => "container",
                    };
                    ui.label(RichText::new(format!("{kind} · {}", c.image)).weak().small());
                });
                ui.horizontal_wrapped(|ui| {
                    let eng = containers::engine().unwrap_or_else(|| "podman".into());
                    if c.running {
                        if crate::theme::small(ui, crate::i18n::tr("Stop")).clicked() {
                            app.start_job(crate::tf!("Stopping {}", "Parando {}", c.name), vec![eng.clone(), "stop".into(), c.name.clone()]);
                        }
                    } else if crate::theme::small(ui, crate::i18n::tr("Turn on")).clicked() {
                        app.start_job(crate::tf!("Starting {}", "Ligando {}", c.name), vec![eng.clone(), "start".into(), c.name.clone()]);
                    }
                    if ui.add_enabled(c.running, egui::Button::new("Terminal").small()).clicked()
                        && let Err(e) = open_terminal(&c.name)
                    {
                        app.toast(e, false);
                    }
                    let busy = inspecting.contains(&c.id);
                    let label = if apps.contains_key(&c.id) { crate::i18n::tr("Refresh apps") } else { crate::i18n::tr("View apps") };
                    if ui.add_enabled(c.running && !busy, egui::Button::new(label).small()).clicked() {
                        crate::bg::inspect(&app.bg, c.clone(), ui.ctx().clone());
                    }
                    if busy {
                        ui.spinner();
                    }
                    for p in &c.ports {
                        let host = if p.host_ip.is_empty() || p.host_ip == "0.0.0.0" { "127.0.0.1" } else { &p.host_ip };
                        let url = if c.kind == Kind::Lab && p.container_port == 8888 {
                            jupyter_url(&st)
                        } else {
                            format!("http://{host}:{}", p.host_port)
                        };
                        if p.container_port != 2222 && crate::theme::small(ui, crate::tf!("open :{}", "abrir :{}", p.host_port)).on_hover_text(&url).clicked() {
                            let _ = open_url(&url);
                        }
                    }
                });
                if let Some(a) = apps.get(&c.id) {
                    if let Some(e) = &a.error {
                        ui.label(RichText::new(e).weak());
                    }
                    let can_open = containers::open_command(c, "true").is_some();
                    egui::CollapsingHeader::new(crate::tf!("Programs with a window ({})", "Programas com janela ({})", a.desktop.len()))
                        .id_salt(format!("d-{}", c.id))
                        .default_open(!a.desktop.is_empty())
                        .show(ui, |ui| {
                            if a.desktop.is_empty() {
                                ui.label(RichText::new(crate::i18n::tr("no app with a shortcut")).weak());
                            }
                            ui.horizontal_wrapped(|ui| {
                                for (name, exec) in &a.desktop {
                                    // Terminal apps (htop, vim) need a terminal on the host, and the
                                    // lab's Jupyter is a page of the server that is already running:
                                    // neither shows anything when started detached in the container.
                                    let in_terminal = a.terminal.iter().any(|t| t == name);
                                    let jupyter = c.kind == Kind::Lab && containers::is_jupyter_exec(exec);
                                    let enabled = can_open || in_terminal || jupyter;
                                    let b = ui.add_enabled(enabled, egui::Button::new(name)).on_hover_text(exec);
                                    let b = if enabled { b } else { b.on_disabled_hover_text(crate::i18n::tr("only apps from the lab and from distrobox containers (which have display access) can be opened")) };
                                    if !b.clicked() {
                                        continue;
                                    }
                                    let r = if jupyter {
                                        let url = if exec.contains("notebook") {
                                            jupyter_url(&st).replacen("/lab?", "/tree?", 1)
                                        } else {
                                            jupyter_url(&st)
                                        };
                                        open_url(&url).map_err(|e| e.to_string())
                                    } else if in_terminal {
                                        open_terminal_cmd(&c.name, &containers::clean_exec(exec))
                                    } else if let Some(cmd) = containers::open_command(c, exec) {
                                        spawn_detached(&cmd).map_err(|e| e.to_string())
                                    } else {
                                        continue;
                                    };
                                    match r {
                                        Ok(()) => app.toast(crate::tf!("Opening {name}", "Abrindo {name}"), true),
                                        Err(e) => app.toast(e, false),
                                    }
                                }
                            });
                        });
                    egui::CollapsingHeader::new(crate::tf!("IDEs and tools ({})", "IDEs e ferramentas ({})", a.tools.len()))
                        .id_salt(format!("t-{}", c.id))
                        .show(ui, |ui| {
                            for (n, p) in &a.tools {
                                ui.label(format!("{n}  —  {p}"));
                            }
                        });
                    filtered_list(ui, &mut app.filter, &format!("p-{}", c.id), crate::i18n::tr("Python packages"),
                        &a.python.iter().map(|(n, v)| format!("{n} {v}")).collect::<Vec<_>>());
                    filtered_list(ui, &mut app.filter, &format!("s-{}", c.id), crate::i18n::tr("System packages (installed by hand)"), &a.system);
                }
            });
            ui.add_space(4.0);
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                crate::i18n::tr("Tip: apps installed in the lab (apt install ... inside the Terminal) show up here after clicking \
                 \"Refresh apps\". To keep what you installed even after updating the image, prefer pip/conda inside ~/lab."),
            )
            .small()
            .weak(),
        );
    });
}

fn filtered_list(ui: &mut egui::Ui, filter: &mut String, id: &str, title: &str, items: &[String]) {
    egui::CollapsingHeader::new(format!("{title} ({})", items.len()))
        .id_salt(id)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("filter:"));
                ui.text_edit_singleline(filter);
            });
            let f = filter.to_lowercase();
            egui::ScrollArea::vertical()
                .id_salt(format!("{id}-scroll"))
                .max_height(180.0)
                .show(ui, |ui| {
                    for it in items
                        .iter()
                        .filter(|i| f.is_empty() || i.to_lowercase().contains(&f))
                    {
                        ui.label(RichText::new(it).monospace().small());
                    }
                });
        });
}
