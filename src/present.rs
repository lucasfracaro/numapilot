//! Presentation on another card ("render on one, improve on the other"): the game renders on the
//! profile's card, gamescope composes on the output card (`--prefer-vk-device`), which takes the
//! finished image, upscales it, generates frames and shows it through its own video output.
//!
//! The chain only sees the final image (like Lossless Scaling on Windows): the real FPS and the
//! input delay stay the game's, frame generation adds about one real frame of delay and turns off
//! under the base FPS of the profile. So the useful number is real FPS against shown FPS: the real
//! one comes from MangoHud inside the game (it counts the game's own presents), the shown one from
//! the multiplier applied only while the base FPS is above the minimum.

use crate::profiles::{Gamescope, Present};
use crate::topo::Gpu;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Upscaling models: (value, name, ready). The planned ones fall back to FSR 1 for now. Anime4K,
/// ArtCNN and FSRCNNX run in numapilot's Vulkan layer inside gamescope (`layer/`,
/// docs/PRESENT-CHAIN.md); without the layer installed they fall back to FSR 1 too. FSRCNNX is
/// downloaded on first use (LGPL, not shipped).
pub const UPSCALERS: [(&str, &str, bool); 12] = [
    ("none", "none (output at the game's resolution)", true),
    ("fsr1", "FSR 1 (gamescope)", true),
    ("nis", "NIS (gamescope)", true),
    ("anime4k-s", "Anime4K CNN 2× S (fastest)", true),
    ("anime4k-m", "Anime4K CNN 2× M", true),
    ("anime4k-l", "Anime4K CNN 2× L (sharpest)", true),
    ("anime4k", "Anime4K (= M)", true),
    ("artcnn-c4f16", "ArtCNN C4F16 (light, sharp edges)", true),
    ("artcnn-c4f32", "ArtCNN C4F32", true),
    ("fsrcnnx", "FSRCNNX 8 (downloaded on first use)", true),
    ("fsrcnnx-16", "FSRCNNX 16 (downloaded on first use)", true),
    ("srvgg", "SRVGGNetCompact (planned)", false),
];

/// Model of numapilot's upscaling layer for an upscaler value, if it is one of its models.
pub fn layer_model(upscaler: &str) -> Option<&'static str> {
    match upscaler {
        "anime4k-s" => Some("anime4k-s"),
        "anime4k-m" | "anime4k" => Some("anime4k-m"),
        "anime4k-l" => Some("anime4k-l"),
        "artcnn-c4f16" => Some("artcnn-c4f16"),
        "artcnn-c4f32" => Some("artcnn-c4f32"),
        "fsrcnnx" => Some("fsrcnnx"),
        "fsrcnnx-16" => Some("fsrcnnx-16"),
        _ => None,
    }
}

/// A layer model that is downloaded on first use and is not on this machine yet.
pub fn model_missing(upscaler: &str) -> bool {
    layer_model(upscaler).is_some_and(|m| {
        crate::vkbench::DOWNLOADS.iter().any(|d| d.0 == m)
            && crate::vkbench::model_source(m).is_none()
    })
}

/// Manifest of numapilot's upscaling layer (VK_LAYER_NUMAPILOT_upscale).
pub const LAYER_MANIFEST: &str = "numapilot_upscale.json";

/// Is the upscaling layer installed where the Vulkan loader looks for implicit layers?
pub fn upscale_layer_installed() -> bool {
    let mut dirs: Vec<PathBuf> = vec![];
    if let Some(h) = std::env::var_os("XDG_DATA_HOME") {
        dirs.push(PathBuf::from(h));
    } else if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/share"));
    }
    let data = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(data.split(':').map(PathBuf::from));
    dirs.push("/etc".into());
    dirs.iter().any(|d| {
        d.join("vulkan/implicit_layer.d")
            .join(LAYER_MANIFEST)
            .is_file()
    })
}

/// Frame generators: (value, name, ready). lsfg-vk is a Vulkan layer: "lsfg" loads it in the game
/// (generates on the game's card, with the user's own lsfg-vk settings), "lsfg-out" only in
/// gamescope (generates on the output card, with a config of numapilot's own). Block matching and
/// RIFE are the chain on the output card (planned).
pub const ENGINES: [(&str, &str, bool); 4] = [
    (
        "lsfg",
        "Lossless Scaling on the game's card (lsfg-vk)",
        true,
    ),
    (
        "lsfg-out",
        "Lossless Scaling on the output card (lsfg-vk in gamescope)",
        true,
    ),
    ("blockmatch", "block matching (planned)", false),
    ("rife", "RIFE v4 lite (planned)", false),
];

