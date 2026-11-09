//! Placas com ajustes da comunidade. Hoje: o governor de clock da BC-250 (lê e edita a
//! configuração dele, qualquer que seja o formato, e reinicia o serviço) e as ferramentas de
//! overclock da CPU pela SMU.

use crate::recipes::Board;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct Service {
    pub name: String,
    pub active: bool,
    pub enabled: bool,
    pub installed: bool,
}

fn systemctl_ok(args: &[&str]) -> bool {
    Command::new("systemctl")
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn services(b: &Board) -> Vec<Service> {
    b.governor_services
        .iter()
        .map(|s| {
            let unit = format!("{s}.service");
            let installed = ["/usr/lib/systemd/system", "/etc/systemd/system"]
                .iter()
                .any(|d| std::path::Path::new(d).join(&unit).exists());
            Service {
                name: s.clone(),
                active: systemctl_ok(&["is-active", &unit]),
                enabled: systemctl_ok(&["is-enabled", &unit]),
                installed,
            }
        })
        .collect()
}

/// Arquivo de configuração do governor em uso (o primeiro que existe).
pub fn governor_config(b: &Board) -> Option<String> {
    b.governor_configs
        .iter()
        .find(|p| std::path::Path::new(p).is_file())
        .cloned()
}

/// Um valor numérico editável da configuração ("chave" ou "tabela.chave").
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    Num {
        key: String,
        value: f64,
        int: bool,
    },
    /// Lista de pontos [[MHz, mV], ...]
    Points {
        key: String,
        value: Vec<(f64, f64)>,
    },
}

fn walk(prefix: &str, t: &toml::Table, out: &mut Vec<Field>) {
    for (k, v) in t {
        let key = if prefix.is_empty() {
            k.clone()
        } else {
            format!("{prefix}.{k}")
        };
        match v {
            toml::Value::Integer(i) => out.push(Field::Num {
                key,
                value: *i as f64,
                int: true,
            }),
            toml::Value::Float(f) => out.push(Field::Num {
                key,
                value: *f,
                int: false,
            }),
            toml::Value::Table(t) => walk(&key, t, out),
            toml::Value::Array(a) => {
                let pts: Option<Vec<(f64, f64)>> = a
                    .iter()
                    .map(|p| {
                        let arr = p.as_array()?;
                        let n = |x: &toml::Value| {
                            x.as_float().or_else(|| x.as_integer().map(|i| i as f64))
                        };
                        Some((n(arr.first()?)?, n(arr.get(1)?)?))
                    })
                    .collect();
                if let Some(p) = pts.filter(|p| !p.is_empty()) {
                    out.push(Field::Points { key, value: p });
                }
            }
            _ => {}
        }
    }
}

pub fn fields(text: &str) -> Result<Vec<Field>, String> {
    let t: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut v = vec![];
    walk("", &t, &mut v);
    Ok(v)
}

fn set_path(t: &mut toml::Table, key: &str, v: toml::Value) {
    let mut parts: Vec<&str> = key.split('.').collect();
    let last = parts.pop().unwrap_or(key);
    let mut cur = t;
    for p in parts {
        cur = match cur
            .entry(p.to_string())
            .or_insert(toml::Value::Table(Default::default()))
        {
            toml::Value::Table(x) => x,
            _ => return,
        };
    }
    cur.insert(last.to_string(), v);
}

/// Aplica os campos editados no texto original (mantém chaves que não conhecemos).
pub fn apply_fields(text: &str, f: &[Field]) -> Result<String, String> {
    let mut t: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    for x in f {
        match x {
            Field::Num { key, value, int } => set_path(
                &mut t,
                key,
                if *int {
                    toml::Value::Integer(value.round() as i64)
                } else {
                    toml::Value::Float(*value)
                },
            ),
            Field::Points { key, value } => set_path(
                &mut t,
                key,
                toml::Value::Array(
                    value
                        .iter()
                        .map(|(a, b)| {
                            toml::Value::Array(vec![
                                toml::Value::Integer(*a as i64),
                                toml::Value::Integer(*b as i64),
                            ])
                        })
                        .collect(),
                ),
            ),
        }
    }
    toml::to_string_pretty(&t).map_err(|e| e.to_string())
}

