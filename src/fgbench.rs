//! M0 of the "render on one card, improve on the other" chain (docs/PRESENT-CHAIN.md): measures,
//! on the card that would do the work, how long each model of the planned chain takes per frame,
//! and fills the time budget of one real frame.
//!
//! - upscaling: SRVGGNetCompact (Real-ESRGAN `realesr-animevideov3`, 2×) through
//!   `realesrgan-ncnn-vulkan`;
//! - frame generation: RIFE v4 through `rife-ncnn-vulkan`;
//! - the copy of the game's image between the two cards is calculated from the PCIe links.
//!
//! Both tools are ncnn on Vulkan (MIT), the same runtime the chain itself will use, so what they
//! measure is close to what the chain will cost. They also read and write PNGs on the processor
//! (in their own threads), so the numbers are an upper bound.
//!
//! The card is chosen like gamescope's output card (`decide::gamescope_split`): DRI_PRIME with
//! the PCI address plus MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE, so Vulkan shows only that
//! card (device 0) and the two MI50s with the same vendor:device are told apart.

use crate::topo::Gpu;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

/// Goals of the plan per stage (ms, 1080p → 1440p at 60 FPS of base).
pub const GOAL_UPSCALE_MS: f64 = 6.0;
/// Flow (≤ 2 ms) plus synthesis (≤ 2 ms) of one generated frame.
pub const GOAL_GENERATE_MS: f64 = 4.0;
/// Composition and presentation of one shown frame (not measured here).
pub const COMPOSE_MS: f64 = 1.0;

/// One ncnn-vulkan tool found on this machine.
#[derive(Debug, Clone, PartialEq)]
pub struct Tool {
    pub bin: PathBuf,
    /// Folder of the models, when it is not where the tool looks by itself.
    pub models: Option<PathBuf>,
}

/// Finds `bin` in PATH (or ~/.local/bin) and the folder of its model `model` (a subfolder or a
/// file prefix): next to the binary, /usr/share/<bin>, /usr/lib/<bin>, /opt/<bin>.
pub fn find_tool(bin: &str, model: &str) -> Option<Tool> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/bin"));
    }
    let exe = dirs.iter().map(|d| d.join(bin)).find(|p| p.is_file())?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let mut cand = vec![];
    if let Some(d) = exe.parent() {
        cand.push(d.to_path_buf());
        cand.push(d.join("models"));
    }
    for base in ["/usr/share", "/usr/lib", "/opt"] {
        cand.push(Path::new(base).join(bin));
        cand.push(Path::new(base).join(bin).join("models"));
    }
    let models = cand.into_iter().find(|d| has_model(d, model));
    Some(Tool { bin: exe, models })
}

/// Does `dir` hold the model (a folder named like it, or `<model>….param`)?
fn has_model(dir: &Path, model: &str) -> bool {
    if dir.join(model).is_dir() {
        return true;
    }
    std::fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten().any(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(model) && n.ends_with(".param")
        })
    })
}

// ---------------------------------------------------------------------------------------------
// Copy between the cards (calculated).

/// Bandwidth of one PCIe link in GB/s from sysfs ("8.0 GT/s PCIe", "16"), after the line
/// encoding (8b/10b up to 5 GT/s, 128b/130b above).
pub fn link_gbps(speed: &str, width: &str) -> Option<f64> {
    let gt: f64 = speed.split_whitespace().next()?.parse().ok()?;
    let lanes: f64 = width.trim().parse().ok()?;
    if gt <= 0.0 || lanes <= 0.0 {
        return None;
    }
    let enc = if gt <= 5.0 { 0.8 } else { 128.0 / 130.0 };
    Some(gt * enc / 8.0 * lanes)
}

