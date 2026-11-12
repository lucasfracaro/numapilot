//! "Tests" tab: a script to test the program by hand (only the items that apply to this
//! machine), with OK / Failed / Skipped and a note on each, plus the command-line self-test
//! and a single report with everything (answers, self-test and the end of the logs).

use crate::App;
use crate::theme;
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq)]
enum When {
    Always,
    Laptop,
    Nvidia,
    NvidiaLaptop,
    Board,
    MultiSocket,
    RyzenLaptop,
    Acer,
    /// some card with a controllable fan
    FanControl,
    /// AMD card with OverDrive off / on
    AmdOdOff,
    AmdOdOn,
}

struct Item {
    id: &'static str,
    area: &'static str,
    title: &'static str,
    how: &'static str,
    expect: &'static str,
    when: When,
}

const fn it(
    id: &'static str,
    area: &'static str,
    title: &'static str,
    how: &'static str,
    expect: &'static str,
    when: When,
) -> Item {
    Item {
        id,
        area,
        title,
        how,
        expect,
        when,
    }
}

use When::*;

const ITEMS: &[Item] = &[
    // Geral
    it(
        "abrir",
        "General",
        "Open the program",
        "Open numapilot from the system menu.",
        "The window opens within a few seconds, in the system theme, with no error message.",
        Always,
    ),
    it(
        "janela-unica",
        "General",
        "Single window",
        "With the window open, open numapilot again from the menu.",
        "The window that was already open comes to the front; no second one opens.",
        Always,
    ),
    it(
        "bandeja",
        "General",
        "Tray icon",
        "Turn on the tray icon (Settings), close the window and click the icon.",
        "The window opens. There is only one numapilot icon in the tray.",
        Always,
    ),
    it(
        "tema",
        "General",
        "Theme",
        "Switch to Light and Dark in the side menu, close and open again.",
        "The theme changes right away and stays as chosen after reopening.",
        Always,
    ),
    it(
        "medidores",
        "General",
        "Top meters",
        "Open a game or benchmark and watch the CPU, memory and card tiles.",
        "The values go up, the bars slide without jumping and match another monitor (Mission Center, nvtop).",
        Always,
    ),
    it(
        "servico",
        "General",
        "Service stopped and restart",
        "In the terminal: sudo systemctl stop numapilot. Then use the \"Start the service\" button.",
        "\"Service stopped\" shows up with the button; the button asks for the password and it goes back to \"Service running\".",
        Always,
    ),
    it(
        "log",
        "General",
        "Log",
        "In the terminal: numapilot log",
        "Shows what was done (service, program and commands) with date and time.",
        Always,
    ),
    // Programas
    it(
        "lista",
        "Programs",
        "Program list",
        "Open any program (browser, game).",
        "It shows up in Programs within 5 seconds, with the socket and the card it is using.",
        Always,
    ),
    it(
        "tabela",
        "Programs",
        "Sort and search",
        "Click the Name, CPU, Memory and GPU headers (again to reverse); search by name and by PID.",
        "The order changes; CPU, memory and GPU of a game match Mission Center or MangoHud.",
        Always,
    ),
    it(
        "encerrar",
        "Programs",
        "End and Force stop",
        "Open one of your programs (e.g. Dolphin), select it and use End; open it again and use Force stop.",
        "End closes it; Force stop asks for confirmation and closes it. kwin, plasmashell and Xwayland stay under \"Session and system\" and the buttons refuse.",
        Always,
    ),
    it(
        "nucleos",
        "Programs",
        "Choose cores",
        "In Adjust…, pick specific cores and save.",
        "In the Map (per-thread chart) the yellow dot shows on those cores and the load stays on them.",
        Always,
    ),
    it(
        "limite-mem",
        "Programs",
        "Memory limit",
        "In Adjust…, check \"Limit the program's memory to\" 2 GB and use the program.",
        "The program does not go past the limit (the excess goes to swap); the PC does not freeze.",
        Always,
    ),
    it(
        "gpu-escolha",
        "Programs",
        "Choose the card",
        "In Adjust…, pick another card and use Reopen.",
        "The program reopens and starts using the chosen card (its usage shows there).",
        Always,
    ),
    it(
        "steam",
        "Programs",
        "Steam games",
        "Use \"Apply to all games\" with Steam closed; then \"Remove from all\".",
        "The games' launch options change and go back to what they were.",
        Always,
    ),
    // Mapa
    it(
        "threads",
        "Map",
        "Per-thread chart",
        "Open Map & measurements → Per-thread chart and run something heavy.",
        "The charts of the threads in use go up; hovering shows the core and the frequency.",
        Always,
    ),
    it(
        "temp-cpu",
        "Map",
        "Processor temperature",
        "Watch the temperature chart under load and then idle.",
        "It follows the real temperature (up under load, down when idle).",
        Always,
    ),
    it(
        "memoria-local",
        "Map",
        "Socket-only memory",
        "In a program's Adjust…, choose to use only its socket's memory.",
        "Under \"Allocations on the other socket\" this program does not increase remote allocations.",
        MultiSocket,
    ),
    it(
        "medir",
        "Map",
        "Measure latency and bandwidth",
        "Click \"Measure now\".",
        "The table fills in with no error.",
        MultiSocket,
    ),
    // Placas
    it(
        "ventoinha",
        "Cards",
        "Fan curve",
        "In Graphics cards, choose \"curve\", apply and then Undo.",
        "The fan follows the curve; Undo goes back to the card's automatic mode.",
        FanControl,
    ),
    it(
        "potencia",
        "Cards",
        "Power limit",
        "Lower a card's power limit and run a game. If the card does not show this control, the vendor locks it: mark Skipped.",
        "The watts at the top stay under the limit; Undo restores the default.",
        Always,
    ),
    it(
        "undervolt",
        "Cards",
        "Automatic undervolt",
        "On AC power, use \"Find the undervolt automatically\" and leave the PC idle.",
        "It picks a load program with 80% usage or more, shows clock and usage, finishes and saves; the terminal closes with Enter.",
        Always,
    ),
    it(
        "nv-offset",
        "Cards",
        "Clock offset (NVIDIA)",
        "Apply a small offset (e.g. +50 MHz) and restart the service.",
        "The value shows as applied and resets by itself after restarting the service.",
        Nvidia,
    ),
    it(
        "overdrive",
        "Cards",
        "OverDrive (AMD)",
        "Use \"Enable OverDrive\" and reboot the PC.",
        "After rebooting, the card's clock and voltage controls become available.",
        AmdOdOff,
    ),
    it(
        "clock-amd",
        "Cards",
        "Clock and voltage (AMD)",
        "On the AMD card, under \"Clock and voltage\", check customize, lower \"GPU max\" and apply; then Undo.",
        "Under load, the card's clock stays under the chosen value; Undo restores the factory value.",
        AmdOdOn,
    ),
    // Machine
    it(
        "modo",
        "Machine",
        "Machine modes",
        "Switch between Game, AI, Quiet and Normal.",
        "The governor and the energy preference change (numapilot selftest shows it); Normal restores what it was.",
        Always,
    ),
    it(
        "horario",
        "Machine",
        "Scheduled mode",
        "Create a schedule that starts 2 minutes from now.",
        "At that time the mode switches by itself; after the end, it goes back.",
        Always,
    ),
    it(
        "energia",
        "Machine",
        "Energy and cost",
        "Enter the kWh price and wait a few minutes.",
        "Today/month start counting and grow with usage.",
        Always,
    ),
    it(
        "saude",
        "Machine",
        "Hardware health",
        "Wait 1 minute after starting the service and open the Health section.",
        "Disks, memory and cards show a status (no \"never collected\").",
        Always,
    ),
    // Notebook
    it(
        "bateria",
        "Laptop",
        "AC power and battery",
        "Unplug the charger; then plug it back in.",
        "Within 10 s numapilot shows \"on battery\" and then \"on AC power\".",
        Laptop,
    ),
    it(
        "nv-auto",
        "Laptop",
        "NVIDIA on automatic",
        "Auto mode. On battery, close whatever uses the NVIDIA; then plug in the charger.",
        "On battery the NVIDIA turns off (numapilot hybrid: \"off\"); on AC power it comes back.",
        NvidiaLaptop,
    ),
    it(
        "nv-segurando",
        "Laptop",
        "Who is holding the NVIDIA",
        "Run: numapilot hybrid",
        "Lists only programs that really use the NVIDIA (no systemd, kwin, etc.).",
        NvidiaLaptop,
    ),
    it(
        "autostart",
        "Laptop",
        "Login programs on the integrated GPU",
        "After logging in, run: numapilot hybrid",
        "Nothing that starts with the system shows up holding the NVIDIA.",
        NvidiaLaptop,
    ),
    it(
        "telas",
        "Laptop",
        "External displays",
        "Connect monitors to HDMI and USB-C/Thunderbolt; move one to another port.",
        "All of them show an image; changing ports does not leave the displays flickering.",
        Laptop,
    ),
    it(
        "ryzenadj",
        "Laptop",
        "Ryzen limits",
        "Change the STAPM limit in Machine and run: sudo ryzenadj --info",
        "The new value shows in ryzenadj and is still there after a few minutes.",
        RyzenLaptop,
    ),
    it(
        "acer",
        "Laptop",
        "Acer controls",
        "Change the Acer profile/fan in Machine. If nothing shows up, run numapilot selftest: the Acer item shows why.",
        "The laptop responds (fan/profile change) and the value stays saved.",
        Acer,
    ),
    // Special board
    it(
        "bc-apps",
        "BC-250",
        "Community apps",
        "In Graphics cards, look at the BC-250 section.",
        "The community apps show up with install/open.",
        Board,
    ),
    it(
        "bc-mem",
        "BC-250",
        "GPU memory",
        "Change the GPU memory and reboot.",
        "After rebooting, the self-test shows the new value under \"GPU memory (TTM)\".",
        Board,
    ),
    it(
        "bc-cu",
        "BC-250",
        "Cores and CUs",
        "Run the self-test.",
        "Shows the active cores and CUs expected for the unlock.",
        Board,
    ),
    it(
        "bc-stream",
        "BC-250",
        "Streaming without a video encoder",
        "Turn on Sunshine from numapilot and connect with Moonlight.",
        "It connects using software encoding, with a stable image.",
        Board,
    ),
    // Outros
    it(
        "ia",
        "Other",
        "AI server",
        "Start a small model and send a question.",
        "It answers; \"Tune better\" finishes and suggests settings.",
        Always,
    ),
    it(
        "lab",
        "Other",
        "Lab",
        "Create a container and open Jupyter.",
        "The container starts and Jupyter opens in the browser.",
        Always,
    ),
    it(
        "streaming",
        "Other",
        "Streaming",
        "Turn on Sunshine and connect with Moonlight; open a game from the phone.",
        "It connects and the game opens.",
        Always,
    ),
    it(
        "peers",
        "Other",
        "Other PCs",
        "Add another PC's link and use \"wake over the network\" with it turned off.",
        "It shows as online; the wake command turns the PC on.",
        Always,
    ),
    it(
        "avisos",
        "Other",
        "Phone notifications",
        "Set up ntfy and send the test.",
        "The notification arrives on the phone.",
        Always,
    ),
    it(
        "backup",
        "Other",
        "Settings backup",
        "Use \"Save from all\" in Other PCs.",
        "The files show up in the backup folder.",
        Always,
    ),
];

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Res {
    pub st: String,
    #[serde(default)]
    pub nota: String,
    #[serde(default)]
    pub quando: String,
}