/// Grava a configuração nova do governor (com backup) e reinicia o serviço. Precisa de root.
pub fn governor_apply(
    b: &Board,
    new_file: &std::path::Path,
    log: &mut dyn FnMut(&str),
) -> Result<(), String> {
    if !crate::util::is_root() {
        return Err("precisa de root (o botão da interface pede a senha)".into());
    }
    let dest = governor_config(b).ok_or("nenhum governor instalado")?;
    let text = std::fs::read_to_string(new_file).map_err(|e| e.to_string())?;
    toml::from_str::<toml::Table>(&text).map_err(|e| format!("configuração inválida: {e}"))?;
    let bak = format!("{dest}.numapilot-bak-{}", crate::history::now());
    std::fs::copy(&dest, &bak).map_err(|e| format!("backup: {e}"))?;
    std::fs::write(&dest, text).map_err(|e| e.to_string())?;
    log(&format!("Gravado {dest} (backup em {bak})"));
    let svc = services(b)
        .into_iter()
        .find(|s| s.active || s.enabled)
        .map(|s| s.name)
        .ok_or("nenhum serviço de governor ligado")?;
    let ok = Command::new("systemctl")
        .args(["restart", &format!("{svc}.service")])
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        std::fs::copy(&bak, &dest).ok();
        return Err(format!(
            "{svc} não reiniciou com a configuração nova: voltei a anterior"
        ));
    }
    log(&format!("{svc} reiniciado"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn governor_toml_roundtrip() {
        let src = "min_frequency = 1000\nmax_frequency = 2000\nmax_voltage = 950\n\n[timing]\ninterval_ms = 100\n\n[[unknown]]\nx = \"y\"\n\npoints = 5\n";
        let f = fields(src).unwrap();
        assert!(f.iter().any(|x| matches!(x, Field::Num { key, value, .. } if key == "max_frequency" && *value == 2000.0)));
        assert!(
            f.iter()
                .any(|x| matches!(x, Field::Num { key, .. } if key == "timing.interval_ms"))
        );
        let new = apply_fields(
            src,
            &[Field::Num {
                key: "max_frequency".into(),
                value: 2175.0,
                int: true,
            }],
        )
        .unwrap();
        assert!(new.contains("max_frequency = 2175"));
        assert!(new.contains("[[unknown]]"), "mantém o que não conhecemos");
        let pts = "safe_points = [[350, 700], [2000, 950]]\n";
        let f = fields(pts).unwrap();
        assert_eq!(
            f,
            vec![Field::Points {
                key: "safe_points".into(),
                value: vec![(350.0, 700.0), (2000.0, 950.0)]
            }]
        );
        let new = apply_fields(
            pts,
            &[Field::Points {
                key: "safe_points".into(),
                value: vec![(350.0, 700.0), (2175.0, 1050.0)],
            }],
        )
        .unwrap();
        assert!(new.contains("2175") && new.contains("1050"));
    }
}

/// Núcleos físicos ativos (pares pacote/núcleo distintos).
pub fn active_cores() -> usize {
    let mut set = std::collections::BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir(crate::util::sys("devices/system/cpu")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if !n.starts_with("cpu") || !n[3..].chars().all(|c| c.is_ascii_digit()) || n.len() == 3
            {
                continue;
            }
            let t = e.path().join("topology");
            let rd = |f: &str| {
                std::fs::read_to_string(t.join(f))
                    .ok()
                    .map(|s| s.trim().to_string())
            };
            if let (Some(p), Some(c)) = (rd("physical_package_id"), rd("core_id")) {
                set.insert((p, c));
            }
        }
    }
    set.len()
}

/// Como o amdgpu em uso lê a tabela de métricas da SMU numa BC-250 com 8 núcleos.
/// O linux-cachyos-bc250 escolhe a tabela pelo número de núcleos desde 2026-08-28
/// (`cs_eight_core_map` marca esse código) e tirou o `cs_legacy_8core_metrics` em 2026-09-17;
/// um kernel sem nenhum dos dois lê a tabela de 6 núcleos. (Lógica do BC250 Control Center,
/// MIT, https://github.com/movacx/bc250-control-center.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricsDecode {
    /// Kernel lê a tabela de 8 núcleos: a SMU precisa do "SMU Reporting Patch" no BIOS.
    EightCore,
    /// `cs_legacy_8core_metrics=1` ligado: opção antiga, removida do kernel.
    Legacy,
    /// Kernel comum: lê a tabela de 6 núcleos.
    Stock,
}

