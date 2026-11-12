//! Inglês do `--help`: os textos vêm dos comentários `///` dos comandos (em português) e são
//! trocados aqui antes de o clap mostrar. Chave = o texto como o clap o guarda.

pub const HELP: &[(&str, &str)] = &[
    ("0 | 1 | all | 0,1", "0 | 1 | all | 0,1"),
    (
        "Ajustes da máquina: status | mode jogo|ia|silencioso|normal",
        "Machine settings: status | mode jogo|ia|silencioso|normal",
    ),
    (
        "Apaga a medição (a escolha de núcleos volta à ordem do sistema)",
        "Deletes the measurement (core choice goes back to the system order)",
    ),
    (
        "Apaga o aprendizado de programas cujo caminho contém este texto",
        "Deletes what was learned about programs whose path contains this text",
    ),
    (
        "Apaga um perfil (o programa volta para o automático)",
        "Deletes a profile (the program goes back to automatic)",
    ),
    (
        "Arquivo (padrão: ~/numapilot-escolhas-MAQUINA.json)",
        "File (default: ~/numapilot-escolhas-MACHINE.json)",
    ),
    (
        "Arquivo do relatório (padrão: ~/numapilot-autoteste-MAQUINA-DATA.txt)",
        "Report file (default: ~/numapilot-autoteste-MACHINE-DATE.txt)",
    ),
    (
        "Arquivo ou pasta de destino (padrão: sua pasta pessoal). Guarde fora do disco do sistema",
        "Destination file or folder (default: your home folder). Keep it off the system disk",
    ),
    ("Atalho para --use compute", "Shortcut for --use compute"),
    ("Atalho para --use graphics", "Shortcut for --use graphics"),
    (
        "Atualiza o próprio numapilot (pacote ou pasta do código): status | run",
        "Updates numapilot itself (package or code folder): status | run",
    ),
    (
        "Chave do programa (ex.: steam:1245620); em status, opcional",
        "Program key (e.g. steam:1245620); optional in status",
    ),
    (
        "Como usar a GPU: graphics (jogo, mesmo numa MI50) | compute (ROCm) | auto",
        "How to use the GPU: graphics (game, even on an MI50) | compute (ROCm) | auto",
    ),
    (
        "Confere a escolha de placa: roda vulkaninfo e glxinfo como um jogo rodaria em cada placa e diz qual respondeu",
        "Checks the card selection: runs vulkaninfo and glxinfo the way a game would on each card and says which one answered",
    ),
    (
        "Daemon do modo automático (rode como serviço systemd, como root)",
        "Automatic mode daemon (run it as a systemd service, as root)",
    ),
    ("Disco (GB)", "Disk (GB)"),
    (
        "Driver oficial da NVIDIA: status | install | update (install/update com sudo)",
        "Official NVIDIA driver: status | install | update (install/update with sudo)",
    ),
    (
        "Endereço de escuta (0.0.0.0 = rede local; 127.0.0.1 = só esta máquina)",
        "Listen address (0.0.0.0 = local network; 127.0.0.1 = this machine only)",
    ),
    (
        "Escolhe socket (NUMA), memória e GPU para cada programa",
        "Chooses socket (NUMA), memory and GPU for each program",
    ),
    (
        "Executa um programa no socket/GPU escolhidos (automático por padrão)",
        "Runs a program on the chosen socket/GPU (automatic by default)",
    ),
    (
        "Fixa um processo já em execução em um nó (ou all) e migra a memória",
        "Pins an already running process to a node (or all) and migrates its memory",
    ),
    (
        "GPU do frame generation do Lossless Scaling (lsfg-vk). Padrão: a mesma do jogo",
        "GPU for Lossless Scaling frame generation (lsfg-vk). Default: the same as the game",
    ),
    (
        "Gera ~/numapilot-relatorio.txt (diagnóstico + sistema) para enviar",
        "Generates ~/numapilot-relatorio.txt (diagnosis + system) to send",
    ),
    (
        "Grava todas as escolhas num arquivo, para levar a outro PC ou compartilhar",
        "Writes all choices to a file, to take to another PC or share",
    ),
    (
        "Guarda todas as configurações num .tar.gz (para reinstalar depois de formatar)",
        "Stores all settings in a .tar.gz (to reinstall after formatting)",
    ),
    (
        "Imprime os IDs ROCR_VISIBLE_DEVICES das GPUs de computação (para scripts)",
        "Prints the ROCR_VISIBLE_DEVICES IDs of the compute GPUs (for scripts)",
    ),
    ("Linhas de cada registro", "Lines from each log"),
    ("Linhas do log", "Log lines"),
    ("Lista os perfis salvos", "Lists the saved profiles"),
    (
        "Lista os processos que o daemon está gerenciando agora",
        "Lists the processes the daemon is managing right now",
    ),
    (
        "Mede a latência do cache em cada núcleo e passa a preferir os mais rápidos (se a diferença for maior que o ruído). Deixe o PC parado; leva alguns segundos",
        "Measures the cache latency of each core and starts preferring the fastest ones (if the difference is larger than the noise). Leave the PC idle; takes a few seconds",
    ),
    (
        "Mede a latência e a banda de memória entre os sockets",
        "Measures memory latency and bandwidth between the sockets",
    ),
    ("Memória (GB)", "Memory (GB)"),
    (
        "Mostra nós NUMA, núcleos, memória e GPUs (com o socket de cada uma)",
        "Shows NUMA nodes, cores, memory and GPUs (with each one's socket)",
    ),
    (
        "Mostra o final dos registros (serviço, programa gráfico, linha de comando) e do journal",
        "Shows the end of the logs (service, graphical program, command line) and of the journal",
    ),
    (
        "Mostra ou apaga o que o modo automático aprendeu",
        "Shows or deletes what automatic mode has learned",
    ),
    (
        "Máquina virtual Windows com uma placa só dela: check [GPU] | prepare GPU | unprepare | create NOME --gpu GPU --iso ARQ",
        "Windows virtual machine with a card of its own: check [GPU] | prepare GPU | unprepare | create NAME --gpu GPU --iso FILE",
    ),
    (
        "NVIDIA: clock máximo a travar (MHz)",
        "NVIDIA: maximum clock to lock (MHz)",
    ),
    (
        "Notebook com NVIDIA: o que abre com o sistema na integrada: status | igpu | normal",
        "Laptop with NVIDIA: what starts with the system on the integrated GPU: status | igpu | normal",
    ),
    (
        "Notebook com NVIDIA: status | auto | hibrido | integrada | nao-mexer | xwayland on|off",
        "Laptop with NVIDIA: status | auto | hibrido | integrada | nao-mexer | xwayland on|off",
    ),
    (
        "Não mexe no boot (OverDrive e parâmetros do kernel)",
        "Leaves the boot alone (OverDrive and kernel parameters)",
    ),
    ("Núcleos lógicos", "Logical cores"),
    (
        "O que abre quando o sistema liga: lista | ligar ID | desligar ID | remover ID | comando NOME \"COMANDO\" | arquivo CAMINHO | app BUSCA | origem ID | desinstalar ID",
        "What starts when the system boots: lista | ligar ID | desligar ID | remover ID | comando NAME \"COMMAND\" | arquivo PATH | app SEARCH | origem ID | desinstalar ID",
    ),
    (
        "Opções de boot que sobrevivem às atualizações do kernel (precisa de sudo para mudar). status | overdrive on|off | gpu-memory GiB|padrao | cmdline [add|remove PARAM...]",
        "Boot options that survive kernel updates (needs sudo to change). status | overdrive on|off | gpu-memory GiB|padrao | cmdline [add|remove PARAM...]",
    ),
    (
        "Otimizador de FPS: testa configurações a cada abertura do jogo (mede com o MangoHud)",
        "FPS optimizer: tests settings each time the game starts (measures with MangoHud)",
    ),
    (
        "Outros PCs: list | add LINK [NOME] | remove NOME | wake NOME (liga pela rede) | pull NOME [todos] (copia perfis) | backup (guarda o de todos)",
        "Other PCs: list | add LINK [NAME] | remove NAME | wake NAME (wakes over the network) | pull NAME [todos] (copies profiles) | backup (stores everyone's)",
    ),
    (
        "Painel web para o celular (rode como usuário; veja o link com o token)",
        "Web panel for the phone (run as your user; see the link with the token)",
    ),
    ("Passos de margem no fim", "Margin steps at the end"),
    (
        "Perfis por programa (os mesmos da interface gráfica)",
        "Per-program profiles (the same as in the graphical interface)",
    ),
    (
        "Placas especiais (BC-250...): setup (o que falta configurar) | status | governor-apply ARQUIVO (com sudo)",
        "Special boards (BC-250...): setup (what is left to configure) | status | governor-apply FILE (with sudo)",
    ),
    (
        "Pula os testes que rodam os comandos do numapilot um por um",
        "Skips the tests that run numapilot's commands one by one",
    ),
    (
        "Receitas da comunidade: show | update [URL] (update com sudo)",
        "Community recipes: show | update [URL] (update with sudo)",
    ),
    (
        "Restaura um backup (use sudo). Também reaplica OverDrive e parâmetros do kernel",
        "Restores a backup (use sudo). Also reapplies OverDrive and kernel parameters",
    ),
    (
        "Salva um perfil. KEY é o executável (caminho completo) ou steam:APPID",
        "Saves a profile. KEY is the executable (full path) or steam:APPID",
    ),
    ("Saída em JSON", "JSON output"),
    ("Saída em JSON (na tela)", "JSON output (on screen)"),
    ("Segundos de carga por passo", "Seconds of load per step"),
    (
        "Segundos de espera antes de abrir (comando, arquivo, app)",
        "Seconds to wait before starting (comando, arquivo, app)",
    ),
    (
        "Serviços do systemd: lista [--usuario] [--rodando|--falhas] | iniciar U | parar U | reiniciar U | log U",
        "systemd services: lista [--usuario] [--rodando|--falhas] | iniciar U | parar U | reiniciar U | log U",
    ),
    (
        "Serviços do usuário (systemctl --user) em vez dos do sistema",
        "User services (systemctl --user) instead of the system ones",
    ),
    (
        "Steam: coloca (ou tira) o numapilot nas opções de inicialização de todos os jogos",
        "Steam: puts numapilot in (or removes it from) the launch options of every game",
    ),
    (
        "Só esta placa (apelido, ex.: amd-0). Sem isso, todas",
        "Only this card (alias, e.g. amd-0). Without it, all of them",
    ),
    (
        "Só este: servico | gui | cli | tray | web",
        "Only this one: servico | gui | cli | tray | web",
    ),
    (
        "Só faz algo em notebook com integrada + NVIDIA (para o instalador)",
        "Only does something on a laptop with integrated + NVIDIA (for the installer)",
    ),
    (
        "Só mostra a decisão, sem executar",
        "Only shows the decision, without running",
    ),
    (
        "Só mostra a última medição guardada",
        "Only shows the last saved measurement",
    ),
    ("Só mostra o link e sai", "Only shows the link and exits"),
    ("Só os que estão rodando", "Only the running ones"),
    ("Só os que falharam", "Only the failed ones"),
    (
        "Tamanho do buffer por teste (MB)",
        "Buffer size per test (MB)",
    ),
    (
        "Tamanho do passo (MHz na NVIDIA, mV na AMD)",
        "Step size (MHz on NVIDIA, mV on AMD)",
    ),
    (
        "Testa cada parte no hardware real e diz o que corrigir",
        "Tests each part on the real hardware and says what to fix",
    ),
    (
        "Testa todas as partes do numapilot nesta máquina (só lê, não muda nada) e grava um relatório",
        "Tests every part of numapilot on this machine (read-only, changes nothing) and writes a report",
    ),
    ("Threads no teste de banda", "Threads in the bandwidth test"),
    (
        "Traz as escolhas de um arquivo exportado (as daqui com a mesma chave são trocadas)",
        "Brings in the choices from an exported file (the ones here with the same key are replaced)",
    ),
    (
        "Turbo por núcleos acordados: a tabela do processador e quantos núcleos estão acordados agora",
        "Turbo by awake cores: the processor's table and how many cores are awake right now",
    ),
    (
        "Undervolt automático de uma GPU (apelido ou PCI): testa passo a passo com carga e grava o último estável",
        "Automatic undervolt of a GPU (alias or PCI): tests step by step under load and saves the last stable one",
    ),
    (
        "Upscaling: fetch (baixa o OptiScaler) | status|install|remove APPID [--int8]",
        "Upscaling: fetch (downloads OptiScaler) | status|install|remove APPID [--int8]",
    ),
    (
        "auto | all | gpu | 0 | 1 | 0,1 | cpus:0-21",
        "auto | all | gpu | 0 | 1 | 0,1 | cpus:0-21",
    ),
    (
        "auto | display | compute | compute:2 | compute:all | none | apelido/PCI (vírgula)",
        "auto | display | compute | compute:2 | compute:all | none | alias/PCI (comma)",
    ),
    (
        "desinstalar: apagar também as configurações e dados do programa",
        "desinstalar: also delete the program's settings and data",
    ),
    ("desinstalar: não perguntar", "desinstalar: do not ask"),
    ("enable | disable | list", "enable | disable | list"),
    ("local | interleave | bind", "local | interleave | bind"),
    (
        "start | status | stop | apply",
        "start | status | stop | apply",
    ),
];

