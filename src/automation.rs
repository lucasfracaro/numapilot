//! Automation stack: n8n (workflows) and Nod (n8n's open-source personal AI agent,
//! https://github.com/n8n-io/nod), started only by hand from the "Lab and containers" tab or
//! `numapilot automation up`. Nothing here starts at boot: n8n runs with `--restart no` and Nod's
//! Compose project gets an override with `restart: "no"` on every service.
//!
//! n8n is one container (`numapilot-n8n`, data in the `numapilot-n8n` volume) on Docker when
//! present, else Podman. Nod is a Compose project (server, PostgreSQL, a Chromium container and a
//! firewalled workspace) built from its repository, and it needs Docker Engine 25+ with Compose v2
//! (it talks to the Docker socket to drive its browser and workspace). When both run on Docker,
//! n8n joins Nod's network as `n8n`, so Nod reaches it at `http://n8n:5678`.

use std::path::PathBuf;
use std::process::{Command, Stdio};

pub const N8N_NAME: &str = "numapilot-n8n";
pub const N8N_VOLUME: &str = "numapilot-n8n";
pub const N8N_IMAGE: &str = "docker.n8n.io/n8nio/n8n:latest";
pub const N8N_PORT: u16 = 5678;
pub const NOD_REPO: &str = "https://github.com/n8n-io/nod.git";
pub const NOD_PORT: u16 = 4310;
/// Name of the override written next to Nod's compose.yaml.
const OVERRIDE: &str = "numapilot.override.yaml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Not created / not installed yet.
    Missing,
    Stopped,
    Running,
    /// The engine it needs is missing (Nod without Docker, n8n without any engine).
    NoEngine,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub n8n: State,
    pub nod: State,
    /// Engine n8n runs on ("docker" or "podman").
    pub engine: Option<String>,
    pub nod_port: u16,
    /// Why Nod cannot run here (no Docker, Docker without permission...).
    pub nod_note: Option<String>,
}

pub fn n8n_url() -> String {
    format!("http://localhost:{N8N_PORT}")
}

pub fn nod_url(port: u16) -> String {
    format!("http://localhost:{port}")
}

/// Nod's clone, next to the other community apps.
pub fn nod_dir() -> PathBuf {
    crate::labcfg::home().join(".local/share/numapilot/comunidade/nod")
}

pub fn nod_compose_dir() -> PathBuf {
    nod_dir().join("deployment/self-host")
}

/// Value of `key` in a .env file (no shell expansion; quotes removed).
pub fn env_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.trim().split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches(['"', '\'']).to_string())
    })
}

fn nod_env() -> String {
    std::fs::read_to_string(nod_compose_dir().join(".env")).unwrap_or_default()
}

/// Access token of the Nod server (asked on its login page).
pub fn nod_token() -> Option<String> {
    env_value(&nod_env(), "NOD_AUTH_TOKEN").filter(|t| !t.is_empty())
}

pub fn nod_port() -> u16 {
    env_value(&nod_env(), "PORT")
        .and_then(|p| p.parse().ok())
        .unwrap_or(NOD_PORT)
}

fn nod_project() -> Option<String> {
    env_value(&nod_env(), "COMPOSE_PROJECT_NAME").filter(|p| !p.is_empty())
}

/// Compose override that keeps every service from starting with Docker at boot.
pub fn override_yaml(services: &[String]) -> String {
    let mut s = String::from(
        "# Written by numapilot: Nod only starts when you turn it on (no restart at boot).\nservices:\n",
    );
    for name in services {
        s.push_str(&format!("  {name}:\n    restart: \"no\"\n"));
    }
    s
}

/// Host time zone (from /etc/localtime), for n8n's schedules.
fn timezone() -> Option<String> {
    let t = std::fs::read_link("/etc/localtime").ok()?;
    let t = t.to_string_lossy();
    t.split_once("zoneinfo/").map(|(_, z)| z.to_string())
}

