//! FSR 4 INT8 inside the game, on the card that renders it.
//!
//! FSR 4 is a temporal upscaler: it needs the game's depth and motion vectors, so it runs inside
//! the game (never on the finished image like the presentation chain in `crate::present`).
//! OptiScaler, loaded by Proton as `dxgi.dll`, takes what the game hands to DLSS, FSR 2+ or XeSS
//! and feeds it to an FSR 4 upscaler DLL. AMD's own DLL only runs its FP8 model on RDNA 4; the
//! INT8 model runs on older cards with an INT8 dot product:
//!
//! - waves of 32 (RDNA 2/3, the BC-250's gfx1013): the BC250 FSR4 fork
//!   (github.com/daniel-h-0/bc250-fsr4-fork, RC12), whose shaders carry the weights as constants;
//! - waves of 64 only (Vega 20 / MI50, CDNA): AMD's leaked FSR 4.0.2 INT8 build by default (AMD's
//!   own builds run on wave64; the copy is the one github.com/Schaka/fsr4-gfx803 keeps), or that
//!   project's 4.1.1 rebuilds (`vega`, `bc250` = "fork"), chosen with `--dll`.
//!
//! Polaris (GCN 4) has no INT8 dot product and needs a patched RADV (fsr4-gfx803): numapilot
//! does not install a second driver, so it only explains that. NVIDIA has real DLSS and RDNA 4 has
//! the official FSR 4 (Proton's swap, `upscale` mode "fsr4").
//!
//! Nothing here is shipped with numapilot: every file is downloaded on the user's machine into
//! `~/.local/share/numapilot/fsr4/` and copied into the game's folder with a manifest, so
//! `upscale::remove` takes exactly that back out.
//!
//! With two cards the game upscales on its own card and lsfg-vk generates frames on the other one
//! (`present` engine "lsfg-out", gamescope on the output card); with one card both run on it.

use crate::topo::Gpu;
use std::path::{Path, PathBuf};
use std::process::Command;

/// How FSR 4 reaches a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// RDNA 4: the official FSR 4 (FP8); Proton's swap is enough (upscale mode "fsr4").
    Native,
    /// NVIDIA: real DLSS.
    Dlss,
    /// INT8 dot product in hardware and waves of 32: the BC250 FSR4 fork's DLL.
    Wave32,
    /// INT8 dot product in hardware, waves of 64 only (Vega 20 / MI50, CDNA): the fork rebuilt for
    /// wave64.
    Wave64,
    /// Waves of 64 without the INT8 dot product (Vega 10, Vega APUs): the wave64 DLL runs, slower
    /// (about 5 ms at 1080p on a Vega 56).
    Wave64Slow,
    /// Polaris (GCN 4): needs a patched RADV (fsr4-gfx803), not installed by numapilot.
    Polaris,
    /// No FSR 4 (RDNA 1 Navi 10, Intel, older GCN): use FSR 3.1 or XeSS.
    Unsupported,
}

impl Route {
    /// numapilot can install FSR 4 INT8 for this card.
    pub fn installable(self) -> bool {
        matches!(self, Route::Wave32 | Route::Wave64 | Route::Wave64Slow)
    }

    /// Short id, stored in the game's manifest.
    pub fn id(self) -> &'static str {
        match self {
            Route::Native => "native",
            Route::Dlss => "dlss",
            Route::Wave32 => "wave32",
            Route::Wave64 | Route::Wave64Slow => "wave64",
            Route::Polaris => "polaris",
            Route::Unsupported => "none",
        }
    }

    /// What this means for the user (English; `tr` where it is shown).
    pub fn explain(self) -> &'static str {
        match self {
            Route::Native => {
                "RDNA 4: official FSR 4; Proton's swap is enough (mode \"FSR 4 (RDNA 4)\")"
            }
            Route::Dlss => "NVIDIA: use the game's own DLSS (mode \"Newest DLSS\")",
            Route::Wave32 => {
                "FSR 4 INT8 with the BC250 FSR4 DLL (INT8 dot product in hardware, waves of 32)"
            }
            Route::Wave64 => {
                "FSR 4 INT8 with the wave64 rebuild of the BC250 FSR4 DLL: runs, but on the MI50 it looked worse than native (Spider-Man, 1440p); prefer the game's upscaler with lsfg-vk on the other card"
            }
            Route::Wave64Slow => {
                "FSR 4 INT8 with the wave64 DLL, without an INT8 dot product: works, but costs about 5 ms per frame at 1080p"
            }
            Route::Polaris => {
                "Polaris has no INT8 dot product: FSR 4 needs a patched RADV (github.com/Schaka/fsr4-gfx803), which numapilot does not install; use FSR 3.1 or XeSS"
            }
            Route::Unsupported => "no FSR 4 on this card: use FSR 3.1 or XeSS",
        }
    }
}

