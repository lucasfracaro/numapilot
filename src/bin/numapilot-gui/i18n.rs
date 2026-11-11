//! Tradução da interface. O português é o idioma do código; o inglês cobre por enquanto a
//! navegação (menu, títulos, barra da aba Programas). O que não está na tabela aparece em
//! português. Para traduzir mais: acrescente pares em `EN` e use `tr("texto")` no lugar do texto.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

const EN: &[(&str, &str)] = &[
    // menu
    ("Programas", "Programs"),
    ("Inicialização", "Startup"),
    ("Serviços", "Services"),
    ("Mapa e medições", "Map & measurements"),
    ("Escolhas salvas", "Saved choices"),
    ("Placas de vídeo", "Graphics cards"),
    ("Máquina", "Machine"),
    ("Laboratório", "Lab"),
    ("Servidor de IA", "AI server"),
    ("Streaming", "Streaming"),
    ("Configurações", "Settings"),
    ("Testes", "Tests"),
    // descrições das abas
    (
        "O que está aberto agora e em qual processador e placa cada um roda",
        "What is open now and which processor and card each one runs on",
    ),
    (
        "Tudo que abre quando o sistema liga: programas do login e serviços",
        "Everything that starts with the system: login programs and services",
    ),
    (
        "Serviços do sistema e do usuário: iniciar, parar, reiniciar e ver o log",
        "System and user services: start, stop, restart and read the log",
    ),
    (
        "Núcleos, memória por socket e medições de desempenho",
        "Cores, memory per socket and performance measurements",
    ),
    (
        "Escolhas por programa que valem toda vez que ele abrir",
        "Per-program choices applied every time it starts",
    ),
    (
        "Ventoinha, potência, clock e undervolt de cada placa",
        "Fan, power, clock and undervolt of each card",
    ),
    (
        "Modo de energia, processador, notebook e saúde do hardware",
        "Power mode, processor, laptop and hardware health",
    ),
    (
        "Containers com ROCm, Jupyter e IDEs",
        "Containers with ROCm, Jupyter and IDEs",
    ),
    (
        "Modelos locais com llama.cpp",
        "Local models with llama.cpp",
    ),
    (
        "Sunshine e Moonlight: jogar de outro aparelho",
        "Sunshine and Moonlight: play from another device",
    ),
    (
        "Outros PCs, avisos, atualização e máquina virtual",
        "Other PCs, notifications, updates and virtual machine",
    ),
    (
        "Roteiro para testar o programa, autoteste e relatório para enviar",
        "Manual test script, self-test and a report to send",
    ),
    // topo e tema
    ("Serviço ativo", "Service running"),
    ("Serviço parado", "Service stopped"),
    ("Ativar o serviço", "Start the service"),
    (
        "O serviço do numapilot não está rodando: as escolhas não são aplicadas.",
        "The numapilot service is not running: choices are not applied.",
    ),
    ("Último comando", "Last command"),
    ("Tema", "Theme"),
    ("Sistema", "System"),
    ("Claro", "Light"),
    ("Escuro", "Dark"),
    ("Memória", "Memory"),
    // aba Programas
    ("Nome", "Name"),
    ("Disco", "Disk"),
    ("Memória da GPU", "GPU memory"),
    ("Placa", "Card"),
    ("Ajustar…", "Adjust…"),
    ("Encerrar", "End"),
    ("Forçar parada", "Force stop"),
    ("Abrir tudo", "Expand all"),
    ("Fechar tudo", "Collapse all"),
    ("Só os meus", "Only mine"),
    ("🔍 Buscar app instalado…", "🔍 Find installed app…"),
    ("Buscar app instalado", "Find installed app"),
    ("Aplicativos", "Applications"),
    ("Sessão e sistema", "Session and system"),
    ("programas em execução", "programs running"),
    // Configurações
    ("Abas do menu", "Menu tabs"),
    ("Idioma", "Language"),
];

fn file() -> std::path::PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::jobs::home().join(".config"))
        .join("numapilot/lang")
}

/// Carrega o idioma salvo; sem escolha, inglês só se o sistema não estiver em português.
pub fn init() {
    let saved = std::fs::read_to_string(file()).unwrap_or_default();
    let en = match saved.trim() {
        "en" => true,
        "pt" => false,
        _ => {
            let l = std::env::var("LC_ALL")
                .or_else(|_| std::env::var("LC_MESSAGES"))
                .or_else(|_| std::env::var("LANG"))
                .unwrap_or_default();
            !(l.is_empty() || l.starts_with("pt") || l == "C" || l.starts_with("C."))
        }
    };
    ENGLISH.store(en, Ordering::Relaxed);
}

pub fn english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

pub fn set_english(en: bool) {
    ENGLISH.store(en, Ordering::Relaxed);
    let f = file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, if en { "en" } else { "pt" });
}

/// Texto no idioma escolhido (o próprio texto se não houver tradução).
pub fn tr(s: &'static str) -> &'static str {
    if !english() {
        return s;
    }
    EN.iter()
        .find(|(pt, _)| *pt == s)
        .map(|(_, en)| *en)
        .unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_duplicate_keys() {
        for (i, (k, _)) in EN.iter().enumerate() {
            assert!(!EN[..i].iter().any(|(o, _)| o == k), "repetido: {k}");
        }
    }
}
