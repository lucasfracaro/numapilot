//! De onde veio um programa (pacote do repositório ou do AUR, Flatpak, npm, pipx, cargo, programa
//! da comunidade baixado pelo numapilot, AppImage ou arquivo solto) e como desinstalar do jeito
//! certo para cada origem. Usado pela aba Inicialização: "Detalhes" e "Desinstalar…".

use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Pacote do pacman (`aur` = não veio de um repositório oficial).
    Pacman {
        pkg: String,
        aur: bool,
        version: String,
    },
    Flatpak {
        id: String,
        user: bool,
    },
    Npm {
        pkg: String,
    },
    Pipx {
        pkg: String,
    },
    Cargo {
        krate: String,
    },
    /// Programa da comunidade baixado pelo numapilot (git).
    Numapilot {
        dir: PathBuf,
    },
    /// Arquivos soltos (AppImage, binário, script). `root` = precisa de sudo para apagar.
    Manual {
        paths: Vec<PathBuf>,
        root: bool,
    },
    Unknown,
}

impl Source {
    pub fn text(&self) -> String {
        match self {
            Source::Pacman {
                pkg,
                aur: false,
                version,
            } => format!("pacote {pkg} {version} (repositório oficial)"),
            Source::Pacman {
                pkg,
                aur: true,
                version,
            } => format!("pacote {pkg} {version} (AUR ou instalado à mão)"),
            Source::Flatpak { id, user } => {
                format!("Flatpak {id}{}", if *user { " (do usuário)" } else { "" })
            }
            Source::Npm { pkg } => format!("pacote global do npm: {pkg}"),
            Source::Pipx { pkg } => format!("pipx: {pkg}"),
            Source::Cargo { krate } => format!("cargo install: {krate}"),
            Source::Numapilot { dir } => format!(
                "programa da comunidade baixado pelo numapilot ({})",
                dir.display()
            ),
            Source::Manual { paths, .. } => match paths.first() {
                Some(p) if p.to_string_lossy().ends_with(".AppImage") => {
                    format!("AppImage: {}", p.display())
                }
                Some(p) => format!("arquivo instalado à mão: {}", p.display()),
                None => "arquivo instalado à mão".into(),
            },
            Source::Unknown => "não descobri de onde veio".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    /// Arquivo que faz abrir (.desktop ou unidade do systemd) e o pacote dono dele.
    pub file: Option<PathBuf>,
    pub file_pkg: Option<String>,
    /// Programa de verdade (o script, se for `node script.js`, `python x.py`...).
    pub exe: Option<PathBuf>,
    pub source: Source,
    /// Descrição do pacote, se houver.
    pub description: Option<String>,
    /// Tamanho em disco (bytes), quando dá para saber.
    pub size: Option<u64>,
    /// Pastas de configuração e dados que existem para ele.
    pub configs: Vec<PathBuf>,
    /// Pasta própria do programa instalado à mão (ex.: ~/homebrew do Decky).
    pub app_dir: Option<PathBuf>,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

/// Divide uma linha de comando respeitando aspas.
pub fn split_command(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut q: Option<char> = None;
    let mut has = false;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match (q, c) {
            (Some(x), c) if c == x => q = None,
            (Some('"'), '\\') => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                q = Some(c);
                has = true;
            }
            (None, c) if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (None, '\\') => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        out.push(cur);
    }
    out
}

const INTERPRETERS: &[&str] = &[
    "node", "nodejs", "python", "python3", "bash", "sh", "zsh", "perl", "ruby", "java", "bun",
    "deno", "electron", "wine",
];

/// O que a linha de comando abre de verdade: ("flatpak", id) ou ("", programa/script).
pub fn program_of(exec: &str) -> Option<(String, String)> {
    let toks = split_command(exec);
    let mut i = 0;
    while i < toks.len() {
        let t = &toks[i];
        let base = crate::util::basename(t);
        if base == "env" || (t.contains('=') && !t.starts_with('/') && !t.starts_with('-')) {
            i += 1;
            continue;
        }
        // sh -c "sleep 10; exec programa": olha dentro
        if matches!(base, "sh" | "bash") && toks.get(i + 1).is_some_and(|a| a == "-c") {
            let inner = toks.get(i + 2)?;
            let last = inner
                .split([';', '&'])
                .map(str::trim)
                .rfind(|p| !p.is_empty() && !p.starts_with("sleep"))?;
            return program_of(last.trim_start_matches("exec ").trim());
        }
        if base == "flatpak" {
            let id = toks[i + 1..]
                .iter()
                .skip_while(|a| *a == "run" || a.starts_with('-'))
                .find(|a| a.contains('.') && !a.starts_with('-'))?;
            return Some((
                "flatpak".into(),
                id.trim_start_matches("--command=").to_string(),
            ));
        }
        if INTERPRETERS.contains(&base) {
            // o script é o primeiro argumento que parece caminho
            if let Some(s) = toks[i + 1..]
                .iter()
                .find(|a| a.contains('/') && !a.starts_with('-'))
            {
                return Some((String::new(), s.clone()));
            }
        }
        return Some((String::new(), t.clone()));
    }
    None
}

fn which(prog: &str) -> Option<PathBuf> {
    if prog.contains('/') {
        let p = PathBuf::from(prog);
        return p.exists().then_some(p);
    }
    let h = home();
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .chain([
            h.join(".local/bin"),
            h.join(".cargo/bin"),
            h.join(".npm-global/bin"),
        ])
        .map(|d| d.join(prog))
        .find(|p| p.is_file())
}

fn out(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd).args(args).output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// Dono de um arquivo no pacman.
fn pacman_owner(p: &Path) -> Option<String> {
    out("pacman", &["-Qqo", &p.to_string_lossy()]).filter(|s| !s.is_empty() && !s.contains('\n'))
}

/// Origem pelo caminho do programa (sem chamar o pacman: os testes usam isto).
pub fn source_from_path(p: &Path, home: &Path) -> Option<Source> {
    let s = p.to_string_lossy();
    if let Some(rest) = s.split("/node_modules/").nth(1) {
        let mut parts = rest.split('/');
        let first = parts.next()?;
        let pkg = if first.starts_with('@') {
            format!("{first}/{}", parts.next()?)
        } else {
            first.to_string()
        };
        return Some(Source::Npm { pkg });
    }
    if let Some(rest) = s.split("/pipx/venvs/").nth(1) {
        return Some(Source::Pipx {
            pkg: rest.split('/').next()?.to_string(),
        });
    }
    for (base, user) in [
        (home.join(".local/share/flatpak/app"), true),
        (PathBuf::from("/var/lib/flatpak/app"), false),
    ] {
        if let Ok(r) = p.strip_prefix(&base) {
            let id = r.iter().next()?.to_string_lossy().to_string();
            return Some(Source::Flatpak { id, user });
        }
    }
    let com = home.join(".local/share/numapilot/comunidade");
    if let Ok(r) = p.strip_prefix(&com) {
        return Some(Source::Numapilot {
            dir: com.join(r.iter().next()?),
        });
    }
    None
}

/// Crate do `cargo install` que instalou este binário.
fn cargo_crate(bin: &str) -> Option<String> {
    let list = out("cargo", &["install", "--list"])?;
    let mut cur = None;
    for l in list.lines() {
        if !l.starts_with(' ') {
            cur = l.split_whitespace().next().map(String::from);
        } else if l.trim() == bin {
            return cur;
        }
    }
    None
}

fn dir_size(p: &Path, budget: &mut u32) -> u64 {
    let Ok(m) = std::fs::symlink_metadata(p) else {
        return 0;
    };
    if m.is_file() {
        return m.len();
    }
    if !m.is_dir() || *budget == 0 {
        return 0;
    }
    std::fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            *budget = budget.saturating_sub(1);
            dir_size(&e.path(), budget)
        })
        .sum()
}

