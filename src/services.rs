//! systemd services (system and user), like the Services tab of a task manager: state,
//! whether it starts at boot, memory, and start/stop/restart and view the log.
//! System services need root: the actions become a `pkexec systemctl ...` command.

use std::collections::BTreeMap;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct Service {
    pub unit: String,
    /// The user's (systemctl --user) or the system's.
    pub user: bool,
    pub description: String,
    /// active, inactive, failed, activating...
    pub active: String,
    /// running, exited, dead...
    pub sub: String,
    /// enabled, disabled, static, masked...
    pub file_state: String,
    pub main_pid: i32,
    pub mem: Option<u64>,
    /// Why it may not be stopped (None = it may).
    pub protected: Option<&'static str>,
}

impl Service {
    pub fn running(&self) -> bool {
        self.active == "active" || self.active == "activating" || self.active == "reloading"
    }
    pub fn failed(&self) -> bool {
        self.active == "failed"
    }
    /// "running", "stopped", "failed"...
    pub fn state_text(&self) -> &'static str {
        match (self.active.as_str(), self.sub.as_str()) {
            ("active", "running") => crate::i18n::tr("running"),
            ("active", "exited") => crate::i18n::tr("finished (ok)"),
            ("active", "waiting") | ("active", "listening") => crate::i18n::tr("waiting"),
            ("active", _) => crate::i18n::tr("active"),
            ("activating", _) => crate::i18n::tr("starting"),
            ("deactivating", _) => crate::i18n::tr("stopping"),
            ("reloading", _) => crate::i18n::tr("reloading"),
            ("failed", _) => crate::t2!("failed", "com falha"),
            _ => crate::i18n::tr("stopped"),
        }
    }
    /// Abre no boot/login?
    pub fn at_boot(&self) -> Option<bool> {
        match self.file_state.as_str() {
            "enabled" | "enabled-runtime" | "alias" => Some(true),
            "disabled" | "masked" => Some(false),
            _ => None,
        }
    }
    fn id(&self) -> String {
        format!("{}:{}", if self.user { "user" } else { "sys" }, self.unit)
    }
}

fn systemctl(user: bool) -> Command {
    let mut c = Command::new("systemctl");
    if user {
        c.arg("--user");
    }
    c
}

/// `systemctl list-units --plain --no-legend` lines: unit, load, active, sub, description.
pub fn parse_list_units(text: &str) -> Vec<(String, String, String, String)> {
    text.lines()
        .filter_map(|l| {
            // failed or not-found comes with "●" in front
            let mut it = l.split_whitespace().skip_while(|t| *t == "●");
            let unit = it.next()?.trim_start_matches('●').to_string();
            if !unit.ends_with(".service") {
                return None;
            }
            let _load = it.next()?;
            let active = it.next()?.to_string();
            let sub = it.next()?.to_string();
            let desc = it.collect::<Vec<_>>().join(" ");
            Some((unit, active, sub, desc))
        })
        .collect()
}

/// All services (loaded and installed), system or user.
pub fn list(user: bool) -> Vec<Service> {
    let mut map: BTreeMap<String, Service> = BTreeMap::new();
    if let Ok(o) = systemctl(user)
        .args([
            "list-units",
            "--type=service",
            "--all",
            "--plain",
            "--no-legend",
            "--no-pager",
        ])
        .output()
    {
        for (unit, active, sub, desc) in parse_list_units(&String::from_utf8_lossy(&o.stdout)) {
            map.insert(
                unit.clone(),
                Service {
                    unit,
                    user,
                    description: desc,
                    active,
                    sub,
                    file_state: String::new(),
                    main_pid: 0,
                    mem: None,
                    protected: None,
                },
            );
        }
    }
    // installed ones that were never loaded (disabled and stopped)
    if let Ok(o) = systemctl(user)
        .args([
            "list-unit-files",
            "--type=service",
            "--no-legend",
            "--no-pager",
        ])
        .output()
    {
        for l in String::from_utf8_lossy(&o.stdout).lines() {
            let mut it = l.split_whitespace();
            let (Some(u), Some(st)) = (it.next(), it.next()) else {
                continue;
            };
            if u.contains("@.") {
                continue;
            }
            let e = map.entry(u.to_string()).or_insert_with(|| Service {
                unit: u.to_string(),
                user,
                description: String::new(),
                active: "inactive".into(),
                sub: "dead".into(),
                file_state: String::new(),
                main_pid: 0,
                mem: None,
                protected: None,
            });
            e.file_state = st.to_string();
        }
    }
    // memory and PID of the ones running (a single call)
    let running: Vec<String> = map
        .values()
        .filter(|s| s.running())
        .map(|s| s.unit.clone())
        .collect();
    if !running.is_empty()
        && let Ok(o) = systemctl(user)
            .args(["show", "-p", "Id,MainPID,MemoryCurrent", "--no-pager"])
            .args(&running)
            .output()
    {
        for m in crate::startup::parse_show(&String::from_utf8_lossy(&o.stdout)) {
            if let Some(s) = m.get("Id").and_then(|id| map.get_mut(id)) {
                s.main_pid = m.get("MainPID").and_then(|p| p.parse().ok()).unwrap_or(0);
                s.mem = m
                    .get("MemoryCurrent")
                    .and_then(|v| v.parse::<u64>().ok())
                    .filter(|v| *v < u64::MAX / 2);
            }
        }
    }
    for s in map.values_mut() {
        if s.description.is_empty() {
            s.description = s.unit.trim_end_matches(".service").to_string();
        }
        s.protected = crate::startup::essential_unit(&s.unit).then_some(crate::i18n::tr(
            "essential for the system, the network or login",
        ));
    }
    // programs started by the desktop become transient units (app-...@hash.service): they are
    // programs, not services (they are in the Programs tab)
    let mut v: Vec<Service> = map
        .into_values()
        .filter(|s| !(s.unit.starts_with("app-") && s.unit.contains('@')))
        .collect();
    v.sort_by(|a, b| a.unit.cmp(&b.unit));
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    Start,
    Stop,
    Restart,
}