/// Arguments of `ENGINE run` that create the n8n container.
pub fn n8n_run_args(engine: &str, tz: Option<&str>) -> Vec<String> {
    let mut v: Vec<String> = [
        "run",
        "-d",
        "--name",
        N8N_NAME,
        "--restart",
        "no",
        "-p",
        &format!("127.0.0.1:{N8N_PORT}:5678"),
        "-v",
        &format!("{N8N_VOLUME}:/home/node/.n8n"),
        "-e",
        "N8N_RUNNERS_ENABLED=true",
        "-e",
        "N8N_DIAGNOSTICS_ENABLED=false",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(tz) = tz {
        v.extend(["-e".into(), format!("GENERIC_TIMEZONE={tz}")]);
        v.extend(["-e".into(), format!("TZ={tz}")]);
    }
    // Lets n8n reach services on the PC, such as the AI server (llama.cpp):
    // Podman already provides host.containers.internal.
    if !engine.ends_with("podman") {
        v.extend([
            "--add-host".into(),
            "host.docker.internal:host-gateway".into(),
        ]);
    }
    v.push(N8N_IMAGE.into());
    v
}

/// Address of the PC seen from inside the n8n container (for the AI server's URL in n8n).
pub fn host_from_container(engine: &str) -> &'static str {
    if engine.ends_with("podman") {
        "host.containers.internal"
    } else {
        "host.docker.internal"
    }
}

fn quiet(cmd: &mut Command) -> Option<std::process::Output> {
    cmd.stdin(Stdio::null()).output().ok()
}

/// Docker usable by this user: Some(Ok) usable, Some(Err(why)) installed but not usable.
fn docker_check() -> Option<Result<(), String>> {
    if !crate::containers::which("docker") {
        return None;
    }
    let out =
        quiet(Command::new("docker").args(["version", "--format", "{{.Server.APIVersion}}"]))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
        return Some(Err(
            if err.contains("permission denied") && in_docker_group() {
                crate::i18n::tr("you are already in the docker group: log out and back in (or restart) for it to apply").into()
            } else if err.contains("permission denied") {
                crate::i18n::tr(
                "Docker is installed but this user cannot use it: sudo usermod -aG docker $USER, then log in again",
            )
            .into()
            } else if err.contains("podman") {
                crate::i18n::tr(
                    "the docker command here is Podman's; Nod needs Docker Engine 25 or newer",
                )
                .into()
            } else {
                crate::i18n::tr("the Docker service is not running: sudo systemctl start docker")
                    .into()
            },
        ));
    }
    let api = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let minor = api.strip_prefix("1.").and_then(|m| m.parse::<u32>().ok());
    if minor.is_some_and(|m| m < 44) {
        return Some(Err(
            crate::i18n::tr("Nod needs Docker Engine 25 or newer").into()
        ));
    }
    if !quiet(Command::new("docker").args(["compose", "version"]))
        .is_some_and(|o| o.status.success())
    {
        return Some(Err(crate::i18n::tr(
            "Docker Compose v2 is missing: sudo pacman -S docker-compose",
        )
        .into()));
    }
    Some(Ok(()))
}

fn user_name() -> String {
    std::env::var("USER")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| {
            quiet(Command::new("id").arg("-un"))
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default()
        })
}

/// The user is listed in /etc/group's docker line (maybe not yet in this login session).
fn in_docker_group() -> bool {
    let user = user_name();
    std::fs::read_to_string("/etc/group").is_ok_and(|g| group_has(&g, "docker", &user))
}

pub fn group_has(etc_group: &str, group: &str, user: &str) -> bool {
    etc_group.lines().any(|l| {
        let f: Vec<&str> = l.split(':').collect();
        f.len() >= 4 && f[0] == group && f[3].split(',').any(|m| m.trim() == user)
    })
}

