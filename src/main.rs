use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use numapilot::config::{Config, CpuSpec, GpuSpec, MemSpec};
use numapilot::decide::{decide_launch, gpu_env, sample_node_loads, Kind, LaunchInput};
use numapilot::learned::{self, Learned};
use numapilot::profiles::{program_key, Profile, Profiles};
use numapilot::topo::{GpuRole, Topology};
use numapilot::util::{basename, format_cpulist};
use numapilot::{ctl, daemon, procfs};
use std::os::unix::process::CommandExt;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "numapilot", version, about = "Escolhe socket (NUMA), memória e GPU para cada programa")]
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
    /// Perfis por programa (os mesmos da interface gráfica).
    Profile {
        #[command(subcommand)]
        action: ProfileCmd,
    },
    /// Imprime os IDs ROCR_VISIBLE_DEVICES das GPUs de computação (para scripts).
    #[command(name = "rocr-ids")]
    RocrIds,
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
    if let Err(e) = real_main() {
        eprintln!("numapilot: erro: {e:#}");
        std::process::exit(1);
    }
}

fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = Config::load()?;
    match cli.cmd {
        Cmd::Topo => cmd_topo(&cfg),
        Cmd::Run { cpu, gpu, mem, usage, fg_gpu, compute, graphics, dry_run, command } => {
            let usage = match (compute, graphics, usage) {
                (true, _, _) => Some(Kind::Compute),
                (_, true, _) => Some(Kind::Graphics),
                (_, _, Some(u)) => Kind::parse_usage(&u)?,
                _ => None,
            };
            cmd_run(&cfg, cpu, gpu, mem, usage, fg_gpu, dry_run, command)
        }
        Cmd::Profile { action } => cmd_profile(action),
        Cmd::Daemon => daemon::run(cfg),
        Cmd::Status => cmd_status(),
        Cmd::Pin { pid, cpu, no_migrate } => cmd_pin(&cfg, pid, &cpu, no_migrate),
        Cmd::Learned { forget } => cmd_learned(forget),
        Cmd::RocrIds => {
            let t = Topology::load(&cfg);
            let ids: Vec<String> = t.compute_gpus().iter().filter_map(|g| g.rocr_id.clone()).collect();
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
            .map(|(u, tt)| format!("{:.1}/{:.1} GiB", u as f64 / 1073741824.0, tt as f64 / 1073741824.0))
            .unwrap_or_else(|| "-".into());
        println!(
            "  {:<10} {}  {}  [{}]\n             nó {}  {}  {}  ocupação {}  VRAM {}  ROCR {}",
            g.alias,
            g.pci,
            g.name,
            role,
            g.numa_node.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
            g.gfx.as_deref().unwrap_or("-"),
            g.render.as_deref().unwrap_or("-"),
            g.busy_percent().map(|b| format!("{b}%")).unwrap_or_else(|| "-".into()),
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
    let profiles = Profiles::load();
    let prog = command.first().context("faltou o programa")?;
    let exe = which(prog);
    let idents = procfs::idents(basename(prog), exe.as_deref(), &command);
    let key = procfs::learn_key(exe.as_deref(), &command, basename(prog));
    let steam = std::env::var("SteamAppId").ok();
    let pkey = program_key(steam.as_deref(), &key);
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
        eprintln!("modo     : {}", if d.manual { "manual" } else { "automático" });
        eprintln!("nós      : {:?}  CPUs {}", d.nodes, format_cpulist(&d.cpus));
        eprintln!("memória  : {:?}", d.mem);
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
                d.gpus.iter().map(|g| format!("{} ({})", g.alias, g.pci)).collect::<Vec<_>>().join(", ")
            }
        );
        for r in &d.reasons {
            eprintln!("motivo   : {r}");
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

    procfs::set_affinity(0, &d.cpus).context("sched_setaffinity")?;
    let wide = d.nodes.len() > 1;
    let pol = match d.mem {
        MemSpec::Local if wide => (procfs::MPOL_DEFAULT, vec![]),
        MemSpec::Local => (procfs::MPOL_PREFERRED, d.nodes.clone()),
        MemSpec::Interleave => (procfs::MPOL_INTERLEAVE, d.nodes.clone()),
        MemSpec::Bind => (procfs::MPOL_BIND, d.nodes.clone()),
    };
    if let Err(e) = procfs::set_mempolicy(pol.0, &pol.1) {
        eprintln!("numapilot: aviso: set_mempolicy falhou ({e}); seguindo só com a afinidade de CPU");
    }
    let mut c = std::process::Command::new(prog);
    c.args(&command[1..]);
    for (k, v) in set {
        c.env(k, v);
    }
    for k in unset {
        c.env_remove(k);
    }
    let err = c.exec();
    bail!("não consegui executar '{prog}': {err}")
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
        ProfileCmd::Set { key, cpu, gpu, usage } => {
            let profile = Profile { label: basename(&key).to_string(), cpu, gpu, usage, ..Default::default() };
            let r = ctl::send(&ctl::Request::SetProfile { key, profile, pid: None })?;
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

fn which(prog: &str) -> Option<String> {
    if prog.contains('/') {
        return std::fs::canonicalize(prog).ok().map(|p| p.to_string_lossy().to_string());
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
    println!("{:>7}  {:<16} {:>6}  {:<12} {:<6} {:<10} {:<14} origem", "PID", "PROGRAMA", "NÚCL.", "ESTADO", "NÓS", "GPU", "CPUs");
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
    println!("{} (pid {pid}): {ok} threads → nó(s) {:?} [{}]", st.comm, nodes, format_cpulist(&cpus));
    if !no_migrate && nodes.len() < topo.nodes.len() {
        let from: Vec<usize> = topo.node_ids().into_iter().filter(|x| !nodes.contains(x)).collect();
        match procfs::migrate_pages(pid, &from, &nodes) {
            Ok(_) => println!("memória migrada para o(s) nó(s) {:?}", nodes),
            Err(e) => println!("aviso: migração de memória falhou: {e}"),
        }
    }
    // Avisa o daemon para não mexer mais neste processo.
    let mut pins = daemon::read_pins();
    pins.retain(|p| procfs::read_stat(p.pid).is_some_and(|s| s.starttime == p.starttime));
    pins.insert(daemon::Pin { pid, starttime: st.starttime });
    let v: Vec<_> = pins.into_iter().collect();
    let _ = std::fs::create_dir_all(learned::run_dir());
    if std::fs::write(daemon::pins_path(), serde_json::to_string(&v)?).is_err() {
        println!("aviso: não consegui registrar o pin para o daemon (rode com sudo); ele pode reposicionar o processo.");
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
        println!("{} entrada(s) removida(s). Reinicie o daemon para ele recarregar.", before - l.map.len());
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
            e.gpu_node.map(|n| n.to_string()).unwrap_or_else(|| "-".into()),
            e.runs
        );
    }
    Ok(())
}
