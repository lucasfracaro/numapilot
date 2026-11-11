//! Leitura de /proc e chamadas de sistema de afinidade/memória.

use crate::util::{basename, parse_cpulist};
use std::collections::HashSet;
use std::fs;
use std::io;

const PF_KTHREAD: u64 = 0x0020_0000;

#[derive(Debug, Clone)]
pub struct Stat {
    pub ppid: i32,
    pub comm: String,
    pub flags: u64,
    pub cpu_ticks: u64,
    pub starttime: u64,
}

impl Stat {
    pub fn is_kthread(&self) -> bool {
        self.flags & PF_KTHREAD != 0
    }
}

pub fn list_pids() -> Vec<i32> {
    let Ok(rd) = fs::read_dir("/proc") else {
        return vec![];
    };
    rd.flatten()
        .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse().ok()))
        .collect()
}

pub fn read_stat(pid: i32) -> Option<Stat> {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_stat(&s)
}

pub fn parse_stat(s: &str) -> Option<Stat> {
    let open = s.find('(')?;
    let close = s.rfind(')')?;
    let comm = s[open + 1..close].to_string();
    let f: Vec<&str> = s[close + 1..].split_whitespace().collect();
    // f[0] é o campo 3 (state); campo N => f[N-3]
    let num = |i: usize| f.get(i).and_then(|x| x.parse::<u64>().ok());
    Some(Stat {
        ppid: f.get(1)?.parse().ok()?,
        comm,
        flags: num(6)?,
        cpu_ticks: num(11)? + num(12)?,
        starttime: num(19)?,
    })
}

pub fn status_field(pid: i32, key: &str) -> Option<String> {
    let s = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    s.lines().find_map(|l| {
        l.strip_prefix(key)
            .and_then(|r| r.strip_prefix(':'))
            .map(|r| r.trim().to_string())
    })
}

pub fn uid(pid: i32) -> Option<u32> {
    status_field(pid, "Uid")?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

pub fn allowed_cpus(pid: i32) -> Option<Vec<usize>> {
    status_field(pid, "Cpus_allowed_list").map(|s| parse_cpulist(&s))
}

pub fn exe(pid: i32) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/exe")).ok().map(|p| {
        p.to_string_lossy()
            .trim_end_matches(" (deleted)")
            .to_string()
    })
}

pub fn cmdline(pid: i32) -> Vec<String> {
    fs::read(format!("/proc/{pid}/cmdline"))
        .map(|b| {
            b.split(|&c| c == 0)
                .filter(|x| !x.is_empty())
                .map(|x| String::from_utf8_lossy(x).to_string())
                .collect()
        })
        .unwrap_or_default()
}

pub fn env_var(pid: i32, key: &str) -> Option<String> {
    let b = fs::read(format!("/proc/{pid}/environ")).ok()?;
    let prefix = format!("{key}=");
    b.split(|&c| c == 0).find_map(|kv| {
        let kv = String::from_utf8_lossy(kv);
        kv.strip_prefix(&prefix).map(|v| v.to_string())
    })
}

