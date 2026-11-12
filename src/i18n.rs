//! Idioma das mensagens. O português é o idioma do código. Texto fixo: `tr("texto")` com o par
//! em `i18n_en.rs`; texto com valores: `tf!("texto {}", "text {}", valor)`. O que não tem par
//! aparece em português.
//!
//! De onde vem o idioma: `NUMAPILOT_LANG` (pt/en), depois `~/.config/numapilot/lang` (a escolha
//! feita na interface), depois, para o root, o último idioma pedido ao serviço
//! (`/var/lib/numapilot/lang`), e por fim o idioma do sistema (inglês só se não for português).
//! O serviço responde no idioma de quem pede: cada pedido leva `"lang"`.

use std::sync::atomic::{AtomicU8, Ordering};

#[path = "i18n_en.rs"]
mod en;

/// 0 = ainda não decidido, 1 = português, 2 = inglês.
static LANG: AtomicU8 = AtomicU8::new(0);

/// `format!` nos dois idiomas: `tf!("texto {}", "text {}", valor)`.
#[macro_export]
macro_rules! tf {
    ($pt:literal, $en:literal $(, $($arg:tt)*)?) => {
        if $crate::i18n::english() {
            format!($en $(, $($arg)*)?)
        } else {
            format!($pt $(, $($arg)*)?)
        }
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

fn parse(s: &str) -> Option<bool> {
    match s.trim().to_lowercase().as_str() {
        "en" | "english" => Some(true),
        "pt" | "pt_br" | "pt-br" | "portugues" | "português" => Some(false),
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
    !(l.is_empty() || l.starts_with("pt") || l == "C" || l == "POSIX" || l.starts_with("C."))
}

pub fn english() -> bool {
    if cfg!(test) {
        return false;
    }
    match LANG.load(Ordering::Relaxed) {
        0 => {
            let en = detect();
            LANG.store(if en { 2 } else { 1 }, Ordering::Relaxed);
            en
        }
        v => v == 2,
    }
}

/// Troca o idioma deste processo.
pub fn set_english(en: bool) {
    LANG.store(if en { 2 } else { 1 }, Ordering::Relaxed);
}

/// Troca o idioma e guarda a escolha do usuário (a interface usa).
pub fn save_user_choice(en: bool) {
    set_english(en);
    let f = user_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, code());
}

pub fn code() -> &'static str {
    if english() { "en" } else { "pt" }
}

/// No serviço: passa a responder (e a publicar o estado) no idioma de quem fez o pedido.
pub fn follow_client(request_json: &str) {
    let Some(en) = serde_json::from_str::<serde_json::Value>(request_json)
        .ok()
        .and_then(|v| v.get("lang").and_then(|l| l.as_str()).and_then(parse))
    else {
        return;
    };
    if en != english() {
        set_english(en);
        let _ = std::fs::write(service_file(), code());
    }
}

/// Texto no idioma escolhido (o próprio texto se não houver tradução).
pub fn tr(s: &'static str) -> &'static str {
    if !english() {
        return s;
    }
    static MAP: std::sync::OnceLock<std::collections::HashMap<&'static str, &'static str>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| en::EN.iter().copied().collect())
        .get(s)
        .copied()
        .unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::en::EN;

    #[test]
    fn no_duplicate_keys() {
        let mut seen = std::collections::HashSet::new();
        for (k, _) in EN {
            assert!(seen.insert(*k), "repetido: {k}");
        }
    }

    /// Todo `tr("…")` da biblioteca e da linha de comando precisa ter o par na tabela.
    #[test]
    fn every_tr_has_a_pair() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let norm = |s: &str| {
            s.replace("\\n", "")
                .split_whitespace()
                .collect::<String>()
                .replace('\\', "")
        };
        let keys: std::collections::HashSet<String> = EN.iter().map(|(k, _)| norm(k)).collect();
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
        assert!(missing.is_empty(), "sem tradução:\n{}", missing.join("\n"));
    }
}
