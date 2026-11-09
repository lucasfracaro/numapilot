//! `numapilot autoteste`: passa por todas as partes do numapilot nesta máquina, só lendo
//! (não muda nada), e grava um relatório com OK / AVISO / FALHOU / PULOU em cada item.

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
    Aviso,
    Falhou,
    Pulou,
}

impl St {
    pub fn label(self) -> &'static str {
        match self {
            St::Ok => "OK",
            St::Aviso => "AVISO",
            St::Falhou => "FALHOU",
            St::Pulou => "PULOU",
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
        self.count(St::Falhou)
    }
    pub fn summary(&self) -> String {
        format!(
            "{} testes: {} OK, {} avisos, {} falharam, {} pulados",
            self.itens.len(),
            self.count(St::Ok),
            self.count(St::Aviso),
            self.count(St::Falhou),
            self.count(St::Pulou)
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

/// Roda todos os testes. `cli`: também roda cada comando do numapilot (só os que leem).
pub fn run(cfg: &Config, cli: bool, out: &mut dyn FnMut(&str)) -> Report {
    let mut r = Run {
        items: vec![],
        out,
        group: String::new(),
    };
    (r.out)(&format!(
        "numapilot {} — autoteste em {} ({})",
        env!("CARGO_PKG_VERSION"),
        hostname(),
        crate::log::now()
    ));
    (r.out)("Só lê: nada é mudado nesta máquina.");

    // ---------------------------------------------------------------- sistema
    r.group("Sistema");
    r.t("Versão", || {
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let how = match crate::selfupdate::how() {
            crate::selfupdate::How::Package(p) => format!("pacote {p}"),
            crate::selfupdate::How::Source(d) => format!("código em {}", d.display()),
            crate::selfupdate::How::Unknown => {
                "instalado pelo install.sh (sem repositório git)".to_string()
            }
        };
        (
            St::Ok,
            format!("{} · {exe} · {how}", env!("CARGO_PKG_VERSION")),
        )
    });
    r.t("Kernel e sistema", || {
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
    r.t("Topologia", || {
        let cpus = topo.all_cpus().len();
        if topo.nodes.is_empty() || cpus == 0 {
            return (St::Falhou, "não achei sockets/núcleos em /sys".into());
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
    r.t("Uso da CPU", || {
        let a = crate::procfs::cpu_times();
        std::thread::sleep(Duration::from_millis(300));
        let b = crate::procfs::cpu_times();
        if a.is_empty() || a.len() != b.len() {
            return (St::Falhou, "não consegui ler /proc/stat".into());
        }
        let (mut busy, mut tot) = (0u64, 0u64);
        for (x, y) in a.iter().zip(&b) {
            busy += y.1.saturating_sub(x.1);
            tot += y.2.saturating_sub(x.2);
        }
        (
            St::Ok,
            format!(
                "{} threads lidos, {:.0}% em uso agora",
                a.len(),
                busy as f64 * 100.0 / tot.max(1) as f64
            ),
        )
    });
    r.t("Memória", || {
        let t = meminfo("MemTotal:");
        let a = meminfo("MemAvailable:");
        if t == 0 {
            return (St::Falhou, "não consegui ler /proc/meminfo".into());
        }
        let per_node: Vec<String> = topo
            .nodes
            .iter()
            .map(|n| {
                format!(
                    "socket {}: {:.1} de {:.0} GB em uso",
                    n.id,
                    n.mem_used_kb() as f64 / 1048576.0,
                    n.mem_total_kb as f64 / 1048576.0
                )
            })
            .collect();
        (
            St::Ok,
            format!(
                "{:.1} de {:.0} GB em uso (sem contar o cache)\n{}",
                (t - a) as f64 / 1048576.0,
                t as f64 / 1048576.0,
                per_node.join("\n")
            ),
        )
    });
    r.t("Temperatura do processador", || {
        let t = crate::cpuctl::package_temps();
        if t.is_empty() {
            (
                St::Aviso,
                "nenhum sensor (k10temp/zenpower/coretemp) encontrado".into(),
            )
        } else {
            let v: Vec<String> = t
                .iter()
                .map(|(p, c)| format!("socket {p}: {c:.0} °C"))
                .collect();
            (St::Ok, v.join(", "))
        }
    });
    r.t("Frequência do processador", || {
        let b = "/sys/devices/system/cpu/cpu0/cpufreq";
        let Some(gov) = read(&format!("{b}/scaling_governor")) else {
            let mhz = crate::cpuctl::cur_mhz_all();
            if let Some(max) = mhz.values().max() {
                return (
                    St::Ok,
                    format!(
                        "{:.2} GHz agora (máx. entre os núcleos, pelo /proc/cpuinfo) · sem cpufreq: só leitura, o firmware não expõe controle (BC-250: use o bc250_smu_oc)",
                        *max as f64 / 1000.0
                    ),
                );
            }
            return (St::Aviso, "sem cpufreq (VM ou driver ausente)".into());
        };
        let drv = read(&format!("{b}/scaling_driver")).unwrap_or_default();
        let cur = read(&format!("{b}/scaling_cur_freq"))
            .and_then(|v| v.parse::<u64>().ok())
            .map(|k| format!("{:.2} GHz", k as f64 / 1e6))
            .unwrap_or_default();
        let epp = read(&format!("{b}/energy_performance_preference"))
            .map(|e| format!(" · EPP {e}"))
            .unwrap_or_default();
        (St::Ok, format!("{drv} · governador {gov} · {cur}{epp}"))
    });

    // ---------------------------------------------------------------- serviço
    r.group("Serviço");
    r.t("Serviço numapilot", || {
        match sh(&["systemctl", "is-active", "numapilot"], 10) {
            Some((true, _)) => (St::Ok, "ativo".into()),
            Some((_, o)) => (
                St::Falhou,
                format!(
                    "{} — ative com: sudo systemctl enable --now numapilot",
                    o.trim()
                ),
            ),
            None => (St::Pulou, "sem systemctl".into()),
        }
    });
    r.t("Conversa com o serviço", || {
        let t0 = Instant::now();
        match crate::ctl::send(&crate::ctl::Request::Ping) {
            Ok(_) => (
                St::Ok,
                format!("respondeu em {} ms", t0.elapsed().as_millis()),
            ),
            Err(e) => (
                St::Falhou,
                format!("{e} ({})", crate::ctl::sock_path().display()),
            ),
        }
    });
    r.t("Registro do serviço", || {
        let p = std::path::Path::new("/var/log/numapilot/daemon.log");
        if !p.exists() {
            return (
                St::Pulou,
                "ainda não existe (o serviço desta versão ainda não rodou)".into(),
            );
        }
        let t = crate::log::tail(p, 400);
        let errs: Vec<&String> = t.iter().filter(|l| l.contains(" ERRO ")).collect();
        let warns = t.iter().filter(|l| l.contains(" AVISO ")).count();
        if errs.is_empty() {
            (
                St::Ok,
                format!("{} linhas recentes, {warns} avisos, nenhum erro", t.len()),
            )
        } else {
            let last: Vec<String> = errs.iter().rev().take(5).map(|s| s.to_string()).collect();
            (
                St::Aviso,
                format!(
                    "{} erros e {warns} avisos nas últimas {} linhas; os mais recentes:\n{}",
                    errs.len(),
                    t.len(),
                    last.join("\n")
                ),
            )
        }
    });
    r.t("Erros do serviço no journal", || {
        match sh(
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
                    (St::Ok, "nenhum neste boot".into())
                } else {
                    (
                        St::Aviso,
                        format!(
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
            None => (St::Pulou, "sem journalctl".into()),
        }
    });
    r.t("Painel web e /metrics", || {
        let active = sh(&["systemctl", "--user", "is-active", "numapilot-web"], 10)
            .is_some_and(|(ok, _)| ok);
        if !active {
            return (
                St::Pulou,
                "painel não está ligado (Configurações → painel no celular)".into(),
            );
        }
        let Ok(tok) = crate::web::token() else {
            return (St::Aviso, "painel ligado, mas sem token".into());
        };
        let url = format!(
            "http://127.0.0.1:{}/metrics?t={}",
            crate::web::DEFAULT_PORT,
            tok.trim()
        );
        match sh(&["curl", "-s", "--noproxy", "*", "-m", "8", &url], 10) {
            Some((true, o)) if o.contains("numapilot_") => (
                St::Ok,
                format!(
                    "respondeu ({} métricas)",
                    o.lines().filter(|l| !l.starts_with('#')).count()
                ),
            ),
            Some((_, o)) => (
                St::Falhou,
                format!("não respondeu direito: {}", o.lines().next().unwrap_or("")),
            ),
            None => (St::Pulou, "sem curl".into()),
        }
    });

    // ---------------------------------------------------------------- configuração
    r.group("Configuração");
    r.t("Configuração principal", || {
        (St::Ok, format!("{} regras", cfg.rules.len()))
    });
    r.t("Máquina (modo, energia, notebook)", || {
        let m = crate::machine::Machine::load();
        match m.validate() {
            Ok(()) => (
                St::Ok,
                format!(
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
                St::Falhou,
                format!("{}: {e}", crate::machine::Machine::path().display()),
            ),
        }
    });
    r.t("Escolhas salvas", || {
        let p = crate::profiles::Profiles::load();
        let bad: Vec<String> = p
            .map
            .iter()
            .filter_map(|(k, v)| v.validate().err().map(|e| format!("{k}: {e}")))
            .collect();
        if bad.is_empty() {
            (St::Ok, format!("{} programas", p.map.len()))
        } else {
            (
                St::Falhou,
                format!("{} inválidas:\n{}", bad.len(), bad.join("\n")),
            )
        }
    });
    r.t("Outros PCs e avisos", || {
        let peers = crate::peers::load();
        let n = crate::notify::Notify::load();
        let nt = if n.enabled {
            match n.validate() {
                Ok(()) => "avisos no celular ligados".to_string(),
                Err(e) => return (St::Falhou, format!("avisos no celular: {e}")),
            }
        } else {
            "avisos no celular desligados".into()
        };
        (St::Ok, format!("{} PCs ligados · {nt}", peers.len()))
    });
    r.t("Receitas e placas especiais", || {
        let rc = crate::recipes::load();
        let boards: Vec<String> = rc
            .boards(&topo)
            .iter()
            .map(|(b, g)| format!("{} ({})", b.name, g.alias))
            .collect();
        (
            St::Ok,
            if boards.is_empty() {
                "nenhuma placa especial".into()
            } else {
                format!("placa especial: {}", boards.join(", "))
            },
        )
    });

    // ---------------------------------------------------------------- GPUs
    r.group("Placas de vídeo");
    if topo.gpus.is_empty() {
        r.t("GPUs", || (St::Aviso, "nenhuma GPU encontrada".into()));
    }
    let vk = if which("vulkaninfo") {
        sh(&["vulkaninfo", "--summary"], 30).map(|(_, o)| o.to_lowercase())
    } else {
        None
    };
    for g in topo.gpus.clone() {
        let a = g.alias.clone();
        r.t(&format!("{a}: sensores"), || {
            let s = crate::gpuctl::read_sensors(&g.pci);
            let mut v = vec![];
            if let Some(t) = s.junction_c.or(s.edge_c) {
                v.push(format!("{t:.0} °C"));
            }
            if let Some(w) = s.power_w {
                v.push(format!("{w:.0} W"));
            }
            if let Some(f) = s.fan_rpm {
                v.push(format!("ventoinha {f} RPM"));
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
                return (St::Aviso, format!("{clk}{why}"));
            }
            if let Some(c) = s.clock_mhz {
                v.push(format!("{c:.0} MHz"));
            }
            if g.is_nvidia() && crate::nvctl::asleep(&g.pci) {
                return (
                    St::Ok,
                    "dormindo (economia de energia): sensores não lidos para não acordar".into(),
                );
            }
            if v.is_empty() && crate::gpuctl::hwmon_dir(&g.pci).is_some() {
                (
                    St::Aviso,
                    "o firmware manda valores impossíveis (ex.: 0 °C, mais de 1000 W): ignorados"
                        .into(),
                )
            } else if v.is_empty() {
                (
                    St::Falhou,
                    "não consegui ler temperatura nem potência".into(),
                )
            } else {
                (St::Ok, v.join(" · "))
            }
        });
        r.t(&format!("{a}: uso e memória"), || {
            let b = g.busy_percent();
            let m = g.vram_used_total();
            if b.is_none() && m.is_none() {
                return (St::Aviso, "uso e VRAM não disponíveis".into());
            }
            (
                St::Ok,
                format!(
                    "{}{}",
                    b.map(|b| format!("{b}% de uso")).unwrap_or_default(),
                    m.map(|(u, t)| format!(
                        " · VRAM {:.1}/{:.1} GB",
                        u as f64 / 1073741824.0,
                        t as f64 / 1073741824.0
                    ))
                    .unwrap_or_default()
                ),
            )
        });
        r.t(&format!("{a}: acesso para programas"), || {
            let Some(rn) = g.render.clone() else {
                return (St::Pulou, "sem nó de render".into());
            };
            let p = format!("/dev/dri/{rn}");
            match std::fs::OpenOptions::new().read(true).write(true).open(&p) {
                Ok(_) => (St::Ok, format!("{p} abre")),
                Err(e) => (
                    St::Falhou,
                    format!("{p}: {e} (seu usuário precisa estar nos grupos render e video)"),
                ),
            }
        });
        r.t(&format!("{a}: controle"), || {
            if g.is_nvidia() {
                match crate::nvctl::get(&g.pci) {
                    Some(n) => {
                        let nv = crate::nvml::offsets(&g.pci)
                            .map(|_| " · NVML ok (deslocamento de clock)")
                            .unwrap_or(" · NVML indisponível (sem deslocamento de clock)");
                        (
                            St::Ok,
                            format!(
                                "nvidia-smi ok · driver {}{nv}{}",
                                crate::nvctl::driver_version().unwrap_or_default(),
                                n.power_limit_w
                                    .map(|w| format!(" · limite {w:.0} W"))
                                    .unwrap_or_default()
                            ),
                        )
                    }
                    None if crate::nvctl::asleep(&g.pci) => (St::Ok, "dormindo".into()),
                    None => (
                        St::Falhou,
                        "nvidia-smi não respondeu para esta placa".into(),
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
                            "clock/voltagem disponíveis (OverDrive ligado)"
                        } else {
                            "clock/voltagem indisponíveis (OverDrive desligado)"
                        },
                        pl.map(|w| format!(" · limite {w:.0} W"))
                            .unwrap_or_default()
                    ),
                )
            }
        });
        r.t(&format!("{a}: Vulkan"), || {
            let Some(vk) = &vk else {
                return (
                    St::Pulou,
                    "vulkaninfo não instalado (pacote vulkan-tools)".into(),
                );
            };
            if g.is_nvidia() && crate::nvctl::asleep(&g.pci) {
                return (St::Pulou, "dormindo".into());
            }
            let id = format!("0x{:04x}", g.device);
            if vk.contains(&id) {
                (St::Ok, format!("aparece no Vulkan ({id})"))
            } else {
                (St::Aviso, format!("não aparece no vulkaninfo ({id})"))
            }
        });
    }

    r.group("Saúde do hardware");
    r.t("Coleta do serviço", || {
        let Some(h) = crate::health::load() else {
            // logo depois do boot (ou de reiniciar o serviço) ainda não deu tempo: não é defeito
            if let Some(s) = service_uptime_secs().filter(|&s| s < 180) {
                return (
                    St::Pulou,
                    format!(
                        "o serviço iniciou há {s} s e coleta 1 min depois: rode o autoteste de novo daqui a pouco"
                    ),
                );
            }
            return (
                St::Aviso,
                "ainda não coletado (o serviço coleta 1 min depois de iniciar)".into(),
            );
        };
        let age = crate::history::now().saturating_sub(h.time) / 60;
        let mut lines = vec![];
        for d in &h.disks {
            lines.push(format!(
                "disco {} {}: SMART {}{}",
                d.dev,
                d.model,
                match d.passed {
                    Some(true) => "ok",
                    Some(false) => "FALHOU",
                    None => "?",
                },
                d.temp_c
                    .map(|t| format!(" · {t:.0} °C"))
                    .unwrap_or_default()
            ));
        }
        for e in &h.disk_errors {
            lines.push(format!("disco não lido: {e}"));
        }
        for b in &h.batteries {
            lines.push(format!(
                "bateria {}: {}",
                b.name,
                b.health_pct
                    .map(|p| format!("{p:.0}% da original"))
                    .unwrap_or_default()
            ));
        }
        let st = if !h.smartctl || h.disks.is_empty() {
            St::Aviso
        } else {
            St::Ok
        };
        (
            st,
            format!(
                "coletado há {age} min · {} disco(s){}\n{}",
                h.disks.len(),
                if h.smartctl {
                    ""
                } else {
                    " (smartctl não encontrado pelo serviço)"
                },
                lines.join("\n")
            ),
        )
    });

    // ---------------------------------------------------------------- notebook
    if crate::notebook::is_laptop() {
        r.group("Notebook");
        r.t("Bateria e tomada", || {
            let b = crate::notebook::battery_info();
            let ac = match crate::notebook::on_ac() {
                Some(true) => "na tomada",
                Some(false) => "na bateria",
                None => "tomada desconhecida",
            };
            (St::Ok, format!("{ac} · {} bateria(s)", b.len()))
        });
        r.t(
            "Perfil de energia",
            || match crate::notebook::platform_profile() {
                Some(p) => (
                    St::Ok,
                    format!(
                        "{p} (opções: {})",
                        crate::notebook::platform_profiles().join(", ")
                    ),
                ),
                None => (
                    St::Aviso,
                    format!(
                        "nenhum perfil de energia disponível\n{}",
                        crate::notebook::acer_diagnose().join("\n")
                    ),
                ),
            },
        );
        r.t("NVIDIA híbrida", || {
            let p = crate::learned::run_dir().join("dgpu.json");
            match std::fs::read_to_string(&p) {
                Ok(t) => match serde_json::from_str::<crate::hybrid::State>(&t) {
                    Ok(s) => (
                        if s.last_error.is_some() {
                            St::Aviso
                        } else {
                            St::Ok
                        },
                        format!(
                            "modo {} · {} · {}{}",
                            s.mode,
                            if s.removed {
                                "desligada (tirada)"
                            } else {
                                "presente"
                            },
                            if s.blocked_by.is_empty() {
                                "nada segurando".to_string()
                            } else {
                                format!("segurada por {}", s.blocked_by.join(", "))
                            },
                            s.last_error
                                .map(|e| format!("\núltimo erro: {e}"))
                                .unwrap_or_default()
                        ),
                    ),
                    Err(e) => (St::Falhou, format!("{}: {e}", p.display())),
                },
                Err(_) => (St::Pulou, "sem NVIDIA híbrida ou serviço parado".into()),
            }
        });
        r.t("Acer", || {
            if !crate::acer::is_acer() {
                return (St::Pulou, "não é Acer".into());
            }
            let k = crate::acer::knobs();
            let have: Vec<String> = k
                .iter()
                .filter_map(|(n, v)| v.as_ref().map(|v| format!("{n}={v}")))
                .collect();
            if have.is_empty() {
                (
                    St::Aviso,
                    format!(
                        "nenhum controle Acer (predator_v4/Linuwu-Sense) disponível\n{}",
                        crate::notebook::acer_diagnose().join("\n")
                    ),
                )
            } else {
                (St::Ok, have.join(" · "))
            }
        });
        r.t("Limites do Ryzen (ryzenadj)", || {
            if !crate::ryzen::available() {
                return (
                    St::Pulou,
                    "ryzenadj não instalado ou não é Ryzen de notebook".into(),
                );
            }
            if unsafe { libc::geteuid() } != 0 {
                return (St::Pulou, "precisa de root para ler (o serviço lê)".into());
            }
            match crate::ryzen::info() {
                Ok(m) => (St::Ok, format!("{} valores lidos", m.len())),
                Err(e) => (St::Falhou, e),
            }
        });
    }

    // ---------------------------------------------------------------- placas especiais
    let rc = crate::recipes::load();
    let boards: Vec<String> = rc
        .boards(&topo)
        .iter()
        .map(|(b, _)| b.name.clone())
        .collect();
    if !boards.is_empty() {
        r.group("Placa especial");
        r.t("Núcleos ativos", || {
            (
                St::Ok,
                format!(
                    "{} núcleos físicos online ({} threads)",
                    crate::boards::active_cores(),
                    std::thread::available_parallelism().map_or(0, |n| n.get())
                ),
            )
        });
        r.t("CUs da GPU", || match crate::boards::active_cu() {
            Some(c) => (St::Ok, format!("{c} CUs ativas")),
            None => (
                St::Aviso,
                "não achei active_cu_number no log do kernel nem no KFD".into(),
            ),
        });
        r.t("Memória da GPU (TTM)", || {
            let now = crate::bootcfg::ttm_now();
            let saved = crate::bootcfg::saved_ttm();
            let gib = |p: u64| p as f64 * 4096.0 / 1073741824.0;
            (
                St::Ok,
                format!(
                    "agora {} · salvo {}",
                    now.map(|p| format!("{:.1} GiB", gib(p)))
                        .unwrap_or("padrão".into()),
                    saved
                        .map(|p| format!("{:.1} GiB", gib(p)))
                        .unwrap_or("nada".into())
                ),
            )
        });
    }

    // ---------------------------------------------------------------- diagnóstico
    r.group("Diagnóstico (numapilot doctor)");
    r.t("Verificações do doctor", || {
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
        let head = format!(
            "{} verificações: {} problemas, {} avisos",
            checks.len(),
            fails.len(),
            warns.len()
        );
        let body = fails
            .iter()
            .map(|s| format!("problema: {s}"))
            .chain(warns.iter().map(|s| format!("aviso: {s}")))
            .collect::<Vec<_>>()
            .join("\n");
        let st = if !fails.is_empty() {
            St::Falhou
        } else if !warns.is_empty() {
            St::Aviso
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
    r.t("Programas auxiliares", || {
        let tools = [
            ("vkmark", "undervolt automático"),
            ("glmark2", "undervolt automático"),
            ("vulkaninfo", "teste do Vulkan"),
            ("smartctl", "saúde dos discos"),
            ("ethtool", "ligar pela rede"),
            ("sunshine", "streaming"),
            ("podman", "laboratório"),
            ("llama-server", "servidor de IA"),
            ("ryzenadj", "limites do Ryzen"),
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
            format!(
                "instalados: {}\nfaltando (só importa se for usar): {}",
                have.join(", "),
                if miss.is_empty() {
                    "nenhum".into()
                } else {
                    miss.join(", ")
                }
            ),
        )
    });

    // ---------------------------------------------------------------- comandos
    if cli {
        r.group("Comandos (só leitura)");
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
                                .unwrap_or("(sem saída)")
                                .chars()
                                .take(110)
                                .collect::<String>()
                        ),
                    ),
                    Some((false, o)) => (
                        St::Falhou,
                        format!(
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
                    None => (St::Falhou, "não consegui rodar".into()),
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

/// Relatório em texto (o que vai para o arquivo).
pub fn render(r: &Report) -> String {
    let mut s = format!(
        "# numapilot — autoteste\n\nversão {} · máquina {} · {}\n{}\n",
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

/// Há quantos segundos o numapilot.service está ativo (None = parado ou sem systemctl).
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
