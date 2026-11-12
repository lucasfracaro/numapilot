//! Single window: opening numapilot again (tray icon, menu) brings the window that is
//! already open to the front instead of opening another.

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

/// Is there already a window? If so, asks it to show up and returns true.
pub fn show_existing() -> bool {
    UnixStream::connect(sock())
        .and_then(|mut s| s.write_all(b"show\n"))
        .is_ok()
}

/// Listens for "show up" requests from later launches.
pub fn listen(ctx: egui::Context) {
    let p = sock();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    // leftover from a window that closed without cleaning up (the connect above failed)
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

/// Brings the window to the front if someone asked.
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

/// Exclusive lock (flock) on a file in the runtime dir; None if another process already has it.
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

/// Removes the socket when the window closes.
pub fn cleanup() {
    let _ = std::fs::remove_file(sock());
}
