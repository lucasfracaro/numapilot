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

/// The file was changed where it is (not a copy that hides the original): undoing puts the
/// original command back in it.
const IN_PLACE: &str = "X-Numapilot-InPlace=true";
/// Adjusted on its own when a choice was saved (not by the "Use in the menu" button): goes back
/// to the original when the choice no longer needs numapilot.
const AUTO: &str = "X-Numapilot-Auto=true";
/// Original DBusActivatable= value, turned off by the adjustment.
const DBUS: &str = "X-Numapilot-DBus=";

/// Does an Exec= go through `numapilot run`?
fn runs_numapilot(exec: &str) -> bool {
    exec_program(exec).is_some_and(|p| basename(&p) == "numapilot") && exec.contains(" run ")
}

/// "numapilot run [options] -- program args" → "program args"; anything else as it is.
pub fn strip_run(exec: &str) -> &str {
    if runs_numapilot(exec)
        && let Some((_, rest)) = exec.split_once(" -- ")
    {
        return rest;
    }
    exec
}

/// Rewrites every Exec= line to go through numapilot.
pub fn wrap_content(content: &str, bin: &str) -> String {
    wrap_with(content, bin, &[])
}

/// Like `wrap_content`, with extra lines after the mark (how to undo it).
fn wrap_with(content: &str, bin: &str, extra: &[&str]) -> String {
    let mut out = String::new();
    let was_marked = content.contains(MARK);
    let mut marked = was_marked;
    for l in content.lines() {
        let t = l.trim_start();
        if let Some(e) = t.strip_prefix("Exec=") {
            if runs_numapilot(e) {
                out.push_str(l);
            } else {
                out.push_str(&format!("Exec={} run -- {e}", exec_arg(bin)));
            }
        } else if let Some(v) = t.strip_prefix("DBusActivatable=") {
            // D-Bus activation ignores Exec=; turned off so numapilot applies.
            out.push_str("DBusActivatable=false");
            if !was_marked && v.trim() != "false" {
                out.push('\n');
                out.push_str(&format!("{DBUS}{}", v.trim()));
            }
        } else {
            out.push_str(l);
        }
        out.push('\n');
        if t == "[Desktop Entry]" && !marked {
            out.push_str(MARK);
            out.push('\n');
            for x in extra {
                out.push_str(x);
                out.push('\n');
            }
            marked = true;
        }
    }
    out
}

/// Undoes `wrap_with`: the Exec= lines lose the `numapilot run --` in front and the marks go.
pub fn unwrap_content(content: &str) -> String {
    let dbus = content
        .lines()
        .find_map(|l| l.trim().strip_prefix(DBUS).map(str::to_string));
    let mut out = String::new();
    for l in content.lines() {
        let t = l.trim_start();
        if t == MARK || t == IN_PLACE || t == AUTO || t.starts_with(DBUS) {
            continue;
        }
        if let Some(e) = t.strip_prefix("Exec=") {
            out.push_str(&format!("Exec={}", strip_run(e)));
        } else if t.starts_with("DBusActivatable=")
            && let Some(v) = &dbus
        {
            out.push_str(&format!("DBusActivatable={v}"));
        } else {
            out.push_str(l);
        }
        out.push('\n');
    }
    out
}

/// Writes keeping the file's permissions (a desktop icon must stay executable to open).
fn write_keeping(p: &Path, text: &str) -> std::io::Result<()> {
    let mode = fs::metadata(p).ok().map(|m| m.permissions());
    fs::write(p, text)?;
    if let Some(m) = mode {
        let _ = fs::set_permissions(p, m);
    }
    Ok(())
}