/// The narrowest link between the card and the root port (the MI50 has a switch of its own, and
/// the slot may be slower than the card), in GB/s.
pub fn path_gbps(pci: &str) -> Option<f64> {
    let dev = crate::util::sys(&format!("bus/pci/devices/{pci}"));
    let mut p = std::fs::canonicalize(&dev).unwrap_or(dev);
    let mut best: Option<f64> = None;
    for _ in 0..8 {
        let s = crate::util::read_trim(p.join("current_link_speed"));
        let w = crate::util::read_trim(p.join("current_link_width"));
        if let (Some(s), Some(w)) = (s, w)
            && let Some(g) = link_gbps(&s, &w)
        {
            best = Some(best.map_or(g, |b: f64| b.min(g)));
        }
        let Some(up) = p.parent() else { break };
        let name = up.file_name().map(|n| n.to_string_lossy().into_owned());
        // stop at the PCI domain (pci0000:00), which has no link of its own
        if name.is_none_or(|n| n.starts_with("pci")) {
            break;
        }
        p = up.to_path_buf();
    }
    best
}

/// Usable share of a PCIe link for a large copy (packet headers and flow control).
pub const PCIE_EFFICIENCY: f64 = 0.75;

/// Time to copy one RGBA8 image of w×h over a link of `gbps` (ms).
pub fn copy_ms(w: u32, h: u32, gbps: f64) -> f64 {
    let bytes = w as f64 * h as f64 * 4.0;
    bytes / (gbps * PCIE_EFFICIENCY * 1e9) * 1000.0
}

// ---------------------------------------------------------------------------------------------
// Budget of one real frame.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Stage {
    /// copy | upscale | generate | compose
    pub stage: String,
    pub what: String,
    /// Time of one run of the stage (ms); None = not measured.
    pub ms: Option<f64>,
    /// Runs per real frame (generation runs once per generated frame).
    pub times: u32,
    /// true = measured on the card; false = calculated or estimated.
    pub measured: bool,
    /// The plan's goal for one run (ms), if it has one.
    pub goal_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Report {
    pub at: u64,
    pub gpu: String,
    pub pci: String,
    pub game_w: u32,
    pub game_h: u32,
    pub scale: u32,
    pub base_fps: u32,
    pub multiplier: u8,
    pub stages: Vec<Stage>,
    /// Time of one real frame the card has (1000 / base FPS).
    pub budget_ms: f64,
    /// Sum of the known stages for one real frame.
    pub total_ms: f64,
    /// Every stage known and the total within the budget.
    pub fits: bool,
    /// Highest base FPS the chain would hold (1000 / total).
    pub max_base_fps: f64,
    pub notes: Vec<String>,
}

/// Puts the stages together for one real frame at `base_fps` with `multiplier` shown frames.
pub fn budget(mut r: Report) -> Report {
    r.budget_ms = 1000.0 / r.base_fps.max(1) as f64;
    r.total_ms = r
        .stages
        .iter()
        .filter_map(|s| s.ms.map(|m| m * s.times as f64))
        .sum();
    let all_known = r.stages.iter().all(|s| s.ms.is_some() || s.times == 0);
    r.fits = all_known && r.total_ms <= r.budget_ms;
    r.max_base_fps = if r.total_ms > 0.0 {
        1000.0 / r.total_ms
    } else {
        0.0
    };
    r
}

/// Time of one item from two runs of a batch tool: the small run pays the start (Vulkan, model
/// loading); the difference divided by the extra items is the time per item.
pub fn per_item_ms(t_small: f64, n_small: u32, t_big: f64, n_big: u32) -> Option<f64> {
    if n_big <= n_small || t_big <= t_small {
        return None;
    }
    Some((t_big - t_small) * 1000.0 / (n_big - n_small) as f64)
}

// ---------------------------------------------------------------------------------------------
// Running the tools.

/// Environment that leaves only `g` visible to Vulkan (device 0).
pub fn card_env(g: &Gpu) -> Vec<(String, String)> {
    vec![
        ("DRI_PRIME".into(), g.dri_prime()),
        (
            "MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE".into(),
            "1".into(),
        ),
    ]
}