pub fn metrics_decode(legacy: Option<&str>, eight_core_map: bool) -> MetricsDecode {
    if matches!(legacy, Some("Y" | "1")) {
        MetricsDecode::Legacy
    } else if eight_core_map {
        MetricsDecode::EightCore
    } else {
        MetricsDecode::Stock
    }
}

/// O que fazer quando a telemetria vem embaralhada, para cada jeito de ler a tabela.
pub fn mismatch_fix(d: MetricsDecode) -> &'static str {
    match d {
        MetricsDecode::EightCore => {
            "o kernel lê a tabela de 8 núcleos, mas a SMU grava a de 6: ligue o \"SMU Reporting \
             Patch\" no biosmod (MeiMeiDXE v3) ou volte para 6 núcleos"
        }
        MetricsDecode::Legacy => {
            "tire amdgpu.cs_legacy_8core_metrics=1 da linha do kernel (Máquina → Parâmetros do \
             kernel): o kernel atual escolhe a tabela sozinho"
        }
        MetricsDecode::Stock => {
            "este kernel lê a tabela de 6 núcleos: desligue o \"SMU Reporting Patch\" no BIOS ou \
             use o kernel linux-cachyos-bc250"
        }
    }
}

/// BC-250 com 8 núcleos e temperatura/potência da GPU embaralhadas (0 °C, ~1470 W): a tabela
/// de métricas que a SMU grava não bate com a que o kernel lê. Os clocks continuam certos.
/// Devolve a explicação quando é esse o caso.
pub fn smu_table_mismatch(pci: &str) -> Option<String> {
    if active_cores() != 8 {
        return None;
    }
    let s = crate::gpuctl::read_sensors(pci);
    if s.edge_c.is_some() || s.power_w.is_some() {
        return None;
    }
    let param = |n: &str| crate::util::sys(&format!("module/amdgpu/parameters/{n}"));
    let legacy = crate::util::read_trim(param("cs_legacy_8core_metrics"));
    let d = metrics_decode(legacy.as_deref(), param("cs_eight_core_map").exists());
    Some(format!(
        "8 núcleos com a tabela de métricas da SMU diferente da que o kernel lê: temperatura e \
         potência da GPU vêm embaralhadas (os clocks estão certos). Para corrigir, {}. Com o \
         cyan-skillfish-governor rodando (fix-metrics), o MangoHud lê o gpu_metrics corrigido \
         por ele.",
        mismatch_fix(d)
    ))
}

/// BC-250: o amdgpu do linux-cachyos-bc250 com `bc250_cc_write_mode=3` limpa, ao carregar, os
/// bits que desligam os shader arrays: os 40 CUs já vêm do boot e o driver conta todos. Aí o
/// desbloqueio ao vivo (umr, bc250-cu-live-manager) não tem o que fazer e mexer nele por baixo
/// do driver é arriscado.
pub fn cu_unlock_by_kernel() -> bool {
    crate::util::read_trim(crate::util::sys(
        "module/amdgpu/parameters/bc250_cc_write_mode",
    ))
    .is_some_and(|v| v == "3")
}

/// Onde o EFI que desbloqueia os núcleos antes do boot (Hexxeh/bc250-efi-core-unlock) costuma
/// ficar.
const EFI_SHIM_PATHS: &[&str] = &[
    "/boot/EFI/BOOT/COREUNLOCK.EFI",
    "/boot/efi/EFI/BOOT/COREUNLOCK.EFI",
    "/efi/EFI/BOOT/COREUNLOCK.EFI",
    "/boot/EFI/BOOT/bc250-unlock.efi",
    "/boot/efi/EFI/BOOT/bc250-unlock.efi",
    "/efi/EFI/BOOT/bc250-unlock.efi",
];

/// Há o EFI de desbloqueio dos núcleos (arquivo ou entrada "CoreUnlock" no efibootmgr)?
pub fn efi_core_unlock() -> bool {
    EFI_SHIM_PATHS
        .iter()
        .any(|p| std::path::Path::new(p).is_file())
        || Command::new("efibootmgr")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .is_some_and(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .to_lowercase()
                    .contains("coreunlock")
            })
}