/// Makes `original` open through numapilot: a copy in the user's menu folder that hides it, or,
/// when it already is in a folder of the user (menu, desktop), the file itself (undoable: the
/// original command stays in it). `auto` = done on its own when a choice was saved.
fn wrap_file(original: &Path, dest: &Path, auto: bool) -> std::io::Result<PathBuf> {
    let c = fs::read_to_string(original)?;
    let in_place = dest == original;
    let mut extra = vec![];
    if in_place {
        extra.push(IN_PLACE);
    }
    if auto {
        extra.push(AUTO);
    }
    let w = wrap_with(&c, &numapilot_bin(), &extra);
    if let Some(d) = dest.parent() {
        fs::create_dir_all(d)?;
    }
    if in_place {
        write_keeping(dest, &w)?;
    } else {
        fs::write(dest, w)?;
    }
    Ok(dest.to_path_buf())
}

pub fn install_override(original: &Path) -> std::io::Result<PathBuf> {
    let dest = override_path(original);
    let r = wrap_file(original, &dest, false);
    if r.is_ok() {
        remember_off(&key_of_file(original).unwrap_or_default(), false);
    }
    r
}

/// Puts a shortcut adjusted by numapilot back as it was (`path` = the adjusted file).
pub fn restore_file(path: &Path) -> std::io::Result<()> {
    let bak = path.with_extension("desktop.numapilot-bak");
    if bak.exists() {
        return fs::rename(bak, path);
    }
    let Ok(c) = fs::read_to_string(path) else {
        return Ok(());
    };
    if !c.contains(MARK) || c.contains(KEY_LINE) {
        // not adjusted, or a shortcut numapilot made itself (has its own button)
        Ok(())
    } else if c.contains(IN_PLACE) {
        write_keeping(path, &unwrap_content(&c))
    } else {
        fs::remove_file(path)
    }
}

pub fn remove_override(original: &Path) -> std::io::Result<()> {
    let dest = override_path(original);
    // put back by hand: saving the choice does not adjust it again
    remember_off(&key_of_file(&dest).unwrap_or_default(), true);
    restore_file(&dest)
}

// ---------- shortcuts of a program, adjusted when its choice is saved ----------

/// Where a shortcut is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Menu,
    Desktop,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Launcher {
    /// The file that counts now (the user's copy when it hides the system's).
    pub path: PathBuf,
    pub place: Place,
    pub name: String,
    /// Already opens through numapilot.
    pub wrapped: bool,
    /// Adjusted on its own when a choice was saved.
    pub auto: bool,
}

/// The desktop folder (XDG_DESKTOP_DIR in ~/.config/user-dirs.dirs, e.g. "Área de trabalho").
pub fn desktop_folder() -> Option<PathBuf> {
    let h = home();
    let cfg = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| h.join(".config"));
    let from_cfg = fs::read_to_string(cfg.join("user-dirs.dirs"))
        .ok()
        .and_then(|t| {
            t.lines().find_map(|l| {
                let v = l.trim().strip_prefix("XDG_DESKTOP_DIR=")?;
                let v = v.trim().trim_matches('"');
                Some(match v.strip_prefix("$HOME") {
                    Some(r) => h.join(r.trim_start_matches('/')),
                    None => PathBuf::from(v),
                })
            })
        });
    from_cfg
        .into_iter()
        .chain([h.join("Desktop")])
        .find(|p| p.is_dir() && *p != h)
}

/// Folders searched for a program's shortcuts, in order (the first file of each name wins in
/// the menu, as the menu itself does).
pub fn launcher_places() -> Vec<(PathBuf, Place)> {
    let mut v: Vec<(PathBuf, Place)> = search_dirs()
        .into_iter()
        .map(|d| (d, Place::Menu))
        .collect();
    if let Some(d) = desktop_folder() {
        v.push((d, Place::Desktop));
    }
    v
}

fn entry_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut main = false;
    for l in text.lines() {
        let l = l.trim();
        if l.starts_with('[') {
            main = l == "[Desktop Entry]";
        } else if main
            && let Some((k, v)) = l.split_once('=')
            && k.trim() == key
        {
            return Some(v.trim());
        }
    }
    None
}

/// Program key of a shortcut file (the same the service gives the program when it runs).
pub fn key_of_file(p: &Path) -> Option<String> {
    let t = fs::read_to_string(p).ok()?;
    crate::apps::key_for_exec(strip_run(entry_value(&t, "Exec")?)).map(|(k, _)| k)
}

