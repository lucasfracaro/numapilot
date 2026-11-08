//! Placas NVIDIA pelo driver oficial (nvidia-smi): sensores, limite de potência, limite de
//! clock e modo persistente. No notebook, a placa dedicada dorme quando ninguém usa: aí o
//! numapilot não chama o nvidia-smi (isso acordaria a placa e gastaria bateria).

use crate::util::{read_trim, sys};
use std::collections::BTreeMap;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const VENDOR: u16 = 0x10de;

fn smi() -> String {
    std::env::var("NUMAPILOT_NVIDIA_SMI").unwrap_or_else(|_| "nvidia-smi".into())
}

pub fn is_nvidia(pci: &str) -> bool {
    read_trim(sys(&format!("bus/pci/devices/{pci}/vendor")))
        .is_some_and(|v| v.trim_start_matches("0x").eq_ignore_ascii_case("10de"))
}

/// "active" | "suspended" | ... (gerência de energia do PCI).
pub fn runtime_status(pci: &str) -> Option<String> {
    read_trim(sys(&format!("bus/pci/devices/{pci}/power/runtime_status")))
}

pub fn asleep(pci: &str) -> bool {
    runtime_status(pci).as_deref() == Some("suspended")
}

/// Driver carregado agora (versão do módulo).
pub fn driver_version() -> Option<String> {
    read_trim(sys("module/nvidia/version"))
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct NvGpu {
    pub index: u32,
    pub pci: String,
    pub name: String,
    pub temp_c: Option<f64>,
    pub power_w: Option<f64>,
    pub power_limit_w: Option<f64>,
    pub power_min_w: Option<f64>,
    pub power_max_w: Option<f64>,
    pub power_default_w: Option<f64>,
    pub fan_pct: Option<f64>,
    pub util_pct: Option<f64>,
    pub mem_used_mb: Option<f64>,
    pub mem_total_mb: Option<f64>,
    pub clock_mhz: Option<f64>,
    pub clock_max_mhz: Option<f64>,
    pub pstate: String,
    pub driver: String,
    pub persistence: bool,
    /// O limite de potência pode ser mudado? Em notebooks o `power.limit` vem [N/A] e o
    /// nvidia-smi -pl só avisa "not supported" (e sai com sucesso).
    pub power_limit_settable: bool,
}

const FIELDS: &str = "index,pci.bus_id,name,temperature.gpu,power.draw,power.limit,power.min_limit,power.max_limit,\
power.default_limit,fan.speed,utilization.gpu,memory.used,memory.total,clocks.gr,clocks.max.gr,pstate,driver_version,persistence_mode,\
enforced.power.limit";

/// "00000000:01:00.0" → "0000:01:00.0"
pub fn norm_pci(s: &str) -> String {
    let s = s.trim().to_lowercase();
    if s.len() > 12 {
        s[s.len() - 12..].to_string()
    } else {
        s
    }
}

pub fn parse(out: &str) -> Vec<NvGpu> {
    out.lines()
        .filter_map(|l| {
            let c: Vec<&str> = l.split(',').map(str::trim).collect();
            if c.len() < 18 {
                return None;
            }
            let f = |i: usize| c.get(i).and_then(|x| x.parse::<f64>().ok());
            Some(NvGpu {
                index: c[0].parse().ok()?,
                pci: norm_pci(c[1]),
                name: c[2].to_string(),
                temp_c: f(3),
                power_w: f(4),
                // notebooks: power.limit vem [N/A]; o limite em uso fica no enforced.power.limit
                power_limit_w: f(5).or(f(18)).or(f(8)),
                power_min_w: f(6),
                power_max_w: f(7),
                power_default_w: f(8),
                fan_pct: f(9),
                util_pct: f(10),
                mem_used_mb: f(11),
                mem_total_mb: f(12),
                clock_mhz: f(13),
                clock_max_mhz: f(14),
                pstate: c[15].to_string(),
                driver: c[16].to_string(),
                persistence: c[17].eq_ignore_ascii_case("enabled"),
                power_limit_settable: f(5).is_some(),
            })
        })
        .collect()
}

static CACHE: Mutex<Option<(Instant, Vec<NvGpu>)>> = Mutex::new(None);

/// Todas as placas NVIDIA (cache de 2 s). Vazio se o driver oficial não estiver carregado
/// ou se todas estiverem dormindo.
pub fn query() -> Vec<NvGpu> {
    if let Ok(c) = CACHE.lock()
        && let Some((t, v)) = c.as_ref()
        && t.elapsed() < Duration::from_secs(2)
    {
        return v.clone();
    }
    let v = Command::new(smi())
        .args([
            format!("--query-gpu={FIELDS}").as_str(),
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| parse(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default();
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((Instant::now(), v.clone()));
    }
    v
}

/// Dados de uma placa, sem acordá-la.
pub fn get(pci: &str) -> Option<NvGpu> {
    if asleep(pci) {
        return None;
    }
    query().into_iter().find(|g| g.pci == pci)
}

/// Sensores no formato comum das GPUs (para a interface, alertas e histórico).
pub fn sensors(pci: &str) -> crate::gpuctl::Sensors {
    let Some(g) = get(pci) else {
        return Default::default();
    };
    crate::gpuctl::Sensors {
        edge_c: g.temp_c,
        fan_pct: g.fan_pct,
        power_w: g.power_w,
        power_cap_w: g.power_limit_w,
        // sem limite ajustável, a interface não mostra o controle
        power_cap_min_w: g.power_min_w.filter(|_| g.power_limit_settable),
        power_cap_max_w: g.power_max_w.filter(|_| g.power_limit_settable),
        ..Default::default()
    }
}

fn index_of(pci: &str) -> Result<u32, String> {
    query()
        .into_iter()
        .find(|g| g.pci == pci)
        .map(|g| g.index)
        .ok_or_else(|| {
            "placa NVIDIA não encontrada pelo nvidia-smi (driver carregado?)".to_string()
        })
}

fn run(args: &[String]) -> Result<(), String> {
    let o = Command::new(smi())
        .args(args)
        .output()
        .map_err(|e| format!("nvidia-smi: {e}"))?;
    if let Ok(mut c) = CACHE.lock() {
        *c = None;
    }
    let text = String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr);
    // o nvidia-smi sai com sucesso mesmo quando não muda nada ("... is not supported ...")
    if o.status.success() && text.to_lowercase().contains("not supported") {
        return Err(format!(
            "esta placa não deixa mudar isso (o fabricante trava): {}",
            text.lines()
                .find(|l| l.to_lowercase().contains("not supported"))
                .unwrap_or("")
                .trim()
        ));
    }
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stdout).trim().to_string()
            + &String::from_utf8_lossy(&o.stderr))
    }
}

