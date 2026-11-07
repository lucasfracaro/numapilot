//! `numapilot doctor`: testa cada parte no hardware real e diz o que corrigir.

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
            "nós",
            Level::Ok,
            format!(
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
            "nós",
            Level::Fail,
            "2 sockets mas 1 nó NUMA".into(),
            Some("Na BIOS: desligue \"Node Interleaving\" (deixe NUMA ativo)"),
        );
    } else {
        add("NUMA", "nós", Level::Info, "1 nó NUMA".into(), None);
    }
    for w in topo.warnings() {
        add("NUMA", "aviso", Level::Warn, w, None);
    }
    let bal = std::fs::read_to_string("/proc/sys/kernel/numa_balancing").unwrap_or_default();
    add(
        "NUMA",
        "balanceamento automático do kernel",
        Level::Info,
        format!("numa_balancing = {}", bal.trim()),
        None,
    );
    let ctrl = std::fs::read_to_string("/sys/fs/cgroup/cgroup.controllers").unwrap_or_default();
    if ctrl.contains("cpuset") && ctrl.contains("memory") {
        add("Sistema", "cgroups v2", Level::Ok, ctrl.trim().into(), None);
    } else {
        add(
            "Sistema",
            "cgroups v2",
            Level::Fail,
            format!("controladores: '{}'", ctrl.trim()),
            Some(
                "O sistema precisa de cgroups v2 (padrão no CachyOS) para limites de memória e núcleos exclusivos",
            ),
        );
    }
    let sd = out(&["systemctl", "--version"], &[]).unwrap_or_default();
    add(
        "Sistema",
        "systemd",
        Level::Info,
        sd.lines().next().unwrap_or("?").into(),
        None,
    );
    let od = crate::bootcfg::overdrive_state();
    if od.active {
        add(
            "Sistema",
            "OverDrive (clock/voltagem)",
            Level::Ok,
            "ligado".into(),
            None,
        );
    } else if od.on_next_boot {
        add(
            "Sistema",
            "OverDrive (clock/voltagem)",
            Level::Info,
            "liga depois de reiniciar".into(),
            Some("Reinicie o computador"),
        );
    } else {
        add(
            "Sistema",
            "OverDrive (clock/voltagem)",
            Level::Info,
            "desligado".into(),
            Some(
                "Para undervolt/overclock: Placas de vídeo → \"Ativar OverDrive\" (ou sudo numapilot boot overdrive on) e reinicie",
            ),
        );
    }
    // ---- NVIDIA e notebook ----
    if topo.gpus.iter().any(|g| g.is_nvidia()) {
        let st = crate::driver::status(&topo);
        match (&st.loaded, &st.installed) {
            (Some(l), _) if st.needs_reboot => add(
                "NVIDIA",
                "driver",
                Level::Warn,
                format!(
                    "carregado {l}, instalado {}",
                    st.installed.clone().unwrap_or_default()
                ),
                Some("Reinicie para usar o driver instalado"),
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
                "usando o nouveau (sem desempenho para jogos)".into(),
                Some(
                    "Placas de vídeo → Instalar driver oficial (ou sudo numapilot driver install)",
                ),
            ),
            (None, _) => add(
                "NVIDIA",
                "driver",
                Level::Fail,
                "driver oficial não carregado".into(),
                Some(
                    "Placas de vídeo → Instalar driver oficial (ou sudo numapilot driver install)",
                ),
            ),
        }
        if let (Some(i), Some(a)) = (&st.installed, &st.available)
            && i != a
        {
            add(
                "NVIDIA",
                "atualização",
                Level::Info,
                format!("{i} → {a} disponível"),
                Some("Placas de vídeo → Atualizar (atualiza o sistema junto)"),
            );
        }
    }
    if crate::notebook::is_laptop() {
        let ac = crate::notebook::on_ac();
        add(
            "Notebook",
            "energia",
            Level::Ok,
            match ac {
                Some(true) => "na tomada".into(),
                Some(false) => "na bateria".into(),
                None => "?".into(),
            },
            None,
        );
        for g in topo.gpus.iter().filter(|g| !g.is_integrated()) {
            let pm = crate::notebook::runtime_pm(&g.pci).unwrap_or_default();
            if pm == "on" {
                add(
                    "Notebook",
                    &format!("{} dormir", g.alias),
                    Level::Info,
                    "a placa dedicada nunca dorme (gasta bateria)".into(),
                    Some("Máquina → Notebook → placa dedicada: dormir quando ninguém usa"),
                );
            } else {
                add(
                    "Notebook",
                    &format!("{} dormir", g.alias),
                    Level::Ok,
                    crate::notebook::dgpu_state(&g.pci),
                    None,
                );
            }
            if !crate::notebook::dgpu_state(&g.pci).starts_with("dormindo") {
                let mut users: Vec<String> = crate::notebook::dgpu_users(&g.pci).into_iter().map(|u| u.1).collect();
                users.sort();
                users.dedup();
                if !users.is_empty() {
                    add(
                        "Notebook",
                        &format!("{} acordada por", g.alias),
                        Level::Info,
                        users.join(", "),
                        Some("Abra esses programas na placa integrada para a dedicada dormir"),
                    );
                }
            }
        }
        if crate::ryzen::applicable() {
            if crate::ryzen::available() {
                match crate::ryzen::info() {
                    Ok(i) => {
                        let l = crate::ryzen::limits_from(&i);
                        add(
                            "Notebook",
                            "limites do Ryzen",
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
                    Err(e) => add("Notebook", "limites do Ryzen", Level::Info, e, Some("rode com sudo para ler (o ryzenadj precisa de root)")),
                }
            } else {
                add("Notebook", "limites do Ryzen", Level::Info, "ryzenadj não instalado".into(), Some("paru -S ryzenadj"));
            }
        }
        if crate::acer::is_acer() {
            if crate::acer::sense_dir().is_some() || crate::acer::kb_dir().is_some() {
                add("Notebook", "Acer (Linuwu-Sense)", Level::Ok, format!("{} ajustes disponíveis", crate::acer::knobs().len()), None);
            } else {
                add(
                    "Notebook",
                    "Acer (Linuwu-Sense)",
                    Level::Info,
                    "módulo não carregado: sem ventoinha manual, limite de 80% e RGB".into(),
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
                "limite de carga",
                Level::Info,
                "o notebook não permite pelo Linux".into(),
                None,
            );
        }
    }
    // ---- placas especiais e receitas da comunidade ----
    let rec = crate::recipes::load();
    for (b, g) in rec.boards(&topo) {
        let area = b.name.clone();
        let k = crate::recipes::running_kernel();
        if crate::recipes::kernel_in(&k, &b.bad_kernels) {
            add(
                &area,
                "kernel",
                Level::Fail,
                format!("{k} está na lista de kernels que quebram esta placa"),
                Some("Instale outra versão (ex.: linux-cachyos-lts) e escolha no boot"),
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
                    format!("{m} (precisa de {min} ou mais novo)"),
                    Some("sudo pacman -Syu"),
                ),
                None => {}
            }
        }
        if !b.governor_services.is_empty() {
            let sv = crate::boards::services(b);
            match sv.iter().find(|s| s.active) {
                Some(s) => add(
                    &area,
                    "governor de clock",
                    Level::Ok,
                    format!("{} rodando", s.name),
                    None,
                ),
                None => add(
                    &area,
                    "governor de clock",
                    Level::Warn,
                    "nenhum rodando: a GPU fica com clock fixo".into(),
                    Some("Placas de vídeo → instalar/ligar o governor"),
                ),
            }
        }
        let cmd = crate::cmdline::running();
        for p in &b.kernel_params {
            if !cmd.split_whitespace().any(|t| t == p) {
                add(
                    &area,
                    &format!("parâmetro {p}"),
                    Level::Info,
                    "recomendado pela comunidade".into(),
                    Some("Máquina → Parâmetros do kernel"),
                );
            }
        }
        let _ = g;
        if let Some((stock, full)) = b.cores {
            let n = crate::boards::active_cores();
            if n >= full {
                add(
                    &area,
                    "núcleos da CPU",
                    Level::Ok,
                    format!("{n} de {full} (desbloqueados)"),
                    None,
                );
            } else if n > 0 {
                add(
                    &area,
                    "núcleos da CPU",
                    Level::Info,
                    format!("{n} de {full} ativos (de fábrica: {stock})"),
                    Some(
                        "Dá para desbloquear pelos programas da comunidade (Placas de vídeo → ajustes da comunidade)",
                    ),
                );
            }
        }
        if let Some((stock, full)) = b.cus {
            match crate::boards::active_cu() {
                Some(n) if n >= full => add(
                    &area,
                    "CUs da GPU",
                    Level::Ok,
                    format!("{n} de {full} (desbloqueados)"),
                    None,
                ),
                Some(n) => add(
                    &area,
                    "CUs da GPU",
                    Level::Info,
                    format!("{n} de {full} ativos no boot (de fábrica: {stock})"),
                    Some(
                        "Desbloqueio sem trocar o kernel pelos programas da comunidade (Placas de vídeo → ajustes da comunidade)",
                    ),
                ),
                None => {}
            }
        }
        for a in &b.apps {
            add(
                &area,
                &a.name,
                if a.installed_bin().is_some() {
                    Level::Ok
                } else {
                    Level::Info
                },
                if a.installed_bin().is_some() {
                    "instalado".into()
                } else {
                    format!("não instalado: {}", a.about)
                },
                a.aur.as_ref().map(|p| format!("paru -S {p}")).as_deref(),
            );
        }
        for n in &b.notes {
            add(&area, "dica", Level::Info, n.clone(), None);
        }
    }
    for n in rec.cpu_notes(&crate::recipes::cpu_model()) {
        add("Processador", "dica", Level::Info, n, None);
    }
    // ---- processador ----
    let unc = crate::cpuctl::uncores();
    if unc.is_empty() {
        add(
            "Processador",
            "uncore",
            Level::Info,
            "controle indisponível".into(),
            Some(
                "sudo modprobe intel_uncore_frequency (e echo intel_uncore_frequency | sudo tee /etc/modules-load.d/uncore.conf)",
            ),
        );
    } else {
        add(
            "Processador",
            "uncore",
            Level::Ok,
            format!("{} socket(s) controláveis", unc.len()),
            None,
        );
    }
    let rapl = crate::cpuctl::rapl_zones();
    add(
        "Processador",
        "potência (RAPL)",
        if rapl.is_empty() {
            Level::Info
        } else {
            Level::Ok
        },
        if rapl.is_empty() {
            "indisponível".into()
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
        "Processador",
        "estados de economia",
        if cs.is_empty() {
            Level::Info
        } else {
            Level::Ok
        },
        if cs.is_empty() {
            "cpuidle indisponível".into()
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
            "Processador",
            "escalonador sched_ext",
            Level::Ok,
            if cur.is_empty() {
                "padrão do kernel".into()
            } else {
                cur
            },
            None,
        ),
        None => add(
            "Processador",
            "escalonador sched_ext",
            Level::Info,
            "scx_loader não instalado".into(),
            Some("sudo pacman -S scx-scheds scx-tools"),
        ),
    }
    let vulns = crate::cmdline::vulnerabilities();
    let mitig: Vec<&str> = crate::cmdline::relevant_mitigations(&vulns)
        .iter()
        .map(|m| m.title)
        .collect();
    if !mitig.is_empty() {
        add(
            "Processador",
            "mitigações ativas",
            Level::Info,
            mitig.join(", "),
            Some("Máquina → Mitigações do processador"),
        );
    }
    match crate::cmdline::detect() {
        Some(l) => add(
            "Sistema",
            "bootloader",
            Level::Ok,
            format!("{} ({})", l.name(), l.path.display()),
            None,
        ),
        None => add(
            "Sistema",
            "bootloader",
            Level::Info,
            "não reconhecido: parâmetros do kernel só à mão".into(),
            None,
        ),
    }
    let bf = crate::fans::list();
    let ipmi = crate::fans::ipmi_fans();
    add(
        "Sistema",
        "ventoinhas da placa-mãe",
        if bf.is_empty() && ipmi.is_empty() {
            Level::Info
        } else {
            Level::Ok
        },
        format!(
            "{} controláveis (hwmon), {} pelo IPMI",
            bf.len(),
            ipmi.len()
        ),
        if bf.is_empty() {
            Some(
                "sudo sensors-detect (pacote lm_sensors) diz qual driver carregar, ex.: sudo modprobe nct6775",
            )
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
            "Sistema",
            "irqbalance",
            Level::Warn,
            "rodando: pode desfazer as interrupções das GPUs".into(),
            Some("sudo systemctl disable --now irqbalance"),
        );
    }

    // ---- serviço ----
    let fresh = std::fs::metadata(crate::daemon::state_path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| std::time::SystemTime::now().duration_since(t).ok())
        .is_some_and(|d| d.as_secs() < 5);
    if fresh {
        add(
            "Serviço",
            "numapilot daemon",
            Level::Ok,
            "rodando".into(),
            None,
        );
    } else {
        add(
            "Serviço",
            "numapilot daemon",
            Level::Fail,
            "parado".into(),
            Some("sudo systemctl enable --now numapilot"),
        );
    }
    match crate::ctl::send(&crate::ctl::Request::Ping) {
        Ok(r) if r.ok => add(
            "Serviço",
            "socket de controle",
            Level::Ok,
            "responde".into(),
            None,
        ),
        _ => add(
            "Serviço",
            "socket de controle",
            Level::Fail,
            "sem resposta".into(),
            Some("Veja: journalctl -u numapilot -n 50"),
        ),
    }
    if let Some(st) = std::fs::read_to_string(status_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Status>(&s).ok())
    {
        add(
            "Serviço",
            "como root",
            if st.root { Level::Ok } else { Level::Fail },
            format!("{}", st.root),
            None,
        );
        add(
            "Serviço",
            "detecção instantânea",
            if st.proc_connector {
                Level::Ok
            } else {
                Level::Warn
            },
            if st.proc_connector {
                "proc connector ativo".into()
            } else {
                "só por varredura".into()
            },
            None,
        );
        match &st.cgroup_last_error {
            Some(e) => add(
                "Serviço",
                "grupos de controle",
                Level::Fail,
                e.clone(),
                Some("Me mande este relatório"),
            ),
            None => add(
                "Serviço",
                "grupos de controle",
                Level::Ok,
                format!("{} aplicados sem erro", st.cgroup_ok_count),
                None,
            ),
        }
        for e in &st.od_errors {
            add("Serviço", "clock/voltagem", Level::Fail, e.clone(), None);
        }
    }

    // ---- GPUs ----
    let have_vk = which("vulkaninfo");
    let have_gl = which("glxinfo");
    for g in &topo.gpus {
        let a = format!("GPU {}", g.alias);
        add(
            &a,
            "identificação",
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
                "desconhecido".into(),
                Some("Defina em [gpu_node] no /etc/numapilot.toml"),
            ),
            None => {}
        }
        add(
            &a,
            "monitor",
            Level::Info,
            if g.display_connected {
                "ligado nesta placa".into()
            } else {
                "nenhum".into()
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
                    "não aparece no KFD".into(),
                    Some("Verifique se o módulo amdgpu carregou com KFD (dmesg | grep kfd)"),
                ),
            }
        }
        let s = gpuctl::read_sensors(&g.pci);
        add(
            &a,
            "ventoinha",
            if s.fan_controllable {
                Level::Ok
            } else {
                Level::Warn
            },
            if s.fan_controllable {
                format!("controlável ({} RPM)", s.fan_rpm.unwrap_or(0))
            } else {
                "sem controle pelo driver".into()
            },
            None,
        );
        add(
            &a,
            "limite de potência",
            if s.power_cap_max_w.is_some() {
                Level::Ok
            } else {
                Level::Warn
            },
            s.power_cap_w
                .map(|w| format!("{w:.0} W"))
                .unwrap_or_else(|| "indisponível".into()),
            None,
        );
        match gpuctl::read_od(&g.pci) {
            Some(od) => add(
                &a,
                "clock/voltagem (OD)",
                Level::Ok,
                format!(
                    "GPU até {:?} MHz, curva {} pontos",
                    od.range_sclk.map(|r| r.1),
                    od.curve.len()
                ),
                None,
            ),
            None => add(
                &a,
                "clock/voltagem (OD)",
                Level::Info,
                "indisponível".into(),
                Some(
                    "sudo numapilot boot overdrive on (ou o botão na aba Placas de vídeo) e reinicie",
                ),
            ),
        }
        if let Some(l) = gpuctl::pcie_link(&g.pci) {
            let d = format!(
                "Gen{} x{} (máx. Gen{} x{})",
                l.generation, l.width, l.max_gen, l.max_width
            );
            if l.width < l.max_width {
                add(
                    &a,
                    "link PCIe",
                    Level::Warn,
                    d,
                    Some(
                        "Link mais estreito que o possível: confira o slot (alguns são x8 ou dividem linhas) e a BIOS",
                    ),
                );
            } else {
                add(
                    &a,
                    "link PCIe",
                    Level::Ok,
                    format!("{d}; a geração cai com a placa parada"),
                    None,
                );
            }
        }
        match gpuctl::bar_info(&g.pci) {
            Some((vis, tot)) if vis * 10 >= tot * 9 => add(
                &a,
                "Resizable BAR",
                Level::Ok,
                format!("ativo ({} MB visíveis)", vis >> 20),
                None,
            ),
            Some((vis, tot)) => add(
                &a,
                "Resizable BAR",
                Level::Info,
                format!("inativo ({} de {} MB visíveis)", vis >> 20, tot >> 20),
                Some(
                    "Na BIOS: \"Above 4G Decoding\" e \"Resizable BAR\" (se existir). Em placas X99/C612 antigas, tente o parâmetro pci=realloc",
                ),
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
                    format!(
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
                    format!("{} dispositivos visíveis", names.len()),
                    Some("A seleção de GPU do Mesa não funcionou: me mande o relatório"),
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

    // ---- ROCm / permissões ----
    let groups = out(&["id", "-nG"], &[]).unwrap_or_default();
    for gname in ["render", "video"] {
        if !groups.split_whitespace().any(|x| x == gname) {
            add(
                "ROCm",
                &format!("grupo {gname}"),
                Level::Warn,
                "usuário fora do grupo".into(),
                Some("sudo usermod -aG render,video $USER (e entre de novo na sessão)"),
            );
        }
    }
    if std::path::Path::new("/dev/kfd").exists() {
        add("ROCm", "/dev/kfd", Level::Ok, "existe".into(), None);
    } else {
        add("ROCm", "/dev/kfd", Level::Fail, "ausente".into(), None);
    }

    // ---- ferramentas ----
    for (bin, why, pkg) in [
        ("podman", "laboratório, servidor de IA", "podman"),
        ("mangohud", "otimizador de FPS", "mangohud"),
        ("gamescope", "gamescope por jogo", "gamescope"),
        ("vainfo", "aba Streaming", "libva-utils"),
        ("steam", "jogos", "steam"),
    ] {
        if which(bin) {
            add("Ferramentas", bin, Level::Ok, "instalado".into(), None);
        } else {
            add(
                "Ferramentas",
                bin,
                Level::Info,
                format!("não instalado ({why})"),
                Some(&format!("sudo pacman -S {pkg}")),
            );
        }
    }
    // vulkaninfo
    if !have_vk {
        add(
            "Ferramentas",
            "vulkaninfo",
            Level::Info,
            "não instalado: não deu para testar a seleção de GPU no Vulkan".into(),
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
                "jogos com numapilot",
                if with == games.len() && !games.is_empty() {
                    Level::Ok
                } else {
                    Level::Info
                },
                format!("{with} de {} jogos instalados", games.len()),
                Some("Interface → Programas → \"Aplicar em todos os jogos da Steam\""),
            );
        }
        None => add(
            "Steam",
            "configuração",
            Level::Info,
            "Steam não encontrada".into(),
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
            Level::Warn => "[aviso]",
            Level::Fail => "[FALHA]",
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
    println!("\nResumo: {fails} falha(s), {warns} aviso(s).");
}

/// Relatório completo para enviar (diagnóstico + topologia + sistema).
pub fn report(checks: &[Check]) -> String {
    let mut s = String::from("# numapilot — relatório de diagnóstico\n\n");
    s.push_str(&format!("versão: {}\n", env!("CARGO_PKG_VERSION")));
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
        ("lspci (árvore)", &["lspci", "-tv"]),
        (
            "lspci (GPUs)",
            &["sh", "-c", "lspci -nnk | grep -A3 -Ei 'vga|display|3d'"],
        ),
        ("numapilot topo", &["numapilot", "topo"]),
        ("gfx906-rocm doctor", &["gfx906-rocm", "doctor"]),
        (
            "serviço (log)",
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
        s.push_str(&out(argv, &[]).unwrap_or_else(|| "(indisponível)\n".into()));
    }
    s
}
