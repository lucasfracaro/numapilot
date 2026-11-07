//! Aba "Configurações": bandeja/início automático, atualização semanal, hardware.

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
                std::env::current_exe()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "numapilot-gui".into())
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
        ui.heading("Geral");
        ui.horizontal(|ui| {
            ui.label("Serviço numapilot:");
            if daemon_ok {
                ui.label(RichText::new("rodando").color(Color32::from_rgb(90, 180, 110)));
            } else {
                ui.label(RichText::new("parado — sudo systemctl enable --now numapilot").color(Color32::from_rgb(220, 80, 60)));
            }
        });
        let mut tray = autostart_path().exists();
        if ui.checkbox(&mut tray, "Ícone na bandeja ao iniciar a sessão (atalhos para o laboratório, IA e esta janela)").changed() {
            match set_autostart(tray) {
                Ok(()) => {
                    if tray {
                        let _ = crate::jobs::spawn_detached(&[
                            std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
                            "--tray".into(),
                        ]);
                    }
                    app.toast(if tray { "Ícone na bandeja ativado" } else { "Ícone na bandeja não abre mais sozinho" }, true);
                }
                Err(e) => app.toast(e.to_string(), false),
            }
        }

        ui.add_space(8.0);
        ui.heading("Atualizações");
        let mut on = auto;
        if ui
            .checkbox(&mut on, "Verificar toda semana (laboratório, llama.cpp e ROCm)")
            .changed()
        {
            app.start_job(
                "Atualização semanal",
                gfx(&["autoupdate", if on { "enable" } else { "disable" }]),
            );
        }
        if let Some(l) = last {
            let ok = l["ok"].as_bool().unwrap_or(false);
            ui.label(
                RichText::new(format!(
                    "Última: {} — {}",
                    l["time"].as_str().unwrap_or("?"),
                    l["summary"].as_str().unwrap_or("").replace(';', ", ")
                ))
                .color(if ok { Color32::from_rgb(90, 180, 110) } else { Color32::from_rgb(230, 160, 40) }),
            );
        }
        updates_ui(app, ui);

        ui.add_space(8.0);
        ui.heading("Painel no celular");
        web_ui(app, ui);

        peers_ui(app, ui);
        ui.add_space(8.0);
        notify_ui(app, ui);

        ui.add_space(8.0);
        ui.heading("Opções de boot");
        let od = numapilot::bootcfg::overdrive_state();
        ui.horizontal(|ui| {
            let mut on = od.on_next_boot;
            if ui
                .add_enabled(!od.from_cmdline, egui::Checkbox::new(&mut on, "OverDrive das placas AMD (undervolt/overclock)"))
                .changed()
            {
                app.start_job(
                    if on { "Ativando o OverDrive" } else { "Desativando o OverDrive" },
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
                (true, true) => "ligado".to_string(),
                (false, true) => "liga ao reiniciar".into(),
                (true, false) => "desliga ao reiniciar".into(),
                (false, false) => "desligado".into(),
            };
            ui.label(RichText::new(txt).weak());
        });
        if od.from_cmdline {
            ui.label(RichText::new("Definido na linha do kernel (amdgpu.ppfeaturemask): mude no bootloader ou remova de lá para controlar por aqui.").small().weak());
        }
        ui.label(
            RichText::new("Fica em /etc/modprobe.d/numapilot.conf e a imagem de boot é refeita: vale para qualquer kernel, inclusive depois de atualizar. Pede a senha.")
                .small()
                .weak(),
        );

        ui.add_space(8.0);
        ui.heading("Diagnóstico");
        ui.label("Testa cada parte no seu hardware (NUMA, cgroups, seleção de GPU no Vulkan, ROCm, ventoinhas, OverDrive, ReBAR, Steam) e diz o que corrigir.");
        ui.horizontal(|ui| {
            if ui.button("Verificar tudo").clicked() {
                app.start_job("Diagnóstico", vec![crate::jobs::sibling("numapilot"), "doctor".into()]);
            }
            if ui
                .button("Fazer backup das configurações")
                .on_hover_text("Salva ~/numapilot-backup-DATA.tar.gz com perfis, ajustes da máquina, ventoinhas, GPUs, laboratório e parâmetros do kernel. Guarde fora do disco do sistema; para restaurar: ./install.sh --restore ARQUIVO")
                .clicked()
            {
                app.start_job("Backup das configurações", vec![crate::jobs::sibling("numapilot"), "backup".into()]);
            }
            if ui.button("Gerar relatório para enviar").on_hover_text("Salva ~/numapilot-relatorio.txt com o diagnóstico e as informações do sistema").clicked() {
                app.start_job("Relatório de diagnóstico", vec![crate::jobs::sibling("numapilot"), "doctor".into(), "--report".into()]);
            }
        });

        ui.add_space(8.0);
        ui.heading("Hardware");
        for n in &app.topo.nodes {
            ui.label(format!(
                "Socket {} — {} threads ({} núcleos físicos), RAM {:.0} GB, CPUs {}",
                n.id,
                n.cpus.len(),
                n.physical_cores,
                n.mem_total_kb as f64 / 1048576.0,
                numapilot::util::format_cpulist(&n.cpus)
            ));
        }
        for g in &app.topo.gpus {
            let role = match g.role {
                GpuRole::Display => "vídeo (monitor ligado)",
                GpuRole::Compute if g.display_connected => "computação + monitor ligado nela",
                GpuRole::Compute => "computação; também roda jogos via PRIME",
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
            ui.label(RichText::new(format!("Aviso: {w}")).color(Color32::from_rgb(230, 160, 40)));
        }
        ui.add_space(8.0);
        ui.heading("Dicas");
        ui.label("• Jogo numa MI50: escolha \"MI50 — jogo/gráfico\" na aba Programas.");
        ui.label("• Lossless Scaling (lsfg-vk): o oficial gera os quadros na mesma GPU do jogo.");
        ui.label("• Configurações avançadas: /etc/numapilot.toml (regras) e /etc/gfx906-rocm.conf (laboratório/ROCm).");
    });
}

