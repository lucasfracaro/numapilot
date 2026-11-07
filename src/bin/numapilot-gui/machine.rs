//! Aba "Máquina": modos de um clique, processadores (governor, EPB, frequência, uncore, potência),
//! latência para jogos, memória, escalonador, ventoinhas da placa-mãe, mitigações e boot.

use crate::App;
use eframe::egui::{self, Color32, RichText};
use numapilot::cmdline;
use numapilot::cpuctl;
use numapilot::ctl::Request;
use numapilot::fans::{self, BoardFan, BoardFanControl, BoardFans};
use numapilot::gpuctl::FanMode;
use numapilot::machine::{self, Machine};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

const ORANGE: Color32 = Color32::from_rgb(230, 160, 40);
const GREEN: Color32 = Color32::from_rgb(110, 190, 120);

#[derive(Default)]
pub struct MachineUi {
    pub edit: Option<Machine>,
    saved: Machine,
    hw: serde_json::Value,
    read_at: Option<Instant>,
    scx: Option<(String, Vec<String>)>,
    fans: Vec<BoardFan>,
    ipmi: Vec<(String, Option<u64>)>,
    fans_saved: BoardFans,
    fan_edit: BTreeMap<String, BoardFanControl>,
    // boot
    boot_loaded: bool,
    loader: Option<String>,
    configured: String,
    vulns: Vec<(String, String)>,
    sel: BTreeSet<String>,
    custom: String,
    dgpu_at: Option<Instant>,
    dgpu_users: Vec<(u32, String)>,
}

