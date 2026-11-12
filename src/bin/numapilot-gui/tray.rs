//! Ícone na bandeja (StatusNotifierItem: KDE, GNOME com extensão AppIndicator...).
//! Mostra as temperaturas das GPUs e dá atalhos para o laboratório, as IDEs e o servidor de IA.

use crate::jobs::{open_url, spawn_detached};
use crate::lab::{code_url, gfx, jupyter_url, open_code_app, open_terminal};
use ksni::blocking::TrayMethods;
use ksni::menu::{StandardItem, SubMenu};
use ksni::{MenuItem, ToolTip, Tray};
use numapilot::config::Config;
use numapilot::gpuctl;
use numapilot::labcfg::LabSettings;
use numapilot::topo::Topology;
use std::time::Duration;

struct NumaTray {
    tip: String,
    hot: bool,
}

/// Abre a janela (ou traz para frente a que já está aberta: ver `single`).
fn open_window() {
    let exe = crate::jobs::self_exe();
    match std::process::Command::new(&exe)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        // espera numa thread para não deixar processo zumbi
        Ok(mut ch) => {
            std::thread::spawn(move || ch.wait());
        }
        Err(e) => {
            let _ = spawn_detached(&[
                "notify-send".into(),
                "-a".into(),
                "numapilot".into(),
                "-i".into(),
                "dialog-error".into(),
                crate::i18n::tr("Não consegui abrir o numapilot").into(),
                format!("{exe}: {e}"),
            ]);
        }
    }
}

fn run(args: &[&str]) {
    let _ = spawn_detached(&gfx(args));
}

fn item(label: &str, f: impl Fn(&mut NumaTray) + Send + Sync + 'static) -> MenuItem<NumaTray> {
    StandardItem {
        label: label.into(),
        activate: Box::new(f),
        ..Default::default()
    }
    .into()
}

impl Tray for NumaTray {
    fn id(&self) -> String {
        "numapilot".into()
    }
    fn title(&self) -> String {
        "numapilot".into()
    }
    fn icon_name(&self) -> String {
        if self.hot {
            "dialog-warning".into()
        } else {
            "preferences-system-performance".into()
        }
    }
    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: "numapilot".into(),
            description: self.tip.clone(),
            ..Default::default()
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        open_window();
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            item(crate::i18n::tr("Abrir numapilot"), |_| open_window()),
            MenuItem::Separator,
            SubMenu {
                label: crate::i18n::tr("Laboratório").into(),
                submenu: vec![
                    item("JupyterLab", |_| {
                        let _ = open_url(&jupyter_url(&LabSettings::load()));
                    }),
                    item(crate::i18n::tr("VS Code (navegador)"), |_| {
                        let _ = open_url(&code_url(&LabSettings::load()));
                    }),
                    item(crate::i18n::tr("VS Code (app do PC)"), |_| {
                        let _ = open_code_app(&LabSettings::load());
                    }),
                    item("PyCharm", |_| run(&["lab", "pycharm"])),
                    item("Terminal", |_| {
                        let _ = open_terminal(&LabSettings::load().get("LAB_NAME"));
                    }),
                    MenuItem::Separator,
                    item(crate::i18n::tr("Ligar"), |_| run(&["lab", "up"])),
                    item(crate::i18n::tr("Desligar"), |_| run(&["lab", "down"])),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: crate::i18n::tr("Servidor de IA").into(),
                submenu: vec![
                    item(crate::i18n::tr("Abrir chat"), |_| {
                        let _ = open_url(&format!(
                            "http://127.0.0.1:{}",
                            LabSettings::load().get("LLM_PORT")
                        ));
                    }),
                    item(crate::i18n::tr("Iniciar"), |_| run(&["llm", "up"])),
                    item(crate::i18n::tr("Parar"), |_| run(&["llm", "down"])),
                ],
                ..Default::default()
            }
            .into(),
            item(crate::i18n::tr("Painel do Sunshine"), |_| {
                let _ = open_url("https://localhost:47990");
            }),
            MenuItem::Separator,
            item(crate::i18n::tr("Sair do ícone"), |_| std::process::exit(0)),
        ]
    }
}

/// Avisa (notificação do sistema) quando uma GPU passa de 90 °C por 3 leituras seguidas;
/// no máximo uma vez a cada 10 minutos por placa.
#[derive(Default)]
struct Alerts {
    streak: std::collections::HashMap<String, u32>,
    last: std::collections::HashMap<String, std::time::Instant>,
    battery_warned: bool,
}

