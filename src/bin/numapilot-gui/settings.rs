//! "Settings" tab: tray/autostart, weekly update, hardware.

use crate::App;
use crate::jobs::home;
use crate::lab::gfx;
use eframe::egui::{self, Color32, RichText};
use numapilot::topo::GpuRole;
use std::path::PathBuf;

pub fn autostart_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("autostart/numapilot-tray.desktop")
}

pub fn set_autostart(on: bool) -> std::io::Result<()> {
    let p = autostart_path();
    if on {
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(
            &p,
            format!(
                "[Desktop Entry]\nType=Application\nName=numapilot (bandeja)\nExec={} --tray\n\
                 Icon=preferences-system-performance\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n",
                crate::jobs::self_exe()
            ),
        )
    } else if p.exists() {
        std::fs::remove_file(p)
    } else {
        Ok(())
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let (daemon_ok, auto, last) = match app.bg.lock() {
        Ok(b) => (b.daemon_ok, b.autoupdate, b.last_update.clone()),
        Err(_) => (false, false, None),
    };
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading(crate::i18n::tr("General"));
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("numapilot service:"));
            if daemon_ok {
                ui.label(RichText::new(crate::i18n::tr("running")).color(Color32::from_rgb(90, 180, 110)));
            } else {
                ui.label(RichText::new(crate::i18n::tr("stopped — sudo systemctl enable --now numapilot")).color(Color32::from_rgb(220, 80, 60)));
            }
        });
        let mut tray = autostart_path().exists();
        if ui.checkbox(&mut tray, crate::i18n::tr("Tray icon when the session starts (shortcuts to the lab, AI and this window)")).changed() {
            match set_autostart(tray) {
                Ok(()) => {
                    if tray {
                        let _ = crate::jobs::spawn_detached(&[
                            crate::jobs::self_exe(),
                            "--tray".into(),
                        ]);
                    }
                    app.toast(if tray { crate::i18n::tr("Tray icon enabled") } else { crate::i18n::tr("Tray icon no longer starts by itself") }, true);
                }
                Err(e) => app.toast(e.to_string(), false),
            }
        }

        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Language"));
        ui.horizontal(|ui| {
            let en = !crate::i18n::portuguese();
            if ui.selectable_label(!en, "Português").clicked() {
                crate::i18n::set_portuguese(true);
            }
            if ui
                .selectable_label(en, "English")
                .on_hover_text("The interface is in English; messages that come from the service and the command line are still in Portuguese.")
                .clicked()
            {
                crate::i18n::set_portuguese(false);
            }
        });
        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Menu tabs"));
        ui.label(
            RichText::new(crate::i18n::tr("Uncheck what you do not use on this PC to make the menu shorter. Nothing is turned off: it is only the menu."))
                .small()
                .weak(),
        );
        ui.horizontal_wrapped(|ui| {
            let mut changed = false;
            for name in crate::hideable_tabs() {
                let mut on = !app.hidden_tabs.iter().any(|h| h == name);
                if ui.checkbox(&mut on, crate::i18n::tr(name)).changed() {
                    if on {
                        app.hidden_tabs.retain(|h| h != name);
                    } else {
                        app.hidden_tabs.push(name.to_string());
                    }
                    changed = true;
                }
            }
            if changed {
                crate::save_hidden_tabs(&app.hidden_tabs);
            }
        });

        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Updates"));
        let mut on = auto;
        if ui
            .checkbox(&mut on, crate::i18n::tr("Check every week (lab, llama.cpp and ROCm)"))
            .changed()
        {
            app.start_job(
                crate::i18n::tr("Weekly update"),
                gfx(&["autoupdate", if on { "enable" } else { "disable" }]),
            );
        }
        if let Some(l) = last {
            let ok = l["ok"].as_bool().unwrap_or(false);
            ui.label(
                RichText::new(crate::tf!(
                    "Last: {} — {}", "Última: {} — {}",
                    l["time"].as_str().unwrap_or("?"),
                    l["summary"].as_str().unwrap_or("").replace(';', ", ")
                ))
                .color(if ok { Color32::from_rgb(90, 180, 110) } else { Color32::from_rgb(230, 160, 40) }),
            );
        }
        updates_ui(app, ui);
        ui.add_space(4.0);
        selfupdate_ui(app, ui);

        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Phone panel"));
        web_ui(app, ui);

        peers_ui(app, ui);
        ui.add_space(8.0);
        notify_ui(app, ui);
        ui.add_space(8.0);
        vm_ui(app, ui);

        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Boot options"));
        let od = numapilot::bootcfg::overdrive_state();
        ui.horizontal(|ui| {
            let mut on = od.on_next_boot;
            if ui
                .add_enabled(!od.from_cmdline, egui::Checkbox::new(&mut on, crate::i18n::tr("OverDrive of the AMD cards (undervolt/overclock)")))
                .changed()
            {
                app.start_job(
                    if on { crate::i18n::tr("Enabling OverDrive") } else { crate::i18n::tr("Disabling OverDrive") },
                    vec![
                        "pkexec".into(),
                        crate::jobs::sibling_abs("numapilot"),
                        "boot".into(),
                        "overdrive".into(),
                        if on { "on" } else { "off" }.into(),
                    ],
                );
            }
            let txt = match (od.active, od.on_next_boot) {
                (true, true) => crate::i18n::tr("on").to_string(),
                (false, true) => crate::i18n::tr("turns on after reboot").into(),
                (true, false) => crate::i18n::tr("turns off after reboot").into(),
                (false, false) => crate::i18n::tr("off").into(),
            };
            ui.label(RichText::new(txt).weak());
        });
        if od.from_cmdline {
            ui.label(RichText::new(crate::i18n::tr("Set on the kernel command line (amdgpu.ppfeaturemask): change it in the bootloader or remove it there to control it from here.")).small().weak());
        }
        ui.label(
            RichText::new(crate::i18n::tr("Lives in /etc/modprobe.d/numapilot.conf and the boot image is rebuilt: works for any kernel, even after updates. Asks for the password."))
                .small()
                .weak(),
        );

        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Diagnosis"));
        ui.label(crate::i18n::tr("Tests each part on your hardware (NUMA, cgroups, Vulkan GPU selection, ROCm, fans, OverDrive, ReBAR, Steam) and says what to fix."));
        ui.horizontal(|ui| {
            if ui.button(crate::i18n::tr("Check everything")).clicked() {
                app.start_job(crate::i18n::tr("Diagnosis"), vec![crate::jobs::sibling("numapilot"), "doctor".into()]);
            }
            if ui
                .button(crate::i18n::tr("Back up the settings"))
                .on_hover_text(crate::i18n::tr("Saves ~/numapilot-backup-DATA.tar.gz with profiles, machine settings, fans, GPUs, lab and kernel parameters. Keep it off the system disk; to restore: ./install.sh --restore FILE"))
                .clicked()
            {
                app.start_job(crate::t2!("Settings backup", "Backup das configurações"), vec![crate::jobs::sibling("numapilot"), "backup".into()]);
            }
            if ui.button(crate::i18n::tr("Generate report to send")).on_hover_text(crate::i18n::tr("Saves ~/numapilot-relatorio.txt with the diagnosis and system information")).clicked() {
                app.start_job(crate::i18n::tr("Diagnosis report"), vec![crate::jobs::sibling("numapilot"), "doctor".into(), "--report".into()]);
            }
        });

        ui.add_space(8.0);
        ui.heading("Hardware");
        for n in &app.topo.nodes {
            ui.label(crate::tf!(
                "Socket {} — {} threads ({} physical cores), RAM {:.0} GB, CPUs {}", "Socket {} — {} threads ({} núcleos físicos), RAM {:.0} GB, CPUs {}",
                n.id,
                n.cpus.len(),
                n.physical_cores,
                n.mem_total_kb as f64 / 1048576.0,
                numapilot::util::format_cpulist(&n.cpus)
            ));
        }
        for g in &app.topo.gpus {
            let role = match g.role {
                GpuRole::Display => crate::i18n::tr("video (monitor connected)"),
                GpuRole::Compute if g.display_connected => crate::i18n::tr("compute + monitor connected to it"),
                GpuRole::Compute => crate::i18n::tr("compute; also runs games through PRIME"),
                GpuRole::Other => "outra",
            };
            ui.label(format!(
                "{} — {} — {} — socket {} — {}",
                g.alias,
                g.pci,
                g.name,
                g.numa_node.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                role
            ));
        }
        for w in app.topo.warnings() {
            ui.label(RichText::new(crate::tf!("Warning: {w}", "Aviso: {w}")).color(Color32::from_rgb(230, 160, 40)));
        }
        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Tips"));
        ui.label(crate::i18n::tr("• Game on an MI50: choose \"MI50 — game/graphics\" in the Programs tab."));
        ui.label(crate::i18n::tr("• Lossless Scaling (lsfg-vk): the official one generates the frames on the same GPU as the game."));
        ui.label(crate::i18n::tr("• Advanced settings: /etc/numapilot.toml (rules) and /etc/gfx906-rocm.conf (lab/ROCm)."));
    });
}

