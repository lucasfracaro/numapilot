//! Atalhos do menu (.desktop): cria uma cópia em ~/.local/share/applications que
//! abre o programa via `numapilot run`, para o perfil (GPU) valer sem digitar comandos.

use crate::util::basename;
use std::fs;
use std::path::{Path, PathBuf};

const MARK: &str = "X-Numapilot=true";

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn user_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"))
        .join("applications")
}

fn search_dirs() -> Vec<PathBuf> {
    let mut v = vec![user_dir()];
    let sys =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    for d in sys.split(':').filter(|d| !d.is_empty()) {
        v.push(Path::new(d).join("applications"));
    }
    v.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    v.push(home().join(".local/share/flatpak/exports/share/applications"));
    v.dedup();
    v
}

/// Primeiro programa de uma linha Exec=, pulando `env` e VAR=valor.
pub fn exec_program(exec: &str) -> Option<String> {
    let mut it = exec.split_whitespace().map(|t| t.trim_matches('"'));
    loop {
        let t = it.next()?;
        if t == "env" || (t.contains('=') && !t.starts_with('/')) {
            continue;
        }
        return Some(t.to_string());
    }
}

fn exec_line(content: &str) -> Option<&str> {
    let mut in_main = false;
    for l in content.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            in_main = l == "[Desktop Entry]";
        } else if in_main && let Some(e) = l.strip_prefix("Exec=") {
            return Some(e);
        }
    }
    None
}

/// Procura o .desktop que abre este executável.
pub fn find_for_exe(exe: &str) -> Option<PathBuf> {
    let exe_base = basename(exe);
    let exe_canon = fs::canonicalize(exe).ok();
    for dir in search_dirs() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(c) = fs::read_to_string(&p) else {
                continue;
            };
            let Some(prog) = exec_line(&c).and_then(exec_program) else {
                continue;
            };
            let prog = prog
                .trim_start_matches(&format!("{} run -- ", numapilot_bin()))
                .to_string();
            if basename(&prog) == exe_base {
                return Some(p);
            }
            if let (Some(a), Some(b)) = (&exe_canon, which(&prog))
                && *a == b
            {
                return Some(p);
            }
        }
    }
    None
}

fn which(prog: &str) -> Option<PathBuf> {
    if prog.contains('/') {
        return fs::canonicalize(prog).ok();
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(prog))
        .find(|p| p.is_file())
        .and_then(|p| fs::canonicalize(p).ok())
}

pub fn numapilot_bin() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("numapilot")))
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "numapilot".into())
}

pub fn override_path(original: &Path) -> PathBuf {
    user_dir().join(original.file_name().unwrap_or_default())
}

pub fn is_overridden(original: &Path) -> bool {
    fs::read_to_string(override_path(original)).is_ok_and(|c| c.contains(MARK))
}

/// Reescreve todas as linhas Exec= para passar pelo numapilot.
pub fn wrap_content(content: &str, bin: &str) -> String {
    let mut out = String::new();
    let mut marked = content.contains(MARK);
    for l in content.lines() {
        let t = l.trim_start();
        if let Some(e) = t.strip_prefix("Exec=") {
            if e.contains(" run -- ") && e.contains("numapilot") {
                out.push_str(l);
            } else {
                out.push_str(&format!("Exec={bin} run -- {e}"));
            }
        } else if t.starts_with("DBusActivatable=") {
            // Ativação por D-Bus ignora o Exec=; desliga para o numapilot valer.
            out.push_str("DBusActivatable=false");
        } else {
            out.push_str(l);
        }
        out.push('\n');
        if t == "[Desktop Entry]" && !marked {
            out.push_str(MARK);
            out.push('\n');
            marked = true;
        }
    }
    out
}

pub fn install_override(original: &Path) -> std::io::Result<PathBuf> {
    let c = fs::read_to_string(original)?;
    let dest = override_path(original);
    fs::create_dir_all(user_dir())?;
    if dest == original && !c.contains(MARK) {
        // O original já está na pasta do usuário: guarda uma cópia.
        fs::write(dest.with_extension("desktop.numapilot-bak"), &c)?;
    }
    fs::write(&dest, wrap_content(&c, &numapilot_bin()))?;
    Ok(dest)
}

pub fn remove_override(original: &Path) -> std::io::Result<()> {
    let dest = override_path(original);
    let bak = dest.with_extension("desktop.numapilot-bak");
    if bak.exists() {
        fs::rename(bak, &dest)
    } else if is_overridden(original) {
        fs::remove_file(dest)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_parsing_and_wrap() {
        assert_eq!(
            exec_program("env FOO=1 /usr/bin/blender %f").as_deref(),
            Some("/usr/bin/blender")
        );
        assert_eq!(
            exec_program("\"/opt/My App/app\" --x").as_deref(),
            Some("/opt/My")
        );
        let c = "[Desktop Entry]\nName=Blender\nExec=blender %f\nDBusActivatable=true\n\n[Desktop Action new]\nExec=blender --new\n";
        let w = wrap_content(c, "/usr/local/bin/numapilot");
        assert!(w.contains("Exec=/usr/local/bin/numapilot run -- blender %f"));
        assert!(w.contains("Exec=/usr/local/bin/numapilot run -- blender --new"));
        assert!(w.contains("DBusActivatable=false"));
        assert!(w.contains(MARK));
        // Idempotente.
        assert_eq!(
            wrap_content(&w, "/usr/local/bin/numapilot")
                .matches("run --")
                .count(),
            2
        );
    }
}
