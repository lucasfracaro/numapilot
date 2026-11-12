//! Steam: lista os jogos instalados e coloca `numapilot run -- %command%` nas opções de
//! inicialização de todos eles (editando o localconfig.vdf com a Steam fechada, com backup).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---------------- VDF (KeyValues em texto) ----------------

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Str(String),
    Obj(Vec<(String, Vdf)>),
}

fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = s.chars().peekable();
    while let Some(&c) = it.peek() {
        match c {
            '"' => {
                it.next();
                let mut t = String::new();
                while let Some(c) = it.next() {
                    match c {
                        '\\' => {
                            if let Some(n) = it.next() {
                                t.push(match n {
                                    'n' => '\n',
                                    't' => '\t',
                                    o => o,
                                });
                            }
                        }
                        '"' => break,
                        o => t.push(o),
                    }
                }
                out.push(format!("\"{t}"));
            }
            '{' | '}' => {
                out.push(c.to_string());
                it.next();
            }
            '/' => {
                // comentário até o fim da linha
                for c in it.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {
                it.next();
            }
            _ => {
                let mut t = String::from("\"");
                while let Some(&c) = it.peek() {
                    if c.is_whitespace() || c == '{' || c == '}' || c == '"' {
                        break;
                    }
                    t.push(c);
                    it.next();
                }
                out.push(t);
            }
        }
    }
    out
}

fn parse_obj(t: &[String], i: &mut usize) -> Vec<(String, Vdf)> {
    let mut v = Vec::new();
    while *i < t.len() {
        if t[*i] == "}" {
            *i += 1;
            break;
        }
        let key = t[*i].trim_start_matches('"').to_string();
        *i += 1;
        if *i >= t.len() {
            break;
        }
        if t[*i] == "{" {
            *i += 1;
            v.push((key, Vdf::Obj(parse_obj(t, i))));
        } else {
            v.push((key, Vdf::Str(t[*i].trim_start_matches('"').to_string())));
            *i += 1;
        }
    }
    v
}

pub fn parse(s: &str) -> Vdf {
    let t = tokens(s);
    let mut i = 0;
    Vdf::Obj(parse_obj(&t, &mut i))
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_obj(v: &[(String, Vdf)], depth: usize, out: &mut String) {
    let ind = "\t".repeat(depth);
    for (k, val) in v {
        match val {
            Vdf::Str(s) => out.push_str(&format!("{ind}\"{}\"\t\t\"{}\"\n", esc(k), esc(s))),
            Vdf::Obj(o) => {
                out.push_str(&format!("{ind}\"{}\"\n{ind}{{\n", esc(k)));
                write_obj(o, depth + 1, out);
                out.push_str(&format!("{ind}}}\n"));
            }
        }
    }
}

pub fn to_string(v: &Vdf) -> String {
    let mut s = String::new();
    if let Vdf::Obj(o) = v {
        write_obj(o, 0, &mut s);
    }
    s
}

impl Vdf {
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Obj(o) => o
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Vdf> {
        match self {
            Vdf::Obj(o) => o
                .iter_mut()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            _ => None,
        }
    }
    /// Pega (ou cria) um objeto filho.
    pub fn child(&mut self, key: &str) -> &mut Vdf {
        if self.get(key).is_none()
            && let Vdf::Obj(o) = self
        {
            o.push((key.to_string(), Vdf::Obj(vec![])));
        }
        self.get_mut(key).unwrap()
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key) {
            Some(Vdf::Str(s)) => Some(s),
            _ => None,
        }
    }
    pub fn set_str(&mut self, key: &str, val: &str) {
        if let Some(Vdf::Str(s)) = self.get_mut(key) {
            *s = val.to_string();
        } else if let Vdf::Obj(o) = self {
            o.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
            o.push((key.to_string(), Vdf::Str(val.to_string())));
        }
    }
    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Obj(o) => o,
            _ => &[],
        }
    }
}

// ---------------- Steam ----------------

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn roots() -> Vec<PathBuf> {
    let h = home();
    let mut v: Vec<PathBuf> = [
        ".local/share/Steam",
        ".steam/steam",
        ".steam/root",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
    ]
    .iter()
    .map(|p| h.join(p))
    .filter(|p| p.join("steamapps").is_dir() || p.join("userdata").is_dir())
    .filter_map(|p| p.canonicalize().ok())
    .collect();
    v.sort();
    v.dedup();
    v
}

