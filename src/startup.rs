//! O que abre quando o sistema liga, no estilo da aba Inicialização do Windows: programas do
//! login (autostart XDG e a restauração de sessão do KDE), serviços do usuário (systemd --user) e
//! serviços do sistema. Liga, desliga, adiciona e remove de forma simples e reversível:
//! - autostart: `Hidden=true` (no do sistema, uma cópia em ~/.config/autostart que só esconde);
//! - serviços: `systemctl [--user] enable|disable` (só o próximo boot, não para o que está rodando).
//!
//! Nada essencial (sessão gráfica, rede, login, numapilot) pode ser desligado por aqui.

use crate::autostart::{get, is_desktop_component, system_dir, user_dir};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Marca dos atalhos criados pelo numapilot (podem ser removidos).
const MARK: &str = "X-Numapilot-Startup=true";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    /// Programas que abrem quando você entra (autostart, restauração de sessão).
    Login,
    /// Serviços do seu usuário (systemd --user): abrem no login, sem janela.
    UserService,
    /// Serviços do sistema: abrem no boot, antes do login.
    SystemService,
    /// Partes do próprio desktop (KDE, GNOME).
    Session,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Login => "Programas no login",
            Group::UserService => "Serviços do usuário",
            Group::SystemService => "Serviços do sistema (boot)",
            Group::Session => "Componentes da sessão",
        }
    }
    pub fn about(self) -> &'static str {
        match self {
            Group::Login => "abrem quando você entra na sessão",
            Group::UserService => "rodam em segundo plano a partir do login",
            Group::SystemService => "rodam desde o boot, antes do login; mudar pede a senha",
            Group::Session => "partes do desktop; as essenciais não podem ser desligadas",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// "xdg:arquivo.desktop", "user:unidade", "sys:unidade" ou "kde:session".
    pub id: String,
    pub group: Group,
    pub name: String,
    /// Comando ou descrição.
    pub detail: String,
    pub enabled: bool,
    /// "você", "pacote", "numapilot", "sistema".
    pub origin: &'static str,
    /// Motivo para não deixar desligar (None = pode).
    pub protected: Option<&'static str>,
    /// Pode apagar de vez (criado por você ou pelo numapilot).
    pub removable: bool,
    /// Rodando agora.
    pub running: bool,
    /// Memória em uso agora (bytes), se der para saber.
    pub mem: Option<u64>,
    /// PIDs encontrados (para abrir na aba Programas).
    pub pids: Vec<i32>,
}

// ---------- autostart XDG ----------

/// Atalhos de inicialização que não podem ser desligados: sem eles a sessão quebra.
const ESSENTIAL_XDG: &[&str] = &[
    "org.kde.plasmashell",
    "polkit",
    "pam_kwallet",
    "at-spi",
    "xdg-user-dirs",
    "powerdevil",
    "org.kde.plasma-fallback-session-restore",
    "gnome-shell",
    "gnome-keyring",
    "org.gnome.SettingsDaemon",
    "xdg-desktop-portal",
];

fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// O atalho roda neste desktop? (OnlyShowIn / NotShowIn)
pub fn runs_here(text: &str, desktops: &[String]) -> bool {
    let list = |k: &str| -> Vec<String> {
        get(text, k)
            .unwrap_or("")
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect()
    };
    let only = list("OnlyShowIn");
    if !only.is_empty() && !desktops.is_empty() && !only.iter().any(|d| desktops.contains(d)) {
        return false;
    }
    !list("NotShowIn").iter().any(|d| desktops.contains(d))
}

fn xdg_enabled(text: &str) -> bool {
    get(text, "Hidden") != Some("true") && get(text, "X-GNOME-Autostart-enabled") != Some("false")
}

