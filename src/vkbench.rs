//! Upscaling as Vulkan compute (step M0b of docs/PRESENT-CHAIN.md): runs mpv user shaders
//! ("hooks", the format Anime4K ships in) as compute passes over storage buffers, with GPU
//! timestamps per pass, offscreen and with no files in between. The same runner becomes the
//! core of the gamescope layer (M1).
//!
//! The mpv texture API a hook uses is provided per bound texture X: `X_size`, `X_pt`, `X_pos`
//! (position of the pixel being written, normalized), `X_tex(pos)` (bilinear, clamped to the
//! edge) and `X_texOff(offset)` (the texel at the current position plus an offset). GLSL is turned
//! into SPIR-V by naga at run time, so no shader compiler is needed on the machine. libvulkan is
//! loaded at run time too: without it the bench just says so.

use anyhow::{Context, Result, anyhow, bail};
use ash::vk;
use std::collections::BTreeMap;

/// Anime4K CNN x2 (MIT, bloc97), shipped in `shaders/anime4k`.
pub const ANIME4K_S: &str = include_str!("../shaders/anime4k/Anime4K_Upscale_CNN_x2_S.glsl");
pub const ANIME4K_M: &str = include_str!("../shaders/anime4k/Anime4K_Upscale_CNN_x2_M.glsl");
pub const ANIME4K_L: &str = include_str!("../shaders/anime4k/Anime4K_Upscale_CNN_x2_L.glsl");

/// Plain bilinear 2×: the floor any upscaler is compared with.
pub const BILINEAR_X2: &str = "//!DESC bilinear-x2
//!HOOK MAIN
//!BIND MAIN
//!SAVE MAIN
//!WIDTH MAIN.w 2 *
//!HEIGHT MAIN.h 2 *
vec4 hook() {
    return MAIN_tex(MAIN_pos);
}
";

/// ArtCNN C4F16 / C4F32 (MIT, Artoriuz), shipped in `shaders/artcnn`: luma doublers.
pub const ARTCNN_C4F16: &str = include_str!("../shaders/artcnn/ArtCNN_C4F16.glsl");
pub const ARTCNN_C4F32: &str = include_str!("../shaders/artcnn/ArtCNN_C4F32.glsl");

/// Models shipped with numapilot: (value, name, shader).
pub const MODELS: [(&str, &str, &str); 6] = [
    ("bilinear", "bilinear 2× (floor)", BILINEAR_X2),
    ("anime4k-s", "Anime4K CNN x2 S", ANIME4K_S),
    ("anime4k-m", "Anime4K CNN x2 M", ANIME4K_M),
    ("anime4k-l", "Anime4K CNN x2 L", ANIME4K_L),
    ("artcnn-c4f16", "ArtCNN C4F16", ARTCNN_C4F16),
    ("artcnn-c4f32", "ArtCNN C4F32", ARTCNN_C4F32),
];

/// Models downloaded on first use, not shipped because of their license (LGPL 3, igv):
/// (value, name, file, URL). Kept in `shader_dir()`.
pub const DOWNLOADS: [(&str, &str, &str, &str); 2] = [
    (
        "fsrcnnx",
        "FSRCNNX x2 8-0-4-1",
        "FSRCNNX_x2_8-0-4-1.glsl",
        "https://github.com/igv/FSRCNN-TensorFlow/releases/download/1.1/FSRCNNX_x2_8-0-4-1.glsl",
    ),
    (
        "fsrcnnx-16",
        "FSRCNNX x2 16-0-4-1",
        "FSRCNNX_x2_16-0-4-1.glsl",
        "https://github.com/igv/FSRCNN-TensorFlow/releases/download/1.1/FSRCNNX_x2_16-0-4-1.glsl",
    ),
];

/// Where downloaded and user shaders live: `~/.local/share/numapilot/shaders`.
pub fn shader_dir() -> std::path::PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".local/share")))
        .unwrap_or_else(|| "/tmp".into());
    base.join("numapilot/shaders")
}

/// Every model id with its name, shipped and downloadable.
pub fn model_names() -> Vec<(&'static str, &'static str)> {
    MODELS
        .iter()
        .map(|m| (m.0, m.1))
        .chain(DOWNLOADS.iter().map(|m| (m.0, m.1)))
        .collect()
}

/// The shader of a model: shipped, downloaded (`shader_dir()`), or a path to an mpv user shader
/// (`/…` or `~/…`, ending in `.glsl`). None when it is unknown or not downloaded yet.
pub fn model_source(id: &str) -> Option<std::borrow::Cow<'static, str>> {
    if let Some(m) = MODELS.iter().find(|m| m.0 == id) {
        return Some(m.2.into());
    }
    let path = if let Some(d) = DOWNLOADS.iter().find(|m| m.0 == id) {
        shader_dir().join(d.2)
    } else if id.ends_with(".glsl") && (id.starts_with('/') || id.starts_with("~/")) {
        match id.strip_prefix("~/") {
            Some(rest) => std::path::Path::new(&std::env::var_os("HOME")?).join(rest),
            None => id.into(),
        }
    } else {
        return None;
    };
    let src = std::fs::read_to_string(path).ok()?;
    (!parse_hooks(&src).is_empty()).then_some(src.into())
}

/// Downloads a model of `DOWNLOADS` into `shader_dir()` (curl). Returns the file.
pub fn fetch(id: &str) -> Result<std::path::PathBuf> {
    let Some(d) = DOWNLOADS.iter().find(|m| m.0 == id) else {
        bail!("{id}: not a downloadable model");
    };
    let dir = shader_dir();
    std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
    let file = dir.join(d.2);
    let part = dir.join(format!("{}.part", d.2));
    let st = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "120", "-o"])
        .arg(&part)
        .arg(d.3)
        .status()
        .context("curl")?;
    if !st.success() {
        let _ = std::fs::remove_file(&part);
        bail!("curl {}: {st}", d.3);
    }
    let src = std::fs::read_to_string(&part).unwrap_or_default();
    if parse_hooks(&src).is_empty() {
        let _ = std::fs::remove_file(&part);
        bail!("{}: not an mpv user shader", d.3);
    }
    std::fs::rename(&part, &file)?;
    Ok(file)
}

// ---------------------------------------------------------------------------------------------
// mpv hooks.

#[derive(Debug, Clone, PartialEq)]
pub struct Pass {
    pub desc: String,
    /// The texture the pass hooks (MAIN, or LUMA for luma models).
    pub hook: String,
    pub binds: Vec<String>,
    pub save: String,
    pub width: String,
    pub height: String,
    /// `//!COMPUTE bw bh [tw th]`: a compute pass writing a bw×bh block per workgroup of
    /// tw×th threads (it calls `imageStore` itself).
    pub compute: Option<[u32; 4]>,
    /// Everything after the `//!` lines: defines and `vec4 hook()` (`void hook()` in compute).
    pub body: String,
}

