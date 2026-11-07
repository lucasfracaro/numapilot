//! Aba "Laboratório e containers": liga/desliga o laboratório, abre as IDEs e mostra
//! os containers com os apps instalados em cada um.

use crate::App;
use crate::jobs::{home, open_url, sibling, spawn_detached};
use eframe::egui::{self, Color32, RichText};
use numapilot::containers::{self, Container, Kind};
use numapilot::labcfg::LabSettings;

// ---------- ações (usadas também pelo ícone da bandeja) ----------

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
        "\n# adicionado pelo numapilot (laboratório das MI50)\nHost gfx906-lab\n    HostName 127.0.0.1\n    Port {}\n    User root\n    StrictHostKeyChecking accept-new\n",
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
        .ok_or("VS Code não está instalado no PC")?;
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
    let eng = containers::engine().ok_or("sem podman/docker")?;
    let inner = vec![
        eng,
        "exec".to_string(),
        "-it".to_string(),
        container.to_string(),
        "bash".to_string(),
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
        return Err(
            "nenhum terminal encontrado (konsole, gnome-terminal, kitty, alacritty, xterm)".into(),
        );
    };
    spawn_detached(&[term, inner].concat()).map_err(|e| e.to_string())
}

pub fn gfx(args: &[&str]) -> Vec<String> {
    let mut v = vec![sibling("gfx906-rocm")];
    v.extend(args.iter().map(|s| s.to_string()));
    v
}

// ---------- interface ----------