/// Draws a QR code (to open the phone panel by pointing the camera).
fn qr(ui: &mut egui::Ui, text: &str) {
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else {
        return;
    };
    let w = code.width();
    let colors = code.to_colors();
    let quiet = 2;
    let px = 4.0;
    let side = (w + 2 * quiet) as f32 * px;
    let (resp, p) = ui.allocate_painter(egui::vec2(side, side), egui::Sense::hover());
    let r = resp.rect;
    p.rect_filled(r, 0.0, Color32::WHITE);
    for (i, c) in colors.iter().enumerate() {
        if *c == qrcode::Color::Dark {
            let (x, y) = ((i % w + quiet) as f32, (i / w + quiet) as f32);
            p.rect_filled(
                egui::Rect::from_min_size(r.min + egui::vec2(x * px, y * px), egui::vec2(px, px)),
                0.0,
                Color32::BLACK,
            );
        }
    }
}

fn notify_ui(app: &mut App, ui: &mut egui::Ui) {
    use numapilot::notify::Notify;
    ui.label(RichText::new(crate::i18n::tr("Phone notifications")).strong());
    ui.label(
        RichText::new(crate::i18n::tr("Through the ntfy app (Android/iOS, free): hot GPU, failing disk, AI server down, PC off the network, undervolt done. Install ntfy and subscribe to the topic (or scan the QR code)."))
            .small()
            .weak(),
    );
    let saved = Notify::load();
    let n = app.notify_edit.get_or_insert_with(|| {
        let mut n = saved.clone();
        if n.topic.is_empty() {
            n.topic = numapilot::notify::random_topic();
        }
        n
    });
    ui.checkbox(&mut n.enabled, crate::i18n::tr("On"));
    egui::Grid::new("ntfy")
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label(crate::i18n::tr("Server"));
            ui.add(egui::TextEdit::singleline(&mut n.server).desired_width(260.0));
            ui.end_row();
            ui.label(crate::i18n::tr("Topic"));
            ui.add(egui::TextEdit::singleline(&mut n.topic).desired_width(260.0));
            ui.end_row();
            ui.label(crate::i18n::tr("Token (optional)"));
            let mut t = n.token.clone().unwrap_or_default();
            ui.add(
                egui::TextEdit::singleline(&mut t)
                    .password(true)
                    .desired_width(260.0),
            );
            n.token = (!t.is_empty()).then_some(t);
            ui.end_row();
        });
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut n.gpu_hot, crate::i18n::tr("Hot GPU"));
        ui.add_enabled(
            n.gpu_hot,
            egui::DragValue::new(&mut n.gpu_hot_c)
                .range(70.0..=105.0)
                .suffix(" °C"),
        );
        ui.checkbox(&mut n.health, crate::i18n::tr("disks/memory/battery"));
        ui.checkbox(&mut n.llm, crate::t2!("AI server", "servidor de IA"));
        ui.checkbox(&mut n.peers, crate::i18n::tr("other PCs"));
        ui.checkbox(&mut n.jobs, crate::i18n::tr("finished tasks"));
    });
    let n = n.clone();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(n != saved, egui::Button::new(crate::i18n::tr("Save")))
            .clicked()
        {
            app.send(numapilot::ctl::Request::SetNotify { notify: n.clone() });
        }
        if ui.button(crate::i18n::tr("Send test")).clicked() {
            match n.send_now(
                crate::i18n::tr("numapilot test"),
                crate::i18n::tr("If this arrived, notifications are working."),
                3,
                &["white_check_mark"],
            ) {
                Ok(()) => app.toast(crate::i18n::tr("Test notification sent"), true),
                Err(e) => app.toast(e, false),
            }
        }
        if ui
            .small_button(crate::i18n::tr("open in the browser"))
            .clicked()
        {
            let _ = crate::jobs::open_url(&n.subscribe_url());
        }
    });
    if !n.topic.is_empty() {
        ui.horizontal(|ui| {
            qr(ui, &n.subscribe_url());
            ui.label(RichText::new(crate::tf!("Subscribe in the app: {}\nAI and other-PC notifications need the phone panel turned on.", "Assine no app: {}\nOs avisos de IA e de outros PCs precisam do painel no celular ligado.", n.subscribe_url())).small().weak());
        });
    }
}

