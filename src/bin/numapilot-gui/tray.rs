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

fn open_window() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = spawn_detached(&[exe.to_string_lossy().to_string()]);
    }
}

fn run(args: &[&str]) {
    let _ = spawn_detached(&gfx(args));
}

fn item(label: &str, f: impl Fn(&mut NumaTray) + Send + Sync + 'static) -> MenuItem<NumaTray> {
    StandardItem { label: label.into(), activate: Box::new(f), ..Default::default() }.into()
}

impl Tray for NumaTray {
    fn id(&self) -> String {
        "numapilot".into()
    }
    fn title(&self) -> String {
        "numapilot".into()
    }
    fn icon_name(&self) -> String {
        if self.hot { "dialog-warning".into() } else { "preferences-system-performance".into() }
    }
    fn tool_tip(&self) -> ToolTip {
        ToolTip { title: "numapilot".into(), description: self.tip.clone(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        open_window();
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            item("Abrir numapilot", |_| open_window()),
            MenuItem::Separator,
            SubMenu {
                label: "Laboratório".into(),
                submenu: vec![
                    item("JupyterLab", |_| {
                        let _ = open_url(&jupyter_url(&LabSettings::load()));
                    }),
                    item("VS Code (navegador)", |_| {
                        let _ = open_url(&code_url(&LabSettings::load()));
                    }),
                    item("VS Code (app do PC)", |_| {
                        let _ = open_code_app(&LabSettings::load());
                    }),
                    item("PyCharm", |_| run(&["lab", "pycharm"])),
                    item("Terminal", |_| {
                        let _ = open_terminal(&LabSettings::load().get("LAB_NAME"));
                    }),
                    MenuItem::Separator,
                    item("Ligar", |_| run(&["lab", "up"])),
                    item("Desligar", |_| run(&["lab", "down"])),
                ],
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: "Servidor de IA".into(),
                submenu: vec![
                    item("Abrir chat", |_| {
                        let _ = open_url(&format!("http://127.0.0.1:{}", LabSettings::load().get("LLM_PORT")));
                    }),
                    item("Iniciar", |_| run(&["llm", "up"])),
                    item("Parar", |_| run(&["llm", "down"])),
                ],
                ..Default::default()
            }
            .into(),
            item("Painel do Sunshine", |_| {
                let _ = open_url("https://localhost:47990");
            }),
            MenuItem::Separator,
            item("Sair do ícone", |_| std::process::exit(0)),
        ]
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
            t.map(|t| format!("{t:.0} °C")).unwrap_or_else(|| "-".into()),
            g.busy_percent().unwrap_or(0),
            s.power_w.map(|w| format!("{w:.0} W")).unwrap_or_else(|| "-".into())
        ));
    }
    (lines.join("\n"), hot)
}

pub fn main_loop() -> Result<(), String> {
    let topo = Topology::load(&Config::load().unwrap_or_default());
    let (tip, hot) = summary(&topo);
    let handle = NumaTray { tip, hot }
        .assume_sni_available(true)
        .spawn()
        .map_err(|e| format!("bandeja indisponível: {e}"))?;
    loop {
        std::thread::sleep(Duration::from_secs(5));
        let (tip, hot) = summary(&topo);
        if handle.update(|t| {
            t.tip = tip;
            t.hot = hot;
        })
        .is_none()
        {
            return Ok(());
        }
    }
}
