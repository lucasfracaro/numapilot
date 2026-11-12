//! `numapilot selftest`: goes through every part of numapilot on this machine, read-only
//! (changes nothing), and writes a report with OK / WARN / FAILED / SKIPPED on each item.

use crate::config::Config;
use crate::containers::which;
use crate::topo::Topology;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum St {
    Ok,
    Warn,
    Failed,
    Skipped,
}

impl St {
    pub fn label(self) -> &'static str {
        match self {
            St::Ok => "OK",
            St::Warn => "WARN",
            St::Failed => "FAILED",
            St::Skipped => "SKIPPED",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub grupo: String,
    pub nome: String,
    pub st: St,
    pub detalhe: String,
    pub ms: u128,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub versao: String,
    pub maquina: String,
    pub data: String,
    pub itens: Vec<Item>,
}

impl Report {
    fn count(&self, s: St) -> usize {
        self.itens.iter().filter(|i| i.st == s).count()
    }
    pub fn failed(&self) -> usize {
        self.count(St::Failed)
    }
    pub fn summary(&self) -> String {
        crate::tf!(
            "{} tests: {} OK, {} warnings, {} failed, {} skipped",
            "{} testes: {} OK, {} avisos, {} falharam, {} pulados",
            self.itens.len(),
            self.count(St::Ok),
            self.count(St::Warn),
            self.count(St::Failed),
            self.count(St::Skipped)
        )
    }
}

pub fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "pc".into())
}

pub fn default_path() -> PathBuf {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into()));
    let d = crate::log::now().replace([' ', ':'], "-");
    home.join(format!(
        "numapilot-autoteste-{}-{}.txt",
        hostname(),
        &d[..16.min(d.len())]
    ))
}

struct Run<'a> {
    items: Vec<Item>,
    out: &'a mut dyn FnMut(&str),
    group: String,
}

impl Run<'_> {
    fn group(&mut self, g: &str) {
        self.group = g.to_string();
        (self.out)("");
        (self.out)(&format!("== {g} =="));
    }
    fn t(&mut self, nome: &str, f: impl FnOnce() -> (St, String)) {
        let t0 = Instant::now();
        let (st, detalhe) = f();
        let ms = t0.elapsed().as_millis();
        (self.out)(&format!(
            "  {:<7} {nome}: {}",
            st.label(),
            detalhe.lines().next().unwrap_or("")
        ));
        for extra in detalhe.lines().skip(1).take(8) {
            (self.out)(&format!("          {extra}"));
        }
        self.items.push(Item {
            grupo: self.group.clone(),
            nome: nome.to_string(),
            st,
            detalhe,
            ms,
        });
    }
}

fn read(p: &str) -> Option<String> {
    std::fs::read_to_string(p)
        .ok()
        .map(|s| s.trim().to_string())
}

fn sh(argv: &[&str], secs: u64) -> Option<(bool, String)> {
    let o = Command::new("timeout")
        .arg(secs.to_string())
        .args(argv)
        .output()
        .ok()?;
    Some((
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr),
    ))
}

