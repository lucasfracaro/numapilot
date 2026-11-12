// Records in the binary the commit it was built from, so `self-update` can tell whether the
// code in the folder is already newer than what is installed. Without git (tarball, package)
// it stays empty.
use std::process::Command;

fn main() {
    // only this folder's git (not a parent folder's, like the AUR PKGBUILD's)
    let has_git = std::path::Path::new(".git").exists();
    let commit = has_git
        .then(|| {
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
        })
        .flatten()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=NUMAPILOT_COMMIT={commit}");
    for p in [".git/HEAD", ".git/refs/heads", ".git/packed-refs"] {
        if std::path::Path::new(p).exists() {
            println!("cargo:rerun-if-changed={p}");
        }
    }
}
