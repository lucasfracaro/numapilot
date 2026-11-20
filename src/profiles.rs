//! Per-program profiles chosen in the graphical interface (or through `numapilot profile`).
//! They live in /var/lib/numapilot/profiles.json (written by the daemon, readable by everyone).

use crate::learned::state_dir;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn auto() -> String {
    "auto".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    /// Friendly name shown in the interface (e.g. "Cyberpunk2077.exe").
    #[serde(default)]
    pub label: String,
    /// "auto" | "gpu" | "0" | "1" | "all" ...
    #[serde(default = "auto")]
    pub cpu: String,
    /// "auto" | "display" | GPU alias/PCI
    #[serde(default = "auto")]
    pub gpu: String,
    /// "auto" | "graphics" | "compute": how to use the chosen GPU.
    #[serde(default = "auto", rename = "use")]
    pub usage: String,
    /// How many logical CPUs the program may use (None = all of the chosen sockets').
    #[serde(default)]
    pub cores: Option<usize>,
    /// May it use both threads of each core (hyper-threading)?
    #[serde(default = "yes")]
    pub smt: bool,
    /// CPUs picked by hand on the map (e.g. "22-29,66-73"); takes priority over sockets/cores.
    #[serde(default)]
    pub cpu_list: Option<String>,
    /// Memory: "auto" | "strict" (only from the cores' socket(s)) | "prefer" | "interleave" | "free"
    #[serde(default = "auto")]
    pub memory: String,
    /// Memory limit in GB.
    #[serde(default)]
    pub mem_max_gb: Option<f64>,
    /// "low" | "normal" | "high"
    #[serde(default = "normal")]
    pub priority: String,
    /// Reserves the cores for this program alone while it runs.
    #[serde(default)]
    pub exclusive: bool,
    /// Makes the program "see" only the allowed cores (Proton/Wine, OpenMP, MKL, .NET, Java...).
    #[serde(default = "yes")]
    pub limit_visible: bool,
    /// While it runs: its cores without the deep idle states and uncore at maximum.
    #[serde(default)]
    pub low_latency: bool,
    /// GPU power profile while it runs: "auto" (leave as is) | "3d" | "compute" | "vr" | "video" | "economia"
    #[serde(default = "auto")]
    pub gpu_power: String,
    /// Starts the game inside gamescope (internal resolution, FSR, FPS limit).
    #[serde(default)]
    pub gamescope: Option<Gamescope>,
    /// Game upscaling (FSR 4/DLSS/XeSS through Proton, or OptiScaler).
    #[serde(default)]
    pub upscale: Option<Upscale>,
    /// Presentation: which card shows the image and how the finished image is improved there
    /// (upscaling model, frame generation). Opens the game inside gamescope.
    #[serde(default)]
    pub present: Option<Present>,
    /// Machine mode while the program is open (jogo, ia, silencioso); goes back when it closes.
    #[serde(default)]
    pub machine_mode: Option<String>,
    /// Processor limit in % of one core (50 = half a core, 200 = two cores).
    #[serde(default)]
    pub cpu_max_pct: Option<f64>,
    /// Maximum turbo: while it runs, the rest of the system leaves the whole socket of its cores,
    /// so the other cores sleep and the clock rises (only on PCs with more than one processor).
    #[serde(default)]
    pub turbo_max: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Upscale {
    /// nenhum | fsr4 | fsr4-rdna3 | fsr3 | dlss | xess | optiscaler
    pub mode: String,
    /// Shows the FSR 4/DLSS watermark (to check that it took effect).
    pub indicator: bool,
    /// OptiScaler: forces FSR 4 INT8 on cards without official support (experimental).
    pub int8: bool,
}

/// "Render on one card, improve on the other": the game renders on the profile's card; the output
/// card (`Gamescope::output_gpu`) takes the finished image (one copy over PCIe), upscales it,
/// generates frames and shows it through its own video output. Works only with the final image (like Lossless Scaling), so the
/// real FPS and the input delay stay the game's. See `crate::present`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Present {
    /// Upscaling of the finished image: none | fsr1 | nis | anime4k | srvgg
    /// (`present::UPSCALERS` says which are ready).
    pub upscaler: String,
    /// Frames shown per real frame: 1 = no frame generation, 2 = 2×, 3 = 3×.
    pub multiplier: u8,
    /// Frame generator: lsfg | blockmatch | rife (`present::ENGINES`).
    pub engine: String,
    /// Base FPS under which frame generation turns off (below ~40 it looks bad).
    pub min_fps: u32,
    /// Measures real FPS (MangoHud in the game) against shown FPS on every start.
    pub measure: bool,
}

