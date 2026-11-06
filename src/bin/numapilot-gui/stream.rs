//! Aba "Streaming": Sunshine (jogar de outro aparelho com Moonlight) e qual GPU codifica o vídeo.

use crate::jobs::open_url;
use crate::App;
use eframe::egui::{self, Color32, RichText};
use std::path::Path;

/// Lê `adapter_name` do sunshine.conf.
pub fn read_adapter(conf: &Path) -> Option<String> {
    std::fs::read_to_string(conf).ok()?.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == "adapter_name").then(|| v.trim().to_string())
    })
}

/// Grava (ou remove, se None) `adapter_name` no sunshine.conf, preservando o resto.
pub fn write_adapter(conf: &Path, dev: Option<&str>) -> std::io::Result<()> {
    let cur = std::fs::read_to_string(conf).unwrap_or_default();
    let mut out: Vec<String> = cur
        .lines()
        .filter(|l| l.split_once('=').map(|(k, _)| k.trim() != "adapter_name").unwrap_or(true))
        .map(String::from)
        .collect();
    if let Some(d) = dev {
        out.push(format!("adapter_name = {d}"));
    }
    if let Some(p) = conf.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(conf, out.join("\n") + "\n")
}

fn ctl_args(s: &crate::bg::Sunshine, action: &str) -> Option<Vec<String>> {
    if let Some(u) = &s.unit {
        return Some(vec!["systemctl".into(), "--user".into(), action.into(), u.clone()]);
    }
    match (action, s.install.as_deref()) {
        ("stop", _) => Some(vec!["pkill".into(), "-x".into(), "sunshine".into()]),
        ("start", Some("flatpak")) => Some(vec!["flatpak".into(), "run".into(), "dev.lizardbyte.app.Sunshine".into()]),
        ("start", Some(_)) => Some(vec!["sunshine".into()]),
        _ => None,
    }
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    let (sun, vaapi) = match app.bg.lock() {
        Ok(b) => (b.sunshine.clone(), b.vaapi.clone()),
        Err(_) => return,
    };
    ui.horizontal(|ui| {
        ui.heading("Streaming de jogos (Sunshine)");
        if sun.install.is_some() {
            let (t, c) = if sun.active { ("ligado", Color32::from_rgb(90, 180, 110)) } else { ("desligado", Color32::GRAY) };
            ui.label(RichText::new(t).color(c));
        }
    });
    ui.label(
        RichText::new(
            "Jogue no celular, notebook ou TV pelo Moonlight. O jogo roda aqui (na MI50 ou na RX470) \
             e uma GPU codifica o vídeo.",
        )
        .weak(),
    );
    ui.separator();

    let Some(how) = sun.install.clone() else {
        ui.label("O Sunshine não está instalado.");
        ui.horizontal(|ui| {
            if ui.button("Instalar (Flatpak)").clicked() {
                app.start_job(
                    "Instalando o Sunshine",
                    vec!["flatpak".into(), "install".into(), "-y".into(), "--user".into(), "flathub".into(), "dev.lizardbyte.app.Sunshine".into()],
                );
            }
            ui.label(RichText::new("ou pelo AUR: paru -S sunshine (ou o seu fork)").weak());
        });
        return;
    };

    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("instalado ({how})")).weak());
        if sun.active {
            if ui.button("Desligar").clicked()
                && let Some(a) = ctl_args(&sun, "stop")
            {
                app.start_job("Desligando o Sunshine", a);
            }
            if ui.button("Reiniciar").clicked()
                && let Some(a) = ctl_args(&sun, "restart").or_else(|| ctl_args(&sun, "start"))
            {
                app.start_job("Reiniciando o Sunshine", a);
            }
        } else if ui.button("Ligar").clicked()
            && let Some(a) = ctl_args(&sun, "start")
        {
            if sun.unit.is_some() {
                app.start_job("Ligando o Sunshine", a);
            } else {
                let _ = crate::jobs::spawn_detached(&a);
                app.toast("Ligando o Sunshine", true);
            }
        }
        if ui.button("Painel do Sunshine").clicked() {
            let _ = open_url("https://localhost:47990");
        }
    });
    ui.add_space(8.0);

    let Some(conf) = sun.conf.clone() else { return };
    let cur = read_adapter(&conf);
    ui.label(RichText::new("GPU que codifica o vídeo").strong());
    egui::Grid::new("enc").num_columns(3).striped(true).spacing([14.0, 6.0]).show(ui, |ui| {
        let mut sel = cur.clone();
        ui.radio_value(&mut sel, None, "Automático");
        ui.label("o Sunshine escolhe");
        ui.label("");
        ui.end_row();
        for g in &app.topo.gpus {
            let Some(r) = &g.render else { continue };
            let dev = format!("/dev/dri/{r}");
            ui.radio_value(&mut sel, Some(dev.clone()), format!("{} ({dev})", g.alias));
            ui.label(vaapi.get(r).cloned().unwrap_or_else(|| "verificando...".into()));
            ui.label(RichText::new(if g.display_connected { "monitor ligado" } else { "" }).weak());
            ui.end_row();
        }
        if sel != cur {
            match write_adapter(&conf, sel.as_deref()) {
                Ok(()) => {
                    app.toast("GPU de codificação salva; reinicie o Sunshine para valer", true);
                }
                Err(e) => app.toast(format!("Não consegui salvar {}: {e}", conf.display()), false),
            }
        }
    });
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            "Dica: escolha a GPU que tem o monitor do jogo, se ela codificar H.264/HEVC. Para jogos na MI50 \
             a imagem é capturada dela; para menos atraso, deixe o jogo e a codificação na mesma placa.",
        )
        .small()
        .weak(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_roundtrip() {
        let p = std::env::temp_dir().join("numapilot-sunshine-test.conf");
        std::fs::write(&p, "sunshine_name = srv\nadapter_name = /dev/dri/renderD128\n").unwrap();
        assert_eq!(read_adapter(&p).as_deref(), Some("/dev/dri/renderD128"));
        write_adapter(&p, Some("/dev/dri/renderD130")).unwrap();
        assert_eq!(read_adapter(&p).as_deref(), Some("/dev/dri/renderD130"));
        assert!(std::fs::read_to_string(&p).unwrap().contains("sunshine_name = srv"));
        write_adapter(&p, None).unwrap();
        assert_eq!(read_adapter(&p), None);
    }
}