/// Makes `n` frames of w×h in `dir`: scaled from the pictures in `src` (game captures, used in
/// a loop) or, without them, ffmpeg's moving test picture.
pub fn make_frames(dir: &Path, n: u32, w: u32, h: u32, src: Option<&Path>) -> anyhow::Result<()> {
    use anyhow::Context;
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir)?;
    let scale = format!("scale={w}:{h}:flags=bicubic");
    let ok = |st: std::io::Result<std::process::Output>| -> anyhow::Result<()> {
        let o = st.context("ffmpeg")?;
        if !o.status.success() {
            anyhow::bail!("ffmpeg: {}", String::from_utf8_lossy(&o.stderr).trim());
        }
        Ok(())
    };
    match src {
        Some(s) => {
            let mut pics: Vec<PathBuf> = std::fs::read_dir(s)
                .with_context(|| s.display().to_string())?
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension().is_some_and(|x| {
                        let x = x.to_string_lossy().to_lowercase();
                        ["png", "jpg", "jpeg", "webp"].contains(&x.as_str())
                    })
                })
                .collect();
            pics.sort();
            if pics.is_empty() {
                anyhow::bail!(crate::tf!(
                    "no pictures (png/jpg/webp) in {}",
                    "nenhuma imagem (png/jpg/webp) em {}",
                    s.display()
                ));
            }
            for i in 0..n {
                let p = &pics[i as usize % pics.len()];
                ok(Command::new("ffmpeg")
                    .args(["-v", "error", "-y", "-i"])
                    .arg(p)
                    .args(["-vf", &scale, "-frames:v", "1"])
                    .arg(dir.join(format!("{:04}.png", i + 1)))
                    .output())?;
            }
        }
        None => ok(Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi", "-i"])
            .arg(format!("testsrc2=size={w}x{h}:rate=60"))
            .args(["-frames:v", &n.to_string()])
            .arg(dir.join("%04d.png"))
            .output())?,
    }
    Ok(())
}

/// Copies the first `n` frames of `from` into a fresh `to`.
fn subset(from: &Path, to: &Path, n: u32) -> std::io::Result<()> {
    let _ = std::fs::remove_dir_all(to);
    std::fs::create_dir_all(to)?;
    for i in 1..=n {
        let f = format!("{i:04}.png");
        std::fs::copy(from.join(&f), to.join(&f))?;
    }
    Ok(())
}

/// Runs one tool and returns how long it took (s).
fn timed(tool: &Tool, args: &[String], env: &[(String, String)]) -> anyhow::Result<f64> {
    let mut c = Command::new(&tool.bin);
    c.args(args);
    for (k, v) in env {
        c.env(k, v);
    }
    let t = Instant::now();
    let o = c.output()?;
    let secs = t.elapsed().as_secs_f64();
    let err = String::from_utf8_lossy(&o.stderr);
    // the ncnn tools print "vkCreateInstance failed" / "invalid gpu device" and still exit 0
    if !o.status.success()
        || err.contains("invalid gpu device")
        || err.contains("vkCreateInstance failed")
    {
        anyhow::bail!(
            "{}: {}",
            tool.bin.display(),
            err.lines().last().unwrap_or("").trim()
        );
    }
    Ok(secs)
}

/// What the run needs.
#[derive(Debug, Clone)]
pub struct Opts {
    pub game_w: u32,
    pub game_h: u32,
    pub scale: u32,
    pub base_fps: u32,
    pub multiplier: u8,
    /// Frames of the big run (the small one has 2).
    pub frames: u32,
    pub upscale_model: String,
    pub rife_model: String,
    /// Game captures to use instead of the test picture.
    pub pictures: Option<PathBuf>,
    /// The game's card, for the copy (None = the copy is left out).
    pub game_pci: Option<String>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            game_w: 1280,
            game_h: 720,
            scale: 2,
            base_fps: 60,
            multiplier: 2,
            frames: 24,
            upscale_model: "realesr-animevideov3".into(),
            rife_model: "rife-v4.6".into(),
            pictures: None,
            game_pci: None,
        }
    }
}

pub const UPSCALE_BIN: &str = "realesrgan-ncnn-vulkan";
pub const RIFE_BIN: &str = "rife-ncnn-vulkan";

