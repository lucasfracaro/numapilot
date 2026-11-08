//! Uma janela só: abrir o numapilot de novo (ícone da bandeja, menu) traz para frente
//! a janela que já está aberta em vez de abrir outra.

use eframe::egui;
use std::io::Write;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

pub static SHOW: AtomicBool = AtomicBool::new(false);

fn runtime(name: &str) -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/tmp/numapilot-{}", unsafe { libc::getuid() })))
        .join(name)
}

fn sock() -> PathBuf {
    runtime("numapilot-gui.sock")
}

/// Já existe uma janela? Se sim, pede para ela aparecer e devolve true.
pub fn show_existing() -> bool {
    UnixStream::connect(sock())
        .and_then(|mut s| s.write_all(b"show\n"))
        .is_ok()
}

/// Escuta pedidos de "aparecer" das próximas aberturas.
pub fn listen(ctx: egui::Context) {
    let p = sock();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    // sobra de uma janela que fechou sem limpar (o connect acima falhou)
    let _ = std::fs::remove_file(&p);
    let Ok(l) = UnixListener::bind(&p) else {
        return;
    };
    std::thread::spawn(move || {
        for s in l.incoming() {
            if s.is_ok() {
                SHOW.store(true, Ordering::SeqCst);
                ctx.request_repaint();
            }
        }
    });
}

/// Traz a janela para frente se alguém pediu.
pub fn handle(ctx: &egui::Context) {
    if SHOW.swap(false, Ordering::SeqCst) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(
            egui::UserAttentionType::Informational,
        ));
    }
}

/// Trava exclusiva (flock) num arquivo do runtime dir; None se outro processo já tem.
pub fn lock(name: &str) -> Option<std::fs::File> {
    use std::os::fd::AsRawFd;
    let p = runtime(name);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let f = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&p)
        .ok()?;
    (unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0).then_some(f)
}

/// Remove o socket ao fechar a janela.
pub fn cleanup() {
    let _ = std::fs::remove_file(sock());
}