/// Desenha um QR code (para abrir o painel no celular apontando a câmera).
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
    ui.label(RichText::new("Avisos no celular").strong());
    ui.label(
        RichText::new("Pelo app ntfy (Android/iOS, grátis): GPU quente, disco falhando, servidor de IA caiu, PC saiu da rede, undervolt pronto. Instale o ntfy e assine o tópico (ou leia o QR code).")
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
    ui.checkbox(&mut n.enabled, "Ligado");
    egui::Grid::new("ntfy").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
        ui.label("Servidor");
        ui.add(egui::TextEdit::singleline(&mut n.server).desired_width(260.0));
        ui.end_row();
        ui.label("Tópico");
        ui.add(egui::TextEdit::singleline(&mut n.topic).desired_width(260.0));
        ui.end_row();
        ui.label("Token (opcional)");
        let mut t = n.token.clone().unwrap_or_default();
        ui.add(egui::TextEdit::singleline(&mut t).password(true).desired_width(260.0));
        n.token = (!t.is_empty()).then_some(t);
        ui.end_row();
    });
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut n.gpu_hot, "GPU quente");
        ui.add_enabled(n.gpu_hot, egui::DragValue::new(&mut n.gpu_hot_c).range(70.0..=105.0).suffix(" °C"));
        ui.checkbox(&mut n.health, "discos/memória/bateria");
        ui.checkbox(&mut n.llm, "servidor de IA");
        ui.checkbox(&mut n.peers, "outros PCs");
        ui.checkbox(&mut n.jobs, "tarefas prontas");
    });
    let n = n.clone();
    ui.horizontal(|ui| {
        if ui.add_enabled(n != saved, egui::Button::new("Salvar")).clicked() {
            app.send(numapilot::ctl::Request::SetNotify { notify: n.clone() });
        }
        if ui.button("Mandar teste").clicked() {
            match n.send_now("Teste do numapilot", "Se chegou, os avisos estão funcionando.", 3, &["white_check_mark"]) {
                Ok(()) => app.toast("Aviso de teste enviado", true),
                Err(e) => app.toast(e, false),
            }
        }
        if ui.small_button("abrir no navegador").clicked() {
            let _ = crate::jobs::open_url(&n.subscribe_url());
        }
    });
    if !n.topic.is_empty() {
        ui.horizontal(|ui| {
            qr(ui, &n.subscribe_url());
            ui.label(RichText::new(format!("Assine no app: {}\nOs avisos de IA e de outros PCs precisam do painel no celular ligado.", n.subscribe_url())).small().weak());
        });
    }
}

