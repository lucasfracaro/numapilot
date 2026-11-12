//! Serviços do systemd (do sistema e do usuário), como a aba Serviços de um gerenciador de
//! tarefas: estado, se abre no boot, memória, e iniciar/parar/reiniciar e ver o log.
//! Serviços do sistema precisam de root: as ações viram um comando `pkexec systemctl ...`.

use std::collections::BTreeMap;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct Service {
    pub unit: String,
    /// Do usuário (systemctl --user) ou do sistema.
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
    /// Por que não deixar parar (None = pode).
    pub protected: Option<&'static str>,
}

impl Service {
    pub fn running(&self) -> bool {
        self.active == "active" || self.active == "activating" || self.active == "reloading"
    }
    pub fn failed(&self) -> bool {
        self.active == "failed"
    }
    /// "rodando", "parado", "com falha"...
    pub fn state_text(&self) -> &'static str {
        match (self.active.as_str(), self.sub.as_str()) {
            ("active", "running") => crate::i18n::tr("rodando"),
            ("active", "exited") => crate::i18n::tr("terminou (ok)"),
            ("active", "waiting") | ("active", "listening") => crate::i18n::tr("esperando"),
            ("active", _) => crate::i18n::tr("ativo"),
            ("activating", _) => crate::i18n::tr("iniciando"),
            ("deactivating", _) => crate::i18n::tr("parando"),
            ("reloading", _) => crate::i18n::tr("recarregando"),
            ("failed", _) => crate::i18n::tr("com falha"),
            _ => crate::i18n::tr("parado"),
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

/// Linhas do `systemctl list-units --plain --no-legend`: unidade, load, active, sub, descrição.
pub fn parse_list_units(text: &str) -> Vec<(String, String, String, String)> {
    text.lines()
        .filter_map(|l| {
            // falha ou não encontrado vem com "●" na frente
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

/// Todos os serviços (carregados e instalados), do sistema ou do usuário.
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
    // instalados que nunca foram carregados (desligados e parados)
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
    // memória e PID de quem está rodando (uma chamada só)
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
            "essencial para o sistema, a rede ou o login",
        ));
    }
    // programas abertos pelo desktop viram unidades temporárias (app-...@hash.service): são
    // programas, não serviços (estão na aba Programas)
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
            Verb::Start => crate::i18n::tr("Iniciar"),
            Verb::Stop => crate::i18n::tr("Parar"),
            Verb::Restart => crate::i18n::tr("Reiniciar"),
        }
    }
}

/// Comando da ação. Do usuário roda direto; do sistema, pelo pkexec (pede a senha).
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
        "serviço {}: {}",
        "service {}: {}",
        s.id(),
        v.word()
    ));
    Ok(c)
}

/// Roda a ação de um serviço do usuário na hora.
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

/// Últimas linhas do log (journal) do serviço.
pub fn logs(s: &Service, lines: u32) -> String {
    unit_logs(&s.unit, s.user, lines)
}

/// Últimas linhas do log (journal) de uma unidade.
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
                    crate::i18n::tr("(nada no log)").into()
                } else {
                    crate::tf!(
                        "(sem acesso ao log: {e}; entre no grupo wheel ou systemd-journal)",
                        "(no access to the log: {e}; join the wheel or systemd-journal group)"
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
    fn lista_de_unidades() {
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
    fn estados_e_protecao() {
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
        assert_eq!(s.state_text(), "rodando");
        assert_eq!(s.at_boot(), Some(true));
        assert!(action_command(&s, Verb::Stop).is_err());
        assert_eq!(
            action_command(&s, Verb::Start).unwrap(),
            vec!["pkexec", "systemctl", "start", "NetworkManager.service"]
        );
        s.user = true;
        s.protected = None;
        s.active = "failed".into();
        assert!(s.failed() && s.state_text() == "com falha");
        assert_eq!(
            action_command(&s, Verb::Restart).unwrap(),
            vec!["systemctl", "--user", "restart", "NetworkManager.service"]
        );
    }
}
