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

/// Upscaling models: (value, name, ready). The planned ones fall back to FSR 1 for now.
pub const UPSCALERS: [(&str, &str, bool); 5] = [
    ("none", "none (output at the game's resolution)", true),
    ("fsr1", "FSR 1 (gamescope)", true),
    ("nis", "NIS (gamescope)", true),
    ("anime4k", "Anime4K (planned)", false),
    ("srvgg", "SRVGGNetCompact (planned)", false),
];

/// Frame generators: (value, name, ready). Lossless Scaling runs inside the game (Vulkan layer);
/// block matching and RIFE are the chain on the output card (planned).
pub const ENGINES: [(&str, &str, bool); 3] = [
    ("lsfg", "Lossless Scaling (lsfg-vk)", true),
    ("blockmatch", "block matching (planned)", false),
    ("rife", "RIFE v4 lite (planned)", false),
];

pub fn upscaler_ready(v: &str) -> bool {
    UPSCALERS.iter().any(|u| u.0 == v && u.2)
}

pub fn engine_ready(v: &str) -> bool {
    ENGINES.iter().any(|e| e.0 == v && e.2)
}

/// Is the lsfg-vk Vulkan layer installed (system or user)?
pub fn lsfg_installed() -> bool {
    let mut dirs = vec![
        PathBuf::from("/usr/share/vulkan/implicit_layer.d"),
        PathBuf::from("/etc/vulkan/implicit_layer.d"),
    ];
    if let Some(h) = std::env::var_os("HOME") {
        dirs.push(PathBuf::from(h).join(".local/share/vulkan/implicit_layer.d"));
    }
    dirs.iter().any(|d| {
        std::fs::read_dir(d).is_ok_and(|r| {
            r.flatten().any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("lsfg")
            })
        })
    })
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
    /// Variables only for the game (frame generation card, MangoHud): they go after "--".
    pub game_env: Vec<(String, String)>,
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
pub fn plan(pr: &Present, gs: &Gamescope, out: Option<&Gpu>, lsfg: bool) -> Plan {
    let mut warnings = vec![];
    let mut g = gs.clone();
    g.fsr = false;
    let mut a = g.argv();
    let up = if upscaler_ready(&pr.upscaler) {
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
    let mut game_env = vec![];
    let mut multiplier = pr.multiplier.max(1);
    if multiplier > 1 {
        if !engine_ready(&pr.engine) {
            warnings.push(crate::tf!(
                "frame generator '{}' is not ready yet: no frame generation",
                "o gerador de quadros '{}' ainda não está pronto: sem geração de quadros",
                pr.engine
            ));
            multiplier = 1;
        } else if pr.engine == "lsfg" && !lsfg {
            warnings.push(crate::i18n::tr("lsfg-vk is not installed: no frame generation").into());
            multiplier = 1;
        } else if pr.engine == "lsfg" {
            // lsfg-vk generates inside the game's process; on the output card it needs a version
            // with dual GPU. The multiplier itself is set in lsfg-vk's own settings.
            if let Some(o) = out {
                set(
                    &mut game_env,
                    "LSFGVK_GPU",
                    crate::decide::pci_decimal(&o.pci),
                );
                set(&mut game_env, "NUMAPILOT_FG_GPU", o.pci.clone());
                warnings.push(crate::tf!(
                    "frame generation (lsfg-vk) on the {}: requires lsfg-vk with dual GPU",
                    "frame generation (lsfg-vk) na {}: requer lsfg-vk com dual GPU",
                    o.alias
                ));
            }
            warnings.push(crate::tf!(
                "lsfg-vk takes the multiplier ({}×) and its own minimum from its settings (lsfg-vk-ui)",
                "o lsfg-vk usa o multiplicador ({}×) e o próprio mínimo das configurações dele (lsfg-vk-ui)",
                multiplier
            ));
        }
    }
    Plan {
        gamescope: a,
        out_pci: out.map(|o| o.pci.clone()),
        game_env,
        multiplier,
        warnings,
        measuring: false,
    }
}

fn set(env: &mut Vec<(String, String)>, k: &str, v: String) {
    env.retain(|(x, _)| x != k);
    env.push((k.into(), v));
}

// ---------------------------------------------------------------------------------------------
// Measurement: real FPS (MangoHud) against shown FPS.

/// Where MangoHud writes the CSVs of the presentation measurement for one program.
pub fn log_dir(key: &str) -> PathBuf {
    crate::tuner::user_state_dir()
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
    crate::tuner::user_state_dir().join("present.json")
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
        let p = plan(&pr, &gs, Some(&b), true);
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
        assert_eq!(
            p.game_prefix(child),
            [
                "env",
                "DRI_PRIME=pci-0000_03_00_0",
                "LSFGVK_GPU=131:0.0",
                "NUMAPILOT_FG_GPU=0000:83:00.0"
            ]
        );
    }

    #[test]
    fn same_card_and_planned_parts() {
        let pr = Present {
            upscaler: "anime4k".into(),
            multiplier: 2,
            engine: "rife".into(),
            ..Default::default()
        };
        let mut p = plan(&pr, &Gamescope::default(), None, true);
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
            false,
        );
        assert_eq!(p.multiplier, 1, "without lsfg-vk there is no generation");
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