impl Default for Present {
    fn default() -> Self {
        Present {
            upscaler: "fsr1".into(),
            multiplier: 1,
            engine: "lsfg".into(),
            min_fps: 40,
            measure: true,
        }
    }
}

impl Present {
    pub fn validate(&self) -> Result<(), String> {
        if !crate::present::UPSCALERS
            .iter()
            .any(|u| u.0 == self.upscaler)
        {
            return Err(crate::tf!(
                "invalid upscaling model: {}",
                "modelo de upscaling inválido: {}",
                self.upscaler
            ));
        }
        if !crate::present::ENGINES.iter().any(|e| e.0 == self.engine) {
            return Err(crate::tf!(
                "invalid frame generator: {}",
                "gerador de quadros inválido: {}",
                self.engine
            ));
        }
        if !(1..=4).contains(&self.multiplier) {
            return Err(crate::i18n::tr("frame generation: from 1× (off) to 4×").into());
        }
        if !(10..=240).contains(&self.min_fps) {
            return Err(crate::i18n::tr("minimum base FPS: from 10 to 240").into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Gamescope {
    /// Resolution the game renders at (None = the display's).
    #[serde(default)]
    pub render_w: Option<u32>,
    #[serde(default)]
    pub render_h: Option<u32>,
    /// Output resolution (None = the display's).
    #[serde(default)]
    pub out_w: Option<u32>,
    #[serde(default)]
    pub out_h: Option<u32>,
    #[serde(default = "yes")]
    pub fullscreen: bool,
    /// Upscales with FSR 1 when the internal resolution is lower.
    #[serde(default)]
    pub fsr: bool,
    #[serde(default)]
    pub fps_limit: Option<u32>,
    /// Card that runs gamescope (upscaling, composition, output): None = the game's card.
    /// Alias/PCI, or "display" (the card with a monitor). The game keeps its own card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_gpu: Option<String>,
}

impl Default for Gamescope {
    fn default() -> Self {
        Gamescope {
            render_w: None,
            render_h: None,
            out_w: None,
            out_h: None,
            fullscreen: true,
            fsr: false,
            fps_limit: None,
            output_gpu: None,
        }
    }
}

impl Gamescope {
    /// gamescope arguments (without the final "--").
    pub fn argv(&self) -> Vec<String> {
        let mut a = vec!["gamescope".to_string()];
        let mut push = |f: &str, v: Option<u32>| {
            if let Some(v) = v {
                a.push(f.into());
                a.push(v.to_string());
            }
        };
        push("-w", self.render_w);
        push("-h", self.render_h);
        push("-W", self.out_w);
        push("-H", self.out_h);
        push("-r", self.fps_limit);
        if self.fullscreen {
            a.push("-f".into());
        }
        if self.fsr {
            a.push("-F".into());
            a.push("fsr".into());
        }
        a
    }
}

fn yes() -> bool {
    true
}
fn normal() -> String {
    "normal".into()
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            label: String::new(),
            cpu: auto(),
            gpu: auto(),
            usage: auto(),
            cores: None,
            smt: true,
            cpu_list: None,
            memory: auto(),
            mem_max_gb: None,
            priority: normal(),
            exclusive: false,
            limit_visible: true,
            low_latency: false,
            gpu_power: auto(),
            gamescope: None,
            upscale: None,
            present: None,
            machine_mode: None,
            cpu_max_pct: None,
            turbo_max: false,
        }
    }
}

impl Profile {
    pub fn is_auto(&self) -> bool {
        let d = Profile {
            label: self.label.clone(),
            ..Default::default()
        };
        *self == d
    }

    /// Number of CPUs to use. With a sched_ext scheduler (LAVD...) the kernel does not enforce
    /// the processor limit (cpu.max), so it becomes a number of cores, rounded up: 150% → 2.
    pub fn cores_for(&self, sched_ext: bool) -> Option<usize> {
        let quota = self
            .cpu_max_pct
            .filter(|_| sched_ext)
            .map(|p| ((p / 100.0).ceil() as usize).max(1));
        match (self.cores, quota) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, None) => a,
            (None, b) => b,
        }
    }