pub fn libraries() -> Vec<PathBuf> {
    let mut libs = Vec::new();
    for r in roots() {
        libs.push(r.join("steamapps"));
        if let Ok(s) = std::fs::read_to_string(r.join("steamapps/libraryfolders.vdf")) {
            let v = parse(&s);
            if let Some(lf) = v.get("libraryfolders") {
                for (_, lib) in lf.entries() {
                    if let Some(p) = lib.str("path") {
                        libs.push(Path::new(p).join("steamapps"));
                    }
                }
            }
        }
    }
    libs.retain(|p| p.is_dir());
    libs.sort();
    libs.dedup();
    libs
}

fn is_tool(name: &str) -> bool {
    [
        "Proton",
        "Steam Linux Runtime",
        "Steamworks Common",
        "SteamVR",
    ]
    .iter()
    .any(|t| name.contains(t))
}

/// (appid, nome) dos jogos instalados (sem Proton/runtimes).
pub fn installed_games() -> Vec<(String, String)> {
    let mut v = Vec::new();
    for lib in libraries() {
        let Ok(rd) = std::fs::read_dir(&lib) else {
            continue;
        };
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if !(n.starts_with("appmanifest_") && n.ends_with(".acf")) {
                continue;
            }
            let Ok(s) = std::fs::read_to_string(e.path()) else {
                continue;
            };
            let m = parse(&s);
            let Some(st) = m.get("AppState") else {
                continue;
            };
            if let (Some(id), Some(name)) = (st.str("appid"), st.str("name"))
                && !is_tool(name)
            {
                v.push((id.to_string(), name.to_string()));
            }
        }
    }
    v.sort_by_key(|a| a.1.to_lowercase());
    v.dedup_by(|a, b| a.0 == b.0);
    v
}

pub fn find_localconfigs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for r in roots() {
        if let Ok(rd) = std::fs::read_dir(r.join("userdata")) {
            for e in rd.flatten() {
                let p = e.path().join("config/localconfig.vdf");
                if p.is_file() {
                    v.push(p);
                }
            }
        }
    }
    // A conta usada por último primeiro.
    v.sort_by_key(|p| std::cmp::Reverse(p.metadata().and_then(|m| m.modified()).ok()));
    v
}

fn apps_mut(root: &mut Vdf) -> &mut Vdf {
    root.child("UserLocalConfigStore")
        .child("Software")
        .child("Valve")
        .child("Steam")
        .child("apps")
}

pub fn read_launch_options(path: &Path) -> Option<HashMap<String, String>> {
    let s = std::fs::read_to_string(path).ok()?;
    let v = parse(&s);
    let apps = v
        .get("UserLocalConfigStore")?
        .get("Software")?
        .get("Valve")?
        .get("Steam")?
        .get("apps")?;
    Some(
        apps.entries()
            .iter()
            .filter_map(|(id, a)| a.str("LaunchOptions").map(|o| (id.clone(), o.to_string())))
            .collect(),
    )
}

pub const WRAP: &str = "numapilot run --";

pub fn has_numapilot(opt: &str) -> bool {
    opt.contains("numapilot run")
}

/// Coloca o numapilot antes do %command%, preservando o que o usuário já tinha.
pub fn wrap_option(opt: &str) -> String {
    let o = opt.trim();
    if has_numapilot(o) {
        o.to_string()
    } else if o.is_empty() {
        format!("{WRAP} %command%")
    } else if o.contains("%command%") {
        o.replacen("%command%", &format!("{WRAP} %command%"), 1)
    } else {
        // Só argumentos do jogo (sem %command%): a Steam os coloca no fim.
        format!("{WRAP} %command% {o}")
    }
}

pub fn unwrap_option(opt: &str) -> String {
    let o = opt.replace(&format!("{WRAP} "), "").replace(WRAP, "");
    let o = o.trim().to_string();
    if o == "%command%" {
        String::new()
    } else if let Some(rest) = o.strip_prefix("%command% ") {
        // "%command% -arg" equivale a "-arg"
        rest.to_string()
    } else {
        o
    }
}

