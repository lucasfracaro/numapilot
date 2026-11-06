//! Ajustes da máquina inteira (processadores, memória, escalonador, ventoinhas IPMI) e os modos
//! de um clique (Jogo, IA, Silencioso, Normal). Quem aplica é o serviço (root); ao voltar para
//! "não mexer", cada ajuste volta ao valor que o sistema tinha quando o serviço iniciou.

use crate::cpuctl;
use crate::learned::state_dir;
use crate::topo::Topology;
use crate::util::{read_trim, read_u64, sys};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SocketTune {
    pub governor: Option<String>,
    /// Energy Performance Bias: 0 desempenho, 4, 6 normal, 8, 15 economia.
    pub epb: Option<u8>,
    pub min_mhz: Option<u32>,
    pub max_mhz: Option<u32>,
    pub uncore_min_mhz: Option<u32>,
    pub uncore_max_mhz: Option<u32>,
    pub power_limit_w: Option<f64>,
}

impl SocketTune {
    pub fn is_empty(&self) -> bool {
        *self == SocketTune::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Machine {
    /// "normal" | "jogo" | "ia" | "silencioso" | "personalizado"
    pub mode: String,
    pub turbo: Option<bool>,
    /// Por nó NUMA (= socket); chave = número do nó em texto (JSON).
    pub sockets: BTreeMap<String, SocketTune>,
    /// Balanceamento NUMA automático do kernel (move páginas sozinho).
    pub numa_balancing: Option<bool>,
    pub zone_reclaim: Option<bool>,
    /// Transparent Huge Pages: "always" | "madvise" | "never"
    pub thp: Option<String>,
    /// Hugepages de 2 MB reservadas por nó (em MB).
    pub hugepages_mb: BTreeMap<String, u64>,
    /// Escalonador sched_ext pelo scx_loader: None = não mexer, "none" = desligar, "lavd"...
    pub scx: Option<String>,
    /// auto | gaming | powersave | lowlatency | server
    pub scx_mode: Option<String>,
    /// Enquanto um programa com "baixa latência" roda, desliga nos núcleos dele os estados de
    /// economia com latência de saída acima disto (µs).
    pub game_latency_us: u64,
    /// ... e fixa o uncore do socket dele no máximo.
    pub game_uncore_max: bool,
    /// Modo das ventoinhas por IPMI (placas Supermicro): 0 padrão, 1 máximo, 2 ótimo, 4 "heavy IO".
    pub ipmi_fan_mode: Option<u8>,
}

impl Default for Machine {
    fn default() -> Self {
        Machine {
            mode: "normal".into(),
            turbo: None,
            sockets: BTreeMap::new(),
            numa_balancing: None,
            zone_reclaim: None,
            thp: None,
            hugepages_mb: BTreeMap::new(),
            scx: None,
            scx_mode: None,
            game_latency_us: 2,
            game_uncore_max: true,
            ipmi_fan_mode: None,
        }
    }
}

impl Machine {
    pub fn socket(&self, node: usize) -> Option<&SocketTune> {
        self.sockets.get(&node.to_string())
    }
    pub fn socket_mut(&mut self, node: usize) -> &mut SocketTune {
        self.sockets.entry(node.to_string()).or_default()
    }
    pub fn path() -> PathBuf {
        state_dir().join("machine.json")
    }
    pub fn load() -> Machine {
        std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }
    pub fn save(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(state_dir())?;
        std::fs::write(Self::path(), serde_json::to_string_pretty(self).unwrap_or_default())
    }

    pub fn validate(&self) -> Result<(), String> {
        for (n, t) in &self.sockets {
            if let (Some(a), Some(b)) = (t.min_mhz, t.max_mhz)
                && a > b
            {
                return Err(format!("socket {n}: frequência mínima maior que a máxima"));
            }
            if let Some(w) = t.power_limit_w
                && w < 20.0
            {
                return Err(format!("socket {n}: limite de potência muito baixo"));
            }
            if t.epb.is_some_and(|e| e > 15) {
                return Err("EPB vai de 0 a 15".into());
            }
        }
        if let Some(t) = &self.thp
            && !["always", "madvise", "never"].contains(&t.as_str())
        {
            return Err(format!("THP inválido: {t}"));
        }
        if let Some(s) = &self.scx
            && !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err("nome de escalonador inválido".into());
        }
        if let Some(m) = &self.scx_mode
            && !["auto", "gaming", "powersave", "lowlatency", "server"].contains(&m.as_str())
        {
            return Err(format!("modo do escalonador inválido: {m}"));
        }
        if self.ipmi_fan_mode.is_some_and(|m| ![0, 1, 2, 4].contains(&m)) {
            return Err("modo de ventoinha IPMI inválido".into());
        }
        Ok(())
    }

    /// Modos de um clique. `game_node` = socket da placa do jogo.
    pub fn preset(name: &str, topo: &Topology, game_node: Option<usize>, keep: &Machine) -> Machine {
        let mut m = Machine {
            mode: name.into(),
            game_latency_us: keep.game_latency_us,
            game_uncore_max: keep.game_uncore_max,
            hugepages_mb: keep.hugepages_mb.clone(),
            ipmi_fan_mode: keep.ipmi_fan_mode,
            ..Default::default()
        };
        let has_scx = crate::containers::which("scxctl");
        let uncore_max = |pkg: u32| {
            cpuctl::uncores().into_iter().find(|u| u.package == pkg).map(|u| (u.init_max_khz.max(u.max_khz) / 1000) as u32)
        };
        let pkg = |n: usize| topo.node(n).and_then(|n| n.sockets.iter().next().copied()).unwrap_or(n as u32);
        match name {
            "jogo" => {
                m.turbo = Some(true);
                m.numa_balancing = Some(false);
                let g = game_node.or(topo.nodes.first().map(|n| n.id)).unwrap_or(0);
                let mut t = SocketTune { governor: Some("performance".into()), epb: Some(0), ..Default::default() };
                if let Some(u) = uncore_max(pkg(g)) {
                    t.uncore_min_mhz = Some(u);
                    t.uncore_max_mhz = Some(u);
                }
                m.sockets.insert(g.to_string(), t);
                if has_scx {
                    m.scx = Some("lavd".into());
                    m.scx_mode = Some("gaming".into());
                }
            }
            "ia" => {
                m.turbo = Some(true);
                m.numa_balancing = Some(false);
                m.thp = Some("always".into());
                for n in &topo.nodes {
                    let mut t = SocketTune { governor: Some("performance".into()), epb: Some(4), ..Default::default() };
                    if let Some(u) = uncore_max(pkg(n.id)) {
                        t.uncore_min_mhz = Some(u);
                        t.uncore_max_mhz = Some(u);
                    }
                    m.sockets.insert(n.id.to_string(), t);
                }
            }
            "silencioso" => {
                m.turbo = Some(false);
                let rapl = cpuctl::rapl_zones();
                for n in &topo.nodes {
                    let cpu0 = n.cpus.first().copied().unwrap_or(0);
                    let mut t = SocketTune { governor: Some(cpuctl::eco_governor(cpu0)), epb: Some(15), ..Default::default() };
                    if let Some(z) = rapl.iter().find(|z| z.package == pkg(n.id))
                        && let Some(max) = z.max_w.or(z.limit_w)
                    {
                        t.power_limit_w = Some((max * 0.6).round().max(40.0));
                    }
                    m.sockets.insert(n.id.to_string(), t);
                }
                if has_scx {
                    m.scx = Some("lavd".into());
                    m.scx_mode = Some("powersave".into());
                }
            }
            _ => {
                m.mode = "normal".into();
                // Normal: tudo como o sistema tinha (mas desliga o escalonador que um modo ligou).
            }
        }
        m
    }
}

pub fn procsys(rel: &str) -> PathBuf {
    std::env::var_os("NUMAPILOT_PROCSYS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/proc/sys")).join(rel)
}