impl MachineUi {
    fn refresh(&mut self) {
        if self.read_at.is_some_and(|t| t.elapsed().as_secs() < 2) {
            return;
        }
        self.read_at = Some(Instant::now());
        self.saved = Machine::load();
        if self.edit.is_none() {
            self.edit = Some(self.saved.clone());
        }
        self.hw = std::fs::read_to_string(numapilot::learned::run_dir().join("hw.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        self.fans = fans::list();
        self.fans_saved = BoardFans::load();
        if !self.boot_loaded {
            self.boot_loaded = true;
            self.scx = machine::scx_status();
            self.ipmi = fans::ipmi_fans();
            self.load_boot();
        }
    }

    fn load_boot(&mut self) {
        self.vulns = cmdline::vulnerabilities();
        match cmdline::configured() {
            Some((l, p)) => {
                self.loader = Some(format!("{} ({})", l.name(), l.path.display()));
                self.configured = p;
            }
            None => {
                self.loader = None;
                self.configured = cmdline::running();
            }
        }
        self.sel = self
            .configured
            .split_whitespace()
            .map(String::from)
            .collect();
    }
}

fn tri(ui: &mut egui::Ui, id: &str, v: &mut Option<bool>, on: &str, off: &str) {
    let txt = match v {
        None => "não mexer",
        Some(true) => on,
        Some(false) => off,
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(txt)
        .show_ui(ui, |ui| {
            ui.selectable_value(v, None, "não mexer");
            ui.selectable_value(v, Some(true), on);
            ui.selectable_value(v, Some(false), off);
        });
}

fn opt_mhz(ui: &mut egui::Ui, v: &mut Option<u32>, def: u32, lo: u32, hi: u32) {
    let mut on = v.is_some();
    let mut x = v.unwrap_or(def);
    ui.checkbox(&mut on, "");
    ui.add_enabled(
        on,
        egui::DragValue::new(&mut x)
            .range(lo..=hi.max(lo))
            .speed(25.0)
            .suffix(" MHz"),
    );
    *v = on.then_some(x);
}

const EPB: &[(u8, &str)] = &[
    (0, "desempenho"),
    (4, "equilibrado (desempenho)"),
    (6, "normal"),
    (8, "equilibrado (economia)"),
    (15, "economia"),
];

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    app.mach.refresh();
    let topo = app.topo.clone();
    let mut send_machine: Option<Machine> = None;
    let mut send_fan: Option<(String, BoardFanControl)> = None;
    let mut job: Option<(String, Vec<String>)> = None;
    let mut app_toast: Option<String> = None;

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mu = &mut app.mach;
        // ---------------- modos ----------------
        ui.heading("Modo da máquina");
        ui.label(RichText::new("Um clique ajusta processadores, memória e escalonador juntos. \"Normal\" devolve tudo como o sistema estava.").weak());
        ui.horizontal_wrapped(|ui| {
            let game_node = app
                .cfg
                .graphics_gpu
                .as_deref()
                .and_then(|a| topo.gpus.iter().find(|g| g.alias == a || g.pci == a))
                .or_else(|| topo.display_gpu())
                .and_then(|g| g.numa_node);
            for (name, label, tip) in [
                ("normal", "Normal", "Tudo como o sistema estava."),
                ("jogo", "Jogo", "Socket da placa de vídeo em desempenho máximo, uncore no máximo, turbo, sem balanceamento NUMA automático e escalonador LAVD em modo jogo."),
                ("ia", "IA", "Os dois sockets em desempenho, uncore no máximo, THP sempre e sem balanceamento NUMA automático."),
                ("silencioso", "Silencioso", "Sem turbo, governor econômico, EPB economia, potência limitada a 60% e LAVD em economia."),
            ] {
                let cur = mu.saved.mode == name;
                let b = egui::Button::new(RichText::new(label).strong()).selected(cur).min_size(egui::vec2(110.0, 30.0));
                if ui.add(b).on_hover_text(tip).clicked() {
                    let m = Machine::preset(name, &topo, game_node, &mu.saved);
                    mu.edit = Some(m.clone());
                    send_machine = Some(m);
                }
            }
            if !["normal", "jogo", "ia", "silencioso"].contains(&mu.saved.mode.as_str()) {
                ui.label(RichText::new(format!("atual: {}", mu.saved.mode)).weak());
            }
        });
        let Some(m) = mu.edit.as_mut() else { return };

        // ---------------- notebook ----------------
        if numapilot::notebook::is_laptop() {
            use numapilot::notebook as nb;
            ui.add_space(8.0);
            ui.heading("Notebook");
            let ac = nb::on_ac();
            for b in nb::battery_info() {
                ui.label(format!(
                    "Bateria {}: {}% · {} · {}{}",
                    b.name,
                    b.pct.map(|p| p.to_string()).unwrap_or("?".into()),
                    match b.status.as_str() {
                        "Charging" => "carregando",
                        "Discharging" => "descarregando",
                        "Full" => "cheia",
                        "Not charging" => "parada no limite",
                        o => o,
                    },
                    b.power_w.map(|w| format!("{w:.1} W")).unwrap_or_default(),
                    b.health_pct.map(|h| format!(" · saúde {h:.0}%")).unwrap_or_default()
                ));
            }
            egui::Grid::new("nb").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
                let modes = [("normal", "Normal"), ("jogo", "Jogo"), ("ia", "IA"), ("silencioso", "Silencioso")];
                let combo = |ui: &mut egui::Ui, id: &str, v: &mut Option<String>| {
                    let txt = v.as_deref().and_then(|x| modes.iter().find(|m| m.0 == x)).map(|m| m.1).unwrap_or("não trocar");
                    egui::ComboBox::from_id_salt(id).selected_text(txt).show_ui(ui, |ui| {
                        ui.selectable_value(v, None, "não trocar");
                        for (k, l) in modes {
                            ui.selectable_value(v, Some(k.to_string()), l);
                        }
                    });
                };
                ui.label(format!("Ao ligar na tomada{}", if ac == Some(true) { " (agora)" } else { "" }));
                combo(ui, "acm", &mut m.ac_mode);
                ui.end_row();
                ui.label(format!("Na bateria{}", if ac == Some(false) { " (agora)" } else { "" }));
                combo(ui, "batm", &mut m.battery_mode);
                ui.end_row();
                if nb::battery_info().iter().any(|b| b.charge_limit.is_some()) {
                    ui.label("Parar de carregar em").on_hover_text("Deixar a bateria em 80% quando o notebook fica sempre na tomada aumenta muito a vida útil dela.");
                    ui.horizontal(|ui| {
                        let mut on = m.charge_limit.is_some();
                        let mut v = m.charge_limit.unwrap_or(80);
                        ui.checkbox(&mut on, "");
                        ui.add_enabled(on, egui::Slider::new(&mut v, 50..=100).suffix("%").step_by(5.0));
                        m.charge_limit = on.then_some(v);
                    });
                    ui.end_row();
                }
                let ch = nb::platform_profiles();
                if !ch.is_empty() {
                    ui.label("Perfil de energia do notebook").on_hover_text("Perfil do fabricante (ACPI): mexe em ventoinhas e limites de potência. Os modos já escolhem um.");
                    egui::ComboBox::from_id_salt("pp")
                        .selected_text(m.platform_profile.clone().unwrap_or_else(|| format!("não mexer ({})", nb::platform_profile().unwrap_or_default())))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut m.platform_profile, None, "não mexer");
                            for c in &ch {
                                ui.selectable_value(&mut m.platform_profile, Some(c.clone()), c);
                            }
                        });
                    ui.end_row();
                }
                for g in topo.gpus.iter().filter(|g| !g.is_integrated()) {
                    ui.label(format!("Placa dedicada {}", g.alias));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(nb::dgpu_state(&g.pci)).weak());
                        tri(ui, &format!("dpm{}", g.pci), &mut m.dgpu_autosuspend, "dormir quando ninguém usa", "sempre ligada");
                    });
                    ui.end_row();
                    if !nb::dgpu_state(&g.pci).starts_with("dormindo") {
                        if mu.dgpu_at.is_none_or(|t| t.elapsed().as_secs() >= 3) {
                            mu.dgpu_at = Some(Instant::now());
                            mu.dgpu_users = nb::dgpu_users(&g.pci);
                        }
                        if !mu.dgpu_users.is_empty() {
                            let mut names: Vec<&str> = mu.dgpu_users.iter().map(|(_, n)| n.as_str()).collect();
                            names.sort();
                            names.dedup();
                            ui.label("");
                            ui.label(
                                RichText::new(format!("Acordada por: {}", names.join(", ")))
                                    .small()
                                    .color(Color32::from_rgb(230, 160, 40)),
                            )
                            .on_hover_text("Esses programas abriram a placa dedicada; enquanto estiverem abertos ela não dorme e gasta bateria. Abra-os na integrada (perfil do programa → placa) para economizar.");
                            ui.end_row();
                        }
                    }
                }
                if nb::has_nvidia_powerd() {
                    ui.label("Dynamic Boost da NVIDIA").on_hover_text("Passa potência do processador para a placa de vídeo em jogos (nvidia-powerd).");
                    tri(ui, "powerd", &mut m.nvidia_powerd, "ligado", "desligado");
                    ui.end_row();
                }
            });
        }

        // ---------------- Ryzen de notebook (ryzenadj) ----------------
        if numapilot::ryzen::applicable() {
            ui.add_space(8.0);
            ui.heading("Limites de potência do Ryzen");
            if !numapilot::ryzen::available() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Instale o ryzenadj para ajustar STAPM/PPT (o equivalente do PL1/PL2 da Intel).").weak());
                    if ui.small_button("instalar (AUR)").clicked() {
                        let helper = if numapilot::containers::which("paru") { "paru" } else { "yay" };
                        if let Err(e) = crate::jobs::in_terminal(&[helper.into(), "-S".into(), "ryzenadj".into()]) {
                            app_toast = Some(e);
                        }
                    }
                });
            } else {
                let now = &mu.hw["ryzen"];
                let r = m.ryzen.get_or_insert_with(Default::default);
                egui::Grid::new("ryzen").num_columns(3).spacing([14.0, 6.0]).show(ui, |ui| {
                    let rows: [(&str, &str, &str, &mut Option<f64>, f64, f64); 4] = [
                        ("Sustentado (STAPM)", "STAPM LIMIT", " W", &mut r.stapm_w, 5.0, 120.0),
                        ("Média curta (PPT lento)", "PPT LIMIT SLOW", " W", &mut r.slow_w, 5.0, 150.0),
                        ("Picos (PPT rápido)", "PPT LIMIT FAST", " W", &mut r.fast_w, 5.0, 150.0),
                        ("Temperatura máxima", "THM LIMIT CORE", " °C", &mut r.tctl_c, 60.0, 105.0),
                    ];
                    for (label, key, unit, v, lo, hi) in rows {
                        ui.label(label);
                        let cur = now[key].as_f64();
                        let mut on = v.is_some();
                        let mut x = v.or(cur).unwrap_or(lo.max(35.0).min(hi));
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut on, "");
                            ui.add_enabled(on, egui::DragValue::new(&mut x).range(lo..=hi).suffix(unit).speed(0.5).fixed_decimals(0));
                        });
                        *v = on.then_some(x);
                        ui.label(RichText::new(cur.map(|c| format!("agora {c:.0}{unit}")).unwrap_or_else(|| "agora ? (serviço)".into())).weak());
                        ui.end_row();
                    }
                });
                if r.is_empty() {
                    m.ryzen = None;
                }
                ui.label(
                    RichText::new("Baixar o STAPM e a temperatura deixa o notebook frio e silencioso; o serviço reaplica se o firmware voltar os valores ao trocar tomada/bateria.")
                        .small()
                        .weak(),
                );
            }
        }

        // ---------------- Acer Predator / Nitro ----------------
        if numapilot::acer::is_acer() || numapilot::acer::sense_dir().is_some() {
            use numapilot::acer;
            ui.add_space(8.0);
            ui.heading("Acer Predator / Nitro");
            let knobs = acer::knobs();
            if knobs.is_empty() && acer::kb_dir().is_none() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Ventoinhas, limite de carga em 80% e RGB precisam do módulo da comunidade Linuwu-Sense.").weak());
                    if ui.small_button("página").clicked() {
                        let _ = crate::jobs::open_url("https://github.com/0x7375646F/Linuwu-Sense");
                    }
                });
            }
            egui::Grid::new("acer").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
                for (key, cur) in &knobs {
                    let Some((_, _, label, help)) = acer::KNOBS.iter().find(|k| k.0 == *key) else { continue };
                    ui.label(*label).on_hover_text(*help);
                    let cur = cur.clone().unwrap_or_default();
                    match *key {
                        "fan_speed" => {
                            let v = m.acer.get(*key).cloned();
                            let (mut c, mut g) = v
                                .as_deref()
                                .and_then(|v| v.split_once(','))
                                .and_then(|(a, b)| Some((a.trim().parse::<u32>().ok()?, b.trim().parse::<u32>().ok()?)))
                                .unwrap_or((0, 0));
                            let mut mode = match (v.is_some(), c, g) {
                                (false, ..) => 0,
                                (true, 0, 0) => 1,
                                (true, 100, 100) => 2,
                                _ => 3,
                            };
                            ui.horizontal(|ui| {
                                for (i, l) in ["não mexer", "automático", "máximo", "manual"].iter().enumerate() {
                                    ui.selectable_value(&mut mode, i, *l);
                                }
                                if mode == 3 {
                                    if (c, g) == (0, 0) || (c, g) == (100, 100) {
                                        (c, g) = (60, 60);
                                    }
                                    ui.add(egui::DragValue::new(&mut c).range(20..=100).prefix("CPU ").suffix("%"));
                                    ui.add(egui::DragValue::new(&mut g).range(20..=100).prefix("GPU ").suffix("%"));
                                }
                                ui.label(RichText::new(format!("agora {cur}")).weak());
                            });
                            match mode {
                                0 => {
                                    m.acer.remove(*key);
                                }
                                1 => {
                                    m.acer.insert(key.to_string(), "0,0".into());
                                }
                                2 => {
                                    m.acer.insert(key.to_string(), "100,100".into());
                                }
                                _ => {
                                    m.acer.insert(key.to_string(), format!("{c},{g}"));
                                }
                            }
                        }
                        "usb_charging" => {
                            let mut v = m.acer.get(*key).cloned();
                            egui::ComboBox::from_id_salt("usbc")
                                .selected_text(v.clone().map(|x| if x == "0" { "não".into() } else { format!("até {x}%") }).unwrap_or_else(|| format!("não mexer ({cur})")))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut v, None, "não mexer");
                                    for x in ["0", "10", "20", "30"] {
                                        ui.selectable_value(&mut v, Some(x.to_string()), if x == "0" { "não".to_string() } else { format!("até {x}%") });
                                    }
                                });
                            match v {
                                Some(x) => m.acer.insert(key.to_string(), x),
                                None => m.acer.remove(*key),
                            };
                        }
                        _ => {
                            let mut v = m.acer.get(*key).map(|x| x == "1");
                            ui.horizontal(|ui| {
                                tri(ui, &format!("acer{key}"), &mut v, "ligado", "desligado");
                                ui.label(RichText::new(format!("agora {}", if cur == "1" { "ligado" } else { "desligado" })).weak());
                            });
                            match v {
                                Some(b) => m.acer.insert(key.to_string(), if b { "1" } else { "0" }.into()),
                                None => m.acer.remove(*key),
                            };
                        }
                    }
                    ui.end_row();
                }
                if acer::kb_dir().is_some() {
                    ui.label("Teclado RGB");
                    ui.horizontal(|ui| {
                        let cur = m.acer.get("kb_effect").cloned();
                        let parts: Vec<u32> = cur.as_deref().unwrap_or("0,1,100,0,255,40,0").split(',').filter_map(|x| x.parse().ok()).collect();
                        let p = |i: usize, d: u32| parts.get(i).copied().unwrap_or(d);
                        let mut on = cur.is_some();
                        let (mut mode, speed, mut bright, dir) = (p(0, 0), p(1, 1), p(2, 100), p(3, 0));
                        let mut rgb = [p(4, 255) as u8, p(5, 40) as u8, p(6, 0) as u8];
                        ui.checkbox(&mut on, "");
                        ui.add_enabled_ui(on, |ui| {
                            egui::ComboBox::from_id_salt("kbfx")
                                .selected_text(acer::EFFECTS.iter().find(|e| e.0 as u32 == mode).map(|e| e.1).unwrap_or("?"))
                                .show_ui(ui, |ui| {
                                    for (k, l) in acer::EFFECTS {
                                        ui.selectable_value(&mut mode, *k as u32, *l);
                                    }
                                });
                            ui.color_edit_button_srgb(&mut rgb);
                            ui.add(egui::Slider::new(&mut bright, 0..=100).suffix("%"));
                        });
                        if on {
                            m.acer.insert("kb_effect".into(), format!("{mode},{speed},{bright},{dir},{},{},{}", rgb[0], rgb[1], rgb[2]));
                        } else {
                            m.acer.remove("kb_effect");
                        }
                    });
                    ui.end_row();
                }
            });
        }

        // ---------------- processadores ----------------
        ui.add_space(8.0);
        ui.heading("Processadores");
        ui.horizontal(|ui| {
            ui.label("Turbo:");
            tri(ui, "turbo", &mut m.turbo, "ligado", "desligado");
            if let Some(t) = cpuctl::turbo() {
                ui.label(RichText::new(format!("agora {}", if t { "ligado" } else { "desligado" })).weak());
            }
        });
        let unc = cpuctl::uncores();
        let rapl = cpuctl::rapl_zones();
        let temps = cpuctl::package_temps();
        egui::Grid::new("sockets").striped(true).num_columns(topo.nodes.len() + 1).spacing([18.0, 6.0]).show(ui, |ui| {
            ui.label("");
            for n in &topo.nodes {
                ui.label(RichText::new(if topo.nodes.len() == 1 { "Processador".to_string() } else { format!("Socket {}", n.id) }).strong());
            }
            ui.end_row();
            ui.label("Agora");
            for n in &topo.nodes {
                let pkg = n.sockets.iter().next().copied().unwrap_or(n.id as u32);
                let c = n.cpus.first().copied().unwrap_or(0);
                let f = cpuctl::freq(c).unwrap_or_default();
                let pw = mu.hw["power"][pkg.to_string()].as_f64();
                ui.label(format!(
                    "{} MHz · {} · {}",
                    f.cur_khz / 1000,
                    temps.get(&pkg).map(|t| format!("{t:.0} °C")).unwrap_or("-".into()),
                    pw.map(|w| format!("{w:.0} W")).unwrap_or("- W".into())
                ));
            }
            ui.end_row();
            ui.label("Governor");
            for n in &topo.nodes {
                let c = n.cpus.first().copied().unwrap_or(0);
                let t = m.socket_mut(n.id);
                let cur = cpuctl::governor(c).unwrap_or_default();
                egui::ComboBox::from_id_salt(format!("gov{}", n.id))
                    .selected_text(t.governor.clone().unwrap_or_else(|| format!("não mexer ({cur})")))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut t.governor, None, "não mexer");
                        for g in cpuctl::governors_available(c) {
                            ui.selectable_value(&mut t.governor, Some(g.clone()), g);
                        }
                    });
            }
            ui.end_row();
            ui.label("Viés de energia (EPB)");
            for n in &topo.nodes {
                let c = n.cpus.first().copied().unwrap_or(0);
                let t = m.socket_mut(n.id);
                let name = |v: u8| EPB.iter().find(|e| e.0 == v).map(|e| e.1.to_string()).unwrap_or(v.to_string());
                egui::ComboBox::from_id_salt(format!("epb{}", n.id))
                    .selected_text(t.epb.map(name).unwrap_or_else(|| format!("não mexer ({})", cpuctl::epb(c).map(name).unwrap_or("?".into()))))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut t.epb, None, "não mexer");
                        for (v, l) in EPB {
                            ui.selectable_value(&mut t.epb, Some(*v), *l);
                        }
                    });
            }
            ui.end_row();
            let c0 = topo.nodes.first().and_then(|n| n.cpus.first().copied()).unwrap_or(0);
            if !cpuctl::epp_available(c0).is_empty() {
                ui.label("Preferência de energia (EPP)").on_hover_text(
                    "O ajuste principal do Ryzen (amd-pstate): de performance a power. Com o governor performance o driver fixa em performance.",
                );
                for n in &topo.nodes {
                    let c = n.cpus.first().copied().unwrap_or(0);
                    let t = m.socket_mut(n.id);
                    egui::ComboBox::from_id_salt(format!("epp{}", n.id))
                        .selected_text(t.epp.clone().unwrap_or_else(|| format!("não mexer ({})", cpuctl::epp(c).unwrap_or_default())))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut t.epp, None, "não mexer");
                            for e in cpuctl::epp_available(c) {
                                if e != "default" {
                                    ui.selectable_value(&mut t.epp, Some(e.clone()), e);
                                }
                            }
                        });
                }
                ui.end_row();
            }
            ui.label("Frequência mín. / máx.");
            for n in &topo.nodes {
                let c = n.cpus.first().copied().unwrap_or(0);
                let f = cpuctl::freq(c).unwrap_or_default();
                let (lo, hi) = ((f.hw_min_khz / 1000) as u32, (f.hw_max_khz / 1000) as u32);
                let t = m.socket_mut(n.id);
                ui.horizontal(|ui| {
                    opt_mhz(ui, &mut t.min_mhz, lo, lo, hi);
                    opt_mhz(ui, &mut t.max_mhz, hi, lo, hi);
                });
            }
            ui.end_row();
            ui.label("Uncore (L3 + memória)").on_hover_text("Uncore mais alto = menor latência de memória e do cache L3, à custa de alguns watts.");
            for n in &topo.nodes {
                let pkg = n.sockets.iter().next().copied().unwrap_or(n.id as u32);
                let t = m.socket_mut(n.id);
                match unc.iter().find(|u| u.package == pkg) {
                    Some(u) => {
                        let (lo, hi) = ((u.init_min_khz.min(u.min_khz) / 1000) as u32, (u.init_max_khz.max(u.max_khz) / 1000) as u32);
                        ui.horizontal(|ui| {
                            opt_mhz(ui, &mut t.uncore_min_mhz, hi, lo, hi);
                            opt_mhz(ui, &mut t.uncore_max_mhz, hi, lo, hi);
                            if let Some(cur) = u.cur_khz {
                                ui.label(RichText::new(format!("agora {}", cur / 1000)).weak());
                            }
                        });
                    }
                    None => {
                        ui.label(RichText::new("indisponível (sudo modprobe intel_uncore_frequency)").weak());
                    }
                }
            }
            ui.end_row();
            ui.label("Limite de potência");
            for n in &topo.nodes {
                let pkg = n.sockets.iter().next().copied().unwrap_or(n.id as u32);
                let t = m.socket_mut(n.id);
                match rapl.iter().find(|r| r.package == pkg) {
                    Some(r) => {
                        let max = r.max_w.or(r.limit_w).unwrap_or(150.0);
                        ui.horizontal(|ui| {
                            let mut on = t.power_limit_w.is_some();
                            ui.checkbox(&mut on, "");
                            let mut w = t.power_limit_w.unwrap_or(max);
                            ui.add_enabled(on, egui::Slider::new(&mut w, 40.0..=max).suffix(" W").step_by(5.0));
                            t.power_limit_w = on.then_some(w);
                        });
                    }
                    None => {
                        ui.label(RichText::new("indisponível (RAPL)").weak());
                    }
                }
            }
            ui.end_row();
            if rapl.iter().any(|r| r.limit2_w.is_some()) {
                ui.label("Pico de potência (PL2)").on_hover_text("Quanto o processador pode puxar por alguns segundos antes de cair para o limite acima.");
                for n in &topo.nodes {
                    let pkg = n.sockets.iter().next().copied().unwrap_or(n.id as u32);
                    let t = m.socket_mut(n.id);
                    match rapl.iter().find(|r| r.package == pkg).and_then(|r| r.limit2_w) {
                        Some(cur) => {
                            ui.horizontal(|ui| {
                                let mut on = t.power_limit2_w.is_some();
                                ui.checkbox(&mut on, "");
                                let mut w = t.power_limit2_w.unwrap_or(cur);
                                ui.add_enabled(on, egui::Slider::new(&mut w, 40.0..=(cur * 1.5).max(100.0)).suffix(" W").step_by(5.0));
                                t.power_limit2_w = on.then_some(w);
                            });
                        }
                        None => {
                            ui.label("");
                        }
                    }
                }
                ui.end_row();
            }
        });

        // ---------------- latência ----------------
        ui.add_space(8.0);
        ui.heading("Latência em jogos");
        ui.label(
            RichText::new(
                "Vale enquanto roda um programa com \"Baixa latência\" (o perfil Jogo liga): nos núcleos dele, o processador não entra \
                 nos estados de economia profundos (acordar deles causa travadinhas) e o uncore do socket fica no máximo. Ao fechar, volta tudo.",
            )
            .weak(),
        );
        let cs = cpuctl::cstates(topo.nodes.first().and_then(|n| n.cpus.first().copied()).unwrap_or(0));
        ui.horizontal(|ui| {
            ui.label("Estados permitidos:");
            let label = |us: u64| {
                let allowed: Vec<String> = cs.iter().filter(|s| s.index > 0 && s.latency_us <= us).map(|s| s.name.clone()).collect();
                if allowed.is_empty() { format!("nenhum (≤ {us} µs)") } else { format!("{} (≤ {us} µs)", allowed.join(", ")) }
            };
            egui::ComboBox::from_id_salt("lat").selected_text(label(m.game_latency_us)).show_ui(ui, |ui| {
                let mut opts: Vec<u64> = cs.iter().filter(|s| s.index > 0).map(|s| s.latency_us).collect();
                opts.dedup();
                if opts.is_empty() {
                    opts = vec![2, 10, 40];
                }
                for us in opts {
                    ui.selectable_value(&mut m.game_latency_us, us, label(us));
                }
            });
            ui.checkbox(&mut m.game_uncore_max, "uncore no máximo");
        });
        if let Some(l) = mu.hw["lowlat"].as_str().filter(|s| !s.is_empty()) {
            ui.label(RichText::new(format!("Ativo agora nos núcleos {l}")).color(GREEN));
        }

        // ---------------- memória ----------------
        ui.add_space(8.0);
        ui.heading("Memória");
        egui::Grid::new("mem").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
            let multi = topo.nodes.len() > 1;
            if multi {
            ui.label("Balanceamento NUMA do kernel")
                .on_hover_text("O kernel move páginas sozinho para perto de quem usa. Com o numapilot cuidando do lugar dos programas, desligar evita migrações que causam travadinhas.");
            tri(ui, "nb", &mut m.numa_balancing, "ligado", "desligado");
            ui.end_row();
            ui.label("Recuperar memória no próprio nó (zone_reclaim)")
                .on_hover_text("Ligado: prefere liberar cache do mesmo nó a usar memória do outro. Bom para latência, ruim para cache de disco.");
            tri(ui, "zr", &mut m.zone_reclaim, "ligado", "desligado");
            ui.end_row();
            }
            ui.label("Transparent Huge Pages")
                .on_hover_text("always: menos falhas de TLB (bom para IA e render na CPU); madvise: só quem pede (padrão).");
            egui::ComboBox::from_id_salt("thp")
                .selected_text(m.thp.clone().unwrap_or_else(|| format!("não mexer ({})", machine::thp().unwrap_or("?".into()))))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut m.thp, None, "não mexer");
                    for t in ["always", "madvise", "never"] {
                        ui.selectable_value(&mut m.thp, Some(t.to_string()), t);
                    }
                });
            ui.end_row();
            for n in &topo.nodes {
                ui.label(if multi { format!("Hugepages reservadas no socket {}", n.id) } else { "Hugepages reservadas".to_string() }).on_hover_text(
                    "Páginas de 2 MB separadas no boot, para programas que usam hugetlbfs (máquinas virtuais, DPDK, alguns bancos de dados). \
                     O llama.cpp e o PyTorch não usam: para eles valem THP e a memória no socket certo.",
                );
                ui.horizontal(|ui| {
                    let k = n.id.to_string();
                    let mut on = m.hugepages_mb.contains_key(&k);
                    let mut gb = m.hugepages_mb.get(&k).copied().unwrap_or(0) as f64 / 1024.0;
                    ui.checkbox(&mut on, "");
                    ui.add_enabled(on, egui::DragValue::new(&mut gb).range(0.0..=(n.mem_total_kb as f64 / 1048576.0 * 0.8).floor()).speed(0.5).suffix(" GB"));
                    if on {
                        m.hugepages_mb.insert(k, (gb * 1024.0) as u64);
                    } else {
                        m.hugepages_mb.remove(&k);
                    }
                    if let Some(h) = machine::hugepages(n.id) {
                        ui.label(RichText::new(format!("agora {:.1} GB", h as f64 * 2.0 / 1024.0)).weak());
                    }
                });
                ui.end_row();
            }
        });

        // ---------------- escalonador ----------------
        ui.add_space(8.0);
        ui.heading("Escalonador (sched_ext)");
        match &mu.scx {
            Some((cur, list)) => {
                ui.horizontal(|ui| {
                    ui.label(format!("Agora: {}", if cur.is_empty() { "padrão do kernel" } else { cur }));
                    egui::ComboBox::from_id_salt("scx")
                        .selected_text(match m.scx.as_deref() {
                            None => "não mexer".to_string(),
                            Some("none") => "padrão do kernel (desligar sched_ext)".to_string(),
                            Some(s) => s.to_string(),
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut m.scx, None, "não mexer");
                            ui.selectable_value(&mut m.scx, Some("none".into()), "padrão do kernel");
                            for s in list {
                                ui.selectable_value(&mut m.scx, Some(s.clone()), s);
                            }
                        });
                    if m.scx.as_deref().is_some_and(|s| s != "none") {
                        egui::ComboBox::from_id_salt("scxm").selected_text(m.scx_mode.clone().unwrap_or("auto".into())).show_ui(ui, |ui| {
                            for x in ["auto", "gaming", "powersave", "lowlatency", "server"] {
                                ui.selectable_value(&mut m.scx_mode, Some(x.to_string()), x);
                            }
                        });
                    }
                });
            }
            None => {
                ui.label(RichText::new("scx_loader não instalado (sudo pacman -S scx-scheds scx-tools).").weak());
            }
        }

        // IPMI (Supermicro)
        if fans::is_supermicro() {
            ui.horizontal(|ui| {
                ui.label("Ventoinhas pelo BMC (IPMI):");
                egui::ComboBox::from_id_salt("ipmi")
                    .selected_text(m.ipmi_fan_mode.map(fans::ipmi_mode_name).unwrap_or("não mexer"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut m.ipmi_fan_mode, None, "não mexer");
                        for x in [0u8, 2, 4, 1] {
                            ui.selectable_value(&mut m.ipmi_fan_mode, Some(x), fans::ipmi_mode_name(x));
                        }
                    });
            });
        }

        ui.add_space(6.0);
        let norm = |m: &Machine| {
            let mut m = m.clone();
            m.sockets.retain(|_, t| !t.is_empty());
            m.mode = String::new();
            m
        };
        let changed = mu.edit.as_ref().is_some_and(|e| norm(e) != norm(&mu.saved));
        ui.horizontal(|ui| {
            if ui.add_enabled(changed, egui::Button::new(RichText::new("Aplicar ajustes da máquina").strong())).clicked()
                && let Some(mut m) = mu.edit.clone()
            {
                m.sockets.retain(|_, t| !t.is_empty());
                m.mode = "personalizado".into();
                mu.edit = Some(m.clone());
                send_machine = Some(m);
            }
            if ui.add_enabled(changed, egui::Button::new("Desfazer")).clicked() {
                mu.edit = Some(mu.saved.clone());
            }
            ui.label(RichText::new("Quem aplica é o serviço; se ele parar, tudo volta ao que era.").small().weak());
        });

        // ---------------- ventoinhas da placa-mãe ----------------
        ui.add_space(10.0);
        ui.heading("Ventoinhas da placa-mãe");
        if mu.fans.is_empty() && mu.ipmi.is_empty() {
            ui.label(
                RichText::new(
                    "Nenhuma ventoinha controlável encontrada. Placas comuns precisam do driver do chip de sensores \
                     (ex.: sudo modprobe nct6775; o sensors-detect diz qual); placas de servidor usam o IPMI (sudo pacman -S ipmitool).",
                )
                .weak(),
            );
        }
        for f in mu.fans.clone() {
            let saved = mu.fans_saved.map.get(&f.id).cloned().unwrap_or_default();
            let e = mu.fan_edit.entry(f.id.clone()).or_insert_with(|| saved.clone());
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(if e.label.is_empty() { f.label.clone() } else { e.label.clone() }).strong());
                    ui.label(RichText::new(format!(
                        "{} · {} · {}",
                        f.id,
                        f.rpm.map(|r| format!("{r} RPM")).unwrap_or("- RPM".into()),
                        f.pct.map(|p| format!("{p:.0}%")).unwrap_or_default()
                    ))
                    .weak());
                });
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut e.label).hint_text("nome (ex.: frente, CPU 0)").desired_width(180.0));
                    ui.radio_value(&mut e.fan, FanMode::Auto, "da placa");
                    ui.radio_value(&mut e.fan, FanMode::Curve, "curva");
                    ui.radio_value(&mut e.fan, FanMode::Fixed, "fixa");
                });
                match e.fan {
                    FanMode::Fixed => {
                        ui.add(egui::Slider::new(&mut e.fixed_pct, 0.0..=100.0).suffix("%"));
                    }
                    FanMode::Curve => {
                        ui.horizontal(|ui| {
                            ui.label("Seguir:");
                            egui::ComboBox::from_id_salt(format!("src{}", f.id)).selected_text(&e.source).show_ui(ui, |ui| {
                                for (s, l) in [("cpu", "CPU mais quente"), ("cpu0", "CPU 0"), ("cpu1", "CPU 1"), ("gpu", "GPU mais quente"), ("max", "o mais quente de tudo")] {
                                    ui.selectable_value(&mut e.source, s.to_string(), l);
                                }
                            });
                            for pt in e.curve.iter_mut() {
                                ui.add(egui::DragValue::new(&mut pt.0).range(20.0..=100.0).suffix("°C"));
                                ui.add(egui::DragValue::new(&mut pt.1).range(0.0..=100.0).suffix("%"));
                                ui.add_space(6.0);
                            }
                        });
                    }
                    FanMode::Auto => {}
                }
                let ch = *e != saved;
                if ui.add_enabled(ch, egui::Button::new("Aplicar")).clicked() {
                    send_fan = Some((f.id.clone(), e.clone()));
                }
            });
        }
        if !mu.ipmi.is_empty() {
            ui.label(RichText::new("Pelo IPMI (só leitura):").strong());
            ui.horizontal_wrapped(|ui| {
                for (n, r) in &mu.ipmi {
                    ui.label(format!("{n}: {}", r.map(|r| format!("{r} RPM")).unwrap_or("-".into())));
                }
            });
        }
        if !mu.fans.is_empty() {
            ui.label(RichText::new(format!("Acima de {:.0} °C na CPU as ventoinhas controladas vão a 100%.", fans::EMERGENCY_CPU_C)).small().weak());
        }

        // ---------------- mitigações ----------------
        ui.add_space(10.0);
        ui.heading("Mitigações do processador");
        ui.label(
            RichText::new(
                "Proteções contra ataques de canal lateral. Desligar devolve desempenho, mas deixa programas maliciosos lerem \
                 dados de outros programas ou do kernel. Valem depois de reiniciar.",
            )
            .weak(),
        );
        let relevant = cmdline::relevant_mitigations(&mu.vulns);
        let all_off = mu.sel.contains("mitigations=off");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Desligar as leves (mantém Spectre e Meltdown)").clicked() {
                mu.sel.remove("mitigations=off");
                for g in cmdline::MITIGATIONS {
                    let on = !g.heavy && relevant.iter().any(|r| r.title == g.title);
                    for p in g.params {
                        if on {
                            mu.sel.insert(p.to_string());
                        } else {
                            mu.sel.remove(*p);
                        }
                    }
                }
            }
            if ui.button("Desligar todas").clicked() {
                mu.sel.insert("mitigations=off".into());
            }
            if ui.button("Todas ligadas (padrão)").clicked() {
                mu.sel.remove("mitigations=off");
                for g in cmdline::MITIGATIONS {
                    for p in g.params {
                        mu.sel.remove(*p);
                    }
                }
            }
        });
        let mut all = all_off;
        if ui.checkbox(&mut all, RichText::new("Desligar todas (mitigations=off)").strong()).changed() {
            if all {
                mu.sel.insert("mitigations=off".into());
            } else {
                mu.sel.remove("mitigations=off");
            }
        }
        egui::Grid::new("mitig").striped(true).num_columns(4).spacing([12.0, 4.0]).show(ui, |ui| {
            for h in ["Desligar", "Situação no seu processador", "Risco sem ela", "Ganho"] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for g in &relevant {
                let mut on = all || g.params.iter().all(|p| mu.sel.contains(*p));
                let r = ui.add_enabled(!all, egui::Checkbox::new(&mut on, g.title));
                if !g.note.is_empty() {
                    r.clone().on_hover_text(g.note);
                }
                if r.changed() {
                    for p in g.params {
                        if on {
                            mu.sel.insert(p.to_string());
                        } else {
                            mu.sel.remove(*p);
                        }
                    }
                }
                let st: Vec<String> = mu
                    .vulns
                    .iter()
                    .filter(|(n, _)| g.vulns.contains(&n.as_str()))
                    .map(|(_, s)| s.split(';').next().unwrap_or(s).to_string())
                    .collect();
                ui.label(RichText::new(st.join(" / ")).small());
                ui.label(RichText::new(g.risk).color(if g.heavy { Color32::from_rgb(220, 80, 60) } else { ORANGE }));
                ui.label(g.gain);
                ui.end_row();
            }
        });
        if relevant.is_empty() {
            ui.label(RichText::new("O kernel não informou vulnerabilidades (ou este processador não é afetado).").weak());
        }

        // ---------------- outros parâmetros ----------------
        ui.add_space(10.0);
        ui.heading("Parâmetros do kernel");
        ui.label(
            RichText::new(match &mu.loader {
                Some(l) => format!("Bootloader: {l}. O numapilot edita esse arquivo (com backup) e regrava as entradas; atualizações do kernel mantêm."),
                None => "Bootloader não reconhecido (Limine, systemd-boot/sdboot-manage, GRUB ou rEFInd): não dá para editar daqui.".into(),
            })
            .weak(),
        );
        for (p, t, d) in cmdline::KNOWN {
            let mut on = mu.sel.contains(*p);
            if ui.checkbox(&mut on, format!("{t} ({p})")).on_hover_text(*d).changed() {
                if on {
                    mu.sel.insert(p.to_string());
                } else {
                    mu.sel.remove(*p);
                }
            }
        }
        ui.horizontal(|ui| {
            ui.label("Outro:");
            ui.add(egui::TextEdit::singleline(&mut mu.custom).hint_text("ex.: amd_iommu=on").desired_width(220.0));
            if ui.small_button("adicionar").clicked() {
                let c = mu.custom.trim().to_string();
                if !c.is_empty() && !c.contains(char::is_whitespace) {
                    mu.sel.insert(c);
                    mu.custom.clear();
                }
            }
        });
        // diferença entre o configurado e o escolhido (só parâmetros que o numapilot conhece ou que foram adicionados aqui)
        let before: BTreeSet<String> = mu.configured.split_whitespace().map(String::from).collect();
        let add: Vec<String> = mu.sel.difference(&before).cloned().collect();
        let rm: Vec<String> = before.difference(&mu.sel).cloned().collect();
        ui.label(RichText::new(format!("Configurado: {}", mu.configured)).small().monospace());
        if !add.is_empty() || !rm.is_empty() {
            ui.label(
                RichText::new(format!(
                    "Vai mudar: {}{}",
                    add.iter().map(|a| format!("+{a} ")).collect::<String>(),
                    rm.iter().map(|a| format!("-{a} ")).collect::<String>()
                ))
                .color(ORANGE),
            );
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(mu.loader.is_some() && (!add.is_empty() || !rm.is_empty()), egui::Button::new(RichText::new("Aplicar no boot").strong()))
                .on_hover_text("Pede a senha, faz backup do arquivo do bootloader e regrava as entradas. Vale depois de reiniciar.")
                .clicked()
            {
                let mut argv = vec!["pkexec".into(), crate::jobs::sibling_abs("numapilot"), "boot".into(), "cmdline".into(), "apply".into()];
                argv.extend(add.iter().map(|a| format!("+{a}")));
                argv.extend(rm.iter().map(|a| format!("-{a}")));
                job = Some(("Parâmetros do kernel".into(), argv));
                mu.boot_loaded = false;
            }
            if ui.button("Recarregar").clicked() {
                mu.boot_loaded = false;
                mu.read_at = None;
            }
        });
        let od = numapilot::bootcfg::overdrive_state();
        ui.label(
            RichText::new(format!(
                "OverDrive das placas AMD: {} (fica em Configurações e na aba Placas de vídeo).",
                if od.on_next_boot { "ligado" } else { "desligado" }
            ))
            .small()
            .weak(),
        );
    });

    if let Some(e) = app_toast {
        app.toast(e, false);
    }
    if let Some(m) = send_machine {
        app.send(Request::SetMachine { machine: m });
        app.mach.read_at = None;
    }
    if let Some((id, control)) = send_fan {
        app.send(Request::SetBoardFan { id, control });
        app.mach.fan_edit.clear();
        app.mach.read_at = None;
    }
    if let Some((t, argv)) = job {
        app.start_job(t, argv);
    }
}