#[derive(Default)]
pub struct Checklist {
    loaded: bool,
    pub res: BTreeMap<String, Res>,
    flags: Option<Flags>,
    pub last_report: Option<PathBuf>,
}

#[derive(Clone, Copy)]
struct Flags {
    laptop: bool,
    nvidia: bool,
    amd: bool,
    board: bool,
    multi: bool,
    ryzen: bool,
    acer: bool,
    fan: bool,
    od_on: bool,
}

fn state_dir() -> PathBuf {
    numapilot::log::user_dir()
}

fn path() -> PathBuf {
    state_dir().join("testes.json")
}

pub fn autotest_path() -> PathBuf {
    state_dir().join("autoteste.txt")
}

impl Checklist {
    fn load(&mut self) {
        if !self.loaded {
            self.loaded = true;
            self.res = std::fs::read_to_string(path())
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
        }
    }
    fn save(&self) {
        let _ = std::fs::create_dir_all(state_dir());
        if let Ok(t) = serde_json::to_string_pretty(&self.res) {
            let _ = std::fs::write(path(), t);
        }
    }
}

fn flags(app: &App) -> Flags {
    let model = numapilot::recipes::cpu_model();
    let laptop = numapilot::notebook::is_laptop();
    Flags {
        laptop,
        nvidia: app.topo.gpus.iter().any(|g| g.is_nvidia()),
        amd: app.topo.gpus.iter().any(|g| g.vendor == 0x1002),
        board: !numapilot::recipes::load().boards(&app.topo).is_empty(),
        multi: app.topo.nodes.len() > 1,
        ryzen: laptop && model.contains("Ryzen"),
        acer: numapilot::acer::is_acer(),
        fan: app.topo.gpus.iter().any(|g| {
            let s = numapilot::gpuctl::read_sensors(&g.pci);
            s.fan_controllable || s.fw_fan_curve
        }),
        od_on: app
            .topo
            .gpus
            .iter()
            .any(|g| g.vendor == 0x1002 && numapilot::gpuctl::read_od(&g.pci).is_some()),
    }
}