/// The card's graphics architecture ("gfx906"), from the driver or from the boards it knows.
fn gfx_of(g: &Gpu) -> Option<String> {
    if let Some(x) = &g.gfx {
        return Some(x.clone());
    }
    if g.vendor != 0x1002 {
        return None;
    }
    // without KFD (no ROCm) the driver does not say: the cards numapilot knows by PCI id
    let gfx = match g.device {
        0x13fe => "gfx1013",          // BC-250 (Cyan Skillfish)
        0x163f | 0x1435 => "gfx1033", // Steam Deck LCD / OLED (Van Gogh)
        0x66a0 | 0x66a1 | 0x66a2 | 0x66a3 | 0x66a7 | 0x66af => "gfx906", // Vega 20: MI50/MI60, Radeon VII
        0x6860..=0x687f => "gfx900",                                     // Vega 10
        0x67c0..=0x67ff | 0x6fdf | 0x6980..=0x699f => "gfx803",          // Polaris
        _ => return None,
    };
    Some(gfx.into())
}

/// Route of FSR 4 for one card.
pub fn route(g: &Gpu) -> Route {
    if g.is_nvidia() {
        return Route::Dlss;
    }
    if g.vendor != 0x1002 {
        return Route::Unsupported;
    }
    match gfx_of(g) {
        Some(x) => route_for_gfx(&x),
        None => Route::Unsupported,
    }
}

/// Route for an AMD graphics architecture ("gfx906", "gfx1013", "gfx1100").
pub fn route_for_gfx(gfx: &str) -> Route {
    let g = gfx.to_ascii_lowercase();
    if g.starts_with("gfx12") {
        Route::Native
    } else if g.starts_with("gfx11") || g.starts_with("gfx103") {
        Route::Wave32
    } else if ["gfx1011", "gfx1012", "gfx1013"].contains(&g.as_str()) {
        // Navi 12/14 and the BC-250 have the dot product instructions, Navi 10 does not
        Route::Wave32
    } else if g == "gfx906" || g == "gfx908" || g.starts_with("gfx90a") || g.starts_with("gfx94") {
        Route::Wave64
    } else if g.starts_with("gfx90") {
        Route::Wave64Slow
    } else if g.starts_with("gfx8") {
        Route::Polaris
    } else {
        Route::Unsupported
    }
}

// ---------------- downloads ----------------

/// Every file numapilot downloads for FSR 4, pinned to one version and checked by SHA-256 before
/// it is used (none of them may be redistributed from this repository: AMD's and NVIDIA's DLLs,
/// OptiScaler is GPL-3). A different file is refused.
pub struct Pinned {
    pub url: &'static str,
    pub sha256: &'static str,
}

