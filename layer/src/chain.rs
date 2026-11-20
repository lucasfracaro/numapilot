//! The work done at each present: small image → buffer → unpack → upscaler passes → pack →
//! real swapchain image. Storage buffers only (no storage images), so it works with any 32-bit
//! swapchain format the driver can copy to (8-bit RGBA/BGRA and 10-bit A2B10G10R10/A2R10G10B10).

use crate::DeviceData;
use anyhow::{Context, Result, anyhow, bail};
use ash::vk;
use std::ffi::c_void;

/// Layout of the 32-bit swapchain pixels handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Px {
    /// R, G, B, A bytes.
    Rgba8,
    /// B, G, R, A bytes.
    Bgra8,
    /// A2B10G10R10: red in the low 10 bits.
    Rgb10A2,
    /// A2R10G10B10: blue in the low 10 bits.
    Bgr10A2,
}

/// The pixel layout of a swapchain format, None when it is not handled (passed through).
pub fn format_px(f: vk::Format) -> Option<Px> {
    match f {
        vk::Format::B8G8R8A8_UNORM | vk::Format::B8G8R8A8_SRGB => Some(Px::Bgra8),
        vk::Format::R8G8B8A8_UNORM
        | vk::Format::R8G8B8A8_SRGB
        | vk::Format::A8B8G8R8_UNORM_PACK32
        | vk::Format::A8B8G8R8_SRGB_PACK32 => Some(Px::Rgba8),
        vk::Format::A2B10G10R10_UNORM_PACK32 => Some(Px::Rgb10A2),
        vk::Format::A2R10G10B10_UNORM_PACK32 => Some(Px::Bgr10A2),
        _ => None,
    }
}

const GROUP: u32 = numapilot::vkbench::GROUP;

/// GLSL that turns one u32 pixel `v` into vec4 RGBA.
fn unpack_expr(px: Px) -> &'static str {
    match px {
        Px::Rgba8 => "unpackUnorm4x8(v)",
        Px::Bgra8 => "unpackUnorm4x8(v).zyxw",
        Px::Rgb10A2 | Px::Bgr10A2 => {
            "vec4(float(v & 1023u), float((v >> 10u) & 1023u), float((v >> 20u) & 1023u), \
             float(v >> 30u) * 341.0) / 1023.0"
        }
    }
}

/// u32 pixels (as copied from the swapchain image) → vec4 RGBA.
pub fn unpack_glsl(w: u32, h: u32, px: Px) -> String {
    let e = unpack_expr(px);
    let sw = if px == Px::Bgr10A2 { ".zyxw" } else { "" };
    format!(
        "#version 450
layout(local_size_x = {GROUP}, local_size_y = {GROUP}, local_size_z = 1) in;
layout(std430, set = 0, binding = 0) buffer Out_ {{ vec4 out_d[]; }};
layout(std430, set = 0, binding = 1) buffer In_ {{ uint in_d[]; }};
void main() {{
    ivec2 g = ivec2(gl_GlobalInvocationID.xy);
    if (g.x >= {w} || g.y >= {h}) {{
        return;
    }}
    int i = g.y * {w} + g.x;
    uint v = in_d[i];
    out_d[i] = ({e}){sw};
}}
"
    )
}

/// vec4 RGBA → u32 pixels in the swapchain's layout (alpha opaque).
pub fn pack_glsl(w: u32, h: u32, px: Px) -> String {
    let body = match px {
        Px::Rgba8 => "packUnorm4x8(c)".to_string(),
        Px::Bgra8 => "packUnorm4x8(c.zyxw)".to_string(),
        Px::Rgb10A2 | Px::Bgr10A2 => {
            "(q.x | (q.y << 10u) | (q.z << 20u) | (3u << 30u))".to_string()
        }
    };
    let pre = match px {
        Px::Rgb10A2 => "    uvec3 q = uvec3(round(c.xyz * 1023.0));\n",
        Px::Bgr10A2 => "    uvec3 q = uvec3(round(c.zyx * 1023.0));\n",
        _ => "",
    };
    format!(
        "#version 450
layout(local_size_x = {GROUP}, local_size_y = {GROUP}, local_size_z = 1) in;
layout(std430, set = 0, binding = 0) buffer Out_ {{ uint out_d[]; }};
layout(std430, set = 0, binding = 1) buffer In_ {{ vec4 in_d[]; }};
void main() {{
    ivec2 g = ivec2(gl_GlobalInvocationID.xy);
    if (g.x >= {w} || g.y >= {h}) {{
        return;
    }}
    int i = g.y * {w} + g.x;
    vec4 c = vec4(clamp(in_d[i].xyz, vec3(0.0), vec3(1.0)), 1.0);
{pre}    out_d[i] = {body};
}}
"
    )
}