fn applies(w: When, f: &Flags) -> bool {
    match w {
        Always => true,
        Laptop => f.laptop,
        Nvidia => f.nvidia,
        NvidiaLaptop => f.nvidia && f.laptop,
        Board => f.board,
        MultiSocket => f.multi,
        RyzenLaptop => f.ryzen,
        Acer => f.acer,
        FanControl => f.fan,
        AmdOdOff => f.amd && !f.od_on,
        AmdOdOn => f.od_on,
    }
}

fn st_label(st: &str) -> &'static str {
    match st {
        "ok" => "OK",
        "falhou" => crate::i18n::tr("FAILED"),
        "pulei" => crate::i18n::tr("SKIPPED"),
        _ => "—",
    }
}

/// Single report: answers, self-test and the end of the logs.
fn report(app: &App) -> Result<PathBuf, String> {
    let f = app.checklist.flags.unwrap_or_else(|| flags(app));
    let items: Vec<&Item> = ITEMS.iter().filter(|i| applies(i.when, &f)).collect();
    let r = &app.checklist.res;
    let count = |s: &str| {
        items
            .iter()
            .filter(|i| r.get(i.id).is_some_and(|x| x.st == s))
            .count()
    };
    let mut s = format!(
        "# numapilot — test report\n\nversion {} · machine {} · {}\n\
         Manual tests: {} items · {} OK · {} failed · {} skipped · {} unanswered\n",
        env!("CARGO_PKG_VERSION"),
        numapilot::selftest::hostname(),
        numapilot::log::now(),
        items.len(),
        count("ok"),
        count("falhou"),
        count("pulei"),
        items.len() - count("ok") - count("falhou") - count("pulei"),
    );
    let mut area = "";
    for i in &items {
        if i.area != area {
            area = i.area;
            s.push_str(&format!("\n## {area}\n"));
        }
        let x = r.get(i.id).cloned().unwrap_or_default();
        s.push_str(&format!("{:<7} {}", st_label(&x.st), i.title));
        if !x.quando.is_empty() {
            s.push_str(&format!(" ({})", x.quando));
        }
        s.push('\n');
        if !x.nota.trim().is_empty() {
            for l in x.nota.lines() {
                s.push_str(&format!("        nota: {l}\n"));
            }
        }
    }
    s.push_str("\n\n");
    match std::fs::read_to_string(autotest_path()) {
        Ok(t) => s.push_str(&t),
        Err(_) => s.push_str(
            "# self-test\n\n(not run: use \"Run the self-test\" before generating the report)\n",
        ),
    }
    s.push_str("\n\n# registros (final)\n");
    for (name, p) in numapilot::log::files() {
        let t = numapilot::log::tail(&p, 150);
        if !t.is_empty() {
            s.push_str(&format!("\n## {name} ({})\n", p.display()));
            for l in t {
                s.push_str(&l);
                s.push('\n');
            }
        }
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into()));
    let d = numapilot::log::now().replace([' ', ':'], "-");
    let out = home.join(format!(
        "numapilot-testes-{}-{}.txt",
        numapilot::selftest::hostname(),
        &d[..16.min(d.len())]
    ));
    std::fs::write(&out, s).map_err(|e| format!("{}: {e}", out.display()))?;
    numapilot::log::info(&crate::tf!(
        "test report generated: {}",
        "relatório de testes gerado: {}",
        out.display()
    ));
    Ok(out)
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    app.checklist.load();
    if app.checklist.flags.is_none() {
        app.checklist.flags = Some(flags(app));
    }
    let f = app.checklist.flags.unwrap();
    let p = theme::pal(ui);
    let items: Vec<&Item> = ITEMS.iter().filter(|i| applies(i.when, &f)).collect();
    let answered = items
        .iter()
        .filter(|i| app.checklist.res.contains_key(i.id))
        .count();
    let failed = items
        .iter()
        .filter(|i| {
            app.checklist
                .res
                .get(i.id)
                .is_some_and(|r| r.st == "falhou")
        })
        .count();

    ui.horizontal_wrapped(|ui| {
        if ui
            .button(crate::i18n::tr("Run the self-test"))
            .on_hover_text(crate::i18n::tr("Tests every part from the command line (read-only, changes nothing). Takes a minute or two."))
            .clicked()
        {
            let exe = crate::jobs::sibling_abs("numapilot");
            app.start_job(
                crate::i18n::tr("Self-test"),
                vec![
                    exe,
                    "selftest".into(),
                    "--out".into(),
                    autotest_path().display().to_string(),
                ],
            );
        }
        if ui
            .button(crate::i18n::tr("Generate report"))
            .on_hover_text(crate::i18n::tr("A file with your answers, the last self-test and the end of the logs, to send."))
            .clicked()
        {
            match report(app) {
                Ok(pth) => {
                    app.toast(crate::tf!("Report: {}", "Relatório: {}", pth.display()), true);
                    app.checklist.last_report = Some(pth);
                }
                Err(e) => app.toast(crate::tf!("Could not generate the report: {e}", "Não consegui gerar o relatório: {e}"), false),
            }
        }
        if ui.button(crate::i18n::tr("Clear answers")).clicked() {
            app.checklist.res.clear();
            app.checklist.save();
        }
        ui.label(
            RichText::new(crate::tf!(
                "{answered} of {} answered · {failed} failed", "{answered} de {} respondidos · {failed} falharam",
                items.len()
            ))
            .color(if failed > 0 { p.err } else { p.weak }),
        );
    });
    if let Some(pth) = &app.checklist.last_report {
        ui.label(
            RichText::new(crate::tf!(
                "Last report: {}",
                "Último relatório: {}",
                pth.display()
            ))
            .small()
            .weak(),
        );
    }
    ui.label(
        RichText::new(
            crate::i18n::tr("Only the tests that apply to this machine are shown. Answers are saved; mark and write a note when something goes wrong."),
        )
        .small()
        .weak(),
    );
    ui.add_space(6.0);

    let mut changed = false;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let mut area = "";
            for i in &items {
                if i.area != area {
                    area = i.area;
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(crate::i18n::tr(area))
                            .size(15.0)
                            .family(egui::FontFamily::Name(theme::BOLD.into())),
                    );
                }
                let entry = app.checklist.res.get(i.id).cloned();
                let st = entry.as_ref().map(|r| r.st.clone()).unwrap_or_default();
                let stroke = match st.as_str() {
                    "ok" => p.ok.gamma_multiply(0.6),
                    "falhou" => p.err.gamma_multiply(0.7),
                    "pulei" => p.weak.gamma_multiply(0.5),
                    _ => p.stroke,
                };
                egui::Frame::new()
                    .fill(p.card)
                    .stroke(egui::Stroke::new(1.0, stroke))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(12, 9))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width() - 12.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(crate::i18n::tr(i.title))
                                    .family(egui::FontFamily::Name(theme::BOLD.into())),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    for (v, l, c) in [
                                        ("pulei", crate::i18n::tr("Skipped"), p.weak),
                                        ("falhou", crate::i18n::tr("Failed"), p.err),
                                        ("ok", "OK", p.ok),
                                    ] {
                                        let sel = st == v;
                                        let txt = if sel {
                                            RichText::new(l).color(Color32::WHITE)
                                        } else {
                                            RichText::new(l).color(c)
                                        };
                                        let b = egui::Button::new(txt)
                                            .fill(if sel { c } else { Color32::TRANSPARENT })
                                            .stroke(egui::Stroke::new(1.0, c.gamma_multiply(0.6)));
                                        if ui.add(b).clicked() {
                                            if sel {
                                                app.checklist.res.remove(i.id);
                                            } else {
                                                let e = app
                                                    .checklist
                                                    .res
                                                    .entry(i.id.to_string())
                                                    .or_default();
                                                e.st = v.to_string();
                                                e.quando = numapilot::log::now();
                                                numapilot::log::info(&crate::tf!(
                                                    "test \"{}\": {}",
                                                    "teste \"{}\": {}",
                                                    i.title,
                                                    st_label(v)
                                                ));
                                            }
                                            changed = true;
                                        }
                                    }
                                },
                            );
                        });
                        ui.label(
                            RichText::new(crate::tf!(
                                "How: {}",
                                "Como: {}",
                                crate::i18n::tr(i.how)
                            ))
                            .size(12.5)
                            .color(p.weak),
                        );
                        ui.label(
                            RichText::new(crate::tf!(
                                "Expected: {}",
                                "Esperado: {}",
                                crate::i18n::tr(i.expect)
                            ))
                            .size(12.5),
                        );
                        if let Some(e) = app.checklist.res.get_mut(i.id) {
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut e.nota)
                                    .hint_text(if e.st == "falhou" {
                                        crate::i18n::tr("What happened? (goes into the report)")
                                    } else {
                                        crate::i18n::tr("note (optional)")
                                    })
                                    .desired_width(f32::INFINITY),
                            );
                            if r.lost_focus() || (r.changed() && e.nota.ends_with(' ')) {
                                changed = true;
                            }
                        }
                    });
                ui.add_space(4.0);
            }
        });
    if changed {
        app.checklist.save();
    }
}