/// OptiScaler build the BC250 FSR4 fork documents and tests (FFX backend, `Fsr4ForceModel`).
pub const OPTISCALER_VERSION: &str = "10.0.0-pre1";
pub const OPTISCALER: Pinned = Pinned {
    url: "https://github.com/optiscaler/OptiScaler-nightly/releases/download/nightly-20260904/OptiScaler_v10.0.0-pre1_20260904.7z",
    sha256: "730d5057338cf68adc3bf38a358985a04629ad00fae305bd717694a392216a53",
};
/// OptiScaler plugin that unlocks the inputs of games that check the card's vendor.
pub const OPTIPATCHER: Pinned = Pinned {
    url: "https://github.com/optiscaler/OptiPatcher/releases/download/v0.41/OptiPatcher_v0.41.asi",
    sha256: "fb12735bfcc0d47f534f2206d57ec34129dc3d22b6405a1c2ef86745ab48b2eb",
};
/// NVIDIA's signed DLSS DLL, from NVIDIA's own repository: OptiScaler offers the game DLSS
/// through it (games that ship their own keep it).
pub const DLSS_HELPER: Pinned = Pinned {
    url: "https://raw.githubusercontent.com/NVIDIA/DLSS/a291cc7d2cc642a51566f3dfd5376f635cd1b284/lib/Windows_x86_64/rel/nvngx_dlss.dll",
    sha256: "be6e434a94ca32499515eb62ca0e6c274526055d568d0426e4c652dcdfb6ee6e",
};
/// BC250 FSR4 RC12 (FSR 4.1.1, INT8, waves of 32): the archive, then the DLL inside (the hash
/// the project publishes).
pub const WAVE32_ARCHIVE: Pinned = Pinned {
    url: "https://github.com/daniel-h-0/bc250-fsr4-fork/releases/download/v4.0.0-rc12/bc250-fsr4-dll-4.0.0-rc12.zip",
    sha256: "646778a0d06c42bf3cdcc99f9396680f5cc4a33658b90c0889fd15751bfb800d",
};
pub const WAVE32_SHA256: &str = "4b0df27761d7ced52c30dcbd1641c55bdcf4d2ae10d59f7e5b4b2f895b6bba79";
pub const WAVE32_VERSION: &str = "4.1.1r12";
/// fsr4-gfx803 r6: its `bc250` DLL is the fork (RC10) rebuilt without the wave32 request.
pub const WAVE64_ARCHIVE: Pinned = Pinned {
    url: "https://github.com/Schaka/fsr4-gfx803/releases/download/r6/fsr4-gfx803-r6-dlls.tar.gz",
    sha256: "7b965c141ce7d623504a172e08903a352eb1e118c46d96c238a3dd8dfc615ba2",
};
/// The wave64 DLL in that archive (`.vega.dll` gives two passes back to AMD's shaders and was
/// faster in one game on a Vega 56; both give the same picture).
pub const WAVE64_DLL: &str = "amd_fidelityfx_upscaler_dx12.bc250.dll";
pub const WAVE64_SHA256: &str = "5343b4e353bad7ac324b9fe2babcd9e9c3790729568e912bd590d6b426f7b324";
/// The Vega build in the same archive: the fork with pass9/pass11 left to AMD's shaders.
pub const WAVE64_VEGA_DLL: &str = "amd_fidelityfx_upscaler_dx12.vega.dll";
pub const WAVE64_VEGA_SHA256: &str =
    "1618d9491fa9a448bf2c89cbb87294842d730e12983426e279f91a507ea4ea36";
/// AMD's leaked FSR 4.0.2 INT8 upscaler, as fsr4-gfx803 keeps it (a plain DLL, no archive).
pub const V402: Pinned = Pinned {
    url: "https://github.com/Schaka/fsr4-gfx803/raw/6aa7c33c32e534845ec62b859a7a46fa6879398c/fsr4_dlls/4.0.2-int8/amd_fidelityfx_upscaler_dx12.dll",
    sha256: "9123f83739e7bb39fcb135cafc339606dec78a74c650338ad275ee45c2d59d02",
};
/// The DLLs a wave64 card can take: (id, label). The first is the default; 4.0.2 is what MI50
/// owners report as the most stable (Lucas, 2026-11-25), the other two are 4.1.1 rebuilds.
pub const WAVE64_DLLS: [(&str, &str); 3] = [
    ("4.0.2", "FSR 4.0.2 INT8 (AMD's leaked build)"),
    ("vega", "FSR 4.1.1 rebuilt for Vega (fsr4-gfx803)"),
    ("fork", "FSR 4.1.1 rebuilt for wave64 (fsr4-gfx803 bc250)"),
];

/// The wave64 DLL to use: a known id, else the default.
pub fn wave64_dll(want: Option<&str>) -> &'static str {
    WAVE64_DLLS
        .iter()
        .find(|(id, _)| Some(*id) == want)
        .map_or(WAVE64_DLLS[0].0, |(id, _)| id)
}

/// What a route installs, as stored in the game's manifest ("wave32", "wave64 4.0.2", ...).
pub fn build_id(r: Route, dll: Option<&str>) -> String {
    match r {
        Route::Wave64 | Route::Wave64Slow => format!("wave64 {}", wave64_dll(dll)),
        _ => r.id().to_string(),
    }
}

/// Name OptiScaler loads the upscaler under.
pub const UPSCALER_DLL: &str = "amd_fidelityfx_upscaler_dx12.dll";
/// Mesa before 26.2.3 miscompiles other shaders next to the fork's (BC250 FSR4 release notes).
pub const MIN_MESA: &str = "26.2.3";

fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("numapilot/fsr4")
}

/// Folder of the downloaded OptiScaler build.
pub fn optiscaler_dir() -> PathBuf {
    data_dir().join(format!("optiscaler-{OPTISCALER_VERSION}"))
}

/// Downloaded upscaler DLL for a route, if it is there.
pub fn local_dll(r: Route, dll: Option<&str>) -> Option<PathBuf> {
    let p = data_dir()
        .join(format!("dll-{}", build_id(r, dll).replace(' ', "-")))
        .join(UPSCALER_DLL);
    p.is_file().then_some(p)
}

fn helper(name: &str) -> PathBuf {
    data_dir().join(name)
}

/// Downloads a pinned file and keeps it only if its SHA-256 matches.
fn download(f: &Pinned, dest: &Path) -> Result<(), String> {
    if let Some(d) = dest.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let part = dest.with_extension("part");
    // an earlier try that finished the transfer but was stopped before the rename
    if part.is_file() && check(&part, f.sha256, f.url).is_ok() {
        return std::fs::rename(&part, dest).map_err(|e| e.to_string());
    }
    // no total time limit (57 MB over a slow line took longer than any fixed one): curl gives up
    // only when the transfer stalls below 1 KB/s for 2 minutes, retries, and resumes the .part
    // left by an earlier try (the SHA-256 below catches a bad resume)
    let st = Command::new("curl")
        .args([
            "-fL",
            "--retry",
            "5",
            "--retry-all-errors",
            "--speed-limit",
            "1024",
            "--speed-time",
            "120",
            "--continue-at",
            "-",
            "-o",
        ])
        .arg(&part)
        .arg(f.url)
        .status()
        .map_err(|e| e.to_string())?;
    if !st.success() {
        // the .part stays: the next try resumes it
        return Err(crate::tf!(
            "download failed (it resumes on the next try): {}",
            "o download falhou (continua de onde parou na próxima vez): {}",
            f.url
        ));
    }
    if let Err(e) = check(&part, f.sha256, f.url) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    std::fs::rename(&part, dest).map_err(|e| e.to_string())
}

/// Refuses a file whose SHA-256 is not the pinned one.
fn check(p: &Path, want: &str, what: &str) -> Result<(), String> {
    match sha256(p) {
        Some(h) if h == want => Ok(()),
        Some(h) => Err(crate::tf!(
            "{} does not match the checksum numapilot expects ({} instead of {}): not used",
            "{} não confere com o checksum que o numapilot espera ({} em vez de {}): não usado",
            what,
            h,
            want
        )),
        None => Err(crate::i18n::tr("could not compute the SHA-256 (sha256sum)").into()),
    }
}

/// Extracts .7z, .zip or .tar.gz with what the system has (7z, unzip, bsdtar or tar).
fn extract(arc: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let n = arc.to_string_lossy().to_lowercase();
    let tries: Vec<Vec<String>> = if n.ends_with(".tar.gz") || n.ends_with(".tgz") {
        vec![vec![
            "tar".into(),
            "-xzf".into(),
            arc.to_string_lossy().into(),
            "-C".into(),
            dest.to_string_lossy().into(),
        ]]
    } else {
        let mut v = vec![];
        if n.ends_with(".7z") {
            v.push(vec![
                "7z".into(),
                "x".into(),
                "-y".into(),
                format!("-o{}", dest.display()),
                arc.to_string_lossy().into(),
            ]);
        } else {
            v.push(vec![
                "unzip".into(),
                "-o".into(),
                arc.to_string_lossy().into(),
                "-d".into(),
                dest.to_string_lossy().into(),
            ]);
        }
        // libarchive's bsdtar reads both and comes with pacman on Arch
        v.push(vec![
            "bsdtar".into(),
            "-xf".into(),
            arc.to_string_lossy().into(),
            "-C".into(),
            dest.to_string_lossy().into(),
        ]);
        v
    };
    for t in &tries {
        if let Ok(s) = Command::new(&t[0])
            .args(&t[1..])
            .stdout(std::process::Stdio::null())
            .status()
            && s.success()
        {
            return Ok(());
        }
    }
    Err(crate::i18n::tr("could not extract the file (install 7zip: sudo pacman -S 7zip)").into())
}

/// SHA-256 of a file (sha256sum).
fn sha256(p: &Path) -> Option<String> {
    let o = Command::new("sha256sum").arg(p).output().ok()?;
    o.status.success().then(|| {
        String::from_utf8_lossy(&o.stdout)
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string()
    })
}