fn peers_ui(app: &mut App, ui: &mut egui::Ui) {
    ui.label(RichText::new("Meus outros PCs").strong());
    ui.label(
        RichText::new("Cole o link do painel de outro PC (Configurações → Painel no celular, lá nele): o painel deste mostra todos juntos.")
            .small()
            .weak(),
    );
    let peers = numapilot::peers::load();
    for p in &peers {
        ui.horizontal(|ui| {
            ui.label(&p.name);
            ui.label(RichText::new(&p.url).small().weak());
            if ui.small_button("abrir").clicked() {
                let _ = crate::jobs::open_url(&p.link());
            }
            if p.mac.is_some()
                && ui
                    .small_button("ligar")
                    .on_hover_text("Manda o pacote do Wake-on-LAN")
                    .clicked()
            {
                match numapilot::peers::wake(&p.name) {
                    Ok(m) => app.toast(m, true),
                    Err(e) => app.toast(e, false),
                }
            }
            if ui
                .small_button("copiar perfis")
                .on_hover_text("Traz os perfis de programa (Ajustar…) que este PC ainda não tem")
                .clicked()
            {
                match numapilot::peers::pull_profiles(&p.name, false) {
                    Ok(m) => app.toast(m, true),
                    Err(e) => app.toast(e, false),
                }
            }
            if ui.small_button("remover").clicked() {
                let _ = numapilot::peers::remove(&p.name);
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
                .hint_text("nome (ex.: BC-250)")
                .desired_width(140.0),
        );
        if ui.button("Adicionar").clicked() {
            let name = app.peer_name.trim().to_string();
            match numapilot::peers::add(&app.peer_link, (!name.is_empty()).then_some(name.as_str()))
            {
                Ok(p) => {
                    app.toast(format!("{} adicionado", p.name), true);
                    app.peer_link.clear();
                    app.peer_name.clear();
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
    ui.label("Veja temperaturas e uso, troque o perfil de um programa e ligue o laboratório/servidor de IA pelo celular (mesma rede).");
    let mut on = app.web_active;
    if ui
        .checkbox(&mut on, "Ligar o painel (inicia junto com a sessão)")
        .changed()
    {
        let verb = if on { "enable" } else { "disable" };
        app.start_job(
            "Painel no celular",
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
                        ui.label("Aponte a câmera do celular ou abra:");
                        ui.monospace(&u);
                        if ui.button("Copiar link").clicked() {
                            ui.ctx().copy_text(u.clone());
                        }
                        ui.label(
                            RichText::new("O link tem a senha de acesso; não compartilhe. Fora de casa, use uma VPN (ex.: Tailscale).")
                                .small()
                                .weak(),
                        );
                    });
                });
            }
            None => {
                ui.label("Sem rede local detectada.");
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
        ui.label("Quando houver versão nova:");
        let mut mode = cfg.get("UPDATE_MODE");
        let before = mode.clone();
        ui.radio_value(
            &mut mode,
            "auto".to_string(),
            "instalar se passar na validação",
        );
        ui.radio_value(&mut mode, "aviso".to_string(), "só me avisar");
        if mode != before {
            cfg.set("UPDATE_MODE", &mode);
            match cfg.save_keys(&["UPDATE_MODE"]) {
                Ok(()) => app.toast("Preferência de atualização salva", true),
                Err(e) => app.toast(e.to_string(), false),
            }
        }
    });
    ui.label(
        RichText::new(format!(
            "Antes de trocar, cada versão nova roda num container à parte e precisa: dar o mesmo resultado da CPU em cada MI50 \
             (matrizes e convolução), ler e escrever metade da VRAM sem erro, copiar entre as placas e não ficar mais de {}% \
             mais lenta que a atual (o llama.cpp é medido em tokens/s). Se falhar, fica a versão atual e a nova não é tentada de novo.",
            cfg.get("REGRESS_PCT")
        ))
        .small()
        .weak(),
    );
    if let Some(st) = avail["stack"].as_str() {
        let txt = match st {
            "gfx906" => "MI50 com ROCm da comunidade",
            "rocm" => "ROCm oficial da AMD",
            "cuda" => "NVIDIA (CUDA)",
            "vulkan" => {
                "Vulkan (GPUs sem ROCm: o laboratório usa a CPU e o servidor de IA usa a GPU pelo Vulkan)"
            }
            o => o,
        };
        ui.label(RichText::new(format!("Pilha de computação deste PC: {txt}")).small());
    }
    let comps = [
        ("lab", "Laboratório (PyTorch)"),
        ("llm", "Servidor de IA (llama.cpp)"),
        ("image", "Imagem ROCm"),
        ("rocm", "ROCm nativo"),
    ];
    egui::Grid::new("upd")
        .striped(true)
        .num_columns(4)
        .spacing([14.0, 4.0])
        .show(ui, |ui| {
            for h in ["", "Em uso", "Disponível", "Travar"] {
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
                        RichText::new(format!("{new} (nova)"))
                            .color(Color32::from_rgb(230, 160, 40)),
                    );
                } else {
                    ui.label(RichText::new("em dia").color(Color32::from_rgb(90, 180, 110)));
                }
                let hold = labcfg::cfg_dir().join(format!("hold-{k}"));
                let mut held = hold.exists();
                if ui
                    .checkbox(&mut held, "")
                    .on_hover_text("Mantém a versão atual mesmo com atualização automática")
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
        ui.label(RichText::new("Ainda não verificado.").weak());
    } else if let Some(t) = avail["time"].as_str() {
        ui.label(
            RichText::new(format!(
                "Verificado em {}",
                t.get(..16).unwrap_or(t).replace('T', " ")
            ))
            .small()
            .weak(),
        );
    }
    ui.horizontal(|ui| {
        if ui.button("Verificar agora").clicked() {
            app.start_job("Procurando versões novas", gfx(&["check"]));
        }
        if ui.button("Atualizar agora (com validação)").clicked() {
            app.start_job("Atualizando com validação", gfx(&["update-all"]));
        }
        if ui
            .button("Validar as versões atuais")
            .on_hover_text("Roda os testes nas GPUs e grava o desempenho de agora como referência")
            .clicked()
        {
            app.start_job("Validando as versões atuais", gfx(&["validate"]));
        }
        if let Some(u) = avail["releases"].as_str()
            && ui.button("Novidades da comunidade").clicked()
        {
            let _ = crate::jobs::open_url(u);
        }
    });
    // histórico
    let hist = std::fs::read_to_string(st.join("history.jsonl")).unwrap_or_default();
    let rows: Vec<serde_json::Value> = hist
        .lines()
        .rev()
        .take(8)
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    if !rows.is_empty() {
        ui.collapsing("Histórico das atualizações", |ui| {
            egui::Grid::new("updh")
                .striped(true)
                .num_columns(5)
                .show(ui, |ui| {
                    for h in ["Data", "O quê", "Versão", "Resultado", "Desempenho"] {
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