/// Troca (ou põe) uma chave na seção [Desktop Entry].
pub fn set_key(text: &str, key: &str, value: Option<&str>) -> String {
    let mut out: Vec<String> = vec![];
    let mut in_entry = false;
    let mut done = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            if in_entry && !done {
                if let Some(v) = value {
                    // antes das linhas em branco que separam as seções
                    let blanks = out.iter().rev().take_while(|l| l.trim().is_empty()).count();
                    out.insert(out.len() - blanks, format!("{key}={v}"));
                }
                done = true;
            }
            in_entry = t == "[Desktop Entry]";
            out.push(l.to_string());
            continue;
        }
        if in_entry && t.strip_prefix(key).is_some_and(|r| r.starts_with('=')) {
            if !done && let Some(v) = value {
                out.push(format!("{key}={v}"));
            }
            done = true;
            continue;
        }
        out.push(l.to_string());
    }
    if !done && let Some(v) = value {
        if !text.contains("[Desktop Entry]") {
            out.insert(0, "[Desktop Entry]".into());
        }
        // fim da seção [Desktop Entry] (ou do arquivo)
        let end = out
            .iter()
            .enumerate()
            .skip_while(|(_, l)| l.trim() != "[Desktop Entry]")
            .skip(1)
            .find(|(_, l)| l.trim().starts_with('['))
            .map(|(i, _)| i)
            .unwrap_or(out.len());
        out.insert(end, format!("{key}={v}"));
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

fn xdg_items(desktops: &[String], procs: &Procs) -> Vec<Item> {
    let mut files: BTreeMap<String, (Option<PathBuf>, Option<PathBuf>)> = BTreeMap::new();
    for (sys, dir) in [(true, system_dir()), (false, user_dir())] {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let f = e.file_name().to_string_lossy().to_string();
            let slot = files.entry(f).or_default();
            if sys {
                slot.0 = Some(p);
            } else {
                slot.1 = Some(p);
            }
        }
    }
    let mut v = vec![];
    for (file, (sys, user)) in files {
        let Some(path) = user.as_ref().or(sys.as_ref()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        if !runs_here(&text, desktops) || get(&text, "Type").is_some_and(|t| t != "Application") {
            continue;
        }
        let stem = file.trim_end_matches(".desktop");
        let essential = ESSENTIAL_XDG.iter().any(|e| stem.starts_with(e));
        let session = sys.is_some() && is_desktop_component(&text, &file);
        let ours = text.contains(MARK);
        let exec = get(&text, "Exec").unwrap_or("").to_string();
        let pids = procs.find(&exec);
        let mem = (!pids.is_empty()).then(|| pids.iter().filter_map(|p| pss(*p)).sum());
        v.push(Item {
            id: format!("xdg:{file}"),
            group: if session || essential {
                Group::Session
            } else {
                Group::Login
            },
            name: get(&text, "Name").unwrap_or(stem).to_string(),
            detail: if exec.is_empty() { file.clone() } else { exec },
            enabled: xdg_enabled(&text),
            origin: if ours {
                "numapilot"
            } else if sys.is_some() {
                "pacote"
            } else {
                "você"
            },
            protected: essential.then_some("essencial para a sessão gráfica"),
            removable: sys.is_none(),
            running: !pids.is_empty(),
            mem,
            pids,
        });
    }
    v
}

fn xdg_set(file: &str, on: bool) -> Result<(), String> {
    let user = user_dir().join(file);
    let sys = system_dir().join(file);
    let base = std::fs::read_to_string(&user)
        .or_else(|_| std::fs::read_to_string(&sys))
        .map_err(|e| format!("{file}: {e}"))?;
    if on && sys.is_file() && user.is_file() {
        // a cópia só existia para esconder o do sistema: apagar volta ao original
        let s = std::fs::read_to_string(&sys).unwrap_or_default();
        if set_key(
            &set_key(&base, "Hidden", None),
            "X-GNOME-Autostart-enabled",
            None,
        ) == set_key(
            &set_key(&s, "Hidden", None),
            "X-GNOME-Autostart-enabled",
            None,
        ) {
            return std::fs::remove_file(&user).map_err(|e| e.to_string());
        }
    }
    let mut t = set_key(&base, "Hidden", (!on).then_some("true"));
    t = set_key(&t, "X-GNOME-Autostart-enabled", None);
    std::fs::create_dir_all(user_dir()).map_err(|e| e.to_string())?;
    std::fs::write(&user, t).map_err(|e| e.to_string())
}

// ---------- restauração de sessão do KDE ----------

fn ksmserverrc() -> PathBuf {
    crate::autostart::user_dir()
        .parent()
        .map(|p| p.join("ksmserverrc"))
        .unwrap_or_else(|| PathBuf::from("ksmserverrc"))
}

/// (restaura a sessão anterior?, programas salvos no último logout)
pub fn kde_session(text: &str) -> (bool, Vec<String>) {
    let mut section = String::new();
    let mut mode = None;
    let mut apps = vec![];
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            section = t.to_string();
            continue;
        }
        if section == "[General]"
            && let Some(m) = t.strip_prefix("loginMode=")
        {
            mode = Some(m.to_string());
        }
        if section == "[Session: saved at previous logout]"
            && t.starts_with("program")
            && let Some((_, p)) = t.split_once('=')
        {
            let p = crate::util::basename(p.trim()).to_string();
            if !p.is_empty() && !p.starts_with("kwin") && !apps.contains(&p) {
                apps.push(p);
            }
        }
    }
    // padrão do Plasma: restaurar a sessão anterior
    let on = mode
        .as_deref()
        .is_none_or(|m| m == "restorePreviousLogout" || m == "restoreSavedSession");
    (on, apps)
}

