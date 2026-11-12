//! Programas instalados, para escolher processador, memória e placa de um programa sem precisar
//! abri-lo antes: atalhos do menu (pacotes, AUR, Flatpak, Snap, AppImage integrado), jogos da
//! Steam e AppImages soltos. Para cada um calcula a mesma chave que o serviço usa quando o
//! programa está rodando, para a escolha salva valer na próxima abertura.

use crate::util::basename;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct App {
    pub name: String,
    /// "Menu", "Flatpak", "Snap", "Steam", "AppImage"
    pub source: &'static str,
    /// Chave do perfil (executável, script ou steam:ID).
    pub key: String,
    /// O que mostrar embaixo do nome (executável, id do Flatpak...).
    pub detail: String,
    pub desktop: Option<PathBuf>,
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/".into()))
}

/// Valor de uma chave do grupo [Desktop Entry].
fn dget<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut main = false;
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            main = l == "[Desktop Entry]";
            continue;
        }
        if main
            && let Some((k, v)) = l.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim());
        }
    }
    None
}

/// Nome no idioma do sistema (Name[pt_BR], Name[pt]) ou o Name.
fn local_name<'a>(text: &'a str) -> Option<&'a str> {
    let lang = std::env::var("LANG").unwrap_or_default();
    let full = lang.split('.').next().unwrap_or("");
    let short = full.split('_').next().unwrap_or("");
    [full, short]
        .iter()
        .filter(|l| !l.is_empty())
        .find_map(|l| dget(text, &format!("Name[{l}]")))
        .or_else(|| dget(text, "Name"))
}

fn which(prog: &str) -> Option<PathBuf> {
    if prog.contains('/') {
        let p = PathBuf::from(prog);
        return p.exists().then_some(p);
    }
    let h = home();
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain([
            h.join(".local/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
        ])
        .map(|d| d.join(prog))
        .find(|p| p.is_file())
}

fn is_elf(p: &Path) -> bool {
    use std::io::Read;
    let mut b = [0u8; 4];
    std::fs::File::open(p)
        .and_then(|mut f| f.read_exact(&mut b))
        .is_ok()
        && b == [0x7f, b'E', b'L', b'F']
}

/// Num script de atalho (ex.: /usr/bin/firefox), o programa que ele abre com `exec`:
/// é esse que aparece rodando. Só caminhos absolutos que existem.
pub fn script_exec_target(text: &str, exists: &dyn Fn(&str) -> bool) -> Option<String> {
    for l in text.lines().map(str::trim) {
        let Some(rest) = l.strip_prefix("exec ") else {
            continue;
        };
        let mut skip = false;
        for tok in rest.split_whitespace() {
            let t = tok.trim_matches(|c| c == '"' || c == '\'');
            if skip {
                skip = false;
                continue;
            }
            if t.starts_with('-') || t.contains('=') {
                // "exec -a nome programa": o nome vem depois do -a
                skip = t == "-a";
                continue;
            }
            if t.starts_with('/') && !t.contains('$') && exists(t) {
                return Some(t.to_string());
            }
            // "exec electron37 /usr/lib/app.asar": não é caminho absoluto, deixa para o shebang
            break;
        }
    }
    None
}

/// Chave que o programa de um `Exec=` vai ter quando estiver rodando, e o que mostrar.
pub fn key_for_exec(exec: &str) -> Option<(String, String)> {
    // tira os códigos de campo do .desktop (%U, %f...)
    let toks: Vec<String> = crate::origin::split_command(exec)
        .into_iter()
        .filter(|t| !(t.len() == 2 && t.starts_with('%')))
        .collect();
    let (kind, prog) = crate::origin::program_of(&toks.join(" "))?;
    if kind == "flatpak" {
        // o comando vem do atalho (--command=), do metadata do Flatpak ou, sem nenhum, do id
        let cmd = toks
            .iter()
            .find_map(|t| t.strip_prefix("--command="))
            .map(|c| basename(c).to_string())
            .or_else(|| flatpak_command(&prog))
            .unwrap_or_else(|| prog.rsplit('.').next().unwrap_or(&prog).to_lowercase());
        return Some((format!("/app/bin/{cmd}"), format!("Flatpak {prog}")));
    }
    // posição do programa de verdade na linha (depois de env VAR=...)
    let start = toks
        .iter()
        .position(|t| basename(t) != "env" && !(t.contains('=') && !t.starts_with('/')))
        .unwrap_or(0);
    let argv: Vec<String> = toks[start..].to_vec();
    let first = argv.first()?;
    let exe = which(first)?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let exe_s = exe.to_string_lossy().to_string();
    if is_elf(&exe) {
        let key = crate::procfs::learn_key(Some(&exe_s), &argv, basename(&exe_s));
        return Some((key, exe_s));
    }
    // script: o que ele abre com exec, ou o interpretador + nome do script
    let text = std::fs::read_to_string(&exe).unwrap_or_default();
    if let Some(t) = script_exec_target(&text, &|p| Path::new(p).is_file()) {
        let t = std::fs::canonicalize(&t)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(t);
        if is_elf(Path::new(&t)) {
            return Some((t.clone(), t));
        }
    }
    let interp = text
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("#!"))
        .map(|l| {
            let mut it = l.split_whitespace();
            let a = it.next().unwrap_or("");
            // #!/usr/bin/env python3
            if basename(a) == "env" {
                it.next().unwrap_or(a).to_string()
            } else {
                a.to_string()
            }
        })
        .and_then(|i| which(&i))
        .map(|p| {
            std::fs::canonicalize(&p)
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        });
    match interp {
        Some(i) => {
            let key =
                crate::procfs::learn_key(Some(&i), &[i.clone(), exe_s.clone()], basename(&exe_s));
            Some((key, exe_s))
        }
        None => Some((exe_s.clone(), exe_s)),
    }
}

