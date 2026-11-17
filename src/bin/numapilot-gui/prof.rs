//! `NUMAPILOT_GUI_STATS=1`: every 10 s, prints to stderr how many frames were drawn and how much
//! time each measured part took per second. For finding what makes the window heavy; costs
//! nothing when the variable is not set.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Acc {
    since: Instant,
    frames: u32,
    causes: BTreeMap<String, u32>,
    parts: BTreeMap<&'static str, (Duration, u32)>,
}

static ACC: Mutex<Option<Acc>> = Mutex::new(None);

pub fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("NUMAPILOT_GUI_STATS").is_some())
}

/// Runs `f` and, with the statistics on, adds its time to `name`.
pub fn time<T>(name: &'static str, f: impl FnOnce() -> T) -> T {
    if !on() {
        return f();
    }
    let t = Instant::now();
    let r = f();
    add(name, t.elapsed());
    r
}

pub fn add(name: &'static str, d: Duration) {
    if let Ok(mut a) = ACC.lock() {
        let a = a.get_or_insert_with(|| Acc {
            since: Instant::now(),
            frames: 0,
            causes: BTreeMap::new(),
            parts: BTreeMap::new(),
        });
        let e = a.parts.entry(name).or_default();
        e.0 += d;
        e.1 += 1;
    }
}

/// Counts one frame and prints the report every 10 s.
pub fn frame(tab: &str, ctx: &eframe::egui::Context) {
    if !on() {
        return;
    }
    let Ok(mut g) = ACC.lock() else { return };
    let a = g.get_or_insert_with(|| Acc {
        since: Instant::now(),
        frames: 0,
        causes: BTreeMap::new(),
        parts: BTreeMap::new(),
    });
    a.frames += 1;
    for c in ctx.repaint_causes() {
        *a.causes.entry(c.to_string()).or_default() += 1;
    }
    let secs = a.since.elapsed().as_secs_f64();
    if secs < 10.0 {
        return;
    }
    let mut parts: Vec<_> = a.parts.iter().collect();
    parts.sort_by(|x, y| y.1.0.cmp(&x.1.0));
    let list: Vec<String> = parts
        .iter()
        .map(|(n, (d, c))| {
            format!(
                "{n} {:.1} ms/s ({:.1} ms × {c})",
                d.as_secs_f64() * 1000.0 / secs,
                d.as_secs_f64() * 1000.0 / (*c).max(1) as f64
            )
        })
        .collect();
    eprintln!(
        "[stats] {tab}: {:.1} frames/s · {}",
        a.frames as f64 / secs,
        list.join(" · ")
    );
    let mut causes: Vec<_> = a.causes.iter().collect();
    causes.sort_by(|x, y| y.1.cmp(x.1));
    for (c, n) in causes.iter().take(6) {
        eprintln!("[stats]   repaint {n}× {c}");
    }
    *g = None;
}
