//! Tudo que abre com o sistema na placa integrada (notebook com NVIDIA): os atalhos de
//! inicialização (~/.config/autostart e /etc/xdg/autostart) e os serviços do usuário ligados
//! no login ganham as variáveis que fazem OpenGL/EGL/Vulkan usarem só a integrada. Assim
//! nenhum deles acorda a NVIDIA à toa. Reversível: o Exec original fica guardado no arquivo.

use crate::topo::Topology;
use std::path::{Path, PathBuf};

const MARK: &str = "X-Numapilot-Exec=";
const COPIED: &str = "X-Numapilot-Copied=true";

fn config_home() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

pub fn user_dir() -> PathBuf {
    config_home().join("autostart")
}

pub fn system_dir() -> PathBuf {
    std::env::var_os("NUMAPILOT_XDG_AUTOSTART")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/xdg/autostart"))
}

pub fn flag_file() -> PathBuf {
    config_home().join("numapilot/autostart-igpu")
}

pub fn enabled() -> bool {
    flag_file().exists()
}

/// Variáveis da integrada (vazio se não for notebook com integrada + NVIDIA). Só as que
/// escolhem a placa (sem as do numapilot, que fariam o serviço tratar o programa como manual).
pub fn igpu_env(topo: &Topology) -> Vec<(String, String)> {
    let Some((ig, _)) = crate::hybrid::pair(topo) else {
        return vec![];
    };
    crate::decide::env_for_gpu(ig, topo)
        .into_iter()
        .filter(|(k, _)| !k.starts_with("NUMAPILOT_") && k != "LSFGVK_GPU")
        .collect()
}

/// Lançadores de jogos ficam como estão: o que eles abrem (os jogos) deve poder ir para a NVIDIA.
pub const SKIP: &[&str] = &[
    "steam",
    "heroic",
    "lutris",
    "bottles",
    "itch",
    "minigalaxy",
    "gamescope",
    "prismlauncher",
    "retroarch",
];

/// Componentes do próprio desktop (Plasma, GNOME...): não usam a NVIDIA e uma cópia deles na pasta
/// do usuário esconderia as atualizações do sistema.
pub(crate) fn is_desktop_component(text: &str, file: &str) -> bool {
    get(text, "OnlyShowIn").is_some()
        || get(text, "X-KDE-autostart-phase").is_some()
        || get(text, "X-GNOME-Autostart-Phase").is_some()
        || get(text, "X-systemd-skip") == Some("true")
        || [
            "org.kde.",
            "at-spi",
            "xdg-user-dirs",
            "polkit",
            "pam_kwallet",
            "baloo",
            "gmenudbusmenuproxy",
            "xembedsniproxy",
            "geoclue",
            "gnome-",
            "kaccess",
            "powerdevil",
            "plasma",
        ]
        .iter()
        .any(|p| file.to_lowercase().starts_with(p))
}

fn is_launcher(text: &str, file: &str) -> bool {
    let exec = get(text, "Exec").unwrap_or("").to_lowercase();
    let first = exec.split_whitespace().next().unwrap_or("");
    let bin = first.rsplit('/').next().unwrap_or(first);
    let f = file.to_lowercase();
    SKIP.iter().any(|s| bin.starts_with(s) || f.contains(s))
}

pub(crate) fn get<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut in_entry = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            in_entry = t == "[Desktop Entry]";
            continue;
        }
        if in_entry && let Some(v) = t.strip_prefix(key).and_then(|r| r.strip_prefix('=')) {
            return Some(v);
        }
    }
    None
}

/// Troca o Exec da seção [Desktop Entry] (e guarda o original).
pub fn wrap(text: &str, prefix: &str) -> Option<String> {
    let exec = get(text, "Exec")?;
    if exec.starts_with("env ") && text.contains(MARK) {
        return None;
    }
    let mut out = String::new();
    let mut in_entry = false;
    let mut done = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            if in_entry && !done {
                out.push_str(&format!("{MARK}{exec}\n"));
                done = true;
            }
            in_entry = t == "[Desktop Entry]";
        }
        if in_entry && t.starts_with("Exec=") {
            out.push_str(&format!("Exec={prefix} {exec}\n"));
            continue;
        }
        if in_entry && t.starts_with(MARK) {
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    if in_entry && !done {
        out.push_str(&format!("{MARK}{exec}\n"));
    }
    Some(out)
}

