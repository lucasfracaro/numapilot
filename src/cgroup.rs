//! Control groups (cgroups v2) through systemd: one "scope" per program in numapilot.slice,
//! with a memory limit, memory only from certain nodes and exclusive cores.
//! Uses systemd itself (busctl/systemctl) so as not to fight it over the cgroup tree.

use crate::util::format_cpulist;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::process::Command;

pub const SLICE: &str = "numapilot.slice";

pub fn scope_name(key: &str) -> String {
    let mut h = DefaultHasher::new();
    key.hash(&mut h);
    format!("numapilot-{:016x}.scope", h.finish())
}

/// argv to create the scope already with the PIDs.
pub fn start_scope_argv(name: &str, pids: &[i32], description: &str) -> Vec<String> {
    let mut v: Vec<String> = [
        "busctl",
        "call",
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "StartTransientUnit",
        "ssa(sv)a(sa(sv))",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.push(name.into());
    v.push("fail".into());
    v.push("3".into());
    v.extend(["PIDs".into(), "au".into(), pids.len().to_string()]);
    v.extend(pids.iter().map(|p| p.to_string()));
    v.extend(["Slice".into(), "s".into(), SLICE.into()]);
    v.extend([
        "Description".into(),
        "s".into(),
        format!("numapilot: {description}"),
    ]);
    v.push("0".into());
    v
}

/// argv to put more PIDs into a scope that already exists.
pub fn attach_argv(name: &str, pids: &[i32]) -> Vec<String> {
    let mut v: Vec<String> = [
        "busctl",
        "call",
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
        "AttachProcessesToUnit",
        "ssau",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.push(name.into());
    v.push(String::new());
    v.push(pids.len().to_string());
    v.extend(pids.iter().map(|p| p.to_string()));
    v
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Limits {
    pub cpus: Option<Vec<usize>>,
    pub mem_nodes: Option<Vec<usize>>,
    pub mem_max_bytes: Option<u64>,
    /// CPU limit in % of one core (systemd's CPUQuota).
    pub cpu_quota_pct: Option<u32>,
}

/// argv of `systemctl set-property --runtime` with the limits (empty = no limit).
pub fn set_props_argv(unit: &str, l: &Limits) -> Vec<String> {
    let nodes = |v: &Option<Vec<usize>>| v.as_ref().map(|x| format_cpulist(x)).unwrap_or_default();
    vec![
        "systemctl".into(),
        "set-property".into(),
        "--runtime".into(),
        unit.into(),
        format!("AllowedCPUs={}", nodes(&l.cpus)),
        format!("AllowedMemoryNodes={}", nodes(&l.mem_nodes)),
        format!(
            "MemoryMax={}",
            l.mem_max_bytes
                .map(|b| b.to_string())
                .unwrap_or_else(|| "infinity".into())
        ),
        // empty = no limit
        format!(
            "CPUQuota={}",
            l.cpu_quota_pct.map(|p| format!("{p}%")).unwrap_or_default()
        ),
    ]
}

fn run(argv: &[String]) -> Result<(), String> {
    let out = Command::new(&argv[0])
        .args(&argv[1..])
        .output()
        .map_err(|e| format!("{}: {e}", argv[0]))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn unit_active(unit: &str) -> bool {
    Command::new("systemctl")
        .args(["show", "-p", "ActiveState", "--value", unit])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
        .unwrap_or(false)
}

/// Makes sure the PIDs are in the program's scope and applies the limits.
pub fn apply(key: &str, label: &str, pids: &[i32], limits: &Limits) -> Result<String, String> {
    let name = scope_name(key);
    if !unit_active(&name) {
        run(&start_scope_argv(&name, pids, label))?;
        run(&set_props_argv(&name, limits))?;
        return Ok(name);
    }
    // PIDs not yet in any scope of this program
    let todo: Vec<i32> = pids
        .iter()
        .copied()
        .filter(|p| !in_scope_of(*p, &name))
        .collect();
    let mut out = name.clone();
    if !todo.is_empty() && run(&attach_argv(&name, &todo)).is_err() {
        // systemd only lets processes join a "delegated" scope. Instead, the reopened program
        // gets a scope of its own (same name + PID) with the same limits.
        let extra = format!("{}-{}.scope", name.trim_end_matches(".scope"), todo[0]);
        run(&start_scope_argv(&extra, &todo, label))?;
        run(&set_props_argv(&extra, limits))?;
        out = extra;
    }
    for u in scopes_of(&name) {
        run(&set_props_argv(&u, limits))?;
    }
    Ok(out)
}

/// Is the process already in a scope of this program (the main one or an extra)?
fn in_scope_of(pid: i32, name: &str) -> bool {
    let base = name.trim_end_matches(".scope");
    std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
        .is_ok_and(|c| c.lines().any(|l| l.contains(&format!("/{base}"))))
}

/// Active scopes of this program: the main one and the extras (name-PID.scope).
fn scopes_of(name: &str) -> Vec<String> {
    let base = name.trim_end_matches(".scope");
    let o = Command::new("systemctl")
        .args(["list-units", "--plain", "--no-legend", "--state=active"])
        .arg(format!("{base}*.scope"))
        .output();
    let mut v: Vec<String> = o
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter_map(|l| l.split_whitespace().next().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if v.is_empty() && unit_active(name) {
        v.push(name.to_string());
    }
    v
}

/// Removes a program's limits (the scopes go away by themselves when the processes end).
pub fn clear(key: &str) {
    for u in scopes_of(&scope_name(key)) {
        let _ = run(&set_props_argv(&u, &Limits::default()));
    }
}

/// Exclusive cores: the rest of the system (user.slice and system.slice) stays off `reserved`.
pub fn set_reserved(all: &[usize], reserved: &[usize]) -> Result<(), String> {
    let rest: Vec<usize> = all
        .iter()
        .copied()
        .filter(|c| !reserved.contains(c))
        .collect();
    let allowed = if reserved.is_empty() || rest.is_empty() {
        String::new()
    } else {
        format_cpulist(&rest)
    };
    for slice in ["user.slice", "system.slice"] {
        run(&[
            "systemctl".into(),
            "set-property".into(),
            "--runtime".into(),
            slice.into(),
            format!("AllowedCPUs={allowed}"),
        ])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_shapes() {
        let a = start_scope_argv("numapilot-x.scope", &[10, 20], "Jogo");
        let tail: Vec<&str> = a.iter().skip(7).map(String::as_str).collect();
        assert_eq!(
            tail,
            vec![
                "numapilot-x.scope",
                "fail",
                "3",
                "PIDs",
                "au",
                "2",
                "10",
                "20",
                "Slice",
                "s",
                "numapilot.slice",
                "Description",
                "s",
                "numapilot: Jogo",
                "0"
            ]
        );
        let p = set_props_argv(
            "u",
            &Limits {
                cpus: Some(vec![0, 1, 2, 44]),
                mem_nodes: Some(vec![0]),
                mem_max_bytes: Some(1 << 30),
                cpu_quota_pct: Some(150),
            },
        );
        assert_eq!(
            &p[4..],
            &[
                "AllowedCPUs=0-2,44",
                "AllowedMemoryNodes=0",
                "MemoryMax=1073741824",
                "CPUQuota=150%"
            ]
        );
        let p = set_props_argv("u", &Limits::default());
        assert_eq!(
            &p[4..],
            &[
                "AllowedCPUs=",
                "AllowedMemoryNodes=",
                "MemoryMax=infinity",
                "CPUQuota="
            ]
        );
        assert_eq!(attach_argv("u", &[5])[7..], ["u", "", "1", "5"]);
        assert!(
            scope_name("steam:1").starts_with("numapilot-")
                && scope_name("steam:1") != scope_name("steam:2")
        );
    }
}
