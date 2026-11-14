//! "Machine" tab: one-click modes, processors (governor, EPB, frequency, uncore, power),
//! gaming latency, memory, scheduler, motherboard fans, mitigations and boot.

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
    nics: Vec<numapilot::wol::Nic>,
    nics_at: Option<Instant>,
    health: Option<numapilot::health::Report>,
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
        self.health = numapilot::health::load();
        if self.nics_at.is_none_or(|t| t.elapsed().as_secs() >= 10) {
            self.nics_at = Some(Instant::now());
            self.nics = numapilot::wol::nics();
        }
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
        None => crate::i18n::tr("leave as is"),
        Some(true) => on,
        Some(false) => off,
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(txt)
        .show_ui(ui, |ui| {
            ui.selectable_value(v, None, crate::i18n::tr("leave as is"));
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
    (4, "balanced (performance)"),
    (6, "normal"),
    (8, "balanced (power saving)"),
    (15, "economia"),
];

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    app.mach.refresh();
    let topo = app.topo.clone();
    let mut send_machine: Option<Machine> = None;
    let mut send_fan: Option<(String, BoardFanControl)> = None;
    let mut job: Option<(String, Vec<String>)> = None;
    let mut app_toast: Option<String> = None;
    let mut dgpu_send: Option<Option<String>> = None;
    let mut app_toast_ok2: Option<String> = None;

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mu = &mut app.mach;
        // ---------------- modos ----------------
        ui.heading(crate::i18n::tr("Machine mode"));
        ui.label(RichText::new(crate::i18n::tr("One click tunes processors, memory and scheduler together. \"Normal\" puts everything back as the system was.")).weak());
        ui.horizontal_wrapped(|ui| {
            let game_node = app
                .cfg
                .graphics_gpu
                .as_deref()
                .and_then(|a| topo.gpus.iter().find(|g| g.alias == a || g.pci == a))
                .or_else(|| topo.display_gpu())
                .and_then(|g| g.numa_node);
            for (name, label, tip) in [
                ("normal", "Normal", crate::i18n::tr("Everything as the system was.")),
                ("jogo", crate::i18n::tr("Game"), crate::i18n::tr("Graphics card's socket at maximum performance, uncore at maximum, turbo, no automatic NUMA balancing and the LAVD scheduler in gaming mode.")),
                ("ia", crate::i18n::tr("AI"), crate::i18n::tr("Both sockets in performance, uncore at maximum, THP always and no automatic NUMA balancing.")),
                ("silencioso", crate::i18n::tr("Quiet"), crate::i18n::tr("No turbo, power-saving governor, EPB power saving, power limited to 60% and LAVD in power-saving mode.")),
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
                ui.label(RichText::new(crate::tf!("current: {}", "atual: {}", mu.saved.mode)).weak());
            }
        });
        let Some(m) = mu.edit.as_mut() else { return };

        // ---------------- notebook ----------------
        if numapilot::notebook::is_laptop() {
            use numapilot::notebook as nb;
            ui.add_space(8.0);
            ui.heading(crate::i18n::tr("Laptop"));
            let ac = nb::on_ac();
            for b in nb::battery_info() {
                ui.label(crate::tf!(
                    "Battery {}: {}% · {} · {}{}", "Bateria {}: {}% · {} · {}{}",
                    b.name,
                    b.pct.map(|p| p.to_string()).unwrap_or("?".into()),
                    match b.status.as_str() {
                        "Charging" => "carregando",
                        "Discharging" => "descarregando",
                        "Full" => "cheia",
                        "Not charging" => crate::i18n::tr("holding at the limit"),
                        o => o,
                    },
                    b.power_w.map(|w| format!("{w:.1} W")).unwrap_or_default(),
                    b.health_pct.map(|h| crate::tf!(" · health {h:.0}%", " · saúde {h:.0}%")).unwrap_or_default()
                ));
            }
            // ---- displays and NVIDIA mode (switched with the session open) ----
            let dstate = numapilot::hybrid::load_state();
            let pair = numapilot::hybrid::pair(&topo);
            if pair.is_some() || dstate.as_ref().is_some_and(|s| s.removed) {
                use numapilot::hybrid;
                let desc = |g: &numapilot::topo::Gpu| {
                    let outs = hybrid::outputs(&g.pci);
                    let on: Vec<String> = outs
                        .iter()
                        .filter(|o| o.connected)
                        .map(|o| {
                            if o.name.starts_with("eDP") {
                                crate::tf!("laptop display ({})", "tela do notebook ({})", o.name)
                            } else if o.name.starts_with("DP") && g.is_integrated() {
                                format!("{} (USB-C/Thunderbolt)", o.name)
                            } else {
                                o.name.clone()
                            }
                        })
                        .collect();
                    let free: Vec<String> = outs.iter().filter(|o| !o.connected && !o.name.starts_with("eDP")).map(|o| o.name.clone()).collect();
                    format!(
                        "{}{}",
                        if on.is_empty() { crate::i18n::tr("no display").to_string() } else { on.join(", ") },
                        if g.is_nvidia() && !free.is_empty() { crate::tf!(" · free outputs: {}", " · saídas livres: {}", free.join(", ")) } else { String::new() }
                    )
                };
                if let Some(ig) = topo.gpus.iter().find(|g| g.is_integrated()) {
                    ui.label(crate::tf!("Displays on the {}: {}", "Telas na {}: {}", ig.alias, desc(ig)));
                }
                match pair {
                    Some((_, nv)) => {
                        ui.label(crate::tf!("Displays on the {}: {}", "Telas na {}: {}", nv.alias, desc(nv)));
                    }
                    None => {
                        ui.label(crate::tf!(
                            "{}: turned off by numapilot (removed from the system; HDMI does not work)", "{}: desligada pelo numapilot (fora do sistema; o HDMI não funciona)",
                            dstate.as_ref().map(|d| d.alias.as_str()).unwrap_or("NVIDIA")
                        ));
                    }
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("NVIDIA:").strong()).on_hover_text(
                        crate::i18n::tr("Switches right away, without leaving the session: \"off\" removes the card from the system (zero power, nothing wakes it, KDE carries on \
                         with the integrated GPU only and HDMI does not work); \"available\" brings it back and KDE adds it by itself. \
                         It only turns off if no program is using the NVIDIA."),
                    );
                    let mut v = m.dgpu_mode.clone();
                    egui::ComboBox::from_id_salt("dgpumode")
                        .selected_text(
                            v.as_deref()
                                .and_then(|x| hybrid::MODES.iter().find(|y| y.0 == x))
                                .map(|y| crate::i18n::tr(y.1))
                                .unwrap_or(crate::i18n::tr("leave as is (sleeps when nobody uses it)")),
                        )
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut v, None, crate::i18n::tr("leave as is (sleeps when nobody uses it)"));
                            for (k, l) in hybrid::MODES {
                                ui.selectable_value(&mut v, Some(k.to_string()), crate::i18n::tr(l));
                            }
                        });
                    if v != m.dgpu_mode {
                        m.dgpu_mode = v.clone();
                        hybrid::remove_login_files();
                        dgpu_send = Some(v);
                    }
                });
                if let Some(st) = &dstate {
                    if st.sleep_only && !st.want_present {
                        ui.label(
                            RichText::new(crate::tf!(
                                "KDE keeps the NVIDIA's video output open, so it is not removed from the system (not safe with the desktop holding it). With no programs using it, it sleeps by itself: now {}.", "O KDE mantém a saída de vídeo da NVIDIA aberta, então ela não é tirada do sistema (não é seguro com o desktop segurando). Sem programas usando, ela dorme sozinha: agora {}.",
                                nb::dgpu_state(&st.pci)
                            ))
                            .small()
                            .weak(),
                        );
                    }
                    for n in &st.blocked_by {
                        if let Some(h) = nb::holder_hint(n) {
                            ui.label(RichText::new(format!("• {h}")).small());
                        }
                    }
                    if !st.blocked_by.is_empty() && !st.want_present {
                        ui.label(
                            RichText::new(crate::tf!(
                                "Waiting to turn off the NVIDIA: in use by {}. Close those programs (or open them on the integrated GPU).", "Esperando para desligar a NVIDIA: em uso por {}. Feche esses programas (ou abra-os na integrada).",
                                st.blocked_by.join(", ")
                            ))
                            .color(Color32::from_rgb(230, 160, 40)),
                        );
                    }
                    if let Some(e) = &st.last_error {
                        ui.label(RichText::new(crate::tf!("Error: {e}", "Erro: {e}")).color(Color32::from_rgb(229, 83, 75)));
                    }
                }
                {
                    use numapilot::autostart;
                    let mut on = autostart::enabled();
                    if ui
                        .checkbox(&mut on, crate::i18n::tr("Everything that starts with the system on the integrated GPU"))
                        .on_hover_text(
                            crate::i18n::tr("Startup programs (ZapZap, notifiers...) and user services (e.g. Sunshine) \
                             use only the 680M, so they do not wake the NVIDIA. Game launchers (Steam, Heroic, Lutris) \
                             stay as they are: games keep running on the NVIDIA. Applies from the next login."),
                        )
                        .changed()
                    {
                        if on {
                            match autostart::apply(&topo) {
                                Ok(ch) => app_toast_ok2 = Some(crate::tf!(
                                    "On the integrated GPU at the next login: {}", "Na integrada no próximo login: {}",
                                    if ch.is_empty() { crate::i18n::tr("nothing new").into() } else { ch.join(", ") }
                                )),
                                Err(e) => app_toast = Some(e),
                            }
                        } else {
                            autostart::revert();
                            app_toast_ok2 = Some(crate::i18n::tr("Startup back to normal (next login)").into());
                        }
                    }
                }
                if !hybrid::xwayland_wrapped() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::i18n::tr("Xwayland may hold the NVIDIA for no reason.")).small().weak());
                        if ui
                            .small_button(crate::i18n::tr("use only the integrated GPU in Xwayland"))
                            .on_hover_text(crate::i18n::tr("Installs /usr/local/bin/Xwayland that uses only Mesa (asks for the password). X11 games set to the NVIDIA still go to it. Applies at the next login."))
                            .clicked()
                        {
                            job = Some((
                                crate::i18n::tr("Xwayland on the integrated GPU only").into(),
                                vec!["pkexec".into(), numapilot::desktop::numapilot_bin(), "hybrid".into(), "xwayland".into(), "on".into()],
                            ));
                        }
                    });
                }
            }
            let fans = nb::ec_fans();
            if !fans.is_empty() {
                ui.label(crate::tf!(
                    "Fans: {}", "Ventoinhas: {}",
                    fans.iter().map(|r| format!("{r} RPM")).collect::<Vec<_>>().join(" · ")
                ));
            }
            egui::Grid::new("nb").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
                let modes = [("normal", "Normal"), ("jogo", crate::i18n::tr("Game")), ("ia", crate::i18n::tr("AI")), ("silencioso", crate::i18n::tr("Quiet"))];
                let combo = |ui: &mut egui::Ui, id: &str, v: &mut Option<String>| {
                    let txt = v.as_deref().and_then(|x| modes.iter().find(|m| m.0 == x)).map(|m| m.1).unwrap_or(crate::i18n::tr("do not switch"));
                    egui::ComboBox::from_id_salt(id).selected_text(txt).show_ui(ui, |ui| {
                        ui.selectable_value(v, None, crate::i18n::tr("do not switch"));
                        for (k, l) in modes {
                            ui.selectable_value(v, Some(k.to_string()), l);
                        }
                    });
                };
                ui.label(crate::tf!("When plugged in{}", "Ao ligar na tomada{}", if ac == Some(true) { crate::i18n::tr(" (now)") } else { "" }));
                combo(ui, "acm", &mut m.ac_mode);
                ui.end_row();
                ui.label(crate::tf!("On battery{}", "Na bateria{}", if ac == Some(false) { crate::i18n::tr(" (now)") } else { "" }));
                combo(ui, "batm", &mut m.battery_mode);
                ui.end_row();
                if nb::battery_info().iter().any(|b| b.charge_limit.is_some()) {
                    ui.label(crate::i18n::tr("Stop charging at")).on_hover_text(crate::i18n::tr("Keeping the battery at 80% when the laptop is always plugged in greatly extends its lifespan."));
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
                    ui.label(crate::i18n::tr("Laptop power profile")).on_hover_text(crate::i18n::tr("Vendor profile (ACPI): changes fans and power limits. The modes already pick one."));
                    egui::ComboBox::from_id_salt("pp")
                        .selected_text(m.platform_profile.clone().unwrap_or_else(|| crate::tf!("leave as is ({})", "não mexer ({})", nb::platform_profile().unwrap_or_default())))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut m.platform_profile, None, crate::i18n::tr("leave as is"));
                            for c in &ch {
                                ui.selectable_value(&mut m.platform_profile, Some(c.clone()), c);
                            }
                        });
                    ui.end_row();
                }
                for g in topo.gpus.iter().filter(|g| !g.is_integrated()) {
                    ui.label(crate::tf!("Dedicated card {}", "Placa dedicada {}", g.alias));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(nb::dgpu_state(&g.pci)).weak());
                        tri(ui, &format!("dpm{}", g.pci), &mut m.dgpu_autosuspend, crate::i18n::tr("sleep when nobody uses it"), crate::i18n::tr("always on"));
                    });
                    ui.end_row();
                    if !nb::dgpu_asleep(&g.pci) {
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
                                RichText::new(crate::tf!("Woken by: {}", "Acordada por: {}", names.join(", ")))
                                    .small()
                                    .color(Color32::from_rgb(230, 160, 40)),
                            )
                            .on_hover_text(crate::i18n::tr("These programs opened the dedicated card; while they are open it does not sleep and drains the battery. Open them on the integrated GPU (program profile → card) to save power."));
                            ui.end_row();
                        }
                    }
                }
                if nb::has_nvidia_powerd() {
                    ui.label(crate::i18n::tr("NVIDIA Dynamic Boost")).on_hover_text(crate::i18n::tr("Shifts power from the processor to the graphics card in games (nvidia-powerd)."));
                    tri(ui, "powerd", &mut m.nvidia_powerd, crate::i18n::tr("on"), crate::i18n::tr("off"));
                    ui.end_row();
                }
            });
        }

        // ---------------- laptop Ryzen (ryzenadj) ----------------
        if numapilot::ryzen::applicable() {
            ui.add_space(8.0);
            ui.heading(crate::i18n::tr("Ryzen power limits"));
            if !numapilot::ryzen::available() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(crate::i18n::tr("Install ryzenadj to tune STAPM/PPT (the equivalent of Intel's PL1/PL2).")).weak());
                    if ui.small_button(crate::i18n::tr("install (AUR)")).clicked() {
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
                    type Row<'a> = (&'a str, &'a str, &'a str, &'a mut Option<f64>, f64, f64);
                    let rows: [Row; 4] = [
                        (crate::i18n::tr("Sustained (STAPM)"), "STAPM LIMIT", " W", &mut r.stapm_w, 5.0, 120.0),
                        (crate::i18n::tr("Short average (slow PPT)"), "PPT LIMIT SLOW", " W", &mut r.slow_w, 5.0, 150.0),
                        (crate::i18n::tr("Peaks (fast PPT)"), "PPT LIMIT FAST", " W", &mut r.fast_w, 5.0, 150.0),
                        (crate::i18n::tr("Maximum temperature"), "THM LIMIT CORE", " °C", &mut r.tctl_c, 60.0, 105.0),
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
                        ui.label(RichText::new(cur.map(|c| crate::tf!("now {c:.0}{unit}", "agora {c:.0}{unit}")).unwrap_or_else(|| crate::i18n::tr("now ? (service)").into())).weak());
                        ui.end_row();
                    }
                });
                if r.is_empty() {
                    m.ryzen = None;
                }
                ui.label(
                    RichText::new(crate::i18n::tr("Lowering STAPM and the temperature keeps the laptop cool and quiet; the service reapplies if the firmware resets the values when switching AC/battery."))
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
                    ui.label(RichText::new(crate::i18n::tr("Fans, the 80% charge limit and RGB need the community module Linuwu-Sense.")).weak());
                    if ui.small_button(crate::i18n::tr("page")).clicked() {
                        let _ = crate::jobs::open_url("https://github.com/0x7375646F/Linuwu-Sense");
                    }
                });
            }
            egui::Grid::new("acer").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
                for (key, cur) in &knobs {
                    let Some((_, _, label, help)) = acer::KNOBS.iter().find(|k| k.0 == *key) else { continue };
                    ui.label(crate::i18n::tr(label)).on_hover_text(crate::i18n::tr(help));
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
                                for (i, l) in [crate::i18n::tr("leave as is"), crate::i18n::tr("automatic"), crate::i18n::tr("maximum"), "manual"].iter().enumerate() {
                                    ui.selectable_value(&mut mode, i, *l);
                                }
                                if mode == 3 {
                                    if (c, g) == (0, 0) || (c, g) == (100, 100) {
                                        (c, g) = (60, 60);
                                    }
                                    ui.add(egui::DragValue::new(&mut c).range(20..=100).prefix("CPU ").suffix("%"));
                                    ui.add(egui::DragValue::new(&mut g).range(20..=100).prefix("GPU ").suffix("%"));
                                }
                                ui.label(RichText::new(crate::tf!("now {cur}", "agora {cur}")).weak());
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
                                .selected_text(v.clone().map(|x| if x == "0" { crate::i18n::tr("no").into() } else { crate::tf!("up to {x}%", "até {x}%") }).unwrap_or_else(|| crate::tf!("leave as is ({cur})", "não mexer ({cur})")))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut v, None, crate::i18n::tr("leave as is"));
                                    for x in ["0", "10", "20", "30"] {
                                        ui.selectable_value(&mut v, Some(x.to_string()), if x == "0" { crate::i18n::tr("no").to_string() } else { crate::tf!("up to {x}%", "até {x}%") });
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
                                tri(ui, &format!("acer{key}"), &mut v, crate::i18n::tr("on"), crate::i18n::tr("off"));
                                ui.label(RichText::new(crate::tf!("now {}", "agora {}", if cur == "1" { crate::i18n::tr("on") } else { crate::i18n::tr("off") })).weak());
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
                    ui.label(crate::i18n::tr("RGB keyboard"));
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
                                .selected_text(acer::EFFECTS.iter().find(|e| e.0 as u32 == mode).map(|e| crate::i18n::tr(e.1)).unwrap_or("?"))
                                .show_ui(ui, |ui| {
                                    for (k, l) in acer::EFFECTS {
                                        ui.selectable_value(&mut mode, *k as u32, crate::i18n::tr(l));
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

        // ---------------- schedules ----------------
        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Scheduled modes"));
        ui.label(
            RichText::new(crate::i18n::tr("E.g. Quiet from 23:00 to 07:00. A program with \"machine mode\" in its profile (Adjust…) takes priority; switching the mode by hand holds until the rule changes."))
                .small()
                .weak(),
        );
        let mut del = None;
        for (i, s) in m.schedule.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("from"));
                ui.add(egui::TextEdit::singleline(&mut s.from).desired_width(48.0));
                ui.label(crate::i18n::tr("to"));
                ui.add(egui::TextEdit::singleline(&mut s.to).desired_width(48.0));
                egui::ComboBox::from_id_salt(format!("sch{i}")).selected_text(s.mode.clone()).show_ui(ui, |ui| {
                    for k in ["silencioso", "normal", "ia", "jogo"] {
                        ui.selectable_value(&mut s.mode, k.to_string(), k);
                    }
                });
                if !s.valid() {
                    ui.label(RichText::new(crate::i18n::tr("invalid time (use HH:MM)")).color(Color32::from_rgb(229, 83, 75)));
                }
                if ui.small_button(crate::i18n::tr("remove")).clicked() {
                    del = Some(i);
                }
            });
        }
        if let Some(i) = del {
            m.schedule.remove(i);
        }
        if ui.small_button(crate::i18n::tr("+ schedule")).clicked() {
            m.schedule.push(machine::Sched { from: "23:00".into(), to: "07:00".into(), mode: "silencioso".into() });
        }

        // ---------------- energia ----------------
        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Energy"));
        {
            let e = numapilot::energy::Energy::load();
            let today = numapilot::energy::today();
            let kwh_today = e.days.get(&today).copied().unwrap_or(0.0);
            let month = e.month(&today[..7]);
            let price = m.price_kwh;
            let money = |k: f64| price.map(|p| format!(" (R$ {:.2})", k * p)).unwrap_or_default();
            let w = mu.hw["watts"].as_f64();
            ui.label(crate::tf!(
                "Now {} · today {kwh_today:.2} kWh{} · this month {month:.1} kWh{}{}", "Agora {} · hoje {kwh_today:.2} kWh{} · este mês {month:.1} kWh{}{}",
                w.map(|w| format!("{w:.0} W")).unwrap_or("? W".into()),
                money(kwh_today),
                money(month),
                e.avg_days(30)
                    .map(|a| crate::tf!(" · average {a:.2} kWh/day, ~{:.0} kWh/month{}", " · média {a:.2} kWh/dia, ~{:.0} kWh/mês{}", a * 30.0, money(a * 30.0)))
                    .unwrap_or_default()
            ));
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("kWh price (R$):"));
                let mut on = m.price_kwh.is_some();
                let mut v = m.price_kwh.unwrap_or(0.9);
                ui.checkbox(&mut on, "");
                ui.add_enabled(on, egui::DragValue::new(&mut v).range(0.05..=5.0).speed(0.01).fixed_decimals(2));
                m.price_kwh = on.then_some(v);
                ui.add_space(12.0);
                ui.label(crate::i18n::tr("Rest of the PC (W):")).on_hover_text(crate::i18n::tr("The service measures processors and graphics cards. Motherboard, memory, disks, fans and power supply losses add some 30-80 W; enter it so the figure gets close to the wall (or measure with a wall meter)."));
                let mut b = m.base_w.unwrap_or(0.0);
                ui.add(egui::DragValue::new(&mut b).range(0.0..=500.0).suffix(" W"));
                m.base_w = (b > 0.0).then_some(b);
            });
            let days: Vec<(&String, &f64)> = e.days.iter().rev().take(14).collect();
            if days.len() > 1 {
                let max = days.iter().map(|d| *d.1).fold(0.0, f64::max).max(0.01);
                ui.horizontal(|ui| {
                    for (d, v) in days.iter().rev() {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 40.0), egui::Sense::hover());
                        let h = (*v / max) as f32 * 40.0;
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(egui::pos2(r.left() + 2.0, r.bottom() - h), egui::pos2(r.right() - 2.0, r.bottom())),
                            2.0,
                            Color32::from_rgb(79, 143, 240),
                        );
                        ui.interact(r, ui.next_auto_id(), egui::Sense::hover()).on_hover_text(format!("{d}: {v:.2} kWh{}", money(**v)));
                    }
                    ui.label(RichText::new(crate::i18n::tr("last 14 days")).small().weak());
                });
            }
        }

        // ---------------- health ----------------
        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Hardware health"));
        match &mu.health {
            None => {
                ui.label(RichText::new(crate::i18n::tr("The service collects 1 min after starting and then every 30 min.")).weak());
            }
            Some(r) => {
                let probs = numapilot::health::problems(r);
                if probs.is_empty() {
                    ui.label(RichText::new(crate::i18n::tr("No problems found.")).color(Color32::from_rgb(110, 190, 120)));
                }
                for (lvl, p) in &probs {
                    ui.label(RichText::new(format!("• {p}")).color(if *lvl >= 2 {
                        Color32::from_rgb(229, 83, 75)
                    } else {
                        Color32::from_rgb(230, 160, 40)
                    }));
                }
                if !r.smartctl {
                    ui.label(RichText::new(crate::i18n::tr("To see the disks: sudo pacman -S smartmontools")).small().weak());
                }
                if !r.dimms.is_empty() {
                    let total: f64 = r.dimms.iter().map(|d| d.size_gb).sum();
                    let conf = r.dimms.iter().filter_map(|d| d.configured).min();
                    let sup = r.dimms.iter().filter_map(|d| d.speed).min();
                    ui.label(crate::tf!(
                        "Memory: {} module(s), {total:.0} GB{}{}", "Memória: {} pente(s), {total:.0} GB{}{}",
                        r.dimms.len(),
                        conf.map(|c| crate::tf!(", running at {c} MT/s", ", rodando a {c} MT/s")).unwrap_or_default(),
                        sup.map(|s| crate::tf!(" (the modules support {s})", " (os pentes suportam {s})")).unwrap_or_default()
                    ))
                    .on_hover_text(
                        r.dimms
                            .iter()
                            .map(|d| format!("{} {}: {:.0} GB {}", d.bank, d.slot, d.size_gb, d.kind))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                    let node_gb: Vec<f64> = app.topo.nodes.iter().map(|n| n.mem_total_kb as f64 / 1048576.0).collect();
                    for n in numapilot::health::mem_notes(&r.dimms, &node_gb) {
                        ui.label(RichText::new(n).small().color(Color32::from_rgb(230, 160, 40)));
                    }
                }
                for e in &r.disk_errors {
                    ui.label(RichText::new(crate::tf!("disk not read: {e}", "disco não lido: {e}")).small().color(Color32::from_rgb(230, 160, 40)));
                }
                egui::Grid::new("health").striped(true).num_columns(5).spacing([14.0, 4.0]).show(ui, |ui| {
                    for d in &r.disks {
                        ui.label(format!("{} {}", d.dev, d.model));
                        ui.label(match d.passed {
                            Some(true) => "SMART ok",
                            Some(false) => crate::i18n::tr("SMART FAILED"),
                            None => "?",
                        });
                        ui.label(d.temp_c.map(|t| format!("{t:.0} °C")).unwrap_or_default());
                        ui.label(match (d.used_pct, d.hours) {
                            (Some(u), Some(h)) => crate::tf!("{u}% used · {h} h powered on", "{u}% usado · {h} h ligado"),
                            (None, Some(h)) => crate::tf!("{h} h powered on", "{h} h ligado"),
                            _ => String::new(),
                        });
                        ui.label(d.written_tb.map(|w| crate::tf!("{w:.1} TB written", "{w:.1} TB escritos")).unwrap_or_default());
                        ui.end_row();
                    }
                    for m in &r.mem {
                        ui.label(crate::tf!("memory {}", "memória {}", m.name));
                        ui.label(if m.ue > 0 { crate::i18n::tr("ERRORS") } else { "ECC ok" });
                        ui.label(crate::tf!("{} corrected", "{} corrigidos", m.ce));
                        ui.label(crate::tf!("{} uncorrected", "{} não corrigidos", m.ue));
                        ui.label("");
                        ui.end_row();
                    }
                    for g in r.gpu.iter().filter(|g| g.block == "umc" || g.ce > 0 || g.ue > 0) {
                        let alias = topo.gpus.iter().find(|x| x.pci == g.pci).map(|x| x.alias.clone()).unwrap_or(g.pci.clone());
                        ui.label(format!("{alias} ({})", if g.block == "umc" { crate::i18n::tr("memory") } else { g.block.as_str() }));
                        ui.label(if g.ue > 0 { crate::i18n::tr("ERRORS") } else { "ECC ok" });
                        ui.label(crate::tf!("{} corrected", "{} corrigidos", g.ce));
                        ui.label(crate::tf!("{} uncorrected", "{} não corrigidos", g.ue));
                        ui.label("");
                        ui.end_row();
                    }
                    for b in &r.batteries {
                        ui.label(crate::tf!("battery {}", "bateria {}", b.name));
                        ui.label(b.health_pct.map(|h| crate::tf!("{h:.0}% of original", "{h:.0}% da original")).unwrap_or_default());
                        ui.label(b.cycles.map(|c| crate::tf!("{c} cycles", "{c} ciclos")).unwrap_or_default());
                        ui.label("");
                        ui.label("");
                        ui.end_row();
                    }
                });
                let age = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
                    .saturating_sub(r.time);
                ui.label(RichText::new(crate::tf!("collected {} min ago", "coletado há {} min", age / 60)).small().weak());
            }
        }

        // ---------------- network: wake over the network ----------------
        if !mu.nics.is_empty() {
            ui.add_space(8.0);
            ui.heading(crate::i18n::tr("Wake over the network (Wake-on-LAN)"));
            ui.label(
                RichText::new(crate::i18n::tr("Lets this PC be turned on from another one's panel (My PCs → Wake). Also needs the BIOS option (Wake on LAN / Power on by PCI-E) and the network cable."))
                    .small()
                    .weak(),
            );
            for n in &mu.nics {
                ui.horizontal(|ui| {
                    let mut on = m.wol.contains(&n.iface);
                    let supports = n.supports.as_deref().is_none_or(|s| s.contains('g'));
                    if ui.add_enabled(supports, egui::Checkbox::new(&mut on, format!("{} ({})", n.iface, n.mac))).changed() {
                        m.wol.retain(|i| i != &n.iface);
                        if on {
                            m.wol.push(n.iface.clone());
                        }
                    }
                    ui.label(
                        RichText::new(match n.wol.as_deref() {
                            Some("g") => crate::i18n::tr("now: on").to_string(),
                            Some("d") => crate::i18n::tr("now: off").to_string(),
                            Some(o) => crate::tf!("now: {o}", "agora: {o}"),
                            None => crate::i18n::tr("now: ? (install ethtool)").to_string(),
                        })
                        .weak(),
                    );
                    if !supports {
                        ui.label(RichText::new(crate::i18n::tr("this adapter does not support it")).small().weak());
                    }
                });
            }
        }

        // ---------------- processadores ----------------
        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Processors"));
        ui.horizontal(|ui| {
            ui.label("Turbo:");
            tri(ui, "turbo", &mut m.turbo, crate::i18n::tr("on"), crate::i18n::tr("off"));
            if let Some(t) = cpuctl::turbo() {
                ui.label(RichText::new(crate::tf!("now {}", "agora {}", if t { crate::i18n::tr("on") } else { crate::i18n::tr("off") })).weak());
            }
        });
        let unc = cpuctl::uncores();
        let rapl = cpuctl::rapl_zones();
        let temps = cpuctl::package_temps();
        egui::Grid::new("sockets").striped(true).num_columns(topo.nodes.len() + 1).spacing([18.0, 6.0]).show(ui, |ui| {
            ui.label("");
            for n in &topo.nodes {
                ui.label(RichText::new(if topo.nodes.len() == 1 { crate::i18n::tr("Processor").to_string() } else { format!("Socket {}", n.id) }).strong());
            }
            ui.end_row();
            ui.label(crate::i18n::tr("Now"));
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
                    .selected_text(t.governor.clone().unwrap_or_else(|| crate::tf!("leave as is ({cur})", "não mexer ({cur})")))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut t.governor, None, crate::i18n::tr("leave as is"));
                        for g in cpuctl::governors_available(c) {
                            ui.selectable_value(&mut t.governor, Some(g.clone()), g);
                        }
                    });
            }
            ui.end_row();
            ui.label(crate::i18n::tr("Energy bias (EPB)"));
            for n in &topo.nodes {
                let c = n.cpus.first().copied().unwrap_or(0);
                let t = m.socket_mut(n.id);
                let name = |v: u8| EPB.iter().find(|e| e.0 == v).map(|e| crate::i18n::tr(e.1).to_string()).unwrap_or(v.to_string());
                egui::ComboBox::from_id_salt(format!("epb{}", n.id))
                    .selected_text(t.epb.map(name).unwrap_or_else(|| crate::tf!("leave as is ({})", "não mexer ({})", cpuctl::epb(c).map(name).unwrap_or("?".into()))))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut t.epb, None, crate::i18n::tr("leave as is"));
                        for (v, l) in EPB {
                            ui.selectable_value(&mut t.epb, Some(*v), crate::i18n::tr(l));
                        }
                    });
            }
            ui.end_row();
            let c0 = topo.nodes.first().and_then(|n| n.cpus.first().copied()).unwrap_or(0);
            if !cpuctl::epp_available(c0).is_empty() {
                ui.label(crate::i18n::tr("Energy preference (EPP)")).on_hover_text(
                    crate::i18n::tr("Ryzen's main knob (amd-pstate): from performance to power. With the performance governor the driver locks it to performance."),
                );
                for n in &topo.nodes {
                    let c = n.cpus.first().copied().unwrap_or(0);
                    let t = m.socket_mut(n.id);
                    egui::ComboBox::from_id_salt(format!("epp{}", n.id))
                        .selected_text(t.epp.clone().unwrap_or_else(|| crate::tf!("leave as is ({})", "não mexer ({})", cpuctl::epp(c).unwrap_or_default())))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut t.epp, None, crate::i18n::tr("leave as is"));
                            for e in cpuctl::epp_available(c) {
                                if !["default", "custom", "dynamic"].contains(&e.as_str()) {
                                    ui.selectable_value(&mut t.epp, Some(e.clone()), e);
                                }
                            }
                        });
                }
                ui.end_row();
            }
            ui.label(crate::i18n::tr("Min / max frequency"));
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
            ui.label(crate::i18n::tr("Uncore (L3 + memory)")).on_hover_text(crate::i18n::tr("Higher uncore = lower memory and L3 cache latency, at the cost of a few watts."));
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
                                ui.label(RichText::new(crate::tf!("now {}", "agora {}", cur / 1000)).weak());
                            }
                        });
                    }
                    None => {
                        ui.label(RichText::new(crate::i18n::tr("unavailable (sudo modprobe intel_uncore_frequency)")).weak());
                    }
                }
            }
            ui.end_row();
            ui.label(crate::i18n::tr("Power limit"));
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
                        ui.label(RichText::new(crate::i18n::tr("unavailable (RAPL)")).weak());
                    }
                }
            }
            ui.end_row();
            if rapl.iter().any(|r| r.limit2_w.is_some()) {
                ui.label(crate::i18n::tr("Power peak (PL2)")).on_hover_text(crate::i18n::tr("How much the processor may draw for a few seconds before dropping to the limit above."));
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

        // ---------------- latency ----------------
        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Gaming latency"));
        ui.label(
            RichText::new(
                crate::i18n::tr("Applies while a program with \"Low latency\" runs (the Game profile turns it on): on its cores, the processor does not enter \
                 the deep idle states (waking from them causes micro-stutter) and the socket's uncore stays at maximum. On close, everything goes back."),
            )
            .weak(),
        );
        let cs = cpuctl::cstates(topo.nodes.first().and_then(|n| n.cpus.first().copied()).unwrap_or(0));
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Allowed states:"));
            let label = |us: u64| {
                let allowed: Vec<String> = cs.iter().filter(|s| s.index > 0 && s.latency_us <= us).map(|s| s.name.clone()).collect();
                if allowed.is_empty() { crate::tf!("none (≤ {us} µs)", "nenhum (≤ {us} µs)") } else { format!("{} (≤ {us} µs)", allowed.join(", ")) }
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
            ui.checkbox(&mut m.game_uncore_max, crate::i18n::tr("uncore at maximum"));
        });
        if let Some(l) = mu.hw["lowlat"].as_str().filter(|s| !s.is_empty()) {
            ui.label(RichText::new(crate::tf!("Active now on cores {l}", "Ativo agora nos núcleos {l}")).color(GREEN));
        }

        // ---------------- memory ----------------
        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Memory"));
        egui::Grid::new("mem").num_columns(2).spacing([14.0, 6.0]).show(ui, |ui| {
            let multi = topo.nodes.len() > 1;
            if multi {
            ui.label(crate::i18n::tr("Kernel NUMA balancing"))
                .on_hover_text(crate::i18n::tr("The kernel moves pages by itself closer to whoever uses them. With numapilot placing the programs, turning it off avoids migrations that cause micro-stutter."));
            tri(ui, "nb", &mut m.numa_balancing, crate::i18n::tr("on"), crate::i18n::tr("off"));
            ui.end_row();
            ui.label(crate::i18n::tr("Reclaim memory on the same node (zone_reclaim)"))
                .on_hover_text(crate::i18n::tr("On: prefers freeing cache on the same node over using memory from the other. Good for latency, bad for disk cache."));
            tri(ui, "zr", &mut m.zone_reclaim, crate::i18n::tr("on"), crate::i18n::tr("off"));
            ui.end_row();
            }
            ui.label("Transparent Huge Pages")
                .on_hover_text(crate::i18n::tr("always: fewer TLB misses (good for AI and CPU rendering); madvise: only for those that ask (default)."));
            egui::ComboBox::from_id_salt("thp")
                .selected_text(m.thp.clone().unwrap_or_else(|| crate::tf!("leave as is ({})", "não mexer ({})", machine::thp().unwrap_or("?".into()))))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut m.thp, None, crate::i18n::tr("leave as is"));
                    for t in ["always", "madvise", "never"] {
                        ui.selectable_value(&mut m.thp, Some(t.to_string()), t);
                    }
                });
            ui.end_row();
            for n in &topo.nodes {
                ui.label(if multi { crate::tf!("Hugepages reserved on socket {}", "Hugepages reservadas no socket {}", n.id) } else { crate::i18n::tr("Reserved hugepages").to_string() }).on_hover_text(
                    crate::i18n::tr("2 MB pages set aside at boot, for programs that use hugetlbfs (virtual machines, DPDK, some databases). \
                     llama.cpp and PyTorch do not use them: for those, THP and memory on the right socket are what count."),
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
                        ui.label(RichText::new(crate::tf!("now {:.1} GB", "agora {:.1} GB", h as f64 * 2.0 / 1024.0)).weak());
                    }
                });
                ui.end_row();
            }
        });

        // ---------------- escalonador ----------------
        ui.add_space(8.0);
        ui.heading(crate::i18n::tr("Scheduler (sched_ext)"));
        match &mu.scx {
            Some((cur, list)) => {
                ui.horizontal(|ui| {
                    ui.label(crate::tf!("Now: {}", "Agora: {}", if cur.is_empty() { crate::i18n::tr("kernel default") } else { cur }));
                    egui::ComboBox::from_id_salt("scx")
                        .selected_text(match m.scx.as_deref() {
                            None => crate::i18n::tr("leave as is").to_string(),
                            Some("none") => crate::i18n::tr("kernel default (turn sched_ext off)").to_string(),
                            Some(s) => s.to_string(),
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut m.scx, None, crate::i18n::tr("leave as is"));
                            ui.selectable_value(&mut m.scx, Some("none".into()), crate::i18n::tr("kernel default"));
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
                ui.label(RichText::new(crate::i18n::tr("scx_loader not installed (sudo pacman -S scx-scheds scx-tools).")).weak());
            }
        }

        // IPMI (Supermicro)
        if fans::is_supermicro() {
            ui.horizontal(|ui| {
                ui.label(crate::i18n::tr("Fans through the BMC (IPMI):"));
                egui::ComboBox::from_id_salt("ipmi")
                    .selected_text(m.ipmi_fan_mode.map(fans::ipmi_mode_name).unwrap_or(crate::i18n::tr("leave as is")))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut m.ipmi_fan_mode, None, crate::i18n::tr("leave as is"));
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
            if ui.add_enabled(changed, egui::Button::new(RichText::new(crate::i18n::tr("Apply machine settings")).strong())).clicked()
                && let Some(mut m) = mu.edit.clone()
            {
                m.sockets.retain(|_, t| !t.is_empty());
                m.mode = "personalizado".into();
                mu.edit = Some(m.clone());
                send_machine = Some(m);
            }
            if ui.add_enabled(changed, egui::Button::new(crate::i18n::tr("Undo"))).clicked() {
                mu.edit = Some(mu.saved.clone());
            }
            ui.label(RichText::new(crate::i18n::tr("The service applies them; if it stops, everything goes back to what it was.")).small().weak());
        });

        // ---------------- motherboard fans ----------------
        ui.add_space(10.0);
        ui.heading(crate::i18n::tr("Motherboard fans"));
        if mu.fans.is_empty() && mu.ipmi.is_empty() {
            ui.label(
                RichText::new(
                    crate::i18n::tr("No controllable fan found. Regular boards need the sensor chip's driver \
                     (e.g. sudo modprobe nct6775; sensors-detect tells which); server boards use IPMI (sudo pacman -S ipmitool)."),
                )
                .weak(),
            );
        }
        // channels with nothing plugged in (0 RPM and not controlled) go to a folded group
        let (connected, empty): (Vec<BoardFan>, Vec<BoardFan>) = mu.fans.clone().into_iter().partition(|f| {
            f.rpm.is_some_and(|r| r > 0) || mu.fans_saved.map.get(&f.id).is_some_and(|c| c.fan != FanMode::Auto)
        });
        let mut shown = connected;
        let show_empty = !empty.is_empty()
            && ui
            .collapsing(
                crate::tf!(
                    "{} channel(s) with no fan spinning",
                    "{} canal(is) sem ventoinha girando",
                    empty.len()
                ),
                |ui| {
                    ui.label(RichText::new(crate::t2!(
                        "Shown here so they can be named or forced; a fan that is stopped by the board's curve also shows 0 RPM.",
                        "Aparecem aqui para dar nome ou forçar; uma ventoinha parada pela curva da placa também mostra 0 RPM."
                    )).small().weak());
                },
            )
            .fully_open();
        if show_empty {
            shown.extend(empty);
        }
        for f in shown {
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
                    ui.add(egui::TextEdit::singleline(&mut e.label).hint_text(crate::i18n::tr("name (e.g. front, CPU 0)")).desired_width(180.0));
                    ui.radio_value(&mut e.fan, FanMode::Auto, crate::i18n::tr("board's own"));
                    ui.radio_value(&mut e.fan, FanMode::Curve, crate::i18n::tr("curve"));
                    ui.radio_value(&mut e.fan, FanMode::Fixed, crate::i18n::tr("fixed"));
                });
                match e.fan {
                    FanMode::Fixed => {
                        ui.add(egui::Slider::new(&mut e.fixed_pct, 0.0..=100.0).suffix("%"));
                    }
                    FanMode::Curve => {
                        ui.horizontal(|ui| {
                            ui.label(crate::i18n::tr("Follow:"));
                            egui::ComboBox::from_id_salt(format!("src{}", f.id)).selected_text(&e.source).show_ui(ui, |ui| {
                                for (s, l) in [("cpu", crate::i18n::tr("Hottest CPU")), ("cpu0", "CPU 0"), ("cpu1", "CPU 1"), ("gpu", crate::i18n::tr("Hottest GPU")), ("vrm", crate::t2!("Voltage regulators (VRM)", "Reguladores de tensão (VRM)")), ("max", crate::i18n::tr("the hottest of all"))] {
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
                if ui.add_enabled(ch, egui::Button::new(crate::i18n::tr("Apply"))).clicked() {
                    send_fan = Some((f.id.clone(), e.clone()));
                }
            });
        }
        if !mu.ipmi.is_empty() {
            ui.label(RichText::new(crate::i18n::tr("Through IPMI (read-only):")).strong());
            ui.horizontal_wrapped(|ui| {
                for (n, r) in &mu.ipmi {
                    ui.label(format!("{n}: {}", r.map(|r| format!("{r} RPM")).unwrap_or("-".into())));
                }
            });
        }
        if !mu.fans.is_empty() {
            ui.label(RichText::new(crate::tf!("Above {:.0} °C on the CPU the controlled fans go to 100%.", "Acima de {:.0} °C na CPU as ventoinhas controladas vão a 100%.", fans::EMERGENCY_CPU_C)).small().weak());
        }

        // ---------------- mitigations ----------------
        ui.add_space(10.0);
        ui.heading(crate::i18n::tr("Processor mitigations"));
        ui.label(
            RichText::new(
                crate::i18n::tr("Protections against side-channel attacks. Turning them off gives performance back, but lets malicious programs read \
                 data from other programs or from the kernel. They apply after a reboot."),
            )
            .weak(),
        );
        let relevant = cmdline::relevant_mitigations(&mu.vulns);
        let all_off = mu.sel.contains("mitigations=off");
        ui.horizontal_wrapped(|ui| {
            if ui.button(crate::i18n::tr("Turn off the light ones (keeps Spectre and Meltdown)")).clicked() {
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
            if ui.button(crate::i18n::tr("Turn all off")).clicked() {
                mu.sel.insert("mitigations=off".into());
            }
            if ui.button(crate::i18n::tr("All on (default)")).clicked() {
                mu.sel.remove("mitigations=off");
                for g in cmdline::MITIGATIONS {
                    for p in g.params {
                        mu.sel.remove(*p);
                    }
                }
            }
        });
        let mut all = all_off;
        if ui.checkbox(&mut all, RichText::new(crate::i18n::tr("Turn all off (mitigations=off)")).strong()).changed() {
            if all {
                mu.sel.insert("mitigations=off".into());
            } else {
                mu.sel.remove("mitigations=off");
            }
        }
        egui::Grid::new("mitig").striped(true).num_columns(4).spacing([12.0, 4.0]).show(ui, |ui| {
            for h in [crate::i18n::tr("Turn off"), crate::i18n::tr("Status on your processor"), crate::i18n::tr("Risk without it"), crate::i18n::tr("Gain")] {
                ui.label(RichText::new(h).strong());
            }
            ui.end_row();
            for g in &relevant {
                let mut on = all || g.params.iter().all(|p| mu.sel.contains(*p));
                let r = ui.add_enabled(!all, egui::Checkbox::new(&mut on, crate::i18n::tr(g.title)));
                if !g.note.is_empty() {
                    r.clone().on_hover_text(crate::i18n::tr(g.note));
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
                ui.label(RichText::new(crate::i18n::tr(g.risk)).color(if g.heavy { Color32::from_rgb(220, 80, 60) } else { ORANGE }));
                ui.label(crate::i18n::tr(g.gain));
                ui.end_row();
            }
        });
        if relevant.is_empty() {
            ui.label(RichText::new(crate::i18n::tr("The kernel reported no vulnerabilities (or this processor is not affected).")).weak());
        }

        // ---------------- other parameters ----------------
        ui.add_space(10.0);
        ui.heading(crate::i18n::tr("Kernel parameters"));
        ui.label(
            RichText::new(match &mu.loader {
                Some(l) => crate::tf!("Bootloader: {l}. numapilot edits this file (with a backup) and rewrites the entries; kernel updates keep them.", "Bootloader: {l}. O numapilot edita esse arquivo (com backup) e regrava as entradas; atualizações do kernel mantêm."),
                None => crate::i18n::tr("Bootloader not recognized (Limine, systemd-boot/sdboot-manage, GRUB or rEFInd): cannot be edited from here.").into(),
            })
            .weak(),
        );
        for (p, t, d) in cmdline::KNOWN {
            let mut on = mu.sel.contains(*p);
            if ui.checkbox(&mut on, format!("{} ({p})", crate::i18n::tr(t))).on_hover_text(crate::i18n::tr(d)).changed() {
                if on {
                    mu.sel.insert(p.to_string());
                } else {
                    mu.sel.remove(*p);
                }
            }
        }
        ui.horizontal(|ui| {
            ui.label(crate::i18n::tr("Other:"));
            ui.add(egui::TextEdit::singleline(&mut mu.custom).hint_text(crate::i18n::tr("e.g. amd_iommu=on")).desired_width(220.0));
            if ui.small_button(crate::i18n::tr("add")).clicked() {
                let c = mu.custom.trim().to_string();
                if !c.is_empty() && !c.contains(char::is_whitespace) {
                    mu.sel.insert(c);
                    mu.custom.clear();
                }
            }
        });
        // difference between what is configured and what was chosen (only parameters numapilot knows or that were added here)
        let before: BTreeSet<String> = mu.configured.split_whitespace().map(String::from).collect();
        let add: Vec<String> = mu.sel.difference(&before).cloned().collect();
        let rm: Vec<String> = before.difference(&mu.sel).cloned().collect();
        ui.label(RichText::new(crate::tf!("Configured: {}", "Configurado: {}", mu.configured)).small().monospace());
        if !add.is_empty() || !rm.is_empty() {
            ui.label(
                RichText::new(crate::tf!(
                    "Will change: {}{}", "Vai mudar: {}{}",
                    add.iter().map(|a| format!("+{a} ")).collect::<String>(),
                    rm.iter().map(|a| format!("-{a} ")).collect::<String>()
                ))
                .color(ORANGE),
            );
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(mu.loader.is_some() && (!add.is_empty() || !rm.is_empty()), egui::Button::new(RichText::new(crate::i18n::tr("Apply at boot")).strong()))
                .on_hover_text(crate::i18n::tr("Asks for the password, backs up the bootloader file and rewrites the entries. Applies after a reboot."))
                .clicked()
            {
                let mut argv = vec!["pkexec".into(), crate::jobs::sibling_abs("numapilot"), "boot".into(), "cmdline".into(), "apply".into()];
                argv.extend(add.iter().map(|a| format!("+{a}")));
                argv.extend(rm.iter().map(|a| format!("-{a}")));
                job = Some((crate::i18n::tr("Kernel parameters").into(), argv));
                mu.boot_loaded = false;
            }
            if ui.button(crate::i18n::tr("Reload")).clicked() {
                mu.boot_loaded = false;
                mu.read_at = None;
            }
        });
        let od = numapilot::bootcfg::overdrive_state();
        ui.label(
            RichText::new(crate::tf!(
                "OverDrive of the AMD cards: {} (found in Settings and in the Graphics cards tab).", "OverDrive das placas AMD: {} (fica em Configurações e na aba Placas de vídeo).",
                if od.on_next_boot { crate::i18n::tr("on") } else { crate::i18n::tr("off") }
            ))
            .small()
            .weak(),
        );
    });

    if let Some(e) = app_toast {
        app.toast(e, false);
    }
    if let Some(m) = app_toast_ok2 {
        app.toast(m, true);
    }
    // the NVIDIA mode applies right away (without "Apply machine settings")
    if let Some(v) = dgpu_send {
        let mut s = app.mach.saved.clone();
        s.dgpu_mode = v;
        app.mach.saved.dgpu_mode = s.dgpu_mode.clone();
        send_machine = Some(s);
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