fn kde_item() -> Option<Item> {
    if !current_desktops().iter().any(|d| d == "kde") {
        return None;
    }
    let text = std::fs::read_to_string(ksmserverrc()).unwrap_or_default();
    let (on, apps) = kde_session(&text);
    Some(Item {
        id: "kde:session".into(),
        group: Group::Login,
        name: "Reabrir o que estava aberto (KDE)".into(),
        detail: if apps.is_empty() {
            "nada salvo no último logout".into()
        } else {
            format!("último logout: {}", apps.join(", "))
        },
        enabled: on,
        origin: "sistema",
        protected: None,
        removable: false,
        running: false,
        mem: None,
        pids: vec![],
    })
}

fn kde_set(on: bool) -> Result<(), String> {
    let p = ksmserverrc();
    let text = std::fs::read_to_string(&p).unwrap_or_default();
    let mode = if on {
        "restorePreviousLogout"
    } else {
        "emptySession"
    };
    let mut out = vec![];
    let mut in_general = false;
    let mut done = false;
    let mut saw_general = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with('[') {
            if in_general && !done {
                out.push(format!("loginMode={mode}"));
                done = true;
            }
            in_general = t == "[General]";
            saw_general |= in_general;
        } else if in_general && t.starts_with("loginMode=") {
            out.push(format!("loginMode={mode}"));
            done = true;
            continue;
        }
        out.push(l.to_string());
    }
    if in_general && !done {
        out.push(format!("loginMode={mode}"));
        done = true;
    }
    if !saw_general || !done {
        out.insert(0, format!("[General]\nloginMode={mode}"));
    }
    std::fs::write(&p, out.join("\n") + "\n").map_err(|e| e.to_string())
}

// ---------- serviços (systemd) ----------

/// Serviços do sistema que não podem ser desligados por aqui.
const ESSENTIAL_UNITS: &[&str] = &[
    "systemd-",
    "dbus",
    "getty@",
    "NetworkManager",
    "sddm",
    "gdm",
    "plasmalogin",
    "lightdm",
    "greetd",
    "polkit",
    "udisks2",
    "upower",
    "iwd",
    "wpa_supplicant",
    "numapilot",
    "pipewire",
    "wireplumber",
    "xdg-",
    "plasma-",
    "at-spi",
];

fn remembered_path() -> PathBuf {
    user_dir()
        .parent()
        .map(|p| p.join("numapilot/startup-off.txt"))
        .unwrap_or_else(|| PathBuf::from("startup-off.txt"))
}

/// Unidade essencial (rede, login, sessão, numapilot): não deixar desligar nem parar.
pub fn essential_unit(unit: &str) -> bool {
    ESSENTIAL_UNITS.iter().any(|e| unit.starts_with(e))
}

/// Unidades desligadas pelo numapilot: continuam na lista para poder ligar de novo.
fn remembered() -> BTreeSet<String> {
    std::fs::read_to_string(remembered_path())
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn remember(id: &str, off: bool) {
    let mut s = remembered();
    if off {
        s.insert(id.to_string());
    } else {
        s.remove(id);
    }
    let p = remembered_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, s.into_iter().collect::<Vec<_>>().join("\n") + "\n");
}

