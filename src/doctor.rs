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
            crate::i18n::tr("nós"),
            Level::Ok,
            crate::tf!(
                "{} nós, {} sockets, {} CPUs",
                "{} nodes, {} sockets, {} CPUs",
                topo.nodes.len(),
                sockets.len(),
                topo.all_cpus().len()
            ),
            None,
        );
    } else if sockets.len() > 1 {
        add(
            "NUMA",
            crate::i18n::tr("nós"),
            Level::Fail,
            crate::i18n::tr("2 sockets mas 1 nó NUMA").into(),
            Some(crate::i18n::tr(
                "Na BIOS: desligue \"Node Interleaving\" (deixe NUMA ativo)",
            )),
        );
    } else {
        add(
            "NUMA",
            crate::i18n::tr("nós"),
            Level::Info,
            crate::i18n::tr("1 nó NUMA").into(),
            None,
        );
    }
    for w in topo.warnings() {
        add("NUMA", "aviso", Level::Warn, w, None);
    }
    let bal = std::fs::read_to_string("/proc/sys/kernel/numa_balancing").unwrap_or_default();
    add(
        "NUMA",
        crate::i18n::tr("balanceamento automático do kernel"),
        Level::Info,
        format!("numa_balancing = {}", bal.trim()),
        None,
    );
    let ctrl = std::fs::read_to_string("/sys/fs/cgroup/cgroup.controllers").unwrap_or_default();
    if ctrl.contains("cpuset") && ctrl.contains("memory") {
        add(
            crate::i18n::tr("Sistema"),
            crate::i18n::tr("cgroups v2"),
            Level::Ok,
            ctrl.trim().into(),
            None,
        );
    } else {
        add(
            crate::i18n::tr("Sistema"),
            crate::i18n::tr("cgroups v2"),
            Level::Fail,
            crate::tf!("controladores: '{}'", "controllers: '{}'", ctrl.trim()),
            Some(crate::i18n::tr(
                "O sistema precisa de cgroups v2 (padrão no CachyOS) para limites de memória e núcleos exclusivos",
            )),
        );
    }
    let sd = out(&["systemctl", "--version"], &[]).unwrap_or_default();
    add(
        crate::i18n::tr("Sistema"),
        "systemd",
        Level::Info,
        sd.lines().next().unwrap_or("?").into(),
        None,
    );
    let od = crate::bootcfg::overdrive_state();
    if od.active {
        add(
            crate::i18n::tr("Sistema"),
            crate::i18n::tr("OverDrive (clock/voltagem)"),
            Level::Ok,
            crate::i18n::tr("ligado").into(),
            None,
        );
    } else if od.on_next_boot {
        add(
            crate::i18n::tr("Sistema"),
            crate::i18n::tr("OverDrive (clock/voltagem)"),
            Level::Info,
            crate::i18n::tr("liga depois de reiniciar").into(),
            Some(crate::i18n::tr("Reinicie o computador")),
        );
    } else {
        add(
            crate::i18n::tr("Sistema"),
            crate::i18n::tr("OverDrive (clock/voltagem)"),
            Level::Info,
            crate::i18n::tr("desligado").into(),
            Some(crate::i18n::tr(
                "Para undervolt/overclock: Placas de vídeo → \"Ativar OverDrive\" (ou sudo numapilot boot overdrive on) e reinicie",
            )),
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
                crate::tf!(
                    "carregado {l}, instalado {}",
                    "loaded {l}, installed {}",
                    st.installed.clone().unwrap_or_default()
                ),
                Some(crate::i18n::tr("Reinicie para usar o driver instalado")),
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
                crate::i18n::tr("usando o nouveau (sem desempenho para jogos)").into(),
                Some(crate::i18n::tr(
                    "Placas de vídeo → Instalar driver oficial (ou sudo numapilot driver install)",
                )),
            ),
            (None, _) => add(
                "NVIDIA",
                "driver",
                Level::Fail,
                crate::i18n::tr("driver oficial não carregado").into(),
                Some(crate::i18n::tr(
                    "Placas de vídeo → Instalar driver oficial (ou sudo numapilot driver install)",
                )),
            ),
        }
        if let (Some(i), Some(a)) = (&st.installed, &st.available)
            && i != a
        {
            add(
                "NVIDIA",
                crate::i18n::tr("atualização"),
                Level::Info,
                crate::tf!("{i} → {a} disponível", "{i} → {a} available"),
                Some(crate::i18n::tr(
                    "Placas de vídeo → Atualizar (atualiza o sistema junto)",
                )),
            );
        }
    }
    if crate::notebook::is_laptop() {
        let ac = crate::notebook::on_ac();
        add(
            "Notebook",
            crate::i18n::tr("energia"),
            Level::Ok,
            match ac {
                Some(true) => crate::i18n::tr("na tomada").into(),
                Some(false) => crate::i18n::tr("na bateria").into(),
                None => "?".into(),
            },
            None,
        );
        for g in topo.gpus.iter().filter(|g| !g.is_integrated()) {
            let pm = crate::notebook::runtime_pm(&g.pci).unwrap_or_default();
            if pm == "on" {
                add(
                    "Notebook",
                    &crate::tf!("{} dormir", "{} sleep", g.alias),
                    Level::Info,
                    crate::i18n::tr("a placa dedicada nunca dorme (gasta bateria)").into(),
                    Some(crate::i18n::tr(
                        "Máquina → Notebook → placa dedicada: dormir quando ninguém usa",
                    )),
                );
            } else {
                add(
                    "Notebook",
                    &crate::tf!("{} dormir", "{} sleep", g.alias),
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
                        &crate::tf!("{} acordada por", "{} woken by", g.alias),
                        Level::Info,
                        users.join(", "),
                        Some(crate::i18n::tr(
                            "Abra esses programas na placa integrada para a dedicada dormir",
                        )),
                    );
                    for u in &users {
                        if let Some(h) = crate::notebook::holder_hint(u) {
                            add(
                                "Notebook",
                                u,
                                Level::Info,
                                crate::i18n::tr("mantém a NVIDIA acordada").into(),
                                Some(h),
                            );
                        }
                    }
                    // KDE: o KWin (e o Xwayland) abrem todas as placas; dá para limitar à integrada
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
                            crate::tf!("echo 'KWIN_DRM_DEVICES=/dev/dri/{c}' | sudo tee -a /etc/environment e entre de novo (se o número da placa mudar após atualizar, refaça)", "echo 'KWIN_DRM_DEVICES=/dev/dri/{c}' | sudo tee -a /etc/environment and log in again (if the card number changes after an update, redo it)")
                        });
                        add(
                            "Notebook",
                            crate::i18n::tr("desktop prendendo a NVIDIA"),
                            Level::Info,
                            crate::i18n::tr("o compositor (KWin/Xwayland) abriu a placa dedicada")
                                .into(),
                            fix.as_deref(),
                        );
                    }
                }
            }
        }
        // acer_wmi com predator_v4=1: perfis (incluindo Turbo) e leitura das ventoinhas
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
                        "ligado: perfis de energia e ventoinhas pelo kernel (o numapilot usa)",
                    )
                    .into(),
                    None,
                );
            }
        }
        // Programas que disputam ventoinha/energia com o numapilot
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
                crate::i18n::tr("outros controladores rodando"),
                Level::Warn,
                rivals.join(", "),
                Some(crate::i18n::tr(
                    "eles mexem nas mesmas ventoinhas/perfis: feche e tire da inicialização",
                )),
            );
        }
        if crate::util::read_trim(crate::util::sys("module/ec_sys/parameters/write_support"))
            .as_deref()
            == Some("Y")
        {
            add(
                "Notebook",
                crate::i18n::tr("escrita no controlador (ec_sys)"),
                Level::Info,
                crate::i18n::tr(
                    "ligada (usada por programas tipo NitroSense que escrevem direto no EC)",
                )
                .into(),
                Some(crate::i18n::tr(
                    "se não usa mais esses programas, remova ec_sys write_support=1 do /etc/modprobe.d",
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
                    crate::i18n::tr("nenhuma").to_string()
                } else {
                    v.join(", ")
                }
            };
            add(
                "Notebook",
                crate::i18n::tr("telas"),
                Level::Ok,
                format!("{}: {} · {}: {}", ig.alias, scr(ig), nv.alias, scr(nv)),
                None,
            );
            let m = crate::machine::Machine::load().dgpu_mode;
            add(
                "Notebook",
                crate::i18n::tr("modo da NVIDIA"),
                Level::Info,
                m.as_deref()
                    .and_then(|m| crate::hybrid::MODES.iter().find(|x| x.0 == m))
                    .map(|x| crate::i18n::tr(x.1).to_string())
                    .unwrap_or_else(|| crate::i18n::tr("não mexer (fica no sistema; dorme quando ninguém usa)").into()),
                m.is_none().then_some(crate::i18n::tr("Máquina → Notebook → NVIDIA: Automático desliga na bateria e devolve na tomada, sem sair da sessão")),
            );
            if !crate::hybrid::xwayland_wrapped() {
                add(
                    "Notebook",
                    "Xwayland",
                    Level::Info,
                    crate::i18n::tr("pode abrir a NVIDIA à toa (programas X11)").into(),
                    Some(crate::i18n::tr(
                        "sudo numapilot hybrid xwayland on (o Xwayland usa só o Mesa)",
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
                    "{} desligada pelo numapilot (só a integrada)",
                    "{} turned off by numapilot (integrated only)",
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
                crate::i18n::tr("ligado: os modos do numapilot trocam o perfil por ele (o seletor do desktop acompanha)").into(),
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
                            crate::i18n::tr("limites do Ryzen"),
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
                        crate::i18n::tr("limites do Ryzen"),
                        Level::Info,
                        e,
                        Some(crate::i18n::tr(
                            "rode com sudo para ler (o ryzenadj precisa de root)",
                        )),
                    ),
                }
            } else {
                add(
                    "Notebook",
                    crate::i18n::tr("limites do Ryzen"),
                    Level::Info,
                    crate::i18n::tr("ryzenadj não instalado").into(),
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
                        "{} ajustes disponíveis",
                        "{} settings available",
                        crate::acer::knobs().len()
                    ),
                    None,
                );
            } else {
                add(
                    "Notebook",
                    "Acer (Linuwu-Sense)",
                    Level::Info,
                    crate::i18n::tr(
                        "módulo não carregado: sem ventoinha manual, limite de 80% e RGB",
                    )
                    .into(),
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
                crate::i18n::tr("limite de carga"),
                Level::Info,
                crate::i18n::tr("o notebook não permite pelo Linux").into(),
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
                crate::tf!(
                    "{k} está na lista de kernels que quebram esta placa",
                    "{k} is on the list of kernels that break this board"
                ),
                Some(crate::i18n::tr(
                    "Instale outra versão (ex.: linux-cachyos-lts) e escolha no boot",
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
                        "{m} (precisa de {min} ou mais novo)",
                        "{m} (needs {min} or newer)"
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
                crate::i18n::tr("sensores da GPU"),
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
                    crate::i18n::tr("governor de clock"),
                    Level::Ok,
                    crate::tf!("{} rodando", "{} running", s.name),
                    None,
                ),
                None => add(
                    &area,
                    crate::i18n::tr("governor de clock"),
                    Level::Warn,
                    crate::i18n::tr("nenhum rodando: a GPU fica com clock fixo").into(),
                    Some(crate::i18n::tr(
                        "Placas de vídeo → instalar/ligar o governor",
                    )),
                ),
            }
        }
        let cmd = crate::cmdline::running();
        for p in &b.kernel_params {
            if !cmd.split_whitespace().any(|t| t == p) {
                add(
                    &area,
                    &crate::tf!("parâmetro {p}", "parameter {p}"),
                    Level::Info,
                    crate::i18n::tr("recomendado pela comunidade").into(),
                    Some(crate::i18n::tr("Máquina → Parâmetros do kernel")),
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
                    crate::i18n::tr("núcleos da CPU"),
                    Level::Ok,
                    crate::tf!("{n} de {full} (desbloqueados)", "{n} of {full} (unlocked)"),
                    None,
                );
                if let Some(p) = crate::boards::core_unlock_persistence(n, full, efi) {
                    add(
                        &area,
                        crate::i18n::tr("desbloqueio dos núcleos"),
                        Level::Info,
                        p.into(),
                        None,
                    );
                }
            } else if n > 0 {
                add(
                    &area,
                    crate::i18n::tr("núcleos da CPU"),
                    Level::Info,
                    crate::tf!(
                        "{n} de {full} ativos (de fábrica: {stock})",
                        "{n} of {full} active (factory: {stock})"
                    ),
                    Some(crate::i18n::tr(
                        "Dá para desbloquear pelos programas da comunidade (Placas de vídeo → ajustes da comunidade)",
                    )),
                );
            }
        }
        if let Some((_, full)) = b.cus
            && crate::boards::cu_unlock_by_kernel()
        {
            add(
                &area,
                crate::i18n::tr("CUs pelo kernel"),
                Level::Info,
                crate::tf!(
                    "o kernel libera os {full} CUs no boot (amdgpu.bc250_cc_write_mode=3)",
                    "the kernel unlocks the {full} CUs at boot (amdgpu.bc250_cc_write_mode=3)"
                ),
                Some(crate::i18n::tr(
                    "Não use o desbloqueio ao vivo (umr, bc250-cu-live-manager) junto: o driver já conta todos",
                )),
            );
        }
        if let Some((stock, full)) = b.cus {
            match crate::boards::cu_now() {
                Some((n, src)) if n >= full => add(
                    &area,
                    crate::i18n::tr("CUs da GPU"),
                    Level::Ok,
                    crate::tf!(
                        "{n} de {full} (desbloqueados; {})",
                        "{n} of {full} (unlocked; {})",
                        src.text()
                    ),
                    None,
                ),
                Some((n, src)) => add(
                    &area,
                    crate::i18n::tr("CUs da GPU"),
                    Level::Info,
                    crate::tf!(
                        "{n} de {full} ({}; de fábrica: {stock})",
                        "{n} of {full} ({}; factory: {stock})",
                        src.text()
                    ),
                    Some(crate::i18n::tr(
                        "Desbloqueio ao vivo pelo BC250 Control Center (Placas de vídeo → ajustes da comunidade); a leitura ao vivo aparece aqui depois de abrir a página de CUs nele",
                    )),
                ),
                None => {}
            }
        }
        for a in &b.apps {
            let ok = a.installed();
            let what = if ok {
                crate::i18n::tr("instalado").to_string()
            } else if a.recommended {
                crate::tf!(
                    "não instalado (recomendado): {}",
                    "not installed (recommended): {}",
                    a.about
                )
            } else {
                crate::tf!("não instalado: {}", "not installed: {}", a.about)
            };
            let hint = if ok {
                None
            } else if a.installable() {
                Some(
                    crate::i18n::tr("Placas de vídeo → ajustes da comunidade → Instalar")
                        .to_string(),
                )
            } else {
                a.url
                    .as_ref()
                    .map(|u| crate::tf!("instalação manual: {u}", "manual install: {u}"))
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
            add(&area, crate::i18n::tr("dica"), Level::Info, n.clone(), None);
        }
    }
    for n in rec.cpu_notes(&crate::recipes::cpu_model()) {
        add(
            crate::i18n::tr("Processador"),
            crate::i18n::tr("dica"),
            Level::Info,
            n,
            None,
        );
    }
    // ---- interrupções no socket certo ----
    if topo.nodes.len() > 1 {
        let devs = crate::irqs::devices();
        if !devs.is_empty() {
            let desc: Vec<String> = devs
                .iter()
                .map(|d| {
                    crate::tf!(
                        "{} {} no socket {}",
                        "{} {} on socket {}",
                        d.kind,
                        d.pci,
                        d.node
                    )
                })
                .collect();
            add(
                crate::i18n::tr("Interrupções"),
                crate::i18n::tr("discos e rede"),
                Level::Ok,
                desc.join(", "),
                None,
            );
        }
        if crate::irqs::irqbalance_active() {
            add(
                crate::i18n::tr("Interrupções"),
                "irqbalance",
                Level::Info,
                crate::i18n::tr(
                    "ligado: ele espalha as interrupções e o serviço reaplica a cada 5 min",
                )
                .into(),
                Some(crate::i18n::tr(
                    "sudo systemctl disable --now irqbalance (opcional)",
                )),
            );
        }
    }
    // ---- saúde do hardware ----
    let rep = if crate::util::is_root() {
        Some(crate::health::collect(&topo))
    } else {
        crate::health::load()
    };
    match rep {
        Some(r) => {
            if !r.smartctl {
                add(
                    crate::i18n::tr("Saúde"),
                    crate::i18n::tr("discos (SMART)"),
                    Level::Info,
                    crate::i18n::tr("smartctl não instalado").into(),
                    Some("sudo pacman -S smartmontools"),
                );
            }
            let probs = crate::health::problems(&r);
            if probs.is_empty() {
                add(
                    crate::i18n::tr("Saúde"),
                    crate::i18n::tr("geral"),
                    Level::Ok,
                    crate::tf!(
                        "{} disco(s), {} controlador(es) de memória ECC, nenhum problema",
                        "{} disk(s), {} ECC memory controller(s), no problems",
                        r.disks.len(),
                        r.mem.len()
                    ),
                    None,
                );
            }
            for (lvl, p) in probs {
                add(
                    crate::i18n::tr("Saúde"),
                    crate::i18n::tr("alerta"),
                    if lvl >= 2 { Level::Fail } else { Level::Warn },
                    p,
                    None,
                );
            }
        }
        None => add(
            crate::i18n::tr("Saúde"),
            crate::i18n::tr("geral"),
            Level::Info,
            crate::i18n::tr("ainda sem coleta (o serviço coleta 1 min depois de iniciar)").into(),
            Some("sudo numapilot doctor"),
        ),
    }
    // ---- processador ----
    let unc = crate::cpuctl::uncores();
    let intel_cpu = crate::recipes::cpu_model().contains("Intel");
    if unc.is_empty() && intel_cpu {
        add(
            crate::i18n::tr("Processador"),
            "uncore",
            Level::Info,
            crate::i18n::tr("controle indisponível").into(),
            Some(crate::i18n::tr(
                "sudo modprobe intel_uncore_frequency (e echo intel_uncore_frequency | sudo tee /etc/modules-load.d/uncore.conf)",
            )),
        );
    } else if !unc.is_empty() {
        add(
            crate::i18n::tr("Processador"),
            "uncore",
            Level::Ok,
            crate::tf!(
                "{} socket(s) controláveis",
                "{} controllable socket(s)",
                unc.len()
            ),
            None,
        );
    }
    let rapl = crate::cpuctl::rapl_zones();
    let no_limits = !rapl.is_empty() && rapl.iter().all(|r| r.limit_w.is_none());
    add(
        crate::i18n::tr("Processador"),
        crate::i18n::tr("potência (RAPL)"),
        if rapl.is_empty() {
            Level::Info
        } else {
            Level::Ok
        },
        if rapl.is_empty() {
            crate::i18n::tr("indisponível").into()
        } else if no_limits {
            if intel_cpu {
                crate::i18n::tr("só medição (sem limites expostos)").into()
            } else if crate::ryzen::applicable() {
                crate::i18n::tr("só medição no Ryzen; os limites vão pelo ryzenadj (Máquina → Limites do Ryzen)")
                    .into()
            } else {
                crate::i18n::tr("só medição no Ryzen (limites pela BIOS)").into()
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
        crate::i18n::tr("Processador"),
        crate::i18n::tr("estados de economia"),
        if cs.is_empty() {
            Level::Info
        } else {
            Level::Ok
        },
        if cs.is_empty() {
            crate::i18n::tr("cpuidle indisponível").into()
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
            crate::i18n::tr("Processador"),
            crate::i18n::tr("escalonador sched_ext"),
            Level::Ok,
            if cur.is_empty() {
                crate::i18n::tr("padrão do kernel").into()
            } else {
                cur
            },
            None,
        ),
        None => add(
            crate::i18n::tr("Processador"),
            crate::i18n::tr("escalonador sched_ext"),
            Level::Info,
            crate::i18n::tr("scx_loader não instalado").into(),
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
            crate::i18n::tr("Processador"),
            crate::i18n::tr("mitigações ativas"),
            Level::Info,
            mitig.join(", "),
            Some(crate::i18n::tr("Máquina → Mitigações do processador")),
        );
    }
    match crate::cmdline::detect() {
        Some(l) => add(
            crate::i18n::tr("Sistema"),
            "bootloader",
            Level::Ok,
            format!("{} ({})", l.name(), l.path.display()),
            None,
        ),
        None => add(
            crate::i18n::tr("Sistema"),
            "bootloader",
            Level::Info,
            crate::i18n::tr("não reconhecido: parâmetros do kernel só à mão").into(),
            None,
        ),
    }
    let bf = crate::fans::list();
    let ipmi = crate::fans::ipmi_fans();
    add(
        crate::i18n::tr("Sistema"),
        crate::i18n::tr("ventoinhas da placa-mãe"),
        if bf.is_empty() && ipmi.is_empty() {
            Level::Info
        } else {
            Level::Ok
        },
        crate::tf!(
            "{} controláveis (hwmon), {} pelo IPMI",
            "{} controllable (hwmon), {} through IPMI",
            bf.len(),
            ipmi.len()
        ),
        if bf.is_empty() {
            Some(crate::i18n::tr(
                "sudo sensors-detect (pacote lm_sensors) diz qual driver carregar, ex.: sudo modprobe nct6775",
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
            crate::i18n::tr("Sistema"),
            "irqbalance",
            Level::Warn,
            crate::i18n::tr("rodando: pode desfazer as interrupções das GPUs").into(),
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
            crate::i18n::tr("Serviço"),
            "numapilot daemon",
            Level::Ok,
            crate::i18n::tr("rodando").into(),
            None,
        );
    } else {
        add(
            crate::i18n::tr("Serviço"),
            "numapilot daemon",
            Level::Fail,
            crate::i18n::tr("parado").into(),
            Some("sudo systemctl enable --now numapilot"),
        );
    }
    match crate::ctl::send(&crate::ctl::Request::Ping) {
        Ok(r) if r.ok => add(
            crate::i18n::tr("Serviço"),
            crate::i18n::tr("socket de controle"),
            Level::Ok,
            crate::i18n::tr("responde").into(),
            None,
        ),
        _ => add(
            crate::i18n::tr("Serviço"),
            crate::i18n::tr("socket de controle"),
            Level::Fail,
            crate::i18n::tr("sem resposta").into(),
            Some(crate::i18n::tr("Veja: journalctl -u numapilot -n 50")),
        ),
    }
    if let Some(st) = std::fs::read_to_string(status_path())
        .ok()
        .and_then(|s| serde_json::from_str::<Status>(&s).ok())
    {
        add(
            crate::i18n::tr("Serviço"),
            crate::i18n::tr("como root"),
            if st.root { Level::Ok } else { Level::Fail },
            format!("{}", st.root),
            None,
        );
        add(
            crate::i18n::tr("Serviço"),
            crate::i18n::tr("detecção instantânea"),
            if st.proc_connector {
                Level::Ok
            } else {
                Level::Warn
            },
            if st.proc_connector {
                crate::i18n::tr("proc connector ativo").into()
            } else {
                crate::i18n::tr("só por varredura").into()
            },
            None,
        );
        match &st.cgroup_last_error {
            Some(e) => add(
                crate::i18n::tr("Serviço"),
                crate::i18n::tr("grupos de controle"),
                Level::Fail,
                e.clone(),
                Some(crate::i18n::tr("Me mande este relatório")),
            ),
            None => add(
                crate::i18n::tr("Serviço"),
                crate::i18n::tr("grupos de controle"),
                Level::Ok,
                crate::tf!(
                    "{} aplicados sem erro",
                    "{} applied with no error",
                    st.cgroup_ok_count
                ),
                None,
            ),
        }
        for e in &st.od_errors {
            add(
                crate::i18n::tr("Serviço"),
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
            crate::i18n::tr("identificação"),
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
                crate::i18n::tr("desconhecido").into(),
                Some(crate::i18n::tr(
                    "Defina em [gpu_node] no /etc/numapilot.toml",
                )),
            ),
            None => {}
        }
        add(
            &a,
            "monitor",
            Level::Info,
            if g.display_connected {
                crate::i18n::tr("ligado nesta placa").into()
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
                    crate::i18n::tr("não aparece no KFD").into(),
                    Some(crate::i18n::tr(
                        "Verifique se o módulo amdgpu carregou com KFD (dmesg | grep kfd)",
                    )),
                ),
            }
        }
        let s = gpuctl::read_sensors(&g.pci);
        // integrada e placa de notebook: ventoinha e potência são do próprio notebook
        let laptop = crate::notebook::is_laptop();
        let soft = if g.is_integrated() || laptop {
            Level::Info
        } else {
            Level::Warn
        };
        // RDNA3/RDNA4: o pwm1 é só leitura; o controle é a curva do firmware em
        // gpu_od/fan_ctrl, que só existe com o OverDrive ligado
        let fan_ok = s.fan_controllable || s.fw_fan_curve;
        let od_off_fan = !fan_ok
            && g.vendor == 0x1002
            && s.fan_rpm.is_some()
            && !crate::bootcfg::overdrive_state().active;
        add(
            &a,
            crate::i18n::tr("ventoinha"),
            if fan_ok { Level::Ok } else { soft },
            if s.fw_fan_curve {
                crate::tf!(
                    "controlável pela curva do firmware ({} RPM)",
                    "controllable through the firmware curve ({} RPM)",
                    s.fan_rpm.unwrap_or(0)
                )
            } else if s.fan_controllable {
                crate::tf!(
                    "controlável ({} RPM)",
                    "controllable ({} RPM)",
                    s.fan_rpm.unwrap_or(0)
                )
            } else if laptop || g.is_integrated() {
                crate::i18n::tr("controlada pelo notebook (perfil de energia)").into()
            } else if od_off_fan {
                crate::i18n::tr("sem controle: OverDrive desligado").into()
            } else {
                crate::i18n::tr("sem controle pelo driver").into()
            },
            od_off_fan.then_some(crate::i18n::tr(
                "Nas placas AMD novas (RX 7000/9000) a ventoinha é controlada pela curva do \
                 firmware, que só aparece com o OverDrive ligado: Placas de vídeo → OverDrive \
                 (precisa reiniciar)",
            )),
        );
        add(
            &a,
            crate::i18n::tr("limite de potência"),
            if s.power_cap_max_w.is_some() || s.power_cap_w.is_some() {
                Level::Ok
            } else {
                soft
            },
            s.power_cap_w
                .map(|w| format!("{w:.0} W"))
                .unwrap_or_else(|| crate::i18n::tr("indisponível").into()),
            None,
        );
        match gpuctl::read_od(&g.pci) {
            _ if g.is_nvidia() => {}
            Some(od) => add(
                &a,
                crate::i18n::tr("clock/voltagem (OD)"),
                Level::Ok,
                {
                    let mut d = od
                        .range_sclk
                        .map(|r| format!("GPU {}-{} MHz", r.0, r.1))
                        .unwrap_or_else(|| crate::i18n::tr("ligado").into());
                    if !od.curve.is_empty() {
                        d +=
                            &crate::tf!(", curva de {} pontos", ", {}-point curve", od.curve.len());
                    }
                    if let Some((lo, _)) = od.range_vo {
                        d += &crate::tf!(", voltagem até {lo} mV", ", voltage down to {lo} mV");
                    }
                    d
                },
                None,
            ),
            None => add(
                &a,
                crate::i18n::tr("clock/voltagem (OD)"),
                Level::Info,
                crate::i18n::tr("indisponível").into(),
                Some(crate::i18n::tr(
                    "sudo numapilot boot overdrive on (ou o botão na aba Placas de vídeo) e reinicie",
                )),
            ),
        }
        if let Some(l) = gpuctl::pcie_link(&g.pci) {
            let d = crate::tf!(
                "Gen{} x{} (máx. Gen{} x{})",
                "Gen{} x{} (max Gen{} x{})",
                l.generation,
                l.width,
                l.max_gen,
                l.max_width
            );
            if l.width < l.max_width {
                add(
                    &a,
                    crate::i18n::tr("link PCIe"),
                    Level::Warn,
                    d,
                    Some(crate::i18n::tr(
                        "Link mais estreito que o possível: confira o slot (alguns são x8 ou dividem linhas) e a BIOS",
                    )),
                );
            } else {
                add(
                    &a,
                    crate::i18n::tr("link PCIe"),
                    Level::Ok,
                    crate::tf!(
                        "{d}; a geração cai com a placa parada",
                        "{d}; the generation drops when the card is idle"
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
                    "ativo ({} MB visíveis)",
                    "active ({} MB visible)",
                    vis >> 20
                ),
                None,
            ),
            Some((vis, tot)) => add(
                &a,
                "Resizable BAR",
                Level::Info,
                crate::tf!(
                    "inativo ({} de {} MB visíveis)",
                    "inactive ({} of {} MB visible)",
                    vis >> 20,
                    tot >> 20
                ),
                Some(crate::i18n::tr(
                    "Na BIOS: \"Above 4G Decoding\" e \"Resizable BAR\" (se existir). Em placas X99/C612 antigas, tente o parâmetro pci=realloc",
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
                        "só esta GPU visível: {}",
                        "only this GPU visible: {}",
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
                        "{} dispositivos visíveis",
                        "{} devices visible",
                        names.len()
                    ),
                    Some(crate::i18n::tr(
                        "A seleção de GPU do Mesa não funcionou: me mande o relatório",
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

    // ---- ROCm / permissões ----
    let groups = out(&["id", "-nG"], &[]).unwrap_or_default();
    let has_compute = topo
        .gpus
        .iter()
        .any(|g| g.role == crate::topo::GpuRole::Compute);
    for gname in ["render", "video"] {
        if !groups.split_whitespace().any(|x| x == gname) {
            add(
                "ROCm",
                &crate::tf!("grupo {gname}", "group {gname}"),
                if has_compute {
                    Level::Warn
                } else {
                    Level::Info
                },
                crate::i18n::tr("usuário fora do grupo").into(),
                Some(crate::i18n::tr(
                    "sudo usermod -aG render,video $USER (e entre de novo na sessão)",
                )),
            );
        }
    }
    if std::path::Path::new("/dev/kfd").exists() {
        add(
            "ROCm",
            "/dev/kfd",
            Level::Ok,
            crate::i18n::tr("existe").into(),
            None,
        );
    } else {
        add(
            "ROCm",
            "/dev/kfd",
            Level::Fail,
            crate::i18n::tr("ausente").into(),
            None,
        );
    }

    // ---- ferramentas ----
    for (bin, why, pkg) in [
        (
            "podman",
            crate::i18n::tr("laboratório, servidor de IA"),
            "podman",
        ),
        ("mangohud", crate::i18n::tr("otimizador de FPS"), "mangohud"),
        (
            "gamescope",
            crate::i18n::tr("gamescope por jogo"),
            "gamescope",
        ),
        ("vainfo", crate::i18n::tr("aba Streaming"), "libva-utils"),
        ("steam", crate::i18n::tr("jogos"), "steam"),
    ] {
        if which(bin) {
            add(
                crate::i18n::tr("Ferramentas"),
                bin,
                Level::Ok,
                crate::i18n::tr("instalado").into(),
                None,
            );
        } else {
            add(
                crate::i18n::tr("Ferramentas"),
                bin,
                Level::Info,
                crate::tf!("não instalado ({why})", "not installed ({why})"),
                Some(&format!("sudo pacman -S {pkg}")),
            );
        }
    }
    // vulkaninfo
    if !have_vk {
        add(
            crate::i18n::tr("Ferramentas"),
            "vulkaninfo",
            Level::Info,
            crate::i18n::tr("não instalado: não deu para testar a seleção de GPU no Vulkan").into(),
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
                crate::i18n::tr("jogos com numapilot"),
                if with == games.len() && !games.is_empty() {
                    Level::Ok
                } else {
                    Level::Info
                },
                crate::tf!(
                    "{with} de {} jogos instalados",
                    "{with} of {} installed games",
                    games.len()
                ),
                Some(crate::i18n::tr(
                    "Interface → Programas → \"Aplicar em todos os jogos da Steam\"",
                )),
            );
        }
        None => add(
            "Steam",
            crate::i18n::tr("configuração"),
            Level::Info,
            crate::i18n::tr("Steam não encontrada").into(),
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
            Level::Warn => crate::i18n::tr("[aviso]"),
            Level::Fail => crate::i18n::tr("[FALHA]"),
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
            "\nResumo: {fails} falha(s), {warns} aviso(s).",
            "\nSummary: {fails} failure(s), {warns} warning(s)."
        )
    );
}

/// Relatório completo para enviar (diagnóstico + topologia + sistema).
pub fn report(checks: &[Check]) -> String {
    let mut s = String::from("# numapilot — relatório de diagnóstico\n\n");
    s.push_str(&crate::tf!(
        "versão: {}\n",
        "version: {}\n",
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
        (crate::i18n::tr("lspci (árvore)"), &["lspci", "-tv"]),
        (
            "lspci (GPUs)",
            &["sh", "-c", "lspci -nnk | grep -A3 -Ei 'vga|display|3d'"],
        ),
        ("numapilot topo", &["numapilot", "topo"]),
        ("gfx906-rocm doctor", &["gfx906-rocm", "doctor"]),
        (
            crate::i18n::tr("serviço (log)"),
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
        s.push_str(&out(argv, &[]).unwrap_or_else(|| crate::i18n::tr("(indisponível)\n").into()));
    }
    s
}