fn peers_ui(app: &mut App, ui: &mut egui::Ui) {
    ui.label(RichText::new(crate::i18n::tr("My other PCs")).strong());
    ui.label(
        RichText::new(crate::i18n::tr("Paste the link of another PC's panel (Settings → Phone panel, on that PC): this one's panel shows them all together."))
            .small()
            .weak(),
    );
    let peers = numapilot::peers::load();
    for p in &peers {
        ui.horizontal(|ui| {
            ui.label(&p.name);
            ui.label(RichText::new(&p.url).small().weak());
            if ui.small_button(crate::i18n::tr("open")).clicked() {
                let _ = crate::jobs::open_url(&p.link());
            }
            if p.mac.is_some()
                && ui
                    .small_button(crate::i18n::tr("turn on"))
                    .on_hover_text(crate::i18n::tr("Sends the Wake-on-LAN packet"))
                    .clicked()
            {
                match numapilot::peers::wake(&p.name) {
                    Ok(m) => app.toast(m, true),
                    Err(e) => app.toast(e, false),
                }
            }
            if ui
                .small_button(crate::i18n::tr("copy profiles"))
                .on_hover_text(crate::i18n::tr(
                    "Brings in the program profiles (Adjust…) this PC does not have yet",
                ))
                .clicked()
            {
                match numapilot::peers::pull_profiles(&p.name, false) {
                    Ok(m) => app.toast(m, true),
                    Err(e) => app.toast(e, false),
                }
            }
            if ui.small_button(crate::i18n::tr("remove")).clicked() {
                let _ = numapilot::peers::remove(&p.name);
            }
        });
    }
    if !peers.is_empty() {
        ui.horizontal(|ui| {
            if ui
                .button(crate::i18n::tr("Back up all now"))
                .on_hover_text(crate::i18n::tr("Stores the settings of each PC that is on in ~/numapilot-backups (the phone panel does this by itself once a day)"))
                .clicked()
            {
                app.start_job(crate::i18n::tr("PC backup"), vec![crate::jobs::sibling_abs("numapilot"), "peers".into(), "backup".into()]);
            }
        });
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut app.peer_link)
                .hint_text("http://192.168.0.x:8577/?t=...")
                .desired_width(320.0),
        );
        ui.add(
            egui::TextEdit::singleline(&mut app.peer_name)
                .hint_text(crate::i18n::tr("name (e.g. BC-250)"))
                .desired_width(140.0),
        );
        if ui.button(crate::i18n::tr("Add")).clicked() {
            let name = app.peer_name.trim().to_string();
            match numapilot::peers::add(&app.peer_link, (!name.is_empty()).then_some(name.as_str()))
            {
                Ok(p) => {
                    app.toast(crate::tf!("{} added", "{} adicionado", p.name), true);
                    app.peer_link.clear();
                    app.peer_name.clear();
                }
                Err(e) => app.toast(e, false),
            }
        }
    });
}

