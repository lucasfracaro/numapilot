//! Containers (podman ou docker): lista, apps instalados em cada um e como abri-los.

use serde_json::Value;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// Laboratório do numapilot (Jupyter, VS Code, PyCharm).
    Lab,
    /// Servidor de IA (llama.cpp).
    Llm,
    /// Distrobox: compartilha a tela e a pasta pessoal, então os apps abrem direto.
    Distrobox,
    Other,
}

#[derive(Debug, Clone)]
pub struct Port {
    pub host_ip: String,
    pub host_port: u16,
    pub container_port: u16,
}

#[derive(Debug, Clone)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    pub running: bool,
    pub status: String,
    pub kind: Kind,
    pub ports: Vec<Port>,
}

#[derive(Debug, Clone, Default)]
pub struct Apps {
    /// Apps com atalho (.desktop): (nome, comando)
    pub desktop: Vec<(String, String)>,
    /// IDEs / ferramentas conhecidas encontradas: (nome, caminho)
    pub tools: Vec<(String, String)>,
    /// Pacotes Python: (nome, versão)
    pub python: Vec<(String, String)>,
    /// Pacotes do sistema instalados explicitamente
    pub system: Vec<String>,
    pub error: Option<String>,
}

pub fn engine() -> Option<String> {
    if let Ok(e) = std::env::var("NUMAPILOT_ENGINE") {
        return Some(e);
    }
    ["podman", "docker"]
        .iter()
        .find(|e| which(e))
        .map(|e| e.to_string())
}

pub fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

fn kind_of(name: &str, labels: &Value, lab: &str, llm: &str) -> Kind {
    if name == lab {
        Kind::Lab
    } else if name == llm {
        Kind::Llm
    } else if labels.get("manager").and_then(Value::as_str) == Some("distrobox")
        || labels.get("distrobox.unshare_groups").is_some()
    {
        Kind::Distrobox
    } else {
        Kind::Other
    }
}

/// "127.0.0.1:8888->8888/tcp, 0.0.0.0:2222->2222/tcp" (formato do docker)
fn parse_docker_ports(s: &str) -> Vec<Port> {
    s.split(',')
        .filter_map(|p| {
            let (host, cont) = p.trim().split_once("->")?;
            let (ip, hp) = host.rsplit_once(':')?;
            let cp = cont.split('/').next()?;
            Some(Port {
                host_ip: ip.to_string(),
                host_port: hp.parse().ok()?,
                container_port: cp.parse().ok()?,
            })
        })
        .collect()
}

