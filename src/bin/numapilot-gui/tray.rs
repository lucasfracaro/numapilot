//! Tray icon (StatusNotifierItem: KDE, GNOME with the AppIndicator extension...).
//! Shows the GPU temperatures and gives shortcuts to the lab, the IDEs and the AI server.

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

/// Opens the window (or brings the one already open to the front: see `single`).
fn open_window() {
    let exe = crate::jobs::self_exe();
    match std::process::Command::new(&exe)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        // waits on a thread so no zombie process is left
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
                crate::i18n::tr("Could not open numapilot").into(),
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
            item(crate::i18n::tr("Open numapilot"), |_| open_window()),
            MenuItem::Separator,
            SubMenu {
                label: crate::i18n::tr("Lab").into(),
                submenu: vec![
                    item("JupyterLab", |_| {
                        let _ = open_url(&jupyter_url(&LabSettings::load()));
                    }),
                    item(crate::i18n::tr("VS Code (browser)"), |_| {
                        let _ = open_url(&code_url(&LabSettings::load()));
                    }),
                    item(crate::i18n::tr("VS Code (PC app)"), |_| {
                        let _ = open_code_app(&LabSettings::load());
                    }),
                    item("PyCharm", |_| run(&["lab", "pycharm"])),
                    item("Terminal", |_| {
                        let _ = open_terminal(&LabSettings::load().get("LAB_NAME"));
                    }),
                    MenuItem::Separator,
                    item(crate::i18n::tr("Turn on"), |_| run(&["lab", "up"])),
                    item(crate::i18n::tr("Turn off"), |_| run(&["lab", "down"])),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: crate::i18n::tr("AI server").into(),
                submenu: vec![
                    item(crate::i18n::tr("Open chat"), |_| {
                        let _ = open_url(&format!(
                            "http://127.0.0.1:{}",
                            LabSettings::load().get("LLM_PORT")
                        ));
                    }),
                    item(crate::i18n::tr("Start"), |_| run(&["llm", "up"])),
                    item(crate::i18n::tr("Stop"), |_| run(&["llm", "down"])),
                ],
                ..Default::default()
            }
            .into(),
            item(crate::i18n::tr("Sunshine panel"), |_| {
                let _ = open_url("https://localhost:47990");
            }),
            MenuItem::Separator,
            item(crate::i18n::tr("Quit the icon"), |_| std::process::exit(0)),
        ]
    }
}

/// Notifies (system notification) when a GPU stays above 90 °C for 3 readings in a row;
/// at most once every 10 minutes per card.
#[derive(Default)]
struct Alerts {
    streak: std::collections::HashMap<String, u32>,
    last: std::collections::HashMap<String, std::time::Instant>,
    battery_warned: bool,
}

impl Alerts {
    fn check(&mut self, topo: &Topology) {
        // "NVIDIA only when needed": display connected to the NVIDIA output after login
        // NVIDIA mode: notifies once when it could not be turned off because a program uses the card.
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
                        crate::tf!("{} still on", "{} ainda ligada", st.alias),
                        crate::tf!(
                            "The NVIDIA was not turned off because it is in use by: {}.",
                            "Não desliguei a NVIDIA porque está em uso por: {}.",
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
                    "{} is at {t:.0} °C (fan {}). Check the airflow or lower the power limit.",
                    "{} está a {t:.0} °C (ventoinha {}). Confira a ventilação ou reduza o limite de potência.",
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
                    crate::tf!("Hot GPU: {}", "GPU quente: {}", g.alias),
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
    // A single icon: if another is already in the tray, quit quietly.
    let _lock = match crate::single::lock("numapilot-tray.lock") {
        Some(l) => l,
        None => return Ok(()),
    };
    let topo = Topology::load(&Config::load().unwrap_or_default());
    let (tip, hot) = summary(&topo);
    let handle = NumaTray { tip, hot }
        .assume_sni_available(true)
        .spawn()
        .map_err(|e| crate::tf!("tray unavailable: {e}", "bandeja indisponível: {e}"))?;
    let mut alerts = Alerts::default();
    loop {
        std::thread::sleep(Duration::from_secs(5));
        // numapilot was reinstalled: replace this icon with the new version.
        if crate::jobs::self_replaced() {
            use std::os::unix::process::CommandExt;
            drop(handle);
            drop(_lock);
            let e = std::process::Command::new(crate::jobs::self_exe())
                .arg("--tray")
                .exec();
            return Err(crate::tf!(
                "could not reopen the updated icon: {e}",
                "não consegui reabrir o ícone atualizado: {e}"
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
