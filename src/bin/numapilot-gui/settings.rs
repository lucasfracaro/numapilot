//! Aba "Configurações": bandeja/início automático, atualização semanal, hardware.

use crate::jobs::home;
use crate::lab::gfx;
use crate::App;
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
                std::env::current_exe().map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|_| "numapilot-gui".into())
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
            .checkbox(&mut on, "Atualizar toda semana (laboratório, llama.cpp e ROCm), testando nas GPUs e voltando se falhar")
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
        if ui.button("Atualizar tudo agora").clicked() {
            app.start_job("Atualizando tudo", gfx(&["auto-update"]));
        }

        ui.add_space(8.0);
        ui.heading("Painel no celular");
        web_ui(app, ui);

        ui.add_space(8.0);
        ui.heading("Diagnóstico");
        ui.label("Testa cada parte no seu hardware (NUMA, cgroups, seleção de GPU no Vulkan, ROCm, ventoinhas, OverDrive, ReBAR, Steam) e diz o que corrigir.");
        ui.horizontal(|ui| {
            if ui.button("Verificar tudo").clicked() {
                app.start_job("Diagnóstico", vec![crate::jobs::sibling("numapilot"), "doctor".into()]);
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
    let Ok(code) = qrcode::QrCode::new(text.as_bytes()) else { return };
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
            p.rect_filled(egui::Rect::from_min_size(r.min + egui::vec2(x * px, y * px), egui::vec2(px, px)), 0.0, Color32::BLACK);
        }
    }
}

fn web_ui(app: &mut App, ui: &mut egui::Ui) {
    if app.web_checked.is_none_or(|t| t.elapsed().as_secs() >= 5) {
        app.web_active = crate::jobs::quick(&["systemctl", "--user", "is-active", "numapilot-web.service"]) == "active";
        app.web_checked = Some(std::time::Instant::now());
    }
    ui.label("Veja temperaturas e uso, troque o perfil de um programa e ligue o laboratório/servidor de IA pelo celular (mesma rede).");
    let mut on = app.web_active;
    if ui.checkbox(&mut on, "Ligar o painel (inicia junto com a sessão)").changed() {
        let verb = if on { "enable" } else { "disable" };
        app.start_job(
            "Painel no celular",
            vec!["systemctl".into(), "--user".into(), verb.into(), "--now".into(), "numapilot-web.service".into()],
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