pub fn parse_list(engine: &str, out: &str, lab: &str, llm: &str) -> Vec<Container> {
    let mut v = Vec::new();
    if engine.ends_with("podman") {
        let Ok(Value::Array(arr)) = serde_json::from_str::<Value>(out) else {
            return v;
        };
        for c in arr {
            let name = c["Names"]
                .get(0)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let ports = c["Ports"]
                .as_array()
                .map(|ps| {
                    ps.iter()
                        .filter_map(|p| {
                            Some(Port {
                                host_ip: p["host_ip"].as_str().unwrap_or("").to_string(),
                                host_port: p["host_port"].as_u64()? as u16,
                                container_port: p["container_port"].as_u64()? as u16,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            v.push(Container {
                id: c["Id"].as_str().unwrap_or("").chars().take(12).collect(),
                kind: kind_of(&name, &c["Labels"], lab, llm),
                name,
                image: c["Image"].as_str().unwrap_or("").to_string(),
                running: c["State"].as_str() == Some("running"),
                status: c["Status"].as_str().unwrap_or("").to_string(),
                ports,
            });
        }
    } else {
        for line in out.lines() {
            let Ok(c) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let name = c["Names"].as_str().unwrap_or("").to_string();
            let labels: serde_json::Map<String, Value> = c["Labels"]
                .as_str()
                .unwrap_or("")
                .split(',')
                .filter_map(|kv| {
                    kv.split_once('=')
                        .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                })
                .collect();
            v.push(Container {
                id: c["ID"].as_str().unwrap_or("").to_string(),
                kind: kind_of(&name, &Value::Object(labels), lab, llm),
                name,
                image: c["Image"].as_str().unwrap_or("").to_string(),
                running: c["State"].as_str() == Some("running"),
                status: c["Status"].as_str().unwrap_or("").to_string(),
                ports: parse_docker_ports(c["Ports"].as_str().unwrap_or("")),
            });
        }
    }
    v.sort_by_key(|c| (c.kind != Kind::Lab, c.kind != Kind::Llm, c.name.clone()));
    v
}

pub fn list(lab: &str, llm: &str) -> Result<Vec<Container>, String> {
    let eng = engine().ok_or(crate::i18n::tr("podman/docker não instalado"))?;
    let args: &[&str] = if eng.ends_with("podman") {
        &["ps", "-a", "--format", "json"]
    } else {
        &["ps", "-a", "--format", "{{json .}}"]
    };
    let out = Command::new(&eng)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(crate::tf!("{eng} ps falhou", "{eng} ps failed"));
    }
    Ok(parse_list(
        &eng,
        &String::from_utf8_lossy(&out.stdout),
        lab,
        llm,
    ))
}

/// Script rodado dentro do container para descobrir o que está instalado.
pub const INSPECT_SCRIPT: &str = r#"
echo '##DESKTOP'
for f in /usr/share/applications/*.desktop /usr/local/share/applications/*.desktop /opt/*/share/applications/*.desktop; do
  [ -f "$f" ] || continue
  grep -q '^NoDisplay=true' "$f" && continue
  n=$(grep -m1 '^Name=' "$f" | cut -d= -f2-); e=$(grep -m1 '^Exec=' "$f" | cut -d= -f2-)
  [ -n "$n" ] && [ -n "$e" ] && printf '%s\t%s\n' "$n" "$e"
done
echo '##TOOLS'
for b in jupyter-lab jupyter code-server code codium pycharm pycharm.sh spyder rstudio nvtop htop python3 R julia; do
  p=$(command -v "$b" 2>/dev/null) && printf '%s\t%s\n' "$b" "$p"
done
for p in /opt/pycharm/bin/pycharm /opt/pycharm/bin/pycharm.sh; do [ -x "$p" ] && printf 'pycharm\t%s\n' "$p"; done
echo '##PYTHON'
python3 -m pip list --format=freeze 2>/dev/null
echo '##SYSTEM'
(apt-mark showmanual 2>/dev/null || pacman -Qqe 2>/dev/null || dnf repoquery --userinstalled --qf '%{name}' 2>/dev/null) | head -n 1000
"#;

pub fn parse_apps(out: &str) -> Apps {
    let mut a = Apps::default();
    let mut sec = "";
    for line in out.lines() {
        if let Some(s) = line.strip_prefix("##") {
            sec = match s.trim() {
                "DESKTOP" => "d",
                "TOOLS" => "t",
                "PYTHON" => "p",
                "SYSTEM" => "s",
                _ => "",
            };
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        match sec {
            "d" => {
                if let Some((n, e)) = line.split_once('\t') {
                    a.desktop.push((n.to_string(), e.to_string()));
                }
            }
            "t" => {
                if let Some((n, p)) = line.split_once('\t')
                    && !a.tools.iter().any(|(_, q)| q == p)
                {
                    a.tools.push((n.to_string(), p.to_string()));
                }
            }
            "p" => {
                if let Some((n, v)) = line.split_once("==") {
                    a.python.push((n.to_string(), v.to_string()));
                } else if let Some((n, v)) = line.split_once(" @ ") {
                    a.python.push((n.to_string(), v.to_string()));
                }
            }
            "s" => a.system.push(line.trim().to_string()),
            _ => {}
        }
    }
    a.desktop.sort();
    a.desktop.dedup();
    a
}

pub fn inspect_apps(c: &Container) -> Apps {
    let Some(eng) = engine() else {
        return Apps {
            error: Some(crate::i18n::tr("sem podman/docker").into()),
            ..Default::default()
        };
    };
    if !c.running {
        return Apps {
            error: Some(crate::i18n::tr("ligue o container para ver os apps").into()),
            ..Default::default()
        };
    }
    match Command::new(&eng)
        .args(["exec", &c.id, "sh", "-c", INSPECT_SCRIPT])
        .stderr(Stdio::null())
        .output()
    {
        Ok(o) => parse_apps(&String::from_utf8_lossy(&o.stdout)),
        Err(e) => Apps {
            error: Some(e.to_string()),
            ..Default::default()
        },
    }
}

/// Remove os códigos de campo (%f, %U...) de uma linha Exec=.
pub fn clean_exec(exec: &str) -> String {
    exec.split_whitespace()
        .filter(|t| !(t.len() == 2 && t.starts_with('%')))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Comando (no host) que abre um app gráfico de dentro do container, se for possível.
pub fn open_command(c: &Container, exec: &str) -> Option<Vec<String>> {
    let eng = engine()?;
    let exec = clean_exec(exec);
    match c.kind {
        Kind::Distrobox => Some(vec![
            "distrobox".into(),
            "enter".into(),
            c.name.clone(),
            "--".into(),
            "sh".into(),
            "-c".into(),
            exec,
        ]),
        Kind::Lab => {
            let mut v = vec![eng, "exec".into(), "-d".into()];
            v.push("-e".into());
            v.push(format!(
                "DISPLAY={}",
                std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into())
            ));
            v.push("-e".into());
            v.push("XAUTHORITY=/lab/Xauthority".into());
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                v.extend([
                    "-e".into(),
                    "XDG_RUNTIME_DIR=/lab".into(),
                    "-e".into(),
                    "WAYLAND_DISPLAY=wayland-0".into(),
                ]);
            }
            v.extend([c.id.clone(), "sh".into(), "-c".into(), exec]);
            Some(v)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn podman_and_docker_lists() {
        let podman = r#"[{"Id":"abcdef1234567890","Names":["gfx906-lab"],"Image":"localhost/gfx906-lab:latest","State":"running","Status":"Up 2 hours","Labels":{},"Ports":[{"host_ip":"127.0.0.1","container_port":8888,"host_port":8888,"range":1,"protocol":"tcp"}]},
          {"Id":"1111","Names":["arch"],"Image":"archlinux","State":"exited","Status":"Exited","Labels":{"manager":"distrobox"},"Ports":null}]"#;
        let v = parse_list("podman", podman, "gfx906-lab", "gfx906-llm");
        assert_eq!(v.len(), 2);
        assert_eq!(
            (v[0].kind, v[0].running, v[0].ports[0].host_port),
            (Kind::Lab, true, 8888)
        );
        assert_eq!((v[1].kind, v[1].running), (Kind::Distrobox, false));
        let docker = r#"{"ID":"f00","Names":"gfx906-llm","Image":"x","State":"running","Status":"Up","Labels":"io.numapilot.model=a.gguf","Ports":"127.0.0.1:8080->8080/tcp"}"#;
        let v = parse_list("docker", docker, "gfx906-lab", "gfx906-llm");
        assert_eq!(
            (
                v[0].kind,
                v[0].ports[0].host_port,
                v[0].ports[0].container_port
            ),
            (Kind::Llm, 8080, 8080)
        );
    }

    #[test]
    fn apps_parsing() {
        let out = "##DESKTOP\nGIMP\tgimp %U\n##TOOLS\njupyter-lab\t/usr/local/bin/jupyter-lab\npython3\t/usr/bin/python3\npycharm\t/opt/pycharm/bin/pycharm\n##PYTHON\nnumpy==2.5.4\ntorch==2.14.1+rocm10.1\nfoo @ file:///x\n##SYSTEM\ngit\nhtop\n";
        let a = parse_apps(out);
        assert_eq!(a.desktop, vec![("GIMP".to_string(), "gimp %U".to_string())]);
        assert_eq!(a.tools.len(), 3);
        assert_eq!(a.python.len(), 3);
        assert_eq!(a.system, vec!["git", "htop"]);
        assert_eq!(clean_exec("gimp %U --new"), "gimp --new");
    }
}