fn thp_path() -> PathBuf {
    sys("kernel/mm/transparent_hugepage/enabled")
}

/// Modo atual do THP (o que está entre colchetes).
pub fn thp() -> Option<String> {
    let s = read_trim(thp_path())?;
    let a = s.find('[')?;
    let b = s[a..].find(']')?;
    Some(s[a + 1..a + b].to_string())
}

fn huge_path(node: usize) -> PathBuf {
    sys(&format!("devices/system/node/node{node}/hugepages/hugepages-2048kB/nr_hugepages"))
}

pub fn hugepages(node: usize) -> Option<u64> {
    read_u64(huge_path(node))
}

/// Valores que o sistema tinha quando o serviço iniciou.
#[derive(Debug, Clone, Default)]
pub struct Orig {
    pub turbo: Option<bool>,
    pub gov: BTreeMap<usize, String>,
    pub freq: BTreeMap<usize, (u64, u64)>,
    pub epb: BTreeMap<usize, u8>,
    pub rapl: BTreeMap<u32, f64>,
    pub numa_balancing: Option<String>,
    pub zone_reclaim: Option<String>,
    pub thp: Option<String>,
    pub huge: BTreeMap<usize, u64>,
    pub ipmi: Option<u8>,
}

impl Orig {
    pub fn capture(topo: &Topology) -> Orig {
        let mut o = Orig { turbo: cpuctl::turbo(), ..Default::default() };
        for c in topo.all_cpus() {
            if let Some(g) = cpuctl::governor(c) {
                o.gov.insert(c, g);
            }
            if let Some(f) = cpuctl::freq(c) {
                o.freq.insert(c, (f.min_khz, f.max_khz));
            }
            if let Some(e) = cpuctl::epb(c) {
                o.epb.insert(c, e);
            }
        }
        for z in cpuctl::rapl_zones() {
            if let Some(l) = z.limit_w {
                o.rapl.insert(z.package, l);
            }
        }
        o.numa_balancing = read_trim(procsys("kernel/numa_balancing"));
        o.zone_reclaim = read_trim(procsys("vm/zone_reclaim_mode"));
        o.thp = thp();
        for n in &topo.nodes {
            if let Some(h) = hugepages(n.id) {
                o.huge.insert(n.id, h);
            }
        }
        o.ipmi = crate::fans::supermicro_mode();
        o
    }
}