/// Measures everything on `g` and puts the budget together.
pub fn run(g: &Gpu, o: &Opts, work: &Path) -> anyhow::Result<Report> {
    let env = card_env(g);
    let mut notes = vec![];
    let (ow, oh) = (o.game_w * o.scale, o.game_h * o.scale);
    let n = o.frames.max(4);
    let mut stages = vec![];

    // copy of the game's image from the game's card to this one
    let copy = o.game_pci.as_deref().and_then(|gp| {
        let a = path_gbps(gp)?;
        let b = path_gbps(&g.pci)?;
        Some(copy_ms(o.game_w, o.game_h, a.min(b)))
    });
    if o.game_pci.is_some() && copy.is_none() {
        notes.push(crate::tf!(
            "PCIe link of the cards not found in sysfs: copy left out",
            "link PCIe das placas não encontrado no sysfs: cópia fora da conta"
        ));
    }
    stages.push(Stage {
        stage: "copy".into(),
        what: crate::tf!(
            "{}×{} RGBA8 between the cards (calculated)",
            "{}×{} RGBA8 entre as placas (calculado)",
            o.game_w,
            o.game_h
        ),
        ms: copy,
        times: u32::from(o.game_pci.is_some()),
        measured: false,
        goal_ms: None,
    });

    // upscaling: frames at the game's resolution
    let small_in = work.join("game-2");
    let big_in = work.join("game");
    make_frames(&big_in, n, o.game_w, o.game_h, o.pictures.as_deref())?;
    subset(&big_in, &small_in, 2)?;
    let up_ms = match find_tool(UPSCALE_BIN, &o.upscale_model) {
        None => {
            notes.push(crate::tf!(
                "{} not installed (AUR: realesrgan-ncnn-vulkan): upscaling not measured",
                "{} não instalado (AUR: realesrgan-ncnn-vulkan): upscaling não medido",
                UPSCALE_BIN
            ));
            None
        }
        Some(t) => {
            let args = |i: &Path, out: &Path| {
                let mut a: Vec<String> = vec![
                    "-i".into(),
                    i.display().to_string(),
                    "-o".into(),
                    out.display().to_string(),
                    "-n".into(),
                    o.upscale_model.clone(),
                    "-s".into(),
                    o.scale.to_string(),
                    "-g".into(),
                    "0".into(),
                    "-f".into(),
                    "png".into(),
                ];
                if let Some(m) = &t.models {
                    a.extend(["-m".into(), m.display().to_string()]);
                }
                a
            };
            let (o1, o2) = (work.join("up-2"), work.join("up"));
            for d in [&o1, &o2] {
                let _ = std::fs::remove_dir_all(d);
                std::fs::create_dir_all(d)?;
            }
            let t1 = timed(&t, &args(&small_in, &o1), &env)?;
            let t2 = timed(&t, &args(&big_in, &o2), &env)?;
            per_item_ms(t1, 2, t2, n)
        }
    };
    stages.push(Stage {
        stage: "upscale".into(),
        what: format!(
            "{} {}× ({}×{} → {}×{})",
            o.upscale_model, o.scale, o.game_w, o.game_h, ow, oh
        ),
        ms: up_ms,
        times: 1,
        measured: up_ms.is_some(),
        goal_ms: Some(GOAL_UPSCALE_MS),
    });

    // frame generation: works on the shown resolution
    let gen_times = u32::from(o.multiplier.max(1)) - 1;
    let gen_ms = if gen_times == 0 {
        None
    } else {
        match find_tool(RIFE_BIN, &o.rife_model) {
            None => {
                notes.push(crate::tf!(
                    "{} not installed (AUR: rife-ncnn-vulkan): frame generation not measured",
                    "{} não instalado (AUR: rife-ncnn-vulkan): geração de quadros não medida",
                    RIFE_BIN
                ));
                None
            }
            Some(t) => {
                let big = work.join("shown");
                let small = work.join("shown-2");
                make_frames(&big, n, ow, oh, o.pictures.as_deref())?;
                subset(&big, &small, 2)?;
                let model = t
                    .models
                    .as_ref()
                    .map(|m| m.join(&o.rife_model).display().to_string())
                    .unwrap_or_else(|| o.rife_model.clone());
                let args = |i: &Path, out: &Path| -> Vec<String> {
                    vec![
                        "-i".into(),
                        i.display().to_string(),
                        "-o".into(),
                        out.display().to_string(),
                        "-m".into(),
                        model.clone(),
                        "-g".into(),
                        "0".into(),
                        "-f".into(),
                        "%08d.png".into(),
                    ]
                };
                let (o1, o2) = (work.join("gen-2"), work.join("gen"));
                for d in [&o1, &o2] {
                    let _ = std::fs::remove_dir_all(d);
                    std::fs::create_dir_all(d)?;
                }
                // a folder of k frames comes out with 2k: about k new frames each run
                let t1 = timed(&t, &args(&small, &o1), &env)?;
                let t2 = timed(&t, &args(&big, &o2), &env)?;
                per_item_ms(t1, 2, t2, n)
            }
        }
    };
    stages.push(Stage {
        stage: "generate".into(),
        what: format!("{} ({}×{})", o.rife_model, ow, oh),
        ms: gen_ms,
        times: gen_times,
        measured: gen_ms.is_some(),
        goal_ms: Some(GOAL_GENERATE_MS),
    });
    stages.push(Stage {
        stage: "compose".into(),
        what: crate::tf!(
            "composition and presentation (estimated)",
            "composição e apresentação (estimado)"
        ),
        ms: Some(COMPOSE_MS),
        times: u32::from(o.multiplier.max(1)),
        measured: false,
        goal_ms: Some(COMPOSE_MS),
    });
    notes.push(crate::tf!(
        "measured times include reading and writing PNGs on the processor: they are an upper bound",
        "os tempos medidos incluem ler e gravar PNG no processador: são um teto"
    ));
    Ok(budget(Report {
        at: crate::history::now(),
        gpu: g.alias.clone(),
        pci: g.pci.clone(),
        game_w: o.game_w,
        game_h: o.game_h,
        scale: o.scale,
        base_fps: o.base_fps,
        multiplier: o.multiplier.max(1),
        stages,
        budget_ms: 0.0,
        total_ms: 0.0,
        fits: false,
        max_base_fps: 0.0,
        notes,
    }))
}