/// Limite de potência (None = padrão da placa). Notebooks costumam travar esse valor.
pub fn set_power_limit(pci: &str, watts: Option<f64>) -> Result<f64, String> {
    let g = get(pci).ok_or("placa dormindo ou sem driver")?;
    let target = watts
        .or(g.power_default_w)
        .ok_or("a placa não informa o limite padrão")?;
    let v = target.clamp(
        g.power_min_w.unwrap_or(0.0),
        g.power_max_w.unwrap_or(target),
    );
    run(&[
        "-i".into(),
        g.index.to_string(),
        "-pl".into(),
        format!("{v:.0}"),
    ])?;
    Ok(v)
}

/// Trava o clock máximo da GPU (None = volta ao normal). Junto com o limite de potência,
/// é o jeito de fazer a placa gastar e esquentar menos pelo nvidia-smi.
pub fn set_clock_max(pci: &str, mhz: Option<u32>) -> Result<(), String> {
    let i = index_of(pci)?.to_string();
    match mhz {
        Some(m) => run(&["-i".into(), i, "-lgc".into(), format!("0,{m}")]),
        None => run(&["-i".into(), i, "-rgc".into()]),
    }
}

pub fn set_persistence(pci: &str, on: bool) -> Result<(), String> {
    let i = index_of(pci)?.to_string();
    run(&[
        "-i".into(),
        i,
        "-pm".into(),
        if on { "1" } else { "0" }.into(),
    ])
}

/// Índices CUDA (ordem do barramento PCI) das placas NVIDIA, para CUDA_VISIBLE_DEVICES.
pub fn cuda_indices(topo: &crate::topo::Topology) -> BTreeMap<String, usize> {
    let mut v: Vec<&str> = topo
        .gpus
        .iter()
        .filter(|g| g.vendor == VENDOR)
        .map(|g| g.pci.as_str())
        .collect();
    v.sort_unstable();
    v.into_iter()
        .enumerate()
        .map(|(i, p)| (p.to_string(), i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_smi() {
        let out = "0, 00000000:01:00.0, NVIDIA GeForce RTX 4060 Laptop GPU, 52, 18.43, 80.00, 5.00, 115.00, 80.00, [N/A], 7, 512, 8188, 1530, 3105, P3, 580.119.02, Disabled\n";
        // notebook de verdade (RTX 3070 Ti, 615.78): power.limit [N/A], limite em uso 115 W
        let nb = parse(
            "0, 00000000:01:00.0, NVIDIA GeForce RTX 3070 Ti Laptop GPU, 43, 9.81, [N/A], 1.00, 150.00, [N/A], [N/A], 0, 207, 8192, 210, 1785, P8, 615.78.08, Disabled, 115.00\n",
        );
        assert_eq!(nb[0].power_limit_w, Some(115.0));
        let v = parse(out);
        assert_eq!(v.len(), 1);
        let g = &v[0];
        assert_eq!(g.pci, "0000:01:00.0");
        assert_eq!(g.temp_c, Some(52.0));
        assert_eq!(
            g.fan_pct, None,
            "notebook não tem ventoinha própria na placa"
        );
        assert_eq!(g.power_max_w, Some(115.0));
        assert_eq!(g.driver, "580.119.02");
        assert!(!g.persistence);
        assert!(parse("lixo\n").is_empty());
    }
}