fn package_of(topo: &Topology, node: usize) -> u32 {
    topo.node(node).and_then(|n| n.sockets.iter().next().copied()).unwrap_or(node as u32)
}

/// Uncore do socket conforme o ajuste salvo (ou o de fábrica).
pub fn apply_uncore(topo: &Topology, node: usize, t: Option<&SocketTune>) -> Result<(), String> {
    let pkg = package_of(topo, node);
    match t.map(|t| (t.uncore_min_mhz, t.uncore_max_mhz)) {
        Some((Some(a), Some(b))) => cpuctl::set_uncore(pkg, a as u64 * 1000, b as u64 * 1000),
        Some((a, b)) if a.is_some() || b.is_some() => {
            let u = cpuctl::uncores().into_iter().find(|u| u.package == pkg).ok_or("uncore indisponível")?;
            let lo = a.map(|x| x as u64 * 1000).unwrap_or(u.init_min_khz);
            let hi = b.map(|x| x as u64 * 1000).unwrap_or(u.init_max_khz);
            cpuctl::set_uncore(pkg, lo, hi)
        }
        _ if cpuctl::uncores().is_empty() => Ok(()),
        _ => cpuctl::reset_uncore(pkg),
    }
}

/// Aplica tudo. `prev` = o que estava aplicado antes (para o escalonador só mudar quando precisa);
/// `base_gov` = governor escolhido por socket na aba Mapa (vale quando o modo não define).
pub fn apply(m: &Machine, prev: Option<&Machine>, orig: &Orig, topo: &Topology, base_gov: &BTreeMap<usize, String>) -> Vec<String> {
    let mut errs = Vec::new();
    let mut e = |r: Result<(), String>, what: &str| {
        if let Err(x) = r {
            errs.push(format!("{what}: {x}"));
        }
    };
    if let Some(t) = m.turbo.or(orig.turbo)
        && cpuctl::turbo() != Some(t)
    {
        e(cpuctl::set_turbo(t), "turbo");
    }
    for n in &topo.nodes {
        let t = m.socket(n.id);
        // governor
        for &c in &n.cpus {
            let g = t.and_then(|t| t.governor.clone()).or_else(|| base_gov.get(&n.id).cloned()).or_else(|| orig.gov.get(&c).cloned());
            if let Some(g) = g
                && cpuctl::governor(c).as_deref() != Some(g.as_str())
            {
                e(cpuctl::set_governor(&[c], &g), &format!("governor cpu{c}"));
            }
            let ep = t.and_then(|t| t.epb).or_else(|| orig.epb.get(&c).copied());
            if let Some(ep) = ep
                && cpuctl::epb(c) != Some(ep)
            {
                e(cpuctl::set_epb(&[c], ep), &format!("EPB cpu{c}"));
            }
            if let Some(f) = cpuctl::freq(c) {
                let (omin, omax) = orig.freq.get(&c).copied().unwrap_or((f.hw_min_khz, f.hw_max_khz));
                let lo = t.and_then(|t| t.min_mhz).map(|x| x as u64 * 1000).unwrap_or(omin);
                let hi = t.and_then(|t| t.max_mhz).map(|x| x as u64 * 1000).unwrap_or(omax);
                if (lo, hi) != (f.min_khz, f.max_khz) {
                    e(cpuctl::set_freq(&[c], lo, hi), &format!("frequência cpu{c}"));
                }
            }
        }
        e(apply_uncore(topo, n.id, t), &format!("uncore socket {}", n.id));
        let pkg = package_of(topo, n.id);
        if let Some(w) = t.and_then(|t| t.power_limit_w).or_else(|| orig.rapl.get(&pkg).copied())
            && cpuctl::rapl_zones().iter().any(|z| z.package == pkg && z.limit_w.map(|l| (l - w).abs() > 0.5).unwrap_or(true))
        {
            e(cpuctl::set_rapl_limit(pkg, w), &format!("potência socket {}", n.id));
        }
        let want = m.hugepages_mb.get(&n.id.to_string()).map(|mb| mb / 2).or_else(|| orig.huge.get(&n.id).copied());
        if let Some(h) = want
            && hugepages(n.id) != Some(h)
        {
            e(std::fs::write(huge_path(n.id), h.to_string()).map_err(|x| x.to_string()), &format!("hugepages nó {}", n.id));
            if let Some(got) = hugepages(n.id)
                && got < h
            {
                e(Err(format!("pedi {h} páginas, o kernel conseguiu {got} (memória fragmentada; tente logo após ligar)")), &format!("hugepages nó {}", n.id));
            }
        }
    }
    let sysw = |rel: &str, v: Option<String>| -> Result<(), String> {
        let Some(v) = v else { return Ok(()) };
        let p = procsys(rel);
        if read_trim(&p).as_deref() == Some(v.as_str()) {
            return Ok(());
        }
        std::fs::write(&p, &v).map_err(|x| format!("{}: {x}", p.display()))
    };
    e(sysw("kernel/numa_balancing", m.numa_balancing.map(|b| if b { "1" } else { "0" }.to_string()).or(orig.numa_balancing.clone())), "numa_balancing");
    e(sysw("vm/zone_reclaim_mode", m.zone_reclaim.map(|b| if b { "1" } else { "0" }.to_string()).or(orig.zone_reclaim.clone())), "zone_reclaim_mode");
    if let Some(t) = m.thp.clone().or(orig.thp.clone())
        && thp().as_deref() != Some(t.as_str())
    {
        e(std::fs::write(thp_path(), &t).map_err(|x| x.to_string()), "THP");
    }
    let prev_scx = prev.map(|p| (p.scx.clone(), p.scx_mode.clone())).unwrap_or((None, None));
    if (m.scx.clone(), m.scx_mode.clone()) != prev_scx {
        e(apply_scx(m.scx.as_deref(), m.scx_mode.as_deref(), prev_scx.0.is_some()), "escalonador");
    }
    if let Some(mode) = m.ipmi_fan_mode.or(orig.ipmi)
        && crate::fans::supermicro_mode() != Some(mode)
    {
        e(crate::fans::supermicro_set_mode(mode), "ventoinhas IPMI");
    }
    errs
}

