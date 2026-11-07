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
