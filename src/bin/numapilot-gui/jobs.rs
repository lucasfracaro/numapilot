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

/// Binário irmão do numapilot-gui (ex.: gfx906-rocm), ou o do PATH.
pub fn sibling(bin: &str) -> String {
    std::env::current_exe()
        .ok()
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
        .and_then(|p| std::env::split_paths(&p).map(|d| d.join(bin)).find(|p| p.is_file()))
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("/usr/bin/{bin}"))
}

pub fn run(title: impl Into<String>, argv: Vec<String>, ctx: eframe::egui::Context) -> Job {
    let job = Job { title: title.into(), out: Arc::new(Mutex::new(String::new())), status: Arc::new(Mutex::new(None)) };
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
        let child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                push(&format!("não consegui executar: {e}"));
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
        push(if ok { "[concluído]" } else { "[terminou com erro]" });
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
    c.args(&argv[1..]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
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
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"))
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