// ---------------- sched_ext (scx_loader / scxctl) ----------------

fn scxctl(args: &[&str]) -> Result<String, String> {
    let o = std::process::Command::new("scxctl").args(args).output().map_err(|e| format!("scxctl: {e}"))?;
    let out = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    if o.status.success() { Ok(out) } else { Err(out.trim().to_string()) }
}

fn apply_scx(sched: Option<&str>, mode: Option<&str>, had: bool) -> Result<(), String> {
    match sched {
        None if had => scxctl(&["restore"]).map(|_| ()),
        None => Ok(()),
        Some("none") => scxctl(&["stop"]).map(|_| ()),
        Some(s) => {
            let mode = mode.unwrap_or("auto");
            scxctl(&["switch", "--sched", s, "--mode", mode])
                .or_else(|_| scxctl(&["start", "--sched", s, "--mode", mode]))
                .map(|_| ())
        }
    }
}

/// (escalonador atual, lista dos disponíveis) pelo scxctl.
pub fn scx_status() -> Option<(String, Vec<String>)> {
    if !crate::containers::which("scxctl") {
        return None;
    }
    let cur = scxctl(&["get"]).unwrap_or_default().lines().next().unwrap_or("").trim().to_string();
    let list = scxctl(&["list"])
        .unwrap_or_default()
        .split(|c: char| c == ',' || c.is_whitespace() || c == '[' || c == ']' || c == '"')
        .map(|s| s.trim().trim_start_matches("scx_").to_string())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && s != "supported" && s != "schedulers")
        .collect();
    Some((cur, list))
}

// ---------------- baixa latência por programa ----------------

/// Estado aplicado para os programas com "baixa latência" (C-states e uncore).
#[derive(Debug, Default)]
pub struct LowLatency {
    pub cpus: Vec<usize>,
    cstates: Vec<(usize, usize)>,
    nodes: Vec<usize>,
}