/// `command=` do metadata de um Flatpak instalado (sistema ou usuário).
fn flatpak_command(id: &str) -> Option<String> {
    for base in [
        PathBuf::from("/var/lib/flatpak/app"),
        home().join(".local/share/flatpak/app"),
    ] {
        let t = std::fs::read_to_string(base.join(id).join("current/active/metadata")).ok();
        if let Some(c) = t.as_deref().and_then(parse_flatpak_command) {
            return Some(c);
        }
    }
    None
}

pub fn parse_flatpak_command(metadata: &str) -> Option<String> {
    let mut app = false;
    for l in metadata.lines().map(str::trim) {
        if l.starts_with('[') {
            app = l == "[Application]";
        } else if app && let Some(c) = l.strip_prefix("command=") {
            return Some(basename(c.trim()).to_string());
        }
    }
    None
}

fn desktop_apps(out: &mut Vec<App>) {
    let mut seen = BTreeSet::new();
    for dir in crate::desktop::search_dirs() {
        let ds = dir.to_string_lossy().to_string();
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let f = e.file_name().to_string_lossy().to_string();
            // o primeiro da lista ganha (o do usuário esconde o do sistema)
            if !seen.insert(f.clone()) {
                continue;
            }
            let Ok(t) = std::fs::read_to_string(&p) else {
                continue;
            };
            if dget(&t, "NoDisplay") == Some("true")
                || dget(&t, "Hidden") == Some("true")
                || dget(&t, "Type").is_some_and(|x| x != "Application")
            {
                continue;
            }
            let Some(exec) = dget(&t, "Exec") else {
                continue;
            };
            // atalho já passado pelo numapilot (menu ajustado): olha o comando original
            let exec = dget(&t, "X-Numapilot-Exec").unwrap_or(exec);
            let exec = strip_numapilot(exec);
            // jogos da Steam com atalho no menu entram pela lista da Steam
            if exec.contains("steam://rungameid/") {
                continue;
            }
            let Some((key, detail)) = key_for_exec(&exec) else {
                continue;
            };
            let source = if ds.contains("flatpak") || key.starts_with("/app/bin/") {
                "Flatpak"
            } else if ds.contains("snapd") || exec.contains("/snap/") {
                "Snap"
            } else if detail.to_lowercase().ends_with(".appimage") {
                "AppImage"
            } else {
                "Menu"
            };
            out.push(App {
                name: local_name(&t).unwrap_or(&f).to_string(),
                source,
                key,
                detail,
                desktop: Some(p),
            });
        }
    }
}