/// Script that installs Docker on Arch/CachyOS (run in a terminal: it asks for the password).
/// Socket activation: Docker only starts when something uses it, and n8n/Nod never restart at
/// boot, so nothing runs until the "Turn on" button.
pub fn docker_install_script(user: &str) -> String {
    let user = user.replace(
        |c: char| !(c.is_ascii_alphanumeric() || "._-".contains(c)),
        "",
    );
    format!(
        "set -e\n\
         command -v pacman >/dev/null || {{ echo 'Only Arch-based systems (pacman) are supported here: install Docker Engine 25+ with Compose v2 by hand.'; exit 1; }}\n\
         echo '== Installing docker, docker-compose and docker-buildx'\n\
         sudo pacman -S --needed docker docker-compose docker-buildx\n\
         echo '== Turning on the Docker socket (Docker starts only when used)'\n\
         sudo systemctl enable --now docker.socket\n\
         echo '== Adding {user} to the docker group'\n\
         sudo usermod -aG docker {user}\n\
         echo\n\
         echo 'Docker is installed. Log out and back in (or restart) so the docker group applies, then use Turn on in the Lab and containers tab.'\n"
    )
}

/// Installs Docker (asks for the sudo password in this terminal).
pub fn install_docker() -> Result<(), String> {
    let st = Command::new("sh")
        .args(["-c", &docker_install_script(&user_name())])
        .status()
        .map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err(crate::i18n::tr("the Docker installation failed (see the messages above)").into())
    }
}

/// Docker is missing or not usable and the install script can fix it.
pub fn docker_installable(st: &Status) -> bool {
    st.nod == State::NoEngine
        && (!crate::containers::which("docker") || st.nod_note.is_some())
        && crate::containers::which("pacman")
}

/// Engine for n8n: Docker when usable (so it can share Nod's network), else the tab's engine.
fn n8n_engine(docker_ok: bool) -> Option<String> {
    if docker_ok {
        Some("docker".into())
    } else {
        crate::containers::engine()
    }
}

fn container_state(engine: &str, name: &str) -> State {
    match quiet(Command::new(engine).args(["inspect", "-f", "{{.State.Running}}", name])) {
        Some(o) if o.status.success() => {
            if String::from_utf8_lossy(&o.stdout).trim() == "true" {
                State::Running
            } else {
                State::Stopped
            }
        }
        _ => State::Missing,
    }
}

fn compose(args: &[&str]) -> Command {
    let mut c = Command::new("docker");
    c.current_dir(nod_compose_dir())
        .args(["compose", "--file", "compose.yaml"]);
    if nod_compose_dir().join(OVERRIDE).is_file() {
        c.args(["--file", OVERRIDE]);
    }
    c.args(["--env-file", ".env"]).args(args);
    c
}

pub fn status() -> Status {
    let docker = docker_check();
    let docker_ok = matches!(docker, Some(Ok(())));
    let engine = n8n_engine(docker_ok);
    let n8n = match &engine {
        Some(e) => container_state(e, N8N_NAME),
        None => State::NoEngine,
    };
    let (nod, nod_note) = match docker {
        None => (
            State::NoEngine,
            Some(crate::i18n::tr("Nod needs Docker: sudo pacman -S docker docker-compose").into()),
        ),
        Some(Err(e)) => (State::NoEngine, Some(e)),
        Some(Ok(())) if !nod_compose_dir().join(".env").is_file() => (State::Missing, None),
        Some(Ok(())) => {
            let running = quiet(&mut compose(&[
                "ps", "--status", "running", "--quiet", "server",
            ]))
            .is_some_and(|o| o.status.success() && !o.stdout.trim_ascii().is_empty());
            (
                if running {
                    State::Running
                } else {
                    State::Stopped
                },
                None,
            )
        }
    };
    Status {
        n8n,
        nod,
        engine,
        nod_port: nod_port(),
        nod_note,
    }
}