/// Pastas de configuração e dados que existem para estes nomes.
pub fn config_dirs(names: &[String], home: &Path) -> Vec<PathBuf> {
    let mut v = vec![];
    for n in names.iter().filter(|n| n.len() >= 3) {
        for base in [".config", ".local/share", ".local/state", ".cache"] {
            v.push(home.join(base).join(n));
        }
        v.push(home.join(format!(".{n}")));
        if n.contains('.') {
            v.push(home.join(".var/app").join(n));
        }
    }
    v.retain(|p| p.is_dir());
    v.sort();
    v.dedup();
    v
}

/// Pasta própria de um programa instalado à mão dentro da pasta pessoal: a primeira pasta
/// abaixo do home (ex.: ~/homebrew), ou a pasta do programa quando a primeira é genérica
/// (~/Apps/Programa). Nada para ~/.local/bin, ~/bin e afins.
pub fn app_dir_of(exe: &Path, home: &Path) -> Option<PathBuf> {
    let rel = exe.strip_prefix(home).ok()?;
    let parts: Vec<_> = rel.iter().collect();
    if parts.len() < 2 {
        return None;
    }
    let first = parts[0].to_string_lossy().to_lowercase();
    // Pastas de ferramentas e de trabalho que guardam muito mais que um programa: apagar
    // ~/go, ~/.cargo, ~/.conda ou ~/Projetos/algo por causa de um binário seria um desastre.
    const NEVER: &[&str] = &[
        ".cargo",
        ".rustup",
        "go",
        ".go",
        ".conda",
        ".mamba",
        "miniconda3",
        "anaconda3",
        "miniforge3",
        ".pyenv",
        ".rbenv",
        ".nvm",
        ".bun",
        ".deno",
        ".dotnet",
        ".npm",
        ".npm-global",
        ".yarn",
        ".pnpm",
        ".pub-cache",
        "flutter",
        ".gradle",
        ".m2",
        ".sdkman",
        ".steam",
        ".var",
        ".wine",
        ".local",
        "snap",
        ".nix-profile",
        ".linuxbrew",
        "android",
        ".android",
        "java",
        "lib",
        ".config",
        ".cache",
        "projetos",
        "projects",
        "documentos",
        "documents",
        "downloads",
        "desktop",
        "área de trabalho",
        "código",
        "code",
        "src",
        "git",
        "repos",
        "dev",
        "workspace",
    ];
    if NEVER.contains(&first.as_str()) {
        return None;
    }
    const GENERIC: &[&str] = &[
        ".local",
        "bin",
        ".bin",
        "apps",
        "applications",
        "aplicativos",
        "programas",
        "downloads",
        "documentos",
        "documents",
        "projetos",
        "projects",
        "opt",
        "desktop",
        "área de trabalho",
    ];
    if first == ".local" || first == "bin" || first == ".bin" {
        return None;
    }
    if GENERIC.contains(&first.as_str()) {
        // ~/Apps/Programa/bin/x → ~/Apps/Programa; ~/Apps/x.AppImage → nada (o arquivo basta)
        return (parts.len() >= 3).then(|| home.join(parts[0]).join(parts[1]));
    }
    Some(home.join(parts[0]))
}

