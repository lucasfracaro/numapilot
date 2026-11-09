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
    /// Placas especiais (BC-250...): status | governor-apply ARQUIVO (com sudo)
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
}

fn main() {
    // `numapilot hybrid | head`: ao fechar o pipe, sai quieto em vez de "cair" com Broken pipe
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    if let Err(e) = real_main() {
        numapilot::log::error(&format!("{e:#}"));
        eprintln!("numapilot: erro: {e:#}");
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
            println!("Relatório: {}", path.display());
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
                    "\nRelatório salvo em {}: é só me mandar esse arquivo.",
                    p.display()
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
                println!("Nenhuma placa especial conhecida neste PC.");
                return Ok(());
            }
            match action.as_str() {
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
                                if s.active { "rodando" } else { "parado" }
                            );
                        }
                        if let Some(c) = numapilot::boards::governor_config(b) {
                            println!("  configuração: {c}");
                        }
                        for n in &b.notes {
                            println!("  - {n}");
                        }
                    }
                    Ok(())
                }
                "governor-apply" => {
                    let f = file.context("faltou o arquivo")?;
                    let b = boards
                        .iter()
                        .map(|x| x.0)
                        .find(|b| !b.governor_configs.is_empty())
                        .context("nenhuma placa com governor")?;
                    numapilot::boards::governor_apply(b, &f, &mut |m| println!("{m}"))
                        .map_err(|e| anyhow::anyhow!(e))
                }
                _ => bail!(
                    "use: numapilot board status | sudo numapilot board governor-apply ARQUIVO"
                ),
            }
        }
        Cmd::Recipes { action, url } => match action.as_str() {
            "show" => {
                let r = numapilot::recipes::load();
                println!(
                    "Receitas versão {}: {} placas, {} processadores, {} arquiteturas",
                    r.version,
                    r.board.len(),
                    r.cpu.len(),
                    r.arch.len()
                );
                let topo = Topology::load(&cfg);
                for g in &topo.gpus {
                    if let Some(a) = r.arch_for(g) {
                        println!(
                            "  {}: {} · computação {} · upscaling {}",
                            g.alias,
                            a.family,
                            a.compute,
                            a.upscale.join(", ")
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
                println!("Receitas atualizadas (versão {v}).");
                Ok(())
            }
            _ => bail!("use: numapilot recipes show | sudo numapilot recipes update [URL]"),
        },
        Cmd::Upscale {
            action,
            appid,
            int8,
        } => {
            use numapilot::upscale;
            if action == "fetch" {
                let v = upscale::fetch(&mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))?;
                println!("OptiScaler {v} pronto.");
                return Ok(());
            }
            let id = appid.context("faltou o appid da Steam")?;
            let root = upscale::steam_game_root(&id)
                .context("jogo não encontrado nas bibliotecas da Steam")?;
            let dir = upscale::exe_dir(&root).context("não achei o executável do jogo")?;
            match action.as_str() {
                "status" => {
                    println!("Pasta do executável: {}", dir.display());
                    match upscale::installed(&dir) {
                        Some(i) => println!(
                            "OptiScaler {} instalado ({} arquivos)",
                            i.version,
                            i.files.len()
                        ),
                        None => println!("OptiScaler não instalado"),
                    }
                    Ok(())
                }
                "install" => {
                    if upscale::local_release().is_none() {
                        upscale::fetch(&mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))?;
                    }
                    let v = upscale::install(&dir, int8).map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "OptiScaler {v} instalado em {}. Não use em jogos online (anti-cheat pode banir).",
                        dir.display()
                    );
                    Ok(())
                }
                "remove" => {
                    upscale::remove(&dir).map_err(|e| anyhow::anyhow!(e))?;
                    println!("OptiScaler removido de {}", dir.display());
                    Ok(())
                }
                _ => bail!("use: numapilot upscale fetch | status|install|remove APPID [--int8]"),
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
                                .map(|t| format!("GPU mais quente {t:.0} °C"))
                                .unwrap_or_default()
                        );
                    }
                    Ok(())
                }
                "add" => {
                    let p = peers::add(
                        &arg.context("faltou o link (o do QR code do outro PC)")?,
                        name.as_deref(),
                    )
                    .map_err(|e| anyhow::anyhow!(e))?;
                    println!("{} adicionado ({})", p.name, p.url);
                    Ok(())
                }
                "remove" => {
                    peers::remove(&arg.context("faltou o nome")?)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!("removido");
                    Ok(())
                }
                "pull" => {
                    let m = peers::pull_profiles(
                        &arg.context("faltou o nome")?,
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
                    let m = peers::wake(&arg.context("faltou o nome")?)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!("{m}");
                    Ok(())
                }
                _ => bail!(
                    "use: numapilot peers list | add LINK [NOME] | remove NOME | wake NOME | pull NOME [todos] | backup"
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
                            if c.ok { " ok " } else { "falta" },
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
                            "(diga a placa para conferir ela também: numapilot vm check rx470-0)"
                        );
                    }
                    Ok(())
                }
                "prepare" => {
                    let g = arg
                        .as_deref()
                        .or(gpu.as_deref())
                        .and_then(find)
                        .context("diga a placa (veja numapilot topo)")?;
                    vm::prepare(&topo, &g, &mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))
                }
                "unprepare" => {
                    vm::unprepare(&mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))
                }
                "create" => {
                    let name = arg.context("diga o nome da VM")?;
                    let g = gpu.as_deref().and_then(find).context("--gpu PLACA")?;
                    let iso = iso.context("--iso arquivo do Windows (.iso)")?;
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
                        "VM {name} criada: abra no virt-manager para instalar o Windows. Depois instale o driver da placa dentro dela."
                    );
                    Ok(())
                }
                _ => bail!(
                    "use: numapilot vm check [GPU] | sudo numapilot vm prepare GPU | sudo numapilot vm unprepare | numapilot vm create NOME --gpu GPU --iso ARQ.iso [--mem 16 --cpus 8 --disk 120]"
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
                        "{} (vale para o próximo Xwayland: saia e entre na sessão, ou feche os programas X11)",
                        if on {
                            "Xwayland usando só o Mesa"
                        } else {
                            "Xwayland normal"
                        }
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
                        "modo da NVIDIA: {act} (o serviço aplica em segundos, sem sair da sessão)"
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
                                            if o.connected { " [tela]" } else { "" }
                                        ))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                );
                            }
                        }
                        None if st.as_ref().is_some_and(|s| s.removed) => {
                            println!(
                                "{}: desligada pelo numapilot (fora do sistema)",
                                st.as_ref().map(|s| s.alias.as_str()).unwrap_or("NVIDIA")
                            );
                        }
                        None => {
                            println!("Não é um notebook com placa integrada + NVIDIA.");
                            return Ok(());
                        }
                    }
                    let m = numapilot::machine::Machine::load().dgpu_mode;
                    println!(
                        "Energia: {}",
                        match numapilot::notebook::on_ac() {
                            Some(true) => "tomada",
                            Some(false) => "bateria",
                            None => "?",
                        }
                    );
                    println!(
                        "Modo: {}",
                        m.as_deref()
                            .and_then(|m| hybrid::MODES.iter().find(|x| x.0 == m))
                            .map(|x| x.1)
                            .unwrap_or("não mexer")
                    );
                    if let Some(st) = &st {
                        if !st.blocked_by.is_empty() {
                            println!(
                                "Esperando para desligar: em uso por {}",
                                st.blocked_by.join(", ")
                            );
                            for n in &st.blocked_by {
                                if let Some(h) = numapilot::notebook::holder_hint(n) {
                                    println!("  · {h}");
                                }
                            }
                        }
                        if st.sleep_only && !st.want_present {
                            println!(
                                "O KDE segura a saída de vídeo da NVIDIA: ela não é tirada do sistema, mas dorme sozinha ({}).",
                                numapilot::notebook::dgpu_state(&st.pci)
                            );
                        }
                    }
                    println!(
                        "Xwayland: {}",
                        if hybrid::xwayland_wrapped() {
                            "só Mesa (não prende a NVIDIA)"
                        } else {
                            "normal (pode prender a NVIDIA)"
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
                let u = u
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("falta o nome do serviço"))?;
                let full = if u.contains('.') {
                    u.to_string()
                } else {
                    format!("{u}.service")
                };
                all.iter().find(|s| s.unit == full).cloned().ok_or_else(|| {
                    anyhow::anyhow!(
                        "serviço {full} não encontrado{}",
                        if usuario {
                            ""
                        } else {
                            " (do usuário? use --usuario)"
                        }
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
                o => bail!("ação desconhecida: {o}"),
            };
            let s = find(&unit)?;
            let c = services::action_command(&s, verb).map_err(|e| anyhow::anyhow!(e))?;
            let st = std::process::Command::new(&c[0]).args(&c[1..]).status()?;
            if !st.success() {
                bail!("{} terminou com erro", c.join(" "));
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
                        anyhow::anyhow!("não achei \"{id}\" (veja numapilot inicio lista)")
                    })
            };
            let arg = |i: usize| {
                args.get(i).cloned().ok_or_else(|| {
                    anyhow::anyhow!("falta um argumento (veja numapilot inicio --help)")
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
                            (true, Some(m)) => format!("rodando, {} MB", m / 1048576),
                            (true, None) => "rodando".into(),
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
                                " (protegido)"
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
                            println!("{}: {}", it.name, if on { "ligado" } else { "desligado" })
                        }
                        Action::NeedsRoot(c) => {
                            let st = std::process::Command::new(&c[0]).args(&c[1..]).status()?;
                            if !st.success() {
                                bail!("{} terminou com erro", c.join(" "));
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
                            "  abre por:   {}{}",
                            f.display(),
                            t.file_pkg
                                .as_ref()
                                .map(|p| format!(" (do pacote {p})"))
                                .unwrap_or_default()
                        );
                    }
                    println!("  comando:    {}", it.exec);
                    if let Some(e) = &t.exe {
                        println!("  programa:   {}", e.display());
                    }
                    println!("  origem:     {}", t.source.text());
                    if let Some(d) = &t.description {
                        println!("  descrição:  {d}");
                    }
                    if let Some(sz) = t.size.filter(|s| *s > 0) {
                        println!("  tamanho:    {:.1} MB", sz as f64 / 1048576.0);
                    }
                    if let Some(d) = &t.app_dir {
                        println!("  pasta:      {}", d.display());
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
                        bail!("não dá para desinstalar: {why}");
                    }
                    println!("\nVai fazer:");
                    for s in &plan.steps {
                        println!("  - {s}");
                    }
                    if !sim {
                        print!("Continuar? [s/N] ");
                        use std::io::Write;
                        std::io::stdout().flush()?;
                        let mut a = String::new();
                        std::io::stdin().read_line(&mut a)?;
                        if !a.trim().eq_ignore_ascii_case("s") {
                            println!("Nada feito.");
                            return Ok(());
                        }
                    }
                    let (_, refused) =
                        numapilot::procctl::terminate(&it.pids, unsafe { libc::getuid() }, false);
                    if let Some(r) = refused {
                        eprintln!("(não encerrei: {r})");
                    }
                    let st = std::process::Command::new("sh")
                        .args(["-c", &plan.script])
                        .status()?;
                    if !st.success() {
                        bail!("a desinstalação terminou com erro");
                    }
                    Ok(())
                }
                "remover" | "rm" => {
                    let it = find(&arg(0)?)?;
                    startup::remove(&it).map_err(|e| anyhow::anyhow!(e))?;
                    println!("{} removido", it.name);
                    Ok(())
                }
                "comando" => {
                    let p = startup::add_command(&arg(0)?, &args[1..].join(" "), atraso)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!("adicionado: {}", p.display());
                    Ok(())
                }
                "arquivo" => {
                    let p = startup::add_file(std::path::Path::new(&arg(0)?), atraso)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!("adicionado: {}", p.display());
                    Ok(())
                }
                "app" => {
                    let q = args.join(" ");
                    let found = startup::search_apps(&q);
                    match found.as_slice() {
                        [(n, p)] => {
                            let d =
                                startup::add_desktop(p, atraso).map_err(|e| anyhow::anyhow!(e))?;
                            println!("{n} vai abrir no login ({})", d.display());
                        }
                        [] => bail!("nenhum programa instalado com \"{q}\""),
                        many => {
                            println!("Mais de um programa; seja mais específico:");
                            for (n, p) in many.iter().take(20) {
                                println!("  {n}  ({})", p.display());
                            }
                        }
                    }
                    Ok(())
                }
                o => bail!("ação desconhecida: {o}"),
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
                        println!("Tudo que abre com o sistema já está na integrada.");
                    } else {
                        println!(
                            "Agora na integrada (vale a partir do próximo login): {}",
                            ch.join(", ")
                        );
                    }
                    Ok(())
                }
                "normal" => {
                    let ch = autostart::revert();
                    println!(
                        "Voltaram ao normal: {}",
                        if ch.is_empty() {
                            "nada".into()
                        } else {
                            ch.join(", ")
                        }
                    );
                    Ok(())
                }
                _ => {
                    println!(
                        "Na integrada: {}",
                        if autostart::enabled() {
                            "ligado"
                        } else {
                            "desligado"
                        }
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
                        println!("  {s:<40} (serviço do usuário)");
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
                How::Package(p) => println!("instalado pelo pacote {p}"),
                How::Source(d) => {
                    println!("instalado pela pasta {}", d.display());
                    let ahead = selfupdate::source_ahead_of_installed(d).unwrap_or(0);
                    if ahead > 0 {
                        pending = true;
                        println!("a pasta tem {ahead} commit(s) que ainda não foram instalados");
                    }
                    match selfupdate::source_behind(d) {
                        Some(0) if ahead == 0 => println!("já está na versão mais nova"),
                        Some(0) => println!("nada novo no repositório remoto"),
                        Some(n) => {
                            pending = true;
                            println!("{n} atualização(ões) novas no repositório")
                        }
                        None => println!("não consegui consultar o repositório"),
                    }
                }
                How::Unknown => println!("origem desconhecida"),
            }
            if pending && action.as_deref() != Some("run") {
                println!("para instalar: numapilot self-update run");
            }
            if action.as_deref() == Some("run") {
                let c = selfupdate::command().map_err(|e| anyhow::anyhow!(e))?;
                let st = std::process::Command::new(&c[0]).args(&c[1..]).status()?;
                if !st.success() {
                    bail!("a atualização terminou com erro ({st})");
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
                .with_context(|| format!("GPU '{gpu}' não encontrada (veja numapilot topo)"))?
                .clone();
            numapilot::undervolt::catch_ctrl_c();
            println!("Deixe o PC parado durante o teste. Ctrl+C para parar e voltar ao que era.");
            let opts = numapilot::undervolt::Opts {
                secs,
                margin,
                clock,
                step,
            };
            match numapilot::undervolt::run(&topo, &g, &opts, &mut |l| println!("{l}")) {
                Ok(o) => {
                    println!(
                        "\nPronto: {} ({} passo(s) estáveis).",
                        o.description, o.steps_ok
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
                        println!("Nenhuma placa NVIDIA neste computador.");
                        return Ok(());
                    }
                    for g in &s.gpus {
                        println!("Placa: {} ({}, id {:04x})", g.1, g.0, g.2);
                    }
                    if let Some(b) = &s.branch {
                        println!("Driver indicado: {}", b.label());
                    }
                    println!(
                        "Carregado agora: {}",
                        s.loaded.clone().unwrap_or("nenhum".into())
                    );
                    println!("Instalado: {}", s.installed.clone().unwrap_or("não".into()));
                    if let Some(a) = &s.available {
                        println!("No repositório: {a}");
                    }
                    if s.nouveau {
                        println!("Usando o nouveau (driver aberto da comunidade) agora.");
                    }
                    if s.needs_reboot {
                        println!("O driver instalado é diferente do carregado: reinicie.");
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
                    "use: numapilot driver status | sudo numapilot driver install | sudo numapilot driver update"
                ),
            }
        }
        Cmd::Backup { out } => {
            let f = numapilot::backup::create(out.as_deref()).map_err(|e| anyhow::anyhow!(e))?;
            println!("Backup salvo em {}", f.display());
            println!(
                "Copie para fora do disco do sistema (pendrive, outro disco, nuvem) antes de formatar."
            );
            println!("Para restaurar: ./install.sh --restore {}", f.display());
            Ok(())
        }
        Cmd::Restore { file, no_boot } => {
            numapilot::backup::restore(&file, !no_boot, &mut |m| println!("{m}"))
                .map_err(|e| anyhow::anyhow!(e))
        }
        Cmd::Web { port, bind, url } => {
            if url {
                println!("{}", numapilot::web::url(port).context("sem rede")?);
                return Ok(());
            }
            numapilot::web::serve(&cfg, &bind, port)
        }
        Cmd::Bench { size_mb, threads } => {
            let t = Topology::load(&cfg);
            println!(
                "Medindo {} nó(s); leva alguns segundos por par...",
                t.nodes.len()
            );
            let r = numapilot::bench::run(&t, size_mb, threads, |m| println!("  {m}"));
            numapilot::bench::print(&r);
            if let Some(p) = numapilot::bench::save(&r) {
                println!(
                    "\nResultado salvo em {} (aparece na interface).",
                    p.display()
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
    println!("Nós NUMA");
    for n in &t.nodes {
        println!(
            "  nó {}  socket(s) {:?}  CPUs {}  ({} lógicos / {} físicos)  RAM {} (livre {})  distâncias {:?}",
            n.id,
            n.sockets,
            format_cpulist(&n.cpus),
            n.cpus.len(),
            n.physical_cores,
            gib(n.mem_total_kb),
            gib(n.mem_free_kb()),
            n.distances
        );
    }
    println!("\nGPUs");
    for g in &t.gpus {
        let role = match g.role {
            GpuRole::Display => "vídeo (monitor ligado)",
            GpuRole::Compute => "computação (ROCm)",
            GpuRole::Other => "outra",
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
            "  {:<10} {}  {}  [{}]\n             nó {}  {}  {}  ocupação {}  VRAM {}  ROCR {}",
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
        );
    }
    let w = t.warnings();
    if !w.is_empty() {
        println!();
        for x in w {
            println!("aviso: {x}");
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
    let prog = command.first().context("faltou o programa")?;
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
            "numapilot: otimizador de FPS, teste {}/{n}: {} (jogue ~{} s numa cena parecida)",
            i + 1,
            v.label,
            secs + 10
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
    let (set, unset) = gpu_env(&d, &topo);

    if dry_run || std::env::var_os("NUMAPILOT_VERBOSE").is_some() {
        eprintln!("programa : {}", command.join(" "));
        eprintln!("perfil   : {pkey}");
        eprintln!(
            "modo     : {}",
            if d.manual { "manual" } else { "automático" }
        );
        eprintln!("nós      : {:?}  CPUs {}", d.nodes, format_cpulist(&d.cpus));
        eprintln!("memória  : {:?}", d.mem);
        eprintln!(
            "prioridade: {}{}",
            d.priority,
            if d.needs_daemon {
                " (aplicada pelo serviço)"
            } else {
                ""
            }
        );
        eprintln!(
            "uso GPU  : {}",
            match d.kind {
                Kind::Graphics => "gráfico (jogo/vídeo)",
                Kind::Compute => "computação (ROCm)",
                Kind::NoGpu => "-",
            }
        );
        eprintln!(
            "GPU      : {}",
            if d.gpus.is_empty() {
                "nenhuma".to_string()
            } else {
                d.gpus
                    .iter()
                    .map(|g| format!("{} ({})", g.alias, g.pci))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        for r in &d.reasons {
            eprintln!("motivo   : {r}");
        }
        for w in numapilot::decide::game_warnings(&topo, &d) {
            eprintln!("atenção  : {w}");
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
        match ctl::send(&ctl::Request::Adopt { pid: me, profile }) {
            Ok(r) if r.ok => {}
            Ok(r) => eprintln!("numapilot: aviso: {}", r.message),
            Err(e) => {
                eprintln!("numapilot: aviso: limites de memória/prioridade não aplicados ({e:#})")
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
            "numapilot: aviso: set_mempolicy falhou ({e}); seguindo só com a afinidade de CPU"
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
                "numapilot: aviso: gamescope não instalado (sudo pacman -S gamescope); abrindo sem ele"
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
                "numapilot: aviso: OptiScaler escolhido mas não instalado na pasta do jogo (Ajustar… → Upscaling → Instalar)"
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
        let st = c
            .status()
            .with_context(|| format!("não consegui executar '{}'", argv[0]))?;
        let dir = numapilot::tuner::log_dir(&pkey);
        let res = numapilot::tuner::newest_csv(&dir, start)
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| numapilot::tuner::parse_csv(&s));
        let mut t = numapilot::tuner::Tuner::load();
        match (res, t.sessions.get_mut(&pkey)) {
            (Some(r), Some(s)) if s.next == i => {
                eprintln!(
                    "numapilot: teste {}: {:.1} FPS médio, 1% low {:.1}",
                    i + 1,
                    r.avg,
                    r.low1
                );
                s.variants[i].result = Some(r);
                s.next += 1;
                let _ = t.save();
            }
            (None, _) => eprintln!(
                "numapilot: o MangoHud não gravou este teste (jogou menos de ~{} s? MangoHud instalado?). Ele se repete na próxima abertura.",
                secs + 10
            ),
            _ => {}
        }
        std::process::exit(st.code().unwrap_or(1));
    }
    let err = c.exec();
    bail!("não consegui executar '{}': {err}", argv[0])
}

fn cmd_profile(action: ProfileCmd) -> Result<()> {
    match action {
        ProfileCmd::List => {
            let p = Profiles::load();
            if p.map.is_empty() {
                println!("Nenhum perfil salvo: tudo no automático.");
            }
            for (k, v) in &p.map {
                println!("{k}\n    cpu={}  gpu={}  uso={}", v.cpu, v.gpu, v.usage);
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
            println!("{} configurações a testar em {label}:", v.len());
            for (i, x) in v.iter().enumerate() {
                println!("  {}. {}", i + 1, x.label);
            }
            t.start(&k, &label, v);
            t.save()?;
            println!(
                "Abra o jogo {} vezes (pelo numapilot run ou pela Steam com o numapilot) e jogue ~70 s em cada.",
                t.sessions[&k].variants.len()
            );
            if which("mangohud").is_none() {
                println!(
                    "Atenção: instale o MangoHud (sudo pacman -S mangohud), ele faz a medição."
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
                    "{} ({}) — {}/{} testados",
                    s.label,
                    s.key,
                    s.variants.iter().filter(|v| v.result.is_some()).count(),
                    s.variants.len()
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
            println!("Otimização de {k} cancelada.");
            Ok(())
        }
        ("apply", Some(k)) => {
            let s = t
                .sessions
                .get(&k)
                .context("não há otimização para esse programa")?;
            let b = s.best().context("nenhum teste terminou ainda")?;
            let mut profile = s.variants[b].profile.clone();
            profile.label = s.label.clone();
            let r = ctl::send(&ctl::Request::SetProfile {
                key: k.clone(),
                profile,
                pid: None,
            })?;
            println!("Usando \"{}\": {}", s.variants[b].label, r.message);
            Ok(())
        }
        _ => bail!("use: numapilot tune start|stop|apply CHAVE  ou  numapilot tune status [CHAVE]"),
    }
}

fn cmd_machine(cfg: &Config, action: &str, value: Option<&str>) -> Result<()> {
    use numapilot::{cpuctl, machine::Machine};
    let topo = Topology::load(cfg);
    match (action, value) {
        ("status", _) => {
            let m = Machine::load();
            println!("Modo: {}", m.mode);
            println!(
                "Turbo: {}",
                cpuctl::turbo()
                    .map(|t| if t { "ligado" } else { "desligado" })
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
                    "Socket {}: governor {} · {} · {}-{} MHz (agora {}) · uncore {} · {} · limite {} · {}",
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
                        .unwrap_or("indisponível".into()),
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
                        .map(|w| format!("consumo {w:.0} W"))
                        .unwrap_or_default(),
                );
            }
            println!(
                "Estados de economia (cpu0): {}",
                cpuctl::cstates(0)
                    .iter()
                    .map(|s| format!(
                        "{} {}µs{}",
                        s.name,
                        s.latency_us,
                        if s.disabled { " (desligado)" } else { "" }
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if let Some(l) = hw["lowlat"].as_str().filter(|s| !s.is_empty()) {
                println!("Baixa latência agora nos núcleos {l}");
            }
            println!("THP: {}", numapilot::machine::thp().unwrap_or("?".into()));
            if let Some((cur, list)) = numapilot::machine::scx_status() {
                println!(
                    "Escalonador: {} (disponíveis: {})",
                    if cur.is_empty() {
                        "padrão do kernel".into()
                    } else {
                        cur
                    },
                    list.join(", ")
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
            "use: numapilot machine status  |  numapilot machine mode jogo|ia|silencioso|normal"
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
                    "Bootloader: {} ({})\nConfigurado: {p}",
                    l.name(),
                    l.path.display()
                ),
                None => println!("Bootloader não reconhecido"),
            }
            println!("Em uso agora: {}", cmdline::running());
            println!("\nParâmetros conhecidos:");
            for (p, t, d) in cmdline::KNOWN {
                println!("  {p:<26} {t}: {d}");
            }
            let v = cmdline::vulnerabilities();
            println!("\nMitigações que se aplicam a este processador:");
            for m in cmdline::relevant_mitigations(&v) {
                println!(
                    "  {:<44} risco {} · ganho {} · {}",
                    m.title,
                    m.risk,
                    m.gain,
                    m.params.join(" ")
                );
            }
            println!("  (tudo de uma vez: mitigations=off)");
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
                    _ => bail!("use +PARAM para adicionar e -PARAM para tirar (recebido {a})"),
                }
            }
            numapilot::cmdline::apply(&add, &rm, &mut |m| println!("{m}"))
                .map_err(|e| anyhow::anyhow!(e))
        }
        ("cmdline", Some(op @ ("add" | "remove"))) => {
            let ps: Vec<String> = args[1..].to_vec();
            if ps.is_empty() {
                bail!("faltou o parâmetro");
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
                "OverDrive (clock/voltagem das placas AMD): {} agora, {} depois de reiniciar{}",
                if s.active { "ligado" } else { "desligado" },
                if s.on_next_boot {
                    "ligado"
                } else {
                    "desligado"
                },
                if s.from_cmdline {
                    " (definido na linha do kernel)"
                } else {
                    ""
                }
            );
            let gib = |p: u64| p as f64 * 4096.0 / 1073741824.0;
            println!(
                "Memória da GPU integrada (ttm): {} agora, {} no boot",
                bootcfg::ttm_now()
                    .map(|p| format!("{:.1} GiB", gib(p)))
                    .unwrap_or("?".into()),
                bootcfg::saved_ttm()
                    .map(|p| format!("{:.1} GiB", gib(p)))
                    .unwrap_or("padrão (metade da RAM)".into())
            );
            println!("Arquivo: {}", bootcfg::conf_path().display());
            match bootcfg::initramfs_tool() {
                Some(t) => println!("Imagem de boot refeita com: {}", t.join(" ")),
                None => println!("Nenhuma ferramenta de initramfs encontrada"),
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
                        .context("use um número em GiB (ex.: 14) ou padrao")?,
                )
            };
            bootcfg::set_gpu_memory(gib, &mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))
        }
        _ => bail!(
            "use: numapilot boot status | sudo numapilot boot overdrive on|off | sudo numapilot boot gpu-memory GiB|padrao | numapilot boot cmdline [add|remove PARAM...]"
        ),
    }
}

fn cmd_steam(action: &str) -> Result<()> {
    use numapilot::steam;
    let games = steam::installed_games();
    let lc = steam::find_localconfigs();
    let Some(path) = lc.first() else {
        bail!("não achei a configuração da Steam (localconfig.vdf)")
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
                "{n} jogo(s) alterado(s) em {} (backup ao lado).",
                path.display()
            );
            Ok(())
        }
        _ => bail!("use: numapilot steam enable | disable | list"),
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
    let s = std::fs::read_to_string(daemon::state_path())
        .context("o daemon não parece estar rodando (systemctl status numapilot)")?;
    let rows: Vec<daemon::Row> = serde_json::from_str(&s)?;
    println!(
        "{:>7}  {:<16} {:>6}  {:<12} {:<6} {:<10} {:<14} origem",
        "PID", "PROGRAMA", "NÚCL.", "ESTADO", "NÓS", "GPU", "CPUs"
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
        _ => bail!("em `pin` use um número de nó, uma lista (0,1) ou all"),
    };
    for n in &nodes {
        if topo.node(*n).is_none() {
            bail!("nó {n} não existe");
        }
    }
    let st = procfs::read_stat(pid).context("processo não encontrado")?;
    let cpus = topo.cpus_of(&nodes);
    let ok = procfs::set_affinity_all(pid, &cpus);
    if ok == 0 {
        bail!("sem permissão (use sudo para processos de outros usuários)");
    }
    println!(
        "{} (pid {pid}): {ok} threads → nó(s) {:?} [{}]",
        st.comm,
        nodes,
        format_cpulist(&cpus)
    );
    if !no_migrate && nodes.len() < topo.nodes.len() {
        let from: Vec<usize> = topo
            .node_ids()
            .into_iter()
            .filter(|x| !nodes.contains(x))
            .collect();
        match procfs::migrate_pages(pid, &from, &nodes) {
            Ok(_) => println!("memória migrada para o(s) nó(s) {:?}", nodes),
            Err(e) => println!("aviso: migração de memória falhou: {e}"),
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
            "aviso: não consegui registrar o pin para o daemon (rode com sudo); ele pode reposicionar o processo."
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
        l.save().context("não consegui salvar (precisa de sudo?)")?;
        println!(
            "{} entrada(s) removida(s). Reinicie o daemon para ele recarregar.",
            before - l.map.len()
        );
        return Ok(());
    }
    if l.map.is_empty() {
        println!("Nada aprendido ainda ({}).", Learned::path().display());
    }
    for (k, e) in &l.map {
        println!(
            "{}  {}  pico {:.0}% de um nó  gpu-nó {}  execuções {}",
            if e.wide { "[2 sockets]" } else { "[1 socket] " },
            k,
            e.peak_frac * 100.0,
            e.gpu_node
                .map(|n| n.to_string())
                .unwrap_or_else(|| "-".into()),
            e.runs
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
                println!("== journal do serviço (este boot) ==");
                print!("{t}");
            }
        }
    }
    if shown == 0 {
        println!("Nenhum registro ainda.");
    }
    Ok(())
}
