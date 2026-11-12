//! Upscaling per game: Proton's automatic swaps (FSR 4, DLSS, XeSS, FSR 3.1) and
//! OptiScaler (installed in the game's folder, with a clean uninstall).
//!
//! The PROTON_*_UPGRADE variables belong to proton-cachyos and GE-Proton: the game must use one
//! of them.

use crate::profiles::Upscale;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Modes: (id, name, explanation).
pub const MODES: &[(&str, &str, &str)] = &[
    ("nenhum", "None", "the game as it is"),
    (
        "fsr4",
        "FSR 4 (RDNA 4)",
        "swaps the game's FSR 3.1 for FSR 4 (proton-cachyos/GE download the DLL)",
    ),
    (
        "fsr4-rdna3",
        "FSR 4 for RDNA 3",
        "the same swap with the RX 7000 compatibility path",
    ),
    ("fsr3", "Newest FSR 3.1", "updates the game's FSR 3.1"),
    (
        "dlss",
        "Newest DLSS (NVIDIA)",
        "updates the game's DLSS DLL",
    ),
    (
        "xess",
        "Newest XeSS",
        "updates the game's XeSS (runs on any card)",
    ),
    (
        "optiscaler",
        "OptiScaler",
        "adds FSR/XeSS (and experimental FSR 4 INT8) to games that only have DLSS or nothing",
    ),
];

/// Environment variables of the mode (OptiScaler must also be installed in the game's folder).
pub fn env(u: &Upscale) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = vec![];
    let mut s = |k: &str, val: &str| v.push((k.to_string(), val.to_string()));
    match u.mode.as_str() {
        "fsr4" => s("PROTON_FSR4_UPGRADE", "1"),
        "fsr4-rdna3" => s("PROTON_FSR4_RDNA3_UPGRADE", "1"),
        "fsr3" => s("PROTON_FSR3_UPGRADE", "1"),
        "dlss" => s("PROTON_DLSS_UPGRADE", "1"),
        "xess" => s("PROTON_XESS_UPGRADE", "1"),
        "optiscaler" => {
            let cur = std::env::var("WINEDLLOVERRIDES").unwrap_or_default();
            let add = "dxgi=n,b";
            s(
                "WINEDLLOVERRIDES",
                &if cur.is_empty() {
                    add.to_string()
                } else {
                    format!("{cur};{add}")
                },
            );
            if u.int8 {
                // RDNA 3 on Linux: vkd3d-proton's compatibility path
                s("DXIL_SPIRV_CONFIG", "wmma_rdna3_workaround");
            }
        }
        _ => {}
    }
    if u.indicator {
        s("PROTON_FSR4_INDICATOR", "1");
        s("PROTON_DLSS_INDICATOR", "1");
    }
    v
}

/// Modes suggested for the card's architecture (from the recipes), in order of preference.
pub fn suggested(arch_upscale: &[String]) -> Vec<&'static str> {
    let mut v = vec![];
    for u in arch_upscale {
        let m = match u.as_str() {
            "fsr4" => "fsr4",
            "fsr4-rdna3" => "fsr4-rdna3",
            "fsr4-int8-experimental" | "fsr3" => {
                if u == "fsr3" {
                    "fsr3"
                } else {
                    "optiscaler"
                }
            }
            "dlss" => "dlss",
            "xess" => "xess",
            _ => continue,
        };
        if !v.contains(&m) {
            v.push(m);
        }
    }
    if !v.contains(&"optiscaler") {
        v.push("optiscaler");
    }
    v
}

// ---------------- OptiScaler ----------------

fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("numapilot/optiscaler")
}

/// Newest version already downloaded (extracted folder).
pub fn local_release() -> Option<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(data_dir())
        .ok()?
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v.pop()
}

fn find_file(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_file() && e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Some(p);
        }
        if p.is_dir()
            && depth > 0
            && let Some(f) = find_file(&p, name, depth - 1)
        {
            return Some(f);
        }
    }
    None
}