/// Desfaz o `wrap`.
pub fn unwrap(text: &str) -> Option<String> {
    let orig = get(text, MARK.trim_end_matches('='))?.to_string();
    let mut out = String::new();
    let mut in_entry = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            in_entry = t == "[Desktop Entry]";
        }
        if t.starts_with(MARK) {
            continue;
        }
        if in_entry && t.starts_with("Exec=") {
            out.push_str(&format!("Exec={orig}\n"));
            continue;
        }
        out.push_str(l);
        out.push('\n');
    }
    Some(out)
}

fn prefix(env: &[(String, String)]) -> String {
    let mut s = String::from("env");
    for (k, v) in env {
        s.push_str(&format!(" {k}={v}"));
    }
    s
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub file: String,
    pub name: String,
    pub wrapped: bool,
    pub hidden: bool,
}

fn desktop_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "desktop"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// O que abre com o sistema (o arquivo do usuário vale por cima do do sistema).
pub fn entries() -> Vec<Entry> {
    let mut map: std::collections::BTreeMap<String, PathBuf> = Default::default();
    for d in [system_dir(), user_dir()] {
        for f in desktop_files(&d) {
            map.insert(
                f.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                f,
            );
        }
    }
    map.into_iter()
        .filter_map(|(file, p)| {
            let t = std::fs::read_to_string(&p).ok()?;
            let hidden = get(&t, "Hidden") == Some("true");
            Some(Entry {
                name: get(&t, "Name").unwrap_or(&file).to_string(),
                wrapped: t.contains(MARK),
                hidden,
                file,
            })
        })
        .collect()
}

/// Serviços do usuário ligados no login (ex.: sunshine.service).
pub fn user_services() -> Vec<String> {
    let base = config_home().join("systemd/user");
    let mut v = vec![];
    for t in [
        "default.target.wants",
        "graphical-session.target.wants",
        "xdg-desktop-autostart.target.wants",
    ] {
        for e in std::fs::read_dir(base.join(t))
            .map(|r| r.flatten().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            let n = e.file_name().to_string_lossy().to_string();
            if n.ends_with(".service") && !n.contains("numapilot") && !v.contains(&n) {
                v.push(n);
            }
        }
    }
    v.sort();
    v
}

fn dropin(svc: &str) -> PathBuf {
    config_home().join(format!("systemd/user/{svc}.d/numapilot-igpu.conf"))
}

/// Aplica (ou reaplica, pegando programas novos). Devolve o que mudou.
pub fn apply(topo: &Topology) -> Result<Vec<String>, String> {
    let env = igpu_env(topo);
    if env.is_empty() {
        return Err(crate::i18n::tr("só para notebook com placa integrada + NVIDIA").into());
    }
    let pre = prefix(&env);
    let mut changed = vec![];
    std::fs::create_dir_all(user_dir()).map_err(|e| e.to_string())?;
    // Cópias de entradas do sistema: some se o sistema tirou, refaz se o sistema mudou, e tira as
    // de componentes do desktop (a versão anterior do numapilot copiava).
    for f in desktop_files(&user_dir()) {
        let Ok(t) = std::fs::read_to_string(&f) else {
            continue;
        };
        if !t.contains(COPIED) {
            continue;
        }
        let name = f
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let sys = std::fs::read_to_string(system_dir().join(&name)).ok();
        let stale = match &sys {
            None => true,
            Some(st) => {
                is_desktop_component(st, &name)
                    || get(st, "Exec") != get(&t, MARK.trim_end_matches('='))
            }
        };
        if stale {
            let _ = std::fs::remove_file(&f);
        }
    }
    for e in entries() {
        if e.hidden || e.wrapped || e.file.contains("numapilot") {
            continue;
        }
        let user = user_dir().join(&e.file);
        let (src, copied) = if user.exists() {
            (user.clone(), false)
        } else {
            (system_dir().join(&e.file), true)
        };
        let Ok(t) = std::fs::read_to_string(&src) else {
            continue;
        };
        if is_launcher(&t, &e.file) || (copied && is_desktop_component(&t, &e.file)) {
            continue;
        }
        menu_override(&t, &pre);
        let Some(mut w) = wrap(&t, &pre) else {
            continue;
        };
        if copied {
            w.push_str(&format!("{COPIED}\n"));
        }
        std::fs::write(&user, w).map_err(|er| format!("{}: {er}", user.display()))?;
        changed.push(e.name);
    }
    let body = format!(
        "# numapilot: este serviço usa só a placa integrada (a NVIDIA pode dormir)\n[Service]\n{}",
        env.iter()
            .map(|(k, v)| format!("Environment={k}={v}\n"))
            .collect::<String>()
    );
    for s in user_services() {
        let f = dropin(&s);
        if std::fs::read_to_string(&f).ok().as_deref() == Some(body.as_str()) {
            continue;
        }
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(&f, &body).map_err(|e| format!("{}: {e}", f.display()))?;
        changed.push(s);
    }
    if let Some(d) = flag_file().parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(flag_file(), "1");
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    Ok(changed)
}