// ---------------------------------------------------------------------------------------------
// Results.

pub fn results_path() -> PathBuf {
    std::env::var_os("NUMAPILOT_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(crate::tuner::user_state_dir)
        .join("present-bench.json")
}

/// Last report of each card (PCI → report).
pub fn load() -> BTreeMap<String, Report> {
    std::fs::read_to_string(results_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn store(r: &Report) -> std::io::Result<()> {
    let mut all = load();
    all.insert(r.pci.clone(), r.clone());
    let p = results_path();
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&all).unwrap_or_default())?;
    std::fs::rename(tmp, p)
}

/// The report as a table for the terminal.
pub fn render(r: &Report) -> String {
    let mut s = crate::tf!(
        "{} ({}): game {}×{} → {}×, {} FPS of base, {}× shown\n",
        "{} ({}): jogo {}×{} → {}×, {} FPS de base, {}× mostrado\n",
        r.gpu,
        r.pci,
        r.game_w,
        r.game_h,
        r.scale,
        r.base_fps,
        r.multiplier
    );
    for st in &r.stages {
        if st.times == 0 {
            continue;
        }
        let ms = match st.ms {
            Some(m) => format!("{m:7.2} ms"),
            None => format!("{:>10}", crate::t2!("?", "?")),
        };
        let goal = match (st.ms, st.goal_ms) {
            (Some(m), Some(g)) if st.measured => {
                if m <= g {
                    crate::tf!(
                        "  within the goal ({g:.0} ms)",
                        "  dentro da meta ({g:.0} ms)"
                    )
                } else {
                    crate::tf!(
                        "  above the goal ({g:.0} ms)",
                        "  acima da meta ({g:.0} ms)"
                    )
                }
            }
            _ => String::new(),
        };
        s.push_str(&format!(
            "  {:<9} {ms} ×{}  {}{goal}\n",
            st.stage, st.times, st.what
        ));
    }
    s.push_str(&crate::tf!(
        "  total {:.2} ms of {:.2} ms per real frame: {}\n",
        "  total {:.2} ms de {:.2} ms por quadro real: {}\n",
        r.total_ms,
        r.budget_ms,
        if r.fits {
            crate::t2!("fits", "cabe")
        } else if r.stages.iter().any(|s| s.ms.is_none() && s.times > 0) {
            crate::t2!("incomplete", "incompleto")
        } else {
            crate::t2!("does not fit", "não cabe")
        }
    ));
    if r.max_base_fps > 0.0 {
        s.push_str(&crate::tf!(
            "  this chain holds up to {:.0} FPS of base\n",
            "  esta cadeia aguenta até {:.0} FPS de base\n",
            r.max_base_fps
        ));
    }
    for n in &r.notes {
        s.push_str(&format!("  - {n}\n"));
    }
    s
}

// ---------------------------------------------------------------------------------------------
// Upscaling as Vulkan compute (M0b): no files, GPU timestamps.

#[derive(Debug, Clone, Serialize)]
pub struct ComputeModel {
    pub model: String,
    pub name: String,
    pub ms: Option<f64>,
    /// Slowest pass (name, ms).
    pub slowest: Option<(String, f64)>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComputeReport {
    pub gpu: String,
    pub pci: String,
    /// What Vulkan used (name, PCI when the driver tells it).
    pub vk_name: String,
    pub vk_pci: Option<String>,
    pub game_w: u32,
    pub game_h: u32,
    pub runs: u32,
    pub models: Vec<ComputeModel>,
}

/// Runs every model of `vkbench::MODELS` on `g` (only that card visible to Vulkan).
pub fn compute(g: &Gpu, w: u32, h: u32, runs: u32) -> anyhow::Result<ComputeReport> {
    for (k, v) in card_env(g) {
        // single-threaded at this point (the CLI, before any Vulkan call)
        unsafe { std::env::set_var(k, v) };
    }
    let ctx = crate::vkbench::Ctx::open()?;
    let vk_pci = ctx.pci();
    if let Some(p) = &vk_pci
        && !p.eq_ignore_ascii_case(&g.pci)
    {
        anyhow::bail!(crate::tf!(
            "Vulkan opened {} ({p}), not the {} ({}): the card filter did not apply",
            "o Vulkan abriu a {} ({p}), não a {} ({}): o filtro de placa não valeu",
            ctx.name,
            g.alias,
            g.pci
        ));
    }
    let mut models = vec![];
    for (id, name, src) in crate::vkbench::MODELS {
        let r = crate::vkbench::run(&ctx, src, w, h, runs, None, false);
        models.push(match r {
            Ok((t, _)) => ComputeModel {
                model: id.into(),
                name: name.into(),
                ms: Some(t.total_ms),
                slowest: t.passes.iter().max_by(|a, b| a.1.total_cmp(&b.1)).cloned(),
                error: None,
            },
            Err(e) => ComputeModel {
                model: id.into(),
                name: name.into(),
                ms: None,
                slowest: None,
                error: Some(format!("{e:#}")),
            },
        });
    }
    Ok(ComputeReport {
        gpu: g.alias.clone(),
        pci: g.pci.clone(),
        vk_name: ctx.name.clone(),
        vk_pci,
        game_w: w,
        game_h: h,
        runs,
        models,
    })
}

pub fn render_compute(r: &ComputeReport) -> String {
    let mut s = crate::tf!(
        "{} ({}, Vulkan: {}): {}×{} → {}×{}, median of {} runs, GPU time\n",
        "{} ({}, Vulkan: {}): {}×{} → {}×{}, mediana de {} rodadas, tempo da GPU\n",
        r.gpu,
        r.pci,
        r.vk_name,
        r.game_w,
        r.game_h,
        r.game_w * 2,
        r.game_h * 2,
        r.runs
    );
    for m in &r.models {
        match (m.ms, &m.error) {
            (Some(ms), _) => {
                let verdict = if ms <= GOAL_UPSCALE_MS {
                    crate::tf!(
                        "within the goal ({:.0} ms)",
                        "dentro da meta ({:.0} ms)",
                        GOAL_UPSCALE_MS
                    )
                } else {
                    crate::tf!(
                        "above the goal ({:.0} ms)",
                        "acima da meta ({:.0} ms)",
                        GOAL_UPSCALE_MS
                    )
                };
                s.push_str(&format!("  {:<20} {ms:8.2} ms  {verdict}\n", m.name));
                if let Some((p, pms)) = &m.slowest {
                    s.push_str(&crate::tf!(
                        "  {:<20} slowest pass: {} ({:.2} ms)\n",
                        "  {:<20} etapa mais lenta: {} ({:.2} ms)\n",
                        "",
                        p,
                        pms
                    ));
                }
            }
            (None, e) => s.push_str(&format!(
                "  {:<20} {}: {}\n",
                m.name,
                crate::t2!("failed", "falhou"),
                e.as_deref().unwrap_or("")
            )),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcie_links() {
        let g3 = link_gbps("8.0 GT/s PCIe", "16").unwrap();
        assert!((g3 - 15.75).abs() < 0.01, "{g3}");
        let g2 = link_gbps("5.0 GT/s PCIe", "16").unwrap();
        assert!((g2 - 8.0).abs() < 1e-9);
        assert!(link_gbps("Unknown", "0").is_none());
        // the plan's number: 1080p RGBA8 at ~12 GB/s real is ~0.7 ms
        let c = copy_ms(1920, 1080, g3);
        assert!(c > 0.6 && c < 0.75, "{c}");
    }

    #[test]
    fn narrowest_link_up_to_the_root() {
        let td = std::env::temp_dir().join(format!("np-fgbench-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&td);
        // root port (x16 gen3) → switch of the card (x16 gen3) → card (x4 gen3, slower)
        let root = td.join("devices/pci0000:80/0000:80:02.0");
        let sw = root.join("0000:81:00.0");
        let card = sw.join("0000:83:00.0");
        std::fs::create_dir_all(&card).unwrap();
        for (d, w) in [(&root, "16"), (&sw, "16"), (&card, "4")] {
            std::fs::write(d.join("current_link_speed"), "8.0 GT/s PCIe\n").unwrap();
            std::fs::write(d.join("current_link_width"), format!("{w}\n")).unwrap();
        }
        std::fs::create_dir_all(td.join("bus/pci/devices")).unwrap();
        std::os::unix::fs::symlink(&card, td.join("bus/pci/devices/0000:83:00.0")).unwrap();
        let _g = crate::util::test_sysfs_lock(&td);
        let g = path_gbps("0000:83:00.0").unwrap();
        assert!((g - 3.94).abs() < 0.01, "{g}");
        assert!(path_gbps("0000:99:00.0").is_none());
        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn budget_of_one_frame() {
        assert!((per_item_ms(1.0, 2, 1.22, 24).unwrap() - 10.0).abs() < 1e-9);
        assert_eq!(per_item_ms(1.0, 2, 0.9, 24), None);
        let st = |stage: &str, ms: Option<f64>, times: u32| Stage {
            stage: stage.into(),
            what: String::new(),
            ms,
            times,
            measured: ms.is_some(),
            goal_ms: None,
        };
        let base = Report {
            at: 0,
            gpu: "gfx906-1".into(),
            pci: "0000:83:00.0".into(),
            game_w: 1280,
            game_h: 720,
            scale: 2,
            base_fps: 60,
            multiplier: 2,
            stages: vec![
                st("copy", Some(0.3), 1),
                st("upscale", Some(5.0), 1),
                st("generate", Some(4.0), 1),
                st("compose", Some(1.0), 2),
            ],
            budget_ms: 0.0,
            total_ms: 0.0,
            fits: false,
            max_base_fps: 0.0,
            notes: vec![],
        };
        let r = budget(base.clone());
        assert!((r.total_ms - 11.3).abs() < 1e-9);
        assert!(r.fits);
        assert!((r.max_base_fps - 1000.0 / 11.3).abs() < 1e-9);
        assert!(render(&r).contains("fits"));
        let mut slow = base.clone();
        slow.stages[2].ms = Some(12.0);
        assert!(!budget(slow).fits);
        let mut unknown = base;
        unknown.stages[1].ms = None;
        let r = budget(unknown);
        assert!(!r.fits, "a stage not measured never fits");
        assert!(render(&r).contains("incomplete"));
    }
}
