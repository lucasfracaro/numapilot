//! File log: what numapilot did, what failed and when.
//!
//! - service (root): `/var/log/numapilot/daemon.log` (and also in the journal)
//! - graphical program and command line: `~/.local/state/numapilot/{gui,cli}.log`
//!
//! Each file moves to `.1` on reaching 2 MB (the current and the previous one are kept).
//! `numapilot log` shows the end of all of them.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_BYTES: u64 = 2 * 1024 * 1024;

static LOG: Mutex<Option<(String, PathBuf)>> = Mutex::new(None);

/// This user's log folder (root: /var/log/numapilot).
pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("NUMAPILOT_LOG_DIR") {
        return PathBuf::from(d);
    }
    if unsafe { libc::geteuid() } == 0 {
        return PathBuf::from("/var/log/numapilot");
    }
    user_dir()
}

/// The user's log folder (even when running as root, for `numapilot log`).
pub fn user_dir() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into()))
                .join(".local/state")
        })
        .join("numapilot")
}

/// Turns on file logging for this component ("daemon", "gui", "cli"...).
pub fn init(component: &str) {
    let d = dir();
    let _ = std::fs::create_dir_all(&d);
    let path = d.join(format!("{component}.log"));
    if let Ok(mut g) = LOG.lock() {
        *g = Some((component.to_string(), path));
    }
    // a fatal error also goes to the file
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let loc = info
            .location()
            .map(|l| format!(" ({}:{})", l.file(), l.line()))
            .unwrap_or_default();
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_default();
        write(
            "ERROR",
            &crate::tf!(
                "the program crashed: {msg}{loc}",
                "o programa caiu: {msg}{loc}"
            ),
        );
        prev(info);
    }));
}

/// Local date and time "2026-11-08 06:47:01".
pub fn now() -> String {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}

/// Writes a line (level: INFO, WARN, ERROR). Without `init`, does nothing.
pub fn write(level: &str, msg: &str) {
    let Ok(g) = LOG.lock() else {
        return;
    };
    let Some((comp, path)) = g.as_ref() else {
        return;
    };
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        for (i, line) in msg.lines().enumerate() {
            let _ = writeln!(
                f,
                "{} {:<5} {}{} {}",
                now(),
                level,
                comp,
                if i == 0 { ":" } else { "+" },
                line
            );
        }
    }
}

pub fn info(msg: &str) {
    write("INFO", msg);
}

pub fn warn(msg: &str) {
    write("WARN", msg);
}

pub fn error(msg: &str) {
    write("ERROR", msg);
}

/// Level from the message text: the service's messages do not state a level, but say "could not"…
pub fn level_of(msg: &str) -> &'static str {
    // "(0 error(s))" is not an error; "10 error(s)" is
    let mut m = msg.to_lowercase();
    while let Some(i) = m.find("0 erro").or_else(|| m.find("0 error")) {
        if m[..i].ends_with(|c: char| c.is_ascii_digit()) {
            break;
        }
        m.replace_range(i..i + "0 erro".len(), "");
    }
    if [
        "erro",
        "falhou",
        "não consegui",
        "nao consegui",
        "caiu",
        "panic",
        "error",
        "failed",
        "could not",
        "crashed",
    ]
    .iter()
    .any(|k| m.contains(k))
    {
        "ERROR"
    } else if m.contains("aviso")
        || m.contains("não aplicad")
        || m.contains("bloquead")
        || m.contains("warning")
        || m.contains("not applied")
        || m.contains("blocked")
    {
        "WARN"
    } else {
        "INFO"
    }
}

/// Service message: goes to the journal (stderr) and to the file.
#[macro_export]
macro_rules! nlog {
    ($($arg:tt)*) => {{
        let m = format!($($arg)*);
        eprintln!("[numapilot] {m}");
        $crate::log::write($crate::log::level_of(&m), &m);
    }};
}

/// Last `n` lines of a file (reading only the end).
pub fn tail(path: &std::path::Path, n: usize) -> Vec<String> {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return vec![];
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let start = len.saturating_sub((n as u64 + 1) * 300);
    let _ = f.seek(SeekFrom::Start(start));
    let mut s = String::new();
    let _ = f.read_to_string(&mut s);
    let lines: Vec<String> = s.lines().map(str::to_string).collect();
    let skip = lines.len().saturating_sub(n);
    lines.into_iter().skip(skip).collect()
}

/// Known log files: (name, path).
pub fn files() -> Vec<(String, PathBuf)> {
    let mut v = vec![(
        "serviço".to_string(),
        PathBuf::from("/var/log/numapilot/daemon.log"),
    )];
    let u = user_dir();
    for c in ["gui", "cli", "tray", "web"] {
        v.push((c.to_string(), u.join(format!("{c}.log"))));
    }
    // commands run with sudo write to root's folder
    for c in ["cli", "web"] {
        v.push((
            format!("{c} (root)"),
            PathBuf::from(format!("/var/log/numapilot/{c}.log")),
        ));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(level_of("GPU x: could not limit the power"), "ERROR");
        assert_eq!(level_of("GPU x: não consegui limitar"), "ERROR");
        assert_eq!(level_of("fan curve not applied: x"), "WARN");
        assert_eq!(level_of("machine mode: game"), "INFO");
        assert_eq!(level_of("machine mode: custom (0 error(s))"), "INFO");
        assert_eq!(level_of("machine mode: game with 2 error(s): x"), "ERROR");
        assert_eq!(level_of("applied (10 error(s))"), "ERROR");
    }

    #[test]
    fn writes_and_rotates() {
        let d = std::env::temp_dir().join(format!("np-log-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        let path = d.join("t.log");
        *LOG.lock().unwrap() = Some(("t".into(), path.clone()));
        info("primeira\nsegunda");
        let t = tail(&path, 10);
        assert_eq!(t.len(), 2);
        assert!(t[0].contains("t: primeira") && t[1].contains("t+ segunda"));
        *LOG.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(d);
    }
}