/// Rastreia o programa de uma linha de comando (e o arquivo que faz ele abrir).
pub fn trace(exec: &str, file: Option<&Path>) -> Trace {
    let h = home();
    let file_pkg = file.and_then(pacman_owner);
    let mut t = Trace {
        file: file.map(Path::to_path_buf),
        file_pkg,
        exe: None,
        source: Source::Unknown,
        description: None,
        size: None,
        configs: vec![],
        app_dir: None,
    };
    let mut names: Vec<String> = vec![];
    match program_of(exec) {
        Some((kind, id)) if kind == "flatpak" => {
            let user = h.join(".local/share/flatpak/app").join(&id).exists();
            t.description = out("flatpak", &["info", &id])
                .and_then(|i| i.lines().next().map(|l| l.trim().to_string()));
            names.push(id.clone());
            t.source = Source::Flatpak { id, user };
        }
        Some((_, prog)) => {
            let Some(link) = which(&prog) else {
                return t;
            };
            let real = std::fs::canonicalize(&link).unwrap_or_else(|_| link.clone());
            t.exe = Some(link.clone());
            let base = crate::util::basename(&link.to_string_lossy()).to_string();
            names.push(base.trim_end_matches(".AppImage").to_lowercase());
            t.source = if let Some(s) = source_from_path(&real, &h) {
                s
            } else if let Some(pkg) = pacman_owner(&real) {
                let version = out("pacman", &["-Q", &pkg])
                    .and_then(|v| v.split_whitespace().nth(1).map(String::from))
                    .unwrap_or_default();
                let aur = out("pacman", &["-Qqm", &pkg]).is_some();
                if let Some(info) = out("pacman", &["-Qi", &pkg]) {
                    let field = |k: &str| {
                        info.lines()
                            .find(|l| l.starts_with(k))
                            .and_then(|l| l.split_once(':'))
                            .map(|(_, v)| v.trim().to_string())
                    };
                    t.description = field("Descri").or_else(|| field("Description"));
                }
                names.push(pkg.clone());
                Source::Pacman { pkg, aur, version }
            } else if real.starts_with(h.join(".cargo/bin")) {
                match cargo_crate(&base) {
                    Some(krate) => Source::Cargo { krate },
                    None => Source::Manual {
                        paths: vec![real.clone()],
                        root: false,
                    },
                }
            } else {
                let mut paths = vec![];
                if link != real {
                    paths.push(link.clone());
                }
                paths.push(real.clone());
                let root = paths.iter().any(|p| {
                    p.parent().is_none_or(|d| {
                        std::fs::metadata(d)
                            .map(|m| {
                                std::os::unix::fs::MetadataExt::uid(&m) != unsafe { libc::getuid() }
                            })
                            .unwrap_or(true)
                    })
                });
                Source::Manual { paths, root }
            };
            match &t.source {
                Source::Npm { pkg } => {
                    names.push(pkg.trim_start_matches('@').replace('/', "-"));
                    if let Some(i) = real.to_string_lossy().find(&format!("/node_modules/{pkg}")) {
                        let dir = PathBuf::from(
                            &real.to_string_lossy()[..i + "/node_modules/".len() + pkg.len()],
                        );
                        t.size = Some(dir_size(&dir, &mut 20_000));
                    }
                }
                Source::Numapilot { dir } => t.size = Some(dir_size(dir, &mut 20_000)),
                Source::Manual { paths, .. } => {
                    t.size = Some(paths.iter().map(|p| dir_size(p, &mut 1)).sum());
                    t.app_dir = app_dir_of(&real, &h);
                }
                _ => {}
            }
        }
        None => {}
    }
    t.configs = config_dirs(&names, &h);
    t
}