    /// Hyper-threading for the chosen cores: off when the processor limit decides the count, so
    /// "2 cores" are 2 physical cores and not the 2 threads of one.
    pub fn smt_for(&self, sched_ext: bool) -> bool {
        self.smt && self.cores_for(sched_ext) == self.cores
    }

    /// Picks some of the cores by itself (count, no HT, or a processor limit as cores), so the
    /// measured cache order matters. A list picked by hand does not use the order.
    pub fn picks_subset(&self, sched_ext: bool) -> bool {
        self.cpu_list.is_none() && (self.cores_for(sched_ext).is_some() || !self.smt)
    }

    /// Anything in the CPU/memory part that is not automatic?
    pub fn has_cpu_choice(&self) -> bool {
        self.cpu != "auto" || self.cores.is_some() || !self.smt || self.cpu_list.is_some()
    }

    /// Needs a systemd control group (cgroup) to take effect.
    pub fn needs_scope(&self) -> bool {
        self.memory == "strict"
            || self.mem_max_gb.is_some()
            || self.exclusive
            || self.cpu_max_pct.is_some()
            || self.turbo_max
    }

    /// Needs the service (root) to apply: cgroup or high priority.
    pub fn needs_daemon_hw(&self) -> bool {
        self.low_latency || self.gpu_power != "auto"
    }

    pub fn needs_daemon(&self) -> bool {
        self.needs_scope() || self.priority == "high"
    }

    /// Something that only reaches the program when it starts through `numapilot run`: the
    /// graphics card, what it sees, gamescope or the upscaler.
    /// A program opened directly from its own shortcut misses these.
    pub fn needs_launcher(&self) -> bool {
        self.gpu != "auto"
            || self.gamescope.is_some()
            || self
                .upscale
                .as_ref()
                .is_some_and(|u| u.mode != "nenhum" && !u.mode.is_empty())
            || (self.limit_visible && self.has_cpu_choice())
    }

    pub fn validate(&self) -> Result<(), String> {
        if !["auto", "strict", "prefer", "interleave", "free"].contains(&self.memory.as_str()) {
            return Err(crate::tf!(
                "invalid memory policy: {}",
                "política de memória inválida: {}",
                self.memory
            ));
        }
        if !["low", "normal", "high"].contains(&self.priority.as_str()) {
            return Err(crate::tf!(
                "invalid priority: {}",
                "prioridade inválida: {}",
                self.priority
            ));
        }
        if !["auto", "3d", "compute", "vr", "video", "economia"].contains(&self.gpu_power.as_str())
        {
            return Err(crate::tf!(
                "invalid GPU power profile: {}",
                "perfil de energia da GPU inválido: {}",
                self.gpu_power
            ));
        }
        if self.cores == Some(0) {
            return Err(crate::i18n::tr("cores: at least 1").into());
        }
        if let Some(m) = self.mem_max_gb
            && m < 0.25
        {
            return Err(crate::i18n::tr("memory limit too low (minimum 0.25 GB)").into());
        }
        if let Some(c) = self.cpu_max_pct
            && !(5.0..=6400.0).contains(&c)
        {
            return Err(
                crate::i18n::tr("processor limit: from 5% to 6400% (100% = one core)").into(),
            );
        }
        if let Some(l) = &self.cpu_list
            && crate::util::parse_cpulist(l).is_empty()
        {
            return Err(crate::tf!(
                "invalid CPU list: {l}",
                "lista de CPUs inválida: {l}"
            ));
        }
        if let Some(pr) = &self.present {
            pr.validate()?;
        }
        Ok(())
    }

