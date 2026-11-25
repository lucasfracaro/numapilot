//! Small AI models that run on the processor (AI server tab, "CPU models"): OCR, speech to
//! text, embeddings and text to speech. Each saved entry is a button that starts one local
//! server (`packaging/ai-apps/server.py`) as a transient systemd user unit, with the chosen
//! socket, cores, hyper-threading and priority. Nothing starts by itself.
//!
//! Saved in ~/.config/gfx906-lab/ai-apps.toml; Python environments and the server script in
//! ~/.local/share/numapilot/ai-apps/.

use crate::labcfg::{cfg_dir, home};
use crate::topo::Topology;
use std::path::PathBuf;

pub const SERVER_PY: &str = include_str!("../packaging/ai-apps/server.py");

/// The models numapilot knows how to install and serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Kind {
    pub id: &'static str,
    pub label: &'static str,
    /// What it does (English; Portuguese pair in i18n_pt).
    pub about: &'static str,
    pub repo: &'static str,
    /// pip packages for its Python environment.
    pub pip: &'static [&'static str],
    /// Needs PyTorch (installed from the CPU-only index).
    pub torch: bool,
    pub port: u16,
    /// The Hugging Face page asks to accept its terms before downloading.
    pub gated: bool,
}

pub const KINDS: &[Kind] = &[
    Kind {
        id: "indra-ocr",
        label: "Indra OCR",
        about: "Text from photos, scans and PDFs",
        repo: "datalab-to/indra-ocr",
        pip: &["indra-ocr"],
        torch: false,
        port: 8091,
        gated: false,
    },
    Kind {
        id: "parakeet-redux",
        label: "Parakeet Redux",
        about: "Speech to text (Portuguese included)",
        repo: "moondream/parakeet-redux",
        pip: &["moondream>=2.4.0"],
        torch: false,
        port: 8092,
        gated: false,
    },
    Kind {
        id: "embeddinggemma",
        label: "EmbeddingGemma 2",
        about: "Text embeddings for search and sorting (OpenAI-compatible API)",
        repo: "google/embeddinggemma-2",
        pip: &[
            "sentence-transformers",
            "transformers",
            "torchvision",
            "pillow",
        ],
        torch: true,
        port: 8093,
        gated: false,
    },
    Kind {
        id: "pocket-tts",
        label: "Pocket TTS",
        about: "Text to speech (Portuguese voice \"rafael\")",
        repo: "kyutai/pocket-tts",
        pip: &["pocket-tts", "scipy"],
        torch: true,
        port: 8094,
        gated: false,
    },
];

pub fn kind(id: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.id == id)
}

/// One saved CPU model (one button).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AiApp {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub port: u16,
    /// NUMA node (socket) to run on; None = all of them.
    #[serde(default)]
    pub node: Option<usize>,
    /// Physical cores to use (0 = all of the chosen sockets).
    #[serde(default)]
    pub cores: u32,
    /// Also use the second thread of each core (hyper-threading).
    #[serde(default)]
    pub smt: bool,
    /// Priority as nice (0 normal … 19 lowest).
    #[serde(default)]
    pub nice: i32,
}

impl AiApp {
    pub fn new(k: &Kind) -> AiApp {
        AiApp {
            name: k.label.to_string(),
            kind: k.id.to_string(),
            port: k.port,
            node: None,
            cores: 4,
            smt: false,
            nice: 0,
        }
    }

