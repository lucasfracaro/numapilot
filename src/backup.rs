//! Backup e restauração de todas as configurações (para reinstalar depois de formatar).
//!
//! O arquivo .tar.gz tem:
//!   sistema/...  arquivos do sistema com o caminho absoluto (/etc, /var/lib/numapilot)
//!   usuario/...  arquivos da sua pasta pessoal, relativos a ela
//!   manifesto.json  versão, data, parâmetros do kernel e serviços do usuário ligados

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Prefixo para as pastas do sistema (só nos testes).
fn sysroot() -> PathBuf {
    std::env::var_os("NUMAPILOT_BACKUP_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

fn under_root(abs: &str) -> PathBuf {
    sysroot().join(abs.trim_start_matches('/'))
}

/// Arquivos/pastas do sistema guardados.
fn system_items() -> Vec<String> {
    let mut v: Vec<String> = [
        "/etc/numapilot.toml",
        "/etc/gfx906-rocm.conf",
        "/etc/modprobe.d/numapilot.conf",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.push(crate::learned::state_dir().to_string_lossy().to_string());
    v
}

/// Arquivos/pastas da pasta pessoal (relativos ao HOME).
const USER_ITEMS: &[&str] = &[
    ".config/gfx906-lab",
    ".config/numapilot",
    ".local/state/numapilot/tuner.json",
    ".config/autostart/numapilot-tray.desktop",
    ".config/systemd/user/gfx906-update.service",
    ".config/systemd/user/gfx906-update.timer",
];

/// Serviços do usuário que voltam ligados.
const USER_UNITS: &[&str] = &["gfx906-update.timer", "numapilot-web.service"];

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub version: String,
    pub created: u64,
    pub hostname: String,
    /// Parâmetros do kernel que o numapilot conhece e estavam configurados.
    pub kernel_params: Vec<String>,
    pub user_units: Vec<String>,
    /// Atalhos do menu que abrem pelo numapilot (relativos ao HOME).
    pub desktop_files: Vec<String>,
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)?.flatten() {
            copy_tree(&e.path(), &dst.join(e.file_name()))?;
        }
    } else if src.is_file() {
        if let Some(p) = dst.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(src, dst)?;
    }
    Ok(())
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

/// Parâmetros do kernel que valem guardar (os que a interface do numapilot controla).
pub fn known_params(configured: &str) -> Vec<String> {
    configured
        .split_whitespace()
        .filter(|t| {
            *t == "mitigations=off"
                || crate::cmdline::KNOWN.iter().any(|(p, _, _)| p == t)
                || crate::cmdline::MITIGATIONS
                    .iter()
                    .any(|m| m.params.contains(t))
        })
        .map(String::from)
        .collect()
}

fn user_unit_enabled(u: &str) -> bool {
    Command::new("systemctl")
        .args(["--user", "is-enabled", u])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Gera o backup. `out` = arquivo ou pasta (padrão: a pasta pessoal).
pub fn create(out: Option<&Path>) -> Result<PathBuf, String> {
    let tmp = std::env::temp_dir().join(format!("numapilot-backup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let mut n = 0;
    for item in system_items() {
        let src = under_root(&item);
        if src.exists() {
            copy_tree(
                &src,
                &tmp.join("sistema").join(item.trim_start_matches('/')),
            )
            .map_err(|e| format!("{item}: {e}"))?;
            n += 1;
        }
    }
    let h = home();
    for item in USER_ITEMS {
        let src = h.join(item);
        if src.exists() {
            copy_tree(&src, &tmp.join("usuario").join(item)).map_err(|e| format!("{item}: {e}"))?;
            n += 1;
        }
    }
    // Atalhos do menu que passam pelo numapilot.
    let mut desktop_files = vec![];
    if let Ok(rd) = std::fs::read_dir(h.join(".local/share/applications")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "desktop")
                && std::fs::read_to_string(&p).is_ok_and(|s| s.contains("numapilot run"))
            {
                let rel = format!(
                    ".local/share/applications/{}",
                    e.file_name().to_string_lossy()
                );
                copy_tree(&p, &tmp.join("usuario").join(&rel)).map_err(|e| e.to_string())?;
                desktop_files.push(rel);
            }
        }
    }
    let m = Manifest {
        version: env!("CARGO_PKG_VERSION").into(),
        created: crate::history::now(),
        hostname: std::fs::read_to_string("/etc/hostname")
            .unwrap_or_default()
            .trim()
            .to_string(),
        kernel_params: crate::cmdline::configured()
            .map(|(_, p)| known_params(&p))
            .unwrap_or_default(),
        user_units: USER_UNITS
            .iter()
            .filter(|u| user_unit_enabled(u))
            .map(|s| s.to_string())
            .collect(),
        desktop_files,
    };
    std::fs::write(
        tmp.join("manifesto.json"),
        serde_json::to_string_pretty(&m).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    let date = Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let name = format!(
        "numapilot-backup-{}.tar.gz",
        if date.is_empty() {
            m.created.to_string()
        } else {
            date
        }
    );
    let dest = match out {
        Some(p) if p.is_dir() => p.join(name),
        Some(p) => p.to_path_buf(),
        None => h.join(name),
    };
    let st = Command::new("tar")
        .arg("czf")
        .arg(&dest)
        .arg("-C")
        .arg(&tmp)
        .arg(".")
        .status()
        .map_err(|e| format!("tar: {e}"))?;
    let _ = std::fs::remove_dir_all(&tmp);
    if !st.success() {
        return Err("tar falhou".into());
    }
    if n == 0 && m.kernel_params.is_empty() {
        return Err("não há nenhuma configuração para guardar ainda".into());
    }
    Ok(dest)
}

/// Pasta pessoal, UID e GID de quem chamou o sudo (ou do usuário atual).
fn target_user() -> (PathBuf, Option<(u32, u32)>) {
    if let Ok(u) = std::env::var("SUDO_USER")
        && let Ok(pw) = std::fs::read_to_string("/etc/passwd")
        && let Some(l) = pw.lines().find(|l| l.split(':').next() == Some(u.as_str()))
    {
        let f: Vec<&str> = l.split(':').collect();
        if f.len() >= 6 {
            return (
                PathBuf::from(f[5]),
                Some((f[2].parse().unwrap_or(0), f[3].parse().unwrap_or(0))),
            );
        }
    }
    (home(), None)
}

fn chown_one(p: &Path, uid: u32, gid: u32) {
    let _ = Command::new("chown")
        .arg("-h")
        .arg(format!("{uid}:{gid}"))
        .arg(p)
        .status();
}

/// Restaura. Precisa de root para as pastas do sistema (use sudo).
pub fn restore(file: &Path, apply_boot: bool, log: &mut dyn FnMut(&str)) -> Result<(), String> {
    let tmp = std::env::temp_dir().join(format!("numapilot-restore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let st = Command::new("tar")
        .arg("xzf")
        .arg(file)
        .arg("-C")
        .arg(&tmp)
        .status()
        .map_err(|e| format!("tar: {e}"))?;
    if !st.success() {
        return Err("não consegui abrir o arquivo de backup".into());
    }
    let m: Manifest = std::fs::read_to_string(tmp.join("manifesto.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .ok_or("arquivo sem manifesto: não é um backup do numapilot")?;
    log(&format!("Backup da versão {} ({})", m.version, m.hostname));
    // sistema
    let sys = tmp.join("sistema");
    let mut modprobe = false;
    if sys.is_dir() {
        if !crate::util::is_root() && std::env::var_os("NUMAPILOT_BACKUP_ROOT").is_none() {
            return Err("para restaurar as configurações do sistema, rode com sudo".into());
        }
        let mut stack = vec![sys.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let rel = p.strip_prefix(&sys).unwrap();
                let dst = sysroot().join(rel);
                if let Some(parent) = dst.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                if dst.exists() {
                    let _ = std::fs::copy(&dst, dst.with_extension("antes-da-restauracao"));
                }
                std::fs::copy(&p, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
                modprobe |= rel.ends_with("modprobe.d/numapilot.conf");
                log(&format!("  /{}", rel.display()));
            }
        }
    }
    // usuário
    let (h, owner) = target_user();
    let usr = tmp.join("usuario");
    if usr.is_dir() {
        copy_tree(&usr, &h).map_err(|e| e.to_string())?;
        if let Some((uid, gid)) = owner {
            // Só o que veio do backup (e as pastas criadas para ele), sem mexer no resto da pasta pessoal.
            for e in walk(&usr) {
                let rel = e.strip_prefix(&usr).unwrap();
                chown_one(&h.join(rel), uid, gid);
            }
        }
        log(&format!("  configurações do usuário em {}", h.display()));
    }
    let _ = std::fs::remove_dir_all(&tmp);
    if modprobe && apply_boot {
        log("Opção do OverDrive restaurada: refazendo a imagem de boot.");
        crate::bootcfg::regenerate(log)?;
    }
    if !m.kernel_params.is_empty() {
        if apply_boot {
            log(&format!(
                "Parâmetros do kernel do backup: {}",
                m.kernel_params.join(" ")
            ));
            if let Err(e) = crate::cmdline::apply(&m.kernel_params, &[], log) {
                log(&format!("Aviso: parâmetros do kernel não aplicados: {e}"));
            }
        } else {
            log(&format!(
                "Parâmetros do kernel no backup (não aplicados): {}",
                m.kernel_params.join(" ")
            ));
        }
    }
    if let Ok(u) = std::env::var("SUDO_USER") {
        for unit in &m.user_units {
            let ok = Command::new("systemctl")
                .args(["--user", "-M", &format!("{u}@"), "enable", "--now", unit])
                .status()
                .is_ok_and(|s| s.success());
            log(&format!(
                "  serviço do usuário {unit}: {}",
                if ok {
                    "ligado"
                } else {
                    "ligue com systemctl --user enable --now"
                }
            ));
        }
    }
    let _ = Command::new("systemctl")
        .args(["try-restart", "numapilot.service"])
        .status();
    log("Restauração concluída. Se o backup tinha OverDrive ou parâmetros do kernel, reinicie.");
    Ok(())
}

/// Tudo dentro de `p`, recursivo (pastas e arquivos).
fn walk(p: &Path) -> Vec<PathBuf> {
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let q = e.path();
            if q.is_dir() {
                v.extend(walk(&q));
            }
            v.push(q);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_and_restore_roundtrip() {
        let base = std::env::temp_dir().join("numapilot-backup-test");
        let _ = std::fs::remove_dir_all(&base);
        let (a, b, home) = (base.join("rootA"), base.join("rootB"), base.join("home"));
        let w = |p: PathBuf, s: &str| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, s).unwrap();
        };
        w(a.join("etc/numapilot.toml"), "[daemon]\n");
        w(
            a.join("etc/modprobe.d/numapilot.conf"),
            "options amdgpu ppfeaturemask=0xfff7ffff\n",
        );
        w(
            home.join(".config/gfx906-lab/settings.env"),
            "LLM_PORT=8081\n",
        );
        w(
            home.join(".local/share/applications/blender.desktop"),
            "Exec=numapilot run -- blender\n",
        );
        w(
            home.join(".local/share/applications/outro.desktop"),
            "Exec=outro\n",
        );
        // Variáveis de ambiente: só este teste mexe nelas (o HOME volta no fim).
        let old_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("NUMAPILOT_BACKUP_ROOT", &a);
            std::env::set_var("HOME", &home);
        }
        let f = create(Some(&base)).unwrap();
        assert!(f.exists());
        let _ = std::fs::remove_dir_all(&home);
        unsafe { std::env::set_var("NUMAPILOT_BACKUP_ROOT", &b) };
        let mut log = vec![];
        restore(&f, false, &mut |m| log.push(m.to_string())).unwrap();
        assert_eq!(
            std::fs::read_to_string(b.join("etc/numapilot.toml")).unwrap(),
            "[daemon]\n"
        );
        assert!(b.join("etc/modprobe.d/numapilot.conf").exists());
        assert_eq!(
            std::fs::read_to_string(home.join(".config/gfx906-lab/settings.env")).unwrap(),
            "LLM_PORT=8081\n"
        );
        assert!(
            home.join(".local/share/applications/blender.desktop")
                .exists()
        );
        assert!(
            !home
                .join(".local/share/applications/outro.desktop")
                .exists(),
            "só os atalhos do numapilot"
        );
        unsafe {
            std::env::remove_var("NUMAPILOT_BACKUP_ROOT");
            if let Some(h) = old_home {
                std::env::set_var("HOME", h);
            }
        }
    }

    #[test]
    fn known_params_filter() {
        let k = known_params("quiet splash mitigations=off mds=off pci=realloc root=UUID=x");
        assert_eq!(k, vec!["mitigations=off", "mds=off", "pci=realloc"]);
    }
}
