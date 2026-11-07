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
                        let _ = open_url(&format!(
                            "http://127.0.0.1:{}",
                            LabSettings::load().get("LLM_PORT")
                        ));
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

/// Avisa (notificação do sistema) quando uma GPU passa de 90 °C por 3 leituras seguidas;
/// no máximo uma vez a cada 10 minutos por placa.
#[derive(Default)]
struct Alerts {
    streak: std::collections::HashMap<String, u32>,
    last: std::collections::HashMap<String, std::time::Instant>,
    nv_screen_warned: bool,
    probe_at: Option<std::time::Instant>,
    battery_warned: bool,
}

impl Alerts {
    fn check(&mut self, topo: &Topology) {
        // "NVIDIA só quando precisar": tela ligada na saída da NVIDIA depois do login
        // O notebook não avisa quando um cabo é ligado na NVIDIA dormindo: confere de tempos em
        // tempos (acorda a placa por um instante): 20 s na tomada, 60 s na bateria.
        if numapilot::hybrid::session_restricted()
            && let Some((_, nv)) = numapilot::hybrid::pair(topo)
        {
            let every = if numapilot::notebook::on_ac() == Some(false) {
                60
            } else {
                20
            };
            if self
                .probe_at
                .is_none_or(|t| t.elapsed() >= Duration::from_secs(every))
            {
                self.probe_at = Some(std::time::Instant::now());
                let _ = numapilot::hybrid::probe_connected(&nv.pci);
            }
        }
        // Login na tomada (desktop nas duas placas) e depois foi para a bateria: a NVIDIA fica
        // presa pelo desktop até sair e entrar.
        if numapilot::hybrid::enabled()
            && !numapilot::hybrid::session_restricted()
            && let Some((_, nv)) = numapilot::hybrid::pair(topo)
        {
            let on_batt = numapilot::notebook::on_ac() == Some(false);
            let nv_screen = numapilot::hybrid::outputs(&nv.pci)
                .iter()
                .any(|o| o.connected);
            if on_batt
                && !nv_screen
                && !numapilot::notebook::dgpu_state(&nv.pci).starts_with("dormindo")
            {
                if !self.battery_warned {
                    self.battery_warned = true;
                    let _ = spawn_detached(&[
                        "notify-send".into(),
                        "-a".into(),
                        "numapilot".into(),
                        "-i".into(),
                        "battery".into(),
                        format!("{} ligada na bateria", nv.alias),
                        "Esta sessão começou na tomada com o desktop nas duas placas. Para a NVIDIA dormir e economizar bateria, saia e entre na sessão.".into(),
                    ]);
                }
            } else if !on_batt {
                self.battery_warned = false;
            }
        }
        if numapilot::hybrid::session_restricted()
            && let Some((_, nv)) = numapilot::hybrid::pair(topo)
        {
            let on: Vec<String> = numapilot::hybrid::outputs(&nv.pci)
                .into_iter()
                .filter(|o| o.connected)
                .map(|o| o.name)
                .collect();
            if on.is_empty() {
                self.nv_screen_warned = false;
            } else if !self.nv_screen_warned {
                self.nv_screen_warned = true;
                let _ = spawn_detached(&[
                    "notify-send".into(),
                    "-a".into(),
                    "numapilot".into(),
                    "-i".into(),
                    "video-display".into(),
                    format!("Tela na saída da {}", nv.alias),
                    format!(
                        "{} está na placa NVIDIA, e esta sessão subiu só na integrada. Saia e entre na sessão para usar esta tela.",
                        on.join(", ")
                    ),
                ]);
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
                let body = format!(
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
                    format!("GPU quente: {}", g.alias),
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
    let topo = Topology::load(&Config::load().unwrap_or_default());
    let (tip, hot) = summary(&topo);
    let handle = NumaTray { tip, hot }
        .assume_sni_available(true)
        .spawn()
        .map_err(|e| format!("bandeja indisponível: {e}"))?;
    let mut alerts = Alerts::default();
    loop {
        std::thread::sleep(Duration::from_secs(5));
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