struct Buf {
    b: vk::Buffer,
    m: vk::DeviceMemory,
}

struct Pass {
    pipe: vk::Pipeline,
    layout: vk::PipelineLayout,
    dsl: vk::DescriptorSetLayout,
    set: vk::DescriptorSet,
    groups: (u32, u32),
}

struct Frame {
    cb: Option<(u32, vk::CommandBuffer)>,
    fence: vk::Fence,
    sem: vk::Semaphore,
    used: bool,
}

pub struct Chain {
    small: Vec<vk::Image>,
    small_mem: Vec<vk::DeviceMemory>,
    real: Vec<vk::Image>,
    small_ext: vk::Extent2D,
    real_ext: vk::Extent2D,
    /// What the passes produce (2 × small).
    out_ext: vk::Extent2D,
    bufs: Vec<Buf>,
    raw_in: usize,
    raw_out: usize,
    passes: Vec<Pass>,
    pool: vk::DescriptorPool,
    frames: Vec<Frame>,
}

unsafe fn mem_type(d: &DeviceData, bits: u32, want: vk::MemoryPropertyFlags) -> Option<u32> {
    (0..d.mem.memory_type_count).find(|&i| {
        bits & (1 << i) != 0 && d.mem.memory_types[i as usize].property_flags.contains(want)
    })
}

unsafe fn buffer(d: &DeviceData, size: u64, usage: vk::BufferUsageFlags) -> Result<Buf> {
    let dev = &d.dev;
    unsafe {
        let b = dev.create_buffer(
            &vk::BufferCreateInfo::default()
                .size(size)
                .usage(usage)
                .sharing_mode(vk::SharingMode::EXCLUSIVE),
            None,
        )?;
        let req = dev.get_buffer_memory_requirements(b);
        let mt = mem_type(
            d,
            req.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )
        .or_else(|| mem_type(d, req.memory_type_bits, vk::MemoryPropertyFlags::empty()))
        .context("no memory type")?;
        let m = dev.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(mt),
            None,
        )?;
        dev.bind_buffer_memory(b, m, 0)?;
        Ok(Buf { b, m })
    }
}

