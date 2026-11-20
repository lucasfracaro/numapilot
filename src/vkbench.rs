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

/// Models of the bench: (value, name, shader).
pub const MODELS: [(&str, &str, &str); 4] = [
    ("bilinear", "bilinear 2× (floor)", BILINEAR_X2),
    ("anime4k-s", "Anime4K CNN x2 S", ANIME4K_S),
    ("anime4k-m", "Anime4K CNN x2 M", ANIME4K_M),
    ("anime4k-l", "Anime4K CNN x2 L", ANIME4K_L),
];

// ---------------------------------------------------------------------------------------------
// mpv hooks.

#[derive(Debug, Clone, PartialEq)]
pub struct Pass {
    pub desc: String,
    pub binds: Vec<String>,
    pub save: String,
    pub width: String,
    pub height: String,
    /// Everything after the `//!` lines: defines and `vec4 hook()`.
    pub body: String,
}

/// Splits a user shader into its passes.
pub fn parse_hooks(src: &str) -> Vec<Pass> {
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
                    binds: vec![],
                    save: String::new(),
                    width: String::new(),
                    height: String::new(),
                    body: String::new(),
                });
                in_header = true;
            }
            let p = cur.as_mut().expect("pass");
            match k {
                "DESC" => p.desc = v,
                "BIND" => p.binds.push(v),
                "SAVE" => p.save = v,
                "WIDTH" => p.width = v,
                "HEIGHT" => p.height = v,
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
        if p.save.is_empty() {
            p.save = "MAIN".into();
        }
    }
    passes
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
    /// Bound textures (binding 1, 2, … in this order).
    pub binds: Vec<String>,
}

pub const GROUP: u32 = 8;

/// Turns the passes into compute shaders for an input of w×h. Returns the passes and the size of
/// every texture (MAIN at the end = the output).
pub fn plan(
    passes: &[Pass],
    w: u32,
    h: u32,
) -> Result<(Vec<Planned>, BTreeMap<String, (u32, u32)>)> {
    let mut sizes = BTreeMap::new();
    sizes.insert("MAIN".to_string(), (w, h));
    let mut out = vec![];
    for p in passes {
        let ow = eval_size(&p.width, &sizes)
            .with_context(|| format!("{}: width '{}'", p.desc, p.width))?;
        let oh = eval_size(&p.height, &sizes)
            .with_context(|| format!("{}: height '{}'", p.desc, p.height))?;
        let mut g = String::from("#version 450\n");
        g.push_str(&format!(
            "layout(local_size_x = {GROUP}, local_size_y = {GROUP}, local_size_z = 1) in;\n"
        ));
        g.push_str("layout(std430, set = 0, binding = 0) buffer Out_ { vec4 out_d[]; };\n");
        for (i, b) in p.binds.iter().enumerate() {
            if !sizes.contains_key(b) {
                bail!("{}: texture '{b}' is bound before it is saved", p.desc);
            }
            g.push_str(&format!(
                "layout(std430, set = 0, binding = {}) buffer {b}_B {{ vec4 {b}_d[]; }};\n",
                i + 1
            ));
        }
        g.push_str("vec2 cur_pos;\n");
        for b in &p.binds {
            let (bw, bh) = sizes[b];
            g.push_str(&format!(
                "const vec2 {b}_size = vec2({bw}.0, {bh}.0);
const vec2 {b}_pt = vec2(1.0 / {bw}.0, 1.0 / {bh}.0);
#define {b}_pos cur_pos
vec4 {b}_fetch(ivec2 p) {{
    p = clamp(p, ivec2(0, 0), ivec2({mw}, {mh}));
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
vec4 {b}_texOff(vec2 off) {{
    return {b}_fetch(ivec2(floor(cur_pos * {b}_size)) + ivec2(off));
}}
",
                mw = bw - 1,
                mh = bh - 1
            ));
        }
        g.push_str(&p.body);
        g.push_str(&format!(
            "void main() {{
    ivec2 gid = ivec2(gl_GlobalInvocationID.xy);
    if (gid.x >= {ow} || gid.y >= {oh}) {{
        return;
    }}
    cur_pos = (vec2(gid) + vec2(0.5, 0.5)) / vec2({ow}.0, {oh}.0);
    out_d[gid.y * {ow} + gid.x] = hook();
}}
"
        ));
        sizes.insert(p.save.clone(), (ow, oh));
        out.push(Planned {
            desc: p.desc.clone(),
            glsl: g,
            save: p.save.clone(),
            out: (ow, oh),
            binds: p.binds.clone(),
        });
    }
    Ok((out, sizes))
}

/// GLSL compute → SPIR-V words.
pub fn compile(glsl: &str) -> Result<Vec<u32>> {
    use naga::back::spv;
    use naga::front::glsl;
    let mut fe = glsl::Frontend::default();
    let opts = glsl::Options::from(naga::ShaderStage::Compute);
    let module = fe
        .parse(&opts, glsl)
        .map_err(|e| anyhow!("GLSL: {}", e.emit_to_string(glsl)))?;
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
    let mut wiring: Vec<(usize, Vec<usize>)> = vec![];
    for p in &planned {
        let ins: Vec<usize> = p.binds.iter().map(|b| cur[b]).collect();
        bufs.push(ctx.buffer(px(p.out), usage, false)?);
        let o = bufs.len() - 1;
        cur.insert(p.save.clone(), o);
        wiring.push((o, ins));
    }
    let out_idx = cur["MAIN"];
    let out_size = sizes["MAIN"];

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
            d.cmd_dispatch(cb, p.out.0.div_ceil(GROUP), p.out.1.div_ceil(GROUP), 1);
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
        if let Some(rb) = &readback_buf {
            barrier(
                cb,
                vk::AccessFlags::SHADER_WRITE,
                vk::AccessFlags::TRANSFER_READ,
                vk::PipelineStageFlags::COMPUTE_SHADER,
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
    let mut per_pass: Vec<Vec<f64>> = vec![vec![]; planned.len()];
    let mut totals = vec![];
    let runs = runs.max(1);
    let mut result = Ok(());
    for _ in 0..runs + 1 {
        let cbs = [cb];
        let r = unsafe {
            d.queue_submit(
                ctx.queue,
                &[vk::SubmitInfo::default().command_buffers(&cbs)],
                fence,
            )
            .and_then(|_| d.wait_for_fences(&[fence], true, 10_000_000_000))
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
    }
}