/// Downloads the latest stable version from GitHub and extracts it (needs 7z or unzip).
pub fn fetch(log: &mut dyn FnMut(&str)) -> Result<String, String> {
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "30",
            "https://api.github.com/repos/optiscaler/OptiScaler/releases/latest",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(crate::i18n::tr("could not query the OptiScaler versions on GitHub").into());
    }
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let tag = j["tag_name"]
        .as_str()
        .ok_or(crate::i18n::tr("response without a version"))?
        .to_string();
    let asset = j["assets"]
        .as_array()
        .and_then(|a| {
            a.iter().find(|x| {
                let n = x["name"].as_str().unwrap_or("").to_lowercase();
                n.contains("optiscaler") && (n.ends_with(".7z") || n.ends_with(".zip"))
            })
        })
        .ok_or(crate::i18n::tr(
            "the release does not have the OptiScaler file",
        ))?;
    let url = asset["browser_download_url"]
        .as_str()
        .ok_or(crate::i18n::tr("no link"))?;
    let name = asset["name"].as_str().unwrap_or("optiscaler.7z");
    let dest = data_dir().join(&tag);
    if dest.join("OptiScaler.ini").exists() || find_file(&dest, "OptiScaler.dll", 2).is_some() {
        log(&crate::tf!(
            "OptiScaler {tag} already downloaded",
            "OptiScaler {tag} já baixado"
        ));
        return Ok(tag);
    }
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    let arc = data_dir().join(name);
    log(&crate::tf!("downloading {name}", "baixando {name}"));
    let st = Command::new("curl")
        .args(["-fL", "--max-time", "300", "-o"])
        .arg(&arc)
        .arg(url)
        .status()
        .map_err(|e| e.to_string())?;
    if !st.success() {
        return Err(crate::i18n::tr("download failed").into());
    }
    let st = if name.ends_with(".7z") {
        Command::new("7z")
            .arg("x")
            .arg("-y")
            .arg(format!("-o{}", dest.display()))
            .arg(&arc)
            .status()
    } else {
        Command::new("unzip")
            .arg("-o")
            .arg(&arc)
            .arg("-d")
            .arg(&dest)
            .status()
    };
    let _ = std::fs::remove_file(&arc);
    match st {
        Ok(s) if s.success() => {}
        Ok(_) => return Err(crate::i18n::tr("could not extract the file").into()),
        Err(_) => return Err(crate::i18n::tr("install 7zip (sudo pacman -S 7zip)").into()),
    }
    log(&crate::tf!(
        "OptiScaler {tag} ready in {}",
        "OptiScaler {tag} pronto em {}",
        dest.display()
    ));
    Ok(tag)
}

const SKIP_EXE: &[&str] = &[
    "unitycrashhandler",
    "crashreport",
    "crashhandler",
    "launcher",
    "redist",
    "vc_redist",
    "vcredist",
    "dotnet",
    "setup",
    "unins",
    "easyanticheat",
    "beservice",
    "dxsetup",
    "ue4prereq",
    "uninstall",
];

/// Folder of the game's main executable (the one with Unreal's "-Shipping" .exe, or the largest .exe).
pub fn exe_dir(game_root: &Path) -> Option<PathBuf> {
    let mut best: Option<(u64, bool, PathBuf)> = None;
    let mut stack = vec![(game_root.to_path_buf(), 0usize)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let n = e.file_name().to_string_lossy().to_lowercase();
            if p.is_dir() {
                if depth < 5 && n != "engine" && !n.starts_with('_') && n != "redist" {
                    stack.push((p, depth + 1));
                }
            } else if n.ends_with(".exe") && !SKIP_EXE.iter().any(|s| n.contains(s)) {
                let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                let shipping = n.contains("-shipping") || n.contains("-wingdk-shipping");
                let better = match &best {
                    None => true,
                    Some((bs, bship, _)) => {
                        (shipping && !bship) || (shipping == *bship && size > *bs)
                    }
                };
                if better {
                    best = Some((size, shipping, d.clone()));
                }
            }
        }
    }
    best.map(|b| b.2)
}