fn systemctl(user: bool) -> Command {
    let mut c = Command::new("systemctl");
    if user {
        c.arg("--user");
    }
    c
}

/// Unidades ligadas (serviços e timers).
fn enabled_units(user: bool) -> Vec<String> {
    let o = systemctl(user)
        .args([
            "list-unit-files",
            "--state=enabled",
            "--type=service,timer",
            "--no-legend",
            "--no-pager",
        ])
        .output();
    let Ok(o) = o else { return vec![] };
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        // modelos (getty@.service) não rodam sozinhos
        .filter(|u| !u.contains("@."))
        .map(String::from)
        .collect()
}

/// Estado da unidade (enabled, disabled, masked...).
fn unit_state(user: bool, unit: &str) -> Option<String> {
    let o = systemctl(user).args(["is-enabled", unit]).output().ok()?;
    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// `systemctl show` de várias unidades de uma vez: blocos separados por linha em branco.
pub fn parse_show(text: &str) -> Vec<BTreeMap<String, String>> {
    text.split("\n\n")
        .map(|b| {
            b.lines()
                .filter_map(|l| l.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>()
        })
        .filter(|m| m.contains_key("Id"))
        .collect()
}

fn unit_items(user: bool) -> Vec<Item> {
    let prefix = if user { "user:" } else { "sys:" };
    let mut units = enabled_units(user);
    for r in remembered() {
        if let Some(u) = r.strip_prefix(prefix)
            && !units.iter().any(|x| x == u)
        {
            units.push(u.to_string());
        }
    }
    if units.is_empty() {
        return vec![];
    }
    let o = systemctl(user)
        .args([
            "show",
            "-p",
            "Id,Description,ActiveState,UnitFileState,MemoryCurrent,ExecStart,MainPID,FragmentPath",
            "--no-pager",
        ])
        .args(&units)
        .output();
    let Ok(o) = o else { return vec![] };
    parse_show(&String::from_utf8_lossy(&o.stdout))
        .into_iter()
        .filter_map(|m| {
            let id = m.get("Id")?.clone();
            let state = m.get("UnitFileState").map(String::as_str).unwrap_or("");
            // estático: não é algo que se liga ou desliga; mascarado só se fomos nós
            let ours_masked = state == "masked" && remembered().contains(&format!("{prefix}{id}"));
            if !matches!(state, "enabled" | "disabled" | "enabled-runtime") && !ours_masked {
                return None;
            }
            let essential = essential_unit(&id);
            let frag = m.get("FragmentPath").cloned().unwrap_or_default();
            // mascarado: o arquivo vira /dev/null (o original é do pacote)
            let mine = frag != "/dev/null"
                && (frag.contains("/.config/systemd/") || frag.starts_with("/etc/systemd/system/"));
            let exec = m
                .get("ExecStart")
                .and_then(|e| e.split("argv[]=").nth(1))
                .map(|e| e.split(" ;").next().unwrap_or(e).trim().to_string())
                .unwrap_or_default();
            let desc = m.get("Description").cloned().unwrap_or_default();
            let timer = id.ends_with(".timer");
            let main_pid: i32 = m.get("MainPID").and_then(|p| p.parse().ok()).unwrap_or(0);
            Some(Item {
                id: format!("{prefix}{id}"),
                group: if user {
                    Group::UserService
                } else {
                    Group::SystemService
                },
                name: if timer {
                    format!("{desc} (agendado)")
                } else if desc.is_empty() {
                    id.clone()
                } else {
                    desc
                },
                detail: if exec.is_empty() {
                    id.clone()
                } else {
                    format!("{id} · {exec}")
                },
                enabled: state.starts_with("enabled"),
                origin: if mine { "você" } else { "pacote" },
                protected: essential.then_some("essencial para o sistema, a rede ou o login"),
                removable: false,
                running: m.get("ActiveState").is_some_and(|s| s == "active"),
                mem: m
                    .get("MemoryCurrent")
                    .and_then(|v| v.parse::<u64>().ok())
                    .filter(|v| *v < u64::MAX / 2),
                pids: if main_pid > 0 { vec![main_pid] } else { vec![] },
            })
        })
        .collect()
}

// ---------- processos rodando (para "rodando agora" e memória) ----------

struct Procs {
    /// (pid, nome curto, executável, cgroup)
    list: Vec<(i32, String, String, String)>,
    me: u32,
}

impl Procs {
    fn scan() -> Procs {
        let me = unsafe { libc::getuid() };
        let mut list = vec![];
        for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else {
                continue;
            };
            let Ok(md) = std::fs::metadata(e.path()) else {
                continue;
            };
            if std::os::unix::fs::MetadataExt::uid(&md) != me {
                continue;
            }
            let comm = std::fs::read_to_string(e.path().join("comm"))
                .unwrap_or_default()
                .trim()
                .to_string();
            let exe = std::fs::read_link(e.path().join("exe"))
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let cg = std::fs::read_to_string(e.path().join("cgroup")).unwrap_or_default();
            list.push((pid, comm, exe, cg));
        }
        Procs { list, me }
    }

    /// Processos de um Exec= de autostart (flatpak pelo id do app no cgroup).
    fn find(&self, exec: &str) -> Vec<i32> {
        let _ = self.me;
        let toks: Vec<&str> = exec
            .split_whitespace()
            .map(|t| t.trim_matches('"'))
            .collect();
        if toks.first().is_some_and(|t| t.ends_with("flatpak")) {
            let Some(app) = toks
                .iter()
                .skip(2)
                .find(|t| !t.starts_with('-') && t.contains('.'))
            else {
                return vec![];
            };
            let tag = format!("app-flatpak-{app}-");
            return self
                .list
                .iter()
                .filter(|p| p.3.contains(&tag))
                .map(|p| p.0)
                .collect();
        }
        let Some(prog) = crate::desktop::exec_program(exec) else {
            return vec![];
        };
        let base = crate::util::basename(&prog);
        if base.is_empty() || base == "sh" || base == "bash" {
            return vec![];
        }
        // comm tem no máximo 15 caracteres
        let short: String = base.chars().take(15).collect();
        self.list
            .iter()
            .filter(|p| crate::util::basename(&p.2) == base || p.1 == short)
            .map(|p| p.0)
            .collect()
    }
}

fn pss(pid: i32) -> Option<u64> {
    let t = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
    t.lines()
        .find(|l| l.starts_with("Pss:"))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse::<u64>().ok())
        .map(|kb| kb * 1024)
}