    /// One-click ready-made profiles.
    pub fn preset(name: &str, base: &Profile) -> Profile {
        let mut p = Profile {
            label: base.label.clone(),
            gpu: base.gpu.clone(),
            usage: base.usage.clone(),
            gamescope: base.gamescope.clone(),
            upscale: base.upscale.clone(),
            present: base.present.clone(),
            machine_mode: base.machine_mode.clone(),
            ..Default::default()
        };
        match name {
            // Game: GPU's socket, physical cores (no core sharing), local memory only,
            // high priority and reserved cores.
            "jogo" => {
                p.cpu = "gpu".into();
                p.cores = Some(16);
                p.smt = false;
                p.memory = "strict".into();
                p.priority = "high".into();
                p.exclusive = true;
                p.low_latency = true;
                p.gpu_power = "3d".into();
                if p.usage == "auto" {
                    p.usage = "graphics".into();
                }
            }
            // AI: socket(s) of the compute GPU(s), memory preferring local.
            "ia" => {
                p.cpu = "gpu".into();
                p.memory = "prefer".into();
                p.gpu_power = "compute".into();
                if p.usage == "auto" {
                    p.usage = "compute".into();
                }
            }
            // Rendering/compiling: everything, interleaved memory (bandwidth of both sockets).
            "render" => {
                p.cpu = "all".into();
                p.memory = "interleave".into();
            }
            // Maximum turbo: 4 physical cores of the GPU's socket, alone on the processor
            // (the rest of the system goes to the other socket), so the clock reaches the highest step.
            "turbo" => {
                p.cpu = "gpu".into();
                p.cores = Some(4);
                p.smt = false;
                p.memory = "strict".into();
                p.priority = "high".into();
                p.exclusive = true;
                p.turbo_max = true;
                p.gpu_power = "3d".into();
                if p.usage == "auto" {
                    p.usage = "graphics".into();
                }
            }
            // Video editing: a whole socket with both threads, memory preferring local.
            "video" => {
                p.cpu = "gpu".into();
                p.memory = "prefer".into();
                p.gpu_power = "video".into();
            }
            // Background: at most one core, low priority (sync tools, launchers...).
            "fundo" => {
                p.priority = "low".into();
                p.cpu_max_pct = Some(100.0);
            }
            // Quiet: few cores, low priority (stays out of the way).
            "silencioso" => {
                p.cores = Some(4);
                p.priority = "low".into();
            }
            _ => {}
        }
        p
    }
}

#[derive(Debug, Default, Clone)]
pub struct Profiles {
    pub map: BTreeMap<String, Profile>,
}

impl Profiles {
    pub fn path() -> PathBuf {
        state_dir().join("profiles.json")
    }

    pub fn load() -> Profiles {
        let map = std::fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str::<BTreeMap<String, Profile>>(&s).ok())
            .unwrap_or_default();
        // profiles saved before keys dropped versions (`stable_key`) keep working
        let map = map
            .into_iter()
            .map(|(k, p)| (crate::procfs::stable_key(&k), p))
            .collect();
        Profiles { map }
    }

    pub fn get(&self, key: &str) -> Option<&Profile> {
        self.map
            .get(key)
            .or_else(|| self.map.get(&crate::procfs::stable_key(key)))
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = state_dir();
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join("profiles.json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_string_pretty(&self.map).unwrap_or_default(),
        )?;
        std::fs::rename(tmp, Self::path())
    }
}

/// Program key: Steam games use the AppId (all of the game's processes
/// stay together); the rest use the learning key (executable / script).
pub fn program_key(steam_appid: Option<&str>, learn_key: &str) -> String {
    match steam_appid {
        Some(id) if !id.is_empty() && id != "0" => format!("steam:{id}"),
        _ => learn_key.to_string(),
    }
}

/// File to exchange choices between PCs or with other people.
#[derive(Debug, Serialize, Deserialize)]
pub struct Exported {
    /// format version
    pub formato: u32,
    #[serde(default)]
    pub maquina: String,
    pub perfis: BTreeMap<String, Profile>,
}

/// Where the export goes by default: ~/numapilot-escolhas-MACHINE.json
pub fn export_path() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into())).join(format!(
        "numapilot-escolhas-{}.json",
        crate::selftest::hostname()
    ))
}

/// Text of the export file with all saved choices.
pub fn export_text() -> String {
    let e = Exported {
        formato: 1,
        maquina: crate::selftest::hostname(),
        perfis: Profiles::load().map,
    };
    serde_json::to_string_pretty(&e).unwrap_or_default()
}

