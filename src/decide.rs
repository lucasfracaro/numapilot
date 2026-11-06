//! Lógica de decisão compartilhada: qual GPU e qual(is) nó(s) usar.

use crate::config::{Config, CpuSpec, GpuSpec, MemSpec, Rule};
use crate::learned::Learned;
use crate::procfs;
use crate::profiles::Profiles;
use crate::topo::{Gpu, GpuRole, Topology};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Graphics,
    Compute,
    NoGpu,
}

impl Kind {
    /// "graphics"/"jogo" → Gráfico, "compute"/"ia" → Computação, "auto" → None
    pub fn parse_usage(s: &str) -> Result<Option<Kind>> {
        Ok(match s.trim().to_lowercase().as_str() {
            "" | "auto" => None,
            "graphics" | "grafico" | "gráfico" | "jogo" | "game" => Some(Kind::Graphics),
            "compute" | "computacao" | "computação" | "ia" | "rocm" => Some(Kind::Compute),
            o => bail!("uso deve ser auto, graphics ou compute (recebido '{o}')"),
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
    /// GPU do frame generation (lsfg-vk), se diferente da do jogo.
    pub fg_gpu: Option<Gpu>,
    pub reasons: Vec<String>,
}

/// Fração ocupada de cada nó, amostrando /proc/stat durante `dur`.
pub fn sample_node_loads(topo: &Topology, dur: Duration) -> HashMap<usize, f64> {
    let a = procfs::cpu_times();
    std::thread::sleep(dur);
    let b = procfs::cpu_times();
    node_loads(topo, &a, &b)
}

pub fn node_loads(topo: &Topology, a: &[(usize, u64, u64)], b: &[(usize, u64, u64)]) -> HashMap<usize, f64> {
    let prev: HashMap<usize, (u64, u64)> = a.iter().map(|&(c, busy, tot)| (c, (busy, tot))).collect();
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
        out.insert(n.id, if tot > 0 { busy as f64 / tot as f64 } else { 0.0 });
    }
    out
}

/// Nó menos carregado, evitando `avoid` se houver alternativa e exigindo ~5% de RAM livre.
pub fn least_loaded_node(topo: &Topology, loads: &HashMap<usize, f64>, avoid: &[usize]) -> usize {
    let mut cands: Vec<&crate::topo::Node> = topo.nodes.iter().filter(|n| !avoid.contains(&n.id)).collect();
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

/// Escolhe a(s) GPU(s). `want` força o tipo de uso (ex.: jogar numa MI50 = Compute GPU usada como Graphics).
pub fn pick_gpus(topo: &Topology, spec: &GpuSpec, want: Option<Kind>, hint: bool) -> Result<(Vec<Gpu>, Kind)> {
    let least_busy = |n: usize| -> Vec<Gpu> {
        let mut c: Vec<&Gpu> = topo.compute_gpus();
        c.sort_by(|a, b| a.load_score().partial_cmp(&b.load_score()).unwrap_or(std::cmp::Ordering::Equal));
        let take = if n == 0 { c.len() } else { n.min(c.len()) };
        c.into_iter().take(take).cloned().collect()
    };
    Ok(match spec {
        GpuSpec::None => (vec![], Kind::NoGpu),
        GpuSpec::Display => (topo.display_gpu().cloned().into_iter().collect(), Kind::Graphics),
        GpuSpec::Compute(n) => {
            let g = least_busy(*n);
            if g.is_empty() {
                bail!("nenhuma GPU de computação (ROCm) encontrada; veja `numapilot topo`");
            }
            (g, want.unwrap_or(Kind::Compute))
        }
        GpuSpec::Named(names) => {
            let mut v = Vec::new();
            for n in names {
                match topo.gpus.iter().find(|g| g.matches(n)) {
                    Some(g) => v.push(g.clone()),
                    None => bail!("GPU '{n}' não encontrada; veja os apelidos com `numapilot topo`"),
                }
            }
            let role_kind = if v.iter().all(|g| g.role == GpuRole::Compute) && !hint_graphics(want) {
                Kind::Compute
            } else {
                Kind::Graphics
            };
            (v, want.unwrap_or(role_kind))
        }
        GpuSpec::Auto => match want {
            Some(Kind::Graphics) => (topo.display_gpu().cloned().into_iter().collect(), Kind::Graphics),
            Some(Kind::Compute) => (least_busy(1), Kind::Compute),
            _ if hint && !topo.compute_gpus().is_empty() => (least_busy(1), Kind::Compute),
            _ => (topo.display_gpu().cloned().into_iter().collect(), Kind::Graphics),
        },
    })
}

fn hint_graphics(want: Option<Kind>) -> bool {
    want == Some(Kind::Graphics)
}

pub struct LaunchInput<'a> {
    pub idents: &'a [String],
    pub learn_key: &'a str,
    /// Chave do perfil (steam:ID ou executável).
    pub key: &'a str,
    pub cpu: Option<CpuSpec>,
    pub gpu: Option<GpuSpec>,
    pub mem: Option<MemSpec>,
    pub usage: Option<Kind>,
    /// GPU do frame generation do lsfg-vk ("same" = a do jogo).
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
        reasons.push("perfil salvo pela interface gráfica".into());
    } else if let Some(r) = rule {
        reasons.push(format!("regra '{}' do config", r.label()));
    }
    let pick = |cli: Option<String>, prof: Option<&str>, rule: Option<&str>| -> String {
        cli.or_else(|| prof.filter(|s| *s != "auto").map(String::from))
            .or_else(|| rule.map(String::from))
            .unwrap_or_else(|| "auto".into())
    };
    let cpu = match &inp.cpu {
        Some(c) => c.clone(),
        None => CpuSpec::parse(&pick(None, profile.map(|p| p.cpu.as_str()), rule.map(|r| r.cpu.as_str())))?,
    };
    let gpu = match &inp.gpu {
        Some(g) => g.clone(),
        None => GpuSpec::parse(&pick(None, profile.map(|p| p.gpu.as_str()), rule.map(|r| r.gpu.as_str())))?,
    };
    let usage = match inp.usage {
        Some(k) => Some(k),
        None => Kind::parse_usage(&pick(None, profile.map(|p| p.usage.as_str()), rule.map(|r| r.usage.as_str())))?,
    };
    let mem = match inp.mem {
        Some(m) => m,
        None => rule.map(|r| MemSpec::parse(&r.mem)).transpose()?.unwrap_or(MemSpec::Local),
    };
    let hint = cfg.is_compute_hint(inp.idents);
    if gpu == GpuSpec::Auto && usage.is_none() && hint {
        reasons.push("programa conhecido de computação → GPU ROCm".into());
    }
    let (gpus, kind) = pick_gpus(topo, &gpu, usage, hint)?;
    if kind == Kind::Graphics && gpus.iter().any(|g| g.role == GpuRole::Compute) {
        reasons.push(format!(
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
                    bail!("nó {id} não existe (nós: {:?})", all);
                }
            }
            (n.clone(), topo.cpus_of(n))
        }
        CpuSpec::Cpus(c) => {
            let nodes: Vec<usize> =
                topo.nodes.iter().filter(|n| n.cpus.iter().any(|x| c.contains(x))).map(|n| n.id).collect();
            (nodes, c.clone())
        }
        CpuSpec::GpuNode | CpuSpec::Auto => {
            if cpu == CpuSpec::Auto && learned_e.is_some_and(|e| e.wide) {
                reasons.push("aprendido: este programa rende mais com os 2 sockets".into());
                (all.clone(), topo.all_cpus())
            } else if !gpu_nodes.is_empty() {
                if gpu_nodes.len() > 1 {
                    reasons.push("GPUs escolhidas estão em sockets diferentes → todos os nós".into());
                } else {
                    reasons.push(format!("nó {} é o socket ligado à GPU", gpu_nodes[0]));
                }
                (gpu_nodes.clone(), topo.cpus_of(&gpu_nodes))
            } else {
                let n = least_loaded_node(topo, loads, &[]);
                reasons.push(format!(
                    "nó {n} é o menos ocupado ({:.0}% de uso)",
                    loads.get(&n).copied().unwrap_or(0.0) * 100.0
                ));
                (vec![n], topo.cpus_of(&[n]))
            }
        }
    };
    if cpus.is_empty() {
        bail!("nenhum CPU selecionado");
    }
    let fg_name = inp.fg_gpu.clone().or_else(|| profile.map(|p| p.fg_gpu.clone())).unwrap_or_else(|| "same".into());
    let fg_gpu = match fg_name.as_str() {
        "" | "same" | "auto" => None,
        n => match topo.gpus.iter().find(|g| g.matches(n)) {
            Some(g) if gpus.first().is_none_or(|x| x.pci != g.pci) => {
                reasons.push(format!("frame generation (lsfg-vk) na {}: requer lsfg-vk com dual GPU", g.alias));
                Some(g.clone())
            }
            Some(_) => None,
            None => bail!("GPU de frame generation '{n}' não encontrada"),
        },
    };
    Ok(Decision { nodes, cpus, gpus, kind, mem, manual: cpu != CpuSpec::Auto, fg_gpu, reasons })
}