/// The VkImageFormatListCreateInfo of the swapchain, if any (mutable-format swapchains).
unsafe fn format_list<'a>(
    info: &'a vk::SwapchainCreateInfoKHR<'_>,
) -> Option<&'a vk::ImageFormatListCreateInfo<'a>> {
    let mut p = info.p_next;
    while !p.is_null() {
        let base = p as *const vk::BaseInStructure<'_>;
        unsafe {
            if (*base).s_type == vk::StructureType::IMAGE_FORMAT_LIST_CREATE_INFO {
                return Some(&*(p as *const vk::ImageFormatListCreateInfo<'_>));
            }
            p = (*base).p_next as *const c_void;
        }
    }
    None
}

impl Chain {
    pub unsafe fn new(
        d: &DeviceData,
        sc: vk::SwapchainKHR,
        info: &vk::SwapchainCreateInfoKHR<'_>,
        real_ext: vk::Extent2D,
        small_ext: vk::Extent2D,
        px: Px,
        shader: &str,
    ) -> Result<Chain> {
        let dev = &d.dev;
        let device = dev.handle();
        let mut n = 0u32;
        unsafe {
            (d.swap.get_swapchain_images_khr)(device, sc, &mut n, std::ptr::null_mut()).result()?;
        }
        let mut real = vec![vk::Image::null(); n as usize];
        unsafe {
            (d.swap.get_swapchain_images_khr)(device, sc, &mut n, real.as_mut_ptr()).result()?;
        }
        real.truncate(n as usize);
        let mut c = Chain {
            small: vec![],
            small_mem: vec![],
            real,
            small_ext,
            real_ext,
            out_ext: small_ext,
            bufs: vec![],
            raw_in: 0,
            raw_out: 0,
            passes: vec![],
            pool: vk::DescriptorPool::null(),
            frames: vec![],
        };
        match unsafe { c.build(d, info, px, shader) } {
            Ok(()) => Ok(c),
            Err(e) => {
                unsafe { c.destroy(d) };
                Err(e)
            }
        }
    }

    unsafe fn build(
        &mut self,
        d: &DeviceData,
        info: &vk::SwapchainCreateInfoKHR<'_>,
        px: Px,
        shader: &str,
    ) -> Result<()> {
        let dev = &d.dev;
        // the small images gamescope draws into
        let mut flags = vk::ImageCreateFlags::empty();
        if info
            .flags
            .contains(vk::SwapchainCreateFlagsKHR::MUTABLE_FORMAT)
        {
            flags |= vk::ImageCreateFlags::MUTABLE_FORMAT | vk::ImageCreateFlags::EXTENDED_USAGE;
        }
        if info
            .flags
            .contains(vk::SwapchainCreateFlagsKHR::SPLIT_INSTANCE_BIND_REGIONS)
        {
            bail!("split-instance swapchains are not handled");
        }
        let fl = unsafe { format_list(info) };
        for _ in 0..self.real.len() {
            let mut ici = vk::ImageCreateInfo::default()
                .flags(flags)
                .image_type(vk::ImageType::TYPE_2D)
                .format(info.image_format)
                .extent(vk::Extent3D {
                    width: self.small_ext.width,
                    height: self.small_ext.height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(info.image_array_layers.max(1))
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(info.image_usage | vk::ImageUsageFlags::TRANSFER_SRC)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED);
            let mut list;
            if let Some(l) = fl {
                list = vk::ImageFormatListCreateInfo::default().view_formats(unsafe {
                    std::slice::from_raw_parts(l.p_view_formats, l.view_format_count as usize)
                });
                ici = ici.push_next(&mut list);
            }
            let img = unsafe { dev.create_image(&ici, None) }.context("small image")?;
            self.small.push(img);
            let req = unsafe { dev.get_image_memory_requirements(img) };
            let mt = unsafe {
                mem_type(
                    d,
                    req.memory_type_bits,
                    vk::MemoryPropertyFlags::DEVICE_LOCAL,
                )
            }
            .context("no memory type for the small image")?;
            let m = unsafe {
                dev.allocate_memory(
                    &vk::MemoryAllocateInfo::default()
                        .allocation_size(req.size)
                        .memory_type_index(mt),
                    None,
                )
            }?;
            self.small_mem.push(m);
            unsafe { dev.bind_image_memory(img, m, 0) }?;
        }

        // compute passes: unpack, the upscaler, pack
        let (sw, sh) = (self.small_ext.width, self.small_ext.height);
        let hooks = numapilot::vkbench::parse_hooks(shader);
        let (planned, sizes) = numapilot::vkbench::plan(&hooks, sw, sh)?;
        let (ow, oh) = sizes["MAIN"];
        self.out_ext = vk::Extent2D {
            width: ow,
            height: oh,
        };
        let usage = vk::BufferUsageFlags::STORAGE_BUFFER
            | vk::BufferUsageFlags::TRANSFER_SRC
            | vk::BufferUsageFlags::TRANSFER_DST;
        let bytes = |w: u32, h: u32, b: u64| w as u64 * h as u64 * b;
        self.bufs
            .push(unsafe { buffer(d, bytes(sw, sh, 4), usage) }?);
        self.raw_in = 0;
        self.bufs
            .push(unsafe { buffer(d, bytes(sw, sh, 16), usage) }?);
        let mut cur: std::collections::BTreeMap<String, usize> = Default::default();
        cur.insert("MAIN".into(), 1);
        // (spirv, out size, out buffer, in buffers)
        let mut specs: Vec<(Vec<u32>, (u32, u32), usize, Vec<usize>)> = vec![(
            numapilot::vkbench::compile(&unpack_glsl(sw, sh, px))?,
            (sw, sh),
            1,
            vec![0],
        )];
        for p in &planned {
            let ins: Vec<usize> = p.binds.iter().map(|b| cur[b]).collect();
            self.bufs
                .push(unsafe { buffer(d, bytes(p.out.0, p.out.1, 16), usage) }?);
            let o = self.bufs.len() - 1;
            cur.insert(p.save.clone(), o);
            specs.push((
                numapilot::vkbench::compile(&p.glsl).with_context(|| p.desc.clone())?,
                p.out,
                o,
                ins,
            ));
        }
        self.bufs
            .push(unsafe { buffer(d, bytes(ow, oh, 4), usage) }?);
        self.raw_out = self.bufs.len() - 1;
        specs.push((
            numapilot::vkbench::compile(&pack_glsl(ow, oh, px))?,
            (ow, oh),
            self.raw_out,
            vec![cur["MAIN"]],
        ));

        let max_b = specs.iter().map(|s| s.3.len() + 1).sum::<usize>() as u32;
        self.pool = unsafe {
            dev.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(specs.len() as u32)
                    .pool_sizes(&[vk::DescriptorPoolSize {
                        ty: vk::DescriptorType::STORAGE_BUFFER,
                        descriptor_count: max_b,
                    }]),
                None,
            )
        }?;
        for (spirv, out, o, ins) in specs {
            let binds: Vec<vk::DescriptorSetLayoutBinding> = (0..=ins.len() as u32)
                .map(|b| {
                    vk::DescriptorSetLayoutBinding::default()
                        .binding(b)
                        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                        .descriptor_count(1)
                        .stage_flags(vk::ShaderStageFlags::COMPUTE)
                })
                .collect();
            unsafe {
                let dsl = dev.create_descriptor_set_layout(
                    &vk::DescriptorSetLayoutCreateInfo::default().bindings(&binds),
                    None,
                )?;
                let layout = dev.create_pipeline_layout(
                    &vk::PipelineLayoutCreateInfo::default().set_layouts(&[dsl]),
                    None,
                )?;
                let module = dev.create_shader_module(
                    &vk::ShaderModuleCreateInfo::default().code(&spirv),
                    None,
                )?;
                let stage = vk::PipelineShaderStageCreateInfo::default()
                    .stage(vk::ShaderStageFlags::COMPUTE)
                    .module(module)
                    .name(c"main");
                let pipe = dev
                    .create_compute_pipelines(
                        vk::PipelineCache::null(),
                        &[vk::ComputePipelineCreateInfo::default()
                            .stage(stage)
                            .layout(layout)],
                        None,
                    )
                    .map_err(|(_, e)| anyhow!("vkCreateComputePipelines: {e}"));
                dev.destroy_shader_module(module, None);
                let pipe = match pipe {
                    Ok(p) => p[0],
                    Err(e) => {
                        dev.destroy_pipeline_layout(layout, None);
                        dev.destroy_descriptor_set_layout(dsl, None);
                        return Err(e);
                    }
                };
                let set = dev.allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(self.pool)
                        .set_layouts(&[dsl]),
                )?[0];
                let infos: Vec<[vk::DescriptorBufferInfo; 1]> = std::iter::once(o)
                    .chain(ins.iter().copied())
                    .map(|bi| {
                        [vk::DescriptorBufferInfo::default()
                            .buffer(self.bufs[bi].b)
                            .offset(0)
                            .range(vk::WHOLE_SIZE)]
                    })
                    .collect();
                let writes: Vec<vk::WriteDescriptorSet> = infos
                    .iter()
                    .enumerate()
                    .map(|(b, i)| {
                        vk::WriteDescriptorSet::default()
                            .dst_set(set)
                            .dst_binding(b as u32)
                            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                            .buffer_info(i)
                    })
                    .collect();
                dev.update_descriptor_sets(&writes, &[]);
                self.passes.push(Pass {
                    pipe,
                    layout,
                    dsl,
                    set,
                    groups: (out.0.div_ceil(GROUP), out.1.div_ceil(GROUP)),
                });
            }
        }
        for _ in 0..self.real.len() {
            unsafe {
                self.frames.push(Frame {
                    cb: None,
                    fence: dev.create_fence(
                        &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
                        None,
                    )?,
                    sem: dev.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)?,
                    used: false,
                });
            }
        }
        Ok(())
    }

    pub fn small_images(&self) -> &[vk::Image] {
        &self.small
    }

    /// Records and submits the work for swapchain image `i` on `queue` (family `family`), after
    /// `waits`. Returns the semaphore the present must wait on.
    pub unsafe fn run(
        &mut self,
        d: &DeviceData,
        queue: vk::Queue,
        family: u32,
        i: u32,
        waits: &[vk::Semaphore],
    ) -> Result<vk::Semaphore> {
        let dev = &d.dev;
        let i = i as usize;
        if i >= self.frames.len() {
            bail!("image index {i} out of range");
        }
        let f = &mut self.frames[i];
        unsafe {
            dev.wait_for_fences(&[f.fence], true, 5_000_000_000)?;
            dev.reset_fences(&[f.fence])?;
        }
        let cb = match f.cb {
            Some((fam, cb)) if fam == family => cb,
            old => {
                let pool = unsafe { d.pool(family) }?;
                if let Some((ofam, ocb)) = old
                    && let Ok(op) = unsafe { d.pool(ofam) }
                {
                    unsafe { dev.free_command_buffers(op, &[ocb]) };
                }
                let cb = unsafe {
                    dev.allocate_command_buffers(
                        &vk::CommandBufferAllocateInfo::default()
                            .command_pool(pool)
                            .level(vk::CommandBufferLevel::PRIMARY)
                            .command_buffer_count(1),
                    )
                }?[0];
                unsafe { d.set_dispatch(cb) };
                f.cb = Some((family, cb));
                cb
            }
        };
        let first_use = !f.used;
        f.used = true;
        let sem = f.sem;
        let fence = f.fence;
        let small = self.small[i];
        let real = self.real[i];
        let color = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };
        let layers = vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        };
        let mem_barrier = |src: vk::AccessFlags, dst: vk::AccessFlags, s, t| unsafe {
            dev.cmd_pipeline_barrier(
                cb,
                s,
                t,
                vk::DependencyFlags::empty(),
                &[vk::MemoryBarrier::default()
                    .src_access_mask(src)
                    .dst_access_mask(dst)],
                &[],
                &[],
            )
        };
        let img_barrier = |img, old, new, src: vk::AccessFlags, dst: vk::AccessFlags, s, t| unsafe {
            dev.cmd_pipeline_barrier(
                cb,
                s,
                t,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[vk::ImageMemoryBarrier::default()
                    .image(img)
                    .old_layout(old)
                    .new_layout(new)
                    .src_access_mask(src)
                    .dst_access_mask(dst)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .subresource_range(color)],
            )
        };
        unsafe {
            dev.reset_command_buffer(cb, vk::CommandBufferResetFlags::empty())?;
            dev.begin_command_buffer(
                cb,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            // the previous frame's passes share the buffers
            mem_barrier(
                vk::AccessFlags::SHADER_WRITE | vk::AccessFlags::TRANSFER_READ,
                vk::AccessFlags::TRANSFER_WRITE | vk::AccessFlags::SHADER_WRITE,
                vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::TRANSFER | vk::PipelineStageFlags::COMPUTE_SHADER,
            );
            // gamescope left the small image ready for presenting
            img_barrier(
                small,
                vk::ImageLayout::PRESENT_SRC_KHR,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                vk::AccessFlags::MEMORY_WRITE,
                vk::AccessFlags::TRANSFER_READ,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::TRANSFER,
            );
            dev.cmd_copy_image_to_buffer(
                cb,
                small,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                self.bufs[self.raw_in].b,
                &[vk::BufferImageCopy::default()
                    .image_subresource(layers)
                    .image_extent(vk::Extent3D {
                        width: self.small_ext.width,
                        height: self.small_ext.height,
                        depth: 1,
                    })],
            );
            img_barrier(
                small,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                vk::ImageLayout::PRESENT_SRC_KHR,
                vk::AccessFlags::TRANSFER_READ,
                vk::AccessFlags::empty(),
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            );
            mem_barrier(
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::SHADER_READ,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
            );
            for p in &self.passes {
                dev.cmd_bind_pipeline(cb, vk::PipelineBindPoint::COMPUTE, p.pipe);
                dev.cmd_bind_descriptor_sets(
                    cb,
                    vk::PipelineBindPoint::COMPUTE,
                    p.layout,
                    0,
                    &[p.set],
                    &[],
                );
                dev.cmd_dispatch(cb, p.groups.0, p.groups.1, 1);
                mem_barrier(
                    vk::AccessFlags::SHADER_WRITE,
                    vk::AccessFlags::SHADER_READ | vk::AccessFlags::TRANSFER_READ,
                    vk::PipelineStageFlags::COMPUTE_SHADER,
                    vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::TRANSFER,
                );
            }
            img_barrier(
                real,
                vk::ImageLayout::UNDEFINED,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::AccessFlags::empty(),
                vk::AccessFlags::TRANSFER_WRITE,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::TRANSFER,
            );
            let (cw, ch) = (
                self.out_ext.width.min(self.real_ext.width),
                self.out_ext.height.min(self.real_ext.height),
            );
            if (cw, ch) != (self.real_ext.width, self.real_ext.height) {
                // odd window: the last row/column the upscaler does not cover stays black
                dev.cmd_clear_color_image(
                    cb,
                    real,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &vk::ClearColorValue {
                        float32: [0.0, 0.0, 0.0, 1.0],
                    },
                    &[color],
                );
                mem_barrier(
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::AccessFlags::TRANSFER_WRITE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::TRANSFER,
                );
            }
            dev.cmd_copy_buffer_to_image(
                cb,
                self.bufs[self.raw_out].b,
                real,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[vk::BufferImageCopy::default()
                    .buffer_row_length(self.out_ext.width)
                    .buffer_image_height(self.out_ext.height)
                    .image_subresource(layers)
                    .image_extent(vk::Extent3D {
                        width: cw,
                        height: ch,
                        depth: 1,
                    })],
            );
            img_barrier(
                real,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::PRESENT_SRC_KHR,
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::empty(),
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
            );
            dev.end_command_buffer(cb)?;
            let stages = vec![vk::PipelineStageFlags::ALL_COMMANDS; waits.len()];
            let cbs = [cb];
            let sigs = [sem];
            dev.queue_submit(
                queue,
                &[vk::SubmitInfo::default()
                    .wait_semaphores(waits)
                    .wait_dst_stage_mask(&stages)
                    .command_buffers(&cbs)
                    .signal_semaphores(&sigs)],
                fence,
            )?;
        }
        if first_use {
            crate::log!("first present on image {i}");
        }
        Ok(sem)
    }

    pub unsafe fn destroy(&mut self, d: &DeviceData) {
        let dev = &d.dev;
        unsafe {
            for f in self.frames.drain(..) {
                let _ = dev.wait_for_fences(&[f.fence], true, 1_000_000_000);
                if let Some((fam, cb)) = f.cb
                    && let Ok(p) = d.pool(fam)
                {
                    dev.free_command_buffers(p, &[cb]);
                }
                dev.destroy_fence(f.fence, None);
                dev.destroy_semaphore(f.sem, None);
            }
            for p in self.passes.drain(..) {
                dev.destroy_pipeline(p.pipe, None);
                dev.destroy_pipeline_layout(p.layout, None);
                dev.destroy_descriptor_set_layout(p.dsl, None);
            }
            if self.pool != vk::DescriptorPool::null() {
                dev.destroy_descriptor_pool(self.pool, None);
                self.pool = vk::DescriptorPool::null();
            }
            for b in self.bufs.drain(..) {
                dev.destroy_buffer(b.b, None);
                dev.free_memory(b.m, None);
            }
            for img in self.small.drain(..) {
                dev.destroy_image(img, None);
            }
            for m in self.small_mem.drain(..) {
                dev.free_memory(m, None);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_and_unpack_compile() {
        for px in [Px::Rgba8, Px::Bgra8, Px::Rgb10A2, Px::Bgr10A2] {
            numapilot::vkbench::compile(&unpack_glsl(64, 36, px)).unwrap();
            numapilot::vkbench::compile(&pack_glsl(128, 72, px)).unwrap();
        }
        assert_eq!(format_px(vk::Format::B8G8R8A8_SRGB), Some(Px::Bgra8));
        assert_eq!(format_px(vk::Format::R8G8B8A8_UNORM), Some(Px::Rgba8));
        assert_eq!(
            format_px(vk::Format::A2B10G10R10_UNORM_PACK32),
            Some(Px::Rgb10A2)
        );
        assert_eq!(
            format_px(vk::Format::A2R10G10B10_UNORM_PACK32),
            Some(Px::Bgr10A2)
        );
        assert_eq!(format_px(vk::Format::R16G16B16A16_SFLOAT), None);
    }
}
