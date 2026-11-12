//! Updating numapilot itself: through the package (AUR/pacman) or through the code folder
//! `install.sh` was run from (git pull + install.sh --yes).

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum How {
    /// Installed from a package: package name.
    Package(String),
    /// Installed by install.sh from this folder (git repository).
    Source(PathBuf),
    Unknown,
}

/// How it was installed (looked up once per run).
pub fn how() -> How {
    static H: std::sync::OnceLock<How> = std::sync::OnceLock::new();
    H.get_or_init(detect).clone()
}

fn detect() -> How {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("/usr/local/bin/numapilot"));
    if let Ok(o) = Command::new("pacman").args(["-Qqo"]).arg(&exe).output()
        && o.status.success()
    {
        let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if !p.is_empty() {
            return How::Package(p);
        }
    }
    let src = crate::learned::state_dir().join("source");
    if let Some(d) = crate::util::read_trim(&src).map(PathBuf::from)
        && d.join(".git").exists()
        && d.join("install.sh").exists()
    {
        return How::Source(d);
    }
    How::Unknown
}

/// Are there new commits in the repository? (None = could not tell)
pub fn source_behind(dir: &std::path::Path) -> Option<u32> {
    let ok = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["fetch", "--quiet"])
        .status()
        .ok()?
        .success();
    if !ok {
        return None;
    }
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-list", "--count", "HEAD..@{u}"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&o.stdout).trim().parse().ok()
}

/// Commit this binary was built from (empty if not from a git repository).
pub const BUILT_COMMIT: &str = env!("NUMAPILOT_COMMIT");

/// How many commits the folder's code has that this binary does not (e.g. committed here and
/// not reinstalled yet). None = could not tell (binary without a recorded commit, or commit gone).
pub fn source_ahead_of_installed(dir: &std::path::Path) -> Option<u32> {
    if BUILT_COMMIT.is_empty() {
        return None;
    }
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-list", "--count", &format!("{BUILT_COMMIT}..HEAD")])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    String::from_utf8_lossy(&o.stdout).trim().parse().ok()
}

/// Command that updates (to run in a terminal: asks for the password and shows progress).
pub fn command() -> Result<Vec<String>, String> {
    match how() {
        How::Package(p) => {
            let helper = if crate::containers::which("paru") { "paru" } else if crate::containers::which("yay") { "yay" } else { "" };
            if helper.is_empty() || !p.ends_with("-git") {
                Ok(vec!["sudo".into(), "pacman".into(), "-Syu".into(), p])
            } else {
                Ok(vec![helper.into(), "-S".into(), "--rebuild".into(), p])
            }
        }
        How::Source(d) => Ok(vec![
            "sh".into(),
            "-c".into(),
            format!(
                "cd '{}' && git pull --ff-only && ./install.sh --yes",
                d.display().to_string().replace('\'', "")
            ),
        ]),
        How::Unknown => Err(crate::i18n::tr("I do not know where numapilot was installed from: run install.sh again from the code folder").into()),
    }
}