/// "0000:83:00.0" → "131:0.0" (formato de barramento PCI em decimal usado pelo lsfg-vk)
pub fn pci_decimal(pci: &str) -> String {
    let p: Vec<&str> = pci.split([':', '.']).collect();
    if p.len() == 4 {
        let h = |s: &str| u32::from_str_radix(s, 16).unwrap_or(0);
        format!("{}:{}.{}", h(p[1]), h(p[2]), h(p[3]))
    } else {
        pci.to_string()
    }
}

/// Variáveis de ambiente para direcionar o programa à(s) GPU(s) escolhida(s).
pub fn gpu_env(d: &Decision, topo: &Topology) -> (Vec<(String, String)>, Vec<String>) {
    let mut set = Vec::new();
    let mut unset = Vec::new();
    match d.kind {
        Kind::Graphics => {
            if let Some(g) = d.gpus.first() {
                // OpenGL e Vulkan (Mesa) escolhem a placa por DRI_PRIME=pci-...
                set.push(("DRI_PRIME".into(), g.dri_prime()));
                // vendor:device só serve se for único (as duas MI50 têm o mesmo ID).
                let same = topo.gpus.iter().filter(|o| o.vendor == g.vendor && o.device == g.device).count();
                if same == 1 {
                    set.push(("MESA_VK_DEVICE_SELECT".into(), format!("{:04x}:{:04x}", g.vendor, g.device)));
                } else {
                    unset.push("MESA_VK_DEVICE_SELECT".into());
                }
                // Lossless Scaling (lsfg-vk): por padrão gera quadros na mesma GPU do jogo
                // (o lsfg-vk oficial exige isso); com perfil dual GPU, na GPU escolhida.
                let fg = d.fg_gpu.as_ref().unwrap_or(g);
                set.push(("LSFGVK_GPU".into(), pci_decimal(&fg.pci)));
                set.push(("NUMAPILOT_FG_GPU".into(), fg.pci.clone()));
            }
        }
        Kind::Compute => {
            let ids: Vec<String> = d.gpus.iter().filter_map(|g| g.rocr_id.clone()).collect();
            if !ids.is_empty() {
                set.push(("ROCR_VISIBLE_DEVICES".into(), ids.join(",")));
                // Índices HIP/CUDA herdados apontariam para a lista já filtrada.
                unset.push("HIP_VISIBLE_DEVICES".into());
                unset.push("CUDA_VISIBLE_DEVICES".into());
            }
        }
        Kind::NoGpu => {}
    }
    let renders: Vec<String> =
        d.gpus.iter().filter_map(|g| g.render.as_ref().map(|r| format!("/dev/dri/{r}"))).collect();
    set.push(("NUMAPILOT_RENDER".into(), renders.join(" ")));
    set.push(("NUMAPILOT_GPUS".into(), d.gpus.iter().map(|g| g.alias.clone()).collect::<Vec<_>>().join(",")));
    set.push(("NUMAPILOT_GPU_PCI".into(), d.gpus.iter().map(|g| g.pci.clone()).collect::<Vec<_>>().join(",")));
    set.push(("NUMAPILOT_CPUS".into(), crate::util::format_cpulist(&d.cpus)));
    set.push((
        "NUMAPILOT_MEMS".into(),
        d.nodes.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(","),
    ));
    set.push(("NUMAPILOT_MODE".into(), if d.manual { "manual" } else { "auto" }.into()));
    (set, unset)
}
