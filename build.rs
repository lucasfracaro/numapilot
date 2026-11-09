// Grava no binário o commit de onde ele foi compilado, para o `self-update` saber se o código
// da pasta já é mais novo que o instalado. Sem git (tarball, pacote), fica vazio.
use std::process::Command;

fn main() {
    // só o git desta pasta (não o de uma pasta acima, como a do PKGBUILD do AUR)
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
