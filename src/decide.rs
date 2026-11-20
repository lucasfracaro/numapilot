//! Shared decision logic: which GPU and which node(s) to use.

use crate::config::{Config, CpuSpec, GpuSpec, MemSpec, Rule};
use crate::learned::Learned;
use crate::procfs;
use crate::profiles::Profiles;
use crate::topo::{Gpu, GpuRole, Topology};
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Graphics,
    Compute,
    NoGpu,
}

impl Kind {
    /// "graphics"/"jogo" → Graphics, "compute"/"ia" → Compute, "auto" → None
    pub fn parse_usage(s: &str) -> Result<Option<Kind>> {
        Ok(match s.trim().to_lowercase().as_str() {
            "" | "auto" => None,
            "graphics" | "grafico" | "gráfico" | "jogo" | "game" => Some(Kind::Graphics),
            "compute" | "computacao" | "computação" | "ia" | "rocm" => Some(Kind::Compute),
            o => bail!(
                "{}",
                crate::tf!(
                    "use must be auto, graphics or compute (got '{o}')",
                    "uso deve ser auto, graphics ou compute (recebido '{o}')"
                )
            ),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Decision {
    pub nodes: Vec<usize>,
    pub cpus: Vec<usize>,
    pub gpus: Vec<Gpu>,
    pub kind: Kind,
    pub mem: MemSpec,
    pub manual: bool,
    /// Limit what the program "sees" (WINE_CPU_TOPOLOGY, *_NUM_THREADS...).
    pub limit_visible: bool,
    /// "low" | "normal" | "high"
    pub priority: String,
    /// Needs the service (cgroup / high priority) before exec.
    pub needs_daemon: bool,
    /// Frame generation GPU (lsfg-vk), if different from the game's.
    pub fg_gpu: Option<Gpu>,
    pub reasons: Vec<String>,
}

/// Busy fraction of each node, sampling /proc/stat for `dur`.
pub fn sample_node_loads(topo: &Topology, dur: Duration) -> HashMap<usize, f64> {
    let a = procfs::cpu_times();
    std::thread::sleep(dur);
    let b = procfs::cpu_times();
    node_loads(topo, &a, &b)
}

pub fn node_loads(
    topo: &Topology,
    a: &[(usize, u64, u64)],
    b: &[(usize, u64, u64)],
) -> HashMap<usize, f64> {
    let prev: HashMap<usize, (u64, u64)> =
        a.iter().map(|&(c, busy, tot)| (c, (busy, tot))).collect();
    let mut out = HashMap::new();
    for n in &topo.nodes {
        let (mut busy, mut tot) = (0u64, 0u64);
        for &(c, bb, bt) in b {
            if n.cpus.contains(&c)
                && let Some(&(pb, pt)) = prev.get(&c)
            {
                busy += bb.saturating_sub(pb);
                tot += bt.saturating_sub(pt);
            }
        }
        out.insert(
            n.id,
            if tot > 0 {
                busy as f64 / tot as f64
            } else {
                0.0
            },
        );
    }
    out
}

/// Least loaded node, avoiding `avoid` if there is an alternative and requiring ~5% free RAM.
pub fn least_loaded_node(topo: &Topology, loads: &HashMap<usize, f64>, avoid: &[usize]) -> usize {
    let mut cands: Vec<&crate::topo::Node> = topo
        .nodes
        .iter()
        .filter(|n| !avoid.contains(&n.id))
        .collect();
    if cands.is_empty() {
        cands = topo.nodes.iter().collect();
    }
    let with_mem: Vec<_> = cands
        .iter()
        .copied()
        .filter(|n| n.mem_total_kb == 0 || n.mem_free_kb() * 20 > n.mem_total_kb)
        .collect();
    let pool = if with_mem.is_empty() { cands } else { with_mem };
    pool.iter()
        .min_by(|x, y| {
            let lx = loads.get(&x.id).copied().unwrap_or(0.0);
            let ly = loads.get(&y.id).copied().unwrap_or(0.0);
            lx.partial_cmp(&ly).unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|n| n.id)
        .unwrap_or(0)
}

/// Picks the GPU(s). `want` forces the kind of use (e.g. gaming on an MI50 = Compute GPU used as Graphics).
pub fn pick_gpus(
    topo: &Topology,
    spec: &GpuSpec,
    want: Option<Kind>,
    hint: bool,
) -> Result<(Vec<Gpu>, Kind)> {
    let least_busy = |n: usize| -> Vec<Gpu> {
        let mut c: Vec<&Gpu> = topo.compute_gpus();
        // No ROCm card (e.g. BC-250, gfx1013): compute goes through Vulkan on the only GPU
        if c.is_empty() && topo.gpus.len() == 1 {
            c = topo.gpus.iter().collect();
        }
        c.sort_by(|a, b| {
            a.load_score()
                .partial_cmp(&b.load_score())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let take = if n == 0 { c.len() } else { n.min(c.len()) };
        c.into_iter().take(take).cloned().collect()
    };
    Ok(match spec {
        GpuSpec::None => (vec![], Kind::NoGpu),
        GpuSpec::Display => (
            topo.gaming_gpu().cloned().into_iter().collect(),
            Kind::Graphics,
        ),
        GpuSpec::Compute(n) => {
            let g = least_busy(*n);
            if g.is_empty() {
                bail!(
                    "{}",
                    crate::tf!(
                        "no compute GPU (ROCm) found; see `numapilot topo`",
                        "nenhuma GPU de computação (ROCm) encontrada; veja `numapilot topo`"
                    )
                );
            }
            (g, want.unwrap_or(Kind::Compute))
        }
        GpuSpec::Named(names) => {
            let mut v = Vec::new();
            for n in names {
                match topo.gpus.iter().find(|g| g.matches(n)) {
                    Some(g) => v.push(g.clone()),
                    None => {
                        bail!(
                            "{}",
                            crate::tf!(
                                "GPU '{n}' not found; see the aliases with `numapilot topo`",
                                "GPU '{n}' não encontrada; veja os apelidos com `numapilot topo`"
                            )
                        )
                    }
                }
            }
            let role_kind = if v.iter().all(|g| g.role == GpuRole::Compute) && !hint_graphics(want)
            {
                Kind::Compute
            } else {
                Kind::Graphics
            };
            (v, want.unwrap_or(role_kind))
        }
        GpuSpec::Auto => match want {
            Some(Kind::Graphics) => (
                topo.gaming_gpu().cloned().into_iter().collect(),
                Kind::Graphics,
            ),
            Some(Kind::Compute) => (least_busy(1), Kind::Compute),
            _ if hint && !topo.compute_gpus().is_empty() => (least_busy(1), Kind::Compute),
            _ => (
                topo.display_gpu().cloned().into_iter().collect(),
                Kind::Graphics,
            ),
        },
    })
}

fn hint_graphics(want: Option<Kind>) -> bool {
    want == Some(Kind::Graphics)
}

pub struct LaunchInput<'a> {
    pub idents: &'a [String],
    pub learn_key: &'a str,
    /// Profile key (steam:ID or executable).
    pub key: &'a str,
    pub cpu: Option<CpuSpec>,
    pub gpu: Option<GpuSpec>,
    pub mem: Option<MemSpec>,
    pub usage: Option<Kind>,
    /// lsfg-vk frame generation GPU ("same" = the game's).
    pub fg_gpu: Option<String>,
}

pub fn decide_launch(
    cfg: &Config,
    topo: &Topology,
    learned: &Learned,
    profiles: &Profiles,
    loads: &HashMap<usize, f64>,
    inp: &LaunchInput,
) -> Result<Decision> {
    let mut reasons = Vec::new();
    let profile = profiles.get(inp.key).filter(|p| !p.is_auto());
    let rule: Option<&Rule> = cfg.find_rule(inp.idents);
    if profile.is_some() {
        reasons.push(crate::i18n::tr("profile saved by the graphical interface").into());
    } else if let Some(r) = rule {
        reasons.push(crate::tf!(
            "rule '{}' from the config",
            "regra '{}' do config",
            r.label()
        ));
    }
    let pick = |cli: Option<String>, prof: Option<&str>, rule: Option<&str>| -> String {
        cli.or_else(|| prof.filter(|s| *s != "auto").map(String::from))
            .or_else(|| rule.map(String::from))
            .unwrap_or_else(|| "auto".into())
    };
    let cpu = match &inp.cpu {
        Some(c) => c.clone(),
        None => CpuSpec::parse(&pick(
            None,
            profile.map(|p| p.cpu.as_str()),
            rule.map(|r| r.cpu.as_str()),
        ))?,
    };
    let gpu = match &inp.gpu {
        Some(g) => g.clone(),
        None => GpuSpec::parse(&pick(
            None,
            profile.map(|p| p.gpu.as_str()),
            rule.map(|r| r.gpu.as_str()),
        ))?,
    };
    let usage = match inp.usage {
        Some(k) => Some(k),
        None => Kind::parse_usage(&pick(
            None,
            profile.map(|p| p.usage.as_str()),
            rule.map(|r| r.usage.as_str()),
        ))?,
    };
    let prof_mem = profile.and_then(|p| match p.memory.as_str() {
        "strict" => Some(MemSpec::Bind),
        "interleave" => Some(MemSpec::Interleave),
        "prefer" | "free" => Some(MemSpec::Local),
        _ => None,
    });
    let mem = match inp.mem {
        Some(m) => m,
        None => match prof_mem {
            Some(m) => m,
            None => rule
                .map(|r| MemSpec::parse(&r.mem))
                .transpose()?
                .unwrap_or(MemSpec::Local),
        },
    };
    let hint = cfg.is_compute_hint(inp.idents);
    if gpu == GpuSpec::Auto && usage.is_none() && hint {
        reasons.push(crate::i18n::tr("known compute program → ROCm GPU").into());
    }
    let (gpus, kind) = pick_gpus(topo, &gpu, usage, hint)?;
    if kind == Kind::Graphics && gpus.iter().any(|g| g.role == GpuRole::Compute) {
        reasons.push(crate::tf!(
            "renders on the {} and the image goes to the card with the monitor (PRIME offload)",
            "renderiza na {} e a imagem vai para a placa com monitor (PRIME offload)",
            gpus[0].alias
        ));
    }

    let gpu_nodes: Vec<usize> = {
        let mut v: Vec<usize> = gpus.iter().filter_map(|g| g.numa_node).collect();
        v.sort_unstable();
        v.dedup();
        v
    };
    let all = topo.node_ids();
    let learned_e = learned.get(inp.learn_key);

    let (nodes, cpus) = match &cpu {
        CpuSpec::All => (all.clone(), topo.all_cpus()),
        CpuSpec::Nodes(n) => {
            for id in n {
                if topo.node(*id).is_none() {
                    bail!(
                        "{}",
                        crate::tf!(
                            "node {id} does not exist (nodes: {:?})",
                            "nó {id} não existe (nós: {:?})",
                            all
                        )
                    );
                }
            }
            (n.clone(), topo.cpus_of(n))
        }
        CpuSpec::Cpus(c) => {
            // Only the CPUs that exist here (cpus:0-99 on a 16-thread PC would hand
            // sched_setaffinity CPUs it rejects, or a list the user did not mean)
            let have = topo.all_cpus();
            let c: Vec<usize> = c.iter().copied().filter(|x| have.contains(x)).collect();
            if c.is_empty() {
                bail!(
                    "{}",
                    crate::tf!(
                        "none of those CPUs exist (this PC has {})",
                        "nenhuma dessas CPUs existe (este PC tem {})",
                        crate::util::format_cpulist(&have)
                    )
                );
            }
            let nodes: Vec<usize> = topo
                .nodes
                .iter()
                .filter(|n| n.cpus.iter().any(|x| c.contains(x)))
                .map(|n| n.id)
                .collect();
            (nodes, c)
        }
        CpuSpec::GpuNode | CpuSpec::Auto => {
            if cpu == CpuSpec::Auto && learned_e.is_some_and(|e| e.wide) {
                reasons.push(
                    crate::i18n::tr("learned: this program does better with both sockets").into(),
                );
                (all.clone(), topo.all_cpus())
            } else if !gpu_nodes.is_empty() {
                if gpu_nodes.len() > 1 {
                    reasons.push(
                        crate::i18n::tr("chosen GPUs are on different sockets → all nodes").into(),
                    );
                } else {
                    reasons.push(crate::tf!(
                        "node {} is the socket connected to the GPU",
                        "nó {} é o socket ligado à GPU",
                        gpu_nodes[0]
                    ));
                }
                (gpu_nodes.clone(), topo.cpus_of(&gpu_nodes))
            } else {
                let n = least_loaded_node(topo, loads, &[]);
                reasons.push(crate::tf!(
                    "node {n} is the least busy ({:.0}% usage)",
                    "nó {n} é o menos ocupado ({:.0}% de uso)",
                    loads.get(&n).copied().unwrap_or(0.0) * 100.0
                ));
                (vec![n], topo.cpus_of(&[n]))
            }
        }
    };
    // The profile's cores: list made on the map, or a count / no hyper-threading.
    let (nodes, cpus) = match profile {
        Some(p) if inp.cpu.is_none() => {
            if let Some(l) = &p.cpu_list {
                let all = topo.all_cpus();
                let v: Vec<usize> = crate::util::parse_cpulist(l)
                    .into_iter()
                    .filter(|c| all.contains(c))
                    .collect();
                if v.is_empty() {
                    (nodes, cpus)
                } else {
                    (topo.nodes_of(&v), v)
                }
            } else if let Some(cores) = p
                .cores_for(crate::machine::sched_ext_active())
                .or((!p.smt).then_some(usize::MAX))
            {
                let count = (cores != usize::MAX).then_some(cores);
                let smt = p.smt_for(crate::machine::sched_ext_active());
                let v = topo.select_cpus(&nodes, count, smt, &[]);
                reasons.push(crate::tf!(
                    "{} CPUs ({} physical cores{}) chosen in the profile, {}",
                    "{} CPUs ({} núcleos físicos{}) escolhidos no perfil, {}",
                    v.len(),
                    topo.physical_count(&v),
                    if smt {
                        ""
                    } else {
                        crate::i18n::tr(", no hyper-threading")
                    },
                    match crate::corebench::order_source() {
                        crate::corebench::OrderSource::Cache => crate::t2!(
                            "fastest cache first (measured here)",
                            "cache mais rápido primeiro (medido aqui)"
                        ),
                        crate::corebench::OrderSource::Firmware => crate::t2!(
                            "the firmware's preferred cores first (the cache latency makes no difference here)",
                            "núcleos preferidos do firmware primeiro (a latência do cache não faz diferença aqui)"
                        ),
                        crate::corebench::OrderSource::System => {
                            crate::t2!("in the system order", "na ordem do sistema")
                        }
                    }
                ));
                if p.cpu_max_pct.is_some()
                    && crate::machine::sched_ext_active()
                    && p.cores_for(true) != p.cores
                {
                    reasons.push(crate::tf!(
                        "processor limit {:.0}% applied as {} core(s): the sched_ext scheduler does not enforce cpu.max",
                        "limite de processador {:.0}% aplicado como {} núcleo(s): o escalonador sched_ext não aplica o cpu.max",
                        p.cpu_max_pct.unwrap_or(0.0),
                        v.len()
                    ));
                }
                (topo.nodes_of(&v), v)
            } else {
                (nodes, cpus)
            }
        }
        _ => (nodes, cpus),
    };
    if cpus.is_empty() {
        bail!(
            "{}",
            crate::tf!("no CPU selected", "nenhum CPU selecionado")
        );
    }
    let fg_name = inp
        .fg_gpu
        .clone()
        .or_else(|| profile.map(|p| p.fg_gpu.clone()))
        .unwrap_or_else(|| "same".into());
    let fg_gpu = match fg_name.as_str() {
        "" | "same" | "auto" => None,
        n => match topo.gpus.iter().find(|g| g.matches(n)) {
            Some(g) if gpus.first().is_none_or(|x| x.pci != g.pci) => {
                reasons.push(crate::tf!(
                    "frame generation (lsfg-vk) on the {}: requires lsfg-vk with dual GPU",
                    "frame generation (lsfg-vk) na {}: requer lsfg-vk com dual GPU",
                    g.alias
                ));
                Some(g.clone())
            }
            Some(_) => None,
            None => bail!(
                "{}",
                crate::tf!(
                    "frame generation GPU '{n}' not found",
                    "GPU de frame generation '{n}' não encontrada"
                )
            ),
        },
    };
    let total = topo.all_cpus().len();
    Ok(Decision {
        manual: cpu != CpuSpec::Auto || profile.is_some_and(|p| p.has_cpu_choice()),
        limit_visible: profile.is_none_or(|p| p.limit_visible) && cpus.len() < total,
        priority: profile
            .map(|p| p.priority.clone())
            .unwrap_or_else(|| "normal".into()),
        needs_daemon: profile.is_some_and(|p| p.needs_daemon()),
        nodes,
        cpus,
        gpus,
        kind,
        mem,
        fg_gpu,
        reasons,
    })
}

/// "0000:83:00.0" → "131:0.0" (decimal PCI bus format used by lsfg-vk)
pub fn pci_decimal(pci: &str) -> String {
    let p: Vec<&str> = pci.split([':', '.']).collect();
    if p.len() == 4 {
        let h = |s: &str| u32::from_str_radix(s, 16).unwrap_or(0);
        format!("{}:{}.{}", h(p[1]), h(p[2]), h(p[3]))
    } else {
        pci.to_string()
    }
}

/// Environment variables that steer the program to the chosen GPU(s).
pub fn gpu_env(d: &Decision, topo: &Topology) -> (Vec<(String, String)>, Vec<String>) {
    let mut set = Vec::new();
    let mut unset = Vec::new();
    match d.kind {
        Kind::Graphics => {
            if let Some(g) = d.gpus.first() {
                // OpenGL and Vulkan (Mesa) pick the card through DRI_PRIME=pci-...
                set.push(("DRI_PRIME".into(), g.dri_prime()));
                // Vulkan (native games and DirectX through DXVK/VKD3D-Proton) sees only this GPU:
                // keeps the game from picking another card by itself (e.g. the MI50 with more VRAM).
                set.push((
                    "MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE".into(),
                    "1".into(),
                ));
                // vendor:device only works if it is unique (both MI50s have the same ID).
                let same = topo
                    .gpus
                    .iter()
                    .filter(|o| o.vendor == g.vendor && o.device == g.device)
                    .count();
                if same == 1 {
                    set.push((
                        "MESA_VK_DEVICE_SELECT".into(),
                        format!("{:04x}:{:04x}", g.vendor, g.device),
                    ));
                } else {
                    unset.push("MESA_VK_DEVICE_SELECT".into());
                }
                // Lossless Scaling (lsfg-vk): by default generates frames on the same GPU as the game
                // (official lsfg-vk requires it); with a dual-GPU profile, on the chosen GPU.
                let fg = d.fg_gpu.as_ref().unwrap_or(g);
                set.push(("LSFGVK_GPU".into(), pci_decimal(&fg.pci)));
                set.push(("NUMAPILOT_FG_GPU".into(), fg.pci.clone()));
                // Official NVIDIA driver (PRIME render offload): OpenGL and Vulkan on the NVIDIA,
                // or the opposite when the chosen one is the other card on a PC with NVIDIA.
                let has_nv = topo.gpus.iter().any(|o| o.is_nvidia());
                if g.is_nvidia() {
                    set.push(("__NV_PRIME_RENDER_OFFLOAD".into(), "1".into()));
                    // "NVIDIA only when needed" session: login limited EGL to Mesa
                    if let Some(egl) = crate::hybrid::nvidia_egl() {
                        set.push(("__EGL_VENDOR_LIBRARY_FILENAMES".into(), egl));
                    }
                    set.push(("__GLX_VENDOR_LIBRARY_NAME".into(), "nvidia".into()));
                    set.push(("__VK_LAYER_NV_optimus".into(), "NVIDIA_only".into()));
                    // Inherited from a program that started on the integrated GPU (e.g. Steam from
                    // startup): an ICD list without the NVIDIA hides it from Vulkan. DRI_PRIME and
                    // MESA_VK_DEVICE_SELECT* stay set above (pointing at the NVIDIA): without them,
                    // Mesa's device-select layer leaves the integrated card first and programs that
                    // take Vulkan device 0 (native games, vkmark) run on it.
                    for v in ["VK_DRIVER_FILES", "VK_ICD_FILENAMES"] {
                        unset.push(v.into());
                    }
                    let nvs = crate::nvctl::cuda_indices(topo);
                    if nvs.len() > 1
                        && let Some(i) = nvs.get(&g.pci)
                    {
                        set.push((
                            "__NV_PRIME_RENDER_OFFLOAD_PROVIDER".into(),
                            format!("NVIDIA-G{i}"),
                        ));
                    }
                } else if has_nv {
                    set.push(("__GLX_VENDOR_LIBRARY_NAME".into(), "mesa".into()));
                    set.push(("__VK_LAYER_NV_optimus".into(), "non_NVIDIA_only".into()));
                    unset.push("__NV_PRIME_RENDER_OFFLOAD".into());
                    // Without this, programs with EGL/Vulkan (Chromium, Qt WebEngine, Electron)
                    // load the NVIDIA driver and wake the laptop card for no reason.
                    let egl = "/usr/share/glvnd/egl_vendor.d/50_mesa.json";
                    if std::path::Path::new(egl).exists() {
                        set.push(("__EGL_VENDOR_LIBRARY_FILENAMES".into(), egl.into()));
                    }
                    let icds = non_nvidia_vk_icds();
                    if !icds.is_empty() {
                        set.push(("VK_DRIVER_FILES".into(), icds.join(":")));
                        set.push(("VK_ICD_FILENAMES".into(), icds.join(":")));
                    }
                }
            }
        }
        Kind::Compute => {
            let nvs = crate::nvctl::cuda_indices(topo);
            let cuda: Vec<String> = d
                .gpus
                .iter()
                .filter_map(|g| nvs.get(&g.pci))
                .map(|i| i.to_string())
                .collect();
            if !cuda.is_empty() {
                set.push(("CUDA_DEVICE_ORDER".into(), "PCI_BUS_ID".into()));
                set.push(("CUDA_VISIBLE_DEVICES".into(), cuda.join(",")));
            }
            let ids: Vec<String> = d.gpus.iter().filter_map(|g| g.rocr_id.clone()).collect();
            if !ids.is_empty() {
                set.push(("ROCR_VISIBLE_DEVICES".into(), ids.join(",")));
                // Inherited HIP/CUDA indices would point into the already filtered list.
                unset.push("HIP_VISIBLE_DEVICES".into());
                if cuda.is_empty() {
                    unset.push("CUDA_VISIBLE_DEVICES".into());
                }
            }
        }
        Kind::NoGpu => {}
    }
    if d.limit_visible {
        let n = d.cpus.len().to_string();
        // Proton/Wine: the Windows game sees only these CPUs (taskset alone does not change what it counts).
        set.push((
            "WINE_CPU_TOPOLOGY".into(),
            format!(
                "{n}:{}",
                d.cpus
                    .iter()
                    .map(|c| c.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        ));
        for k in SOFT_THREAD_VARS {
            set.push((k.to_string(), n.clone()));
        }
    }
    let renders: Vec<String> = d
        .gpus
        .iter()
        .filter_map(|g| g.render.as_ref().map(|r| format!("/dev/dri/{r}")))
        .collect();
    set.push(("NUMAPILOT_RENDER".into(), renders.join(" ")));
    set.push((
        "NUMAPILOT_GPUS".into(),
        d.gpus
            .iter()
            .map(|g| g.alias.clone())
            .collect::<Vec<_>>()
            .join(","),
    ));
    set.push((
        "NUMAPILOT_GPU_PCI".into(),
        d.gpus
            .iter()
            .map(|g| g.pci.clone())
            .collect::<Vec<_>>()
            .join(","),
    ));
    set.push((
        "NUMAPILOT_CPUS".into(),
        crate::util::format_cpulist(&d.cpus),
    ));
    set.push((
        "NUMAPILOT_MEMS".into(),
        d.nodes
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(","),
    ));
    set.push((
        "NUMAPILOT_MODE".into(),
        if d.manual { "manual" } else { "auto" }.into(),
    ));
    // A variable is either set or removed, never both: callers apply the two lists in different
    // orders, and removing after setting silently dropped the GPU choice.
    unset.retain(|k| !set.iter().any(|(s, _)| s == k));
    unset.dedup();
    (set, unset)
}

/// The environment a program gets: `base` without `unset`, plus `set`. Same order as
/// `numapilot run` and `gpu-test` apply them (remove first, then set).
pub fn effective_env(
    base: &[(String, String)],
    set: &[(String, String)],
    unset: &[String],
) -> HashMap<String, String> {
    let mut env: HashMap<String, String> = base
        .iter()
        .filter(|(k, _)| !unset.contains(k))
        .cloned()
        .collect();
    for (k, v) in set {
        env.insert(k.clone(), v.clone());
    }
    env
}

/// "How many threads to use" variables of common libraries. `numapilot run` only sets them
/// if the user has not already (it respects their choice).
pub const SOFT_THREAD_VARS: &[&str] = &[
    "OMP_NUM_THREADS",
    "MKL_NUM_THREADS",
    "OPENBLAS_NUM_THREADS",
    "NUMEXPR_NUM_THREADS",
    "RAYON_NUM_THREADS",
    "GOMAXPROCS",
    "DOTNET_PROCESSOR_COUNT",
];

/// Compatibility warnings for games (common problems on dual-socket servers).
pub fn game_warnings(topo: &Topology, d: &Decision) -> Vec<String> {
    let mut w = Vec::new();
    if d.kind != Kind::Graphics {
        return w;
    }
    let Some(g) = d.gpus.first() else { return w };
    // On a laptop, the dedicated card always renders and the integrated one shows the image (normal).
    if !g.display_connected && !crate::notebook::is_laptop() {
        w.push(crate::tf!(
            "The game will render on the {} but the monitor is on another card. Some games and anti-cheats \
             refuse this (e.g. Crimson Desert) and copying between cards costs performance: connect the monitor to this card.", "O jogo vai renderizar na {} mas o monitor está em outra placa. Alguns jogos e anti-cheats \
             recusam isso (ex.: Crimson Desert) e a cópia entre placas custa desempenho: ligue o monitor nesta placa.",
            g.alias
        ));
    }
    if let Some(n) = g.numa_node
        && !d.nodes.contains(&n)
    {
        w.push(crate::tf!(
            "The {} is on socket {n}, but the chosen cores are on {:?}: every frame crosses between the processors.", "A {} está no socket {n}, mas os núcleos escolhidos estão em {:?}: cada quadro atravessa entre os processadores.",
            g.alias, d.nodes
        ));
    }
    if d.nodes.len() > 1 && topo.nodes.len() > 1 {
        w.push(crate::i18n::tr("The game will use both processors: in games this usually tanks the FPS (NUMA latency).").into());
    }
    let visible = if d.limit_visible {
        d.cpus.len()
    } else {
        topo.all_cpus().len()
    };
    if visible > 32 {
        w.push(crate::tf!(
            "The game will see {visible} threads. Many games and Easy Anti-Cheat fail with that many cores: \
             use the \"Game\" profile (limits what it sees through WINE_CPU_TOPOLOGY).", "O jogo vai enxergar {visible} threads. Vários jogos e o Easy Anti-Cheat falham com tantos núcleos: \
             use o perfil \"Jogo\" (limita o que ele enxerga via WINE_CPU_TOPOLOGY)."
        ));
    }
    w
}

/// Installed Vulkan drivers other than NVIDIA's (radeon, intel, lvp...).
pub fn non_nvidia_vk_icds() -> Vec<String> {
    let mut v: Vec<String> = ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"]
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten())
        .map(|e| e.path())
        .filter(|p| {
            let n = p
                .file_name()
                .map(|f| f.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            n.ends_with(".json")
                && !n.contains("nvidia")
                && !n.contains("lvp")
                && !n.contains("i686")
                && !n.contains("i386")
        })
        .map(|p| p.display().to_string())
        .collect();
    v.sort();
    v
}

/// Fallback for DirectX (DXVK: D3D8–11; VKD3D-Proton: D3D12): filter by the card's Vulkan name.
/// Mesa's selector (device-select layer) already hides the other cards, but it does not exist
/// everywhere (NVIDIA driver, some runtimes/Flatpak); without it DXVK picks by itself.
/// Only when the model is unique in the PC and the name was confirmed by vulkaninfo in this
/// environment. `NUMAPILOT_NO_DX_FILTER=1` turns it off; a filter the user already set is kept.
pub fn dx_filter(
    d: &Decision,
    topo: &Topology,
    set: &[(String, String)],
    unset: &[String],
) -> Vec<(String, String)> {
    if d.kind != Kind::Graphics
        || std::env::var_os("NUMAPILOT_NO_DX_FILTER").is_some()
        || std::env::var_os("NUMAPILOT_SYSFS").is_some()
        || std::env::var_os("DXVK_FILTER_DEVICE_NAME").is_some()
    {
        return vec![];
    }
    let Some(g) = d.gpus.first() else {
        return vec![];
    };
    let same = topo
        .gpus
        .iter()
        .filter(|o| o.vendor == g.vendor && o.device == g.device)
        .count();
    // a single card in the PC: nothing to filter; repeated models: the name does not tell them apart
    if topo.gpus.len() < 2 || same != 1 {
        return vec![];
    }
    match crate::gputest::vk_name_for(g, set, unset) {
        Some(n) => vec![
            ("DXVK_FILTER_DEVICE_NAME".into(), n.clone()),
            ("VKD3D_FILTER_DEVICE_NAME".into(), n),
        ],
        None => vec![],
    }
}

/// Like `env_for_gpu`, with the variables to remove as well.
pub fn env_for_gpu_full(
    g: &crate::topo::Gpu,
    topo: &Topology,
) -> (Vec<(String, String)>, Vec<String>) {
    let d = Decision {
        nodes: vec![],
        cpus: vec![],
        gpus: vec![g.clone()],
        kind: Kind::Graphics,
        mem: crate::config::MemSpec::Local,
        manual: true,
        limit_visible: false,
        priority: "normal".into(),
        needs_daemon: false,
        fg_gpu: None,
        reasons: vec![],
    };
    gpu_env(&d, topo)
}

/// gamescope's output card (profile `gamescope.output_gpu` or `run --out-gpu`): "" | "same" |
/// "auto" = the game's card (None); "display" = the first card with a monitor; else alias/PCI.
/// Also None when it is the game's own card or the game does not use a card for graphics.
pub fn resolve_out_gpu(name: &str, d: &Decision, topo: &Topology) -> Result<Option<Gpu>> {
    let out = match name {
        "" | "same" | "auto" => return Ok(None),
        "display" => topo.gpus.iter().find(|g| g.display_connected),
        n => Some(topo.gpus.iter().find(|g| g.matches(n)).with_context(|| {
            crate::tf!(
                "gamescope output GPU '{n}' not found",
                "GPU de saída do gamescope '{n}' não encontrada"
            )
        })?),
    };
    Ok(match (out, d.gpus.first()) {
        (Some(o), Some(g)) if d.kind == Kind::Graphics && o.pci != g.pci => Some(o.clone()),
        _ => None,
    })
}

/// gamescope on one card, the game inside it on another.
#[derive(Debug, Clone, PartialEq)]
pub struct GamescopeSplit {
    /// `--prefer-vk-device` value (vendor:device in hex).
    pub prefer: String,
    /// Environment of the gamescope process (replaces the game's `set`/`unset`).
    pub set: Vec<(String, String)>,
    pub unset: Vec<String>,
    /// `env -u … VAR=value …` put in front of the game's command (after gamescope's `--`), so
    /// the game gets back its own card's variables.
    pub child: Vec<String>,
}

/// Splits the environment: gamescope sees only the output card (DRI_PRIME with the PCI address
/// plus MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE, which also separates the two MI50s with the
/// same vendor:device; `--prefer-vk-device` alone would take the first of them), and the game
/// keeps the card chosen for it. `parent` reads the inherited environment: a variable the output
/// card changes but the game's choice does not touch goes back to its inherited value.
pub fn gamescope_split(
    out: &Gpu,
    topo: &Topology,
    game_set: &[(String, String)],
    game_unset: &[String],
    parent: &dyn Fn(&str) -> Option<String>,
) -> GamescopeSplit {
    let (out_set, out_unset) = env_for_gpu_full(out, topo);
    let touched: Vec<&str> = out_set
        .iter()
        .map(|(k, _)| k.as_str())
        .chain(out_unset.iter().map(String::as_str))
        .collect();
    let mut set: Vec<(String, String)> = game_set
        .iter()
        .filter(|(k, _)| !touched.contains(&k.as_str()))
        .cloned()
        .collect();
    set.extend(out_set.iter().cloned());
    let mut unset: Vec<String> = game_unset
        .iter()
        .filter(|k| !touched.contains(&k.as_str()))
        .cloned()
        .collect();
    unset.extend(out_unset.iter().cloned());
    let mut dels = vec![];
    let mut adds = vec![];
    let mut seen: Vec<&str> = vec![];
    for k in touched {
        if seen.contains(&k) {
            continue;
        }
        seen.push(k);
        let game = if let Some((_, v)) = game_set.iter().rev().find(|(x, _)| x == k) {
            Some(v.clone())
        } else if game_unset.iter().any(|x| x == k) {
            None
        } else {
            parent(k)
        };
        match game {
            Some(v) => adds.push(format!("{k}={v}")),
            None => {
                dels.push("-u".to_string());
                dels.push(k.to_string());
            }
        }
    }
    let mut child = vec!["env".to_string()];
    child.extend(dels);
    child.extend(adds);
    GamescopeSplit {
        prefer: format!("{:04x}:{:04x}", out.vendor, out.device),
        set,
        unset,
        child,
    }
}

/// Variables for a program to run on a single card (graphics), without going through the service.
pub fn env_for_gpu(g: &crate::topo::Gpu, topo: &Topology) -> Vec<(String, String)> {
    let d = Decision {
        nodes: vec![],
        cpus: vec![],
        gpus: vec![g.clone()],
        kind: Kind::Graphics,
        mem: crate::config::MemSpec::Local,
        manual: true,
        limit_visible: false,
        priority: "normal".into(),
        needs_daemon: false,
        fg_gpu: None,
        reasons: vec![],
    };
    gpu_env(&d, topo).0
}

#[cfg(test)]
mod env_tests {
    use super::*;
    use crate::topo::{Gpu, GpuRole};

    fn gpu(alias: &str, pci: &str, vendor: u16, device: u16, render: &str, role: GpuRole) -> Gpu {
        Gpu {
            alias: alias.into(),
            pci: pci.into(),
            vendor,
            device,
            name: alias.into(),
            numa_node: Some(0),
            card: None,
            render: Some(render.into()),
            display_connected: role == GpuRole::Display,
            gfx: None,
            rocr_id: None,
            role,
        }
    }

    /// Laptop: Radeon 680M (screen) + RTX 3070 Ti Laptop.
    fn laptop() -> Topology {
        Topology {
            nodes: vec![],
            gpus: vec![
                gpu(
                    "nvidia-0",
                    "0000:01:00.0",
                    0x10de,
                    0x24a0,
                    "renderD128",
                    GpuRole::Other,
                ),
                gpu(
                    "gfx1035-0",
                    "0000:74:00.0",
                    0x1002,
                    0x1681,
                    "renderD129",
                    GpuRole::Display,
                ),
            ],
            default_graphics: None,
        }
    }

    /// Server: two MI50s with the same vendor:device + RX 470 (monitor).
    fn server() -> Topology {
        Topology {
            nodes: vec![],
            gpus: vec![
                gpu(
                    "gfx906-0",
                    "0000:03:00.0",
                    0x1002,
                    0x66af,
                    "renderD128",
                    GpuRole::Compute,
                ),
                gpu(
                    "gfx906-1",
                    "0000:83:00.0",
                    0x1002,
                    0x66af,
                    "renderD129",
                    GpuRole::Compute,
                ),
                gpu(
                    "gfx803-0",
                    "0000:84:00.0",
                    0x1002,
                    0x67df,
                    "renderD130",
                    GpuRole::Display,
                ),
            ],
            default_graphics: None,
        }
    }

    fn graphics_on(g: &Gpu) -> Decision {
        Decision {
            nodes: vec![],
            cpus: vec![],
            gpus: vec![g.clone()],
            kind: Kind::Graphics,
            mem: crate::config::MemSpec::Local,
            manual: true,
            limit_visible: false,
            priority: "normal".into(),
            needs_daemon: false,
            fg_gpu: None,
            reasons: vec![],
        }
    }

    fn get<'a>(set: &'a [(String, String)], k: &str) -> Option<&'a str> {
        set.iter()
            .rev()
            .find(|(x, _)| x == k)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn gamescope_on_the_other_mi50() {
        let t = server();
        let d = graphics_on(&t.gpus[0]);
        let (set, unset) = gpu_env(&d, &t);
        let out = resolve_out_gpu("gfx906-1", &d, &t).unwrap().unwrap();
        let s = gamescope_split(&out, &t, &set, &unset, &|_| None);
        assert_eq!(s.prefer, "1002:66af");
        // gamescope sees only the second MI50: by PCI, since the ID is the same on both
        assert_eq!(get(&s.set, "DRI_PRIME"), Some("pci-0000_83_00_0"));
        assert_eq!(
            get(&s.set, "MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE"),
            Some("1")
        );
        assert!(s.unset.contains(&"MESA_VK_DEVICE_SELECT".to_string()));
        assert!(get(&s.set, "MESA_VK_DEVICE_SELECT").is_none());
        // the game gets the first one back
        assert_eq!(s.child[0], "env");
        assert!(s.child.contains(&"DRI_PRIME=pci-0000_03_00_0".to_string()));
        assert!(s.child.contains(&"LSFGVK_GPU=3:0.0".to_string()));
        assert!(
            s.child
                .windows(2)
                .any(|w| w[0] == "-u" && w[1] == "MESA_VK_DEVICE_SELECT")
        );
        // options before the assignments
        let first_assign = s.child.iter().position(|a| a.contains('=')).unwrap();
        assert!(s.child[first_assign..].iter().all(|a| a != "-u"));
    }

    #[test]
    fn gamescope_on_a_unique_card_restores_the_inherited_value() {
        let t = server();
        let d = graphics_on(&t.gpus[1]);
        let (set, unset) = gpu_env(&d, &t);
        let out = resolve_out_gpu("display", &d, &t).unwrap().unwrap();
        assert_eq!(out.alias, "gfx803-0");
        let s = gamescope_split(&out, &t, &set, &unset, &|k| {
            (k == "MESA_VK_DEVICE_SELECT").then(|| "1234:5678".to_string())
        });
        assert_eq!(s.prefer, "1002:67df");
        assert_eq!(get(&s.set, "MESA_VK_DEVICE_SELECT"), Some("1002:67df"));
        assert_eq!(get(&s.set, "DRI_PRIME"), Some("pci-0000_84_00_0"));
        // the game (an MI50) had it removed: removed again inside gamescope
        assert!(
            s.child
                .windows(2)
                .any(|w| w[0] == "-u" && w[1] == "MESA_VK_DEVICE_SELECT")
        );
        assert!(s.child.contains(&"DRI_PRIME=pci-0000_83_00_0".to_string()));
    }

    #[test]
    fn gamescope_output_same_card_or_unknown() {
        let t = server();
        let d = graphics_on(&t.gpus[0]);
        assert!(resolve_out_gpu("same", &d, &t).unwrap().is_none());
        assert!(resolve_out_gpu("0000:03:00.0", &d, &t).unwrap().is_none());
        assert!(resolve_out_gpu("gfx906-9", &d, &t).is_err());
        let mut c = d.clone();
        c.kind = Kind::Compute;
        assert!(resolve_out_gpu("gfx906-1", &c, &t).unwrap().is_none());
    }

    /// What a program started from the integrated GPU (Steam from startup) passes on.
    fn inherited_igpu() -> Vec<(String, String)> {
        [
            ("DRI_PRIME", "pci-0000_74_00_0"),
            ("MESA_VK_DEVICE_SELECT", "1002:1681"),
            (
                "VK_DRIVER_FILES",
                "/usr/share/vulkan/icd.d/radeon_icd.x86_64.json",
            ),
            (
                "VK_ICD_FILENAMES",
                "/usr/share/vulkan/icd.d/radeon_icd.x86_64.json",
            ),
            ("HOME", "/home/u"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
    }

    #[test]
    fn nvidia_on_laptop_keeps_vulkan_selection() {
        let t = laptop();
        let (set, unset) = env_for_gpu_full(&t.gpus[0], &t);
        let env = effective_env(&inherited_igpu(), &set, &unset);
        // Mesa's device-select layer must put the NVIDIA first and alone, or programs that take
        // Vulkan device 0 run on the Radeon.
        assert_eq!(
            env.get("MESA_VK_DEVICE_SELECT").map(String::as_str),
            Some("10de:24a0")
        );
        assert_eq!(
            env.get("MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE")
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(
            env.get("DRI_PRIME").map(String::as_str),
            Some("pci-0000_01_00_0")
        );
        assert_eq!(
            env.get("__NV_PRIME_RENDER_OFFLOAD").map(String::as_str),
            Some("1")
        );
        // The inherited ICD list without the NVIDIA must go.
        assert!(!env.contains_key("VK_DRIVER_FILES"));
        assert!(!env.contains_key("VK_ICD_FILENAMES"));
        assert_eq!(env.get("HOME").map(String::as_str), Some("/home/u"));
    }

    #[test]
    fn integrated_on_laptop_hides_nvidia() {
        let t = laptop();
        let (set, unset) = env_for_gpu_full(&t.gpus[1], &t);
        let env = effective_env(
            &[("__NV_PRIME_RENDER_OFFLOAD".into(), "1".into())],
            &set,
            &unset,
        );
        assert_eq!(
            env.get("MESA_VK_DEVICE_SELECT").map(String::as_str),
            Some("1002:1681")
        );
        assert_eq!(
            env.get("DRI_PRIME").map(String::as_str),
            Some("pci-0000_74_00_0")
        );
        assert!(!env.contains_key("__NV_PRIME_RENDER_OFFLOAD"));
    }

    #[test]
    fn never_set_and_removed_together() {
        let t = laptop();
        for g in &t.gpus {
            let (set, unset) = env_for_gpu_full(g, &t);
            for k in &unset {
                assert!(
                    !set.iter().any(|(s, _)| s == k),
                    "{k} is both set and removed for {}",
                    g.alias
                );
            }
        }
    }
}