fn selfupdate_ui(app: &mut App, ui: &mut egui::Ui) {
    use numapilot::selfupdate::{self, How};
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("numapilot {}", env!("CARGO_PKG_VERSION"))).strong());
        let how = selfupdate::how();
        ui.label(
            RichText::new(match &how {
                How::Package(p) => crate::tf!("package {p}", "pacote {p}"),
                How::Source(d) => crate::tf!("code in {}", "código em {}", d.display()),
                How::Unknown => crate::i18n::tr("unknown source").into(),
            })
            .small()
            .weak(),
        );
        if ui
            .add_enabled(how != How::Unknown, egui::Button::new(crate::i18n::tr("Update numapilot")))
            .on_hover_text(crate::i18n::tr("Downloads the new version, rebuilds and reinstalls in a terminal (asks for the password). Then reopen the interface."))
            .clicked()
        {
            match selfupdate::command() {
                Ok(c) => {
                    if let Err(e) = crate::jobs::in_terminal(&c) {
                        app.toast(e, false);
                    }
                }
                Err(e) => app.toast(e, false),
            }
        }
    });
}

fn web_ui(app: &mut App, ui: &mut egui::Ui) {
    if app.web_checked.is_none_or(|t| t.elapsed().as_secs() >= 5) {
        app.web_active =
            crate::jobs::quick(&["systemctl", "--user", "is-active", "numapilot-web.service"])
                == "active";
        app.web_checked = Some(std::time::Instant::now());
    }
    ui.label(crate::i18n::tr("See temperatures and usage, switch a program's profile and turn on the lab/AI server from the phone (same network)."));
    let mut on = app.web_active;
    if ui
        .checkbox(
            &mut on,
            crate::i18n::tr("Turn on the panel (starts with the session)"),
        )
        .changed()
    {
        let verb = if on { "enable" } else { "disable" };
        app.start_job(
            crate::i18n::tr("Phone panel"),
            vec![
                "systemctl".into(),
                "--user".into(),
                verb.into(),
                "--now".into(),
                "numapilot-web.service".into(),
            ],
        );
        app.web_checked = None;
    }
    if app.web_active {
        match numapilot::web::url(numapilot::web::DEFAULT_PORT) {
            Some(u) => {
                ui.horizontal(|ui| {
                    qr(ui, &u);
                    ui.vertical(|ui| {
                        ui.label(crate::i18n::tr("Point the phone camera or open:"));
                        ui.monospace(&u);
                        if ui.button(crate::i18n::tr("Copy link")).clicked() {
                            ui.ctx().copy_text(u.clone());
                        }
                        ui.label(
                            RichText::new(crate::i18n::tr("The link contains the access password; do not share it. Away from home, use a VPN (e.g. Tailscale)."))
                                .small()
                                .weak(),
                        );
                    });
                });
            }
            None => {
                ui.label(crate::i18n::tr("No local network detected."));
            }
        }
    }
}

