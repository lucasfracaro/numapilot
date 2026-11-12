//! Atualizar o próprio numapilot: pelo pacote (AUR/pacman) ou pela pasta do código de onde o
//! `install.sh` foi rodado (git pull + install.sh --yes).

use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum How {
    /// Instalado por pacote: nome do pacote.
    Package(String),
    /// Instalado pelo install.sh a partir desta pasta (repositório git).
    Source(PathBuf),
    Unknown,
}

/// Como foi instalado (consultado uma vez por execução).
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

/// Há commits novos no repositório? (None = não deu para saber)
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

/// Commit de onde este binário foi compilado (vazio se não foi de um repositório git).
pub const BUILT_COMMIT: &str = env!("NUMAPILOT_COMMIT");

/// Quantos commits o código da pasta tem que este binário não tem (ex.: commitado aqui e ainda
/// não reinstalado). None = não deu para saber (binário sem commit gravado, ou commit sumiu).
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

/// Comando que atualiza (para rodar num terminal: pede senha e mostra o progresso).
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
        How::Unknown => Err(crate::i18n::tr("não sei de onde o numapilot foi instalado: rode o install.sh de novo pela pasta do código").into()),
    }
}