fn meminfo(key: &str) -> u64 {
    read("/proc/meminfo")
        .unwrap_or_default()
        .lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

/// Runs every test. `cli`: also runs each numapilot command (only the read-only ones).
pub fn run(cfg: &Config, cli: bool, out: &mut dyn FnMut(&str)) -> Report {
    let mut r = Run {
        items: vec![],
        out,
        group: String::new(),
    };
    (r.out)(&crate::tf!(
        "numapilot {} — self-test on {} ({})",
        "numapilot {} — autoteste em {} ({})",
        env!("CARGO_PKG_VERSION"),
        hostname(),
        crate::log::now()
    ));
    (r.out)(crate::i18n::tr(
        "Read-only: nothing is changed on this machine.",
    ));

    // ---------------------------------------------------------------- sistema
    r.group(crate::i18n::tr("System"));
    r.t(crate::i18n::tr("Version"), || {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let how = match crate::selfupdate::how() {
            crate::selfupdate::How::Package(p) => crate::tf!("package {p}", "pacote {p}"),
            crate::selfupdate::How::Source(d) => {
                crate::tf!("code in {}", "código em {}", d.display())
            }
            crate::selfupdate::How::Unknown => {
                crate::i18n::tr("installed by install.sh (no git repository)").to_string()
            }
        };
        (
            St::Ok,
            format!("{} · {exe} · {how}", env!("CARGO_PKG_VERSION")),
        )
    });
    r.t(crate::i18n::tr("Kernel and system"), || {
        let os = read("/etc/os-release")
            .unwrap_or_default()
            .lines()
            .find_map(|l| {
                l.strip_prefix("PRETTY_NAME=")
                    .map(|v| v.trim_matches('"').to_string())
            })
            .unwrap_or_default();
        (
            St::Ok,
            format!(
                "{os} · kernel {} · {}",
                crate::recipes::running_kernel(),
                crate::recipes::cpu_model()
            ),
        )
    });
    let topo = Topology::load(cfg);
    r.t(crate::i18n::tr("Topology"), || {
        let cpus = topo.all_cpus().len();
        if topo.nodes.is_empty() || cpus == 0 {
            return (
                St::Failed,
                crate::i18n::tr("could not find sockets/cores in /sys").into(),
            );
        }
        let gpus: Vec<String> = topo
            .gpus
            .iter()
            .map(|g| format!("{} ({}, {})", g.alias, g.name, g.pci))
            .collect();
        (
            St::Ok,
            format!(
                "{} socket(s), {cpus} threads, {} GPU(s)\n{}",
                topo.nodes.len(),
                topo.gpus.len(),
                gpus.join("\n")
            ),
        )
    });
    r.t(crate::i18n::tr("CPU usage"), || {
        let a = crate::procfs::cpu_times();
        std::thread::sleep(Duration::from_millis(300));
        let b = crate::procfs::cpu_times();
        if a.is_empty() || a.len() != b.len() {
            return (
                St::Failed,
                crate::i18n::tr("could not read /proc/stat").into(),
            );
        }
        let (mut busy, mut tot) = (0u64, 0u64);
        for (x, y) in a.iter().zip(&b) {
            busy += y.1.saturating_sub(x.1);
            tot += y.2.saturating_sub(x.2);
        }
        (
            St::Ok,
            crate::tf!(
                "{} threads read, {:.0}% in use now",
                "{} threads lidos, {:.0}% em uso agora",
                a.len(),
                busy as f64 * 100.0 / tot.max(1) as f64
            ),
        )
    });
    r.t(crate::i18n::tr("Memory"), || {
        let t = meminfo("MemTotal:");
        let a = meminfo("MemAvailable:");
        if t == 0 {
            return (
                St::Failed,
                crate::i18n::tr("could not read /proc/meminfo").into(),
            );
        }
        let per_node: Vec<String> = topo
            .nodes
            .iter()
            .map(|n| {
                crate::tf!(
                    "socket {}: {:.1} of {:.0} GB in use",
                    "socket {}: {:.1} de {:.0} GB em uso",
                    n.id,
                    n.mem_used_kb() as f64 / 1048576.0,
                    n.mem_total_kb as f64 / 1048576.0
                )
            })
            .collect();
        (
            St::Ok,
            crate::tf!(
                "{:.1} of {:.0} GB in use (not counting the cache)\n{}",
                "{:.1} de {:.0} GB em uso (sem contar o cache)\n{}",
                (t - a) as f64 / 1048576.0,
                t as f64 / 1048576.0,
                per_node.join("\n")
            ),
        )
    });
    r.t(crate::i18n::tr("Processor temperature"), || {
        let t = crate::cpuctl::package_temps();
        if t.is_empty() {
            (
                St::Warn,
                crate::i18n::tr("no sensor (k10temp/zenpower/coretemp) found").into(),
            )
        } else {
            let v: Vec<String> = t
                .iter()
                .map(|(p, c)| format!("socket {p}: {c:.0} °C"))
                .collect();
            (St::Ok, v.join(", "))
        }
    });
    r.t(crate::i18n::tr("Processor frequency"), || {
        let b = "/sys/devices/system/cpu/cpu0/cpufreq";
        let Some(gov) = read(&format!("{b}/scaling_governor")) else {
            let mhz = crate::cpuctl::cur_mhz_all();
            if let Some(max) = mhz.values().max() {
                return (
                    St::Ok,
                    crate::tf!(
                        "{:.2} GHz now (max among the cores, from /proc/cpuinfo) · no cpufreq: read-only, the firmware exposes no control (BC-250: use bc250_smu_oc)", "{:.2} GHz agora (máx. entre os núcleos, pelo /proc/cpuinfo) · sem cpufreq: só leitura, o firmware não expõe controle (BC-250: use o bc250_smu_oc)",
                        *max as f64 / 1000.0
                    ),
                );
            }
            return (St::Warn, crate::i18n::tr("no cpufreq (VM or missing driver)").into());
        };
        let drv = read(&format!("{b}/scaling_driver")).unwrap_or_default();
        let cur = read(&format!("{b}/scaling_cur_freq"))
            .and_then(|v| v.parse::<u64>().ok())
            .map(|k| format!("{:.2} GHz", k as f64 / 1e6))
            .unwrap_or_default();
        let epp = read(&format!("{b}/energy_performance_preference"))
            .map(|e| format!(" · EPP {e}"))
            .unwrap_or_default();
        (St::Ok, crate::tf!("{drv} · governor {gov} · {cur}{epp}", "{drv} · governador {gov} · {cur}{epp}"))
    });

    // ---------------------------------------------------------------- service
    r.group(crate::i18n::tr("Service"));
    r.t(crate::i18n::tr("numapilot service"), || {
        match sh(&["systemctl", "is-active", "numapilot"], 10) {
            Some((true, _)) => (St::Ok, crate::i18n::tr("active").into()),
            Some((_, o)) => (
                St::Failed,
                crate::tf!(
                    "{} — enable it with: sudo systemctl enable --now numapilot",
                    "{} — ative com: sudo systemctl enable --now numapilot",
                    o.trim()
                ),
            ),
            None => (St::Skipped, crate::i18n::tr("no systemctl").into()),
        }
    });
    r.t(crate::i18n::tr("Talking to the service"), || {
        let t0 = Instant::now();
        match crate::ctl::send(&crate::ctl::Request::Ping) {
            Ok(_) => (
                St::Ok,
                crate::tf!(
                    "answered in {} ms",
                    "respondeu em {} ms",
                    t0.elapsed().as_millis()
                ),
            ),
            Err(e) => (
                St::Failed,
                format!("{e} ({})", crate::ctl::sock_path().display()),
            ),
        }
    });
    r.t(crate::i18n::tr("Service log"), || {
        let p = std::path::Path::new("/var/log/numapilot/daemon.log");
        if !p.exists() {
            return (
                St::Skipped,
                crate::i18n::tr("does not exist yet (this version's service has not run yet)")
                    .into(),
            );
        }
        let t = crate::log::tail(p, 400);
        let errs: Vec<&String> = t
            .iter()
            .filter(|l| l.contains(" ERRO ") || l.contains(" ERROR "))
            .collect();
        let warns = t
            .iter()
            .filter(|l| l.contains(" AVISO ") || l.contains(" WARN "))
            .count();
        if errs.is_empty() {
            (
                St::Ok,
                crate::tf!(
                    "{} recent lines, {warns} warnings, no errors",
                    "{} linhas recentes, {warns} avisos, nenhum erro",
                    t.len()
                ),
            )
        } else {
            let last: Vec<String> = errs.iter().rev().take(5).map(|s| s.to_string()).collect();
            (
                St::Warn,
                crate::tf!(
                    "{} errors and {warns} warnings in the last {} lines; the most recent:\n{}",
                    "{} erros e {warns} avisos nas últimas {} linhas; os mais recentes:\n{}",
                    errs.len(),
                    t.len(),
                    last.join("\n")
                ),
            )
        }
    });
    r.t(
        crate::i18n::tr("Service errors in the journal"),
        || match sh(
            &[
                "journalctl",
                "-u",
                "numapilot",
                "-b",
                "-p",
                "err",
                "--no-pager",
                "-q",
                "-o",
                "cat",
            ],
            15,
        ) {
            Some((_, o)) => {
                let lines: Vec<&str> = o.lines().filter(|l| !l.trim().is_empty()).collect();
                if lines.is_empty() {
                    (St::Ok, crate::i18n::tr("none in this boot").into())
                } else {
                    (
                        St::Warn,
                        crate::tf!(
                            "{} in this boot; the latest:\n{}",
                            "{} neste boot; os últimos:\n{}",
                            lines.len(),
                            lines
                                .iter()
                                .rev()
                                .take(5)
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("\n")
                        ),
                    )
                }
            }
            None => (St::Skipped, crate::i18n::tr("no journalctl").into()),
        },
    );
    r.t(crate::i18n::tr("Web panel and /metrics"), || {
        let active = sh(&["systemctl", "--user", "is-active", "numapilot-web"], 10)
            .is_some_and(|(ok, _)| ok);
        if !active {
            return (
                St::Skipped,
                crate::i18n::tr("panel is not on (Settings → phone panel)").into(),
            );
        }
        let Ok(tok) = crate::web::token() else {
            return (St::Warn, crate::i18n::tr("panel on, but no token").into());
        };
        let url = format!(
            "http://127.0.0.1:{}/metrics?t={}",
            crate::web::DEFAULT_PORT,
            tok.trim()
        );
        match sh(&["curl", "-s", "--noproxy", "*", "-m", "8", &url], 10) {
            Some((true, o)) if o.contains("numapilot_") => (
                St::Ok,
                crate::tf!(
                    "answered ({} metrics)",
                    "respondeu ({} métricas)",
                    o.lines().filter(|l| !l.starts_with('#')).count()
                ),
            ),
            Some((_, o)) => (
                St::Failed,
                crate::tf!(
                    "did not answer properly: {}",
                    "não respondeu direito: {}",
                    o.lines().next().unwrap_or("")
                ),
            ),
            None => (St::Skipped, crate::i18n::tr("no curl").into()),
        }
    });

    // ---------------------------------------------------------------- configuration
    r.group(crate::i18n::tr("Configuration"));
    r.t(crate::i18n::tr("Main configuration"), || {
        (St::Ok, crate::tf!("{} rules", "{} regras", cfg.rules.len()))
    });
    r.t(crate::i18n::tr("Machine (mode, power, laptop)"), || {
        let m = crate::machine::Machine::load();
        match m.validate() {
            Ok(()) => (
                St::Ok,
                crate::tf!(
                    "mode {} · {} schedules{}",
                    "modo {} · {} horários{}",
                    m.mode,
                    m.schedule.len(),
                    m.dgpu_mode
                        .as_ref()
                        .map(|d| format!(" · NVIDIA: {d}"))
                        .unwrap_or_default()
                ),
            ),
            Err(e) => (
                St::Failed,
                format!("{}: {e}", crate::machine::Machine::path().display()),
            ),
        }
    });
    r.t(crate::i18n::tr("Saved choices"), || {
        let p = crate::profiles::Profiles::load();
        let bad: Vec<String> = p
            .map
            .iter()
            .filter_map(|(k, v)| v.validate().err().map(|e| format!("{k}: {e}")))
            .collect();
        if bad.is_empty() {
            (
                St::Ok,
                crate::tf!("{} programs", "{} programas", p.map.len()),
            )
        } else {
            (
                St::Failed,
                crate::tf!(
                    "{} invalid:\n{}",
                    "{} inválidas:\n{}",
                    bad.len(),
                    bad.join("\n")
                ),
            )
        }
    });
    r.t(crate::i18n::tr("Other PCs and notifications"), || {
        let peers = crate::peers::load();
        let n = crate::notify::Notify::load();
        let nt = if n.enabled {
            match n.validate() {
                Ok(()) => crate::i18n::tr("phone notifications on").to_string(),
                Err(e) => {
                    return (
                        St::Failed,
                        crate::tf!("phone notifications: {e}", "avisos no celular: {e}"),
                    );
                }
            }
        } else {
            crate::i18n::tr("phone notifications off").into()
        };
        (
            St::Ok,
            crate::tf!(
                "{} PCs connected · {nt}",
                "{} PCs ligados · {nt}",
                peers.len()
            ),
        )
    });
    r.t(crate::i18n::tr("Recipes and special boards"), || {
        let rc = crate::recipes::load();
        let boards: Vec<String> = rc
            .boards(&topo)
            .iter()
            .map(|(b, g)| format!("{} ({})", b.name, g.alias))
            .collect();
        (
            St::Ok,
            if boards.is_empty() {
                crate::i18n::tr("no special board").into()
            } else {
                crate::tf!("special board: {}", "placa especial: {}", boards.join(", "))
            },
        )
    });

    // ---------------------------------------------------------------- GPUs
    r.group(crate::i18n::tr("Graphics cards"));
    if topo.gpus.is_empty() {
        r.t("GPUs", || {
            (St::Warn, crate::i18n::tr("no GPU found").into())
        });
    }
    let vk = if which("vulkaninfo") {
        sh(&["vulkaninfo", "--summary"], 30).map(|(_, o)| o.to_lowercase())
    } else {
        None
    };
    for g in topo.gpus.clone() {
        let a = g.alias.clone();
        r.t(&crate::tf!("{a}: sensors", "{a}: sensores"), || {
            let s = crate::gpuctl::read_sensors(&g.pci);
            let mut v = vec![];
            if let Some(t) = s.junction_c.or(s.edge_c) {
                v.push(format!("{t:.0} °C"));
            }
            if let Some(w) = s.power_w {
                v.push(format!("{w:.0} W"));
            }
            if let Some(f) = s.fan_rpm {
                v.push(crate::tf!("fan {f} RPM", "ventoinha {f} RPM"));
            }
            if s.edge_c.is_none()
                && s.junction_c.is_none()
                && s.power_w.is_none()
                && let Some(why) = crate::boards::smu_table_mismatch(&g.pci)
            {
                let clk = s
                    .clock_mhz
                    .map(|c| format!("clock {c:.0} MHz · "))
                    .unwrap_or_default();
                return (St::Warn, format!("{clk}{why}"));
            }
            if let Some(c) = s.clock_mhz {
                v.push(format!("{c:.0} MHz"));
            }
            if g.is_nvidia() && crate::nvctl::asleep(&g.pci) {
                return (
                    St::Ok,
                    crate::i18n::tr("asleep (power saving): sensors not read so as not to wake it").into(),
                );
            }
            if v.is_empty() && crate::gpuctl::hwmon_dir(&g.pci).is_some() {
                (
                    St::Warn,
                    crate::i18n::tr("the firmware sends impossible values (e.g. 0 °C, more than 1000 W): ignored")
                        .into(),
                )
            } else if v.is_empty() {
                (
                    St::Failed,
                    crate::i18n::tr("could not read temperature or power").into(),
                )
            } else {
                (St::Ok, v.join(" · "))
            }
        });
        r.t(
            &crate::tf!("{a}: usage and memory", "{a}: uso e memória"),
            || {
                let b = g.busy_percent();
                let m = g.vram_used_total();
                if b.is_none() && m.is_none() {
                    return (
                        St::Warn,
                        crate::i18n::tr("usage and VRAM not available").into(),
                    );
                }
                (
                    St::Ok,
                    format!(
                        "{}{}",
                        b.map(|b| crate::tf!("{b}% usage", "{b}% de uso"))
                            .unwrap_or_default(),
                        m.map(|(u, t)| format!(
                            " · VRAM {:.1}/{:.1} GB",
                            u as f64 / 1073741824.0,
                            t as f64 / 1073741824.0
                        ))
                        .unwrap_or_default()
                    ),
                )
            },
        );
        r.t(
            &crate::tf!("{a}: access for programs", "{a}: acesso para programas"),
            || {
                let Some(rn) = g.render.clone() else {
                    return (St::Skipped, crate::i18n::tr("no render node").into());
                };
                let p = format!("/dev/dri/{rn}");
                match std::fs::OpenOptions::new().read(true).write(true).open(&p) {
                    Ok(_) => (St::Ok, crate::tf!("{p} opens", "{p} abre")),
                    Err(e) => (
                        St::Failed,
                        crate::tf!(
                            "{p}: {e} (your user must be in the render and video groups)",
                            "{p}: {e} (seu usuário precisa estar nos grupos render e video)"
                        ),
                    ),
                }
            },
        );
        r.t(&crate::tf!("{a}: control", "{a}: controle"), || {
            if g.is_nvidia() {
                match crate::nvctl::get(&g.pci) {
                    Some(n) => {
                        let nv = crate::nvml::offsets(&g.pci)
                            .map(|_| crate::i18n::tr(" · NVML ok (clock offset)"))
                            .unwrap_or(crate::i18n::tr(" · NVML unavailable (no clock offset)"));
                        (
                            St::Ok,
                            format!(
                                "nvidia-smi ok · driver {}{nv}{}",
                                crate::nvctl::driver_version().unwrap_or_default(),
                                n.power_limit_w
                                    .map(|w| crate::tf!(" · limit {w:.0} W", " · limite {w:.0} W"))
                                    .unwrap_or_default()
                            ),
                        )
                    }
                    None if crate::nvctl::asleep(&g.pci) => {
                        (St::Ok, crate::i18n::tr("asleep").into())
                    }
                    None => (
                        St::Failed,
                        crate::i18n::tr("nvidia-smi did not answer for this card").into(),
                    ),
                }
            } else {
                let od = std::path::Path::new(&crate::util::sys(&format!(
                    "bus/pci/devices/{}/pp_od_clk_voltage",
                    g.pci
                )))
                .exists();
                let pl = crate::gpuctl::read_sensors(&g.pci).power_cap_w;
                (
                    St::Ok,
                    format!(
                        "{}{}",
                        if od {
                            crate::i18n::tr("clock/voltage available (OverDrive on)")
                        } else {
                            crate::i18n::tr("clock/voltage unavailable (OverDrive off)")
                        },
                        pl.map(|w| crate::tf!(" · limit {w:.0} W", " · limite {w:.0} W"))
                            .unwrap_or_default()
                    ),
                )
            }
        });
        r.t(&format!("{a}: Vulkan"), || {
            let Some(vk) = &vk else {
                return (
                    St::Skipped,
                    crate::i18n::tr("vulkaninfo not installed (vulkan-tools package)").into(),
                );
            };
            if g.is_nvidia() && crate::nvctl::asleep(&g.pci) {
                return (St::Skipped, crate::i18n::tr("asleep").into());
            }
            let id = format!("0x{:04x}", g.device);
            if vk.contains(&id) {
                (
                    St::Ok,
                    crate::tf!("shows up in Vulkan ({id})", "aparece no Vulkan ({id})"),
                )
            } else {
                (
                    St::Warn,
                    crate::tf!(
                        "does not show up in vulkaninfo ({id})",
                        "não aparece no vulkaninfo ({id})"
                    ),
                )
            }
        });
    }

    r.group(crate::i18n::tr("Hardware health"));
    r.t(crate::i18n::tr("Service collection"), || {
        let Some(h) = crate::health::load() else {
            // right after boot (or a service restart) there has not been time yet: not a defect
            if let Some(s) = service_uptime_secs().filter(|&s| s < 180) {
                return (
                    St::Skipped,
                    crate::tf!(
                        "the service started {s} s ago and collects 1 min later: run the self-test again in a moment", "o serviço iniciou há {s} s e coleta 1 min depois: rode o autoteste de novo daqui a pouco"
                    ),
                );
            }
            return (
                St::Warn,
                crate::t2!("not collected yet (the service collects 1 min after starting)", "ainda não coletado (o serviço coleta 1 min depois de iniciar)").into(),
            );
        };
        let age = crate::history::now().saturating_sub(h.time) / 60;
        let mut lines = vec![];
        for d in &h.disks {
            lines.push(crate::tf!(
                "disk {} {}: SMART {}{}", "disco {} {}: SMART {}{}",
                d.dev,
                d.model,
                match d.passed {
                    Some(true) => "ok",
                    Some(false) => "FAILED",
                    None => "?",
                },
                d.temp_c
                    .map(|t| format!(" · {t:.0} °C"))
                    .unwrap_or_default()
            ));
        }
        for e in &h.disk_errors {
            lines.push(crate::tf!("disk not read: {e}", "disco não lido: {e}"));
        }
        for b in &h.batteries {
            lines.push(crate::tf!(
                "battery {}: {}", "bateria {}: {}",
                b.name,
                b.health_pct
                    .map(|p| crate::tf!("{p:.0}% of original", "{p:.0}% da original"))
                    .unwrap_or_default()
            ));
        }
        let st = if !h.smartctl || h.disks.is_empty() {
            St::Warn
        } else {
            St::Ok
        };
        (
            st,
            crate::tf!(
                "collected {age} min ago · {} disk(s){}\n{}", "coletado há {age} min · {} disco(s){}\n{}",
                h.disks.len(),
                if h.smartctl {
                    ""
                } else {
                    crate::i18n::tr(" (smartctl not found by the service)")
                },
                lines.join("\n")
            ),
        )
    });

    // ---------------------------------------------------------------- notebook
    if crate::notebook::is_laptop() {
        r.group("Notebook");
        r.t(crate::i18n::tr("Battery and AC power"), || {
            let b = crate::notebook::battery_info();
            let ac = match crate::notebook::on_ac() {
                Some(true) => crate::i18n::tr("on AC power"),
                Some(false) => crate::i18n::tr("on battery"),
                None => crate::i18n::tr("AC state unknown"),
            };
            (
                St::Ok,
                crate::tf!("{ac} · {} battery(ies)", "{ac} · {} bateria(s)", b.len()),
            )
        });
        r.t(
            crate::i18n::tr("Power profile"),
            || match crate::notebook::platform_profile() {
                Some(p) => (
                    St::Ok,
                    crate::tf!(
                        "{p} (options: {})",
                        "{p} (opções: {})",
                        crate::notebook::platform_profiles().join(", ")
                    ),
                ),
                None => (
                    St::Warn,
                    crate::tf!(
                        "no power profile available\n{}",
                        "nenhum perfil de energia disponível\n{}",
                        crate::notebook::acer_diagnose().join("\n")
                    ),
                ),
            },
        );
        r.t(crate::i18n::tr("Hybrid NVIDIA"), || {
            let p = crate::learned::run_dir().join("dgpu.json");
            match std::fs::read_to_string(&p) {
                Ok(t) => match serde_json::from_str::<crate::hybrid::State>(&t) {
                    Ok(s) => (
                        if s.last_error.is_some() {
                            St::Warn
                        } else {
                            St::Ok
                        },
                        crate::tf!(
                            "mode {} · {} · {}{}",
                            "modo {} · {} · {}{}",
                            s.mode,
                            if s.removed {
                                crate::i18n::tr("off (removed)")
                            } else {
                                crate::i18n::tr("present")
                            },
                            if s.blocked_by.is_empty() {
                                crate::i18n::tr("nothing holding it").to_string()
                            } else {
                                crate::tf!("held by {}", "segurada por {}", s.blocked_by.join(", "))
                            },
                            s.last_error
                                .map(|e| crate::tf!("\nlast error: {e}", "\núltimo erro: {e}"))
                                .unwrap_or_default()
                        ),
                    ),
                    Err(e) => (St::Failed, format!("{}: {e}", p.display())),
                },
                Err(_) => (
                    St::Skipped,
                    crate::i18n::tr("no hybrid NVIDIA or service stopped").into(),
                ),
            }
        });
        r.t("Acer", || {
            if !crate::acer::is_acer() {
                return (St::Skipped, crate::i18n::tr("not an Acer").into());
            }
            let k = crate::acer::knobs();
            let have: Vec<String> = k
                .iter()
                .filter_map(|(n, v)| v.as_ref().map(|v| format!("{n}={v}")))
                .collect();
            if have.is_empty() {
                (
                    St::Warn,
                    crate::tf!(
                        "no Acer control (predator_v4/Linuwu-Sense) available\n{}",
                        "nenhum controle Acer (predator_v4/Linuwu-Sense) disponível\n{}",
                        crate::notebook::acer_diagnose().join("\n")
                    ),
                )
            } else {
                (St::Ok, have.join(" · "))
            }
        });
        r.t(crate::i18n::tr("Ryzen limits (ryzenadj)"), || {
            if !crate::ryzen::available() {
                return (
                    St::Skipped,
                    crate::i18n::tr("ryzenadj not installed or not a laptop Ryzen").into(),
                );
            }
            if unsafe { libc::geteuid() } != 0 {
                return (
                    St::Skipped,
                    crate::i18n::tr("needs root to read (the service reads it)").into(),
                );
            }
            match crate::ryzen::info() {
                Ok(m) => (
                    St::Ok,
                    crate::tf!("{} values read", "{} valores lidos", m.len()),
                ),
                Err(e) => (St::Failed, e),
            }
        });
    }

    // ---------------------------------------------------------------- special boards
    let rc = crate::recipes::load();
    let boards: Vec<String> = rc
        .boards(&topo)
        .iter()
        .map(|(b, _)| b.name.clone())
        .collect();
    if !boards.is_empty() {
        r.group(crate::i18n::tr("Special board"));
        let full_cores = rc
            .boards(&topo)
            .iter()
            .find_map(|(b, _)| b.cores)
            .map(|c| c.1);
        r.t(crate::i18n::tr("Active cores"), || {
            let n = crate::boards::active_cores();
            let efi = crate::boards::efi_core_unlock();
            let keep = full_cores
                .and_then(|f| crate::boards::core_unlock_persistence(n, f, efi))
                .map(|p| format!(" · {p}"))
                .unwrap_or_default();
            (
                St::Ok,
                crate::tf!(
                    "{n} physical cores online ({} threads){keep}",
                    "{n} núcleos físicos online ({} threads){keep}",
                    std::thread::available_parallelism().map_or(0, |n| n.get())
                ),
            )
        });
        r.t(crate::i18n::tr("GPU CUs"), || {
            match crate::boards::cu_now() {
                Some((c, crate::boards::CuSource::Kernel)) => (
                    St::Ok,
                    crate::tf!(
                        "{c} CUs active (unlocked by the kernel: bc250_cc_write_mode=3)",
                        "{c} CUs ativas (liberadas pelo kernel: bc250_cc_write_mode=3)"
                    ),
                ),
                Some((c, src)) => (
                    St::Ok,
                    crate::tf!("{c} CUs active ({})", "{c} CUs ativas ({})", src.text()),
                ),
                None => (
                    St::Warn,
                    crate::i18n::tr("could not find active_cu_number in the kernel log or in KFD")
                        .into(),
                ),
            }
        });
        r.t(crate::i18n::tr("GPU memory (TTM)"), || {
            let now = crate::bootcfg::ttm_now();
            let saved = crate::bootcfg::saved_ttm();
            let gib = |p: u64| p as f64 * 4096.0 / 1073741824.0;
            (
                St::Ok,
                crate::tf!(
                    "now {} · saved {}",
                    "agora {} · salvo {}",
                    now.map(|p| format!("{:.1} GiB", gib(p)))
                        .unwrap_or(crate::i18n::tr("default").into()),
                    saved
                        .map(|p| format!("{:.1} GiB", gib(p)))
                        .unwrap_or(crate::i18n::tr("nothing").into())
                ),
            )
        });
    }

    // ---------------------------------------------------------------- diagnosis
    r.group(crate::i18n::tr("Diagnosis (numapilot doctor)"));
    r.t(crate::i18n::tr("Doctor checks"), || {
        let checks = crate::doctor::run(cfg);
        use crate::doctor::Level;
        let fails: Vec<String> = checks
            .iter()
            .filter(|c| c.level == Level::Fail)
            .map(|c| format!("[{}] {}: {}", c.area, c.name, c.detail))
            .collect();
        let warns: Vec<String> = checks
            .iter()
            .filter(|c| c.level == Level::Warn)
            .map(|c| format!("[{}] {}: {}", c.area, c.name, c.detail))
            .collect();
        let head = crate::tf!(
            "{} checks: {} problems, {} warnings",
            "{} verificações: {} problemas, {} avisos",
            checks.len(),
            fails.len(),
            warns.len()
        );
        let body = fails
            .iter()
            .map(|s| crate::tf!("problem: {s}", "problema: {s}"))
            .chain(
                warns
                    .iter()
                    .map(|s| crate::tf!("warning: {s}", "aviso: {s}")),
            )
            .collect::<Vec<_>>()
            .join("\n");
        let st = if !fails.is_empty() {
            St::Failed
        } else if !warns.is_empty() {
            St::Warn
        } else {
            St::Ok
        };
        (
            st,
            if body.is_empty() {
                head
            } else {
                format!("{head}\n{body}")
            },
        )
    });
    r.t(crate::i18n::tr("Helper programs"), || {
        let tools = [
            ("vkmark", crate::i18n::tr("automatic undervolt")),
            ("glmark2", crate::i18n::tr("automatic undervolt")),
            ("vulkaninfo", crate::i18n::tr("Vulkan test")),
            ("smartctl", crate::i18n::tr("disk health")),
            ("ethtool", crate::i18n::tr("wake over the network")),
            ("sunshine", "streaming"),
            ("podman", crate::i18n::tr("lab")),
            ("llama-server", crate::i18n::tr("AI server")),
            ("ryzenadj", crate::i18n::tr("Ryzen limits")),
            ("nvidia-smi", "NVIDIA"),
        ];
        let have: Vec<&str> = tools.iter().filter(|t| which(t.0)).map(|t| t.0).collect();
        let miss: Vec<String> = tools
            .iter()
            .filter(|t| !which(t.0))
            .map(|t| format!("{} ({})", t.0, t.1))
            .collect();
        (
            St::Ok,
            crate::tf!(
                "installed: {}\nmissing (only matters if you will use it): {}",
                "instalados: {}\nfaltando (só importa se for usar): {}",
                have.join(", "),
                if miss.is_empty() {
                    crate::t2!("none", "nenhum").into()
                } else {
                    miss.join(", ")
                }
            ),
        )
    });

    // ---------------------------------------------------------------- comandos
    if cli {
        r.group(crate::i18n::tr("Commands (read-only)"));
        let exe = std::env::current_exe()
            .map(|p| {
                let s = p.to_string_lossy().to_string();
                s.strip_suffix(" (deleted)")
                    .map(str::to_string)
                    .unwrap_or(s)
            })
            .unwrap_or_else(|_| "numapilot".into());
        let laptop_nv = crate::notebook::is_laptop() && topo.gpus.iter().any(|g| g.is_nvidia());
        let mut cmds: Vec<Vec<&str>> = vec![
            vec!["--version"],
            vec!["topo"],
            vec!["status"],
            vec!["doctor", "--json"],
            vec!["boot", "status"],
            vec!["recipes", "show"],
            vec!["peers", "list"],
            vec!["profile", "list"],
            vec!["learned"],
            vec!["self-update", "status"],
            vec!["vm", "check"],
            vec!["rocr-ids"],
            vec!["run", "--dry-run", "--", "true"],
            vec!["run", "--dry-run", "--gpu", "display", "--", "true"],
            vec!["log", "-n", "3"],
        ];
        if !boards.is_empty() {
            cmds.push(vec!["board", "status"]);
        }
        if laptop_nv {
            cmds.push(vec!["hybrid", "status"]);
            cmds.push(vec!["autostart", "status"]);
        }
        for c in cmds {
            let name = format!("numapilot {}", c.join(" "));
            let exe = exe.clone();
            r.t(&name, || {
                let mut argv = vec![exe.as_str()];
                argv.extend(c.iter().copied());
                let t0 = Instant::now();
                match sh(&argv, 90) {
                    Some((true, o)) => (
                        St::Ok,
                        format!(
                            "{} ms · {}",
                            t0.elapsed().as_millis(),
                            o.lines()
                                .find(|l| !l.trim().is_empty())
                                .unwrap_or(crate::i18n::tr("(no output)"))
                                .chars()
                                .take(110)
                                .collect::<String>()
                        ),
                    ),
                    Some((false, o)) => (
                        St::Failed,
                        crate::tf!(
                            "exited with an error:\n{}",
                            "saiu com erro:\n{}",
                            o.lines()
                                .rev()
                                .take(6)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .collect::<Vec<_>>()
                                .join("\n")
                        ),
                    ),
                    None => (St::Failed, crate::i18n::tr("could not run it").into()),
                }
            });
        }
    }

    Report {
        versao: env!("CARGO_PKG_VERSION").into(),
        maquina: hostname(),
        data: crate::log::now(),
        itens: r.items,
    }
}

/// Text report (what goes into the file).
pub fn render(r: &Report) -> String {
    let mut s = format!(
        "# numapilot — self-test\n\nversion {} · machine {} · {}\n{}\n",
        r.versao,
        r.maquina,
        r.data,
        r.summary()
    );
    let mut g = String::new();
    for i in &r.itens {
        if i.grupo != g {
            g = i.grupo.clone();
            s.push_str(&format!("\n## {g}\n"));
        }
        let mut lines = i.detalhe.lines();
        s.push_str(&format!(
            "{:<7} {} — {} ({} ms)\n",
            i.st.label(),
            i.nome,
            lines.next().unwrap_or(""),
            i.ms
        ));
        for l in lines {
            s.push_str(&format!("        {l}\n"));
        }
    }
    s
}

/// How many seconds numapilot.service has been active (None = stopped or no systemctl).
fn service_uptime_secs() -> Option<u64> {
    let o = std::process::Command::new("systemctl")
        .args([
            "show",
            "numapilot.service",
            "-p",
            "ActiveEnterTimestampMonotonic",
            "--value",
        ])
        .output()
        .ok()?;
    let since_us: u64 = String::from_utf8_lossy(&o.stdout).trim().parse().ok()?;
    if since_us == 0 {
        return None;
    }
    let up = std::fs::read_to_string("/proc/uptime").ok()?;
    let up_s: f64 = up.split_whitespace().next()?.parse().ok()?;
    Some((up_s as u64).saturating_sub(since_us / 1_000_000))
}
