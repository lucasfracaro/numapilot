//! Comandos em segundo plano (ligar laboratório, atualizar...) com a saída visível na interface.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Job {
    pub title: String,
    pub out: Arc<Mutex<String>>,
    /// None = rodando; Some(true/false) = terminou com sucesso/erro
    pub status: Arc<Mutex<Option<bool>>>,
}

impl Job {
    pub fn text(&self) -> String {
        self.out.lock().map(|s| s.clone()).unwrap_or_default()
    }
    pub fn finished(&self) -> Option<bool> {
        self.status.lock().ok().and_then(|s| *s)
    }
}

/// Caminho do próprio numapilot-gui. Depois de reinstalar, o processo antigo vê
/// "/usr/bin/numapilot-gui (deleted)": usa o arquivo novo no mesmo lugar ou o do PATH.
pub fn self_exe() -> String {
    std::env::current_exe()
        .ok()
        .map(|p| {
            let s = p.to_string_lossy().to_string();
            s.strip_suffix(" (deleted)")
                .map(str::to_string)
                .unwrap_or(s)
        })
        .filter(|s| std::path::Path::new(s).is_file())
        .unwrap_or_else(|| "numapilot-gui".into())
}

/// O binário em execução foi trocado (reinstalado) desde que o processo abriu?
pub fn self_replaced() -> bool {
    std::fs::read_link("/proc/self/exe").is_ok_and(|p| p.to_string_lossy().ends_with(" (deleted)"))
}

/// Binário irmão do numapilot-gui (ex.: gfx906-rocm), ou o do PATH.
pub fn sibling(bin: &str) -> String {
    Some(PathBuf::from(self_exe()))
        .and_then(|p| p.parent().map(|d| d.join(bin)))
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| bin.to_string())
}

/// Como `sibling`, mas sempre com caminho absoluto (o pkexec exige).
pub fn sibling_abs(bin: &str) -> String {
    let s = sibling(bin);
    if s.contains('/') {
        return s;
    }
    std::env::var_os("PATH")
        .and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join(bin))
                .find(|p| p.is_file())
        })
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("/usr/bin/{bin}"))
}

pub fn run(title: impl Into<String>, argv: Vec<String>, ctx: eframe::egui::Context) -> Job {
    let job = Job {
        title: title.into(),
        out: Arc::new(Mutex::new(String::new())),
        status: Arc::new(Mutex::new(None)),
    };
    let j = job.clone();
    std::thread::spawn(move || {
        let push = |s: &str| {
            if let Ok(mut o) = j.out.lock() {
                // Tira as cores ANSI e limita o tamanho.
                let clean: String = strip_ansi(s);
                o.push_str(&clean);
                o.push('\n');
                if o.len() > 200_000 {
                    let cut = o.len() - 150_000;
                    o.drain(..cut);
                }
            }
            ctx.request_repaint();
        };
        push(&format!("$ {}", argv.join(" ")));
        numapilot::log::info(&crate::tf!(
            "comando: {} — {}",
            "command: {} — {}",
            j.title,
            argv.join(" ")
        ));
        let child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                push(&crate::tf!(
                    "não consegui executar: {e}",
                    "could not run it: {e}"
                ));
                numapilot::log::error(&crate::tf!(
                    "não consegui executar {}: {e}",
                    "could not run {}: {e}",
                    argv[0]
                ));
                if let Ok(mut s) = j.status.lock() {
                    *s = Some(false);
                }
                return;
            }
        };
        let err = child.stderr.take();
        let j2 = j.clone();
        let ctx2 = ctx.clone();
        let t = std::thread::spawn(move || {
            if let Some(e) = err {
                for l in BufReader::new(e).lines().map_while(Result::ok) {
                    if let Ok(mut o) = j2.out.lock() {
                        o.push_str(&strip_ansi(&l));
                        o.push('\n');
                    }
                    ctx2.request_repaint();
                }
            }
        });
        if let Some(o) = child.stdout.take() {
            for l in BufReader::new(o).lines().map_while(Result::ok) {
                push(&l);
            }
        }
        let _ = t.join();
        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        // no registro: o comando e, se deu erro, o fim da saída
        let title = j.title.clone();
        if ok {
            numapilot::log::info(&crate::tf!(
                "comando concluído: {title}",
                "command finished: {title}"
            ));
        } else {
            let tail = j
                .out
                .lock()
                .map(|o| {
                    let v: Vec<&str> = o.lines().rev().take(15).collect();
                    v.into_iter().rev().collect::<Vec<_>>().join("\n")
                })
                .unwrap_or_default();
            numapilot::log::error(&crate::tf!(
                "comando com erro: {title}\n{tail}",
                "command failed: {title}\n{tail}"
            ));
        }
        push(if ok {
            crate::i18n::tr("[concluído]")
        } else {
            crate::i18n::tr("[terminou com erro]")
        });
        if let Ok(mut s) = j.status.lock() {
            *s = Some(ok);
        }
        ctx.request_repaint();
    });
    job
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        if c != '\r' {
            out.push(c);
        }
    }
    out
}

/// Abre um programa sem prender a interface (nova sessão, sem terminal).
pub fn spawn_detached(argv: &[String]) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut c = Command::new(&argv[0]);
    c.args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    unsafe {
        c.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    c.spawn().map(|_| ())
}

pub fn open_url(url: &str) -> std::io::Result<()> {
    spawn_detached(&["xdg-open".to_string(), url.to_string()])
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Saída de um comando curto (ou vazio se falhar).
pub fn quick(argv: &[&str]) -> String {
    Command::new(argv[0])
        .args(&argv[1..])
        .stderr(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Abre um terminal rodando o comando (para o que pede senha ou confirmação, como o paru).
pub fn in_terminal(cmd: &[String]) -> Result<(), String> {
    let has = |b: &str| numapilot::containers::which(b);
    let term: Vec<String> = if has("konsole") {
        vec!["konsole".into(), "-e".into()]
    } else if has("gnome-terminal") {
        vec!["gnome-terminal".into(), "--".into()]
    } else if has("kitty") {
        vec!["kitty".into()]
    } else if has("alacritty") {
        vec!["alacritty".into(), "-e".into()]
    } else if has("xterm") {
        vec!["xterm".into(), "-e".into()]
    } else {
        return Err(crate::i18n::tr(
            "nenhum terminal encontrado (konsole, gnome-terminal, kitty, alacritty, xterm)",
        )
        .into());
    };
    // Roda o comando e, no fim, deixa claro que acabou: fecha com Enter.
    let quoted: Vec<String> = cmd
        .iter()
        .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
        .collect();
    let script = format!(
        "{}; s=$?; echo; if [ $s -eq 0 ]; then echo 'Terminou.'; else echo \"Terminou com erro ($s).\"; fi; printf 'Enter para fechar esta janela. '; read -r _",
        quoted.join(" ")
    );
    let wrapped = vec!["sh".to_string(), "-c".to_string(), script];
    spawn_detached(&[term, wrapped].concat()).map_err(|e| e.to_string())
}