fn status_badge(ui: &mut egui::Ui, running: bool) {
    let (t, c) = if running {
        ("rodando", Color32::from_rgb(90, 180, 110))
    } else {
        ("parado", Color32::GRAY)
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
                RichText::new(format!("Containers indisponíveis: {e}"))
                    .color(Color32::from_rgb(220, 80, 60)),
            );
            ui.label("Instale o podman:  sudo pacman -S podman");
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
        // ---- laboratório ----
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.heading("Laboratório de dados e IA");
                status_badge(ui, lab_running);
                if let Some(c) = &lab {
                    ui.label(RichText::new(&c.image).weak().small());
                } else {
                    ui.label(RichText::new("ainda não criado").weak());
                }
            });
            ui.label(RichText::new("PyTorch nas MI50 + pandas, scikit-learn, transformers... Seus arquivos ficam em ~/lab.").weak());
            ui.horizontal_wrapped(|ui| {
                if lab_running {
                    if ui.button("Desligar").clicked() {
                        app.start_job("Desligando o laboratório", gfx(&["lab", "down"]));
                    }
                } else if ui.button(if lab.is_some() { "Ligar" } else { "Criar e ligar (demora na 1ª vez)" }).clicked() {
                    app.start_job("Ligando o laboratório", gfx(&["lab", "up"]));
                }
                if ui.add_enabled(lab_running, egui::Button::new("Testar GPUs")).clicked() {
                    app.start_job("Teste das GPUs no laboratório", gfx(&["lab", "test"]));
                }
                if ui.button("Atualizar").on_hover_text("Base nova do PyTorch/ROCm; testa nas GPUs e volta sozinho se falhar").clicked() {
                    app.start_job("Atualizando o laboratório", gfx(&["lab", "update"]));
                }
                if ui.button("Atualizar pacotes Python").clicked() {
                    app.start_job("Reconstruindo o laboratório", gfx(&["lab", "update", "--force"]));
                }
                if ui.button("Voltar versão").clicked() {
                    app.start_job("Voltando para a imagem anterior", gfx(&["lab", "rollback"]));
                }
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Abrir:").strong());
                ui.add_enabled_ui(lab_running, |ui| {
                    if ui.button("JupyterLab").clicked() {
                        let _ = open_url(&jupyter_url(&st));
                    }
                    if ui.button("VS Code (navegador)").on_hover_text(format!("senha: {}", st.token())).clicked() {
                        ui.ctx().copy_text(st.token());
                        let _ = open_url(&code_url(&st));
                        app.toast("Senha do VS Code copiada (cole na tela de login)", true);
                    }
                    if ui.button("VS Code (app do PC)").on_hover_text("Precisa da extensão Remote - SSH").clicked() {
                        match open_code_app(&st) {
                            Ok(()) => app.toast("Abrindo o VS Code conectado ao laboratório", true),
                            Err(e) => app.toast(e, false),
                        }
                    }
                    if ui.button("PyCharm").on_hover_text("Precisa de LAB_PYCHARM=1 na imagem (Configurações do laboratório)").clicked() {
                        app.start_job("Abrindo o PyCharm", gfx(&["lab", "pycharm"]));
                    }
                    if ui.button("Terminal").clicked()
                        && let Err(e) = open_terminal(&st.get("LAB_NAME"))
                    {
                        app.toast(e, false);
                    }
                });
                if ui.button("Pasta ~/lab").clicked() {
                    let _ = open_url(&home().join("lab").to_string_lossy());
                }
            });
            if lab_running {
                ui.label(
                    RichText::new(format!(
                        "SSH: ssh gfx906-lab (porta {}) · interpretador: /usr/bin/python3",
                        st.get("LAB_PORT_SSH")
                    ))
                    .small()
                    .weak(),
                );
            }
        });
        ui.add_space(8.0);

        // ---- containers ----
        ui.horizontal(|ui| {
            ui.heading("Containers");
            ui.label(RichText::new("clique em \"Ver apps\" para listar o que está instalado em cada um").weak());
        });
        if containers.is_empty() && matches!(list, Some(Ok(_))) {
            ui.label("Nenhum container ainda.");
        }
        for c in &containers {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&c.name).strong());
                    status_badge(ui, c.running);
                    let kind = match c.kind {
                        Kind::Lab => "laboratório",
                        Kind::Llm => "servidor de IA",
                        Kind::Distrobox => "distrobox",
                        Kind::Other => "container",
                    };
                    ui.label(RichText::new(format!("{kind} · {}", c.image)).weak().small());
                });
                ui.horizontal_wrapped(|ui| {
                    let eng = containers::engine().unwrap_or_else(|| "podman".into());
                    if c.running {
                        if ui.small_button("Parar").clicked() {
                            app.start_job(format!("Parando {}", c.name), vec![eng.clone(), "stop".into(), c.name.clone()]);
                        }
                    } else if ui.small_button("Ligar").clicked() {
                        app.start_job(format!("Ligando {}", c.name), vec![eng.clone(), "start".into(), c.name.clone()]);
                    }
                    if ui.add_enabled(c.running, egui::Button::new("Terminal").small()).clicked()
                        && let Err(e) = open_terminal(&c.name)
                    {
                        app.toast(e, false);
                    }
                    let busy = inspecting.contains(&c.id);
                    let label = if apps.contains_key(&c.id) { "Atualizar apps" } else { "Ver apps" };
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
                        if p.container_port != 2222 && ui.small_button(format!("abrir :{}", p.host_port)).on_hover_text(&url).clicked() {
                            let _ = open_url(&url);
                        }
                    }
                });
                if let Some(a) = apps.get(&c.id) {
                    if let Some(e) = &a.error {
                        ui.label(RichText::new(e).weak());
                    }
                    let can_open = containers::open_command(c, "true").is_some();
                    egui::CollapsingHeader::new(format!("Programas com janela ({})", a.desktop.len()))
                        .id_salt(format!("d-{}", c.id))
                        .default_open(!a.desktop.is_empty())
                        .show(ui, |ui| {
                            if a.desktop.is_empty() {
                                ui.label(RichText::new("nenhum app com atalho").weak());
                            }
                            ui.horizontal_wrapped(|ui| {
                                for (name, exec) in &a.desktop {
                                    let b = ui.add_enabled(can_open, egui::Button::new(name)).on_hover_text(exec);
                                    let b = if can_open { b } else { b.on_disabled_hover_text("só dá para abrir apps do laboratório e de containers distrobox (que têm acesso à tela)") };
                                    if b.clicked()
                                        && let Some(cmd) = containers::open_command(c, exec)
                                    {
                                        let _ = spawn_detached(&cmd);
                                        app.toast(format!("Abrindo {name}"), true);
                                    }
                                }
                            });
                        });
                    egui::CollapsingHeader::new(format!("IDEs e ferramentas ({})", a.tools.len()))
                        .id_salt(format!("t-{}", c.id))
                        .show(ui, |ui| {
                            for (n, p) in &a.tools {
                                ui.label(format!("{n}  —  {p}"));
                            }
                        });
                    filtered_list(ui, &mut app.filter, &format!("p-{}", c.id), "Pacotes Python",
                        &a.python.iter().map(|(n, v)| format!("{n} {v}")).collect::<Vec<_>>());
                    filtered_list(ui, &mut app.filter, &format!("s-{}", c.id), "Pacotes do sistema (instalados à mão)", &a.system);
                }
            });
            ui.add_space(4.0);
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Dica: apps instalados no laboratório (apt install ... dentro do Terminal) aparecem aqui ao clicar em \
                 \"Atualizar apps\". Para guardar o que instalou mesmo após atualizar a imagem, prefira pip/conda dentro de ~/lab.",
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
                ui.label("filtrar:");
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