pub fn upscaler_ready(v: &str) -> bool {
    UPSCALERS.iter().any(|u| u.0 == v && u.2)
}

pub fn engine_ready(v: &str) -> bool {
    ENGINES.iter().any(|e| e.0 == v && e.2)
}

/// Uses lsfg-vk (either way).
pub fn is_lsfg(engine: &str) -> bool {
    engine == "lsfg" || engine == "lsfg-out"
}

/// Name of a frame generator (English; `tr` where it is shown).
pub fn engine_name(v: &str) -> &'static str {
    ENGINES
        .iter()
        .find(|e| e.0 == v)
        .map(|e| e.1)
        .unwrap_or("?")
}

/// Is the lsfg-vk Vulkan layer installed (system or user)?
pub fn lsfg_installed() -> bool {
    crate::lsfg::layer_manifest().is_some()
}

/// What frame generation through lsfg-vk has on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lsfg {
    /// The lsfg-vk Vulkan layer is not installed.
    Missing,
    /// lsfg-vk is there, but not the DLL of Lossless Scaling (bought and installed through Steam).
    NoDll,
    Ready,
}

impl Lsfg {
    pub fn of(s: &crate::lsfg::Status) -> Lsfg {
        if !s.installed() {
            Lsfg::Missing
        } else if !s.has_dll() {
            Lsfg::NoDll
        } else {
            Lsfg::Ready
        }
    }
}

/// What the presentation block adds to a gamescope launch. The output card itself and the split
/// of the environment (gamescope sees only the output card, the game keeps its own) come from
/// `Gamescope::output_gpu` / `run --out-gpu` and `decide::gamescope_split`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Plan {
    /// gamescope and its arguments (upscaling filter included), without the final "--".
    pub gamescope: Vec<String>,
    /// Output card (None = the game's card composes too).
    pub out_pci: Option<String>,
    /// Variables only for the game (MangoHud): they go after "--".
    pub game_env: Vec<(String, String)>,
    /// Variables for gamescope (and inherited by the game unless `game_env` overrides them):
    /// LSFGVK_CONFIG of the "lsfg-out" generator.
    pub gamescope_env: Vec<(String, String)>,
    /// lsfg-vk runs inside gamescope: the launcher writes `lsfg_out_path()` with
    /// `lsfg_out_config` before starting it.
    pub lsfg_out: bool,
    /// Frames shown per real frame that will really be generated (1 if the generator is missing).
    pub multiplier: u8,
    pub warnings: Vec<String>,
    /// MangoHud measures real × shown (set by the launcher when it is installed).
    pub measuring: bool,
}

impl Plan {
    /// What goes between gamescope's "--" and the game: `child` is `decide::gamescope_split`'s
    /// `env …` (empty without another output card), followed by this plan's game variables.
    pub fn game_prefix(&self, child: Vec<String>) -> Vec<String> {
        let mut c = child;
        if !self.game_env.is_empty() {
            if c.is_empty() {
                c.push("env".into());
            }
            c.extend(self.game_env.iter().map(|(k, v)| format!("{k}={v}")));
        }
        c
    }
}

/// Builds the launch: `gs` is the gamescope block in effect (the profile's or the default one),
/// `out` the output card when it is not the game's (`decide::resolve_out_gpu`).
pub fn plan(pr: &Present, gs: &Gamescope, out: Option<&Gpu>, lsfg: Lsfg) -> Plan {
    let mut p = plan_with(pr, gs, out, lsfg, upscale_layer_installed());
    if model_missing(&pr.upscaler) {
        // `run` downloads it first; here only when that failed
        p.warnings.push(crate::tf!(
            "the upscaling model {} is not downloaded (no internet?): the layer passes the picture through without upscaling",
            "o modelo de upscaling {} não foi baixado (sem internet?): a camada passa a imagem sem ampliar",
            pr.upscaler
        ));
    }
    p
}