/// Pacotes que nunca devem ser removidos por aqui (sistema, sessão, rede, numapilot).
const ESSENTIAL_PKGS: &[&str] = &[
    "systemd",
    "glibc",
    "linux",
    "pipewire",
    "wireplumber",
    "plasma-",
    "kwin",
    "sddm",
    "plasma-login",
    "networkmanager",
    "dbus",
    "polkit",
    "mesa",
    "xdg-user-dirs",
    "xdg-desktop-portal",
    "numapilot",
    "pacman",
    "sudo",
    "bash",
    "coreutils",
    "kde",
    "baloo",
    "powerdevil",
    "upower",
    "udisks",
];

pub fn essential_pkg(pkg: &str) -> bool {
    ESSENTIAL_PKGS.iter().any(|e| {
        pkg == *e || (e.ends_with('-') && pkg.starts_with(e)) || pkg.starts_with(&format!("{e}-"))
    })
}

fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// O que "Desinstalar" vai fazer: passos para mostrar e o script para rodar no terminal.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub steps: Vec<String>,
    pub script: String,
    /// Motivo para não deixar (None = pode).
    pub blocked: Option<String>,
}

/// Pacotes que o `pacman -Rns` tiraria junto (o pacote e as dependências que só ele usa).
/// Só consulta (`--print`), não precisa de senha. Vazio se não deu para saber.
pub fn pacman_removal_preview(pkg: &str) -> Vec<String> {
    std::process::Command::new("pacman")
        .args(["-Rns", "--print", "--print-format", "%n %v", pkg])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Monta o plano. `unit` = (unidade, do usuário?) quando abre por um serviço.
pub fn uninstall_plan(t: &Trace, unit: Option<(&str, bool)>, remove_configs: bool) -> Plan {
    let mut steps = vec![];
    let mut sc = vec!["set -e".to_string()];
    let mut run = |step: String, cmd: String| {
        sc.push(format!("echo; echo {}", q(&format!("==> {step}"))));
        sc.push(cmd);
        steps.push(step);
    };
    let h = home();
    if let Some((u, user)) = unit {
        if user {
            run(
                format!("parar e desligar o serviço {u}"),
                format!("systemctl --user disable --now {} || true", q(u)),
            );
        } else {
            run(
                format!("parar e desligar o serviço {u} (pede a senha)"),
                format!("sudo systemctl disable --now {} || true", q(u)),
            );
        }
    }
    // o arquivo de inicialização sem pacote dono fica para trás: apaga
    if let Some(f) = &t.file
        && t.file_pkg.is_none()
    {
        let fs = f.to_string_lossy();
        if f.starts_with(&h) {
            let reload = fs.contains("/systemd/user/");
            run(
                format!("apagar o arquivo de inicialização {}", f.display()),
                format!(
                    "rm -f {}{}",
                    q(&fs),
                    if reload {
                        "; systemctl --user daemon-reload"
                    } else {
                        ""
                    }
                ),
            );
        } else if fs.starts_with("/etc/systemd/system/") {
            run(
                format!("apagar o serviço {} (pede a senha)", f.display()),
                format!("sudo rm -f {}; sudo systemctl daemon-reload", q(&fs)),
            );
        }
    }
    let mut blocked = None;
    let own = |p: &Path| crate::util::basename(&p.to_string_lossy()).starts_with("numapilot");
    if t.exe.as_deref().is_some_and(own)
        || matches!(&t.source, Source::Manual { paths, .. } if paths.iter().any(|p| own(p)))
    {
        blocked = Some("é o próprio numapilot: use o ./uninstall.sh".into());
    }
    match &t.source {
        Source::Pacman { pkg, .. } => {
            if essential_pkg(pkg) && blocked.is_none() {
                blocked = Some(format!("{pkg} faz parte do sistema"));
            }
            run(
                format!(
                    "remover o pacote {pkg} e as dependências que só ele usava (sudo pacman -Rns; o pacman pede confirmação)"
                ),
                format!("sudo pacman -Rns {}", q(pkg)),
            );
        }
        Source::Flatpak { id, user } => run(
            format!("desinstalar o Flatpak {id}"),
            format!(
                "flatpak uninstall {}{}{}",
                if *user { "--user " } else { "" },
                if remove_configs { "--delete-data " } else { "" },
                q(id)
            ),
        ),
        Source::Npm { pkg } => run(
            format!("npm uninstall -g {pkg}"),
            format!("npm uninstall -g {}", q(pkg)),
        ),
        Source::Pipx { pkg } => run(
            format!("pipx uninstall {pkg}"),
            format!("pipx uninstall {}", q(pkg)),
        ),
        Source::Cargo { krate } => run(
            format!("cargo uninstall {krate}"),
            format!("cargo uninstall {}", q(krate)),
        ),
        Source::Numapilot { dir } => run(
            format!(
                "apagar {} e os comandos dele em ~/.local/bin",
                dir.display()
            ),
            format!(
                "for l in \"$HOME\"/.local/bin/*; do case \"$(readlink -f \"$l\")\" in {}/*) rm -f \"$l\";; esac; done; rm -rf {}",
                q(&dir.to_string_lossy()),
                q(&dir.to_string_lossy())
            ),
        ),
        Source::Manual { paths, root } => {
            let list: Vec<String> = paths.iter().map(|p| q(&p.to_string_lossy())).collect();
            run(
                format!(
                    "apagar {}{}",
                    paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                    if *root { " (pede a senha)" } else { "" }
                ),
                format!(
                    "{}rm -f {}",
                    if *root { "sudo " } else { "" },
                    list.join(" ")
                ),
            );
        }
        Source::Unknown => run(
            "não sei de onde o programa veio: só a inicialização é removida".into(),
            "true".into(),
        ),
    }
    if remove_configs && let Some(d) = &t.app_dir {
        run(
            format!("apagar a pasta do programa {}", d.display()),
            format!("rm -rf {}", q(&d.to_string_lossy())),
        );
    }
    if remove_configs && !matches!(t.source, Source::Flatpak { .. }) {
        for c in t
            .configs
            .iter()
            .filter(|c| t.app_dir.as_ref().is_none_or(|d| !c.starts_with(d)))
        {
            run(
                format!("apagar as configurações {}", c.display()),
                format!("rm -rf {}", q(&c.to_string_lossy())),
            );
        }
    }
    sc.push("echo; echo '==> Pronto'".into());
    Plan {
        steps,
        script: sc.join("\n"),
        blocked,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divide_comando() {
        assert_eq!(
            split_command(r#"sh -c "sleep 3 && arch-update --tray""#),
            vec!["sh", "-c", "sleep 3 && arch-update --tray"]
        );
        assert_eq!(
            split_command("\"/opt/a b/x\" --y"),
            vec!["/opt/a b/x", "--y"]
        );
        assert_eq!(split_command("a\\ b c"), vec!["a b", "c"]);
    }

    #[test]
    fn programa_de_verdade() {
        let p = |e: &str| program_of(e).map(|x| x.1);
        assert_eq!(
            p("/usr/bin/steam -silent").as_deref(),
            Some("/usr/bin/steam")
        );
        assert_eq!(
            p(r#"/bin/sh -c "sleep 3 && arch-update --tray""#).as_deref(),
            Some("arch-update")
        );
        assert_eq!(
            p("/usr/bin/node /home/x/.npm-global/lib/node_modules/openclaw/dist/index.js gateway")
                .as_deref(),
            Some("/home/x/.npm-global/lib/node_modules/openclaw/dist/index.js")
        );
        assert_eq!(
            p("env FOO=1 /home/x/.local/bin/deckscreen-host daemon").as_deref(),
            Some("/home/x/.local/bin/deckscreen-host")
        );
        assert_eq!(
            program_of("/usr/bin/flatpak run --branch=stable --command=zapzap com.rtosta.zapzap"),
            Some(("flatpak".into(), "com.rtosta.zapzap".into()))
        );
        assert_eq!(
            p(r#"sh -c "sleep 15; exec hermes-agent --serve""#).as_deref(),
            Some("hermes-agent")
        );
    }

    #[test]
    fn origem_pelo_caminho() {
        let h = Path::new("/home/x");
        assert_eq!(
            source_from_path(
                Path::new("/home/x/.npm-global/lib/node_modules/openclaw/dist/index.js"),
                h
            ),
            Some(Source::Npm {
                pkg: "openclaw".into()
            })
        );
        assert_eq!(
            source_from_path(Path::new("/usr/lib/node_modules/@scope/tool/bin.js"), h),
            Some(Source::Npm {
                pkg: "@scope/tool".into()
            })
        );
        assert_eq!(
            source_from_path(
                Path::new("/home/x/.local/share/pipx/venvs/yt-dlp/bin/yt-dlp"),
                h
            ),
            Some(Source::Pipx {
                pkg: "yt-dlp".into()
            })
        );
        assert_eq!(
            source_from_path(Path::new("/var/lib/flatpak/app/com.a.B/current/x"), h),
            Some(Source::Flatpak {
                id: "com.a.B".into(),
                user: false
            })
        );
        assert_eq!(
            source_from_path(
                Path::new("/home/x/.local/share/numapilot/comunidade/umr/umr.sh"),
                h
            ),
            Some(Source::Numapilot {
                dir: PathBuf::from("/home/x/.local/share/numapilot/comunidade/umr")
            })
        );
        assert_eq!(
            source_from_path(Path::new("/home/x/.local/bin/deckscreen-host"), h),
            None
        );
    }

    #[test]
    fn pasta_do_programa() {
        let h = Path::new("/home/x");
        assert_eq!(
            app_dir_of(Path::new("/home/x/homebrew/services/PluginLoader"), h),
            Some(PathBuf::from("/home/x/homebrew"))
        );
        assert_eq!(
            app_dir_of(Path::new("/home/x/.local/bin/deckscreen-host"), h),
            None
        );
        assert_eq!(
            app_dir_of(Path::new("/home/x/Apps/Foo/bin/foo"), h),
            Some(PathBuf::from("/home/x/Apps/Foo"))
        );
        assert_eq!(app_dir_of(Path::new("/home/x/Apps/Foo.AppImage"), h), None);
        assert_eq!(app_dir_of(Path::new("/usr/bin/steam"), h), None);
        // pastas de ferramentas e de projetos nunca viram "pasta do programa"
        assert_eq!(app_dir_of(Path::new("/home/x/go/bin/tool"), h), None);
        assert_eq!(
            app_dir_of(Path::new("/home/x/.conda/envs/a/bin/python"), h),
            None
        );
        assert_eq!(app_dir_of(Path::new("/home/x/.dotnet/tools/x"), h), None);
        assert_eq!(
            app_dir_of(Path::new("/home/x/Projetos/meu/target/release/x"), h),
            None
        );
        assert_eq!(
            app_dir_of(Path::new("/home/x/flutter/bin/flutter"), h),
            None
        );
    }

    #[test]
    fn planos() {
        let t = Trace {
            file: Some(home().join(".config/systemd/user/openclaw-gateway.service")),
            file_pkg: None,
            exe: None,
            source: Source::Npm {
                pkg: "openclaw".into(),
            },
            description: None,
            size: None,
            configs: vec![home().join(".openclaw")],
            app_dir: None,
        };
        let p = uninstall_plan(&t, Some(("openclaw-gateway.service", true)), true);
        assert!(p.blocked.is_none());
        assert!(
            p.script
                .contains("systemctl --user disable --now 'openclaw-gateway.service'")
        );
        assert!(p.script.contains("daemon-reload"));
        assert!(p.script.contains("npm uninstall -g 'openclaw'"));
        assert!(p.script.contains("rm -rf"));
        let sem = uninstall_plan(&t, None, false);
        assert!(!sem.script.contains("rm -rf"));
        let sys = Trace {
            source: Source::Pacman {
                pkg: "wireplumber".into(),
                aur: false,
                version: "1".into(),
            },
            file: None,
            ..t.clone()
        };
        assert!(uninstall_plan(&sys, None, false).blocked.is_some());
        let steam = Trace {
            source: Source::Pacman {
                pkg: "steam".into(),
                aur: false,
                version: "1".into(),
            },
            file: None,
            ..t
        };
        let p = uninstall_plan(&steam, None, false);
        assert!(p.blocked.is_none() && p.script.contains("sudo pacman -Rns 'steam'"));
        assert!(
            essential_pkg("plasma-workspace")
                && essential_pkg("linux-cachyos")
                && !essential_pkg("steam")
        );
        let me = Trace {
            source: Source::Manual {
                paths: vec![PathBuf::from("/usr/local/bin/numapilot-gui")],
                root: true,
            },
            file: None,
            exe: Some(PathBuf::from("/usr/local/bin/numapilot-gui")),
            description: None,
            size: None,
            configs: vec![],
            app_dir: None,
            file_pkg: None,
        };
        assert!(uninstall_plan(&me, None, false).blocked.is_some());
    }
}