    pub fn kind(&self) -> Option<&'static Kind> {
        kind(&self.kind)
    }

    /// systemd user unit name.
    pub fn unit(&self) -> String {
        let slug: String = self
            .name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        format!("numapilot-ai-{}", slug.trim_matches('-'))
    }

    /// Logical CPUs for the chosen socket, core count and hyper-threading.
    pub fn cpus(&self, topo: &Topology) -> Vec<usize> {
        let nodes: Vec<usize> = match self.node {
            Some(n) if topo.nodes.iter().any(|x| x.id == n) => vec![n],
            _ => topo.nodes.iter().map(|n| n.id).collect(),
        };
        let per_core = if self.smt { 2 } else { 1 };
        let count = (self.cores > 0).then(|| self.cores as usize * per_core);
        topo.select_cpus(&nodes, count, self.smt, &[])
    }

    /// Command that starts the server (transient user unit, never started at login).
    pub fn up_argv(&self, topo: &Topology) -> Vec<String> {
        let cpus = self.cpus(topo);
        let threads = cpus.len().max(1);
        let list = cpus
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(" ");
        let mut a: Vec<String> = vec![
            "systemd-run".into(),
            "--user".into(),
            format!("--unit={}", self.unit()),
            "--collect".into(),
            format!("--description=numapilot AI: {}", self.name),
            "-p".into(),
            format!("Nice={}", self.nice.clamp(0, 19)),
        ];
        if !list.is_empty() {
            a.push("-p".into());
            a.push(format!("CPUAffinity={list}"));
        }
        if let Some(n) = self.node {
            a.push("-p".into());
            a.push("NUMAPolicy=bind".into());
            a.push("-p".into());
            a.push(format!("NUMAMask={n}"));
        }
        for v in ["OMP_NUM_THREADS", "MKL_NUM_THREADS", "OPENBLAS_NUM_THREADS"] {
            a.push(format!("--setenv={v}={threads}"));
        }
        a.push(
            venv_dir(&self.kind)
                .join("bin/python")
                .to_string_lossy()
                .into(),
        );
        a.push(server_path().to_string_lossy().into());
        a.extend([
            "--kind".into(),
            self.kind.clone(),
            "--port".into(),
            self.port.to_string(),
            "--threads".into(),
            threads.to_string(),
        ]);
        a
    }

    pub fn down_argv(&self) -> Vec<String> {
        vec![
            "systemctl".into(),
            "--user".into(),
            "stop".into(),
            format!("{}.service", self.unit()),
        ]
    }

    pub fn log_argv(&self) -> Vec<String> {
        vec![
            "journalctl".into(),
            "--user".into(),
            "-u".into(),
            format!("{}.service", self.unit()),
            "-n".into(),
            "100".into(),
            "--no-pager".into(),
        ]
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

pub fn root() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".local/share"))
        .join("numapilot/ai-apps")
}

pub fn venv_dir(kind: &str) -> PathBuf {
    root().join(kind).join("venv")
}

pub fn server_path() -> PathBuf {
    root().join("server.py")
}

/// Writes the server script next to the environments (kept in sync with this binary).
pub fn write_server() -> std::io::Result<()> {
    std::fs::create_dir_all(root())?;
    let p = server_path();
    if std::fs::read_to_string(&p).ok().as_deref() != Some(SERVER_PY) {
        std::fs::write(p, SERVER_PY)?;
    }
    Ok(())
}