impl Verb {
    pub fn word(self) -> &'static str {
        match self {
            Verb::Start => "start",
            Verb::Stop => "stop",
            Verb::Restart => "restart",
        }
    }
    pub fn text(self) -> &'static str {
        match self {
            Verb::Start => crate::i18n::tr("Start"),
            Verb::Stop => crate::i18n::tr("Stop"),
            Verb::Restart => crate::i18n::tr("Restart"),
        }
    }
}

/// Command of the action. The user's runs directly; the system's, through pkexec (asks for the password).
pub fn action_command(s: &Service, v: Verb) -> Result<Vec<String>, String> {
    if v != Verb::Start
        && let Some(why) = s.protected
    {
        return Err(format!("{}: {why}", s.unit));
    }
    let mut c = vec![];
    if !s.user {
        c.push("pkexec".to_string());
    }
    c.push("systemctl".into());
    if s.user {
        c.push("--user".into());
    }
    c.push(v.word().into());
    c.push(s.unit.clone());
    crate::log::info(&crate::tf!(
        "service {}: {}",
        "serviço {}: {}",
        s.id(),
        v.word()
    ));
    Ok(c)
}

/// Runs a user service's action right away.
pub fn run_user(s: &Service, v: Verb) -> Result<(), String> {
    let c = action_command(s, v)?;
    let o = Command::new(&c[0])
        .args(&c[1..])
        .output()
        .map_err(|e| e.to_string())?;
    if o.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).trim().to_string())
    }
}

/// Last log (journal) lines of the service.
pub fn logs(s: &Service, lines: u32) -> String {
    unit_logs(&s.unit, s.user, lines)
}

/// Last log (journal) lines of a unit.
pub fn unit_logs(unit: &str, user: bool, lines: u32) -> String {
    let mut c = Command::new("journalctl");
    if user {
        c.arg("--user");
    }
    match c
        .args([
            "-u",
            unit,
            "-n",
            &lines.to_string(),
            "--no-pager",
            "-o",
            "short-iso",
        ])
        .output()
    {
        Ok(o) => {
            let t = String::from_utf8_lossy(&o.stdout).to_string();
            if t.trim().is_empty() || t.contains("-- No entries --") {
                let e = String::from_utf8_lossy(&o.stderr).trim().to_string();
                if e.is_empty() {
                    crate::i18n::tr("(nothing in the log)").into()
                } else {
                    crate::tf!(
                        "(no access to the log: {e}; join the wheel or systemd-journal group)",
                        "(sem acesso ao log: {e}; entre no grupo wheel ou systemd-journal)"
                    )
                }
            } else {
                t
            }
        }
        Err(e) => format!("journalctl: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_list() {
        let t = "accounts-daemon.service   loaded    active   running Accounts Service\n\
                 ● foo.service             not-found inactive dead    foo.service\n\
                 alsa-state.service        loaded    inactive dead    Manage Sound Card State\n\
                 x.socket loaded active listening X\n";
        let v = parse_list_units(t);
        assert_eq!(v.len(), 3);
        assert_eq!(
            v[0],
            (
                "accounts-daemon.service".into(),
                "active".into(),
                "running".into(),
                "Accounts Service".into()
            )
        );
        assert_eq!(v[1].0, "foo.service");
    }

    #[test]
    fn states_and_protection() {
        let mut s = Service {
            unit: "NetworkManager.service".into(),
            user: false,
            description: String::new(),
            active: "active".into(),
            sub: "running".into(),
            file_state: "enabled".into(),
            main_pid: 1,
            mem: None,
            protected: Some("x"),
        };
        assert_eq!(s.state_text(), "running");
        assert_eq!(s.at_boot(), Some(true));
        assert!(action_command(&s, Verb::Stop).is_err());
        assert_eq!(
            action_command(&s, Verb::Start).unwrap(),
            vec!["pkexec", "systemctl", "start", "NetworkManager.service"]
        );
        s.user = true;
        s.protected = None;
        s.active = "failed".into();
        assert!(s.failed() && s.state_text() == "failed");
        assert_eq!(
            action_command(&s, Verb::Restart).unwrap(),
            vec!["systemctl", "--user", "restart", "NetworkManager.service"]
        );
    }
}
