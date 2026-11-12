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
    about = "Chooses socket (NUMA), memory and GPU for each program"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Shows NUMA nodes, cores, memory and GPUs (with each one's socket).
    Topo,
    /// Runs a program on the chosen socket/GPU (automatic by default).
    Run {
        /// auto | all | gpu | 0 | 1 | 0,1 | cpus:0-21
        #[arg(long, short = 'c')]
        cpu: Option<String>,
        /// auto | display | compute | compute:2 | compute:all | none | alias/PCI (comma)
        #[arg(long, short = 'g')]
        gpu: Option<String>,
        /// local | interleave | bind
        #[arg(long, short = 'm')]
        mem: Option<String>,
        /// How to use the GPU: graphics (game, even on an MI50) | compute (ROCm) | auto
        #[arg(long = "use", short = 'u')]
        usage: Option<String>,
        /// GPU for Lossless Scaling frame generation (lsfg-vk). Default: the same as the game.
        #[arg(long)]
        fg_gpu: Option<String>,
        /// Shortcut for --use compute.
        #[arg(long, conflicts_with_all = ["graphics", "usage"])]
        compute: bool,
        /// Shortcut for --use graphics.
        #[arg(long, conflicts_with = "usage")]
        graphics: bool,
        /// Only shows the decision, without running.
        #[arg(long, short = 'n')]
        dry_run: bool,
        #[arg(trailing_var_arg = true, required = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Automatic mode daemon (run it as a systemd service, as root).
    Daemon,
    /// Lists the processes the daemon is managing right now.
    Status,
    /// Pins an already running process to a node (or all) and migrates its memory.
    Pin {
        pid: i32,
        /// 0 | 1 | all | 0,1
        #[arg(long, short = 'c')]
        cpu: String,
        #[arg(long)]
        no_migrate: bool,
    },
    /// Tests each part on the real hardware and says what to fix.
    Doctor {
        /// JSON output.
        #[arg(long)]
        json: bool,
        /// Generates ~/numapilot-relatorio.txt (diagnosis + system) to send.
        #[arg(long)]
        report: bool,
    },
    /// Boot options that survive kernel updates (needs sudo to change). status | overdrive on|off
    /// | gpu-memory GiB|default | cmdline [add|remove PARAM...]
    Boot {
        what: String,
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },
    /// Special boards (BC-250...): setup (what is left to configure) | status | governor-apply
    /// FILE (with sudo)
    Board {
        action: String,
        file: Option<std::path::PathBuf>,
    },
    /// Community recipes: show | update [URL] (update with sudo)
    Recipes { action: String, url: Option<String> },
    /// Upscaling: fetch (downloads OptiScaler) | status|install|remove APPID [--int8]
    Upscale {
        action: String,
        appid: Option<String>,
        #[arg(long)]
        int8: bool,
    },
    /// Other PCs: list | add LINK [NAME] | remove NAME | wake NAME (wakes over the network) |
    /// pull NAME [todos] (copies profiles) | backup (stores everyone's)
    Peers {
        action: String,
        arg: Option<String>,
        name: Option<String>,
    },
    /// Windows virtual machine with a card of its own: check [GPU] | prepare GPU | unprepare |
    /// create NAME --gpu GPU --iso FILE
    Vm {
        action: String,
        arg: Option<String>,
        #[arg(long)]
        gpu: Option<String>,
        #[arg(long)]
        iso: Option<std::path::PathBuf>,
        /// Memory (GB)
        #[arg(long, default_value_t = 16)]
        mem: u32,
        /// Logical cores
        #[arg(long, default_value_t = 8)]
        cpus: usize,
        /// Disk (GB)
        #[arg(long, default_value_t = 120)]
        disk: u32,
    },
    /// systemd services: list [--user] [--running|--failed] | start U | stop U | restart U |
    /// log U
    #[command(name = "services", alias = "servicos")]
    Services {
        action: Option<String>,
        unit: Option<String>,
        /// User services (systemctl --user) instead of the system ones
        #[arg(long = "user", alias = "usuario")]
        user: bool,
        /// Only the running ones
        #[arg(long = "running", alias = "rodando")]
        running: bool,
        /// Only the failed ones
        #[arg(long = "failed", alias = "falhas")]
        failed: bool,
        /// Log lines
        #[arg(short = 'n', long = "lines", alias = "linhas", default_value_t = 60)]
        lines: u32,
    },
    /// What starts when the system boots: list | on ID | off ID | remove ID | command NAME
    /// "COMMAND" | file PATH | app SEARCH | source ID | uninstall ID
    #[command(name = "startup", alias = "inicio")]
    Startup {
        action: Option<String>,
        args: Vec<String>,
        /// desinstalar: also delete the program's settings and data
        #[arg(long)]
        config: bool,
        /// desinstalar: do not ask
        #[arg(long = "yes", alias = "sim")]
        yes: bool,
        /// Seconds to wait before starting (comando, arquivo, app)
        #[arg(long = "delay", alias = "atraso", default_value_t = 0)]
        delay: u32,
    },
    /// Laptop with NVIDIA: what starts with the system on the integrated GPU: status | igpu |
    /// normal
    Autostart {
        action: Option<String>,
        /// Only does something on a laptop with integrated + NVIDIA (for the installer)
        #[arg(long)]
        if_hybrid: bool,
    },
    /// Laptop with NVIDIA: status | auto | hybrid | integrated | leave | xwayland on|off
    Hybrid {
        action: Option<String>,
        arg: Option<String>,
    },
    /// Updates numapilot itself (package or code folder): status | run
    #[command(name = "self-update")]
    SelfUpdate { action: Option<String> },
    /// Automatic undervolt of a GPU (alias or PCI): tests step by step under load and saves the
    /// last stable one
    Undervolt {
        gpu: String,
        /// Seconds of load per step
        #[arg(long, default_value_t = 60)]
        secs: u64,
        /// Margin steps at the end
        #[arg(long, default_value_t = 1)]
        margin: u32,
        /// NVIDIA: maximum clock to lock (MHz)
        #[arg(long)]
        clock: Option<u32>,
        /// Step size (MHz on NVIDIA, mV on AMD)
        #[arg(long)]
        step: Option<i32>,
    },
    /// Official NVIDIA driver: status | install | update (install/update with sudo)
    Driver { action: String },
    /// Stores all settings in a .tar.gz (to reinstall after formatting).
    Backup {
        /// Destination file or folder (default: your home folder). Keep it off the system disk.
        #[arg(long)]
        out: Option<std::path::PathBuf>,
    },
    /// Restores a backup (use sudo). Also reapplies OverDrive and kernel parameters.
    Restore {
        file: std::path::PathBuf,
        /// Leaves the boot alone (OverDrive and kernel parameters).
        #[arg(long)]
        no_boot: bool,
    },
    /// Machine settings: status | mode game|ai|quiet|normal
    Machine {
        action: String,
        value: Option<String>,
    },
    /// Web panel for the phone (run as your user; see the link with the token).
    Web {
        #[arg(long, default_value_t = numapilot::web::DEFAULT_PORT)]
        port: u16,
        /// Listen address (0.0.0.0 = local network; 127.0.0.1 = this machine only).
        #[arg(long, default_value = "0.0.0.0")]
        bind: String,
        /// Only shows the link and exits.
        #[arg(long)]
        url: bool,
    },
    /// FPS optimizer: tests settings each time the game starts (measures with MangoHud).
    Tune {
        /// start | status | stop | apply
        action: String,
        /// Program key (e.g. steam:1245620); optional in status.
        key: Option<String>,
    },
    /// Steam: puts numapilot in (or removes it from) the launch options of every game.
    Steam {
        /// enable | disable | list
        action: String,
    },
    /// Measures memory latency and bandwidth between the sockets.
    Bench {
        /// Buffer size per test (MB).
        #[arg(long, default_value_t = 128)]
        size_mb: usize,
        /// Threads in the bandwidth test.
        #[arg(long, default_value_t = 8)]
        threads: usize,
    },
    /// Per-program profiles (the same as in the graphical interface).
    Profile {
        #[command(subcommand)]
        action: ProfileCmd,
    },
    /// Prints the ROCR_VISIBLE_DEVICES IDs of the compute GPUs (for scripts).
    #[command(name = "rocr-ids")]
    RocrIds,
    /// Checks the card selection: runs vulkaninfo and glxinfo the way a game would on each card
    /// and says which one answered.
    GpuTest {
        /// Only this card (alias, e.g. amd-0). Without it, all of them.
        gpu: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Measures the cache latency of each core and starts preferring the fastest ones (if the
    /// difference is larger than the noise). Leave the PC idle; takes a few seconds.
    BenchCores {
        /// Only shows the last saved measurement.
        #[arg(long = "show", alias = "mostrar")]
        show: bool,
        /// Deletes the measurement (core choice goes back to the system order).
        #[arg(long = "clear", alias = "apagar")]
        clear: bool,
    },
    /// Turbo by awake cores: the processor's table and how many cores are awake right now.
    Turbo,
    /// Shows the end of the logs (service, graphical program, command line) and of the journal.
    Log {
        /// Lines from each log.
        #[arg(short = 'n', default_value_t = 40)]
        lines: usize,
        /// Only this one: service | gui | cli | tray | web
        which: Option<String>,
    },
    /// Tests every part of numapilot on this machine (read-only, changes nothing) and writes a
    /// report.
    #[command(name = "selftest", alias = "autoteste")]
    Selftest {
        /// Report file (default: ~/numapilot-autoteste-MACHINE-DATE.txt).
        #[arg(long = "out", alias = "saida")]
        out: Option<std::path::PathBuf>,
        /// Skips the tests that run numapilot's commands one by one.
        #[arg(long = "quick", alias = "rapido")]
        quick: bool,
        /// JSON output (on screen).
        #[arg(long)]
        json: bool,
    },
    /// Shows or deletes what automatic mode has learned.
    Learned {
        /// Deletes what was learned about programs whose path contains this text.
        #[arg(long)]
        forget: Option<String>,
    },
}

#[derive(Subcommand)]
enum ProfileCmd {
    /// Lists the saved profiles.
    List,
    /// Saves a profile. KEY is the executable (full path) or steam:APPID.
    Set {
        key: String,
        #[arg(long, default_value = "auto")]
        cpu: String,
        #[arg(long, default_value = "auto")]
        gpu: String,
        #[arg(long = "use", default_value = "auto")]
        usage: String,
    },
    /// Deletes a profile (the program goes back to automatic).
    Rm { key: String },
    /// Writes all choices to a file, to take to another PC or share.
    Export {
        /// File (default: ~/numapilot-escolhas-MACHINE.json).
        file: Option<std::path::PathBuf>,
    },
    /// Brings in the choices from an exported file (the ones here with the same key are
    /// replaced).
    Import { file: std::path::PathBuf },
}

fn main() {
    // `numapilot hybrid | head`: when the pipe closes, exit quietly instead of dying with Broken pipe
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    if let Err(e) = real_main() {
        numapilot::log::error(&format!("{e:#}"));
        eprintln!(
            "{}",
            numapilot::tf!("numapilot: error: {e:#}", "numapilot: erro: {e:#}")
        );
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Daemon => numapilot::log::init("daemon"),
        Cmd::Web { .. } => numapilot::log::init("web"),
        // only reading its own log: no need to log
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
        Cmd::BenchCores { show, clear } => cmd_bench_cores(&cfg, show, clear),
        Cmd::GpuTest { gpu, json } => cmd_gpu_test(&cfg, gpu.as_deref(), json),
        Cmd::Selftest { out, quick, json } => {
            let r = numapilot::selftest::run(&cfg, !quick, &mut |l| {
                if !json {
                    println!("{l}")
                }
            });
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
                return Ok(());
            }
            let path = out.unwrap_or_else(numapilot::selftest::default_path);
            std::fs::write(&path, numapilot::selftest::render(&r))?;
            println!();
            println!("{}", r.summary());
            println!(
                "{}",
                numapilot::tf!("Report: {}", "Relatório: {}", path.display())
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
                        "\nReport saved to {}: just send me that file.",
                        "\nRelatório salvo em {}: é só me mandar esse arquivo.",
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
                        "No known special board in this PC.",
                        "Nenhuma placa especial conhecida neste PC."
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
                                "{} ({}): first setup: {}",
                                "{} ({}): primeira configuração: {}",
                                b.name,
                                g.alias,
                                numapilot::boards::setup_summary(&steps)
                            )
                        );
                        for s in &steps {
                            let tag = match s.state {
                                StepState::Ok => numapilot::i18n::tr("ready   "),
                                StepState::Todo => numapilot::i18n::tr("MISSING "),
                                StepState::Opcional => numapilot::i18n::tr("optional"),
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
                                    numapilot::i18n::tr("running")
                                } else {
                                    numapilot::i18n::tr("stopped")
                                }
                            );
                        }
                        if let Some(c) = numapilot::boards::governor_config(b) {
                            println!(
                                "{}",
                                numapilot::tf!("  configuration: {c}", "  configuração: {c}")
                            );
                        }
                        for n in &b.notes {
                            println!("  - {n}");
                        }
                    }
                    Ok(())
                }
                "governor-apply" => {
                    let f = file.context(numapilot::i18n::tr("the file is missing"))?;
                    let b = boards
                        .iter()
                        .map(|x| x.0)
                        .find(|b| !b.governor_configs.is_empty())
                        .context(numapilot::i18n::tr("no board with a governor"))?;
                    numapilot::boards::governor_apply(b, &f, &mut |m| println!("{m}"))
                        .map_err(|e| anyhow::anyhow!(e))
                }
                _ => bail!(
                    "{}",
                    numapilot::tf!(
                        "usage: numapilot board status | sudo numapilot board governor-apply FILE",
                        "use: numapilot board status | sudo numapilot board governor-apply ARQUIVO"
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
                        "Recipes version {}: {} boards, {} processors, {} architectures",
                        "Receitas versão {}: {} placas, {} processadores, {} arquiteturas",
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
                                "  {}: {} · compute {} · upscaling {}",
                                "  {}: {} · computação {} · upscaling {}",
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
                        "Recipes updated (version {v}).",
                        "Receitas atualizadas (versão {v})."
                    )
                );
                Ok(())
            }
            _ => bail!(
                "{}",
                numapilot::tf!(
                    "usage: numapilot recipes show | sudo numapilot recipes update [URL]",
                    "use: numapilot recipes show | sudo numapilot recipes update [URL]"
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
                    numapilot::tf!("OptiScaler {v} ready.", "OptiScaler {v} pronto.")
                );
                return Ok(());
            }
            let id = appid.context(numapilot::i18n::tr("the Steam appid is missing"))?;
            let root = upscale::steam_game_root(&id)
                .context(numapilot::i18n::tr("game not found in the Steam libraries"))?;
            let dir = upscale::exe_dir(&root)
                .context(numapilot::i18n::tr("could not find the game's executable"))?;
            match action.as_str() {
                "status" => {
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Executable folder: {}",
                            "Pasta do executável: {}",
                            dir.display()
                        )
                    );
                    match upscale::installed(&dir) {
                        Some(i) => println!(
                            "{}",
                            numapilot::tf!(
                                "OptiScaler {} installed ({} files)",
                                "OptiScaler {} instalado ({} arquivos)",
                                i.version,
                                i.files.len()
                            )
                        ),
                        None => println!(
                            "{}",
                            numapilot::tf!("OptiScaler not installed", "OptiScaler não instalado")
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
                            "OptiScaler {v} installed in {}. Do not use it in online games (the anti-cheat may ban you).",
                            "OptiScaler {v} instalado em {}. Não use em jogos online (anti-cheat pode banir).",
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
                            "OptiScaler removed from {}",
                            "OptiScaler removido de {}",
                            dir.display()
                        )
                    );
                    Ok(())
                }
                _ => bail!(
                    "{}",
                    numapilot::tf!(
                        "usage: numapilot upscale fetch | status|install|remove APPID [--int8]",
                        "use: numapilot upscale fetch | status|install|remove APPID [--int8]"
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
                                    "hottest GPU {t:.0} °C",
                                    "GPU mais quente {t:.0} °C"
                                ))
                                .unwrap_or_default()
                        );
                    }
                    Ok(())
                }
                "add" => {
                    let p = peers::add(
                        &arg.context(numapilot::i18n::tr(
                            "the link is missing (the one from the other PC's QR code)",
                        ))?,
                        name.as_deref(),
                    )
                    .map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "{}",
                        numapilot::tf!("{} added ({})", "{} adicionado ({})", p.name, p.url)
                    );
                    Ok(())
                }
                "remove" => {
                    peers::remove(&arg.context(numapilot::i18n::tr("the name is missing"))?)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!("{}", numapilot::i18n::tr("removed"));
                    Ok(())
                }
                "pull" => {
                    let m = peers::pull_profiles(
                        &arg.context(numapilot::i18n::tr("the name is missing"))?,
                        matches!(name.as_deref(), Some("todos" | "all")),
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
                    let m = peers::wake(&arg.context(numapilot::i18n::tr("the name is missing"))?)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!("{m}");
                    Ok(())
                }
                _ => bail!(
                    "{}",
                    numapilot::tf!(
                        "usage: numapilot peers list | add LINK [NAME] | remove NAME | wake NAME | pull NAME [all] | backup",
                        "use: numapilot peers list | add LINK [NOME] | remove NOME | wake NOME | pull NOME [all] | backup"
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
                                numapilot::t2!("missing", "falta")
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
                                "(name the card to check it too: numapilot vm check rx470-0)",
                                "(diga a placa para conferir ela também: numapilot vm check rx470-0)"
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
                        .context(numapilot::i18n::tr("name the card (see numapilot topo)"))?;
                    vm::prepare(&topo, &g, &mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))
                }
                "unprepare" => {
                    vm::unprepare(&mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))
                }
                "create" => {
                    let name = arg.context(numapilot::i18n::tr("name the VM"))?;
                    let g = gpu
                        .as_deref()
                        .and_then(find)
                        .context(numapilot::i18n::tr("--gpu CARD"))?;
                    let iso = iso.context(numapilot::i18n::tr("--iso Windows file (.iso)"))?;
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
                            "VM {name} created: open it in virt-manager to install Windows. Then install the card's driver inside it.",
                            "VM {name} criada: abra no virt-manager para instalar o Windows. Depois instale o driver da placa dentro dela."
                        )
                    );
                    Ok(())
                }
                _ => bail!(
                    "{}",
                    numapilot::tf!(
                        "usage: numapilot vm check [GPU] | sudo numapilot vm prepare GPU | sudo numapilot vm unprepare | numapilot vm create NAME --gpu GPU --iso FILE.iso [--mem 16 --cpus 8 --disk 120]",
                        "use: numapilot vm check [GPU] | sudo numapilot vm prepare GPU | sudo numapilot vm unprepare | numapilot vm create NOME --gpu GPU --iso ARQ.iso [--mem 16 --cpus 8 --disk 120]"
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
                            "{} (applies to the next Xwayland: log out and in, or close the X11 programs)",
                            "{} (vale para o próximo Xwayland: saia e entre na sessão, ou feche os programas X11)",
                            if on {
                                numapilot::i18n::tr("Xwayland using only Mesa")
                            } else {
                                numapilot::i18n::tr("normal Xwayland")
                            }
                        )
                    );
                    Ok(())
                }
                "auto" | "hibrido" | "integrada" | "nao-mexer" | "hybrid" | "integrated"
                | "leave" => {
                    // English names on the command line; the stored value does not change
                    let act = match act {
                        "hybrid" => "hibrido",
                        "integrated" => "integrada",
                        "leave" => "nao-mexer",
                        o => o,
                    };
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
                            "NVIDIA mode: {act} (the service applies it within seconds, without leaving the session)",
                            "modo da NVIDIA: {act} (o serviço aplica em segundos, sem sair da sessão)"
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
                                                numapilot::i18n::tr(" [display]")
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
                                    "{}: turned off by numapilot (removed from the system)",
                                    "{}: desligada pelo numapilot (fora do sistema)",
                                    st.as_ref().map(|s| s.alias.as_str()).unwrap_or("NVIDIA")
                                )
                            );
                        }
                        None => {
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "Not a laptop with an integrated card + NVIDIA.",
                                    "Não é um notebook com placa integrada + NVIDIA."
                                )
                            );
                            return Ok(());
                        }
                    }
                    let m = numapilot::machine::Machine::load().dgpu_mode;
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Power: {}",
                            "Energia: {}",
                            match numapilot::notebook::on_ac() {
                                Some(true) => numapilot::i18n::tr("AC power"),
                                Some(false) => numapilot::i18n::tr("battery"),
                                None => "?",
                            }
                        )
                    );
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Mode: {}",
                            "Modo: {}",
                            m.as_deref()
                                .and_then(|m| hybrid::MODES.iter().find(|x| x.0 == m))
                                .map(|x| numapilot::i18n::tr(x.1))
                                .unwrap_or(numapilot::i18n::tr("leave as is"))
                        )
                    );
                    if let Some(st) = &st {
                        if !st.blocked_by.is_empty() {
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "Waiting to turn off: in use by {}",
                                    "Esperando para desligar: em uso por {}",
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
                                    "KDE holds the NVIDIA's video output: it is not removed from the system, but sleeps by itself ({}).",
                                    "O KDE segura a saída de vídeo da NVIDIA: ela não é tirada do sistema, mas dorme sozinha ({}).",
                                    numapilot::notebook::dgpu_state(&st.pci)
                                )
                            );
                        }
                    }
                    println!(
                        "Xwayland: {}",
                        if hybrid::xwayland_wrapped() {
                            numapilot::i18n::tr("Mesa only (does not hold the NVIDIA)")
                        } else {
                            numapilot::i18n::tr("normal (may hold the NVIDIA)")
                        }
                    );
                    Ok(())
                }
            }
        }
        Cmd::Services {
            action,
            unit,
            user,
            running,
            failed,
            lines,
        } => {
            use numapilot::services::{self, Verb};
            let all = services::list(user);
            let find = |u: &Option<String>| -> anyhow::Result<services::Service> {
                let u = u.as_deref().ok_or_else(|| {
                    anyhow::anyhow!(
                        "{}",
                        numapilot::tf!("the service name is missing", "falta o nome do serviço")
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
                            "service {full} not found{}",
                            "serviço {full} não encontrado{}",
                            if user {
                                ""
                            } else {
                                numapilot::i18n::tr(" (a user service? use --user)")
                            }
                        )
                    )
                })
            };
            let verb = match action.as_deref().unwrap_or("lista") {
                "lista" | "list" => {
                    for s in all
                        .iter()
                        .filter(|s| (!running || s.running()) && (!failed || s.failed()))
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
                    print!("{}", services::logs(&find(&unit)?, lines));
                    return Ok(());
                }
                "iniciar" | "start" => Verb::Start,
                "parar" | "stop" => Verb::Stop,
                "reiniciar" | "restart" => Verb::Restart,
                o => bail!(
                    "{}",
                    numapilot::tf!("unknown action: {o}", "ação desconhecida: {o}")
                ),
            };
            let s = find(&unit)?;
            let c = services::action_command(&s, verb).map_err(|e| anyhow::anyhow!(e))?;
            let st = std::process::Command::new(&c[0]).args(&c[1..]).status()?;
            if !st.success() {
                bail!(
                    "{}",
                    numapilot::tf!(
                        "{} finished with an error",
                        "{} terminou com erro",
                        c.join(" ")
                    )
                );
            }
            println!("{}: {}", s.unit, verb.text().to_lowercase());
            Ok(())
        }
        Cmd::Startup {
            action,
            args,
            delay,
            config,
            yes,
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
                                "could not find \"{id}\" (see numapilot startup list)",
                                "não achei \"{id}\" (veja numapilot startup list)"
                            )
                        )
                    })
            };
            let arg = |i: usize| {
                args.get(i).cloned().ok_or_else(|| {
                    anyhow::anyhow!(
                        "{}",
                        numapilot::tf!(
                            "an argument is missing (see numapilot startup --help)",
                            "falta um argumento (veja numapilot startup --help)"
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
                                numapilot::tf!("running, {} MB", "rodando, {} MB", m / 1048576)
                            }
                            (true, None) => numapilot::i18n::tr("running").into(),
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
                                numapilot::i18n::tr(" (protected)")
                            } else {
                                ""
                            }
                        );
                    }
                    Ok(())
                }
                v @ ("ligar" | "desligar" | "on" | "off" | "enable" | "disable") => {
                    let it = find(&arg(0)?)?;
                    let on = matches!(v, "ligar" | "on" | "enable");
                    match startup::set_enabled(&it, on).map_err(|e| anyhow::anyhow!(e))? {
                        Action::Done => {
                            println!(
                                "{}: {}",
                                it.name,
                                if on {
                                    numapilot::i18n::tr("on")
                                } else {
                                    numapilot::i18n::tr("off")
                                }
                            )
                        }
                        Action::NeedsRoot(c) => {
                            let st = std::process::Command::new(&c[0]).args(&c[1..]).status()?;
                            if !st.success() {
                                bail!(
                                    "{}",
                                    numapilot::tf!(
                                        "{} finished with an error",
                                        "{} terminou com erro",
                                        c.join(" ")
                                    )
                                );
                            }
                        }
                    }
                    Ok(())
                }
                v @ ("origem" | "desinstalar" | "source" | "uninstall") => {
                    use numapilot::origin;
                    let it = find(&arg(0)?)?;
                    let t = origin::trace(&it.exec, it.file.as_deref());
                    println!("{}", it.name);
                    if let Some(f) = &t.file {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "  started by: {}{}",
                                "  abre por:   {}{}",
                                f.display(),
                                t.file_pkg
                                    .as_ref()
                                    .map(|p| numapilot::tf!(
                                        " (from package {p})",
                                        " (do pacote {p})"
                                    ))
                                    .unwrap_or_default()
                            )
                        );
                    }
                    println!(
                        "{}",
                        numapilot::tf!("  command:    {}", "  comando:    {}", it.exec)
                    );
                    if let Some(e) = &t.exe {
                        println!(
                            "{}",
                            numapilot::tf!("  program:    {}", "  programa:   {}", e.display())
                        );
                    }
                    println!(
                        "{}",
                        numapilot::tf!("  source:     {}", "  origem:     {}", t.source.text())
                    );
                    if let Some(d) = &t.description {
                        println!(
                            "{}",
                            numapilot::tf!("  description: {d}", "  descrição:  {d}")
                        );
                    }
                    if let Some(sz) = t.size.filter(|s| *s > 0) {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "  size:       {:.1} MB",
                                "  tamanho:    {:.1} MB",
                                sz as f64 / 1048576.0
                            )
                        );
                    }
                    if let Some(d) = &t.app_dir {
                        println!(
                            "{}",
                            numapilot::tf!("  folder:     {}", "  pasta:      {}", d.display())
                        );
                    }
                    for c in &t.configs {
                        println!("  config:     {}", c.display());
                    }
                    if matches!(v, "origem" | "source") {
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
                                "cannot be uninstalled: {why}",
                                "não dá para desinstalar: {why}"
                            )
                        );
                    }
                    println!("{}", numapilot::tf!("\nIt will run:", "\nVai fazer:"));
                    for s in &plan.steps {
                        println!("  - {s}");
                    }
                    if !yes {
                        print!(
                            "{}",
                            numapilot::tf!("Continue? [y/N] ", "Continuar? [s/N] ")
                        );
                        use std::io::Write;
                        std::io::stdout().flush()?;
                        let mut a = String::new();
                        std::io::stdin().read_line(&mut a)?;
                        if !["s", "y"].contains(&a.trim().to_lowercase().as_str()) {
                            println!("{}", numapilot::tf!("Nothing done.", "Nada feito."));
                            return Ok(());
                        }
                    }
                    let (_, refused) =
                        numapilot::procctl::terminate(&it.pids, unsafe { libc::getuid() }, false);
                    if let Some(r) = refused {
                        eprintln!(
                            "{}",
                            numapilot::tf!("(not closed: {r})", "(não encerrei: {r})")
                        );
                    }
                    let st = std::process::Command::new("sh")
                        .args(["-c", &plan.script])
                        .status()?;
                    if !st.success() {
                        bail!(
                            "{}",
                            numapilot::tf!(
                                "the uninstall finished with an error",
                                "a desinstalação terminou com erro"
                            )
                        );
                    }
                    Ok(())
                }
                "remover" | "rm" | "remove" => {
                    let it = find(&arg(0)?)?;
                    startup::remove(&it).map_err(|e| anyhow::anyhow!(e))?;
                    println!("{}", numapilot::tf!("{} removed", "{} removido", it.name));
                    Ok(())
                }
                "comando" | "command" => {
                    let p = startup::add_command(&arg(0)?, &args[1..].join(" "), delay)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "{}",
                        numapilot::tf!("added: {}", "adicionado: {}", p.display())
                    );
                    Ok(())
                }
                "arquivo" | "file" => {
                    let p = startup::add_file(std::path::Path::new(&arg(0)?), delay)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    println!(
                        "{}",
                        numapilot::tf!("added: {}", "adicionado: {}", p.display())
                    );
                    Ok(())
                }
                "app" => {
                    let q = args.join(" ");
                    let found = startup::search_apps(&q);
                    match found.as_slice() {
                        [(n, p)] => {
                            let d =
                                startup::add_desktop(p, delay).map_err(|e| anyhow::anyhow!(e))?;
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "{n} will open at login ({})",
                                    "{n} vai abrir no login ({})",
                                    d.display()
                                )
                            );
                        }
                        [] => bail!(
                            "{}",
                            numapilot::tf!(
                                "no installed program matching \"{q}\"",
                                "nenhum programa instalado com \"{q}\""
                            )
                        ),
                        many => {
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "More than one program; be more specific:",
                                    "Mais de um programa; seja mais específico:"
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
                    numapilot::tf!("unknown action: {o}", "ação desconhecida: {o}")
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
                                "Everything that starts with the system is already on the integrated GPU.",
                                "Tudo que abre com o sistema já está na integrada."
                            )
                        );
                    } else {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "Now on the integrated GPU (applies from the next login): {}",
                                "Agora na integrada (vale a partir do próximo login): {}",
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
                            "Back to normal: {}",
                            "Voltaram ao normal: {}",
                            if ch.is_empty() {
                                numapilot::i18n::tr("nothing").into()
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
                            "On the integrated GPU: {}",
                            "Na integrada: {}",
                            if autostart::enabled() {
                                numapilot::i18n::tr("on")
                            } else {
                                numapilot::i18n::tr("off")
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
                                "  {s:<40} (user service)",
                                "  {s:<40} (serviço do usuário)"
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
                    numapilot::tf!("installed from package {p}", "instalado pelo pacote {p}")
                ),
                How::Source(d) => {
                    println!(
                        "{}",
                        numapilot::tf!(
                            "installed from the folder {}",
                            "instalado pela pasta {}",
                            d.display()
                        )
                    );
                    let ahead = selfupdate::source_ahead_of_installed(d).unwrap_or(0);
                    if ahead > 0 {
                        pending = true;
                        println!(
                            "{}",
                            numapilot::tf!(
                                "the folder has {ahead} commit(s) not installed yet",
                                "a pasta tem {ahead} commit(s) que ainda não foram instalados"
                            )
                        );
                    }
                    match selfupdate::source_behind(d) {
                        Some(0) if ahead == 0 => println!(
                            "{}",
                            numapilot::tf!(
                                "already on the newest version",
                                "já está na versão mais nova"
                            )
                        ),
                        Some(0) => println!(
                            "{}",
                            numapilot::tf!(
                                "nothing new in the remote repository",
                                "nada novo no repositório remoto"
                            )
                        ),
                        Some(n) => {
                            pending = true;
                            println!(
                                "{}",
                                numapilot::tf!(
                                    "{n} new update(s) in the repository",
                                    "{n} atualização(ões) novas no repositório"
                                )
                            )
                        }
                        None => println!(
                            "{}",
                            numapilot::tf!(
                                "could not query the repository",
                                "não consegui consultar o repositório"
                            )
                        ),
                    }
                }
                How::Unknown => println!(
                    "{}",
                    numapilot::tf!("unknown source", "origem desconhecida")
                ),
            }
            if pending && action.as_deref() != Some("run") {
                println!(
                    "{}",
                    numapilot::tf!(
                        "to install: numapilot self-update run",
                        "para instalar: numapilot self-update run"
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
                            "the update finished with an error ({st})",
                            "a atualização terminou com erro ({st})"
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
                        "GPU '{gpu}' not found (see numapilot topo)",
                        "GPU '{gpu}' não encontrada (veja numapilot topo)"
                    )
                })?
                .clone();
            numapilot::undervolt::catch_ctrl_c();
            println!(
                "{}",
                numapilot::tf!(
                    "Leave the PC idle during the test. Ctrl+C to stop and go back to what it was.",
                    "Deixe o PC parado durante o teste. Ctrl+C para parar e voltar ao que era."
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
                            "\nDone: {} ({} stable step(s)).",
                            "\nPronto: {} ({} passo(s) estáveis).",
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
                                "No NVIDIA card in this computer.",
                                "Nenhuma placa NVIDIA neste computador."
                            )
                        );
                        return Ok(());
                    }
                    for g in &s.gpus {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "Card: {} ({}, id {:04x})",
                                "Placa: {} ({}, id {:04x})",
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
                                "Recommended driver: {}",
                                "Driver indicado: {}",
                                b.label()
                            )
                        );
                    }
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Loaded now: {}",
                            "Carregado agora: {}",
                            s.loaded
                                .clone()
                                .unwrap_or(numapilot::t2!("none", "nenhum").into())
                        )
                    );
                    println!(
                        "{}",
                        numapilot::tf!(
                            "Installed: {}",
                            "Instalado: {}",
                            s.installed
                                .clone()
                                .unwrap_or(numapilot::i18n::tr("no").into())
                        )
                    );
                    if let Some(a) = &s.available {
                        println!(
                            "{}",
                            numapilot::tf!("In the repository: {a}", "No repositório: {a}")
                        );
                    }
                    if s.nouveau {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "Using nouveau (the community's open driver) right now.",
                                "Usando o nouveau (driver aberto da comunidade) agora."
                            )
                        );
                    }
                    if s.needs_reboot {
                        println!(
                            "{}",
                            numapilot::tf!(
                                "The installed driver differs from the loaded one: reboot.",
                                "O driver instalado é diferente do carregado: reinicie."
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
                        "usage: numapilot driver status | sudo numapilot driver install | sudo numapilot driver update",
                        "use: numapilot driver status | sudo numapilot driver install | sudo numapilot driver update"
                    )
                ),
            }
        }
        Cmd::Backup { out } => {
            let f = numapilot::backup::create(out.as_deref()).map_err(|e| anyhow::anyhow!(e))?;
            println!(
                "{}",
                numapilot::tf!("Backup saved to {}", "Backup salvo em {}", f.display())
            );
            println!(
                "{}",
                numapilot::tf!(
                    "Copy it off the system disk (USB stick, another disk, cloud) before formatting.",
                    "Copie para fora do disco do sistema (pendrive, outro disco, nuvem) antes de formatar."
                )
            );
            println!(
                "{}",
                numapilot::tf!(
                    "To restore: ./install.sh --restore {}",
                    "Para restaurar: ./install.sh --restore {}",
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
                    numapilot::web::url(port).context(numapilot::i18n::tr("no network"))?
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
                    "Measuring {} node(s); takes a few seconds per pair...",
                    "Medindo {} nó(s); leva alguns segundos por par...",
                    t.nodes.len()
                )
            );
            let r = numapilot::bench::run(&t, size_mb, threads, |m| println!("  {m}"));
            numapilot::bench::print(&r);
            if let Some(p) = numapilot::bench::save(&r) {
                println!(
                    "{}",
                    numapilot::tf!(
                        "\nResult saved to {} (shows up in the interface).",
                        "\nResultado salvo em {} (aparece na interface).",
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
    println!("{}", numapilot::tf!("NUMA nodes", "Nós NUMA"));
    for n in &t.nodes {
        println!(
            "{}",
            numapilot::tf!(
                "  node {}  socket(s) {:?}  CPUs {}  ({} logical / {} physical)  RAM {} (free {})  distances {:?}",
                "  nó {}  socket(s) {:?}  CPUs {}  ({} lógicos / {} físicos)  RAM {} (livre {})  distâncias {:?}",
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
            GpuRole::Display => numapilot::i18n::tr("video (monitor connected)"),
            GpuRole::Compute => numapilot::i18n::tr("compute (ROCm)"),
            GpuRole::Other => numapilot::i18n::tr("another"),
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
                "  {:<10} {}  {}  [{}]\n             node {}  {}  {}  load {}  VRAM {}  ROCR {}",
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
            )
        );
    }
    let w = t.warnings();
    if !w.is_empty() {
        println!();
        for x in w {
            println!("{}", numapilot::tf!("warning: {x}", "aviso: {x}"));
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
        .context(numapilot::i18n::tr("the program is missing"))?;
    let exe = which(prog);
    let idents = procfs::idents(basename(prog), exe.as_deref(), &command);
    let key = procfs::learn_key(exe.as_deref(), &command, basename(prog));
    let steam = std::env::var("SteamAppId").ok();
    let pkey = program_key(steam.as_deref(), &key);
    // FPS optimizer: this launch uses the next configuration in the list.
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
                "numapilot: FPS optimizer, test {}/{n}: {} (play ~{} s in a similar scene)",
                "numapilot: otimizador de FPS, teste {}/{n}: {} (jogue ~{} s numa cena parecida)",
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
            numapilot::tf!("program  : {}", "programa : {}", command.join(" "))
        );
        eprintln!(
            "{}",
            numapilot::tf!("profile  : {pkey}", "perfil   : {pkey}")
        );
        eprintln!(
            "{}",
            numapilot::tf!(
                "mode     : {}",
                "modo     : {}",
                if d.manual {
                    "manual"
                } else {
                    numapilot::i18n::tr("automatic")
                }
            )
        );
        eprintln!(
            "{}",
            numapilot::tf!(
                "nodes    : {:?}  CPUs {}",
                "nós      : {:?}  CPUs {}",
                d.nodes,
                format_cpulist(&d.cpus)
            )
        );
        eprintln!(
            "{}",
            numapilot::tf!("memory   : {:?}", "memória  : {:?}", d.mem)
        );
        eprintln!(
            "{}",
            numapilot::tf!(
                "priority  : {}{}",
                "prioridade: {}{}",
                d.priority,
                if d.needs_daemon {
                    numapilot::i18n::tr(" (applied by the service)")
                } else {
                    ""
                }
            )
        );
        eprintln!(
            "{}",
            numapilot::tf!(
                "GPU use  : {}",
                "uso GPU  : {}",
                match d.kind {
                    Kind::Graphics => numapilot::i18n::tr("graphics (game/video)"),
                    Kind::Compute => numapilot::i18n::tr("compute (ROCm)"),
                    Kind::NoGpu => "-",
                }
            )
        );
        eprintln!(
            "GPU      : {}",
            if d.gpus.is_empty() {
                numapilot::i18n::tr("none").to_string()
            } else {
                d.gpus
                    .iter()
                    .map(|g| format!("{} ({})", g.alias, g.pci))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        );
        for r in &d.reasons {
            eprintln!("{}", numapilot::tf!("reason   : {r}", "motivo   : {r}"));
        }
        for w in numapilot::decide::game_warnings(&topo, &d) {
            eprintln!("{}", numapilot::tf!("warning  : {w}", "atenção  : {w}"));
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

    // What only root can do (cgroup: memory/exclusive cores; high priority): ask the service
    // to apply it to this process before exec — it then also applies to the program.
    if d.needs_daemon {
        let me = std::process::id() as i32;
        let profile = tuning.as_ref().map(|t| t.3.profile.clone());
        // the program's key goes along: at this moment the process is still numapilot, and the
        // service needs to know whose profile and control group it is
        match ctl::send(&ctl::Request::Adopt {
            pid: me,
            profile,
            key: Some(pkey.clone()),
            label: Some(basename(prog).to_string()),
        }) {
            Ok(r) if r.ok => {}
            Ok(r) => eprintln!(
                "{}",
                numapilot::tf!("numapilot: warning: {}", "numapilot: aviso: {}", r.message)
            ),
            Err(e) => {
                eprintln!(
                    "{}",
                    numapilot::tf!(
                        "numapilot: warning: memory/priority limits not applied ({e:#})",
                        "numapilot: aviso: limites de memória/prioridade não aplicados ({e:#})"
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
                "numapilot: warning: set_mempolicy failed ({e}); going on with CPU affinity only",
                "numapilot: aviso: set_mempolicy falhou ({e}); seguindo só com a afinidade de CPU"
            )
        );
    }
    // MangoHud (measuring) and gamescope wrap the command.
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
                    "numapilot: warning: gamescope not installed (sudo pacman -S gamescope); opening without it",
                    "numapilot: aviso: gamescope não instalado (sudo pacman -S gamescope); abrindo sem ele"
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
                    "numapilot: warning: OptiScaler chosen but not installed in the game's folder (Adjust… → Upscaling → Install)",
                    "numapilot: aviso: OptiScaler escolhido mas não instalado na pasta do jogo (Ajustar… → Upscaling → Instalar)"
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
            numapilot::tf!("could not run '{}'", "não consegui executar '{}'", argv[0])
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
                        "numapilot: test {}: {:.1} average FPS, 1% low {:.1}",
                        "numapilot: teste {}: {:.1} FPS médio, 1% low {:.1}",
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
                    "numapilot: MangoHud did not record this test (played less than ~{} s? MangoHud installed?). It repeats on the next start.",
                    "numapilot: o MangoHud não gravou este teste (jogou menos de ~{} s? MangoHud instalado?). Ele se repete na próxima abertura.",
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
            "could not run '{}': {err}",
            "não consegui executar '{}': {err}",
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
                        "No saved profile: everything on automatic.",
                        "Nenhum perfil salvo: tudo no automático."
                    )
                );
            }
            for (k, v) in &p.map {
                println!(
                    "{}",
                    numapilot::tf!(
                        "{k}\n    cpu={}  gpu={}  use={}",
                        "{k}\n    cpu={}  gpu={}  uso={}",
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
                    "Choices written to {}",
                    "Escolhas gravadas em {}",
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
                println!("{}", numapilot::tf!("ignored: {b}", "ignorado: {b}"));
            }
            println!(
                "{}",
                numapilot::tf!("{n} choice(s) imported.", "{n} escolha(s) importada(s).")
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
                    "{} settings to test in {label}:",
                    "{} configurações a testar em {label}:",
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
                    "Open the game {} times (through numapilot run or through Steam with numapilot) and play ~70 s each time.",
                    "Abra o jogo {} vezes (pelo numapilot run ou pela Steam com o numapilot) e jogue ~70 s em cada.",
                    t.sessions[&k].variants.len()
                )
            );
            if which("mangohud").is_none() {
                println!(
                    "{}",
                    numapilot::tf!(
                        "Warning: install MangoHud (sudo pacman -S mangohud), it does the measuring.",
                        "Atenção: instale o MangoHud (sudo pacman -S mangohud), ele faz a medição."
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
                        "{} ({}) — {}/{} tested",
                        "{} ({}) — {}/{} testados",
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
                    "Optimization of {k} cancelled.",
                    "Otimização de {k} cancelada."
                )
            );
            Ok(())
        }
        ("apply", Some(k)) => {
            let s = t.sessions.get(&k).context(numapilot::i18n::tr(
                "there is no optimization for this program",
            ))?;
            let b = s
                .best()
                .context(numapilot::i18n::tr("no test has finished yet"))?;
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
                    "Using \"{}\": {}",
                    "Usando \"{}\": {}",
                    s.variants[b].label,
                    r.message
                )
            );
            Ok(())
        }
        _ => bail!(
            "{}",
            numapilot::tf!(
                "usage: numapilot tune start|stop|apply KEY  or  numapilot tune status [KEY]",
                "use: numapilot tune start|stop|apply CHAVE  ou  numapilot tune status [CHAVE]"
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
            println!("{}", numapilot::tf!("Mode: {}", "Modo: {}", m.mode));
            println!(
                "Turbo: {}",
                cpuctl::turbo()
                    .map(|t| if t {
                        numapilot::i18n::tr("on")
                    } else {
                        numapilot::i18n::tr("off")
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
                        "Socket {}: governor {} · {} · {}-{} MHz (now {}) · uncore {} · {} · limit {} · {}",
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
                            .unwrap_or(numapilot::i18n::tr("unavailable").into()),
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
                            .map(|w| numapilot::tf!("power draw {w:.0} W", "consumo {w:.0} W"))
                            .unwrap_or_default(),
                    )
                );
            }
            println!(
                "{}",
                numapilot::tf!(
                    "Idle states (cpu0): {}",
                    "Estados de economia (cpu0): {}",
                    cpuctl::cstates(0)
                        .iter()
                        .map(|s| format!(
                            "{} {}µs{}",
                            s.name,
                            s.latency_us,
                            if s.disabled {
                                numapilot::i18n::tr(" (off)")
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
                        "Low latency now on cores {l}",
                        "Baixa latência agora nos núcleos {l}"
                    )
                );
            }
            println!("THP: {}", numapilot::machine::thp().unwrap_or("?".into()));
            if let Some((cur, list)) = numapilot::machine::scx_status() {
                println!(
                    "{}",
                    numapilot::tf!(
                        "Scheduler: {} (available: {})",
                        "Escalonador: {} (disponíveis: {})",
                        if cur.is_empty() {
                            numapilot::i18n::tr("kernel default").into()
                        } else {
                            cur
                        },
                        list.join(", ")
                    )
                );
            }
            Ok(())
        }
        (
            "mode",
            Some(name @ ("jogo" | "ia" | "silencioso" | "normal" | "game" | "ai" | "quiet")),
        ) => {
            // English names on the command line; the stored value does not change
            let name = match name {
                "game" => "jogo",
                "ai" => "ia",
                "quiet" => "silencioso",
                o => o,
            };
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
                "usage: numapilot machine status  |  numapilot machine mode game|ai|quiet|normal",
                "use: numapilot machine status  |  numapilot machine mode game|ai|quiet|normal"
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
                        "Bootloader: {} ({})\nConfigured: {p}",
                        "Bootloader: {} ({})\nConfigurado: {p}",
                        l.name(),
                        l.path.display()
                    )
                ),
                None => println!(
                    "{}",
                    numapilot::tf!("Bootloader not recognized", "Bootloader não reconhecido")
                ),
            }
            println!(
                "{}",
                numapilot::tf!("In use now: {}", "Em uso agora: {}", cmdline::running())
            );
            println!(
                "{}",
                numapilot::tf!("\nKnown parameters:", "\nParâmetros conhecidos:")
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
                    "\nMitigations that apply to this processor:",
                    "\nMitigações que se aplicam a este processador:"
                )
            );
            for m in cmdline::relevant_mitigations(&v) {
                println!(
                    "{}",
                    numapilot::tf!(
                        "  {:<44} risk {} · gain {} · {}",
                        "  {:<44} risco {} · ganho {} · {}",
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
                    "  (all at once: mitigations=off)",
                    "  (tudo de uma vez: mitigations=off)"
                )
            );
            Ok(())
        }
        ("cmdline", Some("apply")) => {
            // +param adds, -param removes (several at once, a single backup and one rewrite)
            let mut add = vec![];
            let mut rm = vec![];
            for a in &args[1..] {
                match a.split_at(1) {
                    ("+", p) => add.push(p.to_string()),
                    ("-", p) => rm.push(p.to_string()),
                    _ => bail!(
                        "{}",
                        numapilot::tf!(
                            "use +PARAM to add and -PARAM to remove (got {a})",
                            "use +PARAM para adicionar e -PARAM para tirar (recebido {a})"
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
                    numapilot::tf!("the parameter is missing", "faltou o parâmetro")
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
                    "OverDrive (clock/voltage of the AMD cards): {} now, {} after a reboot{}",
                    "OverDrive (clock/voltagem das placas AMD): {} agora, {} depois de reiniciar{}",
                    if s.active {
                        numapilot::i18n::tr("on")
                    } else {
                        numapilot::i18n::tr("off")
                    },
                    if s.on_next_boot {
                        numapilot::i18n::tr("on")
                    } else {
                        numapilot::i18n::tr("off")
                    },
                    if s.from_cmdline {
                        numapilot::i18n::tr(" (set on the kernel command line)")
                    } else {
                        ""
                    }
                )
            );
            let gib = |p: u64| p as f64 * 4096.0 / 1073741824.0;
            println!(
                "{}",
                numapilot::tf!(
                    "Integrated GPU memory (ttm): {} now, {} at boot",
                    "Memória da GPU integrada (ttm): {} agora, {} no boot",
                    bootcfg::ttm_now()
                        .map(|p| format!("{:.1} GiB", gib(p)))
                        .unwrap_or("?".into()),
                    bootcfg::saved_ttm()
                        .map(|p| format!("{:.1} GiB", gib(p)))
                        .unwrap_or(numapilot::i18n::tr("default (half the RAM)").into())
                )
            );
            println!(
                "{}",
                numapilot::tf!("File: {}", "Arquivo: {}", bootcfg::conf_path().display())
            );
            match bootcfg::initramfs_tool() {
                Some(t) => println!(
                    "{}",
                    numapilot::tf!(
                        "Boot image rebuilt with: {}",
                        "Imagem de boot refeita com: {}",
                        t.join(" ")
                    )
                ),
                None => println!(
                    "{}",
                    numapilot::tf!(
                        "No initramfs tool found",
                        "Nenhuma ferramenta de initramfs encontrada"
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
                            "use a number in GiB (e.g. 14) or default",
                        ))?,
                )
            };
            bootcfg::set_gpu_memory(gib, &mut |m| println!("{m}")).map_err(|e| anyhow::anyhow!(e))
        }
        _ => bail!(
            "{}",
            numapilot::tf!(
                "usage: numapilot boot status | sudo numapilot boot overdrive on|off | sudo numapilot boot gpu-memory GiB|default | numapilot boot cmdline [add|remove PARAM...]",
                "use: numapilot boot status | sudo numapilot boot overdrive on|off | sudo numapilot boot gpu-memory GiB|default | numapilot boot cmdline [add|remove PARAM...]"
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
                "could not find the Steam configuration (localconfig.vdf)",
                "não achei a configuração da Steam (localconfig.vdf)"
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
                    "{n} game(s) changed in {} (backup next to it).",
                    "{n} jogo(s) alterado(s) em {} (backup ao lado).",
                    path.display()
                )
            );
            Ok(())
        }
        _ => bail!(
            "{}",
            numapilot::tf!(
                "usage: numapilot steam enable | disable | list",
                "use: numapilot steam enable | disable | list"
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
        "the daemon does not seem to be running (systemctl status numapilot)",
    ))?;
    let rows: Vec<daemon::Row> = serde_json::from_str(&s)?;
    println!(
        "{}",
        numapilot::tf!(
            "{:>7}  {:<16} {:>6}  {:<12} {:<6} {:<10} {:<14} source",
            "{:>7}  {:<16} {:>6}  {:<12} {:<6} {:<10} {:<14} origem",
            "PID",
            numapilot::i18n::tr("PROGRAM"),
            numapilot::i18n::tr("CORES"),
            numapilot::i18n::tr("STATE"),
            numapilot::i18n::tr("NODES"),
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
                "in `pin` use a node number, a list (0,1) or all",
                "em `pin` use um número de nó, uma lista (0,1) ou all"
            )
        ),
    };
    for n in &nodes {
        if topo.node(*n).is_none() {
            bail!(
                "{}",
                numapilot::tf!("node {n} does not exist", "nó {n} não existe")
            );
        }
    }
    let st = procfs::read_stat(pid).context(numapilot::i18n::tr("process not found"))?;
    let cpus = topo.cpus_of(&nodes);
    let ok = procfs::set_affinity_all(pid, &cpus);
    if ok == 0 {
        bail!(
            "{}",
            numapilot::tf!(
                "no permission (use sudo for other users' processes)",
                "sem permissão (use sudo para processos de outros usuários)"
            )
        );
    }
    println!(
        "{}",
        numapilot::tf!(
            "{} (pid {pid}): {ok} threads → node(s) {:?} [{}]",
            "{} (pid {pid}): {ok} threads → nó(s) {:?} [{}]",
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
                    "memory migrated to node(s) {:?}",
                    "memória migrada para o(s) nó(s) {:?}",
                    nodes
                )
            ),
            Err(e) => println!(
                "{}",
                numapilot::tf!(
                    "warning: memory migration failed: {e}",
                    "aviso: migração de memória falhou: {e}"
                )
            ),
        }
    }
    // Tells the daemon to leave this process alone from now on.
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
                "warning: could not register the pin with the daemon (run with sudo); it may reposition the process.",
                "aviso: não consegui registrar o pin para o daemon (rode com sudo); ele pode reposicionar o processo."
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
        l.save()
            .context(numapilot::i18n::tr("could not save (needs sudo?)"))?;
        println!(
            "{}",
            numapilot::tf!(
                "{} entry(ies) removed. Restart the daemon for it to reload.",
                "{} entrada(s) removida(s). Reinicie o daemon para ele recarregar.",
                before - l.map.len()
            )
        );
        return Ok(());
    }
    if l.map.is_empty() {
        println!(
            "{}",
            numapilot::tf!(
                "Nothing learned yet ({}).",
                "Nada aprendido ainda ({}).",
                Learned::path().display()
            )
        );
    }
    for (k, e) in &l.map {
        println!(
            "{}",
            numapilot::tf!(
                "{}  {}  peak {:.0}% of a node  gpu-node {}  runs {}",
                "{}  {}  pico {:.0}% de um nó  gpu-nó {}  execuções {}",
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
        "servico" | "serviço" | "service" | "daemon" => "serviço",
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
        // the journal has what came before the file log
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
                        "== service journal (this boot) ==",
                        "== journal do serviço (este boot) =="
                    )
                );
                print!("{t}");
            }
        }
    }
    if shown == 0 {
        println!(
            "{}",
            numapilot::tf!("No logs yet.", "Nenhum registro ainda.")
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
                        "{} ({}): asleep, skipped so as not to wake it (test only it: numapilot gpu-test {})",
                        "{} ({}): dormindo, pulei para não acordar (teste só ela: numapilot gpu-test {})",
                        g.alias,
                        g.short_name(),
                        g.alias
                    )
                );
            }
            continue;
        }
        let (mut set, unset) = numapilot::decide::env_for_gpu_full(g, &topo);
        // the same DirectX fallback `run` uses (no effect on vulkaninfo, but it shows up)
        let dxf = numapilot::gputest::vk_name_for(g, &set, &unset);
        let lines = numapilot::gputest::test(g, &topo, &set, &unset);
        if !json {
            println!("{} ({}, {})", g.alias, g.short_name(), g.pci);
            for l in &lines {
                let tag = match l.verdict {
                    Verdict::Certa => numapilot::i18n::tr("right  "),
                    Verdict::Errada => numapilot::i18n::tr("WRONG  "),
                    Verdict::NaoDeu => numapilot::i18n::tr("failed"),
                };
                println!("  {tag} {}: {}", l.api, l.detail);
            }
            if let Some(n) = &dxf {
                println!(
                    "{}",
                    numapilot::tf!(
                        "  name for the DXVK/VKD3D filter: {n}",
                        "  nome para o filtro do DXVK/VKD3D: {n}"
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
            serde_json::json!({"alias": g.alias, "pci": g.pci, numapilot::i18n::tr("lines"): lines, "filtro_dx": dxf}),
        );
    }
    if !found {
        anyhow::bail!(
            "{}",
            numapilot::tf!(
                "card not found (see the aliases in: numapilot topo)",
                "placa não encontrada (veja os apelidos em: numapilot topo)"
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
                "Some choice landed on the wrong card. Send this output and that of: numapilot run --dry-run --gpu ALIAS -- true",
                "Alguma escolha caiu na placa errada. Mande esta saída e a de: numapilot run --dry-run --gpu APELIDO -- true"
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
    // root reads the MSRs directly; a regular user uses what the service published
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
                "Processor {} ({} cores)",
                "Processador {} ({} núcleos)",
                n.id,
                total
            )
        );
        match n.cpus.first().and_then(|c| turbo::deep_enabled(*c)) {
            Some(true) => println!(
                "{}",
                numapilot::tf!(
                    "  awake now: ~{awake:.1} (1 s average)",
                    "  acordados agora: ~{awake:.1} (média de 1 s)"
                )
            ),
            Some(false) => println!(
                "{}",
                numapilot::tf!(
                    "  deep states (C3/C6) OFF: every core counts as awake; turn them on in Machine → Processor",
                    "  estados profundos (C3/C6) DESLIGADOS: todos os núcleos contam como acordados; ligue em Máquina → Processador"
                )
            ),
            None => {
                println!(
                    "{}",
                    numapilot::tf!(
                        "  no cpuidle on this system: the awake cores cannot be estimated",
                        "  sem cpuidle neste sistema: não dá para estimar os núcleos acordados"
                    )
                )
            }
        }
        match tables.get(&(n.id as u32)) {
            Some(t) => {
                println!(
                    "{}",
                    numapilot::tf!(
                        "  table (cores awake → maximum clock): {}",
                        "  tabela (núcleos acordados → clock máximo): {}",
                        turbo::describe(t)
                    )
                );
                if let Some(mhz) = turbo::limit_for(t, awake) {
                    println!(
                        "{}",
                        numapilot::tf!(
                            "  with the current load: up to {:.1} GHz",
                            "  com a carga de agora: até {:.1} GHz",
                            mhz as f64 / 1000.0
                        )
                    );
                }
            }
            None => println!(
                "{}",
                numapilot::tf!(
                    "  turbo table unavailable (Intel only; needs the service running or sudo)",
                    "  tabela de turbo indisponível (só Intel; precisa do serviço rodando ou de sudo)"
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
                    "Measurement deleted: core choice goes back to the system order.",
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
                    "no saved measurement (run: numapilot bench-cores)",
                    "nenhuma medição guardada (rode: numapilot bench-cores)"
                )
            ),
        }
    } else {
        println!(
            "{}",
            numapilot::tf!(
                "Leave the PC idle during the measurement.",
                "Deixe o PC parado durante a medição."
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
                "Processor {} (fastest to slowest):",
                "Processador {} (do mais rápido ao mais lento):",
                n.id
            )
        );
        for (i, cpu, ns) in &v {
            println!(
                "{}",
                numapilot::tf!(
                    "  core {i:>2} (CPU {cpu:>3}): {ns:.2} ns  {:+.1}%",
                    "  núcleo {i:>2} (CPU {cpu:>3}): {ns:.2} ns  {:+.1}%",
                    (ns / best - 1.0) * 100.0
                )
            );
        }
    }
    println!();
    println!(
        "{}",
        numapilot::tf!(
            "Difference between the best and the worst core: {:.1}% · measurement noise: {:.1}%",
            "Diferença entre o melhor e o pior núcleo: {:.1}% · ruído da medição: {:.1}%",
            rank.spread_pct,
            rank.noise_pct
        )
    );
    if rank.useful {
        println!(
            "{}",
            numapilot::tf!(
                "The difference is larger than the noise: core choice now prefers the fastest ones.",
                "A diferença é maior que o ruído: a escolha de núcleos passa a preferir os mais rápidos."
            )
        );
    } else {
        println!(
            "{}",
            numapilot::tf!(
                "The difference is too small (or the measurement varied too much) to count as a criterion: \
             core choice stays in the system order. If the PC was busy, close everything and measure again.",
                "A diferença é pequena demais (ou a medição variou muito) para valer como critério: \
             a escolha de núcleos continua na ordem do sistema. Se o PC estava ocupado, feche tudo e meça de novo."
            )
        );
    }
    if !show {
        let r = ctl::send(&ctl::Request::SetCoreRank { rank })?;
        if !r.ok {
            bail!(
                "{}",
                numapilot::tf!(
                    "measured, but the service did not save it: {}",
                    "medi, mas o serviço não guardou: {}",
                    r.message
                )
            );
        }
    }
    Ok(())
}