// ---------- API ----------

/// Tudo que abre com o sistema, agrupado e em ordem de nome.
pub fn list() -> Vec<Item> {
    let desktops = current_desktops();
    let procs = Procs::scan();
    let mut v = xdg_items(&desktops, &procs);
    v.extend(kde_item());
    v.extend(unit_items(true));
    v.extend(unit_items(false));
    v.sort_by(|a, b| {
        a.group
            .cmp(&b.group)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    v
}

/// O que fazer para ligar ou desligar. Serviços do sistema precisam de root: devolve o comando
/// (`pkexec systemctl ...`) para a interface rodar mostrando a saída.
pub enum Action {
    Done,
    NeedsRoot(Vec<String>),
}

pub fn set_enabled(item: &Item, on: bool) -> Result<Action, String> {
    if !on && let Some(why) = item.protected {
        return Err(format!("{}: {why}", item.name));
    }
    if let Some(file) = item.id.strip_prefix("xdg:") {
        xdg_set(file, on)?;
        return Ok(Action::Done);
    }
    if item.id == "kde:session" {
        kde_set(on)?;
        return Ok(Action::Done);
    }
    let verb = if on { "enable" } else { "disable" };
    if let Some(u) = item.id.strip_prefix("user:") {
        let run = |args: &[&str]| -> Result<(), String> {
            let o = systemctl(true)
                .args(args)
                .output()
                .map_err(|e| e.to_string())?;
            if o.status.success() {
                Ok(())
            } else {
                Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
            }
        };
        if on {
            // desligado com mask (ligado para todos pelo pacote): tira o mask
            if unit_state(true, u).as_deref() == Some("masked") {
                run(&["unmask", u])?;
            }
            if unit_state(true, u).as_deref() != Some("enabled") {
                run(&["enable", u])?;
            }
        } else {
            run(&["disable", u])?;
            // ligado para todos os usuários pelo pacote (/etc/systemd/user): o disable não
            // alcança; o mask desliga só para você
            if unit_state(true, u).as_deref() == Some("enabled") {
                run(&["mask", u])?;
            }
        }
        let st = unit_state(true, u).unwrap_or_default();
        let ok = if on {
            st.starts_with("enabled")
        } else {
            st == "disabled" || st == "masked"
        };
        if !ok {
            return Err(format!("{}: continua {st}", item.name));
        }
        remember(&item.id, !on);
        return Ok(Action::Done);
    }
    if let Some(u) = item.id.strip_prefix("sys:") {
        remember(&item.id, !on);
        return Ok(Action::NeedsRoot(vec![
            "pkexec".into(),
            "systemctl".into(),
            verb.into(),
            u.into(),
        ]));
    }
    Err(format!("não sei mudar {}", item.id))
}

/// Apaga um atalho de inicialização criado por você ou pelo numapilot.
pub fn remove(item: &Item) -> Result<(), String> {
    let Some(file) = item.id.strip_prefix("xdg:").filter(|_| item.removable) else {
        return Err(format!("{} só pode ser desligado, não apagado", item.name));
    };
    std::fs::remove_file(user_dir().join(file)).map_err(|e| e.to_string())
}

fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() { "programa".into() } else { s }
}

/// Aspas de shell para um argumento do Exec= (padrão freedesktop: aspas duplas com escape).
pub fn exec_quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+=:,@".contains(c))
    {
        return arg.to_string();
    }
    let mut s = String::from("\"");
    for c in arg.chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s
}