impl Alerts {
    fn check(&mut self, topo: &Topology) {
        // "NVIDIA só quando precisar": tela ligada na saída da NVIDIA depois do login
        // Modo da NVIDIA: avisa uma vez quando não deu para desligar porque um programa usa a placa.
        if let Some(st) = numapilot::hybrid::load_state() {
            if !st.want_present && !st.blocked_by.is_empty() {
                if !self.battery_warned {
                    self.battery_warned = true;
                    let _ = spawn_detached(&[
                        "notify-send".into(),
                        "-a".into(),
                        "numapilot".into(),
                        "-i".into(),
                        "video-display".into(),
                        crate::tf!("{} ainda ligada", "{} still on", st.alias),
                        crate::tf!(
                            "Não desliguei a NVIDIA porque está em uso por: {}.",
                            "The NVIDIA was not turned off because it is in use by: {}.",
                            st.blocked_by.join(", ")
                        ),
                    ]);
                }
            } else {
                self.battery_warned = false;
            }
        }
        for g in &topo.gpus {
            let s = gpuctl::read_sensors(&g.pci);
            let Some(t) = s.control_temp() else { continue };
            let n = self.streak.entry(g.pci.clone()).or_default();
            *n = if t >= 90.0 { *n + 1 } else { 0 };
            let recent = self
                .last
                .get(&g.pci)
                .is_some_and(|l| l.elapsed() < Duration::from_secs(600));
            if *n >= 3 && !recent {
                self.last.insert(g.pci.clone(), std::time::Instant::now());
                let body = crate::tf!(
                    "{} está a {t:.0} °C (ventoinha {}). Confira a ventilação ou reduza o limite de potência.",
                    "{} is at {t:.0} °C (fan {}). Check the airflow or lower the power limit.",
                    g.alias,
                    s.fan_pct
                        .map(|p| format!("{p:.0}%"))
                        .unwrap_or_else(|| "?".into())
                );
                let _ = spawn_detached(&[
                    "notify-send".into(),
                    "-u".into(),
                    "critical".into(),
                    "-a".into(),
                    "numapilot".into(),
                    "-i".into(),
                    "dialog-warning".into(),
                    crate::tf!("GPU quente: {}", "Hot GPU: {}", g.alias),
                    body,
                ]);
            }
        }
    }
}

fn summary(topo: &Topology) -> (String, bool) {
    let mut lines = Vec::new();
    let mut hot = false;
    for g in &topo.gpus {
        let s = gpuctl::read_sensors(&g.pci);
        let t = s.control_temp();
        hot |= t.is_some_and(|t| t >= 90.0);
        lines.push(format!(
            "{}: {} · {}% · {}",
            g.alias,
            t.map(|t| format!("{t:.0} °C"))
                .unwrap_or_else(|| "-".into()),
            g.busy_percent().unwrap_or(0),
            s.power_w
                .map(|w| format!("{w:.0} W"))
                .unwrap_or_else(|| "-".into())
        ));
    }
    (lines.join("\n"), hot)
}

pub fn main_loop() -> Result<(), String> {
    // Um ícone só: se outro já está na bandeja, sai quieto.
    let _lock = match crate::single::lock("numapilot-tray.lock") {
        Some(l) => l,
        None => return Ok(()),
    };
    let topo = Topology::load(&Config::load().unwrap_or_default());
    let (tip, hot) = summary(&topo);
    let handle = NumaTray { tip, hot }
        .assume_sni_available(true)
        .spawn()
        .map_err(|e| crate::tf!("bandeja indisponível: {e}", "tray unavailable: {e}"))?;
    let mut alerts = Alerts::default();
    loop {
        std::thread::sleep(Duration::from_secs(5));
        // Reinstalaram o numapilot: troca este ícone pela versão nova.
        if crate::jobs::self_replaced() {
            use std::os::unix::process::CommandExt;
            drop(handle);
            drop(_lock);
            let e = std::process::Command::new(crate::jobs::self_exe())
                .arg("--tray")
                .exec();
            return Err(crate::tf!(
                "não consegui reabrir o ícone atualizado: {e}",
                "could not reopen the updated icon: {e}"
            ));
        }
        alerts.check(&topo);
        let (tip, hot) = summary(&topo);
        if handle
            .update(|t| {
                t.tip = tip;
                t.hot = hot;
            })
            .is_none()
        {
            return Ok(());
        }
    }
}