fn find(dir: &Path, name: &str, depth: usize) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_file() && e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Some(p);
        }
        if p.is_dir()
            && depth > 0
            && let Some(f) = find(&p, name, depth - 1)
        {
            return Some(f);
        }
    }
    None
}

/// Downloads OptiScaler 10, OptiPatcher, the DLSS helper and the upscaler DLL for the route
/// (each once, each checked by SHA-256). Returns the DLL.
pub fn fetch(r: Route, dll: Option<&str>, log: &mut dyn FnMut(&str)) -> Result<PathBuf, String> {
    if !r.installable() {
        return Err(crate::i18n::tr(r.explain()).into());
    }
    let od = optiscaler_dir();
    if find(&od, "OptiScaler.dll", 2).is_none() {
        log(&crate::tf!(
            "downloading OptiScaler {}",
            "baixando o OptiScaler {}",
            OPTISCALER_VERSION
        ));
        let arc = data_dir().join("optiscaler.7z");
        download(&OPTISCALER, &arc)?;
        let r = extract(&arc, &od);
        let _ = std::fs::remove_file(&arc);
        r?;
        if find(&od, "OptiScaler.dll", 2).is_none() {
            return Err(crate::i18n::tr("OptiScaler.dll not found in the package").into());
        }
    }
    if !helper("OptiPatcher.asi").is_file() {
        log(crate::i18n::tr("downloading OptiPatcher"));
        download(&OPTIPATCHER, &helper("OptiPatcher.asi"))?;
    }
    if !helper("nvngx_dlss.dll").is_file() {
        log(crate::i18n::tr(
            "downloading NVIDIA's DLSS DLL (OptiScaler offers DLSS to the game through it)",
        ));
        download(&DLSS_HELPER, &helper("nvngx_dlss.dll"))?;
    }
    if let Some(p) = local_dll(r, dll) {
        return Ok(p);
    }
    let build = build_id(r, dll).replace(' ', "-");
    let dest = data_dir().join(format!("dll-{build}"));
    let tmp = data_dir().join(format!("tmp-{build}"));
    let wave64 = wave64_dll(dll);
    if r != Route::Wave32 && wave64 == "4.0.2" {
        log(crate::i18n::tr("downloading the FSR 4.0.2 INT8 DLL"));
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        download(&V402, &dest.join(UPSCALER_DLL))?;
        return Ok(dest.join(UPSCALER_DLL));
    }
    let _ = std::fs::remove_dir_all(&tmp);
    let res = (|| -> Result<PathBuf, String> {
        let (arc, want, sum) = if r == Route::Wave32 {
            log(&crate::tf!(
                "downloading the BC250 FSR4 DLL ({})",
                "baixando a DLL do BC250 FSR4 ({})",
                WAVE32_VERSION
            ));
            let arc = tmp.join("bc250-fsr4.zip");
            download(&WAVE32_ARCHIVE, &arc)?;
            (arc, UPSCALER_DLL, WAVE32_SHA256)
        } else {
            log(crate::i18n::tr(
                "downloading the wave64 FSR 4 DLL (fsr4-gfx803)",
            ));
            let arc = tmp.join("fsr4-gfx803-dlls.tar.gz");
            download(&WAVE64_ARCHIVE, &arc)?;
            if wave64 == "vega" {
                (arc, WAVE64_VEGA_DLL, WAVE64_VEGA_SHA256)
            } else {
                (arc, WAVE64_DLL, WAVE64_SHA256)
            }
        };
        let x = tmp.join("x");
        extract(&arc, &x)?;
        let dll = find(&x, want, 4).ok_or_else(|| {
            crate::tf!(
                "{} not found in the download",
                "{} não encontrado no download",
                want
            )
        })?;
        check(&dll, sum, want)?;
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        std::fs::copy(&dll, dest.join(UPSCALER_DLL)).map_err(|e| e.to_string())?;
        Ok(dest.join(UPSCALER_DLL))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    res
}

// ---------------- the game's folder ----------------

/// Sets `key=value` in `[section]` of an INI text: replaces the key in that section, adds it at the
/// section's end, or adds the section.
pub fn set_ini(text: &str, section: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let head = format!("[{}]", section.to_ascii_lowercase());
    let is_head = |l: &str| l.trim().starts_with('[');
    let start = lines
        .iter()
        .position(|l| l.trim().to_ascii_lowercase() == head);
    let line = format!("{key}={value}");
    match start {
        Some(s) => {
            let end = lines[s + 1..]
                .iter()
                .position(|l| is_head(l))
                .map_or(lines.len(), |i| s + 1 + i);
            let found = (s + 1..end).find(|&i| {
                let l = lines[i].trim_start();
                !l.starts_with(';')
                    && l.split_once('=')
                        .is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))
            });
            match found {
                Some(i) => lines[i] = line,
                None => {
                    // after the section's last non-empty line
                    let mut at = end;
                    while at > s + 1 && lines[at - 1].trim().is_empty() {
                        at -= 1;
                    }
                    lines.insert(at, line);
                }
            }
        }
        None => {
            if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push(format!("[{section}]"));
            lines.push(line);
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// The OptiScaler settings for FSR 4 INT8 (BC250 FSR4's guide): FFX backend, INT8 model,
/// OptiScaler's own frame generation off (lsfg-vk does that), OptiPatcher loaded, watermarks
/// of the upscaler and of the game's frame generation off unless asked for.
pub fn ini_settings(watermark: bool) -> Vec<(&'static str, &'static str, &'static str)> {
    let mark = if watermark { "true" } else { "false" };
    vec![
        ("Upscalers", "Dx12Upscaler", "ffx"),
        ("Upscalers", "Dx11Upscaler", "ffx_12"),
        ("Upscalers", "VulkanUpscaler", "ffx_12"),
        ("FSR", "UpscalerIndex", "0"),
        ("FSR", "Fsr4ForceModel", "2"),
        ("FSR", "FsrNonLinearColorSpace", "false"),
        // written out, so a reinstall without --watermark turns an earlier "true" off
        ("FSR", "Fsr4EnableWatermark", mark),
        ("FrameGen", "Enabled", "false"),
        // the game's own frame generation goes through OptiScaler's FSR-FG, which has its own mark
        ("FSRFG", "EnableWatermark", mark),
        ("Plugins", "LoadAsiPlugins", "true"),
    ]
}

/// Installs OptiScaler with the FSR 4 INT8 DLL of the route in the game's executable folder.
/// Everything copied goes in the manifest, so `upscale::remove` restores the folder.
pub fn install(
    dir: &Path,
    r: Route,
    wave64: Option<&str>,
    watermark: bool,
    log: &mut dyn FnMut(&str),
) -> Result<String, String> {
    let dll = fetch(r, wave64, log)?;
    let od = optiscaler_dir();
    let root = find(&od, "OptiScaler.dll", 2)
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .ok_or(crate::i18n::tr("OptiScaler.dll not found in the package"))?;
    let mut m = crate::upscale::install_from(dir, OPTISCALER_VERSION, &root, false)?;
    // the upscaler DLL: where OptiScaler's package keeps it (OptiScaler/), never over the game's
    let rel = m
        .files
        .iter()
        .find(|f| {
            Path::new(f)
                .file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(UPSCALER_DLL))
        })
        .cloned()
        .unwrap_or_else(|| format!("OptiScaler/{UPSCALER_DLL}"));
    copy_into(dir, &dll, &rel, &mut m.files)?;
    copy_into(
        dir,
        &helper("OptiPatcher.asi"),
        "OptiScaler/plugins/OptiPatcher.asi",
        &mut m.files,
    )?;
    if !dir.join("nvngx_dlss.dll").exists() {
        copy_into(
            dir,
            &helper("nvngx_dlss.dll"),
            "nvngx_dlss.dll",
            &mut m.files,
        )?;
    }
    let ini = dir.join("OptiScaler.ini");
    let mut text = std::fs::read_to_string(&ini).unwrap_or_default();
    for (s, k, v) in ini_settings(watermark) {
        text = set_ini(&text, s, k, v);
    }
    std::fs::write(&ini, text).map_err(|e| e.to_string())?;
    m.fsr4 = Some(build_id(r, wave64));
    crate::upscale::save_manifest(dir, &m)?;
    Ok(match r {
        Route::Wave32 => {
            format!("OptiScaler {OPTISCALER_VERSION} + FSR {WAVE32_VERSION} (BC250 FSR4)")
        }
        _ => {
            let id = wave64_dll(wave64);
            let label = WAVE64_DLLS.iter().find(|d| d.0 == id).map_or("", |d| d.1);
            format!("OptiScaler {OPTISCALER_VERSION} + {label}")
        }
    })
}

fn copy_into(dir: &Path, src: &Path, rel: &str, files: &mut Vec<String>) -> Result<(), String> {
    let to = dir.join(rel);
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    }
    std::fs::copy(src, &to).map_err(|e| format!("{rel}: {e}"))?;
    if !files.iter().any(|f| f == rel) {
        files.push(rel.to_string());
    }
    Ok(())
}

/// Compares dotted versions ("26.2.3" < "26.10").
pub fn version_lt(a: &str, b: &str) -> bool {
    let p = |s: &str| -> Vec<u32> {
        s.split(['.', '-', '_'])
            .map(|x| {
                x.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0)
            })
            .collect()
    };
    p(a) < p(b)
}