/// The shortcuts that start the program `key` (`exe` = its executable when it is open, for
/// shortcuts whose key cannot be worked out). `key_of` gives the key of an Exec= line.
pub fn find_launchers(
    key: &str,
    exe: Option<&str>,
    places: &[(PathBuf, Place)],
    key_of: &dyn Fn(&str) -> Option<String>,
) -> Vec<Launcher> {
    let want = crate::procfs::stable_key(key);
    let exe_base = exe
        .filter(|e| !e.is_empty() && !crate::procfs::is_interpreter(e))
        .map(basename);
    let mut seen = std::collections::BTreeSet::new();
    let mut v = vec![];
    for (dir, place) in places {
        let Ok(rd) = fs::read_dir(dir) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for p in files {
            if p.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let name = p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            // a menu file of the same name earlier in the list hides this one
            if *place == Place::Menu && !seen.insert(name) {
                continue;
            }
            let Ok(t) = fs::read_to_string(&p) else {
                continue;
            };
            if entry_value(&t, "Type").is_some_and(|x| x != "Application")
                || entry_value(&t, "Hidden") == Some("true")
                || t.contains(KEY_LINE)
            {
                continue;
            }
            let Some(exec) = entry_value(&t, "Exec") else {
                continue;
            };
            let orig = strip_run(exec);
            // Steam games start through the Steam client: the launch option does that job
            if orig.contains("steam://") {
                continue;
            }
            let by_key =
                key_of(orig).is_some_and(|k| k == key || crate::procfs::stable_key(&k) == want);
            let by_exe = exe_base
                .is_some_and(|b| exec_program(orig).is_some_and(|prog| basename(&prog) == b));
            if !(by_key || by_exe) {
                continue;
            }
            v.push(Launcher {
                name: entry_value(&t, "Name").unwrap_or_default().to_string(),
                wrapped: runs_numapilot(exec) || t.contains(MARK),
                auto: t.contains(AUTO),
                path: p,
                place: *place,
            });
        }
    }
    v
}

/// Where the adjusted copy of a shortcut goes: the file itself when it is in a folder of the
/// user, otherwise a copy of the same name in the user's menu folder (hides the system's).
fn dest_for(l: &Launcher) -> PathBuf {
    match l.place {
        Place::Desktop => l.path.clone(),
        Place::Menu => override_path(&l.path),
    }
}

/// Programs whose shortcuts the user put back by hand: saving does not adjust them again.
fn off_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"))
        .join("numapilot/shortcuts-off.txt")
}

fn off_keys() -> std::collections::BTreeSet<String> {
    fs::read_to_string(off_path())
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn remember_off(key: &str, off: bool) {
    if key.is_empty() {
        return;
    }
    let mut s = off_keys();
    let changed = if off {
        s.insert(key.to_string())
    } else {
        s.remove(key)
    };
    if changed {
        let p = off_path();
        if let Some(d) = p.parent() {
            let _ = fs::create_dir_all(d);
        }
        let mut t: String = s.into_iter().collect::<Vec<_>>().join("\n");
        t.push('\n');
        let _ = fs::write(p, t);
    }
}

/// Lets saving this program's choice adjust its shortcuts again.
pub fn forget_off(key: &str) {
    remember_off(key, false);
}

/// What saving a choice did to the program's shortcuts.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct LauncherSync {
    /// Now open through numapilot: (adjusted file, name).
    pub wrapped: Vec<(PathBuf, String)>,
    /// Put back as they were (the choice no longer needs numapilot).
    pub restored: Vec<PathBuf>,
    pub errors: Vec<String>,
}

impl LauncherSync {
    pub fn changed(&self) -> bool {
        !self.wrapped.is_empty() || !self.restored.is_empty()
    }
}

