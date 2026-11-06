//! Protocolo do socket de controle do daemon (/run/numapilot/ctl.sock).
//! Uma linha JSON de pedido → uma linha JSON de resposta.

use crate::learned::run_dir;
use crate::profiles::Profile;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub fn sock_path() -> PathBuf {
    run_dir().join("ctl.sock")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    Ping,
    /// Salva o perfil do programa `key`. Se `pid` vier, aplica já a escolha de CPU
    /// em todos os processos desse programa (do mesmo usuário).
    SetProfile { key: String, profile: Profile, pid: Option<i32> },
    DeleteProfile { key: String, pid: Option<i32> },
    /// O `numapilot run` pede para aplicar no próprio processo o que exige root.
    /// Ajustes da máquina (modos, processadores, memória, escalonador).
    SetMachine { machine: crate::machine::Machine },
    /// Ventoinha da placa-mãe.
    SetBoardFan { id: String, control: crate::fans::BoardFanControl },
    Adopt {
        pid: i32,
        /// Perfil a usar só neste processo (otimizador de FPS); None = o salvo.
        #[serde(default)]
        profile: Option<crate::profiles::Profile>,
    },
    /// Move a memória de um programa de volta para o(s) socket(s) dos núcleos dele.
    FixMemory { key: String },
    /// Governador de frequência dos CPUs de um socket (performance, schedutil, powersave...).
    SetGovernor { node: usize, governor: String },
    /// Ventoinha / potência de uma GPU (por endereço PCI).
    SetGpuControl { pci: String, control: crate::gpuctl::GpuControl },
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Response {
    pub ok: bool,
    #[serde(default)]
    pub message: String,
}

impl Response {
    pub fn ok(msg: impl Into<String>) -> Self {
        Response { ok: true, message: msg.into() }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        Response { ok: false, message: msg.into() }
    }
}

pub fn send(req: &Request) -> Result<Response> {
    let mut s = UnixStream::connect(sock_path()).context("daemon não está rodando (systemctl status numapilot)")?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    s.write_all(line.as_bytes())?;
    let mut r = String::new();
    BufReader::new(s).read_line(&mut r)?;
    Ok(serde_json::from_str(r.trim())?)
}

/// UID de quem está do outro lado do socket.
pub fn peer_uid(s: &UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let r = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if r == 0 { Some(cred.uid) } else { None }
}