/// RGB → luma before a luma model (BT.709 weights on the encoded values, like mpv's Y').
const TO_LUMA: &str = "//!DESC rgb to luma
//!HOOK MAIN
//!BIND MAIN
//!SAVE LUMA
vec4 hook() {
    return vec4(dot(MAIN_tex(MAIN_pos).rgb, vec3(0.2126, 0.7152, 0.0722)), 0.0, 0.0, 1.0);
}
";

/// After a luma model: the bilinear RGB picture with its luma replaced by the model's.
const FROM_LUMA: &str = "//!DESC luma onto bilinear rgb
//!HOOK MAIN
//!BIND MAIN
//!BIND LUMA
//!SAVE MAIN
//!WIDTH LUMA.w
//!HEIGHT LUMA.h
vec4 hook() {
    vec4 c = MAIN_tex(MAIN_pos);
    float y = LUMA_tex(LUMA_pos).x;
    c.rgb = clamp(c.rgb + (y - dot(c.rgb, vec3(0.2126, 0.7152, 0.0722))), 0.0, 1.0);
    return vec4(c.rgb, 1.0);
}
";

fn parse_raw(src: &str) -> Vec<Pass> {
    let mut passes: Vec<Pass> = vec![];
    let mut cur: Option<Pass> = None;
    let mut in_header = false;
    for line in src.lines() {
        if let Some(d) = line.strip_prefix("//!") {
            let (k, v) = d.split_once(' ').unwrap_or((d, ""));
            let v = v.trim().to_string();
            if !in_header {
                if let Some(p) = cur.take() {
                    passes.push(p);
                }
                cur = Some(Pass {
                    desc: String::new(),
                    hook: String::new(),
                    binds: vec![],
                    save: String::new(),
                    width: String::new(),
                    height: String::new(),
                    compute: None,
                    body: String::new(),
                });
                in_header = true;
            }
            let p = cur.as_mut().expect("pass");
            match k {
                "DESC" => p.desc = v,
                "HOOK" => p.hook = v,
                "BIND" => p.binds.push(v),
                "SAVE" => p.save = v,
                "WIDTH" => p.width = v,
                "HEIGHT" => p.height = v,
                "COMPUTE" => {
                    let n: Vec<u32> = v
                        .split_whitespace()
                        .filter_map(|x| x.parse().ok())
                        .collect();
                    p.compute = match n[..] {
                        [bw, bh, tw, th] => Some([bw, bh, tw, th]),
                        [bw, bh] => Some([bw, bh, bw, bh]),
                        _ => None,
                    };
                }
                _ => {}
            }
            continue;
        }
        in_header = false;
        if let Some(p) = cur.as_mut() {
            if line.starts_with("//") && p.body.is_empty() {
                continue;
            }
            p.body.push_str(line);
            p.body.push('\n');
        }
    }
    passes.extend(cur);
    for p in &mut passes {
        if p.hook.is_empty() {
            p.hook = "MAIN".into();
        }
        if p.save.is_empty() {
            p.save = p.hook.clone();
        }
    }
    passes
}

/// Splits a user shader into its passes. A luma model (passes hooking LUMA, like FSRCNNX and
/// ArtCNN) gets a pass that makes LUMA from the RGB picture before it and one that puts the
/// doubled luma back onto a bilinear RGB picture after it, so every model goes MAIN → MAIN.
pub fn parse_hooks(src: &str) -> Vec<Pass> {
    let mut passes = parse_raw(src);
    if let Some(last) = passes.iter().rposition(|p| p.hook == "LUMA") {
        // MAIN passes after the luma model (the temporal pass) run on its RGB result
        let after: Vec<Pass> = passes
            .split_off(last + 1)
            .into_iter()
            .filter(|p| p.hook == "MAIN")
            .collect();
        passes.retain(|p| p.hook == "LUMA");
        let mut all = parse_raw(TO_LUMA);
        all.append(&mut passes);
        all.extend(parse_raw(FROM_LUMA));
        all.extend(after);
        return all;
    }
    passes.retain(|p| p.hook == "MAIN");
    passes
}

/// The temporal pass (`shaders/temporal`, numapilot's own): block matching on the game's frames
/// and the previous output reprojected along it, clamped and blended (docs/PRESENT-CHAIN.md, M3).
pub const TEMPORAL: &str = include_str!("../shaders/temporal/temporal.glsl");

/// A model's shader with the temporal pass after it.
pub fn with_temporal(model: &str) -> String {
    format!("{model}\n{TEMPORAL}")
}

pub const FRAMEGEN: &str = include_str!("../shaders/temporal/fg.glsl");

/// A model's shader with the temporal pass and the frame generation pass (texture `FG`: the
/// frame halfway between the previous output and this one).
pub fn with_framegen(model: &str) -> String {
    format!("{model}\n{TEMPORAL}\n{FRAMEGEN}")
}

/// Textures kept from one frame to the next: (kept as, copied from at the end of the frame).
/// `INPUT` is the frame the passes started from, `MAIN` the final output.
pub fn history(sizes: &BTreeMap<String, (u32, u32)>) -> Vec<(&'static str, &'static str)> {
    [("PREV_IN", "INPUT"), ("PREV_OUT", "MAIN")]
        .into_iter()
        .filter(|(k, _)| sizes.contains_key(*k))
        .collect()
}

/// Evaluates mpv's size expressions (reverse Polish: `conv2d_tf.w 2 *`).
pub fn eval_size(expr: &str, sizes: &BTreeMap<String, (u32, u32)>) -> Option<u32> {
    let mut st: Vec<f64> = vec![];
    for t in expr.split_whitespace() {
        if let Some((tex, c)) = t.rsplit_once('.')
            && (c == "w" || c == "h")
            && !tex.chars().all(|ch| ch.is_ascii_digit())
        {
            let (w, h) = sizes.get(tex)?;
            st.push(if c == "w" { *w } else { *h } as f64);
        } else if let Ok(n) = t.parse::<f64>() {
            st.push(n);
        } else {
            let b = st.pop()?;
            let a = st.pop()?;
            st.push(match t {
                "*" => a * b,
                "/" => a / b,
                "+" => a + b,
                "-" => a - b,
                _ => return None,
            });
        }
    }
    let v = st.pop()?;
    (st.is_empty() && v >= 1.0).then_some(v.round() as u32)
}

/// One pass ready to compile: the GLSL compute shader and its sizes.
#[derive(Debug, Clone)]
pub struct Planned {
    pub desc: String,
    pub glsl: String,
    /// Output texture and size.
    pub save: String,
    pub out: (u32, u32),
    /// Bound textures (binding 1, 2, … in this order), by their real names (HOOKED resolved).
    pub binds: Vec<String>,
    /// Workgroups to dispatch.
    pub groups: (u32, u32),
}

pub const GROUP: u32 = 8;

