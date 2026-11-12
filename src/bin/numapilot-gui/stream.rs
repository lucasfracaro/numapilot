//! Aba "Streaming": Sunshine (jogar de outro aparelho com Moonlight) e qual GPU codifica o vídeo.

use crate::App;
use crate::jobs::open_url;
use eframe::egui::{self, Color32, RichText};
use std::path::Path;

/// Lê `adapter_name` do sunshine.conf.
pub fn read_adapter(conf: &Path) -> Option<String> {
    read_key(conf, "adapter_name")
}

pub fn read_key(conf: &Path, key: &str) -> Option<String> {
    std::fs::read_to_string(conf).ok()?.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

/// Grava (ou remove, se None) `adapter_name` no sunshine.conf, preservando o resto.
pub fn write_adapter(conf: &Path, dev: Option<&str>) -> std::io::Result<()> {
    write_key(conf, "adapter_name", dev)
}

pub fn write_key(conf: &Path, key: &str, val: Option<&str>) -> std::io::Result<()> {
    let cur = std::fs::read_to_string(conf).unwrap_or_default();
    let mut out: Vec<String> = cur
        .lines()
        .filter(|l| {
            l.split_once('=')
                .map(|(k, _)| k.trim() != key)
                .unwrap_or(true)
        })
        .map(String::from)
        .collect();
    if let Some(d) = val {
        out.push(format!("{key} = {d}"));
    }
    if let Some(p) = conf.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(conf, out.join("\n") + "\n")
}

fn ctl_args(s: &crate::bg::Sunshine, action: &str) -> Option<Vec<String>> {
    if let Some(u) = &s.unit {
        return Some(vec![
            "systemctl".into(),
            "--user".into(),
            action.into(),
            u.clone(),
        ]);
    }
    match (action, s.install.as_deref()) {
        ("stop", _) => Some(vec!["pkill".into(), "-x".into(), "sunshine".into()]),
        ("start", Some("flatpak")) => Some(vec![
            "flatpak".into(),
            "run".into(),
            "dev.lizardbyte.app.Sunshine".into(),
        ]),
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
        ui.heading(crate::i18n::tr("Streaming de jogos (Sunshine)"));
        if sun.install.is_some() {
            let (t, c) = if sun.active {
                (crate::i18n::tr("ligado"), Color32::from_rgb(90, 180, 110))
            } else {
                (crate::i18n::tr("desligado"), Color32::GRAY)
            };
            ui.label(RichText::new(t).color(c));
        }
    });
    ui.label(
        RichText::new(
            crate::i18n::tr("Jogue no celular, notebook ou TV pelo Moonlight. O jogo roda aqui e uma GPU (ou a CPU) \
             codifica o vídeo."),
        )
        .weak(),
    );
    ui.separator();

    let Some(how) = sun.install.clone() else {
        ui.label(crate::i18n::tr("O Sunshine não está instalado."));
        ui.horizontal(|ui| {
            if ui.button(crate::i18n::tr("Instalar (Flatpak)")).clicked() {
                app.start_job(
                    crate::i18n::tr("Instalando o Sunshine"),
                    vec![
                        "flatpak".into(),
                        "install".into(),
                        "-y".into(),
                        "--user".into(),
                        "flathub".into(),
                        "dev.lizardbyte.app.Sunshine".into(),
                    ],
                );
            }
            ui.label(
                RichText::new(crate::i18n::tr(
                    "ou pelo AUR: paru -S sunshine (ou o seu fork)",
                ))
                .weak(),
            );
        });
        return;
    };

    ui.horizontal(|ui| {
        ui.label(RichText::new(crate::tf!("instalado ({how})", "installed ({how})")).weak());
        if sun.active {
            if ui.button(crate::i18n::tr("Desligar")).clicked()
                && let Some(a) = ctl_args(&sun, "stop")
            {
                app.start_job(crate::i18n::tr("Desligando o Sunshine"), a);
            }
            if ui.button(crate::i18n::tr("Reiniciar")).clicked()
                && let Some(a) = ctl_args(&sun, "restart").or_else(|| ctl_args(&sun, "start"))
            {
                app.start_job(crate::i18n::tr("Reiniciando o Sunshine"), a);
            }
        } else if ui.button(crate::i18n::tr("Ligar")).clicked()
            && let Some(a) = ctl_args(&sun, "start")
        {
            if sun.unit.is_some() {
                app.start_job(crate::i18n::tr("Ligando o Sunshine"), a);
            } else {
                let _ = crate::jobs::spawn_detached(&a);
                app.toast(crate::i18n::tr("Ligando o Sunshine"), true);
            }
        }
        if ui.button(crate::i18n::tr("Painel do Sunshine")).clicked() {
            let _ = open_url("https://localhost:47990");
        }
    });
    ui.add_space(8.0);

    let Some(conf) = sun.conf.clone() else { return };
    let cur = read_adapter(&conf);
    ui.label(RichText::new(crate::i18n::tr("GPU que codifica o vídeo")).strong());
    egui::Grid::new("enc")
        .num_columns(3)
        .striped(true)
        .spacing([14.0, 6.0])
        .show(ui, |ui| {
            let mut sel = cur.clone();
            ui.radio_value(&mut sel, None, crate::i18n::tr("Automático"));
            ui.label(crate::i18n::tr("o Sunshine escolhe"));
            ui.label("");
            ui.end_row();
            for g in &app.topo.gpus {
                let Some(r) = &g.render else { continue };
                let dev = format!("/dev/dri/{r}");
                ui.radio_value(&mut sel, Some(dev.clone()), format!("{} ({dev})", g.alias));
                ui.label(
                    vaapi
                        .get(r)
                        .cloned()
                        .unwrap_or_else(|| "verificando...".into()),
                );
                ui.label(
                    RichText::new(if g.display_connected {
                        crate::i18n::tr("monitor ligado")
                    } else {
                        ""
                    })
                    .weak(),
                );
                ui.end_row();
            }
            if sel != cur {
                match write_adapter(&conf, sel.as_deref()) {
                    Ok(()) => {
                        app.toast(
                            crate::i18n::tr(
                                "GPU de codificação salva; reinicie o Sunshine para valer",
                            ),
                            true,
                        );
                    }
                    Err(e) => app.toast(
                        crate::tf!(
                            "Não consegui salvar {}: {e}",
                            "Could not save {}: {e}",
                            conf.display()
                        ),
                        false,
                    ),
                }
            }
        });
    ui.add_space(8.0);
    ui.label(RichText::new(crate::i18n::tr("Codificador")).strong());
    let rec = numapilot::recipes::load();
    let no_enc: Vec<String> = rec
        .boards(&app.topo)
        .into_iter()
        .filter(|(b, _)| b.no_video_encoder)
        .map(|(b, _)| b.name.clone())
        .collect();
    if !no_enc.is_empty() {
        ui.label(
            RichText::new(crate::tf!(
                "{}: não tem codificador de vídeo funcionando. Use o codificador por software (CPU) abaixo, ou instale o driver VA-API da comunidade pelo BC250 Control Center (H.264 na GPU).", "{}: has no working video encoder. Use the software encoder (CPU) below, or install the community VA-API driver through BC250 Control Center (H.264 on the GPU).",
                no_enc.join(", ")
            ))
            .color(Color32::from_rgb(230, 160, 40)),
        );
    }
    let enc = read_key(&conf, "encoder");
    let preset = read_key(&conf, "sw_preset");
    ui.horizontal(|ui| {
        let mut e = enc.clone();
        egui::ComboBox::from_id_salt("sunenc")
            .selected_text(match e.as_deref() {
                None => crate::i18n::tr("automático"),
                Some("software") => "software (CPU, x264)",
                Some("vaapi") => "VA-API (AMD/Intel)",
                Some("nvenc") => "NVENC (NVIDIA)",
                Some("quicksync") => "Quick Sync (Intel)",
                Some(o) => o,
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut e, None, crate::i18n::tr("automático"));
                ui.selectable_value(&mut e, Some("vaapi".into()), "VA-API (AMD/Intel)");
                ui.selectable_value(&mut e, Some("nvenc".into()), "NVENC (NVIDIA)");
                ui.selectable_value(&mut e, Some("quicksync".into()), "Quick Sync (Intel)");
                ui.selectable_value(&mut e, Some("software".into()), "software (CPU, x264)");
            });
        if e != enc {
            match write_key(&conf, "encoder", e.as_deref()) {
                Ok(()) => app.toast(
                    crate::i18n::tr("Codificador salvo; reinicie o Sunshine"),
                    true,
                ),
                Err(err) => app.toast(err.to_string(), false),
            }
        }
        if e.as_deref() == Some("software") {
            let mut p = preset.clone();
            ui.label(crate::i18n::tr("velocidade:"));
            egui::ComboBox::from_id_salt("sunpreset")
                .selected_text(
                    p.clone()
                        .unwrap_or_else(|| crate::i18n::tr("padrão (superfast)").into()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut p, None, crate::i18n::tr("padrão (superfast)"));
                    for x in ["ultrafast", "superfast", "veryfast", "faster", "fast"] {
                        ui.selectable_value(&mut p, Some(x.to_string()), x);
                    }
                });
            if p != preset {
                let _ = write_key(&conf, "sw_preset", p.as_deref());
                let _ = write_key(&conf, "sw_tune", Some("zerolatency"));
                app.toast(
                    crate::i18n::tr("Velocidade salva; reinicie o Sunshine"),
                    true,
                );
            }
        }
    });
    ui.label(
        RichText::new(crate::i18n::tr("Software: ultrafast gasta menos CPU (imagem pior); veryfast fica mais bonito mas pesa. Comece em superfast a 1080p60."))
            .small()
            .weak(),
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            crate::i18n::tr("Dica: escolha a GPU que tem o monitor do jogo, se ela codificar H.264/HEVC. Para jogos na MI50 \
             a imagem é capturada dela; para menos atraso, deixe o jogo e a codificação na mesma placa."),
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
        std::fs::write(
            &p,
            "sunshine_name = srv\nadapter_name = /dev/dri/renderD128\n",
        )
        .unwrap();
        assert_eq!(read_adapter(&p).as_deref(), Some("/dev/dri/renderD128"));
        write_adapter(&p, Some("/dev/dri/renderD130")).unwrap();
        assert_eq!(read_adapter(&p).as_deref(), Some("/dev/dri/renderD130"));
        assert!(
            std::fs::read_to_string(&p)
                .unwrap()
                .contains("sunshine_name = srv")
        );
        write_adapter(&p, None).unwrap();
        assert_eq!(read_adapter(&p), None);
    }
}