/// Se os núcleos desbloqueados ficam depois de desligar da tomada. Só lê, não muda nada.
pub fn core_unlock_persistence(cores: usize, full: usize, efi: bool) -> Option<&'static str> {
    if cores < full {
        return None;
    }
    Some(if efi {
        "ficam: o EFI de desbloqueio (bc250-efi-core-unlock) liga antes de cada boot"
    } else {
        "se vieram do BIOS (biosmod), ficam; se vieram do bc250-core-unlock, somem ao desligar \
         da tomada (reinício não perde) e é preciso rodar de novo"
    })
}

/// CUs ativos da GPU no boot, pela mensagem do amdgpu ("active_cu_number N"). Lido uma vez por
/// execução; um desbloqueio feito depois do boot (ao vivo, pelo umr) não aparece aqui.
pub fn active_cu() -> Option<u32> {
    static CU: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *CU.get_or_init(read_active_cu)
}

fn read_active_cu() -> Option<u32> {
    Command::new("journalctl")
        .args([
            "-k",
            "-b",
            "0",
            "-o",
            "cat",
            "--no-pager",
            "--grep",
            "active_cu_number",
        ])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .filter(|s| s.contains("active_cu_number"))
        .or_else(|| {
            Command::new("dmesg")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        })
        .and_then(|out| parse_active_cu(&out))
        .or_else(kfd_cu)
}

/// CUs pela topologia do KFD (simd_count / simd_per_cu), legível por qualquer usuário.
/// Útil quando a mensagem do boot já saiu do buffer do kernel.
fn kfd_cu() -> Option<u32> {
    let base = crate::util::sys("class/kfd/kfd/topology/nodes");
    std::fs::read_dir(base)
        .ok()?
        .flatten()
        .filter_map(|e| parse_kfd_cu(&std::fs::read_to_string(e.path().join("properties")).ok()?))
        .max()
}

pub fn parse_kfd_cu(props: &str) -> Option<u32> {
    let get = |k: &str| {
        props.lines().find_map(|l| {
            let (n, v) = l.split_once(' ')?;
            (n == k).then(|| v.trim().parse::<u32>().ok())?
        })
    };
    let (simd, per) = (get("simd_count")?, get("simd_per_cu")?);
    (simd > 0 && per > 0).then(|| simd / per)
}

pub fn parse_active_cu(log: &str) -> Option<u32> {
    log.lines()
        .filter_map(|l| l.split("active_cu_number").nth(1))
        .filter_map(|r| {
            r.trim_start_matches([':', ' ', '='])
                .split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        })
        .next_back()
}

#[cfg(test)]
mod unlock_tests {
    #[test]
    fn cu_log() {
        let l = "amdgpu 0000:01:00.0: amdgpu: SE 1, SH per SE 2, CU per SH 12, active_cu_number 24\nfoo\n";
        assert_eq!(super::parse_active_cu(l), Some(24));
        assert_eq!(super::parse_active_cu("nada"), None);
        let kfd = "cpu_cores_count 0\nsimd_count 48\nsimd_per_cu 2\n";
        assert_eq!(super::parse_kfd_cu(kfd), Some(24));
        assert_eq!(super::parse_kfd_cu("simd_count 0\nsimd_per_cu 0\n"), None);
    }

    #[test]
    fn tabela_da_smu() {
        use super::{MetricsDecode::*, metrics_decode};
        assert_eq!(metrics_decode(Some("Y"), true), Legacy);
        assert_eq!(metrics_decode(Some("N"), true), EightCore);
        assert_eq!(metrics_decode(None, true), EightCore);
        assert_eq!(metrics_decode(None, false), Stock);
        assert!(super::mismatch_fix(Stock).contains("desligue"));
        assert!(super::mismatch_fix(EightCore).contains("ligue"));
    }

    #[test]
    fn persistencia_dos_nucleos() {
        assert_eq!(super::core_unlock_persistence(6, 8, false), None);
        assert!(
            super::core_unlock_persistence(8, 8, true)
                .unwrap()
                .contains("EFI")
        );
        assert!(
            super::core_unlock_persistence(8, 8, false)
                .unwrap()
                .contains("biosmod")
        );
    }
}