/// Reads `[a][b]…` at the start of `s`: the bracket contents and the length read.
fn brackets(s: &str) -> (Vec<String>, usize) {
    let mut out = vec![];
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() && b[i] == b'[' {
        let mut depth = 0;
        let start = i + 1;
        let mut end = None;
        for (j, &c) in b.iter().enumerate().skip(i) {
            match c {
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(j);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(e) = end else { break };
        out.push(s[start..e].to_string());
        i = e + 1;
    }
    (out, i)
}

/// naga's GLSL front end reverses the dimensions of multi-dimensional arrays (`float a[2][3]`
/// becomes 3 arrays of 2), which breaks the shared tiles of compute hooks: they become flat
/// arrays with the index computed by hand.
fn flatten_shared(src: &str) -> String {
    let mut arrays: Vec<(String, Vec<String>)> = vec![];
    let mut s = String::new();
    for line in src.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("shared ")
            && let Some((ty, decl)) = rest.split_once(' ')
            && let Some(open) = decl.find('[')
        {
            let name = decl[..open].trim().to_string();
            let (dims, _) = brackets(&decl[open..]);
            if dims.len() >= 2 {
                let total: Vec<String> = dims.iter().map(|d| format!("({d})")).collect();
                s.push_str(&format!("shared {ty} {name}[{}];\n", total.join(" * ")));
                arrays.push((name, dims));
                continue;
            }
        }
        s.push_str(line);
        s.push('\n');
    }
    for (name, dims) in &arrays {
        let mut out = String::new();
        let mut rest = s.as_str();
        let pat = format!("{name}[");
        while let Some(at) = rest.find(&pat) {
            let before_ok = rest[..at]
                .chars()
                .last()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            out.push_str(&rest[..at]);
            let after = &rest[at + name.len()..];
            let (idx, len) = brackets(after);
            if !before_ok || idx.len() != dims.len() {
                out.push_str(&pat);
                rest = &after[1..];
                continue;
            }
            let mut e = format!("({})", idx[0]);
            for (i, d) in idx.iter().zip(dims.iter()).skip(1) {
                e = format!("({e}) * ({d}) + ({i})");
            }
            out.push_str(&format!("{name}[{e}]"));
            rest = &after[len..];
        }
        out.push_str(rest);
        s = out;
    }
    s
}

/// Rewrites what a compute hook does with images into the buffer API of `plan`:
/// `texelFetch(X_raw, p, 0)` → `X_fetch0(p, 0)` (zero outside, like the padding the networks
/// were trained with), `imageStore(out_image, p, v)` → `out_store(p, v)`, `gl_WorkGroupSize` →
/// a constant, function-local `const` → plain locals (naga wants constant initializers), and
/// no float16 extension (the shaders fall back to 32-bit floats).
fn compute_body(body: &str, binds: &[String], tw: u32, th: u32) -> String {
    let mut b = body.replace("imageStore(out_image,", "out_store(");
    for n in binds {
        b = b.replace(&format!("texelFetch({n}_raw,"), &format!("{n}_fetch0("));
    }
    b = b.replace("gl_WorkGroupSize", &format!("uvec3({tw}u, {th}u, 1u)"));
    let mut out = String::new();
    for line in b.lines() {
        if line.trim_start().starts_with("#extension") {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        let t = line.trim_start();
        if indented && let Some(rest) = t.strip_prefix("const ") {
            out.push_str(&line[..line.len() - t.len()]);
            out.push_str(rest);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    flatten_shared(&out)
}

/// Turns the passes into compute shaders for an input of w×h. Returns the passes and the size of
/// every texture (MAIN at the end = the output).
pub fn plan(
    passes: &[Pass],
    w: u32,
    h: u32,
) -> Result<(Vec<Planned>, BTreeMap<String, (u32, u32)>)> {
    let mut sizes = BTreeMap::new();
    sizes.insert("MAIN".to_string(), (w, h));
    sizes.insert("INPUT".to_string(), (w, h));
    let mut out = vec![];
    for p in passes {
        // the previous frame's textures, the first time a pass asks for them
        for b in &p.binds {
            let size = match b.as_str() {
                "PREV_IN" => Some((w, h)),
                "PREV_OUT" => sizes.get("MAIN").copied(),
                _ => None,
            };
            if let Some(sz) = size {
                sizes.entry(b.clone()).or_insert(sz);
            }
        }
        let Some(&hooked) = sizes.get(&p.hook) else {
            bail!("{}: hooks '{}' before it exists", p.desc, p.hook);
        };
        let dim = |e: &str, i: usize| -> Result<u32> {
            if e.is_empty() {
                return Ok(if i == 0 { hooked.0 } else { hooked.1 });
            }
            eval_size(e, &sizes).with_context(|| format!("{}: size '{e}'", p.desc))
        };
        let ow = dim(&p.width, 0)?;
        let oh = dim(&p.height, 1)?;
        let (tw, th) = match p.compute {
            Some([_, _, tw, th]) => (tw, th),
            None => (GROUP, GROUP),
        };
        let groups = match p.compute {
            Some([bw, bh, _, _]) => (ow.div_ceil(bw), oh.div_ceil(bh)),
            None => (ow.div_ceil(GROUP), oh.div_ceil(GROUP)),
        };
        let real = |b: &String| {
            if b == "HOOKED" {
                p.hook.clone()
            } else {
                b.clone()
            }
        };
        let mut g = String::from("#version 450\n");
        g.push_str(&format!(
            "layout(local_size_x = {tw}, local_size_y = {th}, local_size_z = 1) in;\n"
        ));
        g.push_str("layout(std430, set = 0, binding = 0) buffer Out_ { vec4 out_d[]; };\n");
        for (i, b) in p.binds.iter().enumerate() {
            if !sizes.contains_key(&real(b)) {
                bail!("{}: texture '{b}' is bound before it is saved", p.desc);
            }
            g.push_str(&format!(
                "layout(std430, set = 0, binding = {}) buffer {b}_B {{ vec4 {b}_d[]; }};\n",
                i + 1
            ));
        }
        g.push_str("vec2 cur_pos;\n");
        g.push_str(&format!(
            "const vec2 target_size = vec2({ow}.0, {oh}.0);
void out_store(ivec2 p, vec4 v) {{
    if (p.x >= 0 && p.y >= 0 && p.x < {ow} && p.y < {oh}) {{
        out_d[p.y * {ow} + p.x] = v;
    }}
}}
"
        ));
        for b in &p.binds {
            let (bw, bh) = sizes[&real(b)];
            g.push_str(&format!(
                "const vec2 {b}_size = vec2({bw}.0, {bh}.0);
const vec2 {b}_pt = vec2(1.0 / {bw}.0, 1.0 / {bh}.0);
#define {b}_pos cur_pos
#define {b}_mul 1.0
vec4 {b}_fetch(ivec2 p) {{
    p = clamp(p, ivec2(0, 0), ivec2({mw}, {mh}));
    return {b}_d[p.y * {bw} + p.x];
}}
vec4 {b}_fetch0(ivec2 p, int lod) {{
    if (p.x < 0 || p.y < 0 || p.x > {mw} || p.y > {mh}) {{
        return vec4(0.0);
    }}
    return {b}_d[p.y * {bw} + p.x];
}}
vec4 {b}_tex(vec2 pos) {{
    vec2 t = pos * {b}_size - vec2(0.5, 0.5);
    vec2 fl = floor(t);
    vec2 f = t - fl;
    ivec2 i = ivec2(fl);
    vec4 a = mix({b}_fetch(i), {b}_fetch(i + ivec2(1, 0)), f.x);
    vec4 c = mix({b}_fetch(i + ivec2(0, 1)), {b}_fetch(i + ivec2(1, 1)), f.x);
    return mix(a, c, f.y);
}}
vec4 {b}_texOff_(vec2 off) {{
    return {b}_fetch(ivec2(floor(cur_pos * {b}_size)) + ivec2(off));
}}
#define {b}_texOff(off) {b}_texOff_(vec2(off))
",
                mw = bw - 1,
                mh = bh - 1
            ));
        }
        if p.compute.is_some() {
            g.push_str(&compute_body(&p.body, &p.binds, tw, th));
            g.push_str(&format!(
                "void main() {{
    cur_pos = (vec2(gl_GlobalInvocationID.xy) + vec2(0.5, 0.5)) / target_size;
    hook();
}}
"
            ));
        } else {
            g.push_str(&p.body);
            g.push_str(&format!(
                "void main() {{
    ivec2 gid = ivec2(gl_GlobalInvocationID.xy);
    if (gid.x >= {ow} || gid.y >= {oh}) {{
        return;
    }}
    cur_pos = (vec2(gid) + vec2(0.5, 0.5)) / target_size;
    out_d[gid.y * {ow} + gid.x] = hook();
}}
"
            ));
        }
        sizes.insert(p.save.clone(), (ow, oh));
        if p.save == "MAIN"
            && let Some(&prev) = sizes.get("PREV_OUT")
            && prev != (ow, oh)
        {
            bail!(
                "{}: the output changes size after PREV_OUT was read ({}x{} → {ow}x{oh})",
                p.desc,
                prev.0,
                prev.1
            );
        }
        out.push(Planned {
            desc: p.desc.clone(),
            glsl: g,
            save: p.save.clone(),
            out: (ow, oh),
            binds: p.binds.iter().map(real).collect(),
            groups,
        });
    }
    Ok((out, sizes))
}