/// After a choice is saved (or deleted, `profile` None): the program's own shortcuts (menu and
/// desktop) start it through numapilot when the choice needs it (graphics card, what it sees,
/// upscaler), and go back to the original when it no longer does. Steam games are left alone
/// (their launch option does this) and so are programs whose shortcuts the user put back.
pub fn sync_launchers(
    key: &str,
    profile: Option<&crate::profiles::Profile>,
    exe: Option<&str>,
) -> LauncherSync {
    sync_in(key, profile, exe, &launcher_places(), &off_keys(), &|e| {
        crate::apps::key_for_exec(e).map(|(k, _)| k)
    })
}

fn sync_in(
    key: &str,
    profile: Option<&crate::profiles::Profile>,
    exe: Option<&str>,
    places: &[(PathBuf, Place)],
    off: &std::collections::BTreeSet<String>,
    key_of: &dyn Fn(&str) -> Option<String>,
) -> LauncherSync {
    let mut r = LauncherSync::default();
    if key.is_empty() || key.starts_with("steam:") {
        return r;
    }
    let needs = profile.is_some_and(|p| p.needs_launcher());
    let stable = crate::procfs::stable_key(key);
    if needs
        && off
            .iter()
            .any(|k| k == key || crate::procfs::stable_key(k) == stable)
    {
        return r;
    }
    for l in find_launchers(key, exe, places, key_of) {
        if needs && !l.wrapped {
            match wrap_file(&l.path, &dest_for(&l), true) {
                Ok(p) => r.wrapped.push((p, l.name)),
                Err(e) => r.errors.push(format!("{}: {e}", l.path.display())),
            }
        } else if !needs && l.wrapped && l.auto {
            match restore_file(&l.path) {
                Ok(()) => r.restored.push(l.path),
                Err(e) => r.errors.push(format!("{}: {e}", l.path.display())),
            }
        }
    }
    r
}

/// Undoes what saving did (`files` = `LauncherSync::wrapped`) and remembers not to adjust this
/// program's shortcuts again.
pub fn undo_launchers(key: &str, files: &[PathBuf]) -> std::io::Result<()> {
    remember_off(key, true);
    for f in files {
        restore_file(f)?;
    }
    Ok(())
}