/// Install folder of a Steam game by appid.
pub fn steam_game_root(appid: &str) -> Option<PathBuf> {
    for lib in crate::steam::libraries() {
        let Ok(s) = std::fs::read_to_string(lib.join(format!("appmanifest_{appid}.acf"))) else {
            continue;
        };
        let m = crate::steam::parse(&s);
        if let Some(dir) = m.get("AppState").and_then(|a| a.str("installdir")) {
            let p = lib.join("common").join(dir);
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    None
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Installed {
    pub version: String,
    pub dir: PathBuf,
    pub files: Vec<String>,
    /// dxgi.dll that already existed and was renamed to dxgi-original.dll (OptiScaler loads it).
    pub had_dxgi: bool,
}

const MANIFEST: &str = ".numapilot-optiscaler.json";

pub fn installed(dir: &Path) -> Option<Installed> {
    serde_json::from_str(&std::fs::read_to_string(dir.join(MANIFEST)).ok()?).ok()
}

/// Copies OptiScaler into the executable's folder (as dxgi.dll).
pub fn install(dir: &Path, int8: bool) -> Result<String, String> {
    let (ver, src) = local_release().ok_or(crate::i18n::tr(
        "download OptiScaler first (Download button / numapilot upscale fetch)",
    ))?;
    if installed(dir).is_some() {
        remove(dir)?;
    }
    let dll = find_file(&src, "OptiScaler.dll", 2)
        .ok_or(crate::i18n::tr("OptiScaler.dll not found in the package"))?;
    let root = dll.parent().unwrap().to_path_buf();
    let mut files = vec![];
    let had_dxgi = dir.join("dxgi.dll").exists();
    if had_dxgi {
        std::fs::rename(dir.join("dxgi.dll"), dir.join("dxgi-original.dll"))
            .map_err(|e| e.to_string())?;
    }
    let mut stack = vec![root.clone()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).map_err(|e| e.to_string())?.flatten() {
            let p = e.path();
            let rel = p.strip_prefix(&root).unwrap().to_path_buf();
            let n = rel.to_string_lossy().to_string();
            if p.is_dir() {
                std::fs::create_dir_all(dir.join(&rel)).map_err(|e| e.to_string())?;
                stack.push(p);
                continue;
            }
            // .bat and .reg belong to the Windows installer
            if n.ends_with(".bat") || n.ends_with(".reg") {
                continue;
            }
            let target = if n.eq_ignore_ascii_case("OptiScaler.dll") {
                "dxgi.dll".to_string()
            } else {
                n.clone()
            };
            if dir.join(&target).exists() && target != "dxgi.dll" {
                continue; // does not overwrite the game's files
            }
            std::fs::copy(&p, dir.join(&target)).map_err(|e| format!("{target}: {e}"))?;
            files.push(target);
        }
    }
    if int8 {
        let ini = dir.join("OptiScaler.ini");
        if let Ok(s) = std::fs::read_to_string(&ini) {
            let mut found = false;
            let new: Vec<String> = s
                .lines()
                .map(|l| {
                    if l.trim_start().starts_with("Fsr4ForceEnableInt8") {
                        found = true;
                        "Fsr4ForceEnableInt8=true".to_string()
                    } else {
                        l.to_string()
                    }
                })
                .collect();
            let mut out = new.join("\n");
            if !found {
                out.push_str("\nFsr4ForceEnableInt8=true\n");
            }
            let _ = std::fs::write(&ini, out);
        }
    }
    let m = Installed {
        version: ver.clone(),
        dir: dir.to_path_buf(),
        files,
        had_dxgi,
    };
    std::fs::write(
        dir.join(MANIFEST),
        serde_json::to_string_pretty(&m).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    Ok(ver)
}

/// Removes only what numapilot put there and restores the original dxgi.dll.
pub fn remove(dir: &Path) -> Result<(), String> {
    let m = installed(dir).ok_or(crate::i18n::tr(
        "OptiScaler was not installed by numapilot in this folder",
    ))?;
    for f in &m.files {
        let _ = std::fs::remove_file(dir.join(f));
    }
    if m.had_dxgi && dir.join("dxgi-original.dll").exists() {
        std::fs::rename(dir.join("dxgi-original.dll"), dir.join("dxgi.dll"))
            .map_err(|e| e.to_string())?;
    }
    std::fs::remove_file(dir.join(MANIFEST)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_and_install() {
        let u = Upscale {
            mode: "fsr4-rdna3".into(),
            indicator: true,
            int8: false,
        };
        let e = env(&u);
        assert!(e.contains(&("PROTON_FSR4_RDNA3_UPGRADE".into(), "1".into())));
        assert!(e.iter().any(|(k, _)| k == "PROTON_FSR4_INDICATOR"));
        assert_eq!(
            suggested(&["fsr4".into(), "fsr3".into(), "xess".into()]),
            vec!["fsr4", "fsr3", "xess", "optiscaler"]
        );
        assert_eq!(
            suggested(&["fsr4-int8-experimental".into(), "fsr3".into()]),
            vec!["optiscaler", "fsr3"]
        );

        // fake Unreal game folder + fake OptiScaler package
        let base = std::env::temp_dir().join("numapilot-upscale-test");
        let _ = std::fs::remove_dir_all(&base);
        let game = base.join("Jogo");
        let bin = game.join("Proj/Binaries/Win64");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(game.join("Engine/Binaries/Win64")).unwrap();
        std::fs::write(bin.join("Proj-Win64-Shipping.exe"), vec![0u8; 100]).unwrap();
        std::fs::write(
            game.join("Engine/Binaries/Win64/CrashReportClient.exe"),
            vec![0u8; 1000],
        )
        .unwrap();
        std::fs::write(game.join("Launcher.exe"), vec![0u8; 5000]).unwrap();
        std::fs::write(bin.join("dxgi.dll"), b"reshade").unwrap();
        assert_eq!(exe_dir(&game), Some(bin.clone()));
        unsafe { std::env::set_var("XDG_DATA_HOME", base.join("data")) };
        let rel = base.join("data/numapilot/optiscaler/v0.9.4");
        std::fs::create_dir_all(rel.join("D3D12_Optiscaler")).unwrap();
        std::fs::write(rel.join("OptiScaler.dll"), b"opti").unwrap();
        std::fs::write(
            rel.join("OptiScaler.ini"),
            "[FSR]\nFsr4ForceEnableInt8=auto\n",
        )
        .unwrap();
        std::fs::write(rel.join("OptiScaler Setup.bat"), b"x").unwrap();
        std::fs::write(rel.join("D3D12_Optiscaler/x.dll"), b"x").unwrap();
        assert_eq!(install(&bin, true).unwrap(), "v0.9.4");
        assert_eq!(std::fs::read(bin.join("dxgi.dll")).unwrap(), b"opti");
        assert_eq!(
            std::fs::read(bin.join("dxgi-original.dll")).unwrap(),
            b"reshade"
        );
        assert!(
            std::fs::read_to_string(bin.join("OptiScaler.ini"))
                .unwrap()
                .contains("Fsr4ForceEnableInt8=true")
        );
        assert!(!bin.join("OptiScaler Setup.bat").exists());
        remove(&bin).unwrap();
        assert_eq!(
            std::fs::read(bin.join("dxgi.dll")).unwrap(),
            b"reshade",
            "a dll original volta"
        );
        assert!(!bin.join("OptiScaler.ini").exists() && !bin.join("dxgi-original.dll").exists());
        unsafe { std::env::remove_var("XDG_DATA_HOME") };
    }
}