/// naga reads GLSL's `barrier()` as a barrier on every scope, subgroups included, which needs
/// the subgroup capability; in a compute shader it means the workgroup (shared memory and
/// buffers), so that is what it becomes.
fn workgroup_barriers(b: &mut naga::Block) {
    use naga::{Barrier, Statement as S};
    for st in b.iter_mut() {
        match st {
            S::ControlBarrier(x) | S::MemoryBarrier(x) => {
                *x = Barrier::WORK_GROUP | Barrier::STORAGE;
            }
            S::Block(inner) => workgroup_barriers(inner),
            S::If { accept, reject, .. } => {
                workgroup_barriers(accept);
                workgroup_barriers(reject);
            }
            S::Switch { cases, .. } => {
                for c in cases.iter_mut() {
                    workgroup_barriers(&mut c.body);
                }
            }
            S::Loop {
                body, continuing, ..
            } => {
                workgroup_barriers(body);
                workgroup_barriers(continuing);
            }
            _ => {}
        }
    }
}

/// GLSL compute → SPIR-V words.
pub fn compile(glsl: &str) -> Result<Vec<u32>> {
    use naga::back::spv;
    use naga::front::glsl;
    let mut fe = glsl::Frontend::default();
    let opts = glsl::Options::from(naga::ShaderStage::Compute);
    let mut module = fe
        .parse(&opts, glsl)
        .map_err(|e| anyhow!("GLSL: {}", e.emit_to_string(glsl)))?;
    for (_, f) in module.functions.iter_mut() {
        workgroup_barriers(&mut f.body);
    }
    for e in module.entry_points.iter_mut() {
        workgroup_barriers(&mut e.function.body);
    }
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| anyhow!("validation: {}", e.emit_to_string(glsl)))?;
    let popts = spv::PipelineOptions {
        shader_stage: naga::ShaderStage::Compute,
        entry_point: "main".into(),
    };
    spv::write_vec(&module, &info, &spv::Options::default(), Some(&popts))
        .map_err(|e| anyhow!("SPIR-V: {e}"))
}

// ---------------------------------------------------------------------------------------------
// Vulkan.

/// The first Vulkan device the environment leaves visible (the bench restricts it to one card
/// with `fgbench::card_env`).
pub struct Ctx {
    _entry: ash::Entry,
    instance: ash::Instance,
    pdev: vk::PhysicalDevice,
    dev: ash::Device,
    queue: vk::Queue,
    qfi: u32,
    mem: vk::PhysicalDeviceMemoryProperties,
    /// Nanoseconds per timestamp tick.
    ts_period: f64,
    pub name: String,
}

impl Ctx {
    pub fn open() -> Result<Ctx> {
        let entry = unsafe { ash::Entry::load() }.map_err(|e| anyhow!("libvulkan: {e}"))?;
        let app = vk::ApplicationInfo::default()
            .application_name(c"numapilot-present-bench")
            .api_version(vk::API_VERSION_1_1);
        let instance = unsafe {
            entry.create_instance(
                &vk::InstanceCreateInfo::default().application_info(&app),
                None,
            )
        }
        .context("vkCreateInstance")?;
        let devs = unsafe { instance.enumerate_physical_devices() }?;
        let Some(&pdev) = devs.first() else {
            unsafe { instance.destroy_instance(None) };
            bail!("no Vulkan device");
        };
        let props = unsafe { instance.get_physical_device_properties(pdev) };
        let name = props
            .device_name_as_c_str()
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_default();
        let qf = unsafe { instance.get_physical_device_queue_family_properties(pdev) };
        let qfi = qf
            .iter()
            .position(|q| {
                q.queue_flags.contains(vk::QueueFlags::COMPUTE) && q.timestamp_valid_bits > 0
            })
            .context("no compute queue with timestamps")? as u32;
        let prio = [1.0f32];
        let qci = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(qfi)
            .queue_priorities(&prio)];
        let dev = unsafe {
            instance.create_device(
                pdev,
                &vk::DeviceCreateInfo::default().queue_create_infos(&qci),
                None,
            )
        }
        .context("vkCreateDevice")?;
        let queue = unsafe { dev.get_device_queue(qfi, 0) };
        let mem = unsafe { instance.get_physical_device_memory_properties(pdev) };
        Ok(Ctx {
            _entry: entry,
            instance,
            pdev,
            dev,
            queue,
            qfi,
            mem,
            ts_period: props.limits.timestamp_period as f64,
            name,
        })
    }

    /// PCI address of the device, when the driver tells it (VK_EXT_pci_bus_info).
    pub fn pci(&self) -> Option<String> {
        let exts = unsafe {
            self.instance
                .enumerate_device_extension_properties(self.pdev)
        }
        .ok()?;
        let has = exts.iter().any(|e| {
            e.extension_name_as_c_str()
                .is_ok_and(|n| n == ash::ext::pci_bus_info::NAME)
        });
        if !has {
            return None;
        }
        let mut pci = vk::PhysicalDevicePCIBusInfoPropertiesEXT::default();
        let mut p2 = vk::PhysicalDeviceProperties2::default().push_next(&mut pci);
        unsafe {
            self.instance
                .get_physical_device_properties2(self.pdev, &mut p2)
        };
        Some(format!(
            "{:04x}:{:02x}:{:02x}.{:x}",
            pci.pci_domain, pci.pci_bus, pci.pci_device, pci.pci_function
        ))
    }

    fn mem_type(&self, bits: u32, want: vk::MemoryPropertyFlags) -> Option<u32> {
        (0..self.mem.memory_type_count).find(|&i| {
            bits & (1 << i) != 0
                && self.mem.memory_types[i as usize]
                    .property_flags
                    .contains(want)
        })
    }

    fn buffer(&self, size: u64, usage: vk::BufferUsageFlags, host: bool) -> Result<Buf> {
        let b = unsafe {
            self.dev.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(size)
                    .usage(usage)
                    .sharing_mode(vk::SharingMode::EXCLUSIVE),
                None,
            )
        }?;
        let req = unsafe { self.dev.get_buffer_memory_requirements(b) };
        let want = if host {
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
        } else {
            vk::MemoryPropertyFlags::DEVICE_LOCAL
        };
        let mt = self
            .mem_type(req.memory_type_bits, want)
            .or_else(|| self.mem_type(req.memory_type_bits, vk::MemoryPropertyFlags::empty()))
            .context("no memory type")?;
        let m = unsafe {
            self.dev.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(mt),
                None,
            )
        }
        .context("vkAllocateMemory")?;
        unsafe { self.dev.bind_buffer_memory(b, m, 0) }?;
        Ok(Buf { b, m, size })
    }
}