/// Conteúdo de um atalho de inicialização novo.
pub fn desktop_text(name: &str, exec: &str, delay_s: u32) -> String {
    let exec = if delay_s > 0 {
        format!(
            "sh -c {}",
            exec_quote(&format!("sleep {delay_s}; exec {exec}"))
        )
    } else {
        exec.to_string()
    };
    format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={exec}\nX-GNOME-Autostart-enabled=true\n{MARK}\n",
        name.replace('\n', " ")
    )
}

/// Adiciona um comando qualquer. Devolve o arquivo criado.
pub fn add_command(name: &str, exec: &str, delay_s: u32) -> Result<PathBuf, String> {
    if exec.trim().is_empty() {
        return Err("comando vazio".into());
    }
    let dir = user_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut p = dir.join(format!("{}.desktop", slug(name)));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{}-{n}.desktop", slug(name)));
        n += 1;
    }
    std::fs::write(&p, desktop_text(name, exec.trim(), delay_s)).map_err(|e| e.to_string())?;
    Ok(p)
}

/// Adiciona um arquivo: AppImage, script (.sh) ou binário. Torna executável se precisar
/// (o script sem permissão roda pelo sh).
pub fn add_file(path: &Path, delay_s: u32) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let md = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !md.is_file() {
        return Err(format!("{} não é um arquivo", path.display()));
    }
    let p = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let ps = p.to_string_lossy().to_string();
    let exec_bit = md.permissions().mode() & 0o111 != 0;
    let is_script = ps.ends_with(".sh");
    let exec = if exec_bit {
        exec_quote(&ps)
    } else if is_script {
        format!("sh {}", exec_quote(&ps))
    } else {
        let mut perm = md.permissions();
        perm.set_mode(perm.mode() | 0o755);
        std::fs::set_permissions(&p, perm)
            .map_err(|e| format!("não consegui tornar executável: {e}"))?;
        exec_quote(&ps)
    };
    let name = p
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "programa".into());
    add_command(&name, &exec, delay_s)
}

