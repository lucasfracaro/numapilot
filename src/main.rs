use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use numapilot::config::{Config, CpuSpec, GpuSpec, MemSpec};
use numapilot::decide::{Kind, LaunchInput, decide_launch, gpu_env, sample_node_loads};
use numapilot::learned::{self, Learned};
use numapilot::profiles::{Profile, Profiles, program_key};
use numapilot::topo::{GpuRole, Topology};
use numapilot::util::{basename, format_cpulist};
use numapilot::{ctl, daemon, procfs};
use std::os::unix::process::CommandExt;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "numapilot",
    version,
    about = "Escolhe socket (NUMA), memória e GPU para cada programa"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Mostra nós NUMA, núcleos, memória e GPUs (com o socket de cada uma).
    Topo,
    /// Executa um programa no socket/GPU escolhidos (automático por padrão).
    Run {
        /// auto | all | gpu | 0 | 1 | 0,1 | cpus:0-21
        #[arg(long, short = 'c')]
        cpu: Option<String>,
        /// auto | display | compute | compute:2 | compute:all | none | apelido/PCI (vírgula)
        #[arg(long, short = 'g')]
        gpu: Option<String>,
        /// local | interleave | bind
        #[arg(long, short = 'm')]
        mem: Option<String>,
        /// Como usar a GPU: graphics (jogo, mesmo numa MI50) | compute (ROCm) | auto
        #[arg(long = "use", short = 'u')]
        usage: Option<String>,
        /// GPU do frame generation do Lossless Scaling (lsfg-vk). Padrão: a mesma do jogo.
        #[arg(long)]
        fg_gpu: Option<String>,
        /// Atalho para --use compute.
        #[arg(long, conflicts_with_all = ["graphics", "usage"])]
        compute: bool,
        /// Atalho para --use graphics.
        #[arg(long, conflicts_with = "usage")]
        graphics: bool,
        /// Só mostra a decisão, sem executar.
        #[arg(long, short = 'n')]
        dry_run: bool,
        #[arg(trailing_var_arg = true, required = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Daemon do modo automático (rode como serviço systemd, como root).
    Daemon,
    /// Lista os processos que o daemon está gerenciando agora.
    Status,
    /// Fixa um processo já em execução em um nó (ou all) e migra a memória.
    Pin {
        pid: i32,
        /// 0 | 1 | all | 0,1
        #[arg(long, short = 'c')]
        cpu: String,
        #[arg(long)]
        no_migrate: bool,
    },
    /// Testa cada parte no hardware real e diz o que corrigir.
    Doctor {
        /// Saída em JSON.
        #[arg(long)]
        json: bool,
        /// Gera ~/numapilot-relatorio.txt (diagnóstico + sistema) para enviar.
        #[arg(long)]
        report: bool,
    },
    /// Opções de boot que sobrevivem às atualizações do kernel (precisa de sudo para mudar).
    /// status | overdrive on|off | gpu-memory GiB|padrao | cmdline [add|remove PARAM...]
    Boot {
        what: String,
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Placas especiais (BC-250...): setup (o que falta configurar) | status |
    /// governor-apply ARQUIVO (com sudo)
    Board {
        action: String,
        file: Option<std::path::PathBuf>,
    },
    /// Receitas da comunidade: show | update [URL] (update com sudo)
    Recipes { action: String, url: Option<String> },
    /// Upscaling: fetch (baixa o OptiScaler) | status|install|remove APPID [--int8]
    Upscale {
        action: String,
        appid: Option<String>,
        #[arg(long)]
        int8: bool,
    },
    /// Outros PCs: list | add LINK [NOME] | remove NOME | wake NOME (liga pela rede) | pull NOME [todos] (copia perfis) | backup (guarda o de todos)
    Peers {
        action: String,
        arg: Option<String>,
        name: Option<String>,
    },
    /// Máquina virtual Windows com uma placa só dela: check [GPU] | prepare GPU | unprepare | create NOME --gpu GPU --iso ARQ
    Vm {
        action: String,
        arg: Option<String>,
        #[arg(long)]
        gpu: Option<String>,
        #[arg(long)]
        iso: Option<std::path::PathBuf>,
        /// Memória (GB)
        #[arg(long, default_value_t = 16)]
        mem: u32,
        /// Núcleos lógicos
        #[arg(long, default_value_t = 8)]
        cpus: usize,
        /// Disco (GB)
        #[arg(long, default_value_t = 120)]
        disk: u32,
    },
    /// Serviços do systemd: lista [--usuario] [--rodando|--falhas] | iniciar U | parar U |
    /// reiniciar U | log U
    #[command(alias = "services")]
    Servicos {
        action: Option<String>,
        unit: Option<String>,
        /// Serviços do usuário (systemctl --user) em vez dos do sistema
        #[arg(long)]
        usuario: bool,
        /// Só os que estão rodando
        #[arg(long)]
        rodando: bool,
        /// Só os que falharam
        #[arg(long)]
        falhas: bool,
        /// Linhas do log
        #[arg(short = 'n', long, default_value_t = 60)]
        linhas: u32,
    },
    /// O que abre quando o sistema liga: lista | ligar ID | desligar ID | remover ID |
    /// comando NOME "COMANDO" | arquivo CAMINHO | app BUSCA | origem ID | desinstalar ID
    #[command(alias = "startup")]
    Inicio {
        action: Option<String>,
        args: Vec<String>,
        /// desinstalar: apagar também as configurações e dados do programa
        #[arg(long)]
        config: bool,
        /// desinstalar: não perguntar
        #[arg(long)]
        sim: bool,
        /// Segundos de espera antes de abrir (comando, arquivo, app)
        #[arg(long, default_value_t = 0)]
        atraso: u32,
    },
    /// Notebook com NVIDIA: o que abre com o sistema na integrada: status | igpu | normal
    Autostart {
        action: Option<String>,
        /// Só faz algo em notebook com integrada + NVIDIA (para o instalador)
        #[arg(long)]
        if_hybrid: bool,
    },
    /// Notebook com NVIDIA: status | auto | hibrido | integrada | nao-mexer | xwayland on|off
    Hybrid {
        action: Option<String>,
        arg: Option<String>,
    },
    /// Atualiza o próprio numapilot (pacote ou pasta do código): status | run
    #[command(name = "self-update")]
    SelfUpdate { action: Option<String> },
    /// Undervolt automático de uma GPU (apelido ou PCI): testa passo a passo com carga e grava o último estável
    Undervolt {
        gpu: String,
        /// Segundos de carga por passo
        #[arg(long, default_value_t = 60)]
        secs: u64,
        /// Passos de margem no fim
        #[arg(long, default_value_t = 1)]
        margin: u32,
        /// NVIDIA: clock máximo a travar (MHz)
        #[arg(long)]
        clock: Option<u32>,
        /// Tamanho do passo (MHz na NVIDIA, mV na AMD)
        #[arg(long)]
        step: Option<i32>,
    },
    /// Driver oficial da NVIDIA: status | install | update (install/update com sudo)
    Driver { action: String },
    /// Guarda todas as configurações num .tar.gz (para reinstalar depois de formatar).
    Backup {
        /// Arquivo ou pasta de destino (padrão: sua pasta pessoal). Guarde fora do disco do sistema.
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Restaura um backup (use sudo). Também reaplica OverDrive e parâmetros do kernel.
    Restore {
        file: std::path::PathBuf,
        /// Não mexe no boot (OverDrive e parâmetros do kernel).
        #[arg(long)]
        no_boot: bool,
    },
    /// Ajustes da máquina: status | mode jogo|ia|silencioso|normal
    Machine {
        action: String,
        value: Option<String>,
    },
    /// Painel web para o celular (rode como usuário; veja o link com o token).
    Web {
        #[arg(long, default_value_t = numapilot::web::DEFAULT_PORT)]
        port: u16,
        /// Endereço de escuta (0.0.0.0 = rede local; 127.0.0.1 = só esta máquina).
        #[arg(long, default_value = "0.0.0.0")]
        bind: String,
        /// Só mostra o link e sai.
        #[arg(long)]
        url: bool,
    },
    /// Otimizador de FPS: testa configurações a cada abertura do jogo (mede com o MangoHud).
    Tune {
        /// start | status | stop | apply
        action: String,
        /// Chave do programa (ex.: steam:1245620); em status, opcional.
        key: Option<String>,
    },
    /// Steam: coloca (ou tira) o numapilot nas opções de inicialização de todos os jogos.
    Steam {
        /// enable | disable | list
        action: String,
    },
    /// Mede a latência e a banda de memória entre os sockets.
    Bench {
        /// Tamanho do buffer por teste (MB).
        #[arg(long, default_value_t = 128)]
        size_mb: usize,
        /// Threads no teste de banda.
        #[arg(long, default_value_t = 8)]
        threads: usize,
    },
    /// Perfis por programa (os mesmos da interface gráfica).
    Profile {
        #[command(subcommand)]
        action: ProfileCmd,
    },
    /// Imprime os IDs ROCR_VISIBLE_DEVICES das GPUs de computação (para scripts).
    #[command(name = "rocr-ids")]
    RocrIds,
    /// Confere a escolha de placa: roda vulkaninfo e glxinfo como um jogo rodaria em cada placa
    /// e diz qual respondeu.
    GpuTest {
        /// Só esta placa (apelido, ex.: amd-0). Sem isso, todas.
        gpu: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Mede a latência do cache em cada núcleo e passa a preferir os mais rápidos (se a diferença
    /// for maior que o ruído). Deixe o PC parado; leva alguns segundos.
    BenchCores {
        /// Só mostra a última medição guardada.
        #[arg(long)]
        mostrar: bool,
        /// Apaga a medição (a escolha de núcleos volta à ordem do sistema).
        #[arg(long)]
        apagar: bool,
    },
    /// Turbo por núcleos acordados: a tabela do processador e quantos núcleos estão acordados agora.
    Turbo,
    /// Mostra o final dos registros (serviço, programa gráfico, linha de comando) e do journal.
    Log {
        /// Linhas de cada registro.
        #[arg(short = 'n', default_value_t = 40)]
        lines: usize,
        /// Só este: servico | gui | cli | tray | web
        which: Option<String>,
    },
    /// Testa todas as partes do numapilot nesta máquina (só lê, não muda nada) e grava um relatório.
    Autoteste {
        /// Arquivo do relatório (padrão: ~/numapilot-autoteste-MAQUINA-DATA.txt).
        #[arg(long)]
        saida: Option<std::path::PathBuf>,
        /// Pula os testes que rodam os comandos do numapilot um por um.
        #[arg(long)]
        rapido: bool,
        /// Saída em JSON (na tela).
        #[arg(long)]
        json: bool,
    },
    /// Mostra ou apaga o que o modo automático aprendeu.
    Learned {
        /// Apaga o aprendizado de programas cujo caminho contém este texto.
        #[arg(long)]
        forget: Option<String>,
    },
}

#[derive(Subcommand)]
enum ProfileCmd {
    /// Lista os perfis salvos.
    List,
    /// Salva um perfil. KEY é o executável (caminho completo) ou steam:APPID.
    Set {
        key: String,
        #[arg(long, default_value = "auto")]
        cpu: String,
        #[arg(long, default_value = "auto")]
        gpu: String,
        #[arg(long = "use", default_value = "auto")]
        usage: String,
    },
    /// Apaga um perfil (o programa volta para o automático).
    Rm { key: String },
    /// Grava todas as escolhas num arquivo, para levar a outro PC ou compartilhar.
    Export {
        /// Arquivo (padrão: ~/numapilot-escolhas-MAQUINA.json).
        file: Option<std::path::PathBuf>,
    },
    /// Traz as escolhas de um arquivo exportado (as daqui com a mesma chave são trocadas).
    Import { file: std::path::PathBuf },
}

fn main() {
    // `numapilot hybrid | head`: ao fechar o pipe, sai quieto em vez de "cair" com Broken pipe
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    if let Err(e) = real_main() {
        numapilot::log::error(&format!("{e:#}"));
        eprintln!(
            "{}",
            numapilot::tf!("numapilot: erro: {e:#}", "numapilot: error: {e:#}")
        );
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Daemon => numapilot::log::init("daemon"),
        Cmd::Web { .. } => numapilot::log::init("web"),
        // só leitura do próprio registro: não precisa registrar
        Cmd::Log { .. } => {}
        _ => {
            numapilot::log::init("cli");
            let args: Vec<String> = std::env::args().skip(1).collect();
            numapilot::log::info(&format!("numapilot {}", args.join(" ")));
        }
    }
    let cfg = Config::load()?;
    match cli.cmd {
        Cmd::Topo => cmd_topo(&cfg),
        Cmd::Log { lines, which } => cmd_log(lines, which.as_deref()),
        Cmd::Turbo => cmd_turbo(&cfg),
        Cmd::BenchCores { mostrar, apagar } => cmd_bench_cores(&cfg, mostrar, apagar),
        Cmd::GpuTest { gpu, json } => cmd_gpu_test(&cfg, gpu.as_deref(), json),
        Cmd::Autoteste {
            saida,
            rapido,
            json,
        } => {
            let r = numapilot::selftest::run(&cfg, !rapido, &mut |l| {
                if !json {
                    println!("{l}")
                }
            });
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
                return Ok(());
            }
            let path = saida.unwrap_or_else(numapilot::selftest::default_path);
            std::fs::write(&path, numapilot::selftest::render(&r))?;
            println!();
            println!("{}", r.summary());
            println!(
                "{}",
                numapilot::tf!("Relatório: {}", "Report: {}", path.display())
            );
            if r.failed() > 0 {
                std::process::exit(2);
            }
            Ok(())
        }
        Cmd::Run {
            cpu,
            gpu,
            mem,
            usage,
            fg_gpu,
            compute,
            graphics,
            dry_run,
            command,
        } => {
            let usage = match (compute, graphics, usage) {
                (true, _, _) => Some(Kind::Compute),
                (_, true, _) => Some(Kind::Graphics),
                (_, _, Some(u)) => Kind::parse_usage(&u)?,
                _ => None,
            };
            cmd_run(&cfg, cpu, gpu, mem, usage, fg_gpu, dry_run, command)
        }
        Cmd::Profile { action } => cmd_profile(action),
        Cmd::Doctor { json, report } => {
            let checks = numapilot::doctor::run(&cfg);
            if json {
                println!("{}", serde_json::to_string_pretty(&checks)?);
            } else {
                numapilot::doctor::print(&checks);
            }
            if report {
                let home = std::env::var_os("HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default();
                let p = home.join("numapilot-relatorio.txt");
                std::fs::write(&p, numapilot::doctor::report(&checks))?;
                println!(
                    "{}",
                    numapilot::tf!(
                        "\nRelatório salvo em {}: é só me mandar esse arquivo.",
                        "\nReport saved to {}: just send me that file.",
                        p.display()
                    )
                );
            }
            Ok(())
        }
        Cmd::Steam { action } => cmd_steam(&action),
        Cmd::Tune { action, key } => cmd_tune(&cfg, &action, key),
        Cmd::Boot { what, args } => cmd_boot(&what, &args),
        Cmd::Machine { action, value } => cmd_machine(&cfg, &action, value.as_deref()),
        Cmd::Board { action, file } => {
            let topo = Topology::load(&cfg);
            let r = numapilot::recipes::load();
            let boards = r.boards(&topo);
            if boards.is_empty() {
                println!(
                    "{}",
                    numapilot::tf!(
                        "Nenhuma placa especial conhecida neste PC.",
                        "No known special board in this PC."
                    )
                );
                return Ok(());
            }
            match action.as_str() {
                "setup" | "configurar" => {
                    use numapilot::boards::StepState;
                    for (b, g) in &boards {
                        let steps = numapilot::boards::first_setup(b, &g.pci);
                        println!(
                            "{}",
                            numapilot::tf!(
                                "{} ({}): primeira configuração: {}",
                                "{} ({}): first setup: {}",
                                b.name,
                                g.alias,
                                numapilot::boards::setup_summary(&steps)
                            )
                        );
                        for s in &steps {
                            let tag = match s.state {
                                StepState::Ok => numapilot::i18n::tr("pronto  "),
                                StepState::Todo => numapilot::i18n::tr("FALTA   "),
                                StepState::Opcional => numapilot::i18n::tr("opcional"),
                            };
                            println!("  {tag} {}: {}", s.title, s.detail);
                        }
                    }
                    Ok(())
                }
                "status" => {
                    for (b, g) in &boards {
                        println!("{} ({}, {})", b.name, g.alias, g.pci);
                        for s in numapilot::boards::services(b)
                            .iter()
                            .filter(|s| s.installed)
                        {
                            println!(
                                "  governor {}: {}",
                                s.name,
                                if s.active {
                                    numapilot::i18n::tr("rodando")
                                } else {
                                    numapilot::i18n::tr("parado")
                                }
                            );
                        }
                        if let Some(c) = numapilot::boards::governor_config(b) {
                            println!(
                                "{}",
                                numapilot::tf!("  configuração: {c}", "  configuration: {c}")
                            );
                        }
                        for n in &b.notes {
                            println!("  - {n}");
                        }
                    }
                    Ok(())
                }
                "governor-apply" => {
                    let f = file.context(numapilot::i18n::tr("faltou o arquivo"))?;
                    let b = boards
                        .iter()
                        .map(|x| x.0)
                        .find(|b| !b.governor_configs.is_empty())
                        .context(numapilot::i18n::tr("nenhuma placa com governor"))?;
                    numapilot::boards::governor_apply(b, &f, &mut |m| println!("{m}"))
                        .map_err(|e| anyhow::anyhow!(e))
                }
                _ => bail!(
                    "{}",
                    numapilot::tf!(
                        "use: numapilot board status | sudo numapilot board governor-apply ARQUIVO",
                        "usage: numapilot board status | sudo numapilot board governor-apply FILE"
                    )
                ),
            }
        }
        Cmd::Recipes { action, url } => match action.as_str() {
            "show" => {
                let r = numapilot::recipes::load();
                println!(
                    "{}",
                    numapilot::tf!(
                        "Receitas versão {}: {} placas, {} processadores, {} arquiteturas",
                        "Recipes version {}: {} boards, {} processors, {} architectures",
                        r.version,
                        r.board.len(),
                        r.cpu.len(),
                        r.arch.len()
                    )
                );
                let topo = Topology::load(&cfg);
                for g in &topo.gpus {
                    if let Some(a) = r.arch_for(g) {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "  {}: {} · computação {} · upscaling {}",
                                "  {}: {} · compute {} · upscaling {}",
                                g.alias,
                                a.family,
                                a.compute,
                                a.upscale.join(", ")
                            )
                        );
                    }
                }
                for n in r.cpu_notes(&numapilot::recipes::cpu_model()) {
                    println!("  - {n}");
                }
                Ok(())
            }
            "update" => {
                let u = url.unwrap_or_else(|| numapilot::recipes::DEFAULT_URL.to_string());
                let v = numapilot::recipes::update(&u).map_err(|e| anyhow::anyhow!(e))?;
                println!(
                    "{}",
                    numapilot::tf!(
                        "Receitas atualizadas (versão {v}).",
                        "Recipes updated (version {v})."
                    )
                );
                Ok(())
            }
            _ => bail!(
                "{}",
                numapilot::tf!(
                    "use: numapilot recipes show | sudo numapilot recipes update [URL]",
                    "usage: numapilot recipes show | sudo numapilot recipes update [URL]"
                )
            ),
        },
        Cmd::Upscale {
            action,
            appid,
            int8,
        } => {
            use numapilot::upscale;
            if action == "fetch" {
                let v = upscale::fetch(&mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))?;
                println!(
                    "{}",
                    numapilot::tf!("OptiScaler {v} pronto.", "OptiScaler {v} ready.")
                );
                return Ok(());
            }
            let id = appid.context(numapilot::i18n::tr("faltou o appid da Steam"))?;
            let root = upscale::steam_game_root(&id).context(numapilot::i18n::tr(
                "jogo não encontrado nas bibliotecas da Steam",
            ))?;
            let dir = upscale::exe_dir(&root)
                .context(numapilot::i18n::tr("não achei o executável do jogo"))?;
            match action.as_str() {
                "status" => {
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Pasta do executável: {}",
                            "Executable folder: {}",
                            dir.display()
                        )
                    );
                    match upscale::installed(&dir) {
                        Some(i) => println!(
                            "{}",
                            numapilot::tf!(
                                "OptiScaler {} instalado ({} arquivos)",
                                "OptiScaler {} installed ({} files)",
                                i.version,
                                i.files.len()
                            )
                        ),
                        None => println!(
                            "{}",
                            numapilot::tf!("OptiScaler não instalado", "OptiScaler not installed")
                        ),
                    }
                    Ok(())
                }
                "install" => {
                    if upscale::local_release().is_none() {
                        upscale::fetch(&mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))?;
                    }
                    let v = upscale::install(&dir, int8).map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "{}",
                        numapilot::tf!(
                            "OptiScaler {v} instalado em {}. Não use em jogos online (anti-cheat pode banir).",
                            "OptiScaler {v} installed in {}. Do not use it in online games (the anti-cheat may ban you).",
                            dir.display()
                        )
                    );
                    Ok(())
                }
                "remove" => {
                    upscale::remove(&dir).map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "{}",
                        numapilot::tf!(
                            "OptiScaler removido de {}",
                            "OptiScaler removed from {}",
                            dir.display()
                        )
                    );
                    Ok(())
                }
                _ => bail!(
                    "{}",
                    numapilot::tf!(
                        "use: numapilot upscale fetch | status|install|remove APPID [--int8]",
                        "usage: numapilot upscale fetch | status|install|remove APPID [--int8]"
                    )
                ),
            }
        }
        Cmd::Peers { action, arg, name } => {
            use numapilot::peers;
            match action.as_str() {
                "list" => {
                    for s in peers::summary() {
                        println!(
                            "{:<16} {}  {}",
                            s["name"].as_str().unwrap_or(""),
                            if s["online"].as_bool() == Some(true) {
                                "online "
                            } else {
                                "offline"
                            },
                            s["hot"]
                                .as_f64()
                                .map(|t| numapilot::tf!(
                                    "GPU mais quente {t:.0} °C",
                                    "hottest GPU {t:.0} °C"
                                ))
                                .unwrap_or_default()
                        );
                    }
                    Ok(())
                }
                "add" => {
                    let p = peers::add(
                        &arg.context(numapilot::i18n::tr(
                            "faltou o link (o do QR code do outro PC)",
                        ))?,
                        name.as_deref(),
                    )
                    .map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "{}",
                        numapilot::tf!("{} adicionado ({})", "{} added ({})", p.name, p.url)
                    );
                    Ok(())
                }
                "remove" => {
                    peers::remove(&arg.context(numapilot::i18n::tr("faltou o nome"))?)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!("{}", numapilot::i18n::tr("removido"));
                    Ok(())
                }
                "pull" => {
                    let m = peers::pull_profiles(
                        &arg.context(numapilot::i18n::tr("faltou o nome"))?,
                        name.as_deref() == Some("todos"),
                    )
                    .map_err(|e| anyhow::anyhow!(e))?;
                    println!("{m}");
                    Ok(())
                }
                "backup" => {
                    for m in peers::backup_all() {
                        println!("{m}");
                    }
                    Ok(())
                }
                "wake" => {
                    let m = peers::wake(&arg.context(numapilot::i18n::tr("faltou o nome"))?)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!("{m}");
                    Ok(())
                }
                _ => bail!(
                    "{}",
                    numapilot::tf!(
                        "use: numapilot peers list | add LINK [NOME] | remove NOME | wake NOME | pull NOME [todos] | backup",
                        "usage: numapilot peers list | add LINK [NAME] | remove NAME | wake NAME | pull NAME [todos] | backup"
                    )
                ),
            }
        }
        Cmd::Vm {
            action,
            arg,
            gpu,
            iso,
            mem,
            cpus,
            disk,
        } => {
            use numapilot::vm;
            let cfg = Config::load()?;
            let topo = Topology::load(&cfg);
            let find = |a: &str| {
                topo.gpus
                    .iter()
                    .find(|g| g.alias == a || g.pci == a)
                    .cloned()
            };
            match action.as_str() {
                "check" => {
                    let g = arg.as_deref().or(gpu.as_deref()).and_then(find);
                    for c in vm::check(&topo, g.as_ref()) {
                        println!(
                            "[{}] {}{}{}",
                            if c.ok {
                                " ok "
                            } else {
                                numapilot::i18n::tr("falta")
                            },
                            c.name,
                            if c.detail.is_empty() {
                                String::new()
                            } else {
                                format!(": {}", c.detail)
                            },
                            match (&c.fix, c.ok) {
                                (Some(f), false) => format!("\n        → {f}"),
                                _ => String::new(),
                            }
                        );
                    }
                    if g.is_none() {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "(diga a placa para conferir ela também: numapilot vm check rx470-0)",
                                "(name the card to check it too: numapilot vm check rx470-0)"
                            )
                        );
                    }
                    Ok(())
                }
                "prepare" => {
                    let g = arg
                        .as_deref()
                        .or(gpu.as_deref())
                        .and_then(find)
                        .context(numapilot::i18n::tr("diga a placa (veja numapilot topo)"))?;
                    vm::prepare(&topo, &g, &mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))
                }
                "unprepare" => {
                    vm::unprepare(&mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))
                }
                "create" => {
                    let name = arg.context(numapilot::i18n::tr("diga o nome da VM"))?;
                    let g = gpu
                        .as_deref()
                        .and_then(find)
                        .context(numapilot::i18n::tr("--gpu PLACA"))?;
                    let iso =
                        iso.context(numapilot::i18n::tr("--iso arquivo do Windows (.iso)"))?;
                    vm::create(
                        &topo,
                        &g,
                        &vm::NewVm {
                            name: name.clone(),
                            iso,
                            mem_gb: mem,
                            cpus,
                            disk_gb: disk,
                        },
                    )
                    .map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "{}",
                        numapilot::tf!(
                            "VM {name} criada: abra no virt-manager para instalar o Windows. Depois instale o driver da placa dentro dela.",
                            "VM {name} created: open it in virt-manager to install Windows. Then install the card's driver inside it."
                        )
                    );
                    Ok(())
                }
                _ => bail!(
                    "{}",
                    numapilot::tf!(
                        "use: numapilot vm check [GPU] | sudo numapilot vm prepare GPU | sudo numapilot vm unprepare | numapilot vm create NOME --gpu GPU --iso ARQ.iso [--mem 16 --cpus 8 --disk 120]",
                        "usage: numapilot vm check [GPU] | sudo numapilot vm prepare GPU | sudo numapilot vm unprepare | numapilot vm create NAME --gpu GPU --iso FILE.iso [--mem 16 --cpus 8 --disk 120]"
                    )
                ),
            }
        }
        Cmd::Hybrid { action, arg } => {
            use numapilot::hybrid;
            let cfg = Config::load()?;
            let topo = Topology::load(&cfg);
            let act = action.as_deref().unwrap_or("status");
            match act {
                "xwayland" => {
                    let on = arg.as_deref() != Some("off");
                    hybrid::set_xwayland_wrapper(on).map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "{}",
                        numapilot::tf!(
                            "{} (vale para o próximo Xwayland: saia e entre na sessão, ou feche os programas X11)",
                            "{} (applies to the next Xwayland: log out and in, or close the X11 programs)",
                            if on {
                                numapilot::i18n::tr("Xwayland usando só o Mesa")
                            } else {
                                numapilot::i18n::tr("Xwayland normal")
                            }
                        )
                    );
                    Ok(())
                }
                "auto" | "hibrido" | "integrada" | "nao-mexer" => {
                    hybrid::remove_login_files();
                    let mut m = numapilot::machine::Machine::load();
                    m.dgpu_mode = (act != "nao-mexer").then(|| act.to_string());
                    let r =
                        numapilot::ctl::send(&numapilot::ctl::Request::SetMachine { machine: m })?;
                    if !r.ok {
                        bail!(r.message);
                    }
                    println!(
                        "{}",
                        numapilot::tf!(
                            "modo da NVIDIA: {act} (o serviço aplica em segundos, sem sair da sessão)",
                            "NVIDIA mode: {act} (the service applies it within seconds, without leaving the session)"
                        )
                    );
                    Ok(())
                }
                _ => {
                    hybrid::remove_login_files();
                    let st = hybrid::load_state();
                    match hybrid::pair(&topo) {
                        Some((i, n)) => {
                            for g in [i, n] {
                                let outs = hybrid::outputs(&g.pci);
                                println!(
                                    "{} ({}): {}",
                                    g.alias,
                                    numapilot::notebook::dgpu_state(&g.pci),
                                    outs.iter()
                                        .map(|o| format!(
                                            "{}{}",
                                            o.name,
                                            if o.connected {
                                                numapilot::i18n::tr(" [tela]")
                                            } else {
                                                ""
                                            }
                                        ))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                );
                            }
                        }
                        None if st.as_ref().is_some_and(|s| s.removed) => {
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "{}: desligada pelo numapilot (fora do sistema)",
                                    "{}: turned off by numapilot (removed from the system)",
                                    st.as_ref().map(|s| s.alias.as_str()).unwrap_or("NVIDIA")
                                )
                            );
                        }
                        None => {
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "Não é um notebook com placa integrada + NVIDIA.",
                                    "Not a laptop with an integrated card + NVIDIA."
                                )
                            );
                            return Ok(());
                        }
                    }
                    let m = numapilot::machine::Machine::load().dgpu_mode;
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Energia: {}",
                            "Power: {}",
                            match numapilot::notebook::on_ac() {
                                Some(true) => numapilot::i18n::tr("tomada"),
                                Some(false) => numapilot::i18n::tr("bateria"),
                                None => "?",
                            }
                        )
                    );
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Modo: {}",
                            "Mode: {}",
                            m.as_deref()
                                .and_then(|m| hybrid::MODES.iter().find(|x| x.0 == m))
                                .map(|x| numapilot::i18n::tr(x.1))
                                .unwrap_or(numapilot::i18n::tr("não mexer"))
                        )
                    );
                    if let Some(st) = &st {
                        if !st.blocked_by.is_empty() {
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "Esperando para desligar: em uso por {}",
                                    "Waiting to turn off: in use by {}",
                                    st.blocked_by.join(", ")
                                )
                            );
                            for n in &st.blocked_by {
                                if let Some(h) = numapilot::notebook::holder_hint(n) {
                                    println!("  · {h}");
                                }
                            }
                        }
                        if st.sleep_only && !st.want_present {
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "O KDE segura a saída de vídeo da NVIDIA: ela não é tirada do sistema, mas dorme sozinha ({}).",
                                    "KDE holds the NVIDIA's video output: it is not removed from the system, but sleeps by itself ({}).",
                                    numapilot::notebook::dgpu_state(&st.pci)
                                )
                            );
                        }
                    }
                    println!(
                        "Xwayland: {}",
                        if hybrid::xwayland_wrapped() {
                            numapilot::i18n::tr("só Mesa (não prende a NVIDIA)")
                        } else {
                            numapilot::i18n::tr("normal (pode prender a NVIDIA)")
                        }
                    );
                    Ok(())
                }
            }
        }
        Cmd::Servicos {
            action,
            unit,
            usuario,
            rodando,
            falhas,
            linhas,
        } => {
            use numapilot::services::{self, Verb};
            let all = services::list(usuario);
            let find = |u: &Option<String>| -> anyhow::Result<services::Service> {
                let u = u.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "{}",
                        numapilot::tf!("falta o nome do serviço", "the service name is missing")
                    )
                })?;
                let full = if u.contains('.') {
                    u.to_string()
                } else {
                    format!("{u}.service")
                };
                all.iter().find(|s| s.unit == full).cloned().ok_or_else(|| {
                    anyhow::anyhow!(
                        "{}",
                        numapilot::tf!(
                            "serviço {full} não encontrado{}",
                            "service {full} not found{}",
                            if usuario {
                                ""
                            } else {
                                numapilot::i18n::tr(" (do usuário? use --usuario)")
                            }
                        )
                    )
                })
            };
            let verb = match action.as_deref().unwrap_or("lista") {
                "lista" | "list" => {
                    for s in all
                        .iter()
                        .filter(|s| (!rodando || s.running()) && (!falhas || s.failed()))
                    {
                        let boot = match s.at_boot() {
                            Some(true) => "boot",
                            Some(false) => "-",
                            None => "",
                        };
                        let mem = s
                            .mem
                            .map(|m| format!("{} MB", m / 1048576))
                            .unwrap_or_default();
                        println!(
                            "{:<44} {:<14} {:<5} {:>8}  {}",
                            s.unit,
                            s.state_text(),
                            boot,
                            mem,
                            s.description
                        );
                    }
                    return Ok(());
                }
                "log" | "logs" => {
                    print!("{}", services::logs(&find(&unit)?, linhas));
                    return Ok(());
                }
                "iniciar" | "start" => Verb::Start,
                "parar" | "stop" => Verb::Stop,
                "reiniciar" | "restart" => Verb::Restart,
                o => bail!(
                    "{}",
                    numapilot::tf!("ação desconhecida: {o}", "unknown action: {o}")
                ),
            };
            let s = find(&unit)?;
            let c = services::action_command(&s, verb).map_err(|e| anyhow::anyhow!(e))?;
            let st = std::process::Command::new(&c[0]).args(&c[1..]).status()?;
            if !st.success() {
                bail!(
                    "{}",
                    numapilot::tf!(
                        "{} terminou com erro",
                        "{} finished with an error",
                        c.join(" ")
                    )
                );
            }
            println!("{}: {}", s.unit, verb.text().to_lowercase());
            Ok(())
        }
        Cmd::Inicio {
            action,
            args,
            atraso,
            config,
            sim,
        } => {
            use numapilot::startup::{self, Action};
            let items = startup::list();
            let find = |id: &str| {
                items
                    .iter()
                    .find(|i| i.id == id || i.name.eq_ignore_ascii_case(id))
                    .cloned()
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "{}",
                            numapilot::tf!(
                                "não achei \"{id}\" (veja numapilot inicio lista)",
                                "could not find \"{id}\" (see numapilot inicio lista)"
                            )
                        )
                    })
            };
            let arg = |i: usize| {
                args.get(i).cloned().ok_or_else(|| {
                    anyhow::anyhow!(
                        "{}",
                        numapilot::tf!(
                            "falta um argumento (veja numapilot inicio --help)",
                            "an argument is missing (see numapilot inicio --help)"
                        )
                    )
                })
            };
            match action.as_deref().unwrap_or("lista") {
                "lista" | "list" => {
                    let mut last = None;
                    for i in &items {
                        if last != Some(i.group) {
                            println!("\n== {} ({})", i.group.title(), i.group.about());
                            last = Some(i.group);
                        }
                        let now = match (i.running, i.mem) {
                            (true, Some(m)) => {
                                numapilot::tf!("rodando, {} MB", "running, {} MB", m / 1048576)
                            }
                            (true, None) => numapilot::i18n::tr("rodando").into(),
                            _ => String::new(),
                        };
                        println!(
                            "  [{}] {:<40} {:<10} {:<16} {}{}",
                            if i.enabled { "x" } else { " " },
                            i.name,
                            i.origin,
                            now,
                            i.id,
                            if i.protected.is_some() {
                                numapilot::i18n::tr(" (protegido)")
                            } else {
                                ""
                            }
                        );
                    }
                    Ok(())
                }
                v @ ("ligar" | "desligar" | "on" | "off") => {
                    let it = find(&arg(0)?)?;
                    let on = matches!(v, "ligar" | "on");
                    match startup::set_enabled(&it, on).map_err(|e| anyhow::anyhow!(e))? {
                        Action::Done => {
                            println!(
                                "{}: {}",
                                it.name,
                                if on {
                                    numapilot::i18n::tr("ligado")
                                } else {
                                    numapilot::i18n::tr("desligado")
                                }
                            )
                        }
                        Action::NeedsRoot(c) => {
                            let st = std::process::Command::new(&c[0]).args(&c[1..]).status()?;
                            if !st.success() {
                                bail!(
                                    "{}",
                                    numapilot::tf!(
                                        "{} terminou com erro",
                                        "{} finished with an error",
                                        c.join(" ")
                                    )
                                );
                            }
                        }
                    }
                    Ok(())
                }
                v @ ("origem" | "desinstalar") => {
                    use numapilot::origin;
                    let it = find(&arg(0)?)?;
                    let t = origin::trace(&it.exec, it.file.as_deref());
                    println!("{}", it.name);
                    if let Some(f) = &t.file {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "  abre por:   {}{}",
                                "  started by: {}{}",
                                f.display(),
                                t.file_pkg
                                    .as_ref()
                                    .map(|p| numapilot::tf!(
                                        " (do pacote {p})",
                                        " (from package {p})"
                                    ))
                                    .unwrap_or_default()
                            )
                        );
                    }
                    println!(
                        "{}",
                        numapilot::tf!("  comando:    {}", "  command:    {}", it.exec)
                    );
                    if let Some(e) = &t.exe {
                        println!(
                            "{}",
                            numapilot::tf!("  programa:   {}", "  program:    {}", e.display())
                        );
                    }
                    println!(
                        "{}",
                        numapilot::tf!("  origem:     {}", "  source:     {}", t.source.text())
                    );
                    if let Some(d) = &t.description {
                        println!(
                            "{}",
                            numapilot::tf!("  descrição:  {d}", "  description: {d}")
                        );
                    }
                    if let Some(sz) = t.size.filter(|s| *s > 0) {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "  tamanho:    {:.1} MB",
                                "  size:       {:.1} MB",
                                sz as f64 / 1048576.0
                            )
                        );
                    }
                    if let Some(d) = &t.app_dir {
                        println!(
                            "{}",
                            numapilot::tf!("  pasta:      {}", "  folder:     {}", d.display())
                        );
                    }
                    for c in &t.configs {
                        println!("  config:     {}", c.display());
                    }
                    if v == "origem" {
                        return Ok(());
                    }
                    let unit = it
                        .id
                        .strip_prefix("user:")
                        .map(|u| (u, true))
                        .or_else(|| it.id.strip_prefix("sys:").map(|u| (u, false)));
                    let plan = origin::uninstall_plan(&t, unit, config);
                    if let Some(why) = it.protected.map(String::from).or(plan.blocked.clone()) {
                        bail!(
                            "{}",
                            numapilot::tf!(
                                "não dá para desinstalar: {why}",
                                "cannot be uninstalled: {why}"
                            )
                        );
                    }
                    println!("{}", numapilot::tf!("\nVai fazer:", "\nIt will run:"));
                    for s in &plan.steps {
                        println!("  - {s}");
                    }
                    if !sim {
                        print!(
                            "{}",
                            numapilot::tf!("Continuar? [s/N] ", "Continue? [y/N] ")
                        );
                        use std::io::Write;
                        std::io::stdout().flush()?;
                        let mut a = String::new();
                        std::io::stdin().read_line(&mut a)?;
                        if !["s", "y"].contains(&a.trim().to_lowercase().as_str()) {
                            println!("{}", numapilot::tf!("Nada feito.", "Nothing done."));
                            return Ok(());
                        }
                    }
                    let (_, refused) =
                        numapilot::procctl::terminate(&it.pids, unsafe { libc::getuid() }, false);
                    if let Some(r) = refused {
                        eprintln!(
                            "{}",
                            numapilot::tf!("(não encerrei: {r})", "(not closed: {r})")
                        );
                    }
                    let st = std::process::Command::new("sh")
                        .args(["-c", &plan.script])
                        .status()?;
                    if !st.success() {
                        bail!(
                            "{}",
                            numapilot::tf!(
                                "a desinstalação terminou com erro",
                                "the uninstall finished with an error"
                            )
                        );
                    }
                    Ok(())
                }
                "remover" | "rm" => {
                    let it = find(&arg(0)?)?;
                    startup::remove(&it).map_err(|e| anyhow::anyhow!(e))?;
                    println!("{}", numapilot::tf!("{} removido", "{} removed", it.name));
                    Ok(())
                }
                "comando" => {
                    let p = startup::add_command(&arg(0)?, &args[1..].join(" "), atraso)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "{}",
                        numapilot::tf!("adicionado: {}", "added: {}", p.display())
                    );
                    Ok(())
                }
                "arquivo" => {
                    let p = startup::add_file(std::path::Path::new(&arg(0)?), atraso)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "{}",
                        numapilot::tf!("adicionado: {}", "added: {}", p.display())
                    );
                    Ok(())
                }
                "app" => {
                    let q = args.join(" ");
                    let found = startup::search_apps(&q);
                    match found.as_slice() {
                        [(n, p)] => {
                            let d =
                                startup::add_desktop(p, atraso).map_err(|e| anyhow::anyhow!(e))?;
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "{n} vai abrir no login ({})",
                                    "{n} will open at login ({})",
                                    d.display()
                                )
                            );
                        }
                        [] => bail!(
                            "{}",
                            numapilot::tf!(
                                "nenhum programa instalado com \"{q}\"",
                                "no installed program matching \"{q}\""
                            )
                        ),
                        many => {
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "Mais de um programa; seja mais específico:",
                                    "More than one program; be more specific:"
                                )
                            );
                            for (n, p) in many.iter().take(20) {
                                println!("  {n}  ({})", p.display());
                            }
                        }
                    }
                    Ok(())
                }
                o => bail!(
                    "{}",
                    numapilot::tf!("ação desconhecida: {o}", "unknown action: {o}")
                ),
            }
        }
        Cmd::Autostart { action, if_hybrid } => {
            use numapilot::autostart;
            let cfg = Config::load()?;
            let topo = Topology::load(&cfg);
            if if_hybrid && numapilot::hybrid::pair(&topo).is_none() {
                return Ok(());
            }
            match action.as_deref().unwrap_or("status") {
                "igpu" => {
                    let ch = autostart::apply(&topo).map_err(|e| anyhow::anyhow!(e))?;
                    if ch.is_empty() {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "Tudo que abre com o sistema já está na integrada.",
                                "Everything that starts with the system is already on the integrated GPU."
                            )
                        );
                    } else {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "Agora na integrada (vale a partir do próximo login): {}",
                                "Now on the integrated GPU (applies from the next login): {}",
                                ch.join(", ")
                            )
                        );
                    }
                    Ok(())
                }
                "normal" => {
                    let ch = autostart::revert();
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Voltaram ao normal: {}",
                            "Back to normal: {}",
                            if ch.is_empty() {
                                numapilot::i18n::tr("nada").into()
                            } else {
                                ch.join(", ")
                            }
                        )
                    );
                    Ok(())
                }
                _ => {
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Na integrada: {}",
                            "On the integrated GPU: {}",
                            if autostart::enabled() {
                                numapilot::i18n::tr("ligado")
                            } else {
                                numapilot::i18n::tr("desligado")
                            }
                        )
                    );
                    for e in autostart::entries() {
                        if !e.hidden {
                            println!(
                                "  {:<40} {}",
                                e.name,
                                if e.wrapped { "integrada" } else { "-" }
                            );
                        }
                    }
                    for s in autostart::user_services() {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "  {s:<40} (serviço do usuário)",
                                "  {s:<40} (user service)"
                            )
                        );
                    }
                    Ok(())
                }
            }
        }
        Cmd::SelfUpdate { action } => {
            use numapilot::selfupdate::{self, How};
            let h = selfupdate::how();
            let built = selfupdate::BUILT_COMMIT;
            if built.is_empty() {
                println!("numapilot {}", env!("CARGO_PKG_VERSION"));
            } else {
                println!(
                    "numapilot {} (commit {})",
                    env!("CARGO_PKG_VERSION"),
                    &built[..built.len().min(7)]
                );
            }
            let mut pending = false;
            match &h {
                How::Package(p) => println!(
                    "{}",
                    numapilot::tf!("instalado pelo pacote {p}", "installed from package {p}")
                ),
                How::Source(d) => {
                    println!(
                        "{}",
                        numapilot::tf!(
                            "instalado pela pasta {}",
                            "installed from the folder {}",
                            d.display()
                        )
                    );
                    let ahead = selfupdate::source_ahead_of_installed(d).unwrap_or(0);
                    if ahead > 0 {
                        pending = true;
                        println!(
                            "{}",
                            numapilot::tf!(
                                "a pasta tem {ahead} commit(s) que ainda não foram instalados",
                                "the folder has {ahead} commit(s) not installed yet"
                            )
                        );
                    }
                    match selfupdate::source_behind(d) {
                        Some(0) if ahead == 0 => println!(
                            "{}",
                            numapilot::tf!(
                                "já está na versão mais nova",
                                "already on the newest version"
                            )
                        ),
                        Some(0) => println!(
                            "{}",
                            numapilot::tf!(
                                "nada novo no repositório remoto",
                                "nothing new in the remote repository"
                            )
                        ),
                        Some(n) => {
                            pending = true;
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "{n} atualização(ões) novas no repositório",
                                    "{n} new update(s) in the repository"
                                )
                            )
                        }
                        None => println!(
                            "{}",
                            numapilot::tf!(
                                "não consegui consultar o repositório",
                                "could not query the repository"
                            )
                        ),
                    }
                }
                How::Unknown => println!(
                    "{}",
                    numapilot::tf!("origem desconhecida", "unknown source")
                ),
            }
            if pending && action.as_deref() != Some("run") {
                println!(
                    "{}",
                    numapilot::tf!(
                        "para instalar: numapilot self-update run",
                        "to install: numapilot self-update run"
                    )
                );
            }
            if action.as_deref() == Some("run") {
                let c = selfupdate::command().map_err(|e| anyhow::anyhow!(e))?;
                let st = std::process::Command::new(&c[0]).args(&c[1..]).status()?;
                if !st.success() {
                    bail!(
                        "{}",
                        numapilot::tf!(
                            "a atualização terminou com erro ({st})",
                            "the update finished with an error ({st})"
                        )
                    );
                }
            }
            Ok(())
        }
        Cmd::Undervolt {
            gpu,
            secs,
            margin,
            clock,
            step,
        } => {
            let cfg = Config::load()?;
            let topo = Topology::load(&cfg);
            let g = topo
                .gpus
                .iter()
                .find(|g| g.alias == gpu || g.pci == gpu)
                .with_context(|| {
                    numapilot::tf!(
                        "GPU '{gpu}' não encontrada (veja numapilot topo)",
                        "GPU '{gpu}' not found (see numapilot topo)"
                    )
                })?
                .clone();
            numapilot::undervolt::catch_ctrl_c();
            println!(
                "{}",
                numapilot::tf!(
                    "Deixe o PC parado durante o teste. Ctrl+C para parar e voltar ao que era.",
                    "Leave the PC idle during the test. Ctrl+C to stop and go back to what it was."
                )
            );
            let opts = numapilot::undervolt::Opts {
                secs,
                margin,
                clock,
                step,
            };
            match numapilot::undervolt::run(&topo, &g, &opts, &mut |l| println!("{l}")) {
                Ok(o) => {
                    println!(
                        "{}",
                        numapilot::tf!(
                            "\nPronto: {} ({} passo(s) estáveis).",
                            "\nDone: {} ({} stable step(s)).",
                            o.description,
                            o.steps_ok
                        )
                    );
                    Ok(())
                }
                Err(e) => bail!(e),
            }
        }
        Cmd::Driver { action } => {
            use numapilot::driver;
            let topo = Topology::load(&cfg);
            match action.as_str() {
                "status" => {
                    let s = driver::status(&topo);
                    if s.gpus.is_empty() {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "Nenhuma placa NVIDIA neste computador.",
                                "No NVIDIA card in this computer."
                            )
                        );
                        return Ok(());
                    }
                    for g in &s.gpus {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "Placa: {} ({}, id {:04x})",
                                "Card: {} ({}, id {:04x})",
                                g.1,
                                g.0,
                                g.2
                            )
                        );
                    }
                    if let Some(b) = &s.branch {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "Driver indicado: {}",
                                "Recommended driver: {}",
                                b.label()
                            )
                        );
                    }
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Carregado agora: {}",
                            "Loaded now: {}",
                            s.loaded.clone().unwrap_or("nenhum".into())
                        )
                    );
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Instalado: {}",
                            "Installed: {}",
                            s.installed
                                .clone()
                                .unwrap_or(numapilot::i18n::tr("não").into())
                        )
                    );
                    if let Some(a) = &s.available {
                        println!(
                            "{}",
                            numapilot::tf!("No repositório: {a}", "In the repository: {a}")
                        );
                    }
                    if s.nouveau {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "Usando o nouveau (driver aberto da comunidade) agora.",
                                "Using nouveau (the community's open driver) right now."
                            )
                        );
                    }
                    if s.needs_reboot {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "O driver instalado é diferente do carregado: reinicie.",
                                "The installed driver differs from the loaded one: reboot."
                            )
                        );
                    }
                    Ok(())
                }
                "install" => {
                    driver::install(&topo, &mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))
                }
                "update" => {
                    driver::update(&mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))
                }
                _ => bail!(
                    "{}",
                    numapilot::tf!(
                        "use: numapilot driver status | sudo numapilot driver install | sudo numapilot driver update",
                        "usage: numapilot driver status | sudo numapilot driver install | sudo numapilot driver update"
                    )
                ),
            }
        }
        Cmd::Backup { out } => {
            let f = numapilot::backup::create(out.as_deref()).map_err(|e| anyhow::anyhow!(e))?;
            println!(
                "{}",
                numapilot::tf!("Backup salvo em {}", "Backup saved to {}", f.display())
            );
            println!(
                "{}",
                numapilot::tf!(
                    "Copie para fora do disco do sistema (pendrive, outro disco, nuvem) antes de formatar.",
                    "Copy it off the system disk (USB stick, another disk, cloud) before formatting."
                )
            );
            println!(
                "{}",
                numapilot::tf!(
                    "Para restaurar: ./install.sh --restore {}",
                    "To restore: ./install.sh --restore {}",
                    f.display()
                )
            );
            Ok(())
        }
        Cmd::Restore { file, no_boot } => {
            numapilot::backup::restore(&file, !no_boot, &mut |m| println!("{m}"))
                .map_err(|e| anyhow::anyhow!(e))
        }
        Cmd::Web { port, bind, url } => {
            if url {
                println!(
                    "{}",
                    numapilot::web::url(port).context(numapilot::i18n::tr("sem rede"))?
                );
                return Ok(());
            }
            numapilot::web::serve(&cfg, &bind, port)
        }
        Cmd::Bench { size_mb, threads } => {
            let t = Topology::load(&cfg);
            println!(
                "{}",
                numapilot::tf!(
                    "Medindo {} nó(s); leva alguns segundos por par...",
                    "Measuring {} node(s); takes a few seconds per pair...",
                    t.nodes.len()
                )
            );
            let r = numapilot::bench::run(&t, size_mb, threads, |m| println!("  {m}"));
            numapilot::bench::print(&r);
            if let Some(p) = numapilot::bench::save(&r) {
                println!(
                    "{}",
                    numapilot::tf!(
                        "\nResultado salvo em {} (aparece na interface).",
                        "\nResult saved to {} (shows up in the interface).",
                        p.display()
                    )
                );
            }
            Ok(())
        }
        Cmd::Daemon => daemon::run(cfg),
        Cmd::Status => cmd_status(),
        Cmd::Pin {
            pid,
            cpu,
            no_migrate,
        } => cmd_pin(&cfg, pid, &cpu, no_migrate),
        Cmd::Learned { forget } => cmd_learned(forget),
        Cmd::RocrIds => {
            let t = Topology::load(&cfg);
            let ids: Vec<String> = t
                .compute_gpus()
                .iter()
                .filter_map(|g| g.rocr_id.clone())
                .collect();
            println!("{}", ids.join(","));
            Ok(())
        }
    }
}

