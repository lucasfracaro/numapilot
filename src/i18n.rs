//! Message language. English is the language of the code; Portuguese is the translation.
//! Fixed text: `tr("text")` with its pair in `i18n_pt.rs`; text with values:
//! `tf!("text {}", "texto {}", value)`; two Portuguese forms for the same English text:
//! `t2!("on", "ligada")`. Text without a pair is shown in English.
//!
//! Where the language comes from: `NUMAPILOT_LANG` (en/pt), then `~/.config/numapilot/lang` (the
//! choice made in the interface), then, for root, the last language asked of the service
//! (`/var/lib/numapilot/lang`), and finally the system locale (Portuguese only for `pt_*`).
//! The service answers in the language of whoever asks: every request carries `"lang"`.

use std::sync::atomic::{AtomicU8, Ordering};

#[path = "i18n_pt.rs"]
mod pt;

/// 0 = not decided yet, 1 = English, 2 = Portuguese.
static LANG: AtomicU8 = AtomicU8::new(0);

/// `format!` in both languages: `tf!("text {}", "texto {}", value)`.
#[macro_export]
macro_rules! tf {
    ($en:literal, $pt:literal $(, $($arg:tt)*)?) => {
        if $crate::i18n::portuguese() {
            format!($pt $(, $($arg)*)?)
        } else {
            format!($en $(, $($arg)*)?)
        }
    };
}

/// Fixed text with its Portuguese form inline: `t2!("on", "ligada")`.
#[macro_export]
macro_rules! t2 {
    ($en:literal, $pt:literal) => {
        if $crate::i18n::portuguese() { $pt } else { $en }
    };
}

pub fn user_file() -> std::path::PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        })
        .join("numapilot/lang")
}

fn service_file() -> std::path::PathBuf {
    crate::learned::state_dir().join("lang")
}

/// `Some(true)` = Portuguese.
fn parse(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() {
        "pt" | "pt_br" | "pt-br" | "portugues" | "português" | "portuguese" => Some(true),
        "en" | "english" => Some(false),
        _ => None,
    }
}

fn detect() -> bool {
    if let Some(v) = std::env::var("NUMAPILOT_LANG").ok().and_then(|v| parse(&v)) {
        return v;
    }
    if let Some(v) = std::fs::read_to_string(user_file())
        .ok()
        .and_then(|v| parse(&v))
    {
        return v;
    }
    if unsafe { libc::geteuid() } == 0
        && let Some(v) = std::fs::read_to_string(service_file())
            .ok()
            .and_then(|v| parse(&v))
    {
        return v;
    }
    let l = std::env::var("LC_ALL")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("LC_MESSAGES").ok().filter(|v| !v.is_empty()))
        .or_else(|| std::env::var("LANG").ok())
        .unwrap_or_default();
    l.starts_with("pt")
}

/// Is the language in use Portuguese? (Always English in the tests.)
pub fn portuguese() -> bool {
    if cfg!(test) {
        return false;
    }
    match LANG.load(Ordering::Relaxed) {
        0 => {
            let pt = detect();
            LANG.store(if pt { 2 } else { 1 }, Ordering::Relaxed);
            pt
        }
        v => v == 2,
    }
}

/// Decides the language again (long-lived processes, like the web panel, follow a change made
/// in the interface).
pub fn refresh() {
    LANG.store(0, Ordering::Relaxed);
}

/// Changes the language of this process.
pub fn set_portuguese(pt: bool) {
    LANG.store(if pt { 2 } else { 1 }, Ordering::Relaxed);
}

/// Changes the language and stores the user's choice (used by the interface).
pub fn save_user_choice(pt: bool) {
    set_portuguese(pt);
    let f = user_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, code());
}

pub fn code() -> &'static str {
    if portuguese() { "pt" } else { "en" }
}

/// In the service: start answering (and publishing state) in the language of the request.
pub fn follow_client(request_json: &str) {
    let Some(pt) = serde_json::from_str::<serde_json::Value>(request_json)
        .ok()
        .and_then(|v| v.get("lang").and_then(|l| l.as_str()).and_then(parse))
    else {
        return;
    };
    if pt != portuguese() {
        set_portuguese(pt);
        let _ = std::fs::write(service_file(), code());
    }
}

/// Does this text have a Portuguese pair? (for the interface's own test)
pub fn has_pair(s: &str) -> bool {
    pt::PT.iter().any(|(k, _)| *k == s)
}

/// The text in the language in use (the text itself when there is no translation).
pub fn tr(s: &'static str) -> &'static str {
    if !portuguese() {
        return s;
    }
    static MAP: std::sync::OnceLock<std::collections::HashMap<&'static str, &'static str>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| pt::PT.iter().copied().collect())
        .get(s)
        .copied()
        .unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::pt::PT;

    #[test]
    fn no_duplicate_keys() {
        let mut seen = std::collections::HashSet::new();
        for (k, _) in PT {
            assert!(seen.insert(*k), "duplicate: {k}");
        }
    }

    /// Every `tr("…")` in the library and the command line needs its pair in the table
    /// (unless the text is the same in both languages).
    #[test]
    fn every_tr_has_a_pair() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let missing = missing_pairs(&dir, PT, SAME);
        assert!(
            missing.is_empty(),
            "no Portuguese pair for:\n{}",
            missing.join("\n")
        );
    }

    /// Texts that are the same in both languages.
    const SAME: &[&str] = &["cgroups v2"];

    pub fn missing_pairs(
        dir: &std::path::Path,
        table: &[(&str, &str)],
        same: &[&str],
    ) -> Vec<String> {
        let norm = |s: &str| {
            s.replace("\\n", "")
                .split_whitespace()
                .collect::<String>()
                .replace('\\', "")
        };
        let keys: std::collections::HashSet<String> = table
            .iter()
            .map(|(k, _)| norm(k))
            .chain(same.iter().map(|k| norm(k)))
            .collect();
        let mut missing = Vec::new();
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".rs") || name.starts_with("i18n") {
                continue;
            }
            let text = std::fs::read_to_string(e.path()).unwrap();
            let mut rest = text.as_str();
            while let Some(i) = rest.find("i18n::tr(") {
                rest = rest[i + 9..].trim_start();
                if !rest.starts_with('"') {
                    continue;
                }
                let b = rest.as_bytes();
                let mut end = 1;
                while b[end] != b'"' {
                    end += if b[end] == b'\\' { 2 } else { 1 };
                }
                let lit = &rest[1..end];
                if !keys.contains(&norm(lit)) {
                    missing.push(format!("{name}: {lit}"));
                }
            }
        }
        missing
    }
}
