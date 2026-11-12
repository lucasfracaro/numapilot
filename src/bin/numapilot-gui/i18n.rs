//! Tradução da interface. O português é o idioma do código. Texto fixo: `tr("texto")` com o par
//! em `i18n_en.rs`; texto com valores: `tf!("texto {}", "text {}", valor)`. O que não tem tradução
//! aparece em português (as mensagens que vêm do serviço e da linha de comando, por exemplo).

#[path = "i18n_en.rs"]
mod en;

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

/// Carrega o idioma (escolha salva ou idioma do sistema; a biblioteca decide).
pub fn init() {
    let _ = english();
}

pub fn english() -> bool {
    numapilot::i18n::english()
}

pub fn set_english(en: bool) {
    numapilot::i18n::save_user_choice(en);
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
        // textos que vêm das tabelas da biblioteca (mitigações, modos, ajustes da Acer…)
        .unwrap_or_else(|| numapilot::i18n::tr(s))
}

#[cfg(test)]
mod tests {
    use super::en::EN;

    /// Todo `tr("…")` do código precisa ter o par na tabela (senão o inglês mostra português).
    #[test]
    fn every_tr_has_a_pair() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/numapilot-gui");
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
                let mut end = 1;
                let b = rest.as_bytes();
                while b[end] != b'"' {
                    end += if b[end] == b'\\' { 2 } else { 1 };
                }
                let lit = &rest[1..end];
                // a tabela guarda o texto já interpretado; compara sem as continuações de linha
                let norm = |s: &str| {
                    s.replace("\\n", "")
                        .split_whitespace()
                        .collect::<String>()
                        .replace('\\', "")
                };
                if !EN.iter().any(|(k, _)| norm(k) == norm(lit)) {
                    missing.push(format!("{name}: {lit}"));
                }
            }
        }
        assert!(missing.is_empty(), "sem tradução:\n{}", missing.join("\n"));
    }

    #[test]
    fn no_duplicate_keys() {
        for (i, (k, _)) in EN.iter().enumerate() {
            assert!(!EN[..i].iter().any(|(o, _)| o == k), "repetido: {k}");
        }
    }
}
