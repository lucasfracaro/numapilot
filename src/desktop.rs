//! Menu shortcuts (.desktop): creates a copy in ~/.local/share/applications that
//! starts the program through `numapilot run`, so the profile (GPU) applies with no typing.

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

pub fn search_dirs() -> Vec<PathBuf> {
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

/// First program of an Exec= line, skipping `env` and VAR=value.
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

/// Looks for the .desktop that starts this executable.
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

/// Rewrites every Exec= line to go through numapilot.
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
            // D-Bus activation ignores Exec=; turned off so numapilot applies.
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
        // The original is already in the user's folder: keep a copy.
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

/// Line that ties a shortcut made by numapilot to a program (its profile key).
const KEY_LINE: &str = "X-Numapilot-Key=";

/// The shortcut numapilot made for this program (key), if there is one.
pub fn own_shortcut(key: &str) -> Option<PathBuf> {
    let want = format!("{KEY_LINE}{key}");
    let mut v: Vec<PathBuf> = fs::read_dir(user_dir())
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("desktop"))
        .collect();
    v.sort();
    v.into_iter()
        .find(|p| fs::read_to_string(p).is_ok_and(|c| c.lines().any(|l| l.trim() == want)))
}

/// How to start the program again: what is running (`pid`) or, closed, the key when it is a
/// file. An AppImage starts from its file; a script keeps its interpreter's whole command
/// line; anything else is just the executable (the files it had open, like a game's ROM, are
/// left out). None = unknown (open the program once).
pub fn launch_command(key: &str, pid: Option<i32>) -> Option<Vec<String>> {
    if let Some(pid) = pid.filter(|p| crate::procfs::alive(*p)) {
        let exe = crate::procfs::exe(pid);
        if let Some((file, _)) = crate::procfs::appimage_of(pid, exe.as_deref()) {
            return Some(vec![file]);
        }
        if let Some(exe) = exe.filter(|e| !e.contains("/.mount_")) {
            let argv = crate::procfs::cmdline(pid);
            if crate::procfs::is_interpreter(&exe) && argv.len() > 1 {
                return Some(argv);
            }
            return Some(vec![exe]);
        }
    }
    (!key.contains('*') && !key.contains(' ') && Path::new(key).is_file())
        .then(|| vec![key.to_string()])
}

/// One argument of an Exec= line (desktop entry rules: quotes around anything with spaces or
/// special characters, `%` doubled).
fn exec_arg(a: &str) -> String {
    let a = a.replace('%', "%%");
    if !a.is_empty()
        && a.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+,:=@".contains(c))
    {
        return a;
    }
    let mut q = String::from("\"");
    for c in a.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            q.push('\\');
        }
        q.push(c);
    }
    q.push('"');
    q
}

/// Text of a shortcut that opens `argv` through `numapilot run`.
pub fn shortcut_content(
    key: &str,
    name: &str,
    argv: &[String],
    bin: &str,
    icon: &str,
    categories: &str,
) -> String {
    let exec: Vec<String> = argv.iter().map(|a| exec_arg(a)).collect();
    let name = name.replace('\n', " ");
    format!(
        "[Desktop Entry]\nType=Application\nName={name} (numapilot)\nComment=Opens {name} through numapilot, with its saved adjustment\nExec={} run -- {}\nIcon={icon}\nTerminal=false\nCategories={categories}\n{MARK}\n{KEY_LINE}{key}\n",
        exec_arg(bin),
        exec.join(" ")
    )
}

/// Creates (or replaces) the menu shortcut that opens this program through numapilot.
pub fn create_shortcut(key: &str, name: &str, argv: &[String]) -> std::io::Result<PathBuf> {
    if argv.is_empty() {
        return Err(std::io::Error::other("no command"));
    }
    // icon and categories of the program's own shortcut, when it has one
    let (mut icon, mut cats) = (
        "application-x-executable".to_string(),
        "Utility;".to_string(),
    );
    if let Some(c) = find_for_exe(&argv[0]).and_then(|p| fs::read_to_string(p).ok()) {
        for l in c.lines() {
            if let Some(v) = l.strip_prefix("Icon=") {
                icon = v.to_string();
            } else if let Some(v) = l.strip_prefix("Categories=") {
                cats = v.to_string();
            }
        }
    }
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    fs::create_dir_all(user_dir())?;
    let dest = match own_shortcut(key) {
        Some(p) => p,
        None => {
            let base = if slug.is_empty() {
                "program".to_string()
            } else {
                slug
            };
            let mut p = user_dir().join(format!("numapilot-{base}.desktop"));
            let mut n = 2;
            while p.exists() {
                p = user_dir().join(format!("numapilot-{base}-{n}.desktop"));
                n += 1;
            }
            p
        }
    };
    fs::write(
        &dest,
        shortcut_content(key, name, argv, &numapilot_bin(), &icon, &cats),
    )?;
    Ok(dest)
}

/// Deletes the shortcut numapilot made for this program. Ok(false) = there was none.
pub fn remove_shortcut(key: &str) -> std::io::Result<bool> {
    match own_shortcut(key) {
        Some(p) => fs::remove_file(p).map(|_| true),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_text() {
        let c = shortcut_content(
            "/e/lichii.AppImage",
            "Lichii",
            &[
                "/e/My Games/lichii.AppImage".to_string(),
                "--x=50%".to_string(),
            ],
            "/usr/local/bin/numapilot",
            "applications-games",
            "Game;",
        );
        assert!(c.contains(
            "Exec=/usr/local/bin/numapilot run -- \"/e/My Games/lichii.AppImage\" \"--x=50%%\""
        ));
        assert!(c.contains("X-Numapilot-Key=/e/lichii.AppImage"));
        assert!(c.contains(MARK));
        assert_eq!(
            exec_program(exec_line(&c).unwrap()).as_deref(),
            Some("/usr/local/bin/numapilot")
        );
    }

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