/// Reads an export file and returns only the valid profiles and the errors of the others.
/// Choices tied to another machine's cores or cards are adapted: the CPU list is dropped
/// (the cores are different) and the card goes back to "auto" if the alias does not exist here.
pub fn import_text(
    text: &str,
    gpu_aliases: &[String],
) -> Result<(Vec<(String, Profile)>, Vec<String>), String> {
    let e: Exported = serde_json::from_str(text)
        .map_err(|e| crate::tf!("invalid file: {e}", "arquivo inválido: {e}"))?;
    if e.formato != 1 {
        return Err(crate::tf!(
            "unknown format {} (update numapilot)",
            "formato {} desconhecido (atualize o numapilot)",
            e.formato
        ));
    }
    let this = crate::selftest::hostname();
    let mut ok = vec![];
    let mut bad = vec![];
    for (k, mut p) in e.perfis {
        if e.maquina != this {
            p.cpu_list = None;
            if !["auto", "display"].contains(&p.gpu.as_str()) && !gpu_aliases.contains(&p.gpu) {
                p.gpu = auto();
            }
            if let Some(g) = p.gamescope.as_mut()
                && g.output_gpu
                    .as_ref()
                    .is_some_and(|o| o != "display" && !gpu_aliases.contains(o))
            {
                g.output_gpu = None;
            }
        }
        match p.validate() {
            Ok(()) if !k.is_empty() => ok.push((k, p)),
            Ok(()) => bad.push(crate::i18n::tr("profile without a key").into()),
            Err(err) => bad.push(format!("{k}: {err}")),
        }
    }
    Ok((ok, bad))
}

#[cfg(test)]
mod export_tests {
    use super::*;

    #[test]
    fn import_adapts_other_machine() {
        let mut perfis = BTreeMap::new();
        perfis.insert(
            "steam:1091500".to_string(),
            Profile {
                gpu: "amd-7".into(),
                cpu_list: Some("0-3".into()),
                cores: Some(4),
                ..Default::default()
            },
        );
        perfis.insert(
            "/usr/bin/x".to_string(),
            Profile {
                cores: Some(0),
                ..Default::default()
            },
        );
        let t = serde_json::to_string(&Exported {
            formato: 1,
            maquina: "outro-pc-qualquer".into(),
            perfis,
        })
        .unwrap();
        let (ok, bad) = import_text(&t, &["amd-0".to_string()]).unwrap();
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].1.gpu, "auto");
        assert_eq!(ok[0].1.cpu_list, None);
        assert_eq!(ok[0].1.cores, Some(4));
        assert_eq!(bad.len(), 1);
        assert!(import_text("{}", &[]).is_err());
    }
}

#[cfg(test)]
mod quota_tests {
    use super::Profile;

    #[test]
    fn cpu_limit_as_cores_under_sched_ext() {
        let mut p = Profile {
            cpu_max_pct: Some(150.0),
            ..Default::default()
        };
        assert_eq!(p.cores_for(false), None);
        assert_eq!(p.cores_for(true), Some(2));
        p.cpu_max_pct = Some(30.0);
        assert_eq!(p.cores_for(true), Some(1));
        p.cores = Some(8);
        p.cpu_max_pct = Some(400.0);
        assert_eq!(p.cores_for(true), Some(4));
        assert_eq!(p.cores_for(false), Some(8));
        assert!(!p.smt_for(true), "the limit decides: physical cores");
        assert!(p.smt_for(false));
        p.cpu_max_pct = None;
        assert_eq!(p.cores_for(true), Some(8));
        assert!(p.smt_for(true));
    }

    #[test]
    fn subset_needs_the_cache_order() {
        let p = Profile::default();
        assert!(!p.picks_subset(true), "automatic: all cores");
        let p = Profile {
            cores: Some(4),
            ..Default::default()
        };
        assert!(p.picks_subset(false));
        let p = Profile {
            smt: false,
            ..Default::default()
        };
        assert!(
            p.picks_subset(false),
            "one thread per core is a choice of cores too"
        );
        let p = Profile {
            cpu_max_pct: Some(150.0),
            ..Default::default()
        };
        assert!(p.picks_subset(true) && !p.picks_subset(false));
        let p = Profile {
            cores: Some(4),
            cpu_list: Some("2-5".into()),
            ..Default::default()
        };
        assert!(
            !p.picks_subset(true),
            "a list picked by hand does not use the order"
        );
    }
}