/// Warnings for running FSR 4 INT8 on this card now (the `run` and the interface show them).
pub fn warnings(g: &Gpu, installed_route: Option<&str>, mesa: Option<&str>) -> Vec<String> {
    let r = route(g);
    let mut w = vec![];
    if !r.installable() {
        w.push(crate::tf!(
            "FSR 4 INT8 on the {}: {}",
            "FSR 4 INT8 na {}: {}",
            g.alias,
            crate::i18n::tr(r.explain())
        ));
        return w;
    }
    if let Some(i) = installed_route
        && i != r.id()
    {
        w.push(crate::tf!(
            "the FSR 4 DLL in the game's folder was made for another card ({}); the game now runs on the {} ({}): reinstall it from the profile",
            "a DLL do FSR 4 na pasta do jogo foi feita para outra placa ({}); o jogo agora roda na {} ({}): reinstale pelo perfil",
            i,
            g.alias,
            r.id()
        ));
    }
    if r == Route::Wave32
        && let Some(v) = mesa
        && version_lt(v, MIN_MESA)
    {
        w.push(crate::tf!(
            "Mesa {} has a compiler bug that breaks other shaders next to the BC250 FSR4 DLL: update Mesa to {} or newer",
            "o Mesa {} tem um erro de compilador que quebra outros shaders junto da DLL do BC250 FSR4: atualize o Mesa para {} ou mais novo",
            v,
            MIN_MESA
        ));
    }
    if r == Route::Wave64Slow {
        w.push(crate::i18n::tr(r.explain()).into());
    }
    w
}

