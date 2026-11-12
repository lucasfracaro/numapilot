//! Interface translation. English is the language of the code; Portuguese is the translation.
//! Fixed text: `tr("text")` with its pair in `i18n_pt.rs`; text with values:
//! `tf!("text {}", "texto {}", value)`. The language itself is decided by the library
//! (`numapilot::i18n`), so the interface, the service and the command line agree.

#[path = "i18n_pt.rs"]
mod pt;

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

/// Fixed text with its Portuguese form inline: `t2!("System", "Do sistema")`.
#[macro_export]
macro_rules! t2 {
    ($en:literal, $pt:literal) => {
        if $crate::i18n::portuguese() { $pt } else { $en }
    };
}

/// Loads the language (saved choice or system locale; the library decides).
pub fn init() {
    let _ = portuguese();
}

pub fn portuguese() -> bool {
    numapilot::i18n::portuguese()
}

pub fn set_portuguese(pt: bool) {
    numapilot::i18n::save_user_choice(pt);
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
        // texts that come from the library's tables (mitigations, modes, Acer settings…)
        .unwrap_or_else(|| numapilot::i18n::tr(s))
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

    /// Texts that are the same in both languages.
    const SAME: &[&str] = &[];

    /// Every `tr("…")` in the interface needs its Portuguese pair (here or in the library).
    #[test]
    fn every_tr_has_a_pair() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/numapilot-gui");
        let norm = |s: &str| {
            s.replace("\\n", "")
                .split_whitespace()
                .collect::<String>()
                .replace('\\', "")
        };
        let keys: std::collections::HashSet<String> = PT
            .iter()
            .map(|(k, _)| norm(k))
            .chain(SAME.iter().map(|k| norm(k)))
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
                if !keys.contains(&norm(lit)) && !numapilot::i18n::has_pair(lit) {
                    missing.push(format!("{name}: {lit}"));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "no Portuguese pair for:\n{}",
            missing.join("\n")
        );
    }
}