/// Adiciona um programa instalado pelo atalho do menu (.desktop).
pub fn add_desktop(app: &Path, delay_s: u32) -> Result<PathBuf, String> {
    let text = std::fs::read_to_string(app).map_err(|e| format!("{}: {e}", app.display()))?;
    let file = app
        .file_name()
        .ok_or("atalho sem nome")?
        .to_string_lossy()
        .to_string();
    let dest = user_dir().join(&file);
    if dest.exists() {
        // já existe (talvez desligado): só liga
        let t = std::fs::read_to_string(&dest).unwrap_or_default();
        std::fs::write(
            &dest,
            set_key(
                &set_key(&t, "Hidden", None),
                "X-GNOME-Autostart-enabled",
                None,
            ),
        )
        .map_err(|e| e.to_string())?;
        return Ok(dest);
    }
    let mut t = set_key(&text, "Hidden", None);
    t = set_key(&t, "NoDisplay", None);
    if delay_s > 0
        && let Some(exec) = get(&t, "Exec")
    {
        let e = exec
            .split_whitespace()
            .filter(|a| !a.starts_with('%'))
            .collect::<Vec<_>>()
            .join(" ");
        t = set_key(
            &t,
            "Exec",
            Some(&format!(
                "sh -c {}",
                exec_quote(&format!("sleep {delay_s}; exec {e}"))
            )),
        );
    }
    t = set_key(&t, "X-Numapilot-Startup", Some("true"));
    std::fs::create_dir_all(user_dir()).map_err(|e| e.to_string())?;
    std::fs::write(&dest, t).map_err(|e| e.to_string())?;
    Ok(dest)
}

/// Adiciona um programa que está aberto agora: pelo atalho do menu dele, se achar; senão pela
/// linha de comando com que ele foi aberto.
pub fn add_running(pid: i32, label: &str) -> Result<PathBuf, String> {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| format!("PID {pid}: {e}"))?;
    if let Some(d) = crate::desktop::find_for_exe(&exe) {
        return add_desktop(&d, 0);
    }
    let cmd = std::fs::read(format!("/proc/{pid}/cmdline")).map_err(|e| e.to_string())?;
    let args: Vec<String> = cmd
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| exec_quote(&String::from_utf8_lossy(a)))
        .collect();
    if args.is_empty() {
        return Err(format!("não consegui ler a linha de comando de {label}"));
    }
    add_command(label, &args.join(" "), 0)
}

/// O programa (pelo executável) já abre no login? Devolve o item.
pub fn find_for_exe(exe: &str) -> Option<Item> {
    let base = crate::util::basename(exe);
    let desktops = current_desktops();
    let procs = Procs {
        list: vec![],
        me: 0,
    };
    xdg_items(&desktops, &procs).into_iter().find(|i| {
        crate::desktop::exec_program(&i.detail).is_some_and(|p| crate::util::basename(&p) == base)
    })
}