/// Tells the menu (KDE) to read the shortcuts again.
pub fn refresh_menu() {
    let _ = std::process::Command::new("kbuildsycoca6")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

/// Line that ties a shortcut made by numapilot to a program (its profile key).
const KEY_LINE: &str = "X-Numapilot-Key=";

/// Every shortcut numapilot made, by program key (one read of the folder).
pub fn own_shortcuts() -> std::collections::HashMap<String, PathBuf> {
    let mut m = std::collections::HashMap::new();
    for e in fs::read_dir(user_dir()).into_iter().flatten().flatten() {
        let p = e.path();
        if p.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        if let Ok(c) = fs::read_to_string(&p)
            && let Some(k) = c.lines().find_map(|l| l.trim().strip_prefix(KEY_LINE))
        {
            m.entry(k.to_string()).or_insert(p);
        }
    }
    m
}

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

    #[test]
    fn unwrap_gives_the_original_back() {
        let c = "[Desktop Entry]\nName=Longshot\nExec=\"/opt/Long Shot/launcher.sh\" %U\nDBusActivatable=true\n\n[Desktop Action safe]\nExec=/opt/longshot --safe\n";
        let w = wrap_with(c, "/usr/local/bin/numapilot", &[IN_PLACE, AUTO]);
        assert!(
            w.contains("Exec=/usr/local/bin/numapilot run -- \"/opt/Long Shot/launcher.sh\" %U")
        );
        assert!(w.contains("Exec=/usr/local/bin/numapilot run -- /opt/longshot --safe"));
        assert!(w.contains(IN_PLACE) && w.contains(AUTO) && w.contains("X-Numapilot-DBus=true"));
        assert_eq!(unwrap_content(&w), c);
        assert_eq!(strip_run("numapilot run --gpu x -- game -f"), "game -f");
        assert_eq!(strip_run("game run -- x"), "game run -- x");
    }

    #[test]
    fn saving_a_choice_adjusts_and_restores_shortcuts() {
        use crate::profiles::Profile;
        let root = std::env::temp_dir().join(format!("numapilot-launchers-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (user, sys, desk) = (root.join("user"), root.join("sys"), root.join("desk"));
        for d in [&user, &sys, &desk] {
            fs::create_dir_all(d).unwrap();
        }
        let entry =
            |exec: &str| format!("[Desktop Entry]\nType=Application\nName=Longshot\nExec={exec}\n");
        fs::write(
            sys.join("longshot.desktop"),
            entry("/opt/longshot/launch.sh"),
        )
        .unwrap();
        fs::write(
            desk.join("Longshot.desktop"),
            entry("/opt/longshot/launch.sh %U"),
        )
        .unwrap();
        // hidden by the user's file of the same name
        fs::write(sys.join("other.desktop"), entry("/opt/longshot/launch.sh")).unwrap();
        fs::write(user.join("other.desktop"), entry("/usr/bin/other")).unwrap();
        // Steam shortcut and a different program: left alone
        fs::write(
            desk.join("steam.desktop"),
            entry("steam steam://rungameid/1"),
        )
        .unwrap();
        fs::write(desk.join("x.desktop"), entry("/usr/bin/x")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                desk.join("Longshot.desktop"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        // user_dir() is where menu copies go: point the menu folder of the user at `user`
        let places = vec![
            (user.clone(), Place::Menu),
            (sys.clone(), Place::Menu),
            (desk.clone(), Place::Desktop),
        ];
        let key_of = |e: &str| {
            exec_program(e)
                .filter(|p| p.starts_with("/opt/longshot/"))
                .map(|_| "/opt/longshot/game".to_string())
        };
        let off = std::collections::BTreeSet::new();
        let gpu = Profile {
            gpu: "gfx906-1".into(),
            ..Default::default()
        };
        assert!(gpu.needs_launcher());
        assert!(
            !Profile {
                priority: "low".into(),
                ..Default::default()
            }
            .needs_launcher()
        );

        let ls = find_launchers("/opt/longshot/game", None, &places, &key_of);
        assert_eq!(ls.len(), 2, "{ls:?}");
        // the desktop icon is changed where it is, keeping its permissions
        let l = ls
            .iter()
            .find(|l| l.place == Place::Desktop)
            .unwrap()
            .clone();
        let p = wrap_file(&l.path, &dest_for(&l), true).unwrap();
        assert_eq!(p, desk.join("Longshot.desktop"));
        let t = fs::read_to_string(&p).unwrap();
        assert!(t.contains(" run -- /opt/longshot/launch.sh %U") && t.contains(IN_PLACE));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        // already adjusted: saving again changes nothing
        let ls = find_launchers("/opt/longshot/game", None, &places, &key_of);
        assert!(
            ls.iter()
                .find(|l| l.place == Place::Desktop)
                .unwrap()
                .wrapped
        );
        // a choice that no longer needs numapilot puts the auto-adjusted ones back
        let r = sync_in(
            "/opt/longshot/game",
            Some(&Profile::default()),
            None,
            &places,
            &off,
            &key_of,
        );
        assert_eq!(r.restored, vec![desk.join("Longshot.desktop")]);
        assert_eq!(
            fs::read_to_string(desk.join("Longshot.desktop")).unwrap(),
            entry("/opt/longshot/launch.sh %U")
        );
        // the user put them back by hand: not adjusted again
        let off: std::collections::BTreeSet<String> = ["/opt/longshot/game".to_string()].into();
        assert!(
            !sync_in(
                "/opt/longshot/game",
                Some(&gpu),
                None,
                &places,
                &off,
                &key_of
            )
            .changed()
        );
        // Steam games are never touched
        assert!(
            !sync_in(
                "steam:1",
                Some(&gpu),
                None,
                &places,
                &Default::default(),
                &key_of
            )
            .changed()
        );
        // by the executable of the open program, when the key does not say
        let ls = find_launchers("whatever", Some("/usr/bin/x"), &places, &|_| None);
        assert_eq!(ls.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }
}