/// One line per card: what FSR 4 does there (`numapilot upscale check`).
pub fn report(gpus: &[Gpu]) -> Vec<(bool, String)> {
    gpus.iter()
        .map(|g| {
            let r = route(g);
            (
                r.installable() || r == Route::Native || r == Route::Dlss,
                format!(
                    "{} ({}, {}): {}",
                    g.alias,
                    gfx_of(g).unwrap_or_else(|| format!("{:04x}:{:04x}", g.vendor, g.device)),
                    g.pci,
                    crate::i18n::tr(r.explain())
                ),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes() {
        assert_eq!(route_for_gfx("gfx906"), Route::Wave64);
        assert_eq!(route_for_gfx("gfx1013"), Route::Wave32);
        assert_eq!(route_for_gfx("gfx1033"), Route::Wave32); // Steam Deck
        assert_eq!(route_for_gfx("gfx1010"), Route::Unsupported);
        assert_eq!(route_for_gfx("gfx1035"), Route::Wave32);
        assert_eq!(route_for_gfx("gfx1100"), Route::Wave32);
        assert_eq!(route_for_gfx("gfx1200"), Route::Native);
        assert_eq!(route_for_gfx("gfx900"), Route::Wave64Slow);
        assert_eq!(route_for_gfx("gfx90c"), Route::Wave64Slow);
        assert_eq!(route_for_gfx("gfx803"), Route::Polaris);
        assert!(Route::Wave64.installable() && !Route::Polaris.installable());
        assert_eq!(Route::Wave64Slow.id(), Route::Wave64.id());
    }

    #[test]
    fn ini() {
        let t =
            "[Upscalers]\nDx12Upscaler=auto\n; Dx11Upscaler=auto\n\n[FSR]\nUpscalerIndex=auto\n";
        let t = set_ini(t, "Upscalers", "Dx12Upscaler", "ffx");
        let t = set_ini(&t, "Upscalers", "Dx11Upscaler", "ffx_12");
        let t = set_ini(&t, "FSR", "Fsr4ForceModel", "2");
        let t = set_ini(&t, "FrameGen", "Enabled", "false");
        assert_eq!(
            t,
            "[Upscalers]\nDx12Upscaler=ffx\n; Dx11Upscaler=auto\nDx11Upscaler=ffx_12\n\n[FSR]\nUpscalerIndex=auto\nFsr4ForceModel=2\n\n[FrameGen]\nEnabled=false\n"
        );
        // a key with the same name in another section is left alone
        let t = set_ini("[A]\nEnabled=true\n[B]\nx=1\n", "B", "Enabled", "false");
        assert_eq!(t, "[A]\nEnabled=true\n[B]\nx=1\nEnabled=false\n");
    }

    #[test]
    fn versions_and_assets() {
        assert!(version_lt("26.2.2", MIN_MESA));
        assert!(!version_lt("26.3.1", MIN_MESA));
        assert!(!version_lt("26.10", "26.2.3"));
        // every pinned download has a full SHA-256
        for f in [
            &OPTISCALER,
            &OPTIPATCHER,
            &DLSS_HELPER,
            &WAVE32_ARCHIVE,
            &WAVE64_ARCHIVE,
            &V402,
        ] {
            assert!(f.url.starts_with("https://"));
            assert!(f.sha256.len() == 64 && f.sha256.chars().all(|c| c.is_ascii_hexdigit()));
        }
        // wave64 DLL choice: 4.0.2 by default, unknown names fall back to it
        assert_eq!(wave64_dll(None), "4.0.2");
        assert_eq!(wave64_dll(Some("vega")), "vega");
        assert_eq!(wave64_dll(Some("x")), "4.0.2");
        assert_eq!(build_id(Route::Wave64, None), "wave64 4.0.2");
        assert_eq!(build_id(Route::Wave32, Some("vega")), "wave32");
    }

    #[test]
    fn install_and_remove() {
        let _l = crate::upscale::DATA_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let base = std::env::temp_dir().join("numapilot-fsr4-test");
        let _ = std::fs::remove_dir_all(&base);
        let game = base.join("game/bin");
        std::fs::create_dir_all(&game).unwrap();
        std::fs::write(game.join("Game.exe"), b"exe").unwrap();
        unsafe { std::env::set_var("XDG_DATA_HOME", base.join("data")) };
        // everything already downloaded: install never touches the network
        let od = optiscaler_dir();
        std::fs::create_dir_all(od.join("OptiScaler")).unwrap();
        std::fs::write(od.join("OptiScaler.dll"), b"opti").unwrap();
        std::fs::write(
            od.join("OptiScaler.ini"),
            "[Upscalers]\nDx12Upscaler=auto\n[FSR]\nFsr4ForceModel=auto\n",
        )
        .unwrap();
        std::fs::write(od.join("OptiScaler").join(UPSCALER_DLL), b"amd").unwrap();
        std::fs::write(helper("OptiPatcher.asi"), b"patch").unwrap();
        std::fs::write(helper("nvngx_dlss.dll"), b"dlss").unwrap();
        let d = data_dir().join("dll-wave64-fork");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(UPSCALER_DLL), b"wave64").unwrap();

        install(&game, Route::Wave64, Some("fork"), true, &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(game.join("dxgi.dll")).unwrap(), b"opti");
        assert_eq!(
            std::fs::read(game.join("OptiScaler").join(UPSCALER_DLL)).unwrap(),
            b"wave64"
        );
        assert!(game.join("OptiScaler/plugins/OptiPatcher.asi").is_file());
        let ini = std::fs::read_to_string(game.join("OptiScaler.ini")).unwrap();
        assert!(ini.contains("Dx12Upscaler=ffx\n") && ini.contains("Fsr4ForceModel=2"));
        assert!(ini.contains("Fsr4EnableWatermark=true") && ini.contains("LoadAsiPlugins=true"));
        assert!(ini.contains("[FSRFG]\nEnableWatermark=true"));
        let m = crate::upscale::installed(&game).unwrap();
        assert_eq!(m.fsr4.as_deref(), Some("wave64 fork"));
        crate::upscale::remove(&game).unwrap();
        let left: Vec<_> = std::fs::read_dir(&game)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(left, vec!["Game.exe".to_string()]);
        unsafe { std::env::remove_var("XDG_DATA_HOME") };
    }
}
