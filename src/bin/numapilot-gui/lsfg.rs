//! lsfg-vk setup panel: what frame generation still needs on this machine, with a button per
//! missing piece (AUR install in a terminal, Lossless Scaling through Steam, configurator, check).

use crate::App;
use eframe::egui::{self, RichText};
use numapilot::lsfg;

/// `open` = start expanded even when everything is ready.
pub fn panel(app: &mut App, ui: &mut egui::Ui, id: &str, open: bool) {
    // pacman -Qq and the Steam libraries: in a thread, every 5 s (picks up a finished install)
    let Some(s) = crate::bg::cached(
        "lsfg-status",
        std::time::Duration::from_secs(5),
        ui.ctx(),
        lsfg::status,
    ) else {
        return;
    };
    let p = crate::theme::pal(ui);
    let lines = lsfg::report(&s);
    let missing = lines.iter().filter(|l| !l.0).count();
    let head = if missing == 0 {
        crate::i18n::tr("Lossless Scaling (lsfg-vk): ready").to_string()
    } else {
        crate::tf!(
            "Lossless Scaling (lsfg-vk): {} missing",
            "Lossless Scaling (lsfg-vk): falta(m) {}",
            missing
        )
    };
    egui::CollapsingHeader::new(RichText::new(head).color(if missing == 0 {
        p.ok
    } else {
        p.warn
    }))
    .id_salt(("lsfg-panel", id))
    .default_open(open || missing > 0)
    .show(ui, |ui| {
        for (ok, l) in &lines {
            ui.horizontal_wrapped(|ui| {
                crate::theme::chip(
                    ui,
                    if *ok {
                        crate::t2!("ready", "pronto")
                    } else {
                        crate::i18n::tr("missing")
                    },
                    if *ok { p.ok } else { p.warn },
                );
                ui.label(RichText::new(l).small());
            });
        }
        ui.horizontal_wrapped(|ui| buttons(app, ui, &s));
        ui.label(
            RichText::new(crate::i18n::tr(
                "lsfg-vk is not part of numapilot (its license forbids redistributing it): the \
                     buttons install it from the AUR or open the official pages. On the command \
                     line: numapilot lsfg",
            ))
            .small()
            .weak(),
        );
    });
}

fn buttons(app: &mut App, ui: &mut egui::Ui, s: &lsfg::Status) {
    let open = |app: &mut App, url: &str| {
        if let Err(e) = crate::jobs::open_url(url) {
            app.toast(e.to_string(), false);
        }
    };
    if !s.installed() {
        match s.install_command() {
            Some(cmd) => {
                if crate::theme::small(ui, crate::i18n::tr("Install lsfg-vk (AUR)"))
                    .on_hover_text(crate::tf!(
                        "Runs in a terminal (asks for the password): {}",
                        "Roda num terminal (pede a senha): {}",
                        cmd.join(" ")
                    ))
                    .clicked()
                    && let Err(e) = crate::jobs::in_terminal(&cmd)
                {
                    app.toast(e, false);
                }
            }
            None => {
                if crate::theme::small(ui, crate::i18n::tr("Download lsfg-vk"))
                    .on_hover_text(crate::i18n::tr(
                        "Official builds: extract the file with tar -xf FILE -C ~/.local",
                    ))
                    .clicked()
                {
                    open(app, lsfg::BUILDS_URL);
                }
            }
        }
        if crate::theme::small(ui, crate::i18n::tr("Install guide")).clicked() {
            open(app, lsfg::DOCS_URL);
        }
    }
    if s.lossless.is_none() && s.dll_override.is_none() {
        if crate::theme::small(ui, crate::i18n::tr("Install Lossless Scaling (Steam)"))
            .on_hover_text(crate::i18n::tr(
                "Opens Steam to install it (it has to be bought first). Then pick the \
                 \"lsfg-vk\" branch in Properties → Betas.",
            ))
            .clicked()
        {
            open(app, lsfg::STEAM_INSTALL_URL);
        }
        if crate::theme::small(ui, crate::i18n::tr("Store page")).clicked() {
            open(app, lsfg::STORE_URL);
        }
    } else if s.wrong_branch()
        && crate::theme::small(ui, crate::i18n::tr("Open in Steam"))
            .on_hover_text(crate::i18n::tr("Properties → Betas → choose \"lsfg-vk\""))
            .clicked()
    {
        open(app, "steam://nav/games/details/993090");
    }
    if let Some(ui_bin) = &s.ui
        && crate::theme::small(ui, crate::i18n::tr("Configure (lsfg-vk-ui)"))
            .on_hover_text(crate::i18n::tr(
                "Multiplier, flow scale and per-game profiles of lsfg-vk",
            ))
            .clicked()
        && let Err(e) = crate::jobs::spawn_detached(&[ui_bin.to_string_lossy().into()])
    {
        app.toast(e.to_string(), false);
    }
    if s.config_exists && crate::theme::small(ui, crate::i18n::tr("Open config file")).clicked() {
        open(app, &s.config.to_string_lossy());
    }
    if let Some(cli) = &s.cli
        && crate::theme::small(ui, crate::i18n::tr("Check (healthcheck)"))
            .on_hover_text(crate::i18n::tr(
                "lsfg-vk-cli healthcheck: legacy, duplicate or missing files",
            ))
            .clicked()
        && let Err(e) =
            crate::jobs::in_terminal(&[cli.to_string_lossy().into(), "healthcheck".into()])
    {
        app.toast(e, false);
    }
}