/// `plan` with whether numapilot's upscaling layer is installed given (tests).
pub fn plan_with(pr: &Present, gs: &Gamescope, out: Option<&Gpu>, lsfg: Lsfg, layer: bool) -> Plan {
    let mut warnings = vec![];
    let mut g = gs.clone();
    g.fsr = false;
    let mut a = g.argv();
    let mut gamescope_env = vec![];
    let up = if layer_model(&pr.upscaler).is_some() && !layer {
        warnings.push(crate::tf!(
            "numapilot's upscaling layer is not installed (reinstall numapilot): using FSR 1 instead of {}",
            "a camada de upscaling do numapilot não está instalada (reinstale o numapilot): usando FSR 1 no lugar de {}",
            pr.upscaler
        ));
        "fsr1"
    } else if upscaler_ready(&pr.upscaler) {
        pr.upscaler.as_str()
    } else {
        warnings.push(crate::tf!(
            "upscaling model '{}' is not ready yet: using FSR 1",
            "o modelo de upscaling '{}' ainda não está pronto: usando FSR 1",
            pr.upscaler
        ));
        "fsr1"
    };
    match up {
        "fsr1" => a.extend(["-F".into(), "fsr".into()]),
        "nis" => a.extend(["-F".into(), "nis".into()]),
        _ => {}
    }
    if let Some(model) = layer_model(up) {
        // The layer only acts in gamescope (the game inherits the variables and is left alone).
        // gamescope must present through a Vulkan swapchain: SDL backend, like lsfg-out.
        // gamescope always composes at the window's size, so it scales the game with integer
        // nearest scaling (each game pixel a block of k×k) and the layer reads one texel per
        // block back, runs the model at the game's size and writes the 2× result.
        if !a.iter().any(|x| x == "--backend") {
            a.extend(["--backend".into(), "sdl".into()]);
        }
        a.extend(["-S".into(), "integer".into(), "-F".into(), "nearest".into()]);
        set(&mut gamescope_env, "ENABLE_NUMAPILOT_UPSCALE", "1".into());
        set(&mut gamescope_env, "NUMAPILOT_UPSCALE_MODEL", model.into());
        if let (Some(w), Some(h)) = (g.render_w, g.render_h) {
            set(
                &mut gamescope_env,
                "NUMAPILOT_UPSCALE_SRC",
                format!("{w}x{h}"),
            );
        }
        if let (Some(rw), Some(ow)) = (g.render_w, g.out_w)
            && rw * 2 != ow
        {
            warnings.push(crate::tf!(
                "the layer's models upscale exactly 2×: with the game at {} wide the output should be {} wide (it is {}); smaller than that the layer does nothing, larger leaves a black border",
                "os modelos da camada ampliam exatamente 2×: com o jogo em {} de largura a saída deveria ter {} (está em {}); menor que isso a camada não age, maior deixa uma borda preta",
                rw,
                rw * 2,
                ow
            ));
        }
    }
    if up != "none" && g.render_w.is_none() && g.render_h.is_none() {
        warnings.push(
            crate::i18n::tr(
                "upscaling needs a game resolution lower than the output (gamescope block)",
            )
            .into(),
        );
    }
    if let Some(o) = out
        && !o.display_connected
    {
        warnings.push(crate::tf!(
            "no monitor on the {}: the image only shows up through a monitor connected to it",
            "nenhum monitor na {}: a imagem só aparece num monitor ligado nela",
            o.alias
        ));
    }
    let game_env = vec![];
    let mut lsfg_out = false;
    let mut multiplier = pr.multiplier.max(1);
    if multiplier > 1 {
        if !engine_ready(&pr.engine) {
            warnings.push(crate::tf!(
                "frame generator '{}' is not ready yet: no frame generation",
                "o gerador de quadros '{}' ainda não está pronto: sem geração de quadros",
                pr.engine
            ));
            multiplier = 1;
        } else if is_lsfg(&pr.engine) && lsfg == Lsfg::Missing {
            warnings.push(crate::i18n::tr("lsfg-vk is not installed: no frame generation").into());
            multiplier = 1;
        } else if is_lsfg(&pr.engine) && lsfg == Lsfg::NoDll {
            warnings.push(
                crate::i18n::tr(
                    "Lossless Scaling is not installed through Steam (lsfg-vk only works with its files): buy and install it there first, \"lsfg-vk\" branch; no frame generation",
                )
                .into(),
            );
            multiplier = 1;
        } else if pr.engine == "lsfg-out" {
            // Tested on the server (2026-11-20): official lsfg-vk has no card setting, but loaded
            // only in gamescope it generates where gamescope runs (the output card). It needs
            // gamescope's SDL backend: in the default nested mode the layer is never triggered.
            // The config only has a profile for gamescope, so the game (which inherits
            // LSFGVK_CONFIG) does not generate frames as well.
            if !a.iter().any(|x| x == "--backend") {
                a.extend(["--backend".into(), "sdl".into()]);
            }
            set(
                &mut gamescope_env,
                "LSFGVK_CONFIG",
                lsfg_out_path().to_string_lossy().into_owned(),
            );
            lsfg_out = true;
        } else if pr.engine == "lsfg" {
            // lsfg-vk inside the game, with the user's own settings (multiplier included).
            warnings.push(crate::tf!(
                "lsfg-vk generates on the game's card and takes the multiplier ({}×) and its own minimum from its settings (lsfg-vk-ui)",
                "o lsfg-vk gera na placa do jogo e usa o multiplicador ({}×) e o próprio mínimo das configurações dele (lsfg-vk-ui)",
                multiplier
            ));
        }
    }
    Plan {
        gamescope: a,
        out_pci: out.map(|o| o.pci.clone()),
        game_env,
        gamescope_env,
        lsfg_out,
        multiplier,
        warnings,
        measuring: false,
    }
}