/// "numapilot run [opções] -- programa args" → "programa args"
fn strip_numapilot(exec: &str) -> String {
    let first = exec.split_whitespace().next().unwrap_or("");
    if basename(first) == "numapilot"
        && let Some((_, rest)) = exec.split_once(" -- ")
    {
        return rest.to_string();
    }
    exec.to_string()
}

fn appimages(out: &mut Vec<App>) {
    let h = home();
    for d in [
        "AppImages",
        "Applications",
        "Aplicativos",
        ".local/bin",
        "Downloads",
    ] {
        for e in std::fs::read_dir(h.join(d)).into_iter().flatten().flatten() {
            let p = e.path();
            let n = e.file_name().to_string_lossy().to_string();
            if !n.to_lowercase().ends_with(".appimage") || !p.is_file() {
                continue;
            }
            let key = p.to_string_lossy().to_string();
            if out.iter().any(|a| a.detail == key) {
                continue;
            }
            out.push(App {
                name: n[..n.len() - ".appimage".len()].to_string(),
                source: "AppImage",
                key: key.clone(),
                detail: key,
                desktop: None,
            });
        }
    }
}

/// Todos os programas instalados que dá para achar, por nome.
pub fn list() -> Vec<App> {
    let mut v = vec![];
    desktop_apps(&mut v);
    for (id, name) in crate::steam::installed_games() {
        v.push(App {
            name,
            source: "Steam",
            key: format!("steam:{id}"),
            detail: crate::tf!("Steam, jogo {id}", "Steam, game {id}"),
            desktop: None,
        });
    }
    appimages(&mut v);
    v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    v
}

/// Filtra por nome, executável ou origem (sem diferenciar maiúsculas).
pub fn matches(a: &App, query: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty()
        || a.name.to_lowercase().contains(&q)
        || a.detail.to_lowercase().contains(&q)
        || a.source.to_lowercase().contains(&q)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_target() {
        let ff = "#!/bin/sh\nexec /usr/lib/firefox/firefox \"$@\"\n";
        assert_eq!(
            script_exec_target(ff, &|p| p == "/usr/lib/firefox/firefox").as_deref(),
            Some("/usr/lib/firefox/firefox")
        );
        // com variável ou programa do PATH: não adivinha
        assert_eq!(
            script_exec_target("exec \"$HERE/bin/app\" \"$@\"\n", &|_| true),
            None
        );
        assert_eq!(
            script_exec_target("exec electron37 /usr/lib/x/app.asar\n", &|_| true),
            None
        );
        assert_eq!(
            script_exec_target("exec -a foo /opt/x/x\n", &|p| p == "/opt/x/x").as_deref(),
            Some("/opt/x/x")
        );
        assert_eq!(script_exec_target("echo oi\n", &|_| true), None);
    }

    #[test]
    fn desktop_e_flatpak() {
        let t = "[Desktop Entry]\nName=Files\nName[pt_BR]=Arquivos\nExec=dolphin %u\n[Desktop Action n]\nName=Nova\nExec=x\n";
        assert_eq!(dget(t, "Exec"), Some("dolphin %u"));
        assert_eq!(dget(t, "Name"), Some("Files"));
        assert_eq!(
            parse_flatpak_command(
                "[Application]\nname=com.obsproject.Studio\ncommand=obs\n\n[Context]\n"
            )
            .as_deref(),
            Some("obs")
        );
        assert_eq!(
            key_for_exec(
                "/usr/bin/flatpak run --branch=stable --command=obs com.obsproject.Studio @@u %u @@"
            ),
            Some((
                "/app/bin/obs".into(),
                "Flatpak com.obsproject.Studio".into()
            ))
        );
        assert_eq!(
            strip_numapilot("/usr/local/bin/numapilot run --gpu amd-0 -- steam -silent"),
            "steam -silent"
        );
        assert_eq!(strip_numapilot("steam -silent"), "steam -silent");
    }

    #[test]
    fn busca() {
        let a = App {
            name: "OBS Studio".into(),
            source: "Flatpak",
            key: "/app/bin/obs".into(),
            detail: "Flatpak com.obsproject.Studio".into(),
            desktop: None,
        };
        assert!(
            matches(&a, "obs")
                && matches(&a, "FLATPAK")
                && matches(&a, "")
                && !matches(&a, "steam")
        );
    }
}