pub fn installed(kind: &str) -> bool {
    venv_dir(kind).join("bin/python").exists() && root().join(kind).join(".ready").exists()
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Shell script (run in a terminal) that creates or updates the model's Python environment and
/// downloads the model.
pub fn install_script(k: &Kind) -> String {
    let venv = venv_dir(k.id);
    let dir = root().join(k.id);
    let py = venv.join("bin/python");
    let mut s = String::from("set -e\n");
    s.push_str(&format!("mkdir -p {}\n", sh_quote(&dir.to_string_lossy())));
    s.push_str(&format!(
        "[ -x {py} ] || python3 -m venv {venv}\n",
        py = sh_quote(&py.to_string_lossy()),
        venv = sh_quote(&venv.to_string_lossy())
    ));
    s.push_str(&format!(
        "{} -m pip install -U pip wheel\n",
        sh_quote(&py.to_string_lossy())
    ));
    if k.torch {
        s.push_str(&format!(
            "{} -m pip install -U torch --index-url https://download.pytorch.org/whl/cpu\n",
            sh_quote(&py.to_string_lossy())
        ));
    }
    let pkgs: Vec<String> = k.pip.iter().map(|p| sh_quote(p)).collect();
    let extra = if k.torch {
        " --extra-index-url https://download.pytorch.org/whl/cpu"
    } else {
        ""
    };
    s.push_str(&format!(
        "{} -m pip install -U {}{extra}\n",
        sh_quote(&py.to_string_lossy()),
        pkgs.join(" ")
    ));
    if k.gated {
        s.push_str(&format!(
            "echo; echo 'This model needs its terms accepted at https://huggingface.co/{} and `hf auth login` (or HF_TOKEN).'\n",
            k.repo
        ));
    }
    s.push_str(&format!(
        "{} {} --kind {} --download\n",
        sh_quote(&py.to_string_lossy()),
        sh_quote(&server_path().to_string_lossy()),
        k.id
    ));
    s.push_str(&format!(
        "touch {}\necho; echo 'Ready: {} installed.'\n",
        sh_quote(&dir.join(".ready").to_string_lossy()),
        k.label
    ));
    s
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct AppFile {
    #[serde(default, rename = "app")]
    apps: Vec<AiApp>,
}

pub fn apps_path() -> PathBuf {
    cfg_dir().join("ai-apps.toml")
}

pub fn apps_from_str(s: &str) -> Vec<AiApp> {
    toml::from_str::<AppFile>(s)
        .map(|f| f.apps)
        .unwrap_or_default()
        .into_iter()
        .filter(|a| !a.name.is_empty() && kind(&a.kind).is_some())
        .map(|mut a| {
            if a.port == 0 {
                a.port = kind(&a.kind).map(|k| k.port).unwrap_or(8090);
            }
            a
        })
        .collect()
}

pub fn apps_to_string(list: &[AiApp]) -> String {
    format!(
        "# CPU AI models (numapilot, AI server tab).\n{}",
        toml::to_string(&AppFile {
            apps: list.to_vec()
        })
        .unwrap_or_default()
    )
}

pub fn load_apps() -> Vec<AiApp> {
    std::fs::read_to_string(apps_path())
        .map(|s| apps_from_str(&s))
        .unwrap_or_default()
}

pub fn save_apps(list: &[AiApp]) -> std::io::Result<()> {
    std::fs::create_dir_all(cfg_dir())?;
    std::fs::write(apps_path(), apps_to_string(list))
}

/// Running units among the saved ones (one `systemctl` call).
pub fn running(list: &[AiApp]) -> Vec<bool> {
    if list.is_empty() {
        return Vec::new();
    }
    let units: Vec<String> = list
        .iter()
        .map(|a| format!("{}.service", a.unit()))
        .collect();
    let out = std::process::Command::new("systemctl")
        .args(["--user", "is-active"])
        .args(&units)
        .output();
    let text = out
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let mut states = text
        .lines()
        .map(|l| l.trim() == "active" || l.trim() == "activating");
    list.iter()
        .map(|_| states.next().unwrap_or(false))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topo::Node;
    use std::collections::BTreeSet;

    fn two_sockets() -> Topology {
        let node = |id: usize, base: usize| Node {
            id,
            cpus: (0..8).map(|i| base + i).collect(),
            cores: (0..4).map(|i| vec![base + i, base + 4 + i]).collect(),
            physical_cores: 4,
            sockets: BTreeSet::from([id as u32]),
            mem_total_kb: 0,
            distances: vec![10, 20],
        };
        Topology {
            nodes: vec![node(0, 0), node(1, 8)],
            gpus: vec![],
            default_graphics: None,
        }
    }

    #[test]
    fn cpus_follow_socket_cores_and_smt() {
        let t = two_sockets();
        let mut a = AiApp::new(kind("pocket-tts").unwrap());
        a.node = Some(1);
        a.cores = 2;
        assert_eq!(a.cpus(&t).len(), 2);
        assert!(a.cpus(&t).iter().all(|c| (8..16).contains(c)));
        a.smt = true;
        let c = a.cpus(&t);
        assert_eq!(c.len(), 4);
        a.cores = 0;
        a.smt = false;
        a.node = None;
        assert_eq!(a.cpus(&t).len(), 8);
    }

    #[test]
    fn up_command_has_limits_and_no_autostart() {
        let t = two_sockets();
        let mut a = AiApp::new(kind("indra-ocr").unwrap());
        a.name = "My OCR!".into();
        a.node = Some(0);
        a.cores = 3;
        a.nice = 25;
        let v = a.up_argv(&t);
        assert_eq!(v[0], "systemd-run");
        assert!(v.contains(&"--unit=numapilot-ai-my-ocr".to_string()));
        assert!(v.contains(&"Nice=19".to_string()));
        assert!(v.contains(&"NUMAMask=0".to_string()));
        assert!(v.contains(&"--setenv=OMP_NUM_THREADS=3".to_string()));
        assert!(v.iter().any(|x| x.starts_with("CPUAffinity=")));
        assert!(!v.iter().any(|x| x.contains("enable")));
        assert_eq!(v.last().unwrap(), "3");
    }

    #[test]
    fn apps_round_trip_and_unknown_kinds_dropped() {
        let mut a = AiApp::new(kind("embeddinggemma").unwrap());
        a.node = Some(1);
        let s = apps_to_string(&[a.clone()]);
        assert_eq!(apps_from_str(&s), vec![a]);
        let t =
            "[[app]]\nname = \"x\"\nkind = \"nope\"\n[[app]]\nname = \"y\"\nkind = \"indra-ocr\"\n";
        let l = apps_from_str(t);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].port, 8091);
    }

    #[test]
    fn install_script_quotes_and_marks_ready() {
        let s = install_script(kind("pocket-tts").unwrap());
        assert!(s.contains("download.pytorch.org/whl/cpu"));
        assert!(s.contains("'pocket-tts' 'scipy'"));
        assert!(s.contains("--download"));
        assert!(s.contains(".ready"));
        assert!(!s.contains("auth login"));
    }
}