/// Programas instalados (atalhos do menu) que batem com a busca: (nome, arquivo).
pub fn search_apps(query: &str) -> Vec<(String, PathBuf)> {
    let q = query.to_lowercase();
    let mut seen = BTreeSet::new();
    let mut v = vec![];
    for dir in crate::desktop::search_dirs() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let f = e.file_name().to_string_lossy().to_string();
            if !seen.insert(f.clone()) {
                continue;
            }
            let Ok(t) = std::fs::read_to_string(&p) else {
                continue;
            };
            if get(&t, "NoDisplay") == Some("true")
                || get(&t, "Hidden") == Some("true")
                || get(&t, "Type").is_some_and(|x| x != "Application")
            {
                continue;
            }
            let name = get(&t, "Name").unwrap_or(&f).to_string();
            if q.is_empty() || name.to_lowercase().contains(&q) || f.to_lowercase().contains(&q) {
                v.push((name, p));
            }
        }
    }
    v.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chaves_do_desktop() {
        let t = "[Desktop Entry]\nName=Steam\nExec=steam -silent\n\n[Desktop Action x]\nExec=y\n";
        let off = set_key(t, "Hidden", Some("true"));
        assert!(off.contains("Exec=steam -silent\nHidden=true\n"));
        assert!(off.contains("[Desktop Action x]\nExec=y"));
        assert!(!xdg_enabled(&off));
        let on = set_key(&off, "Hidden", None);
        assert!(!on.contains("Hidden"));
        assert!(xdg_enabled(&on));
        let t2 = set_key("[Desktop Entry]\nHidden=true\n", "Hidden", Some("false"));
        assert_eq!(t2, "[Desktop Entry]\nHidden=false\n");
        assert!(!xdg_enabled(
            "[Desktop Entry]\nX-GNOME-Autostart-enabled=false\n"
        ));
    }

    #[test]
    fn so_no_desktop_certo() {
        let kde = vec!["kde".to_string()];
        assert!(runs_here("[Desktop Entry]\nOnlyShowIn=KDE;\n", &kde));
        assert!(!runs_here("[Desktop Entry]\nOnlyShowIn=GNOME;\n", &kde));
        assert!(!runs_here("[Desktop Entry]\nNotShowIn=KDE;\n", &kde));
        assert!(runs_here("[Desktop Entry]\nName=x\n", &kde));
    }

    #[test]
    fn sessao_do_kde() {
        let t = "[General]\nloginMode=emptySession\n[Session: saved at previous logout]\ncount=2\nprogram1=/usr/bin/dolphin\nprogram2=kwin_x11\n";
        assert_eq!(kde_session(t), (false, vec!["dolphin".to_string()]));
        assert_eq!(kde_session("").0, true);
    }

    #[test]
    fn systemctl_show() {
        let t = "Id=docker.service\nDescription=Docker\nUnitFileState=enabled\n\nId=x.timer\nDescription=X\n";
        let v = parse_show(t);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0]["Description"], "Docker");
    }

    #[test]
    fn comando_com_espaco_e_atraso() {
        assert_eq!(exec_quote("/opt/a b/x.AppImage"), "\"/opt/a b/x.AppImage\"");
        assert_eq!(exec_quote("/usr/bin/steam"), "/usr/bin/steam");
        let t = desktop_text("Hermes", "hermes-agent --serve", 15);
        assert!(t.contains("Exec=sh -c \"sleep 15; exec hermes-agent --serve\""));
        assert!(t.contains(MARK));
    }

    #[test]
    fn adicionar_ligar_desligar_remover() {
        let root = std::env::temp_dir().join(format!("np-startup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let sys = root.join("xdg");
        std::fs::create_dir_all(&sys).unwrap();
        std::fs::write(
            sys.join("steam.desktop"),
            "[Desktop Entry]\nType=Application\nName=Steam\nExec=steam -silent\n",
        )
        .unwrap();
        let _g = crate::util::test_sysfs_lock(&root);
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
            std::env::set_var("NUMAPILOT_XDG_AUTOSTART", &sys);
        }
        let procs = Procs {
            list: vec![],
            me: 0,
        };
        // do sistema: desligar cria a cópia que esconde; ligar apaga a cópia
        let steam = xdg_items(&[], &procs)
            .into_iter()
            .find(|i| i.id == "xdg:steam.desktop")
            .unwrap();
        assert!(steam.enabled && !steam.removable && steam.origin == "pacote");
        set_enabled(&steam, false).unwrap();
        assert!(!xdg_items(&[], &procs)[0].enabled);
        set_enabled(&steam, true).unwrap();
        assert!(!user_dir().join("steam.desktop").exists());
        // comando novo: aparece, desliga, remove
        add_command("Hermes agent", "hermes --serve", 0).unwrap();
        let h = xdg_items(&[], &procs)
            .into_iter()
            .find(|i| i.name == "Hermes agent")
            .unwrap();
        assert!(h.removable && h.origin == "numapilot" && h.enabled);
        set_enabled(&h, false).unwrap();
        remove(&h).unwrap();
        assert!(
            xdg_items(&[], &procs)
                .iter()
                .all(|i| i.name != "Hermes agent")
        );
        // arquivo: script sem permissão roda pelo sh
        let sh = root.join("meu script.sh");
        std::fs::write(&sh, "#!/bin/sh\necho oi\n").unwrap();
        let f = add_file(&sh, 0).unwrap();
        assert!(std::fs::read_to_string(f).unwrap().contains("Exec=sh \""));
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var("NUMAPILOT_XDG_AUTOSTART");
        }
        let _ = std::fs::remove_dir_all(&root);
    }
}
