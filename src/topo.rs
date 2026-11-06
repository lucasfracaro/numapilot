//! Detecção da topologia: nós NUMA, núcleos e GPUs (com o nó em que cada uma está ligada).

use crate::config::Config;
use crate::util::{parse_cpulist, read_trim, read_u64, sys};
use std::collections::{BTreeSet, HashMap};
use std::fs;

#[derive(Debug, Clone)]
pub struct Node {
    pub id: usize,
    pub cpus: Vec<usize>,
    /// Núcleos físicos, cada um com seus CPUs lógicos (irmãos de hyper-threading).
    pub cores: Vec<Vec<usize>>,
    pub physical_cores: usize,
    pub sockets: BTreeSet<u32>,
    pub mem_total_kb: u64,
    pub distances: Vec<u32>,
}

impl Node {
    pub fn mem_free_kb(&self) -> u64 {
        node_meminfo(self.id, "MemFree").unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuRole {
    /// Tem monitor ligado: é a GPU de vídeo / jogos.
    Display,
    /// GPU de computação ROCm (ex.: MI50, gfx906), sem monitor.
    Compute,
    /// Outras (ex.: placa sem monitor que não serve para ROCm).
    Other,
}

#[derive(Debug, Clone)]
pub struct Gpu {
    pub alias: String,
    pub pci: String,
    pub vendor: u16,
    pub device: u16,
    pub name: String,
    pub numa_node: Option<usize>,
    pub card: Option<String>,
    pub render: Option<String>,
    /// Tem monitor ligado nesta placa.
    pub display_connected: bool,
    pub gfx: Option<String>,
    /// Identificador para ROCR_VISIBLE_DEVICES (UUID "GPU-..." ou índice).
    pub rocr_id: Option<String>,
    pub role: GpuRole,
}

impl Gpu {
    pub fn busy_percent(&self) -> Option<u64> {
        read_u64(sys(&format!("bus/pci/devices/{}/gpu_busy_percent", self.pci)))
    }
    pub fn vram_used_total(&self) -> Option<(u64, u64)> {
        let u = read_u64(sys(&format!("bus/pci/devices/{}/mem_info_vram_used", self.pci)))?;
        let t = read_u64(sys(&format!("bus/pci/devices/{}/mem_info_vram_total", self.pci)))?;
        Some((u, t))
    }
    /// Pontuação de ocupação (menor = mais livre).
    pub fn load_score(&self) -> f64 {
        let busy = self.busy_percent().unwrap_or(0) as f64;
        let vram = self
            .vram_used_total()
            .map(|(u, t)| if t > 0 { u as f64 * 100.0 / t as f64 } else { 0.0 })
            .unwrap_or(0.0);
        busy + vram
    }
    /// Formato aceito pelo Mesa em DRI_PRIME: pci-0000_03_00_0
    pub fn dri_prime(&self) -> String {
        format!("pci-{}", self.pci.replace([':', '.'], "_"))
    }
    pub fn matches(&self, name: &str) -> bool {
        let n = name.to_lowercase();
        self.alias.to_lowercase() == n || self.pci == n || self.pci.trim_start_matches("0000:") == n
    }
}

#[derive(Debug, Clone)]
pub struct Topology {
    pub nodes: Vec<Node>,
    pub gpus: Vec<Gpu>,
    /// GPU padrão para gráficos/jogos no automático (config `graphics_gpu`).
    pub default_graphics: Option<String>,
}

impl Topology {
    pub fn load(cfg: &Config) -> Topology {
        let online_cpus = read_trim(sys("devices/system/cpu/online"))
            .map(|s| parse_cpulist(&s))
            .unwrap_or_default();
        let mut nodes = load_nodes();
        if nodes.is_empty() {
            // Kernel sem NUMA: um único "nó" com tudo.
            nodes.push(Node {
                id: 0,
                cpus: online_cpus.clone(),
                cores: group_cores(&online_cpus),
                physical_cores: count_physical(&online_cpus),
                sockets: sockets_of(&online_cpus),
                mem_total_kb: 0,
                distances: vec![10],
            });
        }
        let gpus = load_gpus(cfg, nodes.len());
        Topology { nodes, gpus, default_graphics: cfg.graphics_gpu.clone() }
    }

    pub fn node(&self, id: usize) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_ids(&self) -> Vec<usize> {
        self.nodes.iter().map(|n| n.id).collect()
    }

    pub fn cpus_of(&self, node_ids: &[usize]) -> Vec<usize> {
        let mut v: Vec<usize> = self
            .nodes
            .iter()
            .filter(|n| node_ids.contains(&n.id))
            .flat_map(|n| n.cpus.iter().copied())
            .collect();
        v.sort_unstable();
        v
    }

    pub fn all_cpus(&self) -> Vec<usize> {
        self.cpus_of(&self.node_ids())
    }

    /// Escolhe CPUs dentro dos nós `nodes`.
    /// - `count`: quantos CPUs lógicos (None = todos);
    /// - `smt`: se pode usar os dois "threads" de cada núcleo (senão, 1 por núcleo físico);
    /// - `avoid`: CPUs a evitar se houver alternativa (ex.: reservados por outro programa).
    ///
    /// Com vários nós, distribui os núcleos alternando entre eles.
    pub fn select_cpus(&self, nodes: &[usize], count: Option<usize>, smt: bool, avoid: &[usize]) -> Vec<usize> {
        let units_of = |n: &Node, free: bool| -> Vec<Vec<usize>> {
            n.cores
                .iter()
                .filter(|core| core.iter().all(|c| !avoid.contains(c)) == free)
                .map(|core| if smt { core.clone() } else { core[..1].to_vec() })
                .collect()
        };
        let sel_nodes: Vec<&Node> = self.nodes.iter().filter(|n| nodes.contains(&n.id)).collect();
        // Primeiro os núcleos livres, depois (se faltar) os evitados.
        let per_node: Vec<Vec<Vec<usize>>> = sel_nodes
            .iter()
            .map(|n| {
                let mut u = units_of(n, true);
                u.extend(units_of(n, false));
                u
            })
            .collect();
        let total: usize = per_node.iter().map(|u| u.iter().map(Vec::len).sum::<usize>()).sum();
        let want = count.unwrap_or(total).min(total);
        let mut out = Vec::with_capacity(want);
        let mut idx = vec![0usize; per_node.len()];
        'outer: while out.len() < want {
            let mut progressed = false;
            for (i, units) in per_node.iter().enumerate() {
                if let Some(u) = units.get(idx[i]) {
                    idx[i] += 1;
                    progressed = true;
                    for &c in u {
                        if out.len() >= want {
                            break 'outer;
                        }
                        out.push(c);
                    }
                }
            }
            if !progressed {
                break;
            }
        }
        out.sort_unstable();
        out
    }

    /// Nós que contêm algum dos CPUs.
    pub fn nodes_of(&self, cpus: &[usize]) -> Vec<usize> {
        self.nodes.iter().filter(|n| n.cpus.iter().any(|c| cpus.contains(c))).map(|n| n.id).collect()
    }

    /// GPU para gráficos no automático: a do config; senão a placa de vídeo com
    /// monitor; senão qualquer uma com monitor (ex.: MI50 com saída de vídeo).
    pub fn display_gpu(&self) -> Option<&Gpu> {
        if let Some(name) = &self.default_graphics
            && let Some(g) = self.gpus.iter().find(|g| g.matches(name))
        {
            return Some(g);
        }
        self.gpus
            .iter()
            .find(|g| g.role == GpuRole::Display)
            .or_else(|| self.gpus.iter().find(|g| g.display_connected))
            .or_else(|| self.gpus.iter().find(|g| g.role == GpuRole::Other))
    }

    pub fn compute_gpus(&self) -> Vec<&Gpu> {
        self.gpus.iter().filter(|g| g.role == GpuRole::Compute).collect()
    }

    pub fn gpu_by_render(&self, render: &str) -> Option<&Gpu> {
        self.gpus
            .iter()
            .find(|g| g.render.as_deref() == Some(render) || g.card.as_deref() == Some(render))
    }

    /// Avisos úteis (BIOS com NUMA desligado, GPU sem nó conhecido...).
    pub fn warnings(&self) -> Vec<String> {
        let mut w = Vec::new();
        let all_sockets: BTreeSet<u32> = self.nodes.iter().flat_map(|n| n.sockets.iter().copied()).collect();
        if self.nodes.len() == 1 && all_sockets.len() > 1 {
            w.push(format!(
                "{} sockets mas só 1 nó NUMA: provavelmente 'Node Interleaving' está ligado na BIOS. \
                 Desligue-o (deixe NUMA ativo) para este programa ter efeito.",
                all_sockets.len()
            ));
        }
        if self.nodes.len() > 1 {
            for g in &self.gpus {
                if g.numa_node.is_none() {
                    w.push(format!(
                        "GPU {} ({}) sem nó NUMA informado pelo firmware. Defina em [gpu_node] no /etc/numapilot.toml.",
                        g.alias, g.pci
                    ));
                }
            }
        }
        w
    }
}

fn node_meminfo(id: usize, key: &str) -> Option<u64> {
    let s = fs::read_to_string(sys(&format!("devices/system/node/node{id}/meminfo"))).ok()?;
    for line in s.lines() {
        // "Node 0 MemTotal:       65843044 kB"
        let mut it = line.split_whitespace();
        let (_, _, k, v) = (it.next(), it.next(), it.next()?, it.next()?);
        if k.trim_end_matches(':') == key {
            return v.parse().ok();
        }
    }
    None
}

fn cpu_topo(cpu: usize, f: &str) -> Option<u32> {
    read_trim(sys(&format!("devices/system/cpu/cpu{cpu}/topology/{f}")))?.parse().ok()
}

/// Agrupa CPUs lógicos por núcleo físico (pacote + core_id), na ordem do 1º CPU de cada núcleo.
pub fn group_cores(cpus: &[usize]) -> Vec<Vec<usize>> {
    let mut map: std::collections::BTreeMap<(u32, u32), Vec<usize>> = std::collections::BTreeMap::new();
    for &c in cpus {
        let k = (cpu_topo(c, "physical_package_id").unwrap_or(0), cpu_topo(c, "core_id").unwrap_or(c as u32));
        map.entry(k).or_default().push(c);
    }
    let mut v: Vec<Vec<usize>> = map
        .into_values()
        .map(|mut x| {
            x.sort_unstable();
            x
        })
        .collect();
    v.sort_by_key(|x| x[0]);
    v
}

fn count_physical(cpus: &[usize]) -> usize {
    let set: BTreeSet<(u32, u32)> = cpus
        .iter()
        .map(|&c| (cpu_topo(c, "physical_package_id").unwrap_or(0), cpu_topo(c, "core_id").unwrap_or(c as u32)))
        .collect();
    set.len().max(1)
}

fn sockets_of(cpus: &[usize]) -> BTreeSet<u32> {
    cpus.iter().filter_map(|&c| cpu_topo(c, "physical_package_id")).collect()
}

fn load_nodes() -> Vec<Node> {
    let mut nodes = Vec::new();
    let Ok(rd) = fs::read_dir(sys("devices/system/node")) else { return nodes };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(id) = name.strip_prefix("node").and_then(|s| s.parse::<usize>().ok()) else { continue };
        let cpus = read_trim(e.path().join("cpulist")).map(|s| parse_cpulist(&s)).unwrap_or_default();
        if cpus.is_empty() {
            continue; // nó só de memória: não usamos para colocar processos
        }
        let distances = read_trim(e.path().join("distance"))
            .map(|s| s.split_whitespace().filter_map(|x| x.parse().ok()).collect())
            .unwrap_or_default();
        nodes.push(Node {
            id,
            cores: group_cores(&cpus),
            physical_cores: count_physical(&cpus),
            sockets: sockets_of(&cpus),
            mem_total_kb: node_meminfo(id, "MemTotal").unwrap_or(0),
            cpus,
            distances,
        });
    }
    nodes.sort_by_key(|n| n.id);
    nodes
}

