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
    // a folder left on a loose commit (detached HEAD, e.g. after testing a pull request) has no
    // upstream: compare with origin/main, which the update goes back to
    ["HEAD..@{u}", "HEAD..origin/main"]
        .iter()
        .find_map(|range| {
            let o = Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["rev-list", "--count", range])
                .output()
                .ok()
                .filter(|o| o.status.success())?;
            String::from_utf8_lossy(&o.stdout).trim().parse().ok()
        })
}

/// Shell for the update from a source folder. A folder on a loose commit (detached HEAD) goes
/// back to main first when main already has that commit (git refuses if that would overwrite
/// local changes); otherwise it stops and says what to do.
pub fn source_script(dir: &std::path::Path) -> String {
    format!(
        "cd '{}' && if ! git symbolic-ref -q HEAD >/dev/null; then git fetch --quiet origin main || exit 1; if git merge-base --is-ancestor HEAD origin/main; then echo 'numapilot: the folder is on a loose commit (detached HEAD) that main already has: going back to main' && git checkout main || exit 1; else echo 'numapilot: the folder is on a loose commit (detached HEAD) that main does not have: keep it on a branch (git switch -c NAME) or go back with git switch main, then update again'; exit 1; fi; fi && git pull --ff-only && ./install.sh --yes",
        dir.display().to_string().replace('\'', "")
    )
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
        How::Source(d) => Ok(vec!["sh".into(), "-c".into(), source_script(&d)]),
        How::Unknown => Err(crate::i18n::tr("I do not know where numapilot was installed from: run install.sh again from the code folder").into()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn detached_head_goes_back_to_main() {
        let base = std::env::temp_dir().join("numapilot-selfupdate-test");
        let _ = std::fs::remove_dir_all(&base);
        let git = |dir: &std::path::Path, a: &[&str]| {
            let o = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args([
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "-c",
                    "init.defaultBranch=main",
                ])
                .args(a)
                .output()
                .unwrap();
            assert!(
                o.status.success(),
                "{a:?}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        let up = base.join("up");
        std::fs::create_dir_all(&up).unwrap();
        git(&up, &["init", "-q"]);
        std::fs::write(up.join("install.sh"), "#!/bin/sh\necho installed\n").unwrap();
        git(&up, &["add", "install.sh"]);
        git(&up, &["update-index", "--chmod=+x", "install.sh"]);
        git(&up, &["commit", "-q", "-m", "a"]);
        git(&base, &["clone", "-q", up.to_str().unwrap(), "clone"]);
        let c = base.join("clone");
        git(&up, &["commit", "-q", "--allow-empty", "-m", "b"]);
        git(&c, &["checkout", "-q", "--detach", "HEAD"]);
        assert_eq!(
            super::source_behind(&c),
            Some(1),
            "compares with origin/main"
        );
        let o = std::process::Command::new("sh")
            .args(["-c", &super::source_script(&c)])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(
            o.status.success(),
            "{out} {}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(out.contains("going back to main") && out.contains("installed"));
        assert_eq!(git(&c, &["rev-parse", "--abbrev-ref", "HEAD"]), "main");

        // a commit main does not have: stops
        git(&c, &["checkout", "-q", "--detach", "HEAD"]);
        git(&c, &["commit", "-q", "--allow-empty", "-m", "loose"]);
        let o = std::process::Command::new("sh")
            .args(["-c", &super::source_script(&c)])
            .output()
            .unwrap();
        assert!(!o.status.success());
        assert!(String::from_utf8_lossy(&o.stdout).contains("main does not have"));
        let _ = std::fs::remove_dir_all(&base);
    }
}
