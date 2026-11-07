//! Saúde do hardware: discos (SMART), memória com ECC (EDAC), erros corrigidos/não corrigidos
//! das placas AMD com ECC (RAS: HBM2 das MI50) e desgaste da bateria. O serviço coleta (o SMART
//! precisa de root) e grava em /run/numapilot/health.json; a interface e o diagnóstico leem.

use crate::util::{read_trim, read_u64, sys};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::process::Command;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Disk {
    pub dev: String,
    pub model: String,
    /// nvme | ssd | hdd
    pub kind: String,
    /// Resultado geral do SMART (None = não deu para ler).
    pub passed: Option<bool>,
    pub temp_c: Option<f64>,
    pub hours: Option<u64>,
    /// NVMe: % da vida útil já usada (pode passar de 100).
    pub used_pct: Option<u64>,
    /// NVMe: % de reserva disponível.
    pub spare_pct: Option<u64>,
    pub media_errors: Option<u64>,
    /// SATA: setores realocados / pendentes / incorrigíveis.
    pub realloc: Option<u64>,
    pub pending: Option<u64>,
    pub uncorrectable: Option<u64>,
    pub written_tb: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemCtl {
    pub name: String,
    pub ce: u64,
    pub ue: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuRas {
    pub pci: String,
    /// umc (memória), gfx, sdma, mmhub...
    pub block: String,
    pub ce: u64,
    pub ue: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bat {
    pub name: String,
    pub health_pct: Option<f64>,
    pub cycles: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Report {
    /// segundos desde 1970
    pub time: u64,
    pub smartctl: bool,
    pub disks: Vec<Disk>,
    pub mem: Vec<MemCtl>,
    pub gpu: Vec<GpuRas>,
    pub batteries: Vec<Bat>,
}

/// Discos físicos (sem loop, zram, partições e device-mapper).
pub fn disks() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(sys("block"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with("nvme") && !n[4..].contains(['p', 'c']) || n.starts_with("sd") || n.starts_with("hd"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

fn u(v: &Value) -> Option<u64> {
    v.as_u64()
}

/// Lê a saída `smartctl -j -a` (NVMe e SATA).
pub fn parse_smart(dev: &str, j: &Value) -> Disk {
    let rot = j["rotation_rate"].as_u64();
    let nvme = &j["nvme_smart_health_information_log"];
    let kind = if !nvme.is_null() || dev.starts_with("nvme") {
        "nvme"
    } else if rot == Some(0) {
        "ssd"
    } else {
        "hdd"
    };
    let attr = |id: u64| -> Option<u64> {
        j["ata_smart_attributes"]["table"]
            .as_array()?
            .iter()
            .find(|a| a["id"].as_u64() == Some(id))
            .and_then(|a| a["raw"]["value"].as_u64())
    };
    let written_tb = u(&nvme["data_units_written"])
        .map(|x| x as f64 * 512_000.0 / 1e12)
        .or_else(|| attr(241).map(|lba| lba as f64 * 512.0 / 1e12));
    Disk {
        dev: dev.into(),
        model: j["model_name"].as_str().unwrap_or("").trim().to_string(),
        kind: kind.into(),
        passed: j["smart_status"]["passed"].as_bool(),
        temp_c: j["temperature"]["current"].as_f64(),
        hours: u(&j["power_on_time"]["hours"]),
        used_pct: u(&nvme["percentage_used"]),
        spare_pct: u(&nvme["available_spare"]),
        media_errors: u(&nvme["media_errors"]),
        realloc: attr(5),
        pending: attr(197),
        uncorrectable: attr(198),
        written_tb,
    }
}

fn smart(dev: &str) -> Option<Disk> {
    let o = Command::new("smartctl").args(["-j", "-a", &format!("/dev/{dev}")]).output().ok()?;
    let j: Value = serde_json::from_slice(&o.stdout).ok()?;
    if j["smart_status"].is_null() && j["model_name"].is_null() {
        return None;
    }
    Some(parse_smart(dev, &j))
}

pub fn edac() -> Vec<MemCtl> {
    let Ok(rd) = std::fs::read_dir(sys("devices/system/edac/mc")) else { return vec![] };
    let mut v: Vec<MemCtl> = rd
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("mc"))
        .map(|e| {
            let d = e.path();
            MemCtl {
                name: format!(
                    "{} ({})",
                    e.file_name().to_string_lossy(),
                    read_trim(d.join("mc_name")).unwrap_or_default()
                ),
                ce: read_u64(d.join("ce_count")).unwrap_or(0),
                ue: read_u64(d.join("ue_count")).unwrap_or(0),
            }
        })
        .collect();
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

/// "ue: 0\nce: 3" -> (ce, ue)
pub fn parse_ras(s: &str) -> (u64, u64) {
    let mut ce = 0;
    let mut ue = 0;
    for l in s.lines() {
        if let Some((k, v)) = l.split_once(':') {
            let n = v.trim().parse().unwrap_or(0);
            match k.trim() {
                "ce" => ce = n,
                "ue" => ue = n,
                _ => {}
            }
        }
    }
    (ce, ue)
}

pub fn gpu_ras(pci: &str) -> Vec<GpuRas> {
    let d = sys(&format!("bus/pci/devices/{pci}/ras"));
    let Ok(rd) = std::fs::read_dir(&d) else { return vec![] };
    let mut v: Vec<GpuRas> = rd
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            let block = n.strip_suffix("_err_count")?.to_string();
            let (ce, ue) = parse_ras(&std::fs::read_to_string(e.path()).ok()?);
            Some(GpuRas { pci: pci.into(), block, ce, ue })
        })
        .collect();
    v.sort_by(|a, b| a.block.cmp(&b.block));
    v
}

pub fn collect(topo: &crate::topo::Topology) -> Report {
    let smartctl = crate::containers::which("smartctl");
    let disks = if smartctl { disks().iter().filter_map(|d| smart(d)).collect() } else { vec![] };
    let batteries = crate::notebook::battery_info()
        .into_iter()
        .map(|b| Bat {
            cycles: read_u64(b.dir.join("cycle_count")).filter(|c| *c > 0),
            name: b.name,
            health_pct: b.health_pct,
        })
        .collect();
    Report {
        time: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        smartctl,
        disks,
        mem: edac(),
        gpu: topo.gpus.iter().flat_map(|g| gpu_ras(&g.pci)).collect(),
        batteries,
    }
}

pub fn path() -> std::path::PathBuf {
    crate::learned::run_dir().join("health.json")
}

pub fn load() -> Option<Report> {
    serde_json::from_str(&std::fs::read_to_string(path()).ok()?).ok()
}

/// Gravidade: 2 = trocar logo / risco de perder dados, 1 = de olho, 0 = informação.
pub fn problems(r: &Report) -> Vec<(u8, String)> {
    let mut v = vec![];
    for d in &r.disks {
        let n = if d.model.is_empty() { d.dev.clone() } else { format!("{} ({})", d.dev, d.model) };
        if d.passed == Some(false) {
            v.push((2, format!("{n}: o próprio disco diz que vai falhar (SMART). Faça backup e troque.")));
        }
        if d.media_errors.unwrap_or(0) > 0 {
            v.push((2, format!("{n}: {} erros de mídia", d.media_errors.unwrap_or(0))));
        }
        if d.uncorrectable.unwrap_or(0) > 0 || d.pending.unwrap_or(0) > 0 {
            v.push((2, format!("{n}: setores ruins pendentes/incorrigíveis ({}/{})", d.pending.unwrap_or(0), d.uncorrectable.unwrap_or(0))));
        }
        if d.realloc.unwrap_or(0) > 0 {
            v.push((1, format!("{n}: {} setores realocados (acompanhe se aumenta)", d.realloc.unwrap_or(0))));
        }
        if let Some(p) = d.used_pct
            && p >= 80
        {
            v.push((if p >= 100 { 2 } else { 1 }, format!("{n}: {p}% da vida útil usada")));
        }
        if let Some(s) = d.spare_pct
            && s < 20
        {
            v.push((2, format!("{n}: só {s}% de reserva")));
        }
        if let Some(t) = d.temp_c
            && t >= 70.0
        {
            v.push((1, format!("{n}: {t:.0} °C (quente; melhore a ventilação)")));
        }
    }
    for m in &r.mem {
        if m.ue > 0 {
            v.push((2, format!("memória {}: {} erros NÃO corrigidos (pente com defeito?)", m.name, m.ue)));
        } else if m.ce > 0 {
            v.push((1, format!("memória {}: {} erros corrigidos pelo ECC (se crescer, troque o pente)", m.name, m.ce)));
        }
    }
    for g in &r.gpu {
        if g.ue > 0 {
            v.push((2, format!("GPU {} ({}): {} erros não corrigidos", g.pci, g.block, g.ue)));
        } else if g.ce > 0 {
            v.push((1, format!("GPU {} ({}): {} erros corrigidos pelo ECC", g.pci, g.block, g.ce)));
        }
    }
    for b in &r.batteries {
        if let Some(h) = b.health_pct
            && h < 70.0
        {
            v.push((1, format!("bateria {}: {h:.0}% da capacidade original", b.name)));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_nvme_and_sata() {
        let j: Value = serde_json::from_str(r#"{"model_name":"Samsung 980","smart_status":{"passed":true},"temperature":{"current":41},"power_on_time":{"hours":1234},
            "nvme_smart_health_information_log":{"percentage_used":85,"available_spare":100,"media_errors":0,"data_units_written":20000000}}"#).unwrap();
        let d = parse_smart("nvme0n1", &j);
        assert_eq!((d.kind.as_str(), d.used_pct, d.hours), ("nvme", Some(85), Some(1234)));
        assert!((d.written_tb.unwrap() - 10.24).abs() < 0.01);
        let j: Value = serde_json::from_str(r#"{"model_name":"WD","rotation_rate":7200,"smart_status":{"passed":true},
            "ata_smart_attributes":{"table":[{"id":5,"raw":{"value":8}},{"id":197,"raw":{"value":0}}]}}"#).unwrap();
        let s = parse_smart("sda", &j);
        assert_eq!((s.kind.as_str(), s.realloc, s.pending), ("hdd", Some(8), Some(0)));
        let r = Report { disks: vec![d, s], ..Default::default() };
        let p = problems(&r);
        assert_eq!(p.len(), 2, "{p:?}");
        assert_eq!(parse_ras("ue: 1\nce: 7\n"), (7, 1));
    }
}