pub fn steam_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-x", "steam"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Liga (ou desliga) o numapilot nas opções de inicialização de todos os jogos instalados.
/// Retorna quantos jogos mudaram.
pub fn apply_all(path: &Path, games: &[(String, String)], enable: bool) -> Result<usize, String> {
    if steam_running() {
        return Err(
            crate::i18n::tr("Close Steam first (it rewrites this configuration on exit).").into(),
        );
    }
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let ts = crate::history::now();
    std::fs::write(path.with_extension(format!("vdf.numapilot-bak-{ts}")), &s)
        .map_err(|e| format!("backup: {e}"))?;
    let mut v = parse(&s);
    let apps = apps_mut(&mut v);
    let mut changed = 0;
    for (id, _) in games {
        let app = apps.child(id);
        let cur = app.str("LaunchOptions").unwrap_or("").to_string();
        let new = if enable {
            wrap_option(&cur)
        } else {
            unwrap_option(&cur)
        };
        if new != cur {
            app.set_str("LaunchOptions", &new);
            changed += 1;
        }
    }
    std::fs::write(path, to_string(&v)).map_err(|e| e.to_string())?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LC: &str = r#""UserLocalConfigStore"
{
	"Software"
	{
		"Valve"
		{
			"Steam"
			{
				"apps"
				{
					"1245620"
					{
						"LastPlayed"		"1700000000"
						"LaunchOptions"		"PROTON_LOG=1 %command% -dx12"
					}
					"1091500"
					{
						"LaunchOptions"		"-skipStartScreen"
					}
				}
			}
		}
	}
	"friends"
	{
		"x"		"a \"quoted\" value"
	}
}
"#;

    #[test]
    fn vdf_roundtrip_and_options() {
        let v = parse(LC);
        let s = to_string(&v);
        assert_eq!(parse(&s), v, "ida e volta preserva tudo");
        assert_eq!(
            v.get("UserLocalConfigStore")
                .unwrap()
                .get("friends")
                .unwrap()
                .str("x"),
            Some("a \"quoted\" value")
        );

        assert_eq!(wrap_option(""), "numapilot run -- %command%");
        assert_eq!(
            wrap_option("PROTON_LOG=1 %command% -dx12"),
            "PROTON_LOG=1 numapilot run -- %command% -dx12"
        );
        assert_eq!(
            wrap_option("-skipStartScreen"),
            "numapilot run -- %command% -skipStartScreen"
        );
        assert_eq!(
            wrap_option("numapilot run -- %command%"),
            "numapilot run -- %command%"
        );
        assert_eq!(
            unwrap_option("PROTON_LOG=1 numapilot run -- %command% -dx12"),
            "PROTON_LOG=1 %command% -dx12"
        );
        assert_eq!(unwrap_option("numapilot run -- %command%"), "");
    }

    #[test]
    fn apply_to_file() {
        let d = std::env::temp_dir().join("numapilot-steam-test");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("localconfig.vdf");
        std::fs::write(&p, LC).unwrap();
        let games = vec![
            ("1245620".to_string(), "ELDEN RING".to_string()),
            ("1091500".into(), "Cyberpunk 2077".into()),
            ("999".into(), "Novo".into()),
        ];
        if steam_running() {
            return;
        }
        assert_eq!(apply_all(&p, &games, true).unwrap(), 3);
        let o = read_launch_options(&p).unwrap();
        assert_eq!(
            o["1245620"],
            "PROTON_LOG=1 numapilot run -- %command% -dx12"
        );
        assert_eq!(o["999"], "numapilot run -- %command%");
        assert_eq!(apply_all(&p, &games, true).unwrap(), 0, "idempotente");
        assert_eq!(apply_all(&p, &games, false).unwrap(), 3);
        let o = read_launch_options(&p).unwrap();
        assert_eq!(o["1091500"], "-skipStartScreen");
        assert_eq!(o["999"], "");
        assert!(
            std::fs::read_dir(&d)
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().contains("numapilot-bak"))
        );
    }
}