/// Dados do driver KFD (ROCm) por endereço PCI: (gfx, rocr_id)
fn load_kfd() -> HashMap<String, (String, String)> {
    let mut out = HashMap::new();
    let Ok(rd) = fs::read_dir(sys("class/kfd/kfd/topology/nodes")) else { return out };
    let mut ids: Vec<usize> = rd
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse().ok())
        .collect();
    ids.sort_unstable();
    let mut gpu_index = 0usize;
    for id in ids {
        let Ok(props) = fs::read_to_string(sys(&format!("class/kfd/kfd/topology/nodes/{id}/properties"))) else {
            continue;
        };
        let p: HashMap<&str, u64> = props
            .lines()
            .filter_map(|l| {
                let mut it = l.split_whitespace();
                Some((it.next()?, it.next()?.parse().ok()?))
            })
            .collect();
        if p.get("simd_count").copied().unwrap_or(0) == 0 {
            continue; // nó de CPU
        }
        let loc = p.get("location_id").copied().unwrap_or(0);
        let dom = p.get("domain").copied().unwrap_or(0);
        let pci = format!("{:04x}:{:02x}:{:02x}.{:x}", dom, (loc >> 8) & 0xff, (loc >> 3) & 0x1f, loc & 0x7);
        let v = p.get("gfx_target_version").copied().unwrap_or(0);
        let gfx = format!("gfx{}{:x}{:x}", v / 10000, (v / 100) % 100, v % 100);
        let uid = p.get("unique_id").copied().unwrap_or(0);
        let rocr = if uid != 0 { format!("GPU-{uid:016x}") } else { gpu_index.to_string() };
        out.insert(pci, (gfx, rocr));
        gpu_index += 1;
    }
    out
}

