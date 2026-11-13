//! `numapilot doctor`: tests each part on the real hardware and says what to fix.

use crate::config::Config;
use crate::containers::which;
use crate::gpuctl;
use crate::history::{Status, status_path};
use crate::topo::{GpuRole, Topology};
use serde::Serialize;
use std::process::Command;

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Info,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct Check {
    pub area: String,
    pub name: String,
    pub level: Level,
    pub detail: String,
    pub fix: Option<String>,
}

fn out(argv: &[&str], env: &[(&str, String)]) -> Option<String> {
    let mut c = Command::new("timeout");
    c.arg("20").args(argv);
    for (k, v) in env {
        c.env(k, v);
    }
    let o = c.output().ok()?;
    Some(String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr))
}

pub fn run(cfg: &Config) -> Vec<Check> {
    let mut v = Vec::new();
    let mut add = |area: &str, name: &str, level: Level, detail: String, fix: Option<&str>| {
        v.push(Check {
            area: area.into(),
            name: name.into(),
            level,
            detail,
            fix: fix.map(String::from),
        })
    };
    let topo = Topology::load(cfg);

    // ---- NUMA / kernel ----
    let sockets: std::collections::BTreeSet<u32> = topo
        .nodes
        .iter()
        .flat_map(|n| n.sockets.iter().copied())
        .collect();
    if topo.nodes.len() >= 2 {
        add(
            "NUMA",
            crate::i18n::tr("nodes"),
            Level::Ok,
            crate::tf!(
                "{} nodes, {} sockets, {} CPUs",
                "{} nós, {} sockets, {} CPUs",
                topo.nodes.len(),
                sockets.len(),
                topo.all_cpus().len()
            ),
            None,
        );
    } else if sockets.len() > 1 {
        add(
            "NUMA",
            crate::i18n::tr("nodes"),
            Level::Fail,
            crate::i18n::tr("2 sockets but 1 NUMA node").into(),
            Some(crate::i18n::tr(
                "In the BIOS: turn off \"Node Interleaving\" (keep NUMA enabled)",
            )),
        );
    } else {
        add(
            "NUMA",
            crate::i18n::tr("nodes"),
            Level::Info,
            crate::i18n::tr("1 NUMA node").into(),
            None,
        );
    }
    for w in topo.warnings() {
        add("NUMA", "aviso", Level::Warn, w, None);
    }
    let bal = std::fs::read_to_string("/proc/sys/kernel/numa_balancing").unwrap_or_default();
    add(
        "NUMA",
        crate::i18n::tr("kernel automatic balancing"),
        Level::Info,
        format!("numa_balancing = {}", bal.trim()),
        None,
    );
    let ctrl = std::fs::read_to_string("/sys/fs/cgroup/cgroup.controllers").unwrap_or_default();
    if ctrl.contains("cpuset") && ctrl.contains("memory") {
        add(
            crate::i18n::tr("System"),
            crate::i18n::tr("cgroups v2"),
            Level::Ok,
            ctrl.trim().into(),
            None,
        );
    } else {
        add(
            crate::i18n::tr("System"),
            crate::i18n::tr("cgroups v2"),
            Level::Fail,
            crate::tf!("controllers: '{}'", "controladores: '{}'", ctrl.trim()),
            Some(crate::i18n::tr(
                "The system needs cgroups v2 (default on CachyOS) for memory limits and exclusive cores",
            )),
        );
    }
    let sd = out(&["systemctl", "--version"], &[]).unwrap_or_default();
    add(
        crate::i18n::tr("System"),
        "systemd",
        Level::Info,
        sd.lines().next().unwrap_or("?").into(),
        None,
    );
    let od = crate::bootcfg::overdrive_state();
    if od.active {
        add(
            crate::i18n::tr("System"),
            crate::i18n::tr("OverDrive (clock/voltage)"),
            Level::Ok,
            crate::i18n::tr("on").into(),
            None,
        );
    } else if od.on_next_boot {
        add(
            crate::i18n::tr("System"),
            crate::i18n::tr("OverDrive (clock/voltage)"),
            Level::Info,
            crate::i18n::tr("turns on after a reboot").into(),
            Some(crate::i18n::tr("Reboot the computer")),
        );
    } else {
        add(
            crate::i18n::tr("System"),
            crate::i18n::tr("OverDrive (clock/voltage)"),
            Level::Info,
            crate::i18n::tr("off").into(),
            Some(crate::i18n::tr(
                "For undervolt/overclock: Graphics cards → \"Enable OverDrive\" (or sudo numapilot boot overdrive on) and reboot",
            )),
        );
    }
    // ---- NVIDIA and laptop ----
    if topo.gpus.iter().any(|g| g.is_nvidia()) {
        let st = crate::driver::status(&topo);
        match (&st.loaded, &st.installed) {
            (Some(l), _) if st.needs_reboot => add(
                "NVIDIA",
                "driver",
                Level::Warn,
                crate::tf!(
                    "loaded {l}, installed {}",
                    "carregado {l}, instalado {}",
                    st.installed.clone().unwrap_or_default()
                ),
                Some(crate::i18n::tr("Reboot to use the installed driver")),
            ),
            (Some(l), _) => add(
                "NVIDIA",
                "driver",
                Level::Ok,
                format!(
                    "{l} ({})",
                    st.branch.as_ref().map(|b| b.label()).unwrap_or("")
                ),
                None,
            ),
            (None, _) if st.nouveau => add(
                "NVIDIA",
                "driver",
                Level::Warn,
                crate::i18n::tr("using nouveau (no performance for games)").into(),
                Some(crate::i18n::tr(
                    "Graphics cards → Install official driver (or sudo numapilot driver install)",
                )),
            ),
            (None, _) => add(
                "NVIDIA",
                "driver",
                Level::Fail,
                crate::i18n::tr("official driver not loaded").into(),
                Some(crate::i18n::tr(
                    "Graphics cards → Install official driver (or sudo numapilot driver install)",
                )),
            ),
        }
        if let (Some(i), Some(a)) = (&st.installed, &st.available)
            && i != a
        {
            add(
                "NVIDIA",
                crate::i18n::tr("update"),
                Level::Info,
                crate::tf!("{i} → {a} available", "{i} → {a} disponível"),
                Some(crate::i18n::tr(
                    "Graphics cards → Update (updates the system too)",
                )),
            );
        }
    }
    if crate::notebook::is_laptop() {
        let ac = crate::notebook::on_ac();
        add(
            "Notebook",
            crate::i18n::tr("power"),
            Level::Ok,
            match ac {
                Some(true) => crate::i18n::tr("on AC power").into(),
                Some(false) => crate::i18n::tr("on battery").into(),
                None => "?".into(),
            },
            None,
        );
        for g in topo.gpus.iter().filter(|g| !g.is_integrated()) {
            let pm = crate::notebook::runtime_pm(&g.pci).unwrap_or_default();
            if pm == "on" {
                add(
                    "Notebook",
                    &crate::tf!("{} sleep", "{} dormir", g.alias),
                    Level::Info,
                    crate::i18n::tr("the dedicated card never sleeps (drains the battery)").into(),
                    Some(crate::i18n::tr(
                        "Machine → Laptop → dedicated card: sleep when nobody uses it",
                    )),
                );
            } else {
                add(
                    "Notebook",
                    &crate::tf!("{} sleep", "{} dormir", g.alias),
                    Level::Ok,
                    crate::notebook::dgpu_state(&g.pci),
                    None,
                );
            }
            if !crate::notebook::dgpu_asleep(&g.pci) {
                let mut users: Vec<String> = crate::notebook::dgpu_users(&g.pci)
                    .into_iter()
                    .map(|u| u.1)
                    .collect();
                users.sort();
                users.dedup();
                if !users.is_empty() {
                    add(
                        "Notebook",
                        &crate::tf!("{} woken by", "{} acordada por", g.alias),
                        Level::Info,
                        users.join(", "),
                        Some(crate::i18n::tr(
                            "Open these programs on the integrated card so the dedicated one can sleep",
                        )),
                    );
                    for u in &users {
                        if let Some(h) = crate::notebook::holder_hint(u) {
                            add(
                                "Notebook",
                                u,
                                Level::Info,
                                crate::i18n::tr("keeps the NVIDIA awake").into(),
                                Some(h),
                            );
                        }
                    }
                    // KDE: KWin (and Xwayland) open every card; it can be limited to the integrated one
                    let kde = std::env::var("XDG_CURRENT_DESKTOP")
                        .unwrap_or_default()
                        .contains("KDE");
                    if users
                        .iter()
                        .any(|u| u == "Xwayland" || u.starts_with("kwin"))
                    {
                        let igpu = topo
                            .gpus
                            .iter()
                            .find(|x| x.is_integrated())
                            .and_then(|x| {
                                std::fs::read_dir(crate::util::sys(&format!(
                                    "bus/pci/devices/{}/drm",
                                    x.pci
                                )))
                                .ok()
                            })
                            .and_then(|rd| {
                                rd.flatten()
                                    .map(|e| e.file_name().to_string_lossy().to_string())
                                    .find(|n| n.starts_with("card"))
                            });
                        let fix = igpu.filter(|_| kde).map(|c| {
                            crate::tf!("echo 'KWIN_DRM_DEVICES=/dev/dri/{c}' | sudo tee -a /etc/environment and log in again (if the card number changes after an update, redo it)", "echo 'KWIN_DRM_DEVICES=/dev/dri/{c}' | sudo tee -a /etc/environment e entre de novo (se o número da placa mudar após atualizar, refaça)")
                        });
                        add(
                            "Notebook",
                            crate::i18n::tr("desktop holding the NVIDIA"),
                            Level::Info,
                            crate::i18n::tr(
                                "the compositor (KWin/Xwayland) opened the dedicated card",
                            )
                            .into(),
                            fix.as_deref(),
                        );
                    }
                }
            }
        }
        // acer_wmi with predator_v4=1: profiles (including Turbo) and fan readings
        if crate::acer::is_acer() {
            let pv4 =
                crate::util::read_trim(crate::util::sys("module/acer_wmi/parameters/predator_v4"));
            let pv4_conf = std::fs::read_dir("/etc/modprobe.d").ok().is_some_and(|rd| {
                rd.flatten().any(|e| {
                    std::fs::read_to_string(e.path())
                        .unwrap_or_default()
                        .lines()
                        .any(|l| {
                            !l.trim_start().starts_with('#')
                                && l.contains("acer_wmi")
                                && l.contains("predator_v4=1")
                        })
                })
            });
            if matches!(pv4.as_deref(), Some("Y") | Some("1")) || pv4_conf {
                add(
                    "Notebook",
                    "acer_wmi predator_v4",
                    Level::Ok,
                    crate::i18n::tr(
                        "on: power profiles and fans through the kernel (numapilot uses it)",
                    )
                    .into(),
                    None,
                );
            }
        }
        // Programs that compete with numapilot for fan/power control
        let mut rivals: Vec<String> = vec![];
        if let Ok(rd) = std::fs::read_dir("/proc") {
            for e in rd.flatten() {
                if let Some(c) = crate::util::read_trim(e.path().join("comm")) {
                    let l = c.to_lowercase();
                    if [
                        "nitrosense",
                        "nitro-sense",
                        "predatorsense",
                        "nbfc",
                        "auto-cpufreq",
                        "tlp",
                        "slimbook",
                        "tuned",
                    ]
                    .iter()
                    .any(|r| l.starts_with(r))
                        && !rivals.contains(&c)
                    {
                        rivals.push(c);
                    }
                }
                if let Ok(cmd) = std::fs::read_to_string(e.path().join("cmdline")) {
                    let cmd = cmd.replace('\0', " ").to_lowercase();
                    if cmd.contains("nitrosense") && !rivals.iter().any(|r| r == "nitrosense") {
                        rivals.push("nitrosense".into());
                    }
                }
            }
        }
        if !rivals.is_empty() {
            add(
                "Notebook",
                crate::i18n::tr("other controllers running"),
                Level::Warn,
                rivals.join(", "),
                Some(crate::i18n::tr(
                    "they touch the same fans/profiles: close them and remove them from startup",
                )),
            );
        }
        if crate::util::read_trim(crate::util::sys("module/ec_sys/parameters/write_support"))
            .as_deref()
            == Some("Y")
        {
            add(
                "Notebook",
                crate::i18n::tr("controller write access (ec_sys)"),
                Level::Info,
                crate::i18n::tr(
                    "on (used by programs like NitroSense that write straight to the EC)",
                )
                .into(),
                Some(crate::i18n::tr(
                    "if you no longer use those programs, remove ec_sys write_support=1 from /etc/modprobe.d",
                )),
            );
        }
        if let Some((ig, nv)) = crate::hybrid::pair(&topo) {
            let scr = |g: &crate::topo::Gpu| {
                let v: Vec<String> = crate::hybrid::outputs(&g.pci)
                    .into_iter()
                    .filter(|o| o.connected)
                    .map(|o| o.name)
                    .collect();
                if v.is_empty() {
                    crate::i18n::tr("none").to_string()
                } else {
                    v.join(", ")
                }
            };
            add(
                "Notebook",
                crate::i18n::tr("displays"),
                Level::Ok,
                format!("{}: {} · {}: {}", ig.alias, scr(ig), nv.alias, scr(nv)),
                None,
            );
            let m = crate::machine::Machine::load().dgpu_mode;
            add(
                "Notebook",
                crate::i18n::tr("NVIDIA mode"),
                Level::Info,
                m.as_deref()
                    .and_then(|m| crate::hybrid::MODES.iter().find(|x| x.0 == m))
                    .map(|x| crate::i18n::tr(x.1).to_string())
                    .unwrap_or_else(|| crate::i18n::tr("leave as is (stays in the system; sleeps when nobody uses it)").into()),
                m.is_none().then_some(crate::i18n::tr("Machine → Laptop → NVIDIA: Automatic turns it off on battery and brings it back on AC power, without leaving the session")),
            );
            if !crate::hybrid::xwayland_wrapped() {
                add(
                    "Notebook",
                    "Xwayland",
                    Level::Info,
                    crate::i18n::tr("may open the NVIDIA for no reason (X11 programs)").into(),
                    Some(crate::i18n::tr(
                        "sudo numapilot hybrid xwayland on (Xwayland uses only Mesa)",
                    )),
                );
            }
        }
        if let Some(st) = crate::hybrid::load_state()
            && st.removed
        {
            add(
                "Notebook",
                "NVIDIA",
                Level::Ok,
                crate::tf!(
                    "{} turned off by numapilot (integrated only)",
                    "{} desligada pelo numapilot (só a integrada)",
                    st.alias
                ),
                None,
            );
        }
        if crate::notebook::ppd_active() {
            add(
                "Notebook",
                "power-profiles-daemon",
                Level::Ok,
                crate::i18n::tr("on: numapilot's modes switch the profile through it (the desktop selector follows)").into(),
                None,
            );
        }
        if crate::ryzen::applicable() {
            if crate::ryzen::available() {
                match crate::ryzen::info() {
                    Ok(i) => {
                        let l = crate::ryzen::limits_from(&i);
                        add(
                            "Notebook",
                            crate::i18n::tr("Ryzen limits"),
                            Level::Ok,
                            format!(
                                "STAPM {} W · PPT {} / {} W · {} °C",
                                l.stapm_w.map(|v| format!("{v:.0}")).unwrap_or("?".into()),
                                l.slow_w.map(|v| format!("{v:.0}")).unwrap_or("?".into()),
                                l.fast_w.map(|v| format!("{v:.0}")).unwrap_or("?".into()),
                                l.tctl_c.map(|v| format!("{v:.0}")).unwrap_or("?".into())
                            ),
                            None,
                        )
                    }
                    Err(e) => add(
                        "Notebook",
                        crate::i18n::tr("Ryzen limits"),
                        Level::Info,
                        e,
                        Some(crate::i18n::tr(
                            "run with sudo to read (ryzenadj needs root)",
                        )),
                    ),
                }
            } else {
                add(
                    "Notebook",
                    crate::i18n::tr("Ryzen limits"),
                    Level::Info,
                    crate::i18n::tr("ryzenadj not installed").into(),
                    Some("paru -S ryzenadj"),
                );
            }
        }
        if crate::acer::is_acer() {
            if crate::acer::sense_dir().is_some() || crate::acer::kb_dir().is_some() {
                add(
                    "Notebook",
                    "Acer (Linuwu-Sense)",
                    Level::Ok,
                    crate::tf!(
                        "{} settings available",
                        "{} ajustes disponíveis",
                        crate::acer::knobs().len()
                    ),
                    None,
                );
            } else {
                add(
                    "Notebook",
                    "Acer (Linuwu-Sense)",
                    Level::Info,
                    crate::i18n::tr("module not loaded: no manual fan, 80% limit or RGB").into(),
                    Some("https://github.com/0x7375646F/Linuwu-Sense"),
                );
            }
        }
        if !crate::notebook::battery_info()
            .iter()
            .any(|b| b.charge_limit.is_some())
            && crate::acer::read("battery_limiter").is_none()
        {
            add(
                "Notebook",
                crate::i18n::tr("charge limit"),
                Level::Info,
                crate::i18n::tr("the laptop does not allow it from Linux").into(),
                None,
            );
        }
    }
    // ---- special boards and community recipes ----
    let rec = crate::recipes::load();
    for (b, g) in rec.boards(&topo) {
        let area = b.name.clone();
        let k = crate::recipes::running_kernel();
        if crate::recipes::kernel_in(&k, &b.bad_kernels) {
            add(
                &area,
                "kernel",
                Level::Fail,
                crate::tf!(
                    "{k} is on the list of kernels that break this board",
                    "{k} está na lista de kernels que quebram esta placa"
                ),
                Some(crate::i18n::tr(
                    "Install another version (e.g. linux-cachyos-lts) and pick it at boot",
                )),
            );
        } else if !b.bad_kernels.is_empty() {
            add(&area, "kernel", Level::Ok, k, None);
        }
        if let Some(min) = &b.min_mesa {
            match crate::recipes::mesa_version() {
                Some(m) if crate::recipes::version_ge(&m, min) => {
                    add(&area, "Mesa", Level::Ok, m, None)
                }
                Some(m) => add(
                    &area,
                    "Mesa",
                    Level::Fail,
                    crate::tf!(
                        "{m} (needs {min} or newer)",
                        "{m} (precisa de {min} ou mais novo)"
                    ),
                    Some("sudo pacman -Syu"),
                ),
                None => {}
            }
        }
        if b.id == "bc250"
            && let Some(why) = crate::boards::smu_table_mismatch(&g.pci)
        {
            add(
                &area,
                crate::i18n::tr("GPU sensors"),
                Level::Warn,
                why,
                None,
            );
        }
        if !b.governor_services.is_empty() {
            let sv = crate::boards::services(b);
            match sv.iter().find(|s| s.active) {
                Some(s) => add(
                    &area,
                    crate::i18n::tr("clock governor"),
                    Level::Ok,
                    crate::tf!("{} running", "{} rodando", s.name),
                    None,
                ),
                None => add(
                    &area,
                    crate::i18n::tr("clock governor"),
                    Level::Warn,
                    crate::i18n::tr("none running: the GPU stays at a fixed clock").into(),
                    Some(crate::i18n::tr(
                        "Graphics cards → install/enable the governor",
                    )),
                ),
            }
        }
        let cmd = crate::cmdline::running();
        for p in &b.kernel_params {
            if !cmd.split_whitespace().any(|t| t == p) {
                add(
                    &area,
                    &crate::tf!("parameter {p}", "parâmetro {p}"),
                    Level::Info,
                    crate::i18n::tr("recommended by the community").into(),
                    Some(crate::i18n::tr("Machine → Kernel parameters")),
                );
            }
        }
        let _ = g;
        if let Some((stock, full)) = b.cores {
            let n = crate::boards::active_cores();
            if n >= full {
                let efi = crate::boards::efi_core_unlock();
                add(
                    &area,
                    crate::i18n::tr("CPU cores"),
                    Level::Ok,
                    crate::tf!("{n} of {full} (unlocked)", "{n} de {full} (desbloqueados)"),
                    None,
                );
                if let Some(p) = crate::boards::core_unlock_persistence(n, full, efi) {
                    add(
                        &area,
                        crate::i18n::tr("core unlock"),
                        Level::Info,
                        p.into(),
                        None,
                    );
                }
            } else if n > 0 {
                add(
                    &area,
                    crate::i18n::tr("CPU cores"),
                    Level::Info,
                    crate::tf!(
                        "{n} of {full} active (factory: {stock})",
                        "{n} de {full} ativos (de fábrica: {stock})"
                    ),
                    Some(crate::i18n::tr(
                        "Can be unlocked with the community programs (Graphics cards → community tweaks)",
                    )),
                );
            }
        }
        if let Some((_, full)) = b.cus
            && crate::boards::cu_unlock_by_kernel()
        {
            add(
                &area,
                crate::i18n::tr("CUs by the kernel"),
                Level::Info,
                crate::tf!(
                    "the kernel unlocks the {full} CUs at boot (amdgpu.bc250_cc_write_mode=3)",
                    "o kernel libera os {full} CUs no boot (amdgpu.bc250_cc_write_mode=3)"
                ),
                Some(crate::i18n::tr(
                    "Do not use the live unlock (umr, bc250-cu-live-manager) together with it: the driver already counts them all",
                )),
            );
        }
        if let Some((stock, full)) = b.cus {
            match crate::boards::cu_now() {
                Some((n, src)) if n >= full => add(
                    &area,
                    crate::i18n::tr("GPU CUs"),
                    Level::Ok,
                    crate::tf!(
                        "{n} of {full} (unlocked; {})",
                        "{n} de {full} (desbloqueados; {})",
                        src.text()
                    ),
                    None,
                ),
                Some((n, src)) => add(
                    &area,
                    crate::i18n::tr("GPU CUs"),
                    Level::Info,
                    crate::tf!(
                        "{n} of {full} ({}; factory: {stock})",
                        "{n} de {full} ({}; de fábrica: {stock})",
                        src.text()
                    ),
                    Some(crate::i18n::tr(
                        "Live unlock through BC250 Control Center (Graphics cards → community tweaks); the live reading shows up here after opening its CU page",
                    )),
                ),
                None => {}
            }
        }
        for a in &b.apps {
            let ok = a.installed();
            let what = if ok {
                crate::i18n::tr("installed").to_string()
            } else if a.recommended {
                crate::tf!(
                    "not installed (recommended): {}",
                    "não instalado (recomendado): {}",
                    crate::i18n::tr_text(&a.about)
                )
            } else {
                crate::tf!(
                    "not installed: {}",
                    "não instalado: {}",
                    crate::i18n::tr_text(&a.about)
                )
            };
            let hint = if ok {
                None
            } else if a.installable() {
                Some(crate::i18n::tr("Graphics cards → community tweaks → Install").to_string())
            } else {
                a.url
                    .as_ref()
                    .map(|u| crate::tf!("manual install: {u}", "instalação manual: {u}"))
            };
            add(
                &area,
                &a.name,
                if ok { Level::Ok } else { Level::Info },
                what,
                hint.as_deref(),
            );
        }
        for n in &b.notes {
            add(
                &area,
                crate::i18n::tr("tip"),
                Level::Info,
                crate::i18n::tr_text(n).to_string(),
                None,
            );
        }
    }
    for n in rec.cpu_notes(&crate::recipes::cpu_model()) {
        add(
            crate::i18n::tr("Processor"),
            crate::i18n::tr("tip"),
            Level::Info,
            crate::i18n::tr_text(&n).to_string(),
            None,
        );
    }
    // ---- interrupts on the right socket ----
    if topo.nodes.len() > 1 {
        let devs = crate::irqs::devices();
        if !devs.is_empty() {
            let desc: Vec<String> = devs
                .iter()
                .map(|d| {
                    crate::tf!(
                        "{} {} on socket {}",
                        "{} {} no socket {}",
                        d.kind,
                        d.pci,
                        d.node
                    )
                })
                .collect();
            add(
                crate::i18n::tr("Interrupts"),
                crate::i18n::tr("disks and network"),
                Level::Ok,
                desc.join(", "),
                None,
            );
        }
        if crate::irqs::irqbalance_active() {
            add(
                crate::i18n::tr("Interrupts"),
                "irqbalance",
                Level::Info,
                crate::i18n::tr(
                    "on: it spreads the interrupts and the service reapplies every 5 min",
                )
                .into(),
                Some(crate::i18n::tr(
                    "sudo systemctl disable --now irqbalance (optional)",
                )),
            );
        }
    }
    // ---- hardware health ----
    let rep = if crate::util::is_root() {
        Some(crate::health::collect(&topo))
    } else {
        crate::health::load()
    };
    match rep {
        Some(r) => {
            if !r.smartctl {
                add(
                    crate::i18n::tr("Health"),
                    crate::i18n::tr("disks (SMART)"),
                    Level::Info,
                    crate::i18n::tr("smartctl not installed").into(),
                    Some("sudo pacman -S smartmontools"),
                );
            }
            let probs = crate::health::problems(&r);
            if probs.is_empty() {
                add(
                    crate::i18n::tr("Health"),
                    crate::i18n::tr("overall"),
                    Level::Ok,
                    crate::tf!(
                        "{} disk(s), {} ECC memory controller(s), no problems",
                        "{} disco(s), {} controlador(es) de memória ECC, nenhum problema",
                        r.disks.len(),
                        r.mem.len()
                    ),
                    None,
                );
            }
            for (lvl, p) in probs {
                add(
                    crate::i18n::tr("Health"),
                    crate::i18n::tr("alert"),
                    if lvl >= 2 { Level::Fail } else { Level::Warn },
                    p,
                    None,
                );
            }
        }
        None => add(
            crate::i18n::tr("Health"),
            crate::i18n::tr("overall"),
            Level::Info,
            crate::i18n::tr("not collected yet (the service collects 1 min after starting)").into(),
            Some("sudo numapilot doctor"),
        ),
    }
    // ---- processador ----
    let unc = crate::cpuctl::uncores();
    let intel_cpu = crate::recipes::cpu_model().contains("Intel");
    if unc.is_empty() && intel_cpu {
        add(
            crate::i18n::tr("Processor"),
            "uncore",
            Level::Info,
            crate::i18n::tr("control unavailable").into(),
            Some(crate::i18n::tr(
                "sudo modprobe intel_uncore_frequency (and echo intel_uncore_frequency | sudo tee /etc/modules-load.d/uncore.conf)",
            )),
        );
    } else if !unc.is_empty() {
        add(
            crate::i18n::tr("Processor"),
            "uncore",
            Level::Ok,
            crate::tf!(
                "{} controllable socket(s)",
                "{} socket(s) controláveis",
                unc.len()
            ),
            None,
        );
    }
    let rapl = crate::cpuctl::rapl_zones();
    let no_limits = !rapl.is_empty() && rapl.iter().all(|r| r.limit_w.is_none());
    add(
        crate::i18n::tr("Processor"),
        crate::i18n::tr("power (RAPL)"),
        if rapl.is_empty() {
            Level::Info
        } else {
            Level::Ok
        },
        if rapl.is_empty() {
            crate::i18n::tr("unavailable").into()
        } else if no_limits {
            if intel_cpu {
                crate::i18n::tr("measurement only (no limits exposed)").into()
            } else if crate::ryzen::applicable() {
                crate::i18n::tr("measurement only on Ryzen; the limits go through ryzenadj (Machine → Ryzen limits)")
                    .into()
            } else {
                crate::i18n::tr("measurement only on Ryzen (limits in the BIOS)").into()
            }
        } else {
            rapl.iter()
                .map(|r| {
                    format!(
                        "socket {}: {}",
                        r.package,
                        r.limit_w.map(|w| format!("{w:.0} W")).unwrap_or("?".into())
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        },
        None,
    );
    let cs = crate::cpuctl::cstates(0);
    add(
        crate::i18n::tr("Processor"),
        crate::i18n::tr("idle states"),
        if cs.is_empty() {
            Level::Info
        } else {
            Level::Ok
        },
        if cs.is_empty() {
            crate::i18n::tr("cpuidle unavailable").into()
        } else {
            cs.iter()
                .map(|s| format!("{} {}µs", s.name, s.latency_us))
                .collect::<Vec<_>>()
                .join(", ")
        },
        None,
    );
    match crate::machine::scx_status() {
        Some((cur, _)) => add(
            crate::i18n::tr("Processor"),
            crate::i18n::tr("sched_ext scheduler"),
            Level::Ok,
            if cur.is_empty() {
                crate::i18n::tr("kernel default").into()
            } else {
                cur
            },
            None,
        ),
        None => add(
            crate::i18n::tr("Processor"),
            crate::i18n::tr("sched_ext scheduler"),
            Level::Info,
            crate::i18n::tr("scx_loader not installed").into(),
            Some("sudo pacman -S scx-scheds scx-tools"),
        ),
    }
    let vulns = crate::cmdline::vulnerabilities();
    let mitig: Vec<&str> = crate::cmdline::relevant_mitigations(&vulns)
        .iter()
        .map(|m| crate::i18n::tr(m.title))
        .collect();
    if !mitig.is_empty() {
        add(
            crate::i18n::tr("Processor"),
            crate::i18n::tr("active mitigations"),
            Level::Info,
            mitig.join(", "),
            Some(crate::i18n::tr("Machine → Processor mitigations")),
        );
    }
    match crate::cmdline::detect() {
        Some(l) => add(
            crate::i18n::tr("System"),
            "bootloader",
            Level::Ok,
            format!("{} ({})", l.name(), l.path.display()),
            None,
        ),
        None => add(
            crate::i18n::tr("System"),
            "bootloader",
            Level::Info,
            crate::i18n::tr("not recognized: kernel parameters only by hand").into(),
            None,
        ),
    }
    let bf = crate::fans::list();
    let ipmi = crate::fans::ipmi_fans();
    add(
        crate::i18n::tr("System"),
        crate::i18n::tr("motherboard fans"),
        if bf.is_empty() && ipmi.is_empty() {
            Level::Info
        } else {
            Level::Ok
        },
        crate::tf!(
            "{} controllable (hwmon), {} through IPMI",
            "{} controláveis (hwmon), {} pelo IPMI",
            bf.len(),
            ipmi.len()
        ),
        if bf.is_empty() {
            Some(crate::i18n::tr(
                "sudo sensors-detect (lm_sensors package) tells which driver to load, e.g. sudo modprobe nct6775",
            ))
        } else {
            None
        },
    );
    if !out(&["pgrep", "-x", "irqbalance"], &[])
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        add(
            crate::i18n::tr("System"),
            "irqbalance",
            Level::Warn,
            crate::i18n::tr("running: may undo the GPU interrupt tuning").into(),
            Some("sudo systemctl disable --now irqbalance"),
        );
    }

    // ---- service ----
    let fresh = std::fs::metadata(crate::daemon::state_path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
        .is_some_and(|d| d.as_secs() < 5);
    if fresh {
        add(
            crate::i18n::tr("Service"),
            "numapilot daemon",
            Level::Ok,
            crate::i18n::tr("running").into(),
            None,
        );
    } else {
        add(
            crate::i18n::tr("Service"),
            "numapilot daemon",
            Level::Fail,
            crate::i18n::tr("stopped").into(),
            Some("sudo systemctl enable --now numapilot"),
        );
    }
    match crate::ctl::send(&crate::ctl::Request::Ping) {
        Ok(r) if r.ok => add(
            crate::i18n::tr("Service"),
            crate::i18n::tr("control socket"),
            Level::Ok,
            crate::i18n::tr("answers").into(),
            None,
        ),
        _ => add(
            crate::i18n::tr("Service"),
            crate::i18n::tr("control socket"),
            Level::Fail,
            crate::i18n::tr("no response").into(),
            Some(crate::i18n::tr("See: journalctl -u numapilot -n 50")),
        ),
    }
    if let Some(st) = std::fs::read_to_string(status_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Status>(&s).ok())
    {
        add(
            crate::i18n::tr("Service"),
            crate::i18n::tr("as root"),
            if st.root { Level::Ok } else { Level::Fail },
            format!("{}", st.root),
            None,
        );
        add(
            crate::i18n::tr("Service"),
            crate::i18n::tr("instant detection"),
            if st.proc_connector {
                Level::Ok
            } else {
                Level::Warn
            },
            if st.proc_connector {
                crate::i18n::tr("proc connector active").into()
            } else {
                crate::i18n::tr("scanning only").into()
            },
            None,
        );
        match &st.cgroup_last_error {
            Some(e) => add(
                crate::i18n::tr("Service"),
                crate::i18n::tr("control groups"),
                Level::Fail,
                e.clone(),
                Some(crate::i18n::tr("Send me this report")),
            ),
            None => add(
                crate::i18n::tr("Service"),
                crate::i18n::tr("control groups"),
                Level::Ok,
                crate::tf!(
                    "{} applied with no error",
                    "{} aplicados sem erro",
                    st.cgroup_ok_count
                ),
                None,
            ),
        }
        for e in &st.od_errors {
            add(
                crate::i18n::tr("Service"),
                "clock/voltagem",
                Level::Fail,
                e.clone(),
                None,
            );
        }
    }

    // ---- GPUs ----
    let have_vk = which("vulkaninfo");
    let have_gl = which("glxinfo");
    for g in &topo.gpus {
        let a = format!("GPU {}", g.alias);
        add(
            &a,
            crate::i18n::tr("identification"),
            Level::Info,
            format!(
                "{} · {} · {:?} · gfx {}",
                g.pci,
                g.name,
                g.role,
                g.gfx.as_deref().unwrap_or("-")
            ),
            None,
        );
        match g.numa_node {
            Some(n) => add(&a, "socket", Level::Ok, format!("socket {n}"), None),
            None if topo.nodes.len() > 1 => add(
                &a,
                "socket",
                Level::Fail,
                crate::i18n::tr("unknown").into(),
                Some(crate::i18n::tr(
                    "Set it in [gpu_node] in /etc/numapilot.toml",
                )),
            ),
            None => {}
        }
        add(
            &a,
            "monitor",
            Level::Info,
            if g.display_connected {
                crate::i18n::tr("connected to this card").into()
            } else {
                crate::t2!("none", "nenhum").into()
            },
            None,
        );
        if g.role == GpuRole::Compute {
            match &g.rocr_id {
                Some(id) => add(
                    &a,
                    "ROCm",
                    Level::Ok,
                    format!("ROCR_VISIBLE_DEVICES={id}"),
                    None,
                ),
                None => add(
                    &a,
                    "ROCm",
                    Level::Fail,
                    crate::i18n::tr("does not show up in KFD").into(),
                    Some(crate::i18n::tr(
                        "Check whether the amdgpu module loaded with KFD (dmesg | grep kfd)",
                    )),
                ),
            }
        }
        let s = gpuctl::read_sensors(&g.pci);
        // integrated and laptop cards: fan and power belong to the laptop itself
        let laptop = crate::notebook::is_laptop();
        let soft = if g.is_integrated() || laptop {
            Level::Info
        } else {
            Level::Warn
        };
        // RDNA3/RDNA4: pwm1 is read-only; control is the firmware curve in
        // gpu_od/fan_ctrl, which only exists with OverDrive on
        let fan_ok = s.fan_controllable || s.fw_fan_curve;
        let od_off_fan = !fan_ok
            && g.vendor == 0x1002
            && s.fan_rpm.is_some()
            && !crate::bootcfg::overdrive_state().active;
        add(
            &a,
            crate::i18n::tr("fan"),
            if fan_ok { Level::Ok } else { soft },
            if s.fw_fan_curve {
                crate::tf!(
                    "controllable through the firmware curve ({} RPM)",
                    "controlável pela curva do firmware ({} RPM)",
                    s.fan_rpm.unwrap_or(0)
                )
            } else if s.fan_controllable {
                crate::tf!(
                    "controllable ({} RPM)",
                    "controlável ({} RPM)",
                    s.fan_rpm.unwrap_or(0)
                )
            } else if laptop || g.is_integrated() {
                crate::i18n::tr("controlled by the laptop (power profile)").into()
            } else if od_off_fan {
                crate::i18n::tr("no control: OverDrive is off").into()
            } else {
                crate::i18n::tr("no control from the driver").into()
            },
            od_off_fan.then_some(crate::i18n::tr(
                "On the new AMD cards (RX 7000/9000) the fan is controlled by the firmware \
                 curve, which only shows up with OverDrive on: Graphics cards → OverDrive \
                 (needs a reboot)",
            )),
        );
        add(
            &a,
            crate::i18n::tr("power limit"),
            if s.power_cap_max_w.is_some() || s.power_cap_w.is_some() {
                Level::Ok
            } else {
                soft
            },
            s.power_cap_w
                .map(|w| format!("{w:.0} W"))
                .unwrap_or_else(|| crate::i18n::tr("unavailable").into()),
            None,
        );
        match gpuctl::read_od(&g.pci) {
            _ if g.is_nvidia() => {}
            Some(od) => add(
                &a,
                crate::i18n::tr("clock/voltage (OD)"),
                Level::Ok,
                {
                    let mut d = od
                        .range_sclk
                        .map(|r| format!("GPU {}-{} MHz", r.0, r.1))
                        .unwrap_or_else(|| crate::i18n::tr("on").into());
                    if !od.curve.is_empty() {
                        d +=
                            &crate::tf!(", {}-point curve", ", curva de {} pontos", od.curve.len());
                    }
                    if let Some((lo, _)) = od.range_vo {
                        d += &crate::tf!(", voltage down to {lo} mV", ", voltagem até {lo} mV");
                    }
                    d
                },
                None,
            ),
            None => add(
                &a,
                crate::i18n::tr("clock/voltage (OD)"),
                Level::Info,
                crate::i18n::tr("unavailable").into(),
                Some(crate::i18n::tr(
                    "sudo numapilot boot overdrive on (or the button in the Graphics cards tab) and reboot",
                )),
            ),
        }
        if let Some(l) = gpuctl::pcie_link(&g.pci) {
            let d = crate::tf!(
                "Gen{} x{} (max Gen{} x{})",
                "Gen{} x{} (máx. Gen{} x{})",
                l.generation,
                l.width,
                l.max_gen,
                l.max_width
            );
            if l.width < l.max_width {
                add(
                    &a,
                    crate::i18n::tr("PCIe link"),
                    Level::Warn,
                    d,
                    Some(crate::i18n::tr(
                        "Link narrower than possible: check the slot (some are x8 or share lanes) and the BIOS",
                    )),
                );
            } else {
                add(
                    &a,
                    crate::i18n::tr("PCIe link"),
                    Level::Ok,
                    crate::tf!(
                        "{d}; the generation drops when the card is idle",
                        "{d}; a geração cai com a placa parada"
                    ),
                    None,
                );
            }
        }
        match gpuctl::bar_info(&g.pci) {
            Some((vis, tot)) if vis * 10 >= tot * 9 => add(
                &a,
                "Resizable BAR",
                Level::Ok,
                crate::tf!(
                    "active ({} MB visible)",
                    "ativo ({} MB visíveis)",
                    vis >> 20
                ),
                None,
            ),
            Some((vis, tot)) => add(
                &a,
                "Resizable BAR",
                Level::Info,
                crate::tf!(
                    "inactive ({} of {} MB visible)",
                    "inativo ({} de {} MB visíveis)",
                    vis >> 20,
                    tot >> 20
                ),
                Some(crate::i18n::tr(
                    "In the BIOS: \"Above 4G Decoding\" and \"Resizable BAR\" (if present). On old X99/C612 boards, try the pci=realloc parameter",
                )),
            ),
            None => {}
        }
        if have_vk && g.render.is_some() {
            let env = [
                ("DRI_PRIME", g.dri_prime()),
                ("MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE", "1".into()),
            ];
            let o = out(&["vulkaninfo", "--summary"], &env).unwrap_or_default();
            let names: Vec<&str> = o
                .lines()
                .filter(|l| l.trim_start().starts_with("deviceName"))
                .collect();
            if names.len() == 1 {
                add(
                    &a,
                    "Vulkan (DXVK/VKD3D)",
                    Level::Ok,
                    crate::tf!(
                        "only this GPU visible: {}",
                        "só esta GPU visível: {}",
                        names[0].split('=').nth(1).unwrap_or("").trim()
                    ),
                    None,
                );
            } else {
                add(
                    &a,
                    "Vulkan (DXVK/VKD3D)",
                    Level::Fail,
                    crate::tf!(
                        "{} devices visible",
                        "{} dispositivos visíveis",
                        names.len()
                    ),
                    Some(crate::i18n::tr(
                        "Mesa's GPU selection did not work: send me the report",
                    )),
                );
            }
        }
        if have_gl && g.render.is_some() {
            let o = out(&["glxinfo", "-B"], &[("DRI_PRIME", g.dri_prime())]).unwrap_or_default();
            if let Some(r) = o.lines().find(|l| l.contains("OpenGL renderer string")) {
                add(
                    &a,
                    "OpenGL",
                    Level::Ok,
                    r.split(':').nth(1).unwrap_or("").trim().into(),
                    None,
                );
            }
        }
    }

    // ---- ROCm / permissions ----
    let groups = out(&["id", "-nG"], &[]).unwrap_or_default();
    let has_compute = topo
        .gpus
        .iter()
        .any(|g| g.role == crate::topo::GpuRole::Compute);
    for gname in ["render", "video"] {
        if !groups.split_whitespace().any(|x| x == gname) {
            add(
                "ROCm",
                &crate::tf!("group {gname}", "grupo {gname}"),
                if has_compute {
                    Level::Warn
                } else {
                    Level::Info
                },
                crate::i18n::tr("user not in the group").into(),
                Some(crate::i18n::tr(
                    "sudo usermod -aG render,video $USER (and log in again)",
                )),
            );
        }
    }
    if std::path::Path::new("/dev/kfd").exists() {
        add(
            "ROCm",
            "/dev/kfd",
            Level::Ok,
            crate::i18n::tr("exists").into(),
            None,
        );
    } else {
        add(
            "ROCm",
            "/dev/kfd",
            Level::Fail,
            crate::i18n::tr("missing").into(),
            None,
        );
    }

    // ---- ferramentas ----
    for (bin, why, pkg) in [
        ("podman", crate::i18n::tr("lab, AI server"), "podman"),
        ("mangohud", crate::i18n::tr("FPS optimizer"), "mangohud"),
        (
            "gamescope",
            crate::i18n::tr("gamescope per game"),
            "gamescope",
        ),
        ("vainfo", crate::i18n::tr("Streaming tab"), "libva-utils"),
        ("steam", crate::i18n::tr("games"), "steam"),
    ] {
        if which(bin) {
            add(
                crate::i18n::tr("Tools"),
                bin,
                Level::Ok,
                crate::i18n::tr("installed").into(),
                None,
            );
        } else {
            add(
                crate::i18n::tr("Tools"),
                bin,
                Level::Info,
                crate::tf!("not installed ({why})", "não instalado ({why})"),
                Some(&format!("sudo pacman -S {pkg}")),
            );
        }
    }
    // vulkaninfo
    if !have_vk {
        add(
            crate::i18n::tr("Tools"),
            "vulkaninfo",
            Level::Info,
            crate::i18n::tr("not installed: could not test GPU selection in Vulkan").into(),
            Some("sudo pacman -S vulkan-tools"),
        );
    }
    // ---- Steam ----
    match crate::steam::find_localconfigs().first() {
        Some(lc) => {
            let games = crate::steam::installed_games();
            let opts = crate::steam::read_launch_options(lc).unwrap_or_default();
            let with = games
                .iter()
                .filter(|(id, _)| opts.get(id).is_some_and(|o| crate::steam::has_numapilot(o)))
                .count();
            add(
                "Steam",
                crate::i18n::tr("games with numapilot"),
                if with == games.len() && !games.is_empty() {
                    Level::Ok
                } else {
                    Level::Info
                },
                crate::tf!(
                    "{with} of {} installed games",
                    "{with} de {} jogos instalados",
                    games.len()
                ),
                Some(crate::i18n::tr(
                    "Interface → Programs → \"Apply to all Steam games\"",
                )),
            );
        }
        None => add(
            "Steam",
            crate::i18n::tr("configuration"),
            Level::Info,
            crate::i18n::tr("Steam not found").into(),
            None,
        ),
    }
    v
}

pub fn print(checks: &[Check]) {
    let mut area = String::new();
    for c in checks {
        if c.area != area {
            println!("\n== {} ==", c.area);
            area = c.area.clone();
        }
        let tag = match c.level {
            Level::Ok => "[ ok ]",
            Level::Info => "[info]",
            Level::Warn => crate::i18n::tr("[warn]"),
            Level::Fail => crate::i18n::tr("[FAIL]"),
        };
        println!("{tag} {}: {}", c.name, c.detail);
        if let Some(f) = &c.fix
            && c.level != Level::Ok
        {
            println!("        → {f}");
        }
    }
    let fails = checks.iter().filter(|c| c.level == Level::Fail).count();
    let warns = checks.iter().filter(|c| c.level == Level::Warn).count();
    println!(
        "{}",
        crate::tf!(
            "\nSummary: {fails} failure(s), {warns} warning(s).",
            "\nResumo: {fails} falha(s), {warns} aviso(s)."
        )
    );
}

/// Full report to send (diagnosis + topology + system).
pub fn report(checks: &[Check]) -> String {
    let mut s = String::from("# numapilot — diagnosis report\n\n");
    s.push_str(&crate::tf!(
        "version: {}\n",
        "versão: {}\n",
        env!("CARGO_PKG_VERSION")
    ));
    for c in checks {
        s.push_str(&format!(
            "[{:?}] {} / {}: {}{}\n",
            c.level,
            c.area,
            c.name,
            c.detail,
            c.fix
                .as_ref()
                .map(|f| format!("  (→ {f})"))
                .unwrap_or_default()
        ));
    }
    let sections: &[(&str, &[&str])] = &[
        ("uname", &["uname", "-a"]),
        ("cmdline", &["cat", "/proc/cmdline"]),
        ("lscpu", &["lscpu"]),
        ("numactl", &["numactl", "-H"]),
        (crate::i18n::tr("lspci (tree)"), &["lspci", "-tv"]),
        (
            "lspci (GPUs)",
            &["sh", "-c", "lspci -nnk | grep -A3 -Ei 'vga|display|3d'"],
        ),
        ("numapilot topo", &["numapilot", "topo"]),
        ("gfx906-rocm doctor", &["gfx906-rocm", "doctor"]),
        (
            crate::i18n::tr("service (log)"),
            &["journalctl", "-u", "numapilot", "-n", "80", "--no-pager"],
        ),
        (
            "vulkan",
            &[
                "sh",
                "-c",
                "MESA_VK_DEVICE_SELECT=list vulkaninfo 2>&1 | head -20",
            ],
        ),
    ];
    for (title, argv) in sections {
        s.push_str(&format!("\n## {title}\n"));
        s.push_str(&out(argv, &[]).unwrap_or_else(|| crate::i18n::tr("(unavailable)\n").into()));
    }
    s
}
