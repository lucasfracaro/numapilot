//! Protocol of the daemon's control socket (/run/numapilot/ctl.sock).
//! One JSON request line → one JSON response line.

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
    /// Saves the profile of program `key`. If `pid` is given, applies the CPU choice right away
    /// to every process of that program (same user).
    SetProfile {
        key: String,
        profile: Profile,
        pid: Option<i32>,
    },
    DeleteProfile {
        key: String,
        pid: Option<i32>,
    },
    /// `numapilot run` asks to apply to its own process what requires root.
    /// Machine settings (modes, processors, memory, scheduler).
    SetMachine {
        machine: crate::machine::Machine,
    },
    /// Phone notifications (ntfy).
    /// Stores the cache latency per core measurement (`numapilot bench-cores`).
    SetCoreRank {
        rank: crate::corebench::CoreRank,
    },
    SetNotify {
        notify: crate::notify::Notify,
    },
    /// Motherboard fan.
    SetBoardFan {
        id: String,
        control: crate::fans::BoardFanControl,
    },
    Adopt {
        pid: i32,
        /// Profile to use only on this process (FPS optimizer); None = the saved one.
        #[serde(default)]
        profile: Option<crate::profiles::Profile>,
        /// Key of the program about to be started (the process is still `numapilot run`).
        #[serde(default)]
        key: Option<String>,
        #[serde(default)]
        label: Option<String>,
    },
    /// Moves a program's memory back to the socket(s) of its cores.
    FixMemory {
        key: String,
    },
    /// Frequency governor of a socket's CPUs (performance, schedutil, powersave...).
    SetGovernor {
        node: usize,
        governor: String,
    },
    /// Fan / power of a GPU (by PCI address).
    SetGpuControl {
        pci: String,
        control: crate::gpuctl::GpuControl,
        /// Trial (automatic undervolt): applies but does not write to disk; if the PC freezes,
        /// it comes back with the last good value.
        #[serde(default)]
        trial: bool,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Response {
    pub ok: bool,
    #[serde(default)]
    pub message: String,
}

impl Response {
    pub fn ok(msg: impl Into<String>) -> Self {
        Response {
            ok: true,
            message: msg.into(),
        }
    }
    pub fn err(msg: impl Into<String>) -> Self {
        Response {
            ok: false,
            message: msg.into(),
        }
    }
}

pub fn send(req: &Request) -> Result<Response> {
    let mut s = UnixStream::connect(sock_path()).context(crate::i18n::tr(
        "daemon is not running (systemctl status numapilot)",
    ))?;
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    // the service answers in the language of whoever asks
    let mut v = serde_json::to_value(req)?;
    if let Some(o) = v.as_object_mut() {
        o.insert("lang".into(), crate::i18n::code().into());
    }
    let mut line = serde_json::to_string(&v)?;
    line.push('\n');
    s.write_all(line.as_bytes())?;
    let mut r = String::new();
    BufReader::new(s).read_line(&mut r)?;
    Ok(serde_json::from_str(r.trim())?)
}

/// UID of whoever is on the other side of the socket.
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