fn tr(s: &str) -> Option<&'static str> {
    HELP.iter().find(|(pt, _)| *pt == s).map(|(_, en)| *en)
}

/// Troca as descrições de um comando e de tudo abaixo dele.
pub fn localize(mut c: clap::Command) -> clap::Command {
    if let Some(e) = c.get_about().and_then(|a| tr(&a.to_string())) {
        c = c.about(e);
    }
    if let Some(e) = c.get_long_about().and_then(|a| tr(&a.to_string())) {
        c = c.long_about(e);
    }
    c = c.mut_args(|mut a| {
        if let Some(e) = a.get_help().and_then(|h| tr(&h.to_string())) {
            a = a.help(e);
        }
        if let Some(e) = a.get_long_help().and_then(|h| tr(&h.to_string())) {
            a = a.long_help(e);
        }
        a
    });
    let subs: Vec<String> = c
        .get_subcommands()
        .map(|s| s.get_name().to_string())
        .collect();
    for n in subs {
        c = c.mut_subcommand(n, localize);
    }
    c
}

/// Textos do `--help` ainda sem tradução (para o teste).
#[cfg(test)]
pub fn missing(c: &clap::Command, out: &mut Vec<String>) {
    let mut check = |s: Option<String>| {
        if let Some(s) = s
            && tr(&s).is_none()
        {
            out.push(s);
        }
    };
    check(c.get_about().map(|a| a.to_string()));
    check(c.get_long_about().map(|a| a.to_string()));
    for a in c.get_arguments() {
        check(a.get_help().map(|h| h.to_string()));
        check(a.get_long_help().map(|h| h.to_string()));
    }
    for s in c.get_subcommands() {
        missing(s, out);
    }
}
