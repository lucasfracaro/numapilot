//! Limites de potência do Ryzen de notebook (e APUs) pelo ryzenadj: STAPM (sustentado),
//! PPT rápido e lento, e o limite de temperatura. É o equivalente do PL1/PL2 da Intel;
//! o RAPL do Ryzen só mede, não limita.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::process::Command;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
    /// Sustentado (STAPM), W.
    pub stapm_w: Option<f64>,
    /// Picos curtos (PPT rápido), W.
    pub fast_w: Option<f64>,
    /// Média de alguns segundos (PPT lento), W.
    pub slow_w: Option<f64>,
    /// Temperatura máxima (Tctl), °C.
    pub tctl_c: Option<f64>,
}

impl Limits {
    pub fn is_empty(&self) -> bool {
        *self == Limits::default()
    }

    pub fn validate(&self) -> Result<(), String> {
        for (n, v) in [
            ("STAPM", self.stapm_w),
            (crate::i18n::tr("PPT rápido"), self.fast_w),
            (crate::i18n::tr("PPT lento"), self.slow_w),
        ] {
            if let Some(v) = v
                && !(5.0..=200.0).contains(&v)
            {
                return Err(crate::tf!(
                    "{n}: {v} W fora da faixa 5-200 W",
                    "{n}: {v} W out of the 5-200 W range"
                ));
            }
        }
        if let Some(t) = self.tctl_c
            && !(60.0..=105.0).contains(&t)
        {
            return Err(crate::tf!(
                "temperatura máxima {t} °C fora da faixa 60-105 °C",
                "maximum temperature {t} °C out of the 60-105 °C range"
            ));
        }
        Ok(())
    }

    /// Junta: o que está em `self`, senão o de `base`.
    pub fn or(&self, base: &Limits) -> Limits {
        Limits {
            stapm_w: self.stapm_w.or(base.stapm_w),
            fast_w: self.fast_w.or(base.fast_w),
            slow_w: self.slow_w.or(base.slow_w),
            tctl_c: self.tctl_c.or(base.tctl_c),
        }
    }

    /// Diferença relevante (meio watt / meio grau) entre o pedido e o lido.
    pub fn differs(&self, now: &Limits) -> bool {
        let d = |a: Option<f64>, b: Option<f64>| matches!((a, b), (Some(a), Some(b)) if (a - b).abs() > 0.5);
        d(self.stapm_w, now.stapm_w)
            || d(self.fast_w, now.fast_w)
            || d(self.slow_w, now.slow_w)
            || d(self.tctl_c, now.tctl_c)
    }
}

fn bin() -> String {
    std::env::var("NUMAPILOT_RYZENADJ").unwrap_or_else(|_| "ryzenadj".into())
}

pub fn available() -> bool {
    std::env::var_os("NUMAPILOT_RYZENADJ").is_some() || crate::containers::which("ryzenadj")
}

/// Só faz sentido em Ryzen de notebook/APU (o ryzenadj não mexe em Ryzen de desktop nem em EPYC).
pub fn applicable() -> bool {
    let m = crate::recipes::cpu_model();
    m.contains("AMD")
        && (crate::notebook::is_laptop() || m.contains("with Radeon") || m.contains("APU"))
}

/// Tabela do `ryzenadj -i`: "| STAPM LIMIT | 45.000 | stapm-limit |" -> nome -> valor.
pub fn parse_info(s: &str) -> BTreeMap<String, f64> {
    let mut m = BTreeMap::new();
    for l in s.lines() {
        let cols: Vec<&str> = l
            .split('|')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .collect();
        if cols.len() >= 2
            && let Ok(v) = cols[1].parse::<f64>()
            && v.is_finite()
        {
            m.insert(cols[0].to_string(), v);
        }
    }
    m
}

pub fn limits_from(info: &BTreeMap<String, f64>) -> Limits {
    Limits {
        stapm_w: info.get("STAPM LIMIT").copied(),
        fast_w: info.get("PPT LIMIT FAST").copied(),
        slow_w: info.get("PPT LIMIT SLOW").copied(),
        tctl_c: info.get("THM LIMIT CORE").copied(),
    }
}

/// Lê a tabela (precisa de root; usa o módulo ryzen_smu ou /dev/mem).
pub fn info() -> Result<BTreeMap<String, f64>, String> {
    let o = Command::new(bin())
        .arg("-i")
        .output()
        .map_err(|e| format!("ryzenadj: {e}"))?;
    let m = parse_info(&String::from_utf8_lossy(&o.stdout));
    if m.is_empty() {
        let err = String::from_utf8_lossy(&o.stderr);
        return Err(crate::tf!(
            "ryzenadj -i não leu nada: {}",
            "ryzenadj -i read nothing: {}",
            err.lines().next_back().unwrap_or("")
        ));
    }
    Ok(m)
}

pub fn args(l: &Limits) -> Vec<String> {
    let mut a = vec![];
    let mw = |w: f64| ((w * 1000.0).round() as u64).to_string();
    if let Some(w) = l.stapm_w {
        a.push(format!("--stapm-limit={}", mw(w)));
    }
    if let Some(w) = l.fast_w {
        a.push(format!("--fast-limit={}", mw(w)));
    }
    if let Some(w) = l.slow_w {
        a.push(format!("--slow-limit={}", mw(w)));
    }
    if let Some(t) = l.tctl_c {
        a.push(format!("--tctl-temp={}", t.round() as u64));
    }
    a
}

pub fn apply(l: &Limits) -> Result<(), String> {
    l.validate()?;
    let a = args(l);
    if a.is_empty() {
        return Ok(());
    }
    let o = Command::new(bin())
        .args(&a)
        .output()
        .map_err(|e| format!("ryzenadj: {e}"))?;
    let out = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    if !o.status.success() || out.contains("not supported") || out.contains("Failed") {
        return Err(crate::tf!(
            "ryzenadj recusou: {}",
            "ryzenadj refused: {}",
            out.lines().rfind(|l| !l.trim().is_empty()).unwrap_or("?")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let s = "CPU Family: Rembrandt\n| Name | Value | Parameter |\n|---|---|---|\n| STAPM LIMIT | 45.000 | stapm-limit |\n| STAPM VALUE | 10.352 | |\n| PPT LIMIT FAST | 65.000 | fast-limit |\n| PPT LIMIT SLOW | 54.000 | slow-limit |\n| StapmTimeConst | nan | stapm-time |\n| THM LIMIT CORE | 95.000 | tctl-temp |\n";
        let i = parse_info(s);
        assert!(!i.contains_key("StapmTimeConst"));
        let l = limits_from(&i);
        assert_eq!(
            l,
            Limits {
                stapm_w: Some(45.0),
                fast_w: Some(65.0),
                slow_w: Some(54.0),
                tctl_c: Some(95.0)
            }
        );
        assert_eq!(
            args(&Limits {
                stapm_w: Some(35.0),
                tctl_c: Some(85.0),
                ..Default::default()
            }),
            vec!["--stapm-limit=35000", "--tctl-temp=85"]
        );
        assert!(
            Limits {
                fast_w: Some(500.0),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Limits {
                stapm_w: Some(35.0),
                ..Default::default()
            }
            .differs(&l)
        );
    }
}