fn updates_ui(app: &mut App, ui: &mut egui::Ui) {
    use numapilot::labcfg::{self, LabSettings};
    let st = labcfg::state_dir();
    let avail: serde_json::Value = std::fs::read_to_string(st.join("available.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let mut cfg = LabSettings::load();
    ui.horizontal(|ui| {
        ui.label(crate::i18n::tr("When there is a new version:"));
        let mut mode = cfg.get("UPDATE_MODE");
        let before = mode.clone();
        ui.radio_value(
            &mut mode,
            "auto".to_string(),
            crate::i18n::tr("install if it passes validation"),
        );
        ui.radio_value(
            &mut mode,
            "aviso".to_string(),
            crate::i18n::tr("just notify me"),
        );
        if mode != before {
            cfg.set("UPDATE_MODE", &mode);
            match cfg.save_keys(&["UPDATE_MODE"]) {
                Ok(()) => app.toast(crate::i18n::tr("Update preference saved"), true),
                Err(e) => app.toast(e.to_string(), false),
            }
        }
    });
    ui.label(
        RichText::new(crate::tf!(
            "Before switching, each new version runs in a separate container and must: give the same result as the CPU on each MI50 \
             (matrices and convolution), read and write half the VRAM with no error, copy between the cards and not be more than {}% \
             slower than the current one (llama.cpp is measured in tokens/s). If it fails, the current version stays and the new one is not tried again.", "Antes de trocar, cada versão nova roda num container à parte e precisa: dar o mesmo resultado da CPU em cada MI50 \
             (matrizes e convolução), ler e escrever metade da VRAM sem erro, copiar entre as placas e não ficar mais de {}% \
             mais lenta que a atual (o llama.cpp é medido em tokens/s). Se falhar, fica a versão atual e a nova não é tentada de novo.",
            cfg.get("REGRESS_PCT")
        ))
        .small()
        .weak(),
    );
    if let Some(st) = avail["stack"].as_str() {
        let txt = match st {
            "gfx906" => crate::i18n::tr("MI50 with community ROCm"),
            "rocm" => crate::i18n::tr("AMD's official ROCm"),
            "cuda" => "NVIDIA (CUDA)",
            "vulkan" => crate::i18n::tr(
                "Vulkan (GPUs without ROCm: the lab uses the CPU and the AI server uses the GPU through Vulkan)",
            ),
            o => o,
        };
        ui.label(
            RichText::new(crate::tf!(
                "This PC's compute stack: {txt}",
                "Pilha de computação deste PC: {txt}"
            ))
            .small(),
        );
    }
    let comps = [
        ("lab", crate::i18n::tr("Lab (PyTorch)")),
        ("llm", crate::i18n::tr("AI server (llama.cpp)")),
        ("image", crate::i18n::tr("ROCm image")),
        ("rocm", crate::i18n::tr("Native ROCm")),
    ];
    egui::Grid::new("upd")
        .striped(true)
        .num_columns(4)
        .spacing([14.0, 4.0])
        .show(ui, |ui| {
            for h in [
                "",
                crate::i18n::tr("In use"),
                crate::i18n::tr("Available"),
                crate::i18n::tr("Pin"),
            ] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for (k, label) in comps {
                let a = &avail[k];
                if a.is_null() {
                    continue;
                }
                let short = |v: &serde_json::Value| {
                    v.as_str()
                        .unwrap_or("?")
                        .rsplit(':')
                        .next()
                        .unwrap_or("?")
                        .to_string()
                };
                let (cur, new) = (short(&a["current"]), short(&a["new"]));
                ui.label(label);
                ui.label(&cur);
                if new != cur {
                    ui.label(
                        RichText::new(crate::tf!("{new} (new)", "{new} (nova)"))
                            .color(Color32::from_rgb(230, 160, 40)),
                    );
                } else {
                    ui.label(
                        RichText::new(crate::i18n::tr("up to date"))
                            .color(Color32::from_rgb(90, 180, 110)),
                    );
                }
                let hold = labcfg::cfg_dir().join(format!("hold-{k}"));
                let mut held = hold.exists();
                if ui
                    .checkbox(&mut held, "")
                    .on_hover_text(crate::i18n::tr(
                        "Keeps the current version even with automatic updates",
                    ))
                    .changed()
                {
                    let _ = std::fs::create_dir_all(labcfg::cfg_dir());
                    let _ = if held {
                        std::fs::write(&hold, "interface\n")
                    } else {
                        std::fs::remove_file(&hold)
                    };
                }
                ui.end_row();
            }
        });
    if avail.is_null() {
        ui.label(RichText::new(crate::i18n::tr("Not checked yet.")).weak());
    } else if let Some(t) = avail["time"].as_str() {
        ui.label(
            RichText::new(crate::tf!(
                "Checked on {}",
                "Verificado em {}",
                t.get(..16).unwrap_or(t).replace('T', " ")
            ))
            .small()
            .weak(),
        );
    }
    ui.horizontal(|ui| {
        if ui.button(crate::i18n::tr("Check now")).clicked() {
            app.start_job(crate::i18n::tr("Looking for new versions"), gfx(&["check"]));
        }
        if ui
            .button(crate::i18n::tr("Update now (with validation)"))
            .clicked()
        {
            app.start_job(
                crate::i18n::tr("Updating with validation"),
                gfx(&["update-all"]),
            );
        }
        if ui
            .button(crate::i18n::tr("Validate the current versions"))
            .on_hover_text(crate::i18n::tr(
                "Runs the tests on the GPUs and saves the current performance as the reference",
            ))
            .clicked()
        {
            app.start_job(
                crate::i18n::tr("Validating the current versions"),
                gfx(&["validate"]),
            );
        }
        if let Some(u) = avail["releases"].as_str()
            && ui.button(crate::i18n::tr("Community news")).clicked()
        {
            let _ = crate::jobs::open_url(u);
        }
    });
    // history
    let hist = std::fs::read_to_string(st.join("history.jsonl")).unwrap_or_default();
    let rows: Vec<serde_json::Value> = hist
        .lines()
        .rev()
        .take(8)
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if !rows.is_empty() {
        ui.collapsing(crate::i18n::tr("Update history"), |ui| {
            egui::Grid::new("updh")
                .striped(true)
                .num_columns(5)
                .show(ui, |ui| {
                    for h in [
                        crate::i18n::tr("Date"),
                        crate::i18n::tr("What"),
                        crate::i18n::tr("Version"),
                        crate::i18n::tr("Result"),
                        crate::i18n::tr("Performance"),
                    ] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for r in rows {
                        let s = |k: &str| {
                            r[k].as_str()
                                .unwrap_or("")
                                .rsplit(':')
                                .next()
                                .unwrap_or("")
                                .to_string()
                        };
                        ui.label(r["time"].as_str().unwrap_or("").get(..10).unwrap_or(""));
                        ui.label(r["comp"].as_str().unwrap_or(""));
                        ui.label(format!("{} → {}", s("from"), s("to")));
                        let res = r["result"].as_str().unwrap_or("");
                        ui.label(RichText::new(res).color(if res == "ok" {
                            Color32::from_rgb(90, 180, 110)
                        } else {
                            Color32::from_rgb(230, 160, 40)
                        }));
                        let m = &r["metrics"];
                        let perf = if let Some(t) = m["tflops"].as_array() {
                            format!(
                                "fp16 {} TFLOPS",
                                t.iter()
                                    .filter_map(|x| x.as_f64())
                                    .map(|x| format!("{x:.1}"))
                                    .collect::<Vec<_>>()
                                    .join(" / ")
                            )
                        } else if m["tg"].is_number() {
                            format!("{} / {} tok/s", m["pp"], m["tg"])
                        } else {
                            String::new()
                        };
                        ui.label(perf);
                        ui.end_row();
                    }
                });
        });
    }
}

#[derive(Default)]
pub struct VmUi {
    gpu: String,
    checks: Vec<numapilot::vm::Check>,
    name: String,
    iso: String,
    mem: u32,
    cpus: usize,
    disk: u32,
}

fn vm_ui(app: &mut App, ui: &mut egui::Ui) {
    egui::CollapsingHeader::new(RichText::new(crate::i18n::tr("Windows virtual machine (games with anti-cheat)")).strong())
        .id_salt("vm")
        .show(ui, |ui| {
            ui.label(
                RichText::new(crate::i18n::tr("One graphics card is reserved for Windows in a VM (VFIO). Linux stays on the other. Some anti-cheats (e.g. Vanguard) refuse VMs: check the game first."))
                    .small()
                    .weak(),
            );
            let gpus: Vec<numapilot::topo::Gpu> = app.topo.gpus.iter().filter(|g| !g.is_integrated()).cloned().collect();
            let v = &mut app.vm_ui;
            if v.mem == 0 {
                (v.mem, v.cpus, v.disk, v.name) = (16, 8, 120, "windows".into());
            }
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("VM card:"));
                egui::ComboBox::from_id_salt("vmgpu")
                    .selected_text(if v.gpu.is_empty() { crate::i18n::tr("choose").to_string() } else { v.gpu.clone() })
                    .show_ui(ui, |ui| {
                        for g in &gpus {
                            ui.selectable_value(&mut v.gpu, g.alias.clone(), format!("{} ({})", g.alias, g.name));
                        }
                    });
                if ui.button(crate::i18n::tr("Check")).clicked() {
                    let g = gpus.iter().find(|g| g.alias == v.gpu);
                    v.checks = numapilot::vm::check(&app.topo, g);
                }
            });
            for c in &v.checks {
                ui.label(
                    RichText::new(format!(
                        "{} {}{}",
                        if c.ok { "ok   " } else { crate::i18n::tr("missing") },
                        c.name,
                        if c.detail.is_empty() { String::new() } else { format!(": {}", c.detail) }
                    ))
                    .color(if c.ok { Color32::from_rgb(110, 190, 120) } else { Color32::from_rgb(230, 160, 40) }),
                );
                if !c.ok
                    && let Some(f) = &c.fix
                {
                    ui.label(RichText::new(format!("    {f}")).small().monospace());
                }
            }
            let gsel = v.gpu.clone();
            let mut job: Option<(String, Vec<String>)> = None;
            let mut term: Option<Vec<String>> = None;
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!gsel.is_empty(), egui::Button::new(crate::i18n::tr("Prepare the card")))
                    .on_hover_text(crate::i18n::tr("Binds the card to vfio-pci at boot, enables the IOMMU and rebuilds the boot image (asks for the password). Reboot afterwards."))
                    .clicked()
                {
                    job = Some((crate::i18n::tr("Preparing the VM").into(), vec!["pkexec".into(), crate::jobs::sibling_abs("numapilot"), "vm".into(), "prepare".into(), gsel.clone()]));
                }
                if ui.button(crate::i18n::tr("Give back to Linux")).clicked() {
                    job = Some((crate::i18n::tr("Giving the card back").into(), vec!["pkexec".into(), crate::jobs::sibling_abs("numapilot"), "vm".into(), "unprepare".into()]));
                }
                if numapilot::containers::which("virt-manager") && ui.button(crate::i18n::tr("Open virt-manager")).clicked() {
                    let _ = crate::jobs::spawn_detached(&["virt-manager".into()]);
                }
            });
            egui::Grid::new("vmnew").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                ui.label(crate::i18n::tr("Name"));
                ui.add(egui::TextEdit::singleline(&mut v.name).desired_width(160.0));
                ui.end_row();
                ui.label(crate::i18n::tr("Windows ISO"));
                ui.add(egui::TextEdit::singleline(&mut v.iso).hint_text("/home/.../Win11.iso").desired_width(320.0));
                ui.end_row();
                ui.label(crate::i18n::tr("Memory / cores / disk"));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut v.mem).range(4..=256).suffix(" GB"));
                    ui.add(egui::DragValue::new(&mut v.cpus).range(2..=64).suffix(crate::i18n::tr(" cores")));
                    ui.add(egui::DragValue::new(&mut v.disk).range(64..=2000).suffix(" GB"));
                });
                ui.end_row();
            });
            if ui
                .add_enabled(!gsel.is_empty() && !v.iso.is_empty(), egui::Button::new(crate::i18n::tr("Create the VM")))
                .on_hover_text(crate::i18n::tr("UEFI, TPM, cores and memory from the card's socket; then install Windows through virt-manager"))
                .clicked()
            {
                term = Some(vec![
                    crate::jobs::sibling_abs("numapilot"),
                    "vm".into(),
                    "create".into(),
                    v.name.clone(),
                    "--gpu".into(),
                    gsel.clone(),
                    "--iso".into(),
                    v.iso.clone(),
                    "--mem".into(),
                    v.mem.to_string(),
                    "--cpus".into(),
                    v.cpus.to_string(),
                    "--disk".into(),
                    v.disk.to_string(),
                ]);
            }
            if let Some((t, c)) = job {
                app.start_job(&t, c);
            }
            if let Some(c) = term
                && let Err(e) = crate::jobs::in_terminal(&c)
            {
                app.toast(e, false);
            }
        });
}