fn set(env: &mut Vec<(String, String)>, k: &str, v: String) {
    env.retain(|(x, _)| x != k);
    env.push((k.into(), v));
}

/// Name of numapilot's lsfg-vk profile (only in its own config file).
pub const LSFG_OUT_PROFILE: &str = "numapilot-output";
/// Processes the "lsfg-out" profile is active in.
pub const LSFG_OUT_ACTIVE_IN: [&str; 2] = ["gamescope", "gamescope-wl"];

/// numapilot's own lsfg-vk config for generation inside gamescope. The user's `conf.toml` is
/// never written.
pub fn lsfg_out_path() -> PathBuf {
    state_dir().join("lsfg-vk-output.toml")
}

/// lsfg-vk config for generation inside gamescope, made from the user's (`user`, may be empty):
/// the top-level keys and `[global]` stay (DLL path, fp16…), the user's profiles are left out (a
/// game matched by one would generate as well) and one profile matches gamescope only, with the
/// other settings (flow scale, pacing…) of the user's first profile and the chosen multiplier.
/// `dll` goes into `[global]` when it has none.
pub fn lsfg_out_config(user: &str, dll: Option<&std::path::Path>, multiplier: u8) -> String {
    #[derive(PartialEq)]
    enum Part {
        Top,
        Global,
        FirstProfile,
        Other,
    }
    let (mut top, mut global, mut first) = (vec![], vec![], vec![]);
    let mut part = Part::Top;
    let mut profiles = 0;
    for line in user.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            let head: String = t.chars().filter(|c| !c.is_whitespace()).collect();
            part = if head.starts_with("[global]") {
                Part::Global
            } else if head.starts_with("[[profile]]") {
                profiles += 1;
                if profiles == 1 {
                    Part::FirstProfile
                } else {
                    Part::Other
                }
            } else {
                Part::Other
            };
            continue;
        }
        let key = t.split('=').next().unwrap_or("").trim();
        match part {
            Part::Top => top.push(line),
            Part::Global => global.push(line),
            Part::FirstProfile if !["name", "active_in", "multiplier"].contains(&key) => {
                first.push(line)
            }
            _ => {}
        }
    }
    let trim = |v: &mut Vec<&str>| {
        while v.last().is_some_and(|l| l.trim().is_empty()) {
            v.pop();
        }
    };
    trim(&mut top);
    trim(&mut global);
    trim(&mut first);
    let q = |s: &str| toml::Value::String(s.into()).to_string();
    let mut o = String::from(
        "# Written by numapilot for frame generation inside gamescope (lsfg-vk on the output card).\n\
         # Made from your lsfg-vk config, which is left as it is. Rewritten at every start.\n",
    );
    // lsfg-vk 2.x refuses a file without its format version ("Unsupported configuration
    // version (must be 2)"): the user's is kept, else 2 (no conf.toml yet).
    if !top
        .iter()
        .any(|l| l.split('=').next().is_some_and(|k| k.trim() == "version"))
    {
        top.insert(0, "version = 2");
    }
    for l in &top {
        o.push_str(l);
        o.push('\n');
    }
    if !top.is_empty() {
        o.push('\n');
    }
    o.push_str("[global]\n");
    if let Some(d) = dll.filter(|_| crate::lsfg::config_dll(&global.join("\n")).is_none()) {
        o.push_str(&format!("dll = {}\n", q(&d.to_string_lossy())));
    }
    for l in &global {
        o.push_str(l);
        o.push('\n');
    }
    o.push_str(&format!(
        "\n[[profile]]\nname = {}\nactive_in = [{}]\nmultiplier = {}\n",
        q(LSFG_OUT_PROFILE),
        LSFG_OUT_ACTIVE_IN.map(q).join(", "),
        multiplier.max(2)
    ));
    for l in &first {
        o.push_str(l);
        o.push('\n');
    }
    o
}