impl Drop for Ctx {
    fn drop(&mut self) {
        unsafe {
            let _ = self.dev.device_wait_idle();
            self.dev.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

struct Buf {
    b: vk::Buffer,
    m: vk::DeviceMemory,
    size: u64,
}

/// Times of one chain: per pass and the whole chain (ms, median of the runs).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Timing {
    pub passes: Vec<(String, f64)>,
    pub total_ms: f64,
    pub out: (u32, u32),
}

/// Runs a user shader on an input of w×h `runs` times. With `readback`, also returns the output
/// pixels (RGBA, f32) of the last run.
pub fn run(
    ctx: &Ctx,
    shader: &str,
    w: u32,
    h: u32,
    runs: u32,
    input: Option<&[f32]>,
    readback: bool,
) -> Result<(Timing, Vec<f32>)> {
    let frames: Vec<&[f32]> = input.into_iter().collect();
    run_frames(ctx, shader, w, h, runs, &frames, readback.then_some("MAIN"))
}

/// `run` over a sequence of input frames (frame i of the runs is `frames[i % len]`; none = flat
/// grey), keeping the history textures (`history`) from one frame to the next. `read` returns
/// that texture after the last frame.
pub fn run_frames(
    ctx: &Ctx,
    shader: &str,
    w: u32,
    h: u32,
    runs: u32,
    frames: &[&[f32]],
    read: Option<&str>,
) -> Result<(Timing, Vec<f32>)> {
    let input = frames.first().copied();
    let passes = parse_hooks(shader);
    if passes.is_empty() {
        bail!("no passes in the shader");
    }
    let (planned, sizes) = plan(&passes, w, h)?;
    let spirv: Vec<Vec<u32>> = planned
        .iter()
        .map(|p| compile(&p.glsl).with_context(|| p.desc.clone()))
        .collect::<Result<_>>()?;
    let d = &ctx.dev;
    let usage = vk::BufferUsageFlags::STORAGE_BUFFER
        | vk::BufferUsageFlags::TRANSFER_DST
        | vk::BufferUsageFlags::TRANSFER_SRC;
    // one buffer per version of a texture: MAIN in, then every SAVE (a SAVE MAIN is the output)
    let mut bufs: Vec<Buf> = vec![];
    let px = |(w, h): (u32, u32)| w as u64 * h as u64 * 16;
    bufs.push(ctx.buffer(px((w, h)), usage, false)?);
    let mut cur: BTreeMap<String, usize> = BTreeMap::new();
    cur.insert("MAIN".into(), 0);
    cur.insert("INPUT".into(), 0);
    let kept = history(&sizes);
    for (k, _) in &kept {
        bufs.push(ctx.buffer(px(sizes[*k]), usage, false)?);
        cur.insert(k.to_string(), bufs.len() - 1);
    }
    let mut wiring: Vec<(usize, Vec<usize>)> = vec![];
    for p in &planned {
        let ins: Vec<usize> = p.binds.iter().map(|b| cur[b]).collect();
        bufs.push(ctx.buffer(px(p.out), usage, false)?);
        let o = bufs.len() - 1;
        cur.insert(p.save.clone(), o);
        wiring.push((o, ins));
    }
    // (kept buffer, buffer it is copied from at the end of each frame)
    let copies: Vec<(usize, usize)> = kept.iter().map(|(k, src)| (cur[*k], cur[*src])).collect();
    let readback = read.is_some();
    let read = read.unwrap_or("MAIN");
    let Some(&out_idx) = cur.get(read) else {
        bail!("no texture '{read}'");
    };
    let out_size = sizes[read];

    // descriptors and pipelines
    let max_b = planned.iter().map(|p| p.binds.len() + 1).max().unwrap_or(1) as u32;
    let pool = unsafe {
        d.create_descriptor_pool(
            &vk::DescriptorPoolCreateInfo::default()
                .max_sets(planned.len() as u32)
                .pool_sizes(&[vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::STORAGE_BUFFER,
                    descriptor_count: max_b * planned.len() as u32,
                }]),
            None,
        )
    }?;
    let mut layouts = vec![];
    let mut playouts = vec![];
    let mut pipes = vec![];
    let mut sets = vec![];
    let mut modules = vec![];
    for (i, p) in planned.iter().enumerate() {
        let binds: Vec<vk::DescriptorSetLayoutBinding> = (0..=p.binds.len() as u32)
            .map(|b| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(b)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE)
            })
            .collect();
        let dsl = unsafe {
            d.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&binds),
                None,
            )
        }?;
        layouts.push(dsl);
        let pl = unsafe {
            d.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&[dsl]),
                None,
            )
        }?;
        playouts.push(pl);
        let m = unsafe {
            d.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&spirv[i]), None)
        }?;
        modules.push(m);
        let stage = vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::COMPUTE)
            .module(m)
            .name(c"main");
        let pipe = unsafe {
            d.create_compute_pipelines(
                vk::PipelineCache::null(),
                &[vk::ComputePipelineCreateInfo::default()
                    .stage(stage)
                    .layout(pl)],
                None,
            )
        }
        .map_err(|(_, e)| anyhow!("{}: vkCreateComputePipelines: {e}", p.desc))?[0];
        pipes.push(pipe);
        let set = unsafe {
            d.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&[dsl]),
            )
        }?[0];
        let (o, ins) = &wiring[i];
        let infos: Vec<[vk::DescriptorBufferInfo; 1]> = std::iter::once(*o)
            .chain(ins.iter().copied())
            .map(|bi| {
                [vk::DescriptorBufferInfo::default()
                    .buffer(bufs[bi].b)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)]
            })
            .collect();
        let writes: Vec<vk::WriteDescriptorSet> = infos
            .iter()
            .enumerate()
            .map(|(b, info)| {
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(b as u32)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(info)
            })
            .collect();
        unsafe { d.update_descriptor_sets(&writes, &[]) };
        sets.push(set);
    }

    // input
    let staging_in = match input {
        Some(px_in) => {
            let s = ctx.buffer(bufs[0].size, vk::BufferUsageFlags::TRANSFER_SRC, true)?;
            unsafe {
                let ptr = d.map_memory(s.m, 0, s.size, vk::MemoryMapFlags::empty())? as *mut f32;
                let n = (s.size / 4) as usize;
                std::ptr::copy_nonoverlapping(px_in.as_ptr(), ptr, px_in.len().min(n));
                d.unmap_memory(s.m);
            }
            Some(s)
        }
        None => None,
    };
    let readback_buf = if readback {
        Some(ctx.buffer(bufs[out_idx].size, vk::BufferUsageFlags::TRANSFER_DST, true)?)
    } else {
        None
    };

    let n_ts = planned.len() as u32 + 1;
    let qpool = unsafe {
        d.create_query_pool(
            &vk::QueryPoolCreateInfo::default()
                .query_type(vk::QueryType::TIMESTAMP)
                .query_count(n_ts),
            None,
        )
    }?;
    let cpool = unsafe {
        d.create_command_pool(
            &vk::CommandPoolCreateInfo::default().queue_family_index(ctx.qfi),
            None,
        )
    }?;
    let cb = unsafe {
        d.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default()
                .command_pool(cpool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1),
        )
    }?[0];
    let barrier = |cb: vk::CommandBuffer,
                   src: vk::AccessFlags,
                   dst: vk::AccessFlags,
                   sstage,
                   dstage| unsafe {
        d.cmd_pipeline_barrier(
            cb,
            sstage,
            dstage,
            vk::DependencyFlags::empty(),
            &[vk::MemoryBarrier::default()
                .src_access_mask(src)
                .dst_access_mask(dst)],
            &[],
            &[],
        )
    };
    unsafe {
        d.begin_command_buffer(cb, &vk::CommandBufferBeginInfo::default())?;
        match &staging_in {
            Some(s) => d.cmd_copy_buffer(
                cb,
                s.b,
                bufs[0].b,
                &[vk::BufferCopy::default().size(bufs[0].size)],
            ),
            // 0.5 in every channel
            None => d.cmd_fill_buffer(cb, bufs[0].b, 0, vk::WHOLE_SIZE, 0x3f00_0000),
        }
        barrier(
            cb,
            vk::AccessFlags::TRANSFER_WRITE,
            vk::AccessFlags::SHADER_READ,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::COMPUTE_SHADER,
        );
        d.cmd_reset_query_pool(cb, qpool, 0, n_ts);
        d.cmd_write_timestamp(cb, vk::PipelineStageFlags::TOP_OF_PIPE, qpool, 0);
        for (i, p) in planned.iter().enumerate() {
            d.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, pipes[i]);
            d.cmd_bind_descriptor_sets(
                cb,
                vk::PipelineBindPoint::COMPUTE,
                playouts[i],
                0,
                &[sets[i]],
                &[],
            );
            d.cmd_dispatch(cb, p.groups.0, p.groups.1, 1);
            barrier(
                cb,
                vk::AccessFlags::SHADER_WRITE,
                vk::AccessFlags::SHADER_READ,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
            );
            d.cmd_write_timestamp(
                cb,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                qpool,
                i as u32 + 1,
            );
        }
        if !copies.is_empty() {
            // this frame's input and output become the next frame's history
            barrier(
                cb,
                vk::AccessFlags::SHADER_WRITE | vk::AccessFlags::SHADER_READ,
                vk::AccessFlags::TRANSFER_READ | vk::AccessFlags::TRANSFER_WRITE,
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::TRANSFER,
            );
            for &(dst, src) in &copies {
                d.cmd_copy_buffer(
                    cb,
                    bufs[src].b,
                    bufs[dst].b,
                    &[vk::BufferCopy::default().size(bufs[dst].size)],
                );
            }
        }
        if let Some(rb) = &readback_buf {
            barrier(
                cb,
                vk::AccessFlags::SHADER_WRITE | vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::TRANSFER_READ,
                vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER,
            );
            d.cmd_copy_buffer(
                cb,
                bufs[out_idx].b,
                rb.b,
                &[vk::BufferCopy::default().size(rb.size)],
            );
        }
        d.end_command_buffer(cb)?;
    }
    let fence = unsafe { d.create_fence(&vk::FenceCreateInfo::default(), None) }?;
    // the history starts black
    if !copies.is_empty() {
        let init = unsafe {
            d.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(cpool)
                    .level(vk::CommandBufferLevel::PRIMARY)
                    .command_buffer_count(1),
            )
        }?[0];
        unsafe {
            d.begin_command_buffer(init, &vk::CommandBufferBeginInfo::default())?;
            for &(dst, _) in &copies {
                d.cmd_fill_buffer(init, bufs[dst].b, 0, vk::WHOLE_SIZE, 0);
            }
            d.end_command_buffer(init)?;
            let cbs = [init];
            d.queue_submit(
                ctx.queue,
                &[vk::SubmitInfo::default().command_buffers(&cbs)],
                fence,
            )?;
            d.wait_for_fences(&[fence], true, 60_000_000_000)?;
            d.reset_fences(&[fence])?;
        }
    }
    let mut per_pass: Vec<Vec<f64>> = vec![vec![]; planned.len()];
    let mut totals = vec![];
    let runs = runs.max(1);
    let mut result = Ok(());
    for run in 0..runs as usize + 1 {
        // the next input frame into the staging buffer
        if frames.len() > 1
            && let Some(st) = &staging_in
        {
            let f = frames[run % frames.len()];
            unsafe {
                let ptr = d.map_memory(st.m, 0, st.size, vk::MemoryMapFlags::empty())? as *mut f32;
                std::ptr::copy_nonoverlapping(f.as_ptr(), ptr, f.len().min((st.size / 4) as usize));
                d.unmap_memory(st.m);
            }
        }
        let cbs = [cb];
        let r = unsafe {
            d.queue_submit(
                ctx.queue,
                &[vk::SubmitInfo::default().command_buffers(&cbs)],
                fence,
            )
            .and_then(|_| d.wait_for_fences(&[fence], true, 60_000_000_000))
            .and_then(|_| d.reset_fences(&[fence]))
        };
        if let Err(e) = r {
            result = Err(anyhow!("vkQueueSubmit: {e}"));
            break;
        }
        let mut ts = vec![0u64; n_ts as usize];
        if let Err(e) = unsafe {
            d.get_query_pool_results(
                qpool,
                0,
                &mut ts,
                vk::QueryResultFlags::TYPE_64 | vk::QueryResultFlags::WAIT,
            )
        } {
            result = Err(anyhow!("timestamps: {e}"));
            break;
        }
        let ms = |a: u64, b: u64| b.wrapping_sub(a) as f64 * ctx.ts_period / 1e6;
        for i in 0..planned.len() {
            per_pass[i].push(ms(ts[i], ts[i + 1]));
        }
        totals.push(ms(ts[0], ts[planned.len()]));
    }
    let mut pixels = vec![];
    if result.is_ok()
        && let Some(rb) = &readback_buf
    {
        unsafe {
            let ptr = d.map_memory(rb.m, 0, rb.size, vk::MemoryMapFlags::empty())? as *const f32;
            pixels = std::slice::from_raw_parts(ptr, (rb.size / 4) as usize).to_vec();
            d.unmap_memory(rb.m);
        }
    }
    unsafe {
        let _ = d.device_wait_idle();
        d.destroy_fence(fence, None);
        d.destroy_command_pool(cpool, None);
        d.destroy_query_pool(qpool, None);
        for &p in &pipes {
            d.destroy_pipeline(p, None);
        }
        for &m in &modules {
            d.destroy_shader_module(m, None);
        }
        for &l in &playouts {
            d.destroy_pipeline_layout(l, None);
        }
        for &l in &layouts {
            d.destroy_descriptor_set_layout(l, None);
        }
        d.destroy_descriptor_pool(pool, None);
        for b in bufs
            .iter()
            .chain(staging_in.iter())
            .chain(readback_buf.iter())
        {
            d.destroy_buffer(b.b, None);
            d.free_memory(b.m, None);
        }
    }
    result?;
    // the first run warms up (caches, clocks): left out
    let skip = usize::from(totals.len() > 1);
    let med = |v: &[f64]| -> f64 {
        let mut s = v[skip..].to_vec();
        s.sort_by(|a, b| a.total_cmp(b));
        s.get(s.len() / 2).copied().unwrap_or(0.0)
    };
    Ok((
        Timing {
            passes: planned
                .iter()
                .zip(&per_pass)
                .map(|(p, v)| (p.desc.clone(), med(v)))
                .collect(),
            total_ms: med(&totals),
            out: out_size,
        },
        pixels,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hooks_and_sizes() {
        for (name, _, src) in MODELS {
            let p = parse_hooks(src);
            assert!(!p.is_empty(), "{name}");
            let (planned, sizes) = plan(&p, 1280, 720).unwrap();
            assert_eq!(sizes["MAIN"], (2560, 1440), "{name}");
            assert_eq!(planned.last().unwrap().out, (2560, 1440));
        }
        let s = parse_hooks(ANIME4K_S);
        assert_eq!(s.len(), 5);
        assert_eq!(s[0].binds, ["MAIN"]);
        assert_eq!(s[4].save, "MAIN");
        assert_eq!(s[4].binds, ["MAIN", "conv2d_last_tf"]);
        let mut sz = BTreeMap::new();
        sz.insert("a".to_string(), (640, 360));
        assert_eq!(eval_size("a.w 2 *", &sz), Some(1280));
        assert_eq!(eval_size("a.h", &sz), Some(360));
        assert_eq!(eval_size("b.w", &sz), None);
        assert_eq!(eval_size("*", &sz), None);
    }

    #[test]
    fn luma_models_and_compute_hooks() {
        let p = parse_hooks(ARTCNN_C4F16);
        assert_eq!(p.first().unwrap().desc, "rgb to luma");
        assert_eq!(p.last().unwrap().desc, "luma onto bilinear rgb");
        assert!(p[1..p.len() - 1].iter().all(|x| x.hook == "LUMA"));
        assert_eq!(p[1].compute, Some([24, 32, 12, 16]));
        let (planned, sizes) = plan(&p, 1280, 720).unwrap();
        assert_eq!(sizes["LUMA"], (2560, 1440));
        assert_eq!(sizes["conv2d_6"], (1280, 720));
        assert_eq!(
            planned[1].groups,
            (2560u32.div_ceil(24), 1440u32.div_ceil(32))
        );
        assert!(planned[1].glsl.contains("LUMA_fetch0("));
        assert!(!planned[1].glsl.contains("imageStore"));
        // a pass without WIDTH/HEIGHT/SAVE writes its hooked texture at its size
        let src = "//!HOOK LUMA\n//!BIND LUMA\nvec4 hook() { return LUMA_texOff(0); }\n";
        let p = parse_hooks(src);
        assert_eq!(p.len(), 3);
        assert_eq!(p[1].save, "LUMA");
        let (_, sizes) = plan(&p, 64, 36).unwrap();
        assert_eq!(sizes["MAIN"], (64, 36));
        // naga reverses multi-dimensional arrays: shared tiles become flat
        let f = flatten_shared("shared V4 inp[4][isize.y][isize.x];\nx = inp[0][y][x + 1];\n");
        assert_eq!(
            f,
            "shared V4 inp[(4) * (isize.y) * (isize.x)];\n\
             x = inp[(((0)) * (isize.y) + (y)) * (isize.x) + (x + 1)];\n"
        );
        assert_eq!(model_source("artcnn-c4f32").as_deref(), Some(ARTCNN_C4F32));
        assert!(model_source("nonexistent").is_none());
        assert!(model_names().iter().any(|m| m.0 == "fsrcnnx"));
    }

    #[test]
    fn temporal_after_any_model() {
        for (name, _, src) in MODELS {
            let p = parse_hooks(&with_temporal(src));
            assert_eq!(
                p.last().unwrap().desc,
                "temporal: reproject and blend",
                "{name}"
            );
            let (planned, sizes) = plan(&p, 1280, 720).unwrap();
            assert_eq!(sizes["MAIN"], (2560, 1440), "{name}");
            assert_eq!(sizes["PREV_OUT"], (2560, 1440), "{name}");
            assert_eq!(sizes["PREV_IN"], (1280, 720), "{name}");
            assert_eq!(sizes["MOTION"], (160, 90), "{name}");
            assert_eq!(
                history(&sizes),
                [("PREV_IN", "INPUT"), ("PREV_OUT", "MAIN")],
                "{name}"
            );
            for q in &planned[planned.len() - 2..] {
                compile(&q.glsl).unwrap_or_else(|e| panic!("{name} / {}: {e:#}", q.desc));
            }
            // frame generation after it, at the output size
            let (planned, sizes) = plan(&parse_hooks(&with_framegen(src)), 1280, 720).unwrap();
            assert_eq!(sizes["FG"], (2560, 1440), "{name}");
            let q = planned.last().unwrap();
            assert_eq!(q.save, "FG");
            compile(&q.glsl).unwrap_or_else(|e| panic!("{name} / {}: {e:#}", q.desc));
        }
        // the luma models keep their wrapping, the temporal pass comes after
        let p = parse_hooks(&with_temporal(ARTCNN_C4F16));
        let i = p
            .iter()
            .position(|x| x.desc == "luma onto bilinear rgb")
            .unwrap();
        assert_eq!(p[i + 1].desc, "temporal: luma");
        assert!(history(&plan(&parse_hooks(ANIME4K_S), 64, 36).unwrap().1).is_empty());
    }

    #[test]
    fn every_pass_compiles() {
        for (name, _, src) in MODELS {
            let (planned, _) = plan(&parse_hooks(src), 64, 36).unwrap();
            for p in &planned {
                compile(&p.glsl).unwrap_or_else(|e| panic!("{name} / {}: {e:#}", p.desc));
            }
        }
    }

    /// Runs on a real Vulkan device when there is one (lavapipe is enough); skipped otherwise.
    #[test]
    fn runs_on_vulkan_when_there_is_one() {
        let Ok(ctx) = Ctx::open() else {
            eprintln!("no Vulkan device: skipped");
            return;
        };
        // bilinear of a constant picture is the same constant
        let (t, px) = run(&ctx, BILINEAR_X2, 16, 8, 2, None, true).unwrap();
        assert_eq!(t.out, (32, 16));
        assert_eq!(px.len(), 32 * 16 * 4);
        assert!(px.iter().all(|v| (v - 0.5).abs() < 1e-6), "{:?}", &px[..8]);
        // a horizontal ramp stays a ramp, twice as wide
        let mut ramp = vec![0f32; 16 * 8 * 4];
        for y in 0..8 {
            for x in 0..16 {
                for c in 0..4 {
                    ramp[(y * 16 + x) * 4 + c] = x as f32;
                }
            }
        }
        let (_, px) = run(&ctx, BILINEAR_X2, 16, 8, 1, Some(&ramp), true).unwrap();
        let at = |x: usize, y: usize| px[(y * 32 + x) * 4];
        assert!((at(5, 3) - 2.25).abs() < 1e-5, "{}", at(5, 3));
        assert!(at(0, 0).abs() < 1e-6 && (at(31, 0) - 15.0).abs() < 1e-6);
        // Anime4K S: residual on top of bilinear; a flat picture stays close to flat
        let (t, px) = run(&ctx, ANIME4K_S, 32, 16, 1, None, true).unwrap();
        assert_eq!(t.passes.len(), 5);
        assert!(px.iter().all(|v| v.is_finite()));
        let inner = px[(16 * 64 + 32) * 4];
        assert!((inner - 0.5).abs() < 0.2, "{inner}");
        // ArtCNN (luma, compute hooks with shared tiles) stays close to bilinear on a smooth picture
        let (w, h) = (40usize, 24usize);
        let mut pic = vec![1f32; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let v = 0.5 + 0.3 * (x as f32 * 0.2).sin() * (y as f32 * 0.15).cos();
                pic[(y * w + x) * 4] = v;
                pic[(y * w + x) * 4 + 1] = v * 0.8;
                pic[(y * w + x) * 4 + 2] = 1.0 - v;
            }
        }
        let (_, base) = run(&ctx, BILINEAR_X2, w as u32, h as u32, 1, Some(&pic), true).unwrap();
        let (_, px) = run(&ctx, ARTCNN_C4F16, w as u32, h as u32, 1, Some(&pic), true).unwrap();
        let mean = base
            .iter()
            .zip(&px)
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / base.len() as f32;
        assert!(mean < 0.02, "{mean}");
        // block matching finds a known shift: frame B is frame A moved 3 right and 2 down
        let (w, h) = (64usize, 48usize);
        let tex = |x: i32, y: i32| {
            let (x, y) = (x as f32, y as f32);
            0.5 + 0.2 * (x * 0.7).sin() * (y * 0.45).cos() + 0.2 * ((x + 2.0 * y) * 0.31).sin()
        };
        let frame = |dx: i32, dy: i32| {
            let mut f = vec![1f32; w * h * 4];
            for y in 0..h {
                for x in 0..w {
                    let v = tex(x as i32 - dx, y as i32 - dy);
                    for c in 0..3 {
                        f[(y * w + x) * 4 + c] = v;
                    }
                }
            }
            f
        };
        let (a, b) = (frame(0, 0), frame(3, 2));
        let shader = with_temporal(BILINEAR_X2);
        let (_, mv) = run_frames(
            &ctx,
            &shader,
            w as u32,
            h as u32,
            1,
            &[&a, &b],
            Some("MOTION"),
        )
        .unwrap();
        // inner blocks (8×6 grid): the block was 3 left and 2 up in the previous frame
        for by in 1..5 {
            for bx in 1..7 {
                let i = (by * 8 + bx) * 4;
                assert_eq!((mv[i], mv[i + 1]), (-3.0, -2.0), "block {bx},{by}");
            }
        }
        // the generated frame halfway through a 2-pixel move is the picture moved by 1
        let (c, d) = (frame(0, 0), frame(2, 0));
        let fg = with_framegen(BILINEAR_X2);
        let (_, mid) = run_frames(&ctx, &fg, w as u32, h as u32, 1, &[&c, &d], Some("FG")).unwrap();
        let (_, want) = run(
            &ctx,
            BILINEAR_X2,
            w as u32,
            h as u32,
            1,
            Some(&frame(1, 0)),
            true,
        )
        .unwrap();
        let (_, last) = run(&ctx, BILINEAR_X2, w as u32, h as u32, 1, Some(&d), true).unwrap();
        let ow = w * 2;
        let inner = |v: &[f32], u: &[f32]| {
            let mut sum = 0.0;
            let mut n = 0;
            for y in 24..h * 2 - 24 {
                for x in 24..ow - 24 {
                    for ch in 0..3 {
                        let i = (y * ow + x) * 4 + ch;
                        sum += (v[i] - u[i]).abs();
                        n += 1;
                    }
                }
            }
            sum / n as f32
        };
        let (good, lazy) = (inner(&mid, &want), inner(&last, &want));
        assert!(
            good < 0.02 && good < lazy / 3.0,
            "halfway {good}, last frame {lazy}"
        );
        // a still picture stays itself through the temporal pass
        let (_, still) =
            run_frames(&ctx, &shader, w as u32, h as u32, 4, &[&a], Some("MAIN")).unwrap();
        let (_, plain) = run(&ctx, BILINEAR_X2, w as u32, h as u32, 1, Some(&a), true).unwrap();
        let diff = still
            .iter()
            .zip(&plain)
            .map(|(x, y)| (x - y).abs())
            .fold(0f32, f32::max);
        assert!(diff < 0.02, "{diff}");
    }
}