/// Todas as variáveis de ambiente do processo (só de processos seus, ou como root).
pub fn environ(pid: i32) -> Vec<(String, String)> {
    fs::read(format!("/proc/{pid}/environ"))
        .map(|b| {
            b.split(|&c| c == 0)
                .filter_map(|kv| {
                    let kv = String::from_utf8_lossy(kv);
                    kv.split_once('=')
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn alive(pid: i32) -> bool {
    unsafe { libc::kill(pid, 0) == 0 }
}

/// Nomes usados para casar regras: comm, nome do executável e dos primeiros argumentos.
pub fn idents(comm: &str, exe: Option<&str>, argv: &[String]) -> Vec<String> {
    let mut v = vec![comm.to_string()];
    if let Some(e) = exe {
        v.push(basename(e).to_string());
        v.push(e.to_string());
    }
    for (i, a) in argv.iter().enumerate() {
        // Os 3 primeiros argumentos e qualquer .exe (jogos via Proton/Wine ficam no fim da linha).
        if !a.starts_with('-') && (i < 3 || a.to_lowercase().ends_with(".exe")) {
            v.push(basename(a).to_string());
        }
    }
    v.dedup();
    v
}

/// Chave estável de "programa" para o aprendizado. Para interpretadores
/// (python, wine...) usa também o script/exe passado como argumento.
pub fn learn_key(exe: Option<&str>, argv: &[String], comm: &str) -> String {
    const INTERP: &[&str] = &[
        "python*",
        "wine*",
        "proton*",
        "java",
        "node",
        "bash",
        "sh",
        "dash",
        "zsh",
        "fish",
        "perl",
        "ruby",
        "ld-linux*",
        "box64",
    ];
    let Some(exe) = exe else {
        return comm.to_string();
    };
    let base = basename(exe);
    if INTERP.iter().any(|p| crate::util::glob_match(p, base)) {
        // python -m modulo / python -c "código"
        if let Some(i) = argv.iter().position(|a| a == "-m") {
            return format!(
                "{exe} -m {}",
                argv.get(i + 1).map(String::as_str).unwrap_or("")
            );
        }
        if argv.iter().any(|a| a == "-c") {
            return format!("{exe} -c [{comm}]");
        }
        let arg = argv
            .iter()
            .skip(1)
            .find(|a| !a.starts_with('-'))
            .map(|a| basename(a).to_string());
        match arg {
            Some(a) => format!("{exe} {a}"),
            None => format!("{exe} [{comm}]"),
        }
    } else {
        exe.to_string()
    }
}

/// Render nodes / cards DRM abertos pelo processo ("renderD128", "card1").
pub fn open_drm(pid: i32) -> HashSet<String> {
    let mut out = HashSet::new();
    let Ok(rd) = fs::read_dir(format!("/proc/{pid}/fd")) else {
        return out;
    };
    for e in rd.flatten() {
        if let Ok(t) = fs::read_link(e.path())
            && let Some(n) = t.to_str().and_then(|s| s.strip_prefix("/dev/dri/"))
        {
            out.insert(n.to_string());
        }
    }
    out
}

pub fn threads(pid: i32) -> Vec<i32> {
    fs::read_dir(format!("/proc/{pid}/task"))
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str()?.parse().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Ticks de CPU por CPU lógico a partir de /proc/stat: (ocupado, total)
pub fn cpu_times() -> Vec<(usize, u64, u64)> {
    // NUMAPILOT_PROC_STAT: outro arquivo no lugar do /proc/stat (testes e capturas de tela)
    let path = std::env::var("NUMAPILOT_PROC_STAT").unwrap_or_else(|_| "/proc/stat".into());
    let Ok(s) = fs::read_to_string(path) else {
        return vec![];
    };
    s.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?;
            let id: usize = name.strip_prefix("cpu")?.parse().ok()?;
            let v: Vec<u64> = it.filter_map(|x| x.parse().ok()).collect();
            if v.len() < 5 {
                return None;
            }
            let total: u64 = v.iter().take(8).sum();
            let idle = v[3] + v[4];
            Some((id, total - idle, total))
        })
        .collect()
}

pub fn clk_tck() -> f64 {
    let t = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if t > 0 { t as f64 } else { 100.0 }
}

// ---------- syscalls ----------

/// CPUs em que a thread pode rodar agora.
pub fn get_affinity(tid: i32) -> Option<Vec<usize>> {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        if libc::sched_getaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &mut set) != 0 {
            return None;
        }
        Some(
            (0..libc::CPU_SETSIZE as usize)
                .filter(|c| libc::CPU_ISSET(*c, &set))
                .collect(),
        )
    }
}

pub fn set_affinity(tid: i32, cpus: &[usize]) -> io::Result<()> {
    unsafe {
        let mut set: libc::cpu_set_t = std::mem::zeroed();
        libc::CPU_ZERO(&mut set);
        for &c in cpus {
            libc::CPU_SET(c, &mut set);
        }
        if libc::sched_setaffinity(tid, std::mem::size_of::<libc::cpu_set_t>(), &set) != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Aplica a afinidade em todas as threads do processo. Retorna quantas deram certo.
pub fn set_affinity_all(pid: i32, cpus: &[usize]) -> usize {
    let mut ok = 0;
    for tid in threads(pid) {
        if set_affinity(tid, cpus).is_ok() {
            ok += 1;
        }
    }
    ok
}

fn nodemask(nodes: &[usize]) -> Vec<libc::c_ulong> {
    let bits = libc::c_ulong::BITS as usize;
    let max = nodes.iter().copied().max().unwrap_or(0);
    let mut m = vec![0 as libc::c_ulong; max / bits + 1];
    for &n in nodes {
        m[n / bits] |= 1 << (n % bits);
    }
    m
}

pub const MPOL_DEFAULT: i32 = 0;
pub const MPOL_PREFERRED: i32 = 1;
pub const MPOL_BIND: i32 = 2;
pub const MPOL_INTERLEAVE: i32 = 3;

/// Política de memória do próprio processo (herdada pelo programa após o exec).
pub fn set_mempolicy(mode: i32, nodes: &[usize]) -> io::Result<()> {
    let r = if mode == MPOL_DEFAULT {
        unsafe {
            libc::syscall(
                libc::SYS_set_mempolicy,
                mode,
                std::ptr::null::<libc::c_ulong>(),
                0usize,
            )
        }
    } else {
        let m = nodemask(nodes);
        let maxnode = m.len() * libc::c_ulong::BITS as usize + 1;
        unsafe { libc::syscall(libc::SYS_set_mempolicy, mode, m.as_ptr(), maxnode) }
    };
    if r != 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Move as páginas do processo que estão em `from` para `to`.
pub fn migrate_pages(pid: i32, from: &[usize], to: &[usize]) -> io::Result<i64> {
    if from.is_empty() || to.is_empty() {
        return Ok(0);
    }
    let max = from.iter().chain(to).copied().max().unwrap_or(0);
    let mut f = nodemask(from);
    let mut t = nodemask(to);
    let words = max / libc::c_ulong::BITS as usize + 1;
    f.resize(words, 0);
    t.resize(words, 0);
    let maxnode = words * libc::c_ulong::BITS as usize + 1;
    let r = unsafe {
        libc::syscall(
            libc::SYS_migrate_pages,
            pid,
            maxnode,
            f.as_ptr(),
            t.as_ptr(),
        )
    };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_with_weird_comm() {
        let s = "1234 (Game (x64).exe) S 1000 1234 1234 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 30 0 987654 0 0";
        let st = parse_stat(s).unwrap();
        assert_eq!(st.comm, "Game (x64).exe");
        assert_eq!(st.ppid, 1000);
        assert_eq!(st.cpu_ticks, 300);
        assert_eq!(st.starttime, 987654);
        assert!(!st.is_kthread());
    }

    #[test]
    fn learn_keys() {
        let argv = vec![
            "/usr/bin/python3".to_string(),
            "-u".into(),
            "/opt/ComfyUI/main.py".into(),
        ];
        assert_eq!(
            learn_key(Some("/usr/bin/python3.12"), &argv, "python3"),
            "/usr/bin/python3.12 main.py"
        );
        assert_eq!(
            learn_key(Some("/usr/bin/blender"), &[], "blender"),
            "/usr/bin/blender"
        );
    }

    #[test]
    fn own_process_readable() {
        let pid = std::process::id() as i32;
        let st = read_stat(pid).unwrap();
        assert!(st.cpu_ticks < u64::MAX);
        assert!(allowed_cpus(pid).is_some_and(|c| !c.is_empty()));
        assert!(!cpu_times().is_empty());
    }

    #[test]
    fn self_affinity_and_policy() {
        let cpus = allowed_cpus(0)
            .or_else(|| allowed_cpus(std::process::id() as i32))
            .unwrap();
        // Aplicar a mesma afinidade atual nunca deve falhar.
        set_affinity(0, &cpus).unwrap();
        // Política padrão sempre pode ser aplicada.
        set_mempolicy(MPOL_DEFAULT, &[]).unwrap();
        // Preferir o nó 0 também (existe em qualquer sistema).
        set_mempolicy(MPOL_PREFERRED, &[0]).unwrap();
        set_mempolicy(MPOL_DEFAULT, &[]).unwrap();
    }
}

// ---------- prioridade ----------

/// "Nice" em todas as threads (-20 = mais prioridade, 19 = menos). Negativo exige root.
pub fn set_nice_all(pid: i32, nice: i32) -> usize {
    let mut ok = 0;
    for tid in threads(pid) {
        if unsafe { libc::setpriority(libc::PRIO_PROCESS, tid as libc::id_t, nice) } == 0 {
            ok += 1;
        }
    }
    ok
}

/// Prioridade de disco (classe "best effort", nível 0 = mais alta, 7 = mais baixa).
pub fn set_ioprio_all(pid: i32, level: i32) -> usize {
    const IOPRIO_WHO_PROCESS: i32 = 1;
    const IOPRIO_CLASS_BE: i32 = 2;
    let prio = (IOPRIO_CLASS_BE << 13) | level.clamp(0, 7);
    let mut ok = 0;
    for tid in threads(pid) {
        if unsafe { libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, tid, prio) } == 0 {
            ok += 1;
        }
    }
    ok
}

/// (nice, nível de E/S) para "low" | "normal" | "high"
pub fn priority_values(p: &str) -> (i32, i32) {
    match p {
        "high" => (-5, 0),
        "low" => (10, 7),
        _ => (0, 4),
    }
}

// ---------- onde está a memória ----------

/// Bytes de memória do processo em cada nó NUMA, a partir de /proc/PID/numa_maps.
pub fn mem_by_node(pid: i32) -> std::collections::BTreeMap<usize, u64> {
    let mut out = std::collections::BTreeMap::new();
    let Ok(s) = fs::read_to_string(format!("/proc/{pid}/numa_maps")) else {
        return out;
    };
    parse_numa_maps(&s, &mut out);
    out
}

pub fn parse_numa_maps(s: &str, out: &mut std::collections::BTreeMap<usize, u64>) {
    for line in s.lines() {
        let mut page_kb = 4u64;
        let mut counts: Vec<(usize, u64)> = Vec::new();
        for tok in line.split_whitespace() {
            if let Some(v) = tok.strip_prefix("kernelpagesize_kB=") {
                page_kb = v.parse().unwrap_or(4);
            } else if let Some(rest) = tok.strip_prefix('N')
                && let Some((n, c)) = rest.split_once('=')
                && let (Ok(n), Ok(c)) = (n.parse::<usize>(), c.parse::<u64>())
            {
                counts.push((n, c));
            }
        }
        for (n, c) in counts {
            *out.entry(n).or_insert(0) += c * page_kb * 1024;
        }
    }
}

/// Contadores do nó (numa_hit, numa_miss, local_node, other_node...).
pub fn numastat(node: usize) -> std::collections::BTreeMap<String, u64> {
    let p = crate::util::sys(&format!("devices/system/node/node{node}/numastat"));
    fs::read_to_string(p)
        .map(|s| {
            s.lines()
                .filter_map(|l| {
                    let mut it = l.split_whitespace();
                    Some((it.next()?.to_string(), it.next()?.parse().ok()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod numa_tests {
    use super::*;

    #[test]
    fn numa_maps_parsing() {
        let s = "7f00 default anon=10 dirty=10 N0=6 N1=4 kernelpagesize_kB=4\n\
                 7f10 bind:1 file=/x mapped=3 N1=3 kernelpagesize_kB=2048\n\
                 7f20 default stack anon=1\n";
        let mut m = std::collections::BTreeMap::new();
        parse_numa_maps(s, &mut m);
        assert_eq!(m.get(&0), Some(&(6 * 4096)));
        assert_eq!(m.get(&1), Some(&(4 * 4096 + 3 * 2048 * 1024)));
        assert_eq!(priority_values("high"), (-5, 0));
    }

    #[test]
    fn own_numa_maps_readable() {
        let m = mem_by_node(std::process::id() as i32);
        assert!(
            m.values().sum::<u64>() > 0,
            "deveria ler o próprio numa_maps"
        );
    }
}