impl LowLatency {
    /// Ajusta para o novo conjunto de núcleos (vazio = devolve tudo).
    pub fn set(&mut self, want: Vec<usize>, m: &Machine, topo: &Topology) {
        if want == self.cpus {
            return;
        }
        cpuctl::restore_cstates(&self.cstates);
        for n in &self.nodes {
            let _ = apply_uncore(topo, *n, m.socket(*n));
        }
        self.cstates = cpuctl::limit_latency(&want, m.game_latency_us);
        self.nodes = topo.nodes_of(&want);
        if m.game_uncore_max {
            for n in &self.nodes {
                let pkg = package_of(topo, *n);
                if let Some(u) = cpuctl::uncores().into_iter().find(|u| u.package == pkg) {
                    let top = u.init_max_khz.max(u.max_khz);
                    let _ = cpuctl::set_uncore(pkg, top, top);
                }
            }
        }
        if !want.is_empty() {
            eprintln!(
                "[numapilot] baixa latência nos núcleos {} ({} estados de economia desligados)",
                crate::util::format_cpulist(&want),
                self.cstates.len()
            );
        } else if !self.cpus.is_empty() {
            eprintln!("[numapilot] baixa latência desligada");
        }
        self.cpus = want;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_and_serde() {
        let mut m = Machine::default();
        assert!(m.validate().is_ok());
        m.sockets.insert("0".into(), SocketTune { min_mhz: Some(3000), max_mhz: Some(2000), ..Default::default() });
        assert!(m.validate().is_err());
        m.sockets.clear();
        m.thp = Some("sempre".into());
        assert!(m.validate().is_err());
        m.thp = Some("madvise".into());
        m.scx = Some("lavd".into());
        m.scx_mode = Some("gaming".into());
        assert!(m.validate().is_ok());
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(serde_json::from_str::<Machine>(&s).unwrap(), m);
        // Arquivo antigo/parcial ganha os padrões.
        let p: Machine = serde_json::from_str(r#"{"mode":"jogo"}"#).unwrap();
        assert_eq!(p.game_latency_us, 2);
        assert!(p.game_uncore_max);
    }

    #[test]
    fn low_latency_on_fake_cpus() {
        let root = std::env::temp_dir().join("numapilot-lowlat-test");
        let _ = std::fs::remove_dir_all(&root);
        let mk = |p: &str, v: &str| {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, v).unwrap();
        };
        for c in 0..4 {
            for (i, (n, l)) in [("POLL", 0), ("C1", 2), ("C1E", 10), ("C6", 133)].iter().enumerate() {
                mk(&format!("devices/system/cpu/cpu{c}/cpuidle/state{i}/name"), n);
                mk(&format!("devices/system/cpu/cpu{c}/cpuidle/state{i}/latency"), &l.to_string());
                mk(&format!("devices/system/cpu/cpu{c}/cpuidle/state{i}/disable"), "0");
            }
        }
        let u = "devices/system/cpu/intel_uncore_frequency/package_00_die_00/";
        for (f, v) in [("min_freq_khz", "1200000"), ("max_freq_khz", "2800000"), ("initial_min_freq_khz", "1200000"), ("initial_max_freq_khz", "2800000")] {
            mk(&format!("{u}{f}"), v);
        }
        let _g = crate::util::test_sysfs_lock(&root);
        let node = crate::topo::Node {
            id: 0,
            cpus: vec![0, 1, 2, 3],
            cores: vec![vec![0], vec![1], vec![2], vec![3]],
            physical_cores: 4,
            sockets: [0u32].into_iter().collect(),
            mem_total_kb: 0,
            distances: vec![10],
        };
        let topo = Topology { nodes: vec![node], gpus: vec![], default_graphics: None };
        let m = Machine::default();
        let mut ll = LowLatency::default();
        ll.set(vec![0, 1], &m, &topo);
        let dis = |c: usize, i: usize| std::fs::read_to_string(root.join(format!("devices/system/cpu/cpu{c}/cpuidle/state{i}/disable"))).unwrap();
        assert_eq!((dis(0, 1).as_str(), dis(0, 2).as_str(), dis(0, 3).as_str()), ("0", "1", "1"));
        assert_eq!(dis(2, 3), "0", "outros núcleos não mudam");
        assert_eq!(crate::cpuctl::uncores()[0].min_khz, 2800000);
        ll.set(vec![], &m, &topo);
        assert_eq!(dis(0, 3), "0");
        assert_eq!(crate::cpuctl::uncores()[0].min_khz, 1200000);
    }
}