/// Writes `lsfg_out_path()` from the user's config (`lsfg::config_path`) and the DLL found.
pub fn write_lsfg_out(st: &crate::lsfg::Status, multiplier: u8) -> std::io::Result<PathBuf> {
    // LSFGVK_CONFIG may point at numapilot's own file (inherited): read the user's real one then.
    let mut user = crate::lsfg::config_path();
    if user == lsfg_out_path() {
        user = crate::lsfg::home().join(".config/lsfg-vk/conf.toml");
    }
    let text = std::fs::read_to_string(&user).unwrap_or_default();
    let dll = st
        .dll_override
        .clone()
        .or_else(|| st.lossless.as_ref().and_then(|l| l.dll.clone()));
    let p = lsfg_out_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(&p, lsfg_out_config(&text, dll.as_deref(), multiplier))?;
    Ok(p)
}

// ---------------------------------------------------------------------------------------------
// Measurement: real FPS (MangoHud) against shown FPS.

/// Where MangoHud writes the CSVs of the presentation measurement for one program.
pub fn log_dir(key: &str) -> PathBuf {
    state_dir()
        .join("present-logs")
        .join(crate::tuner::safe_key(key))
}

/// MangoHud variables for the game: logs every frame of the whole session (starts after 5 s, so
/// loading screens weigh less).
pub fn mangohud_env(key: &str) -> Vec<(String, String)> {
    let dir = log_dir(key);
    let _ = std::fs::create_dir_all(&dir);
    let mine = format!(
        "autostart_log=5,log_interval=0,output_folder={},log_versioning=0",
        dir.display()
    );
    let cfg = match std::env::var("MANGOHUD_CONFIG") {
        Ok(c) if !c.trim().is_empty() => format!("{c},{mine}"),
        _ => mine,
    };
    vec![
        ("MANGOHUD".into(), "1".into()),
        ("MANGOHUD_CONFIG".into(), cfg),
    ]
}

/// One session: what the game rendered and what was shown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Measured {
    /// When it ended (seconds since 1970).
    pub at: u64,
    pub seconds: f64,
    /// Real FPS (the game's frames) and its 1% low.
    pub real_avg: f64,
    pub real_low1: f64,
    /// Shown FPS: real frames plus the generated ones while generation was on.
    pub shown_avg: f64,
    /// true = counted on the output; false = calculated from the multiplier and the minimum.
    pub shown_measured: bool,
    pub multiplier: u8,
    pub min_fps: u32,
    /// How many times generation turned off (base FPS fell under the minimum) and for how long.
    pub turned_off: u32,
    pub off_seconds: f64,
    /// Output card (PCI) or None when the game's card shows too.
    #[serde(default)]
    pub out_pci: Option<String>,
}

/// Window (ms) of the base FPS that decides whether generation is on, like a generator would.
const WINDOW_MS: f64 = 500.0;

/// Reads the frame times of MangoHud's CSV (ms).
pub fn frametimes(csv: &str) -> Vec<f64> {
    let mut lines = csv.lines();
    let Some(header) = lines
        .by_ref()
        .find(|l| l.starts_with("fps,") || l.contains(",frametime"))
    else {
        return vec![];
    };
    let cols: Vec<&str> = header.split(',').map(str::trim).collect();
    let Some(fi) = cols.iter().position(|c| *c == "frametime") else {
        return vec![];
    };
    lines
        .filter_map(|l| l.split(',').nth(fi)?.trim().parse::<f64>().ok())
        .filter(|v| *v > 0.0 && *v < 5000.0)
        .collect()
}

/// Real against shown from the game's frame times. Generation counts as on while the base FPS of
/// the last half second is at or above `min_fps`.
pub fn measure(ft: &[f64], multiplier: u8, min_fps: u32) -> Option<Measured> {
    if ft.len() < 30 {
        return None;
    }
    let total_ms: f64 = ft.iter().sum();
    let mut sorted = ft.to_vec();
    sorted.sort_by(|a, b| b.total_cmp(a));
    let n1 = ((sorted.len() as f64 * 0.01).ceil() as usize).max(1);
    let low1 = 1000.0 / (sorted[..n1].iter().sum::<f64>() / n1 as f64);
    let m = multiplier.max(1) as f64;
    let mut shown = 0.0;
    let mut off_ms = 0.0;
    let mut turned_off = 0;
    let mut on = true;
    let (mut start, mut win) = (0usize, 0.0f64);
    for (i, &t) in ft.iter().enumerate() {
        win += t;
        while win - ft[start] >= WINDOW_MS && start < i {
            win -= ft[start];
            start += 1;
        }
        let base = (i + 1 - start) as f64 * 1000.0 / win;
        let now_on = m > 1.0 && base >= min_fps as f64;
        if on && !now_on && m > 1.0 {
            turned_off += 1;
        }
        on = now_on;
        if now_on {
            shown += m;
        } else {
            shown += 1.0;
            if m > 1.0 {
                off_ms += t;
            }
        }
    }
    let secs = total_ms / 1000.0;
    Some(Measured {
        at: crate::history::now(),
        seconds: secs,
        real_avg: ft.len() as f64 / secs,
        real_low1: low1,
        shown_avg: shown / secs,
        shown_measured: false,
        multiplier: multiplier.max(1),
        min_fps,
        turned_off,
        off_seconds: off_ms / 1000.0,
        out_pci: None,
    })
}