fn run(log: &mut dyn FnMut(&str), cmd: &mut Command) -> Result<(), String> {
    let shown = format!("{:?}", cmd).replace('"', "");
    log(&format!("$ {shown}"));
    let st = cmd
        .stdin(Stdio::null())
        .status()
        .map_err(|e| format!("{shown}: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(crate::tf!("failed: {}", "falhou: {}", shown))
    }
}

fn n8n_up(engine: &str, log: &mut dyn FnMut(&str)) -> Result<(), String> {
    match container_state(engine, N8N_NAME) {
        State::Running => Ok(()),
        State::Stopped => run(log, Command::new(engine).args(["start", N8N_NAME])),
        _ => {
            log(&crate::i18n::tr(
                "Creating the n8n container (downloads the image the first time)",
            ));
            run(
                log,
                Command::new(engine).args(n8n_run_args(engine, timezone().as_deref())),
            )
        }
    }
}

/// Writes the override with every service of Nod's compose.yaml.
fn write_override() -> Result<(), String> {
    let out = Command::new("docker")
        .current_dir(nod_compose_dir())
        .args([
            "compose",
            "--file",
            "compose.yaml",
            "--env-file",
            ".env",
            "config",
            "--services",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let services: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    std::fs::write(nod_compose_dir().join(OVERRIDE), override_yaml(&services))
        .map_err(|e| e.to_string())
}

fn nod_install(log: &mut dyn FnMut(&str)) -> Result<(), String> {
    if !nod_dir().join(".git").is_dir() {
        log(&crate::i18n::tr("Downloading Nod (github.com/n8n-io/nod)"));
        if let Some(p) = nod_dir().parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        run(
            log,
            Command::new("git")
                .args(["clone", "--depth", "1", NOD_REPO])
                .arg(nod_dir()),
        )?;
    }
    if !nod_compose_dir().join(".env").is_file() {
        // Creates .env with the access token, the vault key and the database password.
        run(
            log,
            Command::new("sh")
                .current_dir(nod_compose_dir())
                .arg("setup.sh"),
        )?;
    }
    write_override()
}

/// Puts n8n on Nod's network as "n8n" (already there = fine).
fn link_n8n(log: &mut dyn FnMut(&str)) {
    let Some(project) = nod_project() else { return };
    let net = format!("{project}_outbound");
    let out = quiet(
        Command::new("docker").args(["network", "connect", "--alias", "n8n", &net, N8N_NAME]),
    );
    match out {
        Some(o) if o.status.success() => log(&crate::i18n::tr(
            "n8n joined Nod's network: in Nod, n8n is at http://n8n:5678",
        )),
        Some(o) if String::from_utf8_lossy(&o.stderr).contains("already exists") => {}
        Some(o) => log(&crate::tf!(
            "could not put n8n on Nod's network: {}",
            "não deu para pôr o n8n na rede do Nod: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        None => {}
    }
}

/// Turns both on (creates/installs what is missing). Nod is skipped without Docker.
pub fn up(log: &mut dyn FnMut(&str), rebuild: bool) -> Result<(), String> {
    let docker = docker_check();
    let docker_ok = matches!(docker, Some(Ok(())));
    let engine = n8n_engine(docker_ok).ok_or(crate::i18n::tr("no podman/docker"))?;
    n8n_up(&engine, log)?;
    match docker {
        Some(Ok(())) => {
            nod_install(log)?;
            log(&crate::i18n::tr(
                "Starting Nod (the first time it builds its images: several minutes)",
            ));
            let mut args = vec!["up", "-d", "--wait", "--wait-timeout", "900"];
            if rebuild {
                args.push("--build");
            }
            run(log, &mut compose(&args))?;
            link_n8n(log);
        }
        Some(Err(e)) => log(&crate::tf!("Nod skipped: {e}", "Nod não foi ligado: {e}")),
        None => log(&crate::i18n::tr(
            "Nod skipped: it needs Docker (sudo pacman -S docker docker-compose)",
        )),
    }
    log(&format!("n8n: {}", n8n_url()));
    if docker_ok {
        log(&format!("Nod: {}", nod_url(nod_port())));
    }
    Ok(())
}

pub fn down(log: &mut dyn FnMut(&str)) -> Result<(), String> {
    let docker_ok = matches!(docker_check(), Some(Ok(())));
    let mut res = Ok(());
    if let Some(e) = n8n_engine(docker_ok)
        && container_state(&e, N8N_NAME) == State::Running
    {
        res = run(log, Command::new(&e).args(["stop", N8N_NAME]));
    }
    if docker_ok && nod_compose_dir().join(".env").is_file() {
        let r = run(log, &mut compose(&["stop"]));
        res = res.and(r);
    }
    res
}

/// New n8n image and Nod's latest code, then turns both on again.
pub fn update(log: &mut dyn FnMut(&str)) -> Result<(), String> {
    let docker_ok = matches!(docker_check(), Some(Ok(())));
    let engine = n8n_engine(docker_ok).ok_or(crate::i18n::tr("no podman/docker"))?;
    run(log, Command::new(&engine).args(["pull", N8N_IMAGE]))?;
    if container_state(&engine, N8N_NAME) != State::Missing {
        // The data lives in the volume: recreating keeps the workflows.
        let _ = run(log, Command::new(&engine).args(["rm", "-f", N8N_NAME]));
    }
    if docker_ok && nod_dir().join(".git").is_dir() {
        run(
            log,
            Command::new("git")
                .current_dir(nod_dir())
                .args(["pull", "--ff-only"]),
        )?;
    }
    up(log, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docker_group_membership() {
        let g = "wheel:x:998:lucas\ndocker:x:970:ana,lucas\n";
        assert!(group_has(g, "docker", "lucas"));
        assert!(!group_has(g, "docker", "bob"));
        assert!(!group_has("docker:x:970:\n", "docker", "lucas"));
    }

    #[test]
    fn docker_script_installs_compose_and_adds_the_user() {
        let s = docker_install_script("lucas; rm -rf /");
        assert!(s.contains("pacman -S --needed docker docker-compose docker-buildx"));
        assert!(s.contains("systemctl enable --now docker.socket"));
        assert!(s.contains("usermod -aG docker lucasrm-rf"));
    }

    #[test]
    fn reads_env_values() {
        let t = "COMPOSE_PROJECT_NAME=nod-ab12\nPORT=4310\nNOD_AUTH_TOKEN=\"abc\"\n# x=1\n";
        assert_eq!(env_value(t, "NOD_AUTH_TOKEN").as_deref(), Some("abc"));
        assert_eq!(
            env_value(t, "COMPOSE_PROJECT_NAME").as_deref(),
            Some("nod-ab12")
        );
        assert_eq!(env_value(t, "MISSING"), None);
    }

    #[test]
    fn override_turns_off_every_restart() {
        let y = override_yaml(&["server".into(), "postgres".into()]);
        assert!(y.contains("  server:\n    restart: \"no\"\n"));
        assert!(y.contains("  postgres:\n    restart: \"no\"\n"));
    }

    #[test]
    fn n8n_never_restarts_and_only_listens_locally() {
        let a = n8n_run_args("docker", Some("America/Sao_Paulo"));
        let s = a.join(" ");
        assert!(s.contains("--restart no"));
        assert!(s.contains("-p 127.0.0.1:5678:5678"));
        assert!(s.contains("GENERIC_TIMEZONE=America/Sao_Paulo"));
        assert!(s.contains("host.docker.internal:host-gateway"));
        assert_eq!(a.last().map(String::as_str), Some(N8N_IMAGE));
        assert!(
            !n8n_run_args("podman", None)
                .join(" ")
                .contains("--add-host")
        );
    }
}
