//! Funções auxiliares: caminhos do sysfs, listas de CPU, glob.

use std::fs;
use std::path::{Path, PathBuf};

/// Raiz do sysfs. Pode ser trocada com NUMAPILOT_SYSFS (usado nos testes).
pub fn sys_root() -> PathBuf {
    std::env::var_os("NUMAPILOT_SYSFS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/sys"))
}

/// Testes que trocam o NUMAPILOT_SYSFS rodam um de cada vez.
#[cfg(test)]
static SYSFS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub fn test_sysfs_lock(root: &Path) -> std::sync::MutexGuard<'static, ()> {
    let g = SYSFS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe { std::env::set_var("NUMAPILOT_SYSFS", root) };
    g
}

pub fn sys(rel: &str) -> PathBuf {
    sys_root().join(rel.trim_start_matches('/'))
}

pub fn read_trim(p: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(p).ok().map(|s| s.trim().to_string())
}

pub fn read_u64(p: impl AsRef<Path>) -> Option<u64> {
    read_trim(p)?.parse().ok()
}

/// "0-21,44-65" -> [0,1,...,21,44,...,65]
pub fn parse_cpulist(s: &str) -> Vec<usize> {
    let mut v = Vec::new();
    for part in s.trim().split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>())
                && a <= b {
                    v.extend(a..=b);
                }
        } else if let Ok(a) = part.parse() {
            v.push(a);
        }
    }
    v.sort_unstable();
    v.dedup();
    v
}

/// [0,1,2,5] -> "0-2,5"
pub fn format_cpulist(cpus: &[usize]) -> String {
    let mut c: Vec<usize> = cpus.to_vec();
    c.sort_unstable();
    c.dedup();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let start = c[i];
        let mut end = start;
        while i + 1 < c.len() && c[i + 1] == end + 1 {
            i += 1;
            end = c[i];
        }
        if start == end {
            out.push(start.to_string());
        } else {
            out.push(format!("{start}-{end}"));
        }
        i += 1;
    }
    out.join(",")
}

/// Glob simples, sem diferenciar maiúsculas: `*` e `?`.
pub fn glob_match(pat: &str, s: &str) -> bool {
    let p: Vec<char> = pat.to_lowercase().chars().collect();
    let t: Vec<char> = s.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// Último componente de um caminho, aceitando "/" e "\" (caminhos do Wine).
pub fn basename(s: &str) -> &str {
    s.rsplit(['/', '\\']).next().unwrap_or(s)
}

pub fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpulist_roundtrip() {
        let v = parse_cpulist("0-21,44-65");
        assert_eq!(v.len(), 44);
        assert_eq!(format_cpulist(&v), "0-21,44-65");
        assert_eq!(parse_cpulist("3"), vec![3]);
        assert_eq!(format_cpulist(&[5, 1, 2, 3]), "1-3,5");
        assert!(parse_cpulist("").is_empty());
    }

    #[test]
    fn globs() {
        assert!(glob_match("*.exe", "Cyberpunk2077.exe"));
        assert!(glob_match("llama-*", "llama-server"));
        assert!(glob_match("steam", "STEAM"));
        assert!(!glob_match("steam", "steamwebhelper"));
        assert!(glob_match("wine?4*", "wine64-preloader"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn basenames() {
        assert_eq!(basename("/usr/bin/blender"), "blender");
        assert_eq!(basename("Z:\\games\\Game.exe"), "Game.exe");
        assert_eq!(basename("steam"), "steam");
    }
}
