//! Frame generators of other projects that numapilot can turn on per game, the way GOverlay does:
//! Vulkan layers installed on the machine and driven only through the game's environment.
//!
//! - MAKO Renderer (github.com/eugeniosegala/MAKO, GPL-3.0-or-later): descends from lsfg-vk 2 and
//!   brings its own open frame generation model (MAKO FG, no Lossless Scaling needed), 2–5×.
//!   Its `mako-launch` wrapper orders the Vulkan layers (MangoHud included) and keeps lsfg-vk out.
//! - afmf-linux (github.com/serialexperimentslainnnn/afmf-linux, MIT): "AMD Fluid Motion Frames"
//!   style 2× from the colour buffer, with AMD's FidelityFX optical flow; settings by environment.
//!
//! numapilot ships none of them and copies no code from them or from GOverlay (GPL-3): it only
//! finds their layer manifests and sets the documented variables. They are installed by GOverlay
//! (Frame Generation tab), by their own installers or from the AUR.

use std::path::PathBuf;

/// A layer numapilot knows: (id, Vulkan layer name, project page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Known {
    pub id: &'static str,
    pub layer: &'static str,
    pub name: &'static str,
    pub url: &'static str,
}

pub const MAKO: Known = Known {
    id: "mako",
    layer: "VK_LAYER_MAKO_render",
    name: "MAKO Renderer",
    url: "https://github.com/eugeniosegala/MAKO",
};

pub const AFMF: Known = Known {
    id: "afmf",
    layer: "VK_LAYER_AFMF",
    name: "afmf-linux",
    url: "https://github.com/serialexperimentslainnnn/afmf-linux",
};

pub const ALL: [Known; 2] = [MAKO, AFMF];

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"))
}

/// Folders where the Vulkan loader looks for implicit layers (user first).
pub fn implicit_dirs() -> Vec<PathBuf> {
    let mut v = vec![];
    v.push(
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
            .join("vulkan/implicit_layer.d"),
    );
    v.push(home().join(".local/share/vulkan/implicit_layer.d"));
    let data = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    v.extend(
        data.split(':')
            .filter(|s| !s.is_empty())
            .map(|d| PathBuf::from(d).join("vulkan/implicit_layer.d")),
    );
    v.push("/etc/vulkan/implicit_layer.d".into());
    let mut seen = vec![];
    v.retain(|d| {
        let new = !seen.contains(d);
        seen.push(d.clone());
        new
    });
    v
}

/// Names of the layers a manifest declares (`layer` or `layers`).
pub fn manifest_layers(json: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return vec![];
    };
    let mut out = vec![];
    if let Some(n) = v["layer"]["name"].as_str() {
        out.push(n.to_string());
    }
    if let Some(a) = v["layers"].as_array() {
        out.extend(
            a.iter()
                .filter_map(|l| l["name"].as_str().map(String::from)),
        );
    }
    out
}

/// The manifest of a Vulkan layer (by its layer name), in the implicit layer folders.
pub fn find_in(dirs: &[PathBuf], layer: &str) -> Option<PathBuf> {
    dirs.iter().find_map(|d| {
        let mut files: Vec<PathBuf> = std::fs::read_dir(d)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        files.sort();
        files.into_iter().find(|p| {
            std::fs::read_to_string(p)
                .map(|t| manifest_layers(&t).iter().any(|n| n == layer))
                .unwrap_or(false)
        })
    })
}

pub fn find(layer: &str) -> Option<PathBuf> {
    find_in(&implicit_dirs(), layer)
}

/// `mako-launch` (MAKO's own launcher), in PATH or ~/.local/bin.
pub fn mako_launch() -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.push(home().join(".local/bin"));
    dirs.into_iter()
        .map(|d| d.join("mako-launch"))
        .find(|p| p.is_file())
}

/// What of the other frame generators this machine has.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Found {
    /// MAKO Renderer's layer is installed.
    pub mako: bool,
    /// MAKO's launcher (preferred: it puts the layers in the order MAKO supports).
    pub mako_launch: Option<PathBuf>,
    /// afmf-linux's layer is installed.
    pub afmf: bool,
}

impl Found {
    pub fn detect() -> Found {
        let dirs = implicit_dirs();
        Found {
            mako: find_in(&dirs, MAKO.layer).is_some(),
            mako_launch: mako_launch(),
            afmf: find_in(&dirs, AFMF.layer).is_some(),
        }
    }

    /// Everything installed (tests).
    pub fn all() -> Found {
        Found {
            mako: true,
            mako_launch: Some("/usr/bin/mako-launch".into()),
            afmf: true,
        }
    }
}

/// Lines for `numapilot upscale check`: (installed, text).
pub fn report() -> Vec<(bool, String)> {
    let f = Found::detect();
    ALL.iter()
        .map(|k| {
            let on = match k.id {
                "mako" => f.mako,
                _ => f.afmf,
            };
            let what = if on {
                match (k.id, &f.mako_launch) {
                    ("mako", Some(l)) => crate::tf!(
                        "installed (launcher {})",
                        "instalado (lançador {})",
                        l.display()
                    ),
                    _ => crate::i18n::tr("installed").to_string(),
                }
            } else {
                crate::tf!(
                    "not installed (GOverlay → Frame Generation, or {})",
                    "não instalado (GOverlay → Frame Generation, ou {})",
                    k.url
                )
            };
            (
                on,
                format!(
                    "{}: {} — {what}",
                    k.name,
                    crate::i18n::tr(crate::present::engine_name(k.id))
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifests() {
        let one =
            r#"{"file_format_version":"1.0.0","layer":{"name":"VK_LAYER_AFMF","type":"GLOBAL"}}"#;
        assert_eq!(manifest_layers(one), ["VK_LAYER_AFMF"]);
        let many = r#"{"layers":[{"name":"A"},{"name":"VK_LAYER_MAKO_render"}]}"#;
        assert_eq!(manifest_layers(many), ["A", "VK_LAYER_MAKO_render"]);
        assert!(manifest_layers("not json").is_empty());

        let d = std::env::temp_dir().join(format!("numapilot-vkl-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("x.json"), one).unwrap();
        std::fs::write(d.join("y.txt"), many).unwrap();
        let dirs = [d.clone(), d.join("missing")];
        assert_eq!(find_in(&dirs, AFMF.layer), Some(d.join("x.json")));
        // only .json files are manifests
        assert_eq!(find_in(&dirs, MAKO.layer), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