/// Last measurement of each program (key → result), in the user's state.
pub fn results_path() -> PathBuf {
    state_dir().join("present.json")
}

/// The user's state folder, or `NUMAPILOT_STATE_DIR` when it is set (tests and separate
/// checkouts keep their measurements away from the installed numapilot's).
fn state_dir() -> PathBuf {
    std::env::var_os("NUMAPILOT_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(crate::tuner::user_state_dir)
}

pub fn load_results() -> BTreeMap<String, Measured> {
    std::fs::read_to_string(results_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn store_result(key: &str, m: &Measured) -> std::io::Result<()> {
    let mut all = load_results();
    all.insert(key.to_string(), m.clone());
    let p = results_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&all).unwrap_or_default())?;
    std::fs::rename(tmp, p)
}

/// One line for the CLI and the interface.
pub fn summary(m: &Measured) -> String {
    let shown = if m.shown_measured {
        crate::tf!("{:.0} shown", "{:.0} mostrados", m.shown_avg)
    } else {
        crate::tf!(
            "{:.0} shown (calculated)",
            "{:.0} mostrados (calculado)",
            m.shown_avg
        )
    };
    let mut s = crate::tf!(
        "real {:.0} FPS (1% low {:.0}) × {shown} in {:.0} s",
        "real {:.0} FPS (1% low {:.0}) × {shown} em {:.0} s",
        m.real_avg,
        m.real_low1,
        m.seconds
    );
    if m.multiplier > 1 {
        s.push_str(&crate::tf!(
            "; generation turned off {} time(s) under {} FPS ({:.0} s)",
            "; a geração desligou {} vez(es) abaixo de {} FPS ({:.0} s)",
            m.turned_off,
            m.min_fps,
            m.off_seconds
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topo::GpuRole;

    fn gpu(alias: &str, pci: &str, device: u16, display: bool) -> Gpu {
        Gpu {
            alias: alias.into(),
            pci: pci.into(),
            vendor: 0x1002,
            device,
            name: alias.into(),
            numa_node: Some(0),
            card: None,
            render: None,
            display_connected: display,
            gfx: None,
            rocr_id: None,
            role: GpuRole::Compute,
        }
    }

    #[test]
    fn filter_once_and_game_env_after_the_split() {
        let b = gpu("gfx906-1", "0000:83:00.0", 0x66af, false);
        let gs = Gamescope {
            render_w: Some(1920),
            render_h: Some(1080),
            fsr: true,
            ..Default::default()
        };
        let pr = Present {
            upscaler: "nis".into(),
            multiplier: 2,
            ..Default::default()
        };
        let p = plan(&pr, &gs, Some(&b), Lsfg::Ready);
        assert_eq!(p.out_pci.as_deref(), Some("0000:83:00.0"));
        assert_eq!(p.gamescope.iter().filter(|x| *x == "-F").count(), 1);
        assert!(p.gamescope.windows(2).any(|w| w == ["-F", "nis"]));
        assert!(
            p.warnings.iter().any(|w| w.contains("no monitor")),
            "{:?}",
            p.warnings
        );
        assert_eq!(p.multiplier, 2);
        let child = vec!["env".to_string(), "DRI_PRIME=pci-0000_03_00_0".to_string()];
        // lsfg-vk in the game: nothing for gamescope, the game keeps its own card
        assert_eq!(p.game_prefix(child.clone()), child);
        assert!(p.gamescope_env.is_empty() && !p.lsfg_out);
        assert!(!p.gamescope.contains(&"--backend".to_string()));
    }

    #[test]
    fn anime4k_through_the_layer() {
        let gs = Gamescope {
            render_w: Some(1280),
            render_h: Some(720),
            out_w: Some(2560),
            out_h: Some(1440),
            ..Default::default()
        };
        let pr = Present {
            upscaler: "anime4k-l".into(),
            multiplier: 1,
            ..Default::default()
        };
        let p = plan_with(&pr, &gs, None, Lsfg::Ready, true);
        // gamescope scales by whole blocks, the layer reads one texel per block
        assert!(
            p.gamescope.windows(2).any(|w| w == ["-F", "nearest"])
                && p.gamescope.windows(2).any(|w| w == ["-S", "integer"])
                && !p.gamescope.windows(2).any(|w| w == ["-F", "fsr"]),
            "{:?}",
            p.gamescope
        );
        assert!(
            p.gamescope_env
                .contains(&("NUMAPILOT_UPSCALE_SRC".into(), "1280x720".into()))
        );
        assert!(p.gamescope.windows(2).any(|w| w == ["--backend", "sdl"]));
        assert!(
            p.gamescope_env
                .contains(&("ENABLE_NUMAPILOT_UPSCALE".into(), "1".into()))
        );
        assert!(
            p.gamescope_env
                .contains(&("NUMAPILOT_UPSCALE_MODEL".into(), "anime4k-l".into()))
        );
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        // the old value is M; an output that is not 2× warns
        let p = plan_with(
            &Present {
                upscaler: "anime4k".into(),
                multiplier: 1,
                ..Default::default()
            },
            &Gamescope {
                out_w: Some(1920),
                out_h: Some(1080),
                ..gs.clone()
            },
            None,
            Lsfg::Ready,
            true,
        );
        assert!(
            p.gamescope_env
                .contains(&("NUMAPILOT_UPSCALE_MODEL".into(), "anime4k-m".into()))
        );
        assert!(
            p.warnings.iter().any(|w| w.contains("2×")),
            "{:?}",
            p.warnings
        );
        // ArtCNN and FSRCNNX go through the layer too
        for m in ["artcnn-c4f16", "artcnn-c4f32", "fsrcnnx", "fsrcnnx-16"] {
            assert!(upscaler_ready(m), "{m}");
            let p = plan_with(
                &Present {
                    upscaler: m.into(),
                    ..pr.clone()
                },
                &gs,
                None,
                Lsfg::Ready,
                true,
            );
            assert!(
                p.gamescope_env
                    .contains(&("NUMAPILOT_UPSCALE_MODEL".into(), m.into())),
                "{m}"
            );
        }
        assert!(!model_missing("artcnn-c4f16") && !model_missing("fsr1"));
        // without the layer: FSR 1 in gamescope
        let p = plan_with(&pr, &gs, None, Lsfg::Ready, false);
        assert!(p.gamescope.windows(2).any(|w| w == ["-F", "fsr"]));
        assert!(p.gamescope_env.is_empty());
        assert!(p.warnings.iter().any(|w| w.contains("layer")));
        // with lsfg-out on top: one --backend only
        let p = plan_with(
            &Present {
                engine: "lsfg-out".into(),
                multiplier: 2,
                ..pr.clone()
            },
            &gs,
            None,
            Lsfg::Ready,
            true,
        );
        assert_eq!(p.gamescope.iter().filter(|x| *x == "--backend").count(), 1);
        assert_eq!(p.gamescope_env.len(), 4);
    }

    #[test]
    fn lsfg_on_the_output_card() {
        let b = gpu("gfx906-1", "0000:83:00.0", 0x66af, true);
        let pr = Present {
            upscaler: "none".into(),
            multiplier: 3,
            engine: "lsfg-out".into(),
            ..Default::default()
        };
        let p = plan(&pr, &Gamescope::default(), Some(&b), Lsfg::Ready);
        assert!(p.lsfg_out);
        assert_eq!(p.multiplier, 3);
        assert!(p.gamescope.windows(2).any(|w| w == ["--backend", "sdl"]));
        assert_eq!(p.gamescope_env.len(), 1);
        assert_eq!(p.gamescope_env[0].0, "LSFGVK_CONFIG");
        assert!(p.game_env.is_empty());
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        // not without the Lossless Scaling files
        let p = plan(&pr, &Gamescope::default(), Some(&b), Lsfg::NoDll);
        assert!(!p.lsfg_out && p.gamescope_env.is_empty());
        assert_eq!(p.multiplier, 1);
    }

    #[test]
    fn own_lsfg_config_keeps_global_and_drops_profiles() {
        let user = "version = 2\n\n[global]\nallow_fp16 = true\ndll = \"/g/LS/lsfg-vk.dll\"\n\n\
                    [[profile]]\nname = \"Skyrim\"\nactive_in = [\"SkyrimSE.exe\"]\nmultiplier = 2\n\
                    flow_scale = 0.8\n\n[[profile]]\nname = \"other\"\nactive_in = [\"x\"]\nflow_scale = 0.2\n";
        let c = lsfg_out_config(user, Some(std::path::Path::new("/other.dll")), 3);
        let v: toml::Table = c.parse().expect(&c);
        assert_eq!(v["version"].as_integer(), Some(2));
        assert_eq!(v["global"]["dll"].as_str(), Some("/g/LS/lsfg-vk.dll"));
        assert_eq!(v["global"]["allow_fp16"].as_bool(), Some(true));
        let ps = v["profile"].as_array().unwrap();
        assert_eq!(ps.len(), 1, "{c}");
        assert_eq!(ps[0]["name"].as_str(), Some(LSFG_OUT_PROFILE));
        assert_eq!(ps[0]["multiplier"].as_integer(), Some(3));
        assert_eq!(ps[0]["flow_scale"].as_float(), Some(0.8));
        let act: Vec<_> = ps[0]["active_in"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|x| x.as_str())
            .collect();
        assert_eq!(act, ["gamescope", "gamescope-wl"]);
        assert!(!c.contains("Skyrim"));
        // no config yet: the DLL found in Steam goes into [global]
        let c = lsfg_out_config("", Some(std::path::Path::new("/s/LS/lsfg-vk.dll")), 2);
        let v: toml::Table = c.parse().expect(&c);
        assert_eq!(v["global"]["dll"].as_str(), Some("/s/LS/lsfg-vk.dll"));
        assert_eq!(v["profile"].as_array().unwrap().len(), 1);
        assert_eq!(v["version"].as_integer(), Some(2), "{c}");
        // a config with only [global] (no version line) gets one too, once
        let c = lsfg_out_config("[global]\nallow_fp16 = false\n", None, 2);
        let v: toml::Table = c.parse().expect(&c);
        assert_eq!(v["version"].as_integer(), Some(2));
        assert_eq!(c.matches("version").count(), 1);
    }

    #[test]
    fn same_card_and_planned_parts() {
        let pr = Present {
            upscaler: "srvgg".into(),
            multiplier: 2,
            engine: "rife".into(),
            ..Default::default()
        };
        let mut p = plan(&pr, &Gamescope::default(), None, Lsfg::Ready);
        assert_eq!(p.out_pci, None);
        assert_eq!(p.multiplier, 1, "RIFE is not ready");
        assert!(p.gamescope.windows(2).any(|w| w == ["-F", "fsr"]));
        assert!(p.game_prefix(vec![]).is_empty());
        p.game_env.push(("MANGOHUD".into(), "1".into()));
        assert_eq!(p.game_prefix(vec![]), ["env", "MANGOHUD=1"]);
        let p = plan(
            &Present {
                multiplier: 2,
                ..Default::default()
            },
            &Gamescope::default(),
            None,
            Lsfg::Missing,
        );
        assert_eq!(p.multiplier, 1, "without lsfg-vk there is no generation");
        let p = plan(
            &Present {
                multiplier: 2,
                ..Default::default()
            },
            &Gamescope::default(),
            None,
            Lsfg::NoDll,
        );
        assert_eq!(
            p.multiplier, 1,
            "without Lossless Scaling from Steam there is no generation"
        );
        assert!(p.warnings.iter().any(|w| w.contains("Steam")));
    }

    #[test]
    fn real_against_shown() {
        // 10 s at 60 FPS, then 2 s at 30 FPS, then 5 s at 60 FPS
        let mut ft = vec![1000.0 / 60.0; 600];
        ft.extend(vec![1000.0 / 30.0; 60]);
        ft.extend(vec![1000.0 / 60.0; 300]);
        let m = measure(&ft, 2, 40).unwrap();
        assert!((m.seconds - 17.0).abs() < 0.01, "{m:?}");
        assert!((m.real_avg - 960.0 / 17.0).abs() < 0.01);
        assert_eq!(m.turned_off, 1);
        // about the 2 s at 30 FPS (the half-second window lags at both ends)
        assert!(m.off_seconds > 1.5 && m.off_seconds < 2.5, "{m:?}");
        assert!(m.shown_avg > 1.8 * m.real_avg && m.shown_avg < 2.0 * m.real_avg);
        let m1 = measure(&ft, 1, 40).unwrap();
        assert_eq!(m1.turned_off, 0);
        assert!((m1.shown_avg - m1.real_avg).abs() < 1e-9);
        assert!(measure(&ft[..10], 2, 40).is_none());
        let csv = "os,cpu\nArch,Xeon\nfps,frametime,cpu_load\n60,16.6,10\n60,16.8,11\n";
        assert_eq!(frametimes(csv), vec![16.6, 16.8]);
        assert!(frametimes("lixo").is_empty());
    }
}