/// O mesmo programa aberto pelo menu (ou pela restauração da sessão) também vai para a
/// integrada: atalho do menu com as mesmas variáveis.
fn menu_override(autostart_text: &str, pre: &str) {
    let Some(prog) = crate::desktop::exec_program(get(autostart_text, "Exec").unwrap_or("")) else {
        return;
    };
    let prog_name = prog.rsplit('/').next().unwrap_or(&prog).to_string();
    let user_apps = crate::desktop::user_dir();
    for d in crate::desktop::search_dirs() {
        if d == user_apps {
            continue;
        }
        for f in desktop_files(&d) {
            let Ok(t) = std::fs::read_to_string(&f) else {
                continue;
            };
            let Some(p) = crate::desktop::exec_program(get(&t, "Exec").unwrap_or("")) else {
                continue;
            };
            if p.rsplit('/').next() != Some(prog_name.as_str()) {
                continue;
            }
            let dst = user_apps.join(f.file_name().unwrap_or_default());
            if dst.exists() {
                return;
            }
            if let Some(mut w) = wrap(&t, pre) {
                w.push_str(&format!("{COPIED}\n"));
                let _ = std::fs::create_dir_all(&user_apps);
                let _ = std::fs::write(&dst, w);
            }
            return;
        }
    }
}

/// Desfaz tudo.
pub fn revert() -> Vec<String> {
    for f in desktop_files(&crate::desktop::user_dir()) {
        if std::fs::read_to_string(&f).is_ok_and(|t| t.contains(MARK) && t.contains(COPIED)) {
            let _ = std::fs::remove_file(&f);
        }
    }
    let mut changed = vec![];
    for f in desktop_files(&user_dir()) {
        let Ok(t) = std::fs::read_to_string(&f) else {
            continue;
        };
        if !t.contains(MARK) {
            continue;
        }
        let name = get(&t, "Name").unwrap_or("?").to_string();
        if t.contains(COPIED) {
            let _ = std::fs::remove_file(&f);
        } else if let Some(u) = unwrap(&t) {
            let _ = std::fs::write(&f, u);
        }
        changed.push(name);
    }
    for s in user_services() {
        let f = dropin(&s);
        if f.exists() {
            let _ = std::fs::remove_file(&f);
            changed.push(s);
        }
    }
    let _ = std::fs::remove_file(flag_file());
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_roundtrip() {
        let t = "[Desktop Entry]\nName=ZapZap\nExec=zapzap --hideStart %u\nType=Application\n\n[Desktop Action New]\nExec=zapzap --new\n";
        let w = wrap(t, "env A=1").unwrap();
        assert!(w.contains("Exec=env A=1 zapzap --hideStart %u\n"));
        assert!(w.contains("Exec=zapzap --new\n"), "{w}");
        assert!(w.contains("X-Numapilot-Exec=zapzap --hideStart %u"));
        assert!(wrap(&w, "env A=1").is_none());
        let u = unwrap(&w).unwrap();
        assert!(u.contains("Exec=zapzap --hideStart %u\n") && !u.contains(MARK));
        assert!(u.contains("Exec=zapzap --new\n"));
    }
}