fn pci_name(vendor: u16, device: u16) -> Option<String> {
    let db = fs::read_to_string("/usr/share/hwdata/pci.ids").ok()?;
    let v = format!("{vendor:04x}");
    let d = format!("{device:04x}");
    let mut in_vendor = false;
    for line in db.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            in_vendor = line.starts_with(&v);
        } else if in_vendor && !line.starts_with("\t\t") {
            let l = line.trim_start();
            if l.starts_with(&d) {
                return Some(l[4..].trim().to_string());
            }
        }
    }
    None
}

fn load_gpus(cfg: &Config, node_count: usize) -> Vec<Gpu> {
    let kfd = load_kfd();
    let mut gpus = Vec::new();
    let Ok(rd) = fs::read_dir(sys("bus/pci/devices")) else { return gpus };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let pci = e.file_name().to_string_lossy().to_string();
        let path = e.path();
        let class = read_trim(path.join("class")).unwrap_or_default();
        if !class.starts_with("0x03") {
            continue;
        }
        let hex = |f: &str| {
            read_trim(path.join(f))
                .and_then(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                .unwrap_or(0)
        };
        let (vendor, device) = (hex("vendor"), hex("device"));
        let mut card = None;
        let mut render = None;
        if let Ok(drm) = fs::read_dir(path.join("drm")) {
            for d in drm.flatten() {
                let n = d.file_name().to_string_lossy().to_string();
                if n.starts_with("renderD") {
                    render = Some(n);
                } else if n.starts_with("card") {
                    card = Some(n);
                }
            }
        }
        let display_connected = card.as_ref().is_some_and(|c| connector_connected(c));
        let numa_node = cfg
            .gpu_node
            .get(&pci)
            .copied()
            .or_else(|| {
                read_trim(path.join("numa_node"))
                    .and_then(|s| s.parse::<i64>().ok())
                    .filter(|&n| n >= 0)
                    .map(|n| n as usize)
            })
            .or(if node_count == 1 { Some(0) } else { None });
        let (gfx, rocr_id) = match kfd.get(&pci) {
            Some((g, r)) => (Some(g.clone()), Some(r.clone())),
            None => (None, None),
        };
        let gfx_major: u32 = gfx
            .as_deref()
            .and_then(|g| g.strip_prefix("gfx"))
            .and_then(|g| g.get(..g.len().saturating_sub(2)))
            .and_then(|m| m.parse().ok())
            .unwrap_or(0);
        // Placas com ROCm moderno (gfx9+, ex.: MI50) são "computação" mesmo com monitor ligado;
        // elas continuam podendo rodar jogos (uso gráfico).
        let role = if gfx.is_some() && gfx_major >= 9 {
            GpuRole::Compute
        } else if display_connected {
            GpuRole::Display
        } else {
            GpuRole::Other
        };
        let name = pci_name(vendor, device).unwrap_or_else(|| format!("{vendor:04x}:{device:04x}"));
        gpus.push(Gpu {
            alias: String::new(),
            pci,
            vendor,
            device,
            name,
            numa_node,
            card,
            render,
            display_connected,
            gfx,
            rocr_id,
            role,
        });
    }
    // Apelidos: os do config; senão "gfx906-0", "gfx906-1", "gfx803-0"...; senão "gpuN".
    let mut counters: HashMap<String, usize> = HashMap::new();
    for (i, g) in gpus.iter_mut().enumerate() {
        g.alias = if let Some(a) = cfg.aliases.get(&g.pci) {
            a.clone()
        } else if let Some(gfx) = &g.gfx {
            let c = counters.entry(gfx.clone()).or_insert(0);
            let a = format!("{gfx}-{c}");
            *c += 1;
            a
        } else {
            format!("gpu{i}")
        };
    }
    gpus
}