fn gib(kb: u64) -> String {
    format!("{:.1} GiB", kb as f64 / 1024.0 / 1024.0)
}

fn cmd_topo(cfg: &Config) -> Result<()> {
    let t = Topology::load(cfg);
    println!("{}", numapilot::tf!("Nós NUMA", "NUMA nodes"));
    for n in &t.nodes {
        println!(
            "{}",
            numapilot::tf!(
                "  nó {}  socket(s) {:?}  CPUs {}  ({} lógicos / {} físicos)  RAM {} (livre {})  distâncias {:?}",
                "  node {}  socket(s) {:?}  CPUs {}  ({} logical / {} physical)  RAM {} (free {})  distances {:?}",
                n.id,
                n.sockets,
                format_cpulist(&n.cpus),
                n.cpus.len(),
                n.physical_cores,
                gib(n.mem_total_kb),
                gib(n.mem_free_kb()),
                n.distances
            )
        );
    }
    println!("\nGPUs");
    for g in &t.gpus {
        let role = match g.role {
            GpuRole::Display => numapilot::i18n::tr("vídeo (monitor ligado)"),
            GpuRole::Compute => numapilot::i18n::tr("computação (ROCm)"),
            GpuRole::Other => numapilot::i18n::tr("outra"),
        };
        let vram = g
            .vram_used_total()
            .map(|(u, tt)| {
                format!(
                    "{:.1}/{:.1} GiB",
                    u as f64 / 1073741824.0,
                    tt as f64 / 1073741824.0
                )
            })
            .unwrap_or_else(|| "-".into());
        println!(
            "{}",
            numapilot::tf!(
                "  {:<10} {}  {}  [{}]\n             nó {}  {}  {}  ocupação {}  VRAM {}  ROCR {}",
                "  {:<10} {}  {}  [{}]\n             node {}  {}  {}  load {}  VRAM {}  ROCR {}",
                g.alias,
                g.pci,
                g.name,
                role,
                g.numa_node
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".into()),
                g.gfx.as_deref().unwrap_or("-"),
                g.render.as_deref().unwrap_or("-"),
                g.busy_percent()
                    .map(|b| format!("{b}%"))
                    .unwrap_or_else(|| "-".into()),
                vram,
                g.rocr_id.as_deref().unwrap_or("-"),
            )
        );
    }
    let w = t.warnings();
    if !w.is_empty() {
        println!();
        for x in w {
            println!("{}", numapilot::tf!("aviso: {x}", "warning: {x}"));
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_run(
    cfg: &Config,
    cpu: Option<String>,
    gpu: Option<String>,
    mem: Option<String>,
    usage: Option<Kind>,
    fg_gpu: Option<String>,
    dry_run: bool,
    command: Vec<String>,
) -> Result<()> {
    let topo = Topology::load(cfg);
    let learned = Learned::load();
    let mut profiles = Profiles::load();
    let prog = command
        .first()
        .context(numapilot::i18n::tr("faltou o programa"))?;
    let exe = which(prog);
    let idents = procfs::idents(basename(prog), exe.as_deref(), &command);
    let key = procfs::learn_key(exe.as_deref(), &command, basename(prog));
    let steam = std::env::var("SteamAppId").ok();
    let pkey = program_key(steam.as_deref(), &key);
    // Otimizador de FPS: nesta abertura usa a próxima configuração da lista.
    let tuning = numapilot::tuner::Tuner::load()
        .sessions
        .get(&pkey)
        .filter(|s| !s.done())
        .and_then(|s| {
            s.current()
                .cloned()
                .map(|v| (s.next, s.variants.len(), s.seconds, v))
        });
    if let Some((i, n, secs, v)) = &tuning {
        eprintln!(
            "{}",
            numapilot::tf!(
                "numapilot: otimizador de FPS, teste {}/{n}: {} (jogue ~{} s numa cena parecida)",
                "numapilot: FPS optimizer, test {}/{n}: {} (play ~{} s in a similar scene)",
                i + 1,
                v.label,
                secs + 10
            )
        );
        profiles.map.insert(pkey.clone(), v.profile.clone());
    }
    let loads = sample_node_loads(&topo, Duration::from_millis(200));
    let inp = LaunchInput {
        idents: &idents,
        learn_key: &key,
        key: &pkey,
        cpu: cpu.as_deref().map(CpuSpec::parse).transpose()?,
        gpu: gpu.as_deref().map(GpuSpec::parse).transpose()?,
        mem: mem.as_deref().map(MemSpec::parse).transpose()?,
        usage,
        fg_gpu,
    };
    let d = decide_launch(cfg, &topo, &learned, &profiles, &loads, &inp)?;
    let (mut set, unset) = gpu_env(&d, &topo);
    set.extend(numapilot::decide::dx_filter(&d, &topo, &set, &unset));

    if dry_run || std::env::var_os("NUMAPILOT_VERBOSE").is_some() {
        eprintln!(
            "{}",
            numapilot::tf!("programa : {}", "program  : {}", command.join(" "))
        );
        eprintln!(
            "{}",
            numapilot::tf!("perfil   : {pkey}", "profile  : {pkey}")
        );
        eprintln!(
            "{}",
            numapilot::tf!(
                "modo     : {}",
                "mode     : {}",
                if d.manual {
                    "manual"
                } else {
                    numapilot::i18n::tr("automático")
                }
            )
        );
        eprintln!(
            "{}",
            numapilot::tf!(
                "nós      : {:?}  CPUs {}",
                "nodes    : {:?}  CPUs {}",
                d.nodes,
                format_cpulist(&d.cpus)
            )
        );
        eprintln!(
            "{}",
            numapilot::tf!("memória  : {:?}", "memory   : {:?}", d.mem)
        );
        eprintln!(
            "{}",
            numapilot::tf!(
                "prioridade: {}{}",
                "priority  : {}{}",
                d.priority,
                if d.needs_daemon {
                    numapilot::i18n::tr(" (aplicada pelo serviço)")
                } else {
                    ""
                }
            )
        );
        eprintln!(
            "{}",
            numapilot::tf!(
                "uso GPU  : {}",
                "GPU use  : {}",
                match d.kind {
                    Kind::Graphics => numapilot::i18n::tr("gráfico (jogo/vídeo)"),
                    Kind::Compute => numapilot::i18n::tr("computação (ROCm)"),
                    Kind::NoGpu => "-",
                }
            )
        );
        eprintln!(
            "GPU      : {}",
            if d.gpus.is_empty() {
                numapilot::i18n::tr("nenhuma").to_string()
            } else {
                d.gpus
                    .iter()
                    .map(|g| format!("{} ({})", g.alias, g.pci))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        for r in &d.reasons {
            eprintln!("{}", numapilot::tf!("motivo   : {r}", "reason   : {r}"));
        }
        for w in numapilot::decide::game_warnings(&topo, &d) {
            eprintln!("{}", numapilot::tf!("atenção  : {w}", "warning  : {w}"));
        }
        for (k, v) in &set {
            eprintln!("env      : {k}={v}");
        }
        for k in &unset {
            eprintln!("env      : unset {k}");
        }
        if dry_run {
            return Ok(());
        }
    }

    // O que só o root faz (cgroup: memória/núcleos exclusivos; prioridade alta): pede ao serviço
    // para aplicar neste processo antes do exec — vale também para o programa.
    if d.needs_daemon {
        let me = std::process::id() as i32;
        let profile = tuning.as_ref().map(|t| t.3.profile.clone());
        // a chave do programa vai junto: neste momento o processo ainda é o numapilot, e o
        // serviço precisa saber de quem é o perfil e o grupo de controle
        match ctl::send(&ctl::Request::Adopt {
            pid: me,
            profile,
            key: Some(pkey.clone()),
            label: Some(basename(prog).to_string()),
        }) {
            Ok(r) if r.ok => {}
            Ok(r) => eprintln!(
                "{}",
                numapilot::tf!("numapilot: aviso: {}", "numapilot: warning: {}", r.message)
            ),
            Err(e) => {
                eprintln!(
                    "{}",
                    numapilot::tf!(
                        "numapilot: aviso: limites de memória/prioridade não aplicados ({e:#})",
                        "numapilot: warning: memory/priority limits not applied ({e:#})"
                    )
                )
            }
        }
    }
    if d.priority == "low" {
        let (nice, io) = procfs::priority_values("low");
        procfs::set_nice_all(std::process::id() as i32, nice);
        procfs::set_ioprio_all(std::process::id() as i32, io);
    }
    procfs::set_affinity(0, &d.cpus).context("sched_setaffinity")?;
    let wide = d.nodes.len() > 1;
    let pol = match d.mem {
        MemSpec::Local if wide => (procfs::MPOL_DEFAULT, vec![]),
        MemSpec::Local => (procfs::MPOL_PREFERRED, d.nodes.clone()),
        MemSpec::Interleave => (procfs::MPOL_INTERLEAVE, d.nodes.clone()),
        MemSpec::Bind => (procfs::MPOL_BIND, d.nodes.clone()),
    };
    if let Err(e) = procfs::set_mempolicy(pol.0, &pol.1) {
        eprintln!(
            "{}",
            numapilot::tf!(
                "numapilot: aviso: set_mempolicy falhou ({e}); seguindo só com a afinidade de CPU",
                "numapilot: warning: set_mempolicy failed ({e}); going on with CPU affinity only"
            )
        );
    }
    // MangoHud (medição) e gamescope envolvem o comando.
    let mut argv = command.clone();
    if tuning.is_some() && which("mangohud").is_some() && basename(&argv[0]) != "mangohud" {
        argv.insert(0, "mangohud".into());
    }
    if let Some(g) = profiles.get(&pkey).and_then(|p| p.gamescope.clone())
        && std::env::var_os("GAMESCOPE_WAYLAND_DISPLAY").is_none()
        && basename(&command[0]) != "gamescope"
    {
        if which("gamescope").is_some() {
            let mut a = g.argv();
            a.push("--".into());
            a.append(&mut argv);
            argv = a;
        } else {
            eprintln!(
                "{}",
                numapilot::tf!(
                    "numapilot: aviso: gamescope não instalado (sudo pacman -S gamescope); abrindo sem ele",
                    "numapilot: warning: gamescope not installed (sudo pacman -S gamescope); opening without it"
                )
            );
        }
    }
    let mut c = std::process::Command::new(&argv[0]);
    c.args(&argv[1..]);
    for (k, v) in set {
        if numapilot::decide::SOFT_THREAD_VARS.contains(&k.as_str())
            && std::env::var_os(&k).is_some()
        {
            continue;
        }
        c.env(k, v);
    }
    if d.limit_visible {
        let mut jto = std::env::var("JAVA_TOOL_OPTIONS").unwrap_or_default();
        if !jto.contains("ActiveProcessorCount") {
            jto = format!("{jto} -XX:ActiveProcessorCount={}", d.cpus.len())
                .trim()
                .to_string();
            c.env("JAVA_TOOL_OPTIONS", jto);
        }
    }
    for k in unset {
        c.env_remove(k);
    }
    if let Some(u) = profiles
        .get(&pkey)
        .and_then(|p| p.upscale.clone())
        .filter(|u| u.mode != "nenhum" && !u.mode.is_empty())
    {
        if u.mode == "optiscaler"
            && let Some(id) = pkey.strip_prefix("steam:")
            && numapilot::upscale::steam_game_root(id)
                .and_then(|r| numapilot::upscale::exe_dir(&r))
                .and_then(|d| numapilot::upscale::installed(&d))
                .is_none()
        {
            eprintln!(
                "{}",
                numapilot::tf!(
                    "numapilot: aviso: OptiScaler escolhido mas não instalado na pasta do jogo (Ajustar… → Upscaling → Instalar)",
                    "numapilot: warning: OptiScaler chosen but not installed in the game's folder (Adjust… → Upscaling → Install)"
                )
            );
        }
        for (k, v) in numapilot::upscale::env(&u) {
            c.env(k, v);
        }
    }
    if let Some((i, _, secs, _)) = tuning {
        for (k, v) in numapilot::tuner::mangohud_env(&pkey, secs) {
            c.env(k, v);
        }
        let start = numapilot::history::now();
        let st = c.status().with_context(|| {
            numapilot::tf!("não consegui executar '{}'", "could not run '{}'", argv[0])
        })?;
        let dir = numapilot::tuner::log_dir(&pkey);
        let res = numapilot::tuner::newest_csv(&dir, start)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| numapilot::tuner::parse_csv(&s));
        let mut t = numapilot::tuner::Tuner::load();
        match (res, t.sessions.get_mut(&pkey)) {
            (Some(r), Some(s)) if s.next == i => {
                eprintln!(
                    "{}",
                    numapilot::tf!(
                        "numapilot: teste {}: {:.1} FPS médio, 1% low {:.1}",
                        "numapilot: test {}: {:.1} average FPS, 1% low {:.1}",
                        i + 1,
                        r.avg,
                        r.low1
                    )
                );
                s.variants[i].result = Some(r);
                s.next += 1;
                let _ = t.save();
            }
            (None, _) => eprintln!(
                "{}",
                numapilot::tf!(
                    "numapilot: o MangoHud não gravou este teste (jogou menos de ~{} s? MangoHud instalado?). Ele se repete na próxima abertura.",
                    "numapilot: MangoHud did not record this test (played less than ~{} s? MangoHud installed?). It repeats on the next start.",
                    secs + 10
                )
            ),
            _ => {}
        }
        std::process::exit(st.code().unwrap_or(1));
    }
    let err = c.exec();
    bail!(
        "{}",
        numapilot::tf!(
            "não consegui executar '{}': {err}",
            "could not run '{}': {err}",
            argv[0]
        )
    )
}

fn cmd_profile(action: ProfileCmd) -> Result<()> {
    match action {
        ProfileCmd::List => {
            let p = Profiles::load();
            if p.map.is_empty() {
                println!(
                    "{}",
                    numapilot::tf!(
                        "Nenhum perfil salvo: tudo no automático.",
                        "No saved profile: everything on automatic."
                    )
                );
            }
            for (k, v) in &p.map {
                println!(
                    "{}",
                    numapilot::tf!(
                        "{k}\n    cpu={}  gpu={}  uso={}",
                        "{k}\n    cpu={}  gpu={}  use={}",
                        v.cpu,
                        v.gpu,
                        v.usage
                    )
                );
            }
            Ok(())
        }
        ProfileCmd::Set {
            key,
            cpu,
            gpu,
            usage,
        } => {
            let profile = Profile {
                label: basename(&key).to_string(),
                cpu,
                gpu,
                usage,
                ..Default::default()
            };
            let r = ctl::send(&ctl::Request::SetProfile {
                key,
                profile,
                pid: None,
            })?;
            println!("{}", r.message);
            if r.ok { Ok(()) } else { bail!("falhou") }
        }
        ProfileCmd::Rm { key } => {
            let r = ctl::send(&ctl::Request::DeleteProfile { key, pid: None })?;
            println!("{}", r.message);
            if r.ok { Ok(()) } else { bail!("falhou") }
        }
        ProfileCmd::Export { file } => {
            let f = file.unwrap_or_else(numapilot::profiles::export_path);
            std::fs::write(&f, numapilot::profiles::export_text())?;
            println!(
                "{}",
                numapilot::tf!(
                    "Escolhas gravadas em {}",
                    "Choices written to {}",
                    f.display()
                )
            );
            Ok(())
        }
        ProfileCmd::Import { file } => {
            let cfg = Config::load().unwrap_or_default();
            let topo = Topology::load(&cfg);
            let aliases: Vec<String> = topo.gpus.iter().map(|g| g.alias.clone()).collect();
            let text = std::fs::read_to_string(&file)?;
            let (ok, bad) = numapilot::profiles::import_text(&text, &aliases)
                .map_err(|e| anyhow::anyhow!(e))?;
            let mut n = 0;
            for (key, profile) in ok {
                let r = ctl::send(&ctl::Request::SetProfile {
                    key: key.clone(),
                    profile,
                    pid: None,
                })?;
                if r.ok {
                    n += 1;
                } else {
                    println!("{key}: {}", r.message);
                }
            }
            for b in &bad {
                println!("{}", numapilot::tf!("ignorado: {b}", "ignored: {b}"));
            }
            println!(
                "{}",
                numapilot::tf!("{n} escolha(s) importada(s).", "{n} choice(s) imported.")
            );
            Ok(())
        }
    }
}

fn cmd_tune(cfg: &Config, action: &str, key: Option<String>) -> Result<()> {
    use numapilot::tuner::{self, Tuner};
    let mut t = Tuner::load();
    match (action, key) {
        ("start", Some(k)) => {
            let base = Profiles::load().get(&k).cloned().unwrap_or_default();
            let topo = Topology::load(cfg);
            let v = tuner::variants(&topo, &base);
            let label = if base.label.is_empty() {
                k.clone()
            } else {
                base.label.clone()
            };
            println!(
                "{}",
                numapilot::tf!(
                    "{} configurações a testar em {label}:",
                    "{} settings to test in {label}:",
                    v.len()
                )
            );
            for (i, x) in v.iter().enumerate() {
                println!("  {}. {}", i + 1, x.label);
            }
            t.start(&k, &label, v);
            t.save()?;
            println!(
                "{}",
                numapilot::tf!(
                    "Abra o jogo {} vezes (pelo numapilot run ou pela Steam com o numapilot) e jogue ~70 s em cada.",
                    "Open the game {} times (through numapilot run or through Steam with numapilot) and play ~70 s each time.",
                    t.sessions[&k].variants.len()
                )
            );
            if which("mangohud").is_none() {
                println!(
                    "{}",
                    numapilot::tf!(
                        "Atenção: instale o MangoHud (sudo pacman -S mangohud), ele faz a medição.",
                        "Warning: install MangoHud (sudo pacman -S mangohud), it does the measuring."
                    )
                );
            }
            Ok(())
        }
        ("status", k) => {
            for s in t
                .sessions
                .values()
                .filter(|s| k.as_deref().is_none_or(|k| k == s.key))
            {
                println!(
                    "{}",
                    numapilot::tf!(
                        "{} ({}) — {}/{} testados",
                        "{} ({}) — {}/{} tested",
                        s.label,
                        s.key,
                        s.variants.iter().filter(|v| v.result.is_some()).count(),
                        s.variants.len()
                    )
                );
                let best = s.best();
                for (i, v) in s.variants.iter().enumerate() {
                    let r = v
                        .result
                        .as_ref()
                        .map(|r| {
                            format!(
                                "{:6.1} FPS  1% low {:6.1}  0,1% {:6.1}",
                                r.avg, r.low1, r.low01
                            )
                        })
                        .unwrap_or_else(|| "      —".into());
                    println!(
                        "  {} {}. {:<45} {r}",
                        if best == Some(i) {
                            "*"
                        } else if i == s.next {
                            ">"
                        } else {
                            " "
                        },
                        i + 1,
                        v.label
                    );
                }
            }
            Ok(())
        }
        ("stop", Some(k)) => {
            t.sessions.remove(&k);
            t.save()?;
            println!(
                "{}",
                numapilot::tf!(
                    "Otimização de {k} cancelada.",
                    "Optimization of {k} cancelled."
                )
            );
            Ok(())
        }
        ("apply", Some(k)) => {
            let s = t
                .sessions
                .get(&k)
                .context(numapilot::i18n::tr("não há otimização para esse programa"))?;
            let b = s
                .best()
                .context(numapilot::i18n::tr("nenhum teste terminou ainda"))?;
            let mut profile = s.variants[b].profile.clone();
            profile.label = s.label.clone();
            let r = ctl::send(&ctl::Request::SetProfile {
                key: k.clone(),
                profile,
                pid: None,
            })?;
            println!(
                "{}",
                numapilot::tf!(
                    "Usando \"{}\": {}",
                    "Using \"{}\": {}",
                    s.variants[b].label,
                    r.message
                )
            );
            Ok(())
        }
        _ => bail!(
            "{}",
            numapilot::tf!(
                "use: numapilot tune start|stop|apply CHAVE  ou  numapilot tune status [CHAVE]",
                "usage: numapilot tune start|stop|apply KEY  or  numapilot tune status [KEY]"
            )
        ),
    }
}

fn cmd_machine(cfg: &Config, action: &str, value: Option<&str>) -> Result<()> {
    use numapilot::{cpuctl, machine::Machine};
    let topo = Topology::load(cfg);
    match (action, value) {
        ("status", _) => {
            let m = Machine::load();
            println!("{}", numapilot::tf!("Modo: {}", "Mode: {}", m.mode));
            println!(
                "Turbo: {}",
                cpuctl::turbo()
                    .map(|t| if t {
                        numapilot::i18n::tr("ligado")
                    } else {
                        numapilot::i18n::tr("desligado")
                    })
                    .unwrap_or("?")
            );
            let hw: serde_json::Value =
                std::fs::read_to_string(numapilot::learned::run_dir().join("hw.json"))
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
                    .unwrap_or_default();
            let temps = cpuctl::package_temps();
            let unc = cpuctl::uncores();
            let rapl = cpuctl::rapl_zones();
            for n in &topo.nodes {
                let pkg = n.sockets.iter().next().copied().unwrap_or(n.id as u32);
                let c = n.cpus.first().copied().unwrap_or(0);
                let f = cpuctl::freq(c).unwrap_or_default();
                println!(
                    "{}",
                    numapilot::tf!(
                        "Socket {}: governor {} · {} · {}-{} MHz (agora {}) · uncore {} · {} · limite {} · {}",
                        "Socket {}: governor {} · {} · {}-{} MHz (now {}) · uncore {} · {} · limit {} · {}",
                        n.id,
                        cpuctl::governor(c).unwrap_or("?".into()),
                        match (cpuctl::epp(c), cpuctl::epb(c)) {
                            (Some(p), _) => format!("EPP {p}"),
                            (None, Some(b)) => format!("EPB {b}"),
                            _ => "EPB ?".into(),
                        },
                        f.min_khz / 1000,
                        f.max_khz / 1000,
                        f.cur_khz / 1000,
                        unc.iter()
                            .find(|u| u.package == pkg)
                            .map(|u| format!("{}-{} MHz", u.min_khz / 1000, u.max_khz / 1000))
                            .unwrap_or(numapilot::i18n::tr("indisponível").into()),
                        temps
                            .get(&pkg)
                            .map(|t| format!("{t:.0} °C"))
                            .unwrap_or("-".into()),
                        rapl.iter()
                            .find(|r| r.package == pkg)
                            .and_then(|r| r.limit_w)
                            .map(|w| format!("{w:.0} W"))
                            .unwrap_or("-".into()),
                        hw["power"][pkg.to_string()]
                            .as_f64()
                            .map(|w| numapilot::tf!("consumo {w:.0} W", "power draw {w:.0} W"))
                            .unwrap_or_default(),
                    )
                );
            }
            println!(
                "{}",
                numapilot::tf!(
                    "Estados de economia (cpu0): {}",
                    "Idle states (cpu0): {}",
                    cpuctl::cstates(0)
                        .iter()
                        .map(|s| format!(
                            "{} {}µs{}",
                            s.name,
                            s.latency_us,
                            if s.disabled {
                                numapilot::i18n::tr(" (desligado)")
                            } else {
                                ""
                            }
                        ))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            );
            if let Some(l) = hw["lowlat"].as_str().filter(|s| !s.is_empty()) {
                println!(
                    "{}",
                    numapilot::tf!(
                        "Baixa latência agora nos núcleos {l}",
                        "Low latency now on cores {l}"
                    )
                );
            }
            println!("THP: {}", numapilot::machine::thp().unwrap_or("?".into()));
            if let Some((cur, list)) = numapilot::machine::scx_status() {
                println!(
                    "{}",
                    numapilot::tf!(
                        "Escalonador: {} (disponíveis: {})",
                        "Scheduler: {} (available: {})",
                        if cur.is_empty() {
                            numapilot::i18n::tr("padrão do kernel").into()
                        } else {
                            cur
                        },
                        list.join(", ")
                    )
                );
            }
            Ok(())
        }
        ("mode", Some(name @ ("jogo" | "ia" | "silencioso" | "normal"))) => {
            let game_node = cfg
                .graphics_gpu
                .as_deref()
                .and_then(|a| topo.gpus.iter().find(|g| g.alias == a || g.pci == a))
                .or_else(|| topo.display_gpu())
                .and_then(|g| g.numa_node);
            let m = Machine::preset(name, &topo, game_node, &Machine::load());
            let r = ctl::send(&ctl::Request::SetMachine { machine: m })?;
            println!("{}", r.message);
            if r.ok { Ok(()) } else { bail!("falhou") }
        }
        _ => bail!(
            "{}",
            numapilot::tf!(
                "use: numapilot machine status  |  numapilot machine mode jogo|ia|silencioso|normal",
                "usage: numapilot machine status  |  numapilot machine mode jogo|ia|silencioso|normal"
            )
        ),
    }
}

fn cmd_boot(what: &str, args: &[String]) -> Result<()> {
    use numapilot::bootcfg;
    let value = args.first().map(String::as_str);
    match (what, value) {
        ("cmdline", None | Some("list")) => {
            use numapilot::cmdline;
            match cmdline::configured() {
                Some((l, p)) => println!(
                    "{}",
                    numapilot::tf!(
                        "Bootloader: {} ({})\nConfigurado: {p}",
                        "Bootloader: {} ({})\nConfigured: {p}",
                        l.name(),
                        l.path.display()
                    )
                ),
                None => println!(
                    "{}",
                    numapilot::tf!("Bootloader não reconhecido", "Bootloader not recognized")
                ),
            }
            println!(
                "{}",
                numapilot::tf!("Em uso agora: {}", "In use now: {}", cmdline::running())
            );
            println!(
                "{}",
                numapilot::tf!("\nParâmetros conhecidos:", "\nKnown parameters:")
            );
            for (p, t, d) in cmdline::KNOWN {
                println!(
                    "  {p:<26} {}: {}",
                    numapilot::i18n::tr(t),
                    numapilot::i18n::tr(d)
                );
            }
            let v = cmdline::vulnerabilities();
            println!(
                "{}",
                numapilot::tf!(
                    "\nMitigações que se aplicam a este processador:",
                    "\nMitigations that apply to this processor:"
                )
            );
            for m in cmdline::relevant_mitigations(&v) {
                println!(
                    "{}",
                    numapilot::tf!(
                        "  {:<44} risco {} · ganho {} · {}",
                        "  {:<44} risk {} · gain {} · {}",
                        numapilot::i18n::tr(m.title),
                        numapilot::i18n::tr(m.risk),
                        numapilot::i18n::tr(m.gain),
                        m.params.join(" ")
                    )
                );
            }
            println!(
                "{}",
                numapilot::tf!(
                    "  (tudo de uma vez: mitigations=off)",
                    "  (all at once: mitigations=off)"
                )
            );
            Ok(())
        }
        ("cmdline", Some("apply")) => {
            // +param liga, -param tira (vários de uma vez, um único backup e uma regravação)
            let mut add = vec![];
            let mut rm = vec![];
            for a in &args[1..] {
                match a.split_at(1) {
                    ("+", p) => add.push(p.to_string()),
                    ("-", p) => rm.push(p.to_string()),
                    _ => bail!(
                        "{}",
                        numapilot::tf!(
                            "use +PARAM para adicionar e -PARAM para tirar (recebido {a})",
                            "use +PARAM to add and -PARAM to remove (got {a})"
                        )
                    ),
                }
            }
            numapilot::cmdline::apply(&add, &rm, &mut |m| println!("{m}"))
                .map_err(|e| anyhow::anyhow!(e))
        }
        ("cmdline", Some(op @ ("add" | "remove"))) => {
            let ps: Vec<String> = args[1..].to_vec();
            if ps.is_empty() {
                bail!(
                    "{}",
                    numapilot::tf!("faltou o parâmetro", "the parameter is missing")
                );
            }
            let (add, rm) = if op == "add" {
                (ps, vec![])
            } else {
                (vec![], ps)
            };
            numapilot::cmdline::apply(&add, &rm, &mut |m| println!("{m}"))
                .map_err(|e| anyhow::anyhow!(e))
        }
        ("status", _) => {
            let s = bootcfg::overdrive_state();
            println!(
                "{}",
                numapilot::tf!(
                    "OverDrive (clock/voltagem das placas AMD): {} agora, {} depois de reiniciar{}",
                    "OverDrive (clock/voltage of the AMD cards): {} now, {} after a reboot{}",
                    if s.active {
                        numapilot::i18n::tr("ligado")
                    } else {
                        numapilot::i18n::tr("desligado")
                    },
                    if s.on_next_boot {
                        numapilot::i18n::tr("ligado")
                    } else {
                        numapilot::i18n::tr("desligado")
                    },
                    if s.from_cmdline {
                        numapilot::i18n::tr(" (definido na linha do kernel)")
                    } else {
                        ""
                    }
                )
            );
            let gib = |p: u64| p as f64 * 4096.0 / 1073741824.0;
            println!(
                "{}",
                numapilot::tf!(
                    "Memória da GPU integrada (ttm): {} agora, {} no boot",
                    "Integrated GPU memory (ttm): {} now, {} at boot",
                    bootcfg::ttm_now()
                        .map(|p| format!("{:.1} GiB", gib(p)))
                        .unwrap_or("?".into()),
                    bootcfg::saved_ttm()
                        .map(|p| format!("{:.1} GiB", gib(p)))
                        .unwrap_or(numapilot::i18n::tr("padrão (metade da RAM)").into())
                )
            );
            println!(
                "{}",
                numapilot::tf!("Arquivo: {}", "File: {}", bootcfg::conf_path().display())
            );
            match bootcfg::initramfs_tool() {
                Some(t) => println!(
                    "{}",
                    numapilot::tf!(
                        "Imagem de boot refeita com: {}",
                        "Boot image rebuilt with: {}",
                        t.join(" ")
                    )
                ),
                None => println!(
                    "{}",
                    numapilot::tf!(
                        "Nenhuma ferramenta de initramfs encontrada",
                        "No initramfs tool found"
                    )
                ),
            }
            Ok(())
        }
        ("overdrive", Some(v @ ("on" | "off"))) => {
            bootcfg::set_overdrive(v == "on", &mut |m| println!("{m}"))
                .map_err(|e| anyhow::anyhow!(e))
        }
        ("gpu-memory", Some(v)) => {
            let gib = if v == "padrao" || v == "default" || v == "off" {
                None
            } else {
                Some(
                    v.trim_end_matches(['G', 'g'])
                        .replace(',', ".")
                        .parse::<f64>()
                        .context(numapilot::i18n::tr(
                            "use um número em GiB (ex.: 14) ou padrao",
                        ))?,
                )
            };
            bootcfg::set_gpu_memory(gib, &mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))
        }
        _ => bail!(
            "{}",
            numapilot::tf!(
                "use: numapilot boot status | sudo numapilot boot overdrive on|off | sudo numapilot boot gpu-memory GiB|padrao | numapilot boot cmdline [add|remove PARAM...]",
                "usage: numapilot boot status | sudo numapilot boot overdrive on|off | sudo numapilot boot gpu-memory GiB|padrao | numapilot boot cmdline [add|remove PARAM...]"
            )
        ),
    }
}

fn cmd_steam(action: &str) -> Result<()> {
    use numapilot::steam;
    let games = steam::installed_games();
    let lc = steam::find_localconfigs();
    let Some(path) = lc.first() else {
        bail!(
            "{}",
            numapilot::tf!(
                "não achei a configuração da Steam (localconfig.vdf)",
                "could not find the Steam configuration (localconfig.vdf)"
            )
        )
    };
    match action {
        "list" => {
            let opts = steam::read_launch_options(path).unwrap_or_default();
            for (id, name) in &games {
                let o = opts.get(id).cloned().unwrap_or_default();
                println!(
                    "{} {id:>8}  {name}  {}",
                    if steam::has_numapilot(&o) {
                        "[x]"
                    } else {
                        "[ ]"
                    },
                    o
                );
            }
            Ok(())
        }
        "enable" | "disable" => {
            let n = steam::apply_all(path, &games, action == "enable")
                .map_err(|e| anyhow::anyhow!(e))?;
            println!(
                "{}",
                numapilot::tf!(
                    "{n} jogo(s) alterado(s) em {} (backup ao lado).",
                    "{n} game(s) changed in {} (backup next to it).",
                    path.display()
                )
            );
            Ok(())
        }
        _ => bail!(
            "{}",
            numapilot::tf!(
                "use: numapilot steam enable | disable | list",
                "usage: numapilot steam enable | disable | list"
            )
        ),
    }
}

fn which(prog: &str) -> Option<String> {
    if prog.contains('/') {
        return std::fs::canonicalize(prog)
            .ok()
            .map(|p| p.to_string_lossy().to_string());
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(prog))
        .find(|p| p.is_file())
        .and_then(|p| std::fs::canonicalize(p).ok())
        .map(|p| p.to_string_lossy().to_string())
}

fn cmd_status() -> Result<()> {
    let s = std::fs::read_to_string(daemon::state_path()).context(numapilot::i18n::tr(
        "o daemon não parece estar rodando (systemctl status numapilot)",
    ))?;
    let rows: Vec<daemon::Row> = serde_json::from_str(&s)?;
    println!(
        "{}",
        numapilot::tf!(
            "{:>7}  {:<16} {:>6}  {:<12} {:<6} {:<10} {:<14} origem",
            "{:>7}  {:<16} {:>6}  {:<12} {:<6} {:<10} {:<14} source",
            "PID",
            numapilot::i18n::tr("PROGRAMA"),
            numapilot::i18n::tr("NÚCL."),
            numapilot::i18n::tr("ESTADO"),
            numapilot::i18n::tr("NÓS"),
            "GPU",
            "CPUs"
        )
    );
    for r in rows {
        println!(
            "{:>7}  {:<16} {:>6.1}  {:<12} {:<6} {:<10} {:<14} {}",
            r.pid, r.comm, r.cores, r.state, r.nodes, r.gpus, r.cpus, r.origin
        );
    }
    Ok(())
}

fn cmd_pin(cfg: &Config, pid: i32, cpu: &str, no_migrate: bool) -> Result<()> {
    let topo = Topology::load(cfg);
    let nodes = match CpuSpec::parse(cpu)? {
        CpuSpec::All => topo.node_ids(),
        CpuSpec::Nodes(n) => n,
        _ => bail!(
            "{}",
            numapilot::tf!(
                "em `pin` use um número de nó, uma lista (0,1) ou all",
                "in `pin` use a node number, a list (0,1) or all"
            )
        ),
    };
    for n in &nodes {
        if topo.node(*n).is_none() {
            bail!(
                "{}",
                numapilot::tf!("nó {n} não existe", "node {n} does not exist")
            );
        }
    }
    let st = procfs::read_stat(pid).context(numapilot::i18n::tr("processo não encontrado"))?;
    let cpus = topo.cpus_of(&nodes);
    let ok = procfs::set_affinity_all(pid, &cpus);
    if ok == 0 {
        bail!(
            "{}",
            numapilot::tf!(
                "sem permissão (use sudo para processos de outros usuários)",
                "no permission (use sudo for other users' processes)"
            )
        );
    }
    println!(
        "{}",
        numapilot::tf!(
            "{} (pid {pid}): {ok} threads → nó(s) {:?} [{}]",
            "{} (pid {pid}): {ok} threads → node(s) {:?} [{}]",
            st.comm,
            nodes,
            format_cpulist(&cpus)
        )
    );
    if !no_migrate && nodes.len() < topo.nodes.len() {
        let from: Vec<usize> = topo
            .node_ids()
            .into_iter()
            .filter(|x| !nodes.contains(x))
            .collect();
        match procfs::migrate_pages(pid, &from, &nodes) {
            Ok(_) => println!(
                "{}",
                numapilot::tf!(
                    "memória migrada para o(s) nó(s) {:?}",
                    "memory migrated to node(s) {:?}",
                    nodes
                )
            ),
            Err(e) => println!(
                "{}",
                numapilot::tf!(
                    "aviso: migração de memória falhou: {e}",
                    "warning: memory migration failed: {e}"
                )
            ),
        }
    }
    // Avisa o daemon para não mexer mais neste processo.
    let mut pins = daemon::read_pins();
    pins.retain(|p| procfs::read_stat(p.pid).is_some_and(|s| s.starttime == p.starttime));
    pins.insert(daemon::Pin {
        pid,
        starttime: st.starttime,
    });
    let v: Vec<_> = pins.into_iter().collect();
    let _ = std::fs::create_dir_all(learned::run_dir());
    if std::fs::write(daemon::pins_path(), serde_json::to_string(&v)?).is_err() {
        println!(
            "{}",
            numapilot::tf!(
                "aviso: não consegui registrar o pin para o daemon (rode com sudo); ele pode reposicionar o processo.",
                "warning: could not register the pin with the daemon (run with sudo); it may reposition the process."
            )
        );
    }
    Ok(())
}

fn cmd_learned(forget: Option<String>) -> Result<()> {
    let mut l = Learned::load();
    if let Some(pat) = forget {
        let before = l.map.len();
        l.map.retain(|k, _| !k.contains(&pat));
        l.dirty = true;
        l.save().context(numapilot::i18n::tr(
            "não consegui salvar (precisa de sudo?)",
        ))?;
        println!(
            "{}",
            numapilot::tf!(
                "{} entrada(s) removida(s). Reinicie o daemon para ele recarregar.",
                "{} entry(ies) removed. Restart the daemon for it to reload.",
                before - l.map.len()
            )
        );
        return Ok(());
    }
    if l.map.is_empty() {
        println!(
            "{}",
            numapilot::tf!(
                "Nada aprendido ainda ({}).",
                "Nothing learned yet ({}).",
                Learned::path().display()
            )
        );
    }
    for (k, e) in &l.map {
        println!(
            "{}",
            numapilot::tf!(
                "{}  {}  pico {:.0}% de um nó  gpu-nó {}  execuções {}",
                "{}  {}  peak {:.0}% of a node  gpu-node {}  runs {}",
                if e.wide { "[2 sockets]" } else { "[1 socket] " },
                k,
                e.peak_frac * 100.0,
                e.gpu_node
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "-".into()),
                e.runs
            )
        );
    }
    Ok(())
}

fn cmd_log(lines: usize, which: Option<&str>) -> Result<()> {
    let want = which.map(|w| match w {
        "servico" | "serviço" | "daemon" => "serviço",
        o => o,
    });
    let mut shown = 0;
    for (name, path) in numapilot::log::files() {
        if want.is_some_and(|w| !name.starts_with(w)) {
            continue;
        }
        let t = numapilot::log::tail(&path, lines);
        if t.is_empty() {
            continue;
        }
        shown += 1;
        println!("== {name} ({}) ==", path.display());
        for l in t {
            println!("{l}");
        }
        println!();
    }
    if want.is_none_or(|w| w == "serviço") {
        // o journal tem o que veio antes do registro em arquivo
        let o = std::process::Command::new("journalctl")
            .args([
                "-u",
                "numapilot",
                "-b",
                "--no-pager",
                "-o",
                "short-iso",
                "-n",
            ])
            .arg(lines.to_string())
            .output();
        if let Ok(o) = o {
            let t = String::from_utf8_lossy(&o.stdout);
            if !t.trim().is_empty() && !t.contains("No entries") {
                shown += 1;
                println!(
                    "{}",
                    numapilot::tf!(
                        "== journal do serviço (este boot) ==",
                        "== service journal (this boot) =="
                    )
                );
                print!("{t}");
            }
        }
    }
    if shown == 0 {
        println!(
            "{}",
            numapilot::tf!("Nenhum registro ainda.", "No logs yet.")
        );
    }
    Ok(())
}

fn cmd_gpu_test(cfg: &Config, only: Option<&str>, json: bool) -> Result<()> {
    use numapilot::gputest::Verdict;
    let topo = Topology::load(cfg);
    let mut all = vec![];
    let mut wrong = 0;
    let mut found = false;
    for g in &topo.gpus {
        if only.is_some_and(|o| o != g.alias && o != g.pci) {
            continue;
        }
        found = true;
        if g.is_nvidia() && numapilot::nvctl::asleep(&g.pci) && only.is_none() {
            if !json {
                println!(
                    "{}",
                    numapilot::tf!(
                        "{} ({}): dormindo, pulei para não acordar (teste só ela: numapilot gpu-test {})",
                        "{} ({}): asleep, skipped so as not to wake it (test only it: numapilot gpu-test {})",
                        g.alias,
                        g.short_name(),
                        g.alias
                    )
                );
            }
            continue;
        }
        let (mut set, unset) = numapilot::decide::env_for_gpu_full(g, &topo);
        // a mesma reserva de DirectX que o `run` usa (sem efeito no vulkaninfo, mas aparece)
        let dxf = numapilot::gputest::vk_name_for(g, &set, &unset);
        let lines = numapilot::gputest::test(g, &topo, &set, &unset);
        if !json {
            println!("{} ({}, {})", g.alias, g.short_name(), g.pci);
            for l in &lines {
                let tag = match l.verdict {
                    Verdict::Certa => numapilot::i18n::tr("certa  "),
                    Verdict::Errada => numapilot::i18n::tr("ERRADA "),
                    Verdict::NaoDeu => numapilot::i18n::tr("não deu"),
                };
                println!("  {tag} {}: {}", l.api, l.detail);
            }
            if let Some(n) = &dxf {
                println!(
                    "{}",
                    numapilot::tf!(
                        "  nome para o filtro do DXVK/VKD3D: {n}",
                        "  name for the DXVK/VKD3D filter: {n}"
                    )
                );
            }
        }
        wrong += lines
            .iter()
            .filter(|l| l.verdict == Verdict::Errada)
            .count();
        set.clear();
        all.push(
            serde_json::json!({"alias": g.alias, "pci": g.pci, numapilot::i18n::tr("linhas"): lines, "filtro_dx": dxf}),
        );
    }
    if !found {
        anyhow::bail!(
            "{}",
            numapilot::tf!(
                "placa não encontrada (veja os apelidos em: numapilot topo)",
                "card not found (see the aliases in: numapilot topo)"
            )
        );
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&all)?);
    } else if wrong > 0 {
        println!();
        println!(
            "{}",
            numapilot::tf!(
                "Alguma escolha caiu na placa errada. Mande esta saída e a de: numapilot run --dry-run --gpu APELIDO -- true",
                "Some choice landed on the wrong card. Send this output and that of: numapilot run --dry-run --gpu ALIAS -- true"
            )
        );
    }
    if wrong > 0 {
        std::process::exit(2);
    }
    Ok(())
}

fn cmd_turbo(cfg: &Config) -> Result<()> {
    use numapilot::turbo;
    let topo = Topology::load(cfg);
    // root lê os MSRs direto; usuário comum usa o que o serviço publicou
    let tables: std::collections::BTreeMap<u32, turbo::Table> = if unsafe { libc::geteuid() } == 0 {
        turbo::tables(&topo).clone()
    } else {
        std::fs::read_to_string(numapilot::learned::run_dir().join("hw.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| serde_json::from_value(v.get("turbo")?.clone()).ok())
            .unwrap_or_default()
    };
    let all: Vec<usize> = topo.nodes.iter().flat_map(|n| n.cpus.clone()).collect();
    let a = turbo::deep_time(&all);
    let t0 = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_secs(1));
    let b = turbo::deep_time(&all);
    let dt = t0.elapsed().as_micros() as f64;
    for n in &topo.nodes {
        let total = n.physical_cores.max(n.cores.len());
        let awake = turbo::awake_cores(&a, &b, dt, &n.cores);
        println!(
            "{}",
            numapilot::tf!(
                "Processador {} ({} núcleos)",
                "Processor {} ({} cores)",
                n.id,
                total
            )
        );
        match n.cpus.first().and_then(|c| turbo::deep_enabled(*c)) {
            Some(true) => println!(
                "{}",
                numapilot::tf!(
                    "  acordados agora: ~{awake:.1} (média de 1 s)",
                    "  awake now: ~{awake:.1} (1 s average)"
                )
            ),
            Some(false) => println!(
                "{}",
                numapilot::tf!(
                    "  estados profundos (C3/C6) DESLIGADOS: todos os núcleos contam como acordados; ligue em Máquina → Processador",
                    "  deep states (C3/C6) OFF: every core counts as awake; turn them on in Machine → Processor"
                )
            ),
            None => {
                println!(
                    "{}",
                    numapilot::tf!(
                        "  sem cpuidle neste sistema: não dá para estimar os núcleos acordados",
                        "  no cpuidle on this system: the awake cores cannot be estimated"
                    )
                )
            }
        }
        match tables.get(&(n.id as u32)) {
            Some(t) => {
                println!(
                    "{}",
                    numapilot::tf!(
                        "  tabela (núcleos acordados → clock máximo): {}",
                        "  table (cores awake → maximum clock): {}",
                        turbo::describe(t)
                    )
                );
                if let Some(mhz) = turbo::limit_for(t, awake) {
                    println!(
                        "{}",
                        numapilot::tf!(
                            "  com a carga de agora: até {:.1} GHz",
                            "  with the current load: up to {:.1} GHz",
                            mhz as f64 / 1000.0
                        )
                    );
                }
            }
            None => println!(
                "{}",
                numapilot::tf!(
                    "  tabela de turbo indisponível (só Intel; precisa do serviço rodando ou de sudo)",
                    "  turbo table unavailable (Intel only; needs the service running or sudo)"
                )
            ),
        }
    }
    Ok(())
}

fn cmd_bench_cores(cfg: &Config, show: bool, clear: bool) -> Result<()> {
    use numapilot::corebench;
    let topo = Topology::load(cfg);
    if clear {
        let r = ctl::send(&ctl::Request::SetCoreRank {
            rank: corebench::CoreRank::default(),
        })?;
        println!(
            "{}",
            if r.ok {
                numapilot::i18n::tr(
                    "Medição apagada: a escolha de núcleos volta à ordem do sistema.",
                )
            } else {
                r.message.as_str()
            }
        );
        return Ok(());
    }
    let rank = if show {
        match corebench::saved() {
            Some(r) if !r.ns.is_empty() => r,
            _ => bail!(
                "{}",
                numapilot::tf!(
                    "nenhuma medição guardada (rode: numapilot bench-cores)",
                    "no saved measurement (run: numapilot bench-cores)"
                )
            ),
        }
    } else {
        println!(
            "{}",
            numapilot::tf!(
                "Deixe o PC parado durante a medição.",
                "Leave the PC idle during the measurement."
            )
        );
        corebench::measure(&topo, &mut |l| println!("{l}")).map_err(|e| anyhow::anyhow!(e))?
    };
    for n in &topo.nodes {
        let mut v: Vec<(usize, usize, f32)> = n
            .cores
            .iter()
            .enumerate()
            .filter_map(|(i, c)| Some((i, *c.first()?, *rank.ns.get(c.first()?)?)))
            .collect();
        v.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
        let Some(best) = v.first().map(|x| x.2) else {
            continue;
        };
        println!(
            "{}",
            numapilot::tf!(
                "Processador {} (do mais rápido ao mais lento):",
                "Processor {} (fastest to slowest):",
                n.id
            )
        );
        for (i, cpu, ns) in &v {
            println!(
                "{}",
                numapilot::tf!(
                    "  núcleo {i:>2} (CPU {cpu:>3}): {ns:.2} ns  {:+.1}%",
                    "  core {i:>2} (CPU {cpu:>3}): {ns:.2} ns  {:+.1}%",
                    (ns / best - 1.0) * 100.0
                )
            );
        }
    }
    println!();
    println!(
        "{}",
        numapilot::tf!(
            "Diferença entre o melhor e o pior núcleo: {:.1}% · ruído da medição: {:.1}%",
            "Difference between the best and the worst core: {:.1}% · measurement noise: {:.1}%",
            rank.spread_pct,
            rank.noise_pct
        )
    );
    if rank.useful {
        println!(
            "{}",
            numapilot::tf!(
                "A diferença é maior que o ruído: a escolha de núcleos passa a preferir os mais rápidos.",
                "The difference is larger than the noise: core choice now prefers the fastest ones."
            )
        );
    } else {
        println!(
            "{}",
            numapilot::tf!(
                "A diferença é pequena demais (ou a medição variou muito) para valer como critério: \
             a escolha de núcleos continua na ordem do sistema. Se o PC estava ocupado, feche tudo e meça de novo.",
                "The difference is too small (or the measurement varied too much) to count as a criterion: \
             core choice stays in the system order. If the PC was busy, close everything and measure again."
            )
        );
    }
    if !show {
        let r = ctl::send(&ctl::Request::SetCoreRank { rank })?;
        if !r.ok {
            bail!(
                "{}",
                numapilot::tf!(
                    "medi, mas o serviço não guardou: {}",
                    "measured, but the service did not save it: {}",
                    r.message
                )
            );
        }
    }
    Ok(())
}