fn connector_connected(card: &str) -> bool {
    let Ok(rd) = fs::read_dir(sys("class/drm")) else { return false };
    let prefix = format!("{card}-");
    rd.flatten().any(|e| {
        e.file_name().to_string_lossy().starts_with(&prefix)
            && read_trim(e.path().join("status")).as_deref() == Some("connected")
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::config::{Config, CpuSpec, GpuSpec};
    use crate::decide::{decide_launch, gpu_env, Kind, LaunchInput};
    use crate::learned::Learned;
    use crate::profiles::{Profile, Profiles};
    use std::path::Path;

    fn w(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    /// Simula a máquina: 2x E5-2696 v4 (22c/44t cada), RX470 + 2x MI50.
    pub fn fake_sysfs(root: &Path) {
        let _ = fs::remove_dir_all(root);
        w(root, "devices/system/cpu/online", "0-87\n");
        for cpu in 0..88usize {
            let (pkg, core) = if (0..22).contains(&cpu) || (44..66).contains(&cpu) { (0, cpu % 22) } else { (1, cpu % 22) };
            w(root, &format!("devices/system/cpu/cpu{cpu}/topology/physical_package_id"), &format!("{pkg}\n"));
            w(root, &format!("devices/system/cpu/cpu{cpu}/topology/core_id"), &format!("{core}\n"));
        }
        for (n, cpus, dist) in [(0, "0-21,44-65", "10 21"), (1, "22-43,66-87", "21 10")] {
            w(root, &format!("devices/system/node/node{n}/cpulist"), &format!("{cpus}\n"));
            w(root, &format!("devices/system/node/node{n}/distance"), &format!("{dist}\n"));
            w(root, &format!("devices/system/node/node{n}/meminfo"),
              &format!("Node {n} MemTotal:       67108864 kB\nNode {n} MemFree:        50331648 kB\n"));
        }
        let gpus = [
            ("0000:03:00.0", "0x67df", "0", "card0", "renderD128", "20"),
            ("0000:04:00.0", "0x66a1", "0", "card1", "renderD129", "95"),
            ("0000:83:00.0", "0x66a1", "1", "card2", "renderD130", "0"),
        ];
        for (pci, dev, node, card, render, busy) in gpus {
            let base = format!("bus/pci/devices/{pci}");
            w(root, &format!("{base}/class"), if dev == "0x67df" { "0x030000\n" } else { "0x038000\n" });
            w(root, &format!("{base}/vendor"), "0x1002\n");
            w(root, &format!("{base}/device"), &format!("{dev}\n"));
            w(root, &format!("{base}/numa_node"), &format!("{node}\n"));
            w(root, &format!("{base}/gpu_busy_percent"), &format!("{busy}\n"));
            w(root, &format!("{base}/drm/{card}/dev"), "x");
            w(root, &format!("{base}/drm/{render}/dev"), "x");
        }
        w(root, "bus/pci/devices/0000:00:1f.0/class", "0x060100\n");
        w(root, "class/drm/card0-DP-1/status", "connected\n");
        w(root, "class/drm/card1-DP-1/status", "disconnected\n");
        let kfd = [
            (0, "simd_count 0\n"),
            (1, "simd_count 0\n"),
            (2, "simd_count 128\ngfx_target_version 80003\nlocation_id 768\ndomain 0\nunique_id 0\n"),
            (3, "simd_count 240\ngfx_target_version 90006\nlocation_id 1024\ndomain 0\nunique_id 1311768467463790320\n"),
            (4, "simd_count 240\ngfx_target_version 90006\nlocation_id 33536\ndomain 0\nunique_id 81985529216486895\n"),
        ];
        for (id, p) in kfd {
            w(root, &format!("class/kfd/kfd/topology/nodes/{id}/properties"), p);
        }
    }

    #[test]
    fn fake_machine_end_to_end() {
        let root = std::env::temp_dir().join("numapilot-fake-sysfs");
        fake_sysfs(&root);
        unsafe { std::env::set_var("NUMAPILOT_SYSFS", &root) };
        let cfg: Config = toml::from_str(include_str!("../packaging/numapilot.toml")).unwrap();
        let t = Topology::load(&cfg);

        assert_eq!(t.nodes.len(), 2);
        assert_eq!(t.nodes[0].cpus.len(), 44);
        assert_eq!(t.nodes[0].physical_cores, 22);
        assert_eq!(t.nodes[1].sockets.iter().copied().collect::<Vec<_>>(), vec![1]);
        assert!(t.warnings().is_empty(), "{:?}", t.warnings());

        // Núcleos físicos com seus irmãos de HT (0 e 44 são o mesmo núcleo).
        assert_eq!(t.nodes[0].cores.len(), 22);
        assert_eq!(t.nodes[0].cores[0], vec![0, 44]);
        // 8 CPUs no socket 1 com HT = 4 núcleos inteiros.
        assert_eq!(t.select_cpus(&[1], Some(8), true, &[]), vec![22, 23, 24, 25, 66, 67, 68, 69]);
        // 8 CPUs sem HT = 8 núcleos físicos diferentes.
        assert_eq!(t.select_cpus(&[1], Some(8), false, &[]), vec![22, 23, 24, 25, 26, 27, 28, 29]);
        // Nos dois sockets, alterna: 2 núcleos de cada.
        assert_eq!(t.select_cpus(&[0, 1], Some(4), false, &[]), vec![0, 1, 22, 23]);
        // Evita CPUs reservados quando possível.
        assert_eq!(t.select_cpus(&[0], Some(2), false, &[0, 1]), vec![2, 3]);
        // Sem quantidade: todos (ou só um por núcleo, sem HT).
        assert_eq!(t.select_cpus(&[0], None, true, &[]).len(), 44);
        assert_eq!(t.select_cpus(&[0], None, false, &[]).len(), 22);
        assert_eq!(t.nodes_of(&[3, 70]), vec![0, 1]);

        assert_eq!(t.gpus.len(), 3);
        let a: Vec<_> = t.gpus.iter().map(|g| (g.alias.as_str(), g.role, g.numa_node)).collect();
        assert_eq!(a, vec![
            ("gfx803-0", GpuRole::Display, Some(0)),
            ("gfx906-0", GpuRole::Compute, Some(0)),
            ("gfx906-1", GpuRole::Compute, Some(1)),
        ]);
        assert_eq!(t.gpus[0].rocr_id.as_deref(), Some("0"));
        assert_eq!(t.gpus[1].rocr_id.as_deref(), Some("GPU-123456789abcdef0"));
        assert_eq!(t.gpus[0].dri_prime(), "pci-0000_03_00_0");
        assert_eq!(t.gpu_by_render("renderD130").unwrap().alias, "gfx906-1");

        let learned = Learned::default();
        let loads: HashMap<usize, f64> = [(0, 0.9), (1, 0.1)].into();
        let no_profiles = Profiles::default();
        let run_full = |idents: &[&str], cpu: Option<&str>, gpu: Option<&str>, usage: Option<Kind>,
                        learned: &Learned, profiles: &Profiles| {
            let idents: Vec<String> = idents.iter().map(|s| s.to_string()).collect();
            let inp = LaunchInput {
                idents: &idents,
                learn_key: "/usr/bin/x",
                key: "steam:1091500",
                cpu: cpu.map(|c| CpuSpec::parse(c).unwrap()),
                gpu: gpu.map(|g| GpuSpec::parse(g).unwrap()),
                mem: None,
                usage,
                fg_gpu: None,
            };
            decide_launch(&cfg, &t, learned, profiles, &loads, &inp).unwrap()
        };
        let run = |idents: &[&str], cpu: Option<&str>, gpu: Option<&str>, learned: &Learned| {
            run_full(idents, cpu, gpu, None, learned, &no_profiles)
        };

        // Jogo: regra "jogos" → GPU de vídeo e socket dela.
        let d = run(&["steam"], None, None, &learned);
        assert_eq!((d.kind, d.nodes.clone(), d.manual), (Kind::Graphics, vec![0], true));
        let (env, _) = gpu_env(&d, &t);
        assert!(env.contains(&("DRI_PRIME".into(), "pci-0000_03_00_0".into())));
        assert!(env.contains(&("MESA_VK_DEVICE_SELECT".into(), "1002:67df".into())));

        // llama-server: regra IA → MI50 menos ocupada (a do nó 1, a outra está 95%).
        let d = run(&["llama-server"], None, None, &learned);
        assert_eq!((d.kind, d.nodes.clone()), (Kind::Compute, vec![1]));
        assert_eq!(d.gpus[0].alias, "gfx906-1");
        let (env, unset) = gpu_env(&d, &t);
        assert!(env.iter().any(|(k, v)| k == "ROCR_VISIBLE_DEVICES" && v == "GPU-0123456789abcdef"));
        assert!(unset.contains(&"HIP_VISIBLE_DEVICES".to_string()));

        // As duas MI50 (sockets diferentes) → todos os nós.
        let d = run(&["llama-server"], None, Some("compute:all"), &learned);
        assert_eq!(d.nodes, vec![0, 1]);

        // Programa desconhecido sem GPU → nó menos ocupado (1).
        let d = run(&["meuapp"], None, Some("none"), &learned);
        assert_eq!((d.nodes.clone(), d.manual), (vec![1], false));

        // Desconhecido, automático → GPU de vídeo e o socket dela.
        let d = run(&["meuapp"], None, None, &learned);
        assert_eq!((d.kind, d.nodes.clone()), (Kind::Graphics, vec![0]));

        // Manual: escolher GPU e socket à mão.
        let d = run(&["meuapp"], Some("1"), Some("gfx906-0"), &learned);
        assert_eq!((d.nodes.clone(), d.gpus[0].alias.as_str(), d.manual), (vec![1], "gfx906-0", true));

        // Aprendido como "2 sockets" → automático já começa com todos.
        let mut l2 = Learned::default();
        l2.update("/usr/bin/x", |e| e.wide = true);
        let d = run(&["meuapp"], None, Some("none"), &l2);
        assert_eq!(d.nodes, vec![0, 1]);
        assert_eq!(d.cpus.len(), 88);

        // Regra all (blender).
        let d = run(&["blender"], None, None, &learned);
        assert_eq!(d.nodes, vec![0, 1]);

        // Jogo numa MI50 (uso gráfico): DRI_PRIME na MI50, sem MESA_VK_DEVICE_SELECT
        // (as duas MI50 têm o mesmo ID), lsfg-vk na mesma GPU, socket da MI50.
        let d = run_full(&["meujogo"], None, Some("gfx906-1"), Some(Kind::Graphics), &learned, &no_profiles);
        assert_eq!((d.kind, d.nodes.clone()), (Kind::Graphics, vec![1]));
        let (env, unset) = gpu_env(&d, &t);
        assert!(env.contains(&("DRI_PRIME".into(), "pci-0000_83_00_0".into())));
        assert!(env.contains(&("LSFGVK_GPU".into(), "131:0.0".into())));
        assert!(!env.iter().any(|(k, _)| k == "MESA_VK_DEVICE_SELECT" || k == "ROCR_VISIBLE_DEVICES"));
        assert!(unset.contains(&"MESA_VK_DEVICE_SELECT".to_string()));

        // Perfil salvo pela interface vence a regra "jogos" do config.
        let mut profs = Profiles::default();
        profs.map.insert(
            "steam:1091500".into(),
            Profile { label: "Cyberpunk2077.exe".into(), gpu: "gfx906-0".into(), usage: "graphics".into(), fg_gpu: "gfx906-1".into(), ..Default::default() },
        );
        let d = run_full(&["Cyberpunk2077.exe"], None, None, None, &learned, &profs);
        assert_eq!((d.kind, d.gpus[0].alias.as_str(), d.nodes.clone(), d.manual), (Kind::Graphics, "gfx906-0", vec![0], true)); // cpu auto no perfil → vale a regra "jogos" (socket da GPU)
        let (env, _) = gpu_env(&d, &t);
        assert!(env.contains(&("LSFGVK_GPU".into(), "131:0.0".into())), "frame gen na outra MI50");

        // MI50 com monitor ligado continua sendo GPU de computação (e pode jogar direto).
        w(&root, "class/drm/card2-DP-1/status", "connected\n");
        let t2 = Topology::load(&cfg);
        let mi = t2.gpus.iter().find(|g| g.alias == "gfx906-1").unwrap();
        assert!(mi.display_connected && mi.role == GpuRole::Compute);
        assert_eq!(t2.compute_gpus().len(), 2);
        assert_eq!(t2.display_gpu().unwrap().alias, "gfx803-0");
        // Perfil com 8 núcleos físicos no socket da GPU, memória estrita e "enxergar" só eles.
        let mut profs2 = Profiles::default();
        profs2.map.insert(
            "steam:1091500".into(),
            Profile { gpu: "gfx906-1".into(), usage: "graphics".into(), cpu: "gpu".into(), cores: Some(8), smt: false,
                      memory: "strict".into(), priority: "high".into(), ..Default::default() },
        );
        let d = run_full(&["Game.exe"], None, None, None, &learned, &profs2);
        assert_eq!(d.cpus, vec![22, 23, 24, 25, 26, 27, 28, 29]);
        assert_eq!(d.mem, crate::config::MemSpec::Bind);
        assert!(d.needs_daemon && d.limit_visible);
        let (env, _) = gpu_env(&d, &t);
        assert!(env.contains(&("WINE_CPU_TOPOLOGY".into(), "8:22,23,24,25,26,27,28,29".into())));
        assert!(env.contains(&("OMP_NUM_THREADS".into(), "8".into())));
        assert!(env.contains(&("MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE".into(), "1".into())));
        // gfx906-1 sem monitor (no fake ainda) → aviso de renderizar numa placa e exibir em outra.
        let w = crate::decide::game_warnings(&t, &d);
        assert!(w.iter().any(|x| x.contains("monitor")), "{w:?}");
        assert!(!w.iter().any(|x| x.contains("threads")), "8 threads não deveria avisar: {w:?}");
        // Sem perfil, jogo em "all" → avisa dos 2 processadores e das 88 threads.
        let d2 = run(&["meujogo"], Some("all"), Some("display"), &learned);
        let w2 = crate::decide::game_warnings(&t, &d2);
        assert!(w2.iter().any(|x| x.contains("dois processadores")) && w2.iter().any(|x| x.contains("88 threads")), "{w2:?}");

        let mut cfg2 = cfg.clone();
        cfg2.graphics_gpu = Some("gfx906-1".into());
        assert_eq!(Topology::load(&cfg2).display_gpu().unwrap().alias, "gfx906-1");
    }
}
