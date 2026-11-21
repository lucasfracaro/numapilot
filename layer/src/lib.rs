//! VK_LAYER_NUMAPILOT_upscale: step M1 of docs/PRESENT-CHAIN.md.
//!
//! Loaded only in gamescope (SDL backend, so gamescope presents through a Vulkan swapchain).
//! gamescope composes at the window's size and, started with `-S integer -F nearest`, draws the
//! game as blocks of k×k identical pixels (k ≥ 2). The layer gives gamescope images of its own
//! for the swapchain; at present it reads one texel per block (the game's frame at its own size,
//! `NUMAPILOT_UPSCALE_SRC=WxH`), runs the upscaler (an mpv user shader such as Anime4K, run as
//! compute by `numapilot::vkbench`) and writes the 2× result into the real swapchain image, which
//! is then presented.
//!
//! Turned on by the implicit layer manifest's `ENABLE_NUMAPILOT_UPSCALE=1`; it only acts in
//! processes named in `NUMAPILOT_UPSCALE_PROCS` (default: gamescope), so a game that inherits the
//! variable is passed through untouched. Model: `NUMAPILOT_UPSCALE_MODEL` (anime4k-s|m|l,
//! artcnn-c4f16|c4f32, fsrcnnx(-16) once downloaded, bilinear, or a path to an mpv user shader;
//! default anime4k-m); `NUMAPILOT_UPSCALE_TEMPORAL=1` adds the temporal pass
//! (`shaders/temporal/temporal.glsl`, history kept between presents). Any failure falls back to
//! passing everything through.

#![allow(clippy::missing_safety_doc, non_snake_case)]

mod chain;

use ash::vk;
use ash::vk::Handle;
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::sync::{Arc, Mutex, OnceLock};

pub const LAYER_NAME: &CStr = c"VK_LAYER_NUMAPILOT_upscale";

macro_rules! log {
    ($($a:tt)*) => {
        if crate::verbose() {
            eprintln!("numapilot-upscale: {}", format!($($a)*));
        }
    };
}
pub(crate) use log;

fn verbose() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| std::env::var_os("NUMAPILOT_UPSCALE_DEBUG").is_some_and(|v| v != "0"))
}

/// Does the layer act in this process?
pub fn active() -> bool {
    static A: OnceLock<bool> = OnceLock::new();
    *A.get_or_init(|| {
        let comm = std::fs::read_to_string("/proc/self/comm").unwrap_or_default();
        let procs = std::env::var("NUMAPILOT_UPSCALE_PROCS")
            .unwrap_or_else(|_| "gamescope,gamescope-wl".into());
        let on = proc_matches(comm.trim(), &procs);
        if on {
            eprintln!(
                "numapilot-upscale: on in {} (model {})",
                comm.trim(),
                model_name()
            );
        }
        on
    })
}

pub fn proc_matches(comm: &str, procs: &str) -> bool {
    procs
        .split(',')
        .map(str::trim)
        .any(|p| !p.is_empty() && (p == "*" || p == comm))
}

pub fn model_name() -> String {
    std::env::var("NUMAPILOT_UPSCALE_MODEL").unwrap_or_else(|_| "anime4k-m".into())
}

/// The mpv user shader of the model: shipped (`numapilot::vkbench::MODELS`), downloaded
/// (FSRCNNX, `numapilot::vkbench::shader_dir()`) or a path to an mpv user shader.
pub fn model_shader(name: &str) -> Option<std::borrow::Cow<'static, str>> {
    numapilot::vkbench::model_source(name)
}

/// `NUMAPILOT_UPSCALE_TEMPORAL=1`: the temporal pass (estimated motion, history) after the model.
pub fn temporal() -> bool {
    std::env::var("NUMAPILOT_UPSCALE_TEMPORAL").is_ok_and(|v| v == "1")
}

// ---------------------------------------------------------------------------------------------
// Loader structures (vk_layer.h), not in ash.

#[repr(C)]
struct LayerInstanceLink {
    next: *mut LayerInstanceLink,
    gipa: vk::PFN_vkGetInstanceProcAddr,
    gpdpa: *const c_void,
}

#[repr(C)]
struct LayerDeviceLink {
    next: *mut LayerDeviceLink,
    gipa: vk::PFN_vkGetInstanceProcAddr,
    gdpa: vk::PFN_vkGetDeviceProcAddr,
}

/// VkLayerInstanceCreateInfo / VkLayerDeviceCreateInfo: the union's first member is a pointer.
#[repr(C)]
struct LayerCreateInfo {
    s_type: vk::StructureType,
    p_next: *const c_void,
    function: i32,
    u: *mut c_void,
}

const VK_LAYER_LINK_INFO: i32 = 0;
const VK_LOADER_DATA_CALLBACK: i32 = 1;

type SetDeviceLoaderData = unsafe extern "system" fn(vk::Device, *mut c_void) -> vk::Result;

#[repr(C)]
pub struct NegotiateLayerInterface {
    s_type: i32,
    p_next: *mut c_void,
    loader_layer_interface_version: u32,
    gipa: *const c_void,
    gdpa: *const c_void,
    gpdpa: *const c_void,
}

unsafe fn find_create_info(
    mut p: *const c_void,
    ty: vk::StructureType,
    func: i32,
) -> *mut LayerCreateInfo {
    while !p.is_null() {
        let ci = p as *mut LayerCreateInfo;
        unsafe {
            if (*ci).s_type == ty && (*ci).function == func {
                return ci;
            }
            p = (*ci).p_next;
        }
    }
    std::ptr::null_mut()
}

// ---------------------------------------------------------------------------------------------
// State.

pub(crate) struct InstanceData {
    gipa: vk::PFN_vkGetInstanceProcAddr,
    fp: ash::InstanceFnV1_0,
    surface: ash::khr::surface::InstanceFn,
}

pub(crate) struct DeviceData {
    pub(crate) dev: ash::Device,
    pub(crate) pdev: vk::PhysicalDevice,
    gdpa: vk::PFN_vkGetDeviceProcAddr,
    pub(crate) swap: ash::khr::swapchain::DeviceFn,
    set_loader_data: Option<SetDeviceLoaderData>,
    pub(crate) mem: vk::PhysicalDeviceMemoryProperties,
    instance: vk::Instance,
    /// Command pools per queue family, made on the first present on that family.
    pools: Mutex<HashMap<u32, vk::CommandPool>>,
}

struct Swap {
    device: u64,
    chain: chain::Chain,
}

#[derive(Default)]
struct State {
    instances: HashMap<u64, Arc<InstanceData>>,
    /// physical device → instance
    pdevs: HashMap<u64, u64>,
    devices: HashMap<u64, Arc<DeviceData>>,
    /// queue → (device, family)
    queues: HashMap<u64, (u64, u32)>,
    swaps: HashMap<u64, Arc<Mutex<Swap>>>,
}

fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default()))
}

fn st() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

fn instance_of_pdev(p: vk::PhysicalDevice) -> Option<Arc<InstanceData>> {
    let s = st();
    let i = s.pdevs.get(&p.as_raw()).copied();
    match i {
        Some(i) => s.instances.get(&i).cloned(),
        // only one instance in practice (gamescope): take it
        None => s.instances.values().next().cloned(),
    }
}

// ---------------------------------------------------------------------------------------------
// Entry points.

#[unsafe(no_mangle)]
pub unsafe extern "system" fn vkNegotiateLoaderLayerInterfaceVersion(
    v: *mut NegotiateLayerInterface,
) -> vk::Result {
    if v.is_null() {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    }
    unsafe {
        if (*v).loader_layer_interface_version > 2 {
            (*v).loader_layer_interface_version = 2;
        }
        (*v).gipa = numapilot_GetInstanceProcAddr as *const c_void;
        (*v).gdpa = numapilot_GetDeviceProcAddr as *const c_void;
        (*v).gpdpa = std::ptr::null();
    }
    vk::Result::SUCCESS
}

fn pfn<T>(f: T) -> vk::PFN_vkVoidFunction {
    debug_assert_eq!(
        std::mem::size_of::<T>(),
        std::mem::size_of::<*const c_void>()
    );
    let p: *const c_void = unsafe { std::mem::transmute_copy(&f) };
    unsafe { std::mem::transmute::<*const c_void, vk::PFN_vkVoidFunction>(p) }
}

fn device_fn(name: &CStr) -> vk::PFN_vkVoidFunction {
    match name.to_bytes() {
        b"vkGetDeviceProcAddr" => pfn(numapilot_GetDeviceProcAddr as vk::PFN_vkGetDeviceProcAddr),
        b"vkDestroyDevice" => pfn(numapilot_DestroyDevice as vk::PFN_vkDestroyDevice),
        b"vkGetDeviceQueue" => pfn(numapilot_GetDeviceQueue as vk::PFN_vkGetDeviceQueue),
        b"vkGetDeviceQueue2" => pfn(numapilot_GetDeviceQueue2 as vk::PFN_vkGetDeviceQueue2),
        b"vkCreateSwapchainKHR" => {
            pfn(numapilot_CreateSwapchainKHR as vk::PFN_vkCreateSwapchainKHR)
        }
        b"vkDestroySwapchainKHR" => {
            pfn(numapilot_DestroySwapchainKHR as vk::PFN_vkDestroySwapchainKHR)
        }
        b"vkGetSwapchainImagesKHR" => {
            pfn(numapilot_GetSwapchainImagesKHR as vk::PFN_vkGetSwapchainImagesKHR)
        }
        b"vkQueuePresentKHR" => pfn(numapilot_QueuePresentKHR as vk::PFN_vkQueuePresentKHR),
        _ => None,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn numapilot_GetInstanceProcAddr(
    instance: vk::Instance,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    if name.is_null() {
        return None;
    }
    let n = unsafe { CStr::from_ptr(name) };
    let ours = match n.to_bytes() {
        b"vkGetInstanceProcAddr" => {
            pfn(numapilot_GetInstanceProcAddr as vk::PFN_vkGetInstanceProcAddr)
        }
        b"vkCreateInstance" => pfn(numapilot_CreateInstance as vk::PFN_vkCreateInstance),
        b"vkDestroyInstance" => pfn(numapilot_DestroyInstance as vk::PFN_vkDestroyInstance),
        b"vkCreateDevice" => pfn(numapilot_CreateDevice as vk::PFN_vkCreateDevice),
        b"vkEnumeratePhysicalDevices" => {
            pfn(numapilot_EnumeratePhysicalDevices as vk::PFN_vkEnumeratePhysicalDevices)
        }
        _ => device_fn(n),
    };
    if ours.is_some() {
        return ours;
    }
    let data = st().instances.get(&instance.as_raw()).cloned();
    match data {
        Some(d) => unsafe { (d.gipa)(instance, name) },
        None => None,
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "system" fn numapilot_GetDeviceProcAddr(
    device: vk::Device,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    if name.is_null() {
        return None;
    }
    let n = unsafe { CStr::from_ptr(name) };
    let ours = device_fn(n);
    if ours.is_some() {
        return ours;
    }
    let data = st().devices.get(&device.as_raw()).cloned();
    match data {
        Some(d) => unsafe { (d.gdpa)(device, name) },
        None => None,
    }
}

unsafe extern "system" fn numapilot_CreateInstance(
    ci: *const vk::InstanceCreateInfo<'_>,
    alloc: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Instance,
) -> vk::Result {
    unsafe {
        let info = find_create_info(
            (*ci).p_next,
            vk::StructureType::LOADER_INSTANCE_CREATE_INFO,
            VK_LAYER_LINK_INFO,
        );
        if info.is_null() || (*info).u.is_null() {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        }
        let link = (*info).u as *mut LayerInstanceLink;
        let gipa = (*link).gipa;
        (*info).u = (*link).next as *mut c_void;
        let Some(create) = gipa(vk::Instance::null(), c"vkCreateInstance".as_ptr()) else {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        };
        let create: vk::PFN_vkCreateInstance = std::mem::transmute(create);
        let r = create(ci, alloc, out);
        if r != vk::Result::SUCCESS {
            return r;
        }
        let inst = *out;
        let load = |n: &CStr| -> *const c_void {
            gipa(inst, n.as_ptr()).map_or(std::ptr::null(), |f| f as *const c_void)
        };
        let data = InstanceData {
            gipa,
            fp: ash::InstanceFnV1_0::load(load),
            surface: ash::khr::surface::InstanceFn::load(load),
        };
        st().instances.insert(inst.as_raw(), Arc::new(data));
        r
    }
}

unsafe extern "system" fn numapilot_DestroyInstance(
    instance: vk::Instance,
    alloc: *const vk::AllocationCallbacks<'_>,
) {
    let data = {
        let mut s = st();
        s.pdevs.retain(|_, i| *i != instance.as_raw());
        s.instances.remove(&instance.as_raw())
    };
    if let Some(d) = data {
        unsafe { (d.fp.destroy_instance)(instance, alloc) };
    }
}

unsafe extern "system" fn numapilot_EnumeratePhysicalDevices(
    instance: vk::Instance,
    count: *mut u32,
    out: *mut vk::PhysicalDevice,
) -> vk::Result {
    let Some(d) = st().instances.get(&instance.as_raw()).cloned() else {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    };
    let r = unsafe { (d.fp.enumerate_physical_devices)(instance, count, out) };
    if !out.is_null() && (r == vk::Result::SUCCESS || r == vk::Result::INCOMPLETE) {
        let n = unsafe { *count } as usize;
        let mut s = st();
        for i in 0..n {
            let p = unsafe { *out.add(i) };
            s.pdevs.insert(p.as_raw(), instance.as_raw());
        }
    }
    r
}

unsafe extern "system" fn numapilot_CreateDevice(
    pdev: vk::PhysicalDevice,
    ci: *const vk::DeviceCreateInfo<'_>,
    alloc: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::Device,
) -> vk::Result {
    let Some(inst) = instance_of_pdev(pdev) else {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    };
    let instance_handle = {
        let s = st();
        s.pdevs
            .get(&pdev.as_raw())
            .copied()
            .or_else(|| s.instances.keys().next().copied())
            .unwrap_or(0)
    };
    unsafe {
        let info = find_create_info(
            (*ci).p_next,
            vk::StructureType::LOADER_DEVICE_CREATE_INFO,
            VK_LAYER_LINK_INFO,
        );
        if info.is_null() || (*info).u.is_null() {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        }
        let link = (*info).u as *mut LayerDeviceLink;
        let gipa = (*link).gipa;
        let gdpa = (*link).gdpa;
        (*info).u = (*link).next as *mut c_void;
        let cb_info = find_create_info(
            (*ci).p_next,
            vk::StructureType::LOADER_DEVICE_CREATE_INFO,
            VK_LOADER_DATA_CALLBACK,
        );
        let set_loader_data: Option<SetDeviceLoaderData> = if cb_info.is_null() {
            None
        } else {
            Some(std::mem::transmute::<*mut c_void, SetDeviceLoaderData>(
                (*cb_info).u,
            ))
        };
        let instance = vk::Instance::from_raw(instance_handle);
        let Some(create) = gipa(instance, c"vkCreateDevice".as_ptr()) else {
            return vk::Result::ERROR_INITIALIZATION_FAILED;
        };
        let create: vk::PFN_vkCreateDevice = std::mem::transmute(create);
        let r = create(pdev, ci, alloc, out);
        if r != vk::Result::SUCCESS {
            return r;
        }
        let device = *out;
        let mut ifp = inst.fp.clone();
        ifp.get_device_proc_addr = gdpa;
        let dev = ash::Device::load(&ifp, device);
        let swap = ash::khr::swapchain::DeviceFn::load(|n| {
            gdpa(device, n.as_ptr()).map_or(std::ptr::null(), |f| f as *const c_void)
        });
        let mut mem = vk::PhysicalDeviceMemoryProperties::default();
        (inst.fp.get_physical_device_memory_properties)(pdev, &mut mem);
        let data = DeviceData {
            dev,
            pdev,
            gdpa,
            swap,
            set_loader_data,
            mem,
            instance,
            pools: Mutex::new(HashMap::new()),
        };
        st().devices.insert(device.as_raw(), Arc::new(data));
        r
    }
}

unsafe extern "system" fn numapilot_DestroyDevice(
    device: vk::Device,
    alloc: *const vk::AllocationCallbacks<'_>,
) {
    let data = {
        let mut s = st();
        s.queues.retain(|_, (d, _)| *d != device.as_raw());
        let gone: Vec<u64> = s
            .swaps
            .iter()
            .filter(|(_, sw)| {
                sw.lock()
                    .map(|x| x.device == device.as_raw())
                    .unwrap_or(false)
            })
            .map(|(k, _)| *k)
            .collect();
        let swaps: Vec<_> = gone.iter().filter_map(|k| s.swaps.remove(k)).collect();
        (s.devices.remove(&device.as_raw()), swaps)
    };
    if let (Some(d), swaps) = data {
        unsafe {
            let _ = d.dev.device_wait_idle();
            for sw in swaps {
                if let Ok(mut sw) = sw.lock() {
                    sw.chain.destroy(&d);
                }
            }
            for (_, p) in d.pools.lock().unwrap_or_else(|e| e.into_inner()).drain() {
                d.dev.destroy_command_pool(p, None);
            }
            (d.dev.fp_v1_0().destroy_device)(device, alloc);
        }
    }
}

unsafe extern "system" fn numapilot_GetDeviceQueue(
    device: vk::Device,
    family: u32,
    index: u32,
    out: *mut vk::Queue,
) {
    let Some(d) = st().devices.get(&device.as_raw()).cloned() else {
        return;
    };
    unsafe {
        (d.dev.fp_v1_0().get_device_queue)(device, family, index, out);
        st().queues
            .insert((*out).as_raw(), (device.as_raw(), family));
    }
}

unsafe extern "system" fn numapilot_GetDeviceQueue2(
    device: vk::Device,
    info: *const vk::DeviceQueueInfo2<'_>,
    out: *mut vk::Queue,
) {
    let Some(d) = st().devices.get(&device.as_raw()).cloned() else {
        return;
    };
    unsafe {
        (d.dev.fp_v1_1().get_device_queue2)(device, info, out);
        st().queues.insert(
            (*out).as_raw(),
            (device.as_raw(), (*info).queue_family_index),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Swapchain. gamescope composes at its output size (-W/-H, then the window's size) whatever the
// surface says, so the layer cannot make it compose smaller. Instead gamescope scales the game
// with integer nearest scaling (`-S integer -F nearest`, set by numapilot's profile), each game
// pixel becomes a block, and the layer reads one texel per block to get the game's frame back.

/// The game's size inside gamescope: `NUMAPILOT_UPSCALE_SRC=WxH` (numapilot passes gamescope's
/// -w/-h), else half the window.
pub fn game_size(real: (u32, u32)) -> (u32, u32) {
    std::env::var("NUMAPILOT_UPSCALE_SRC")
        .ok()
        .and_then(|v| {
            let (w, h) = v.split_once('x')?;
            Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
        })
        .filter(|&(w, h): &(u32, u32)| w > 0 && h > 0)
        .unwrap_or(((real.0 / 2).max(1), (real.1 / 2).max(1)))
}

unsafe extern "system" fn numapilot_CreateSwapchainKHR(
    device: vk::Device,
    ci: *const vk::SwapchainCreateInfoKHR<'_>,
    alloc: *const vk::AllocationCallbacks<'_>,
    out: *mut vk::SwapchainKHR,
) -> vk::Result {
    let Some(d) = st().devices.get(&device.as_raw()).cloned() else {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    };
    let pass = || unsafe { (d.swap.create_swapchain_khr)(device, ci, alloc, out) };
    if !active() {
        return pass();
    }
    let info = unsafe { &*ci };
    let Some(px) = chain::format_px(info.image_format) else {
        eprintln!(
            "numapilot-upscale: format {:?} not handled: passing through",
            info.image_format
        );
        return pass();
    };
    let model = model_name();
    let Some(shader) = model_shader(&model) else {
        eprintln!("numapilot-upscale: unknown or not downloaded model '{model}': passing through");
        return pass();
    };
    let (shader, model) = if temporal() {
        (
            std::borrow::Cow::Owned(numapilot::vkbench::with_temporal(&shader)),
            format!("{model} + temporal"),
        )
    } else {
        (shader, model)
    };
    let Some(inst) = instance_of_pdev(d.pdev) else {
        return pass();
    };
    let mut caps = vk::SurfaceCapabilitiesKHR::default();
    let r = unsafe {
        (inst.surface.get_physical_device_surface_capabilities_khr)(d.pdev, info.surface, &mut caps)
    };
    if r != vk::Result::SUCCESS {
        return pass();
    }
    let real = info.image_extent;
    let game = game_size((real.width, real.height));
    let Some(src) = chain::Src::of((real.width, real.height), game) else {
        eprintln!(
            "numapilot-upscale: the {}x{} window is not at least 2× the game ({}x{}): passing through",
            real.width, real.height, game.0, game.1
        );
        return pass();
    };
    if !caps
        .supported_usage_flags
        .contains(vk::ImageUsageFlags::TRANSFER_DST)
    {
        log!("swapchain cannot be a copy target: passing through");
        return pass();
    }
    let mut ci2 = *info;
    ci2.image_usage |= vk::ImageUsageFlags::TRANSFER_DST;
    let r = unsafe { (d.swap.create_swapchain_khr)(device, &ci2, alloc, out) };
    if r != vk::Result::SUCCESS {
        return r;
    }
    let sc = unsafe { *out };
    match unsafe { chain::Chain::new(&d, sc, info, real, src, px, &shader) } {
        Ok(chain) => {
            eprintln!(
                "numapilot-upscale: game {}x{} (drawn {}× at {},{}) → {}x{} in a {}x{} window with {model}",
                src.size.0,
                src.size.1,
                src.stride,
                src.offset.0,
                src.offset.1,
                src.size.0 * 2,
                src.size.1 * 2,
                real.width,
                real.height
            );
            st().swaps.insert(
                sc.as_raw(),
                Arc::new(Mutex::new(Swap {
                    device: device.as_raw(),
                    chain,
                })),
            );
            vk::Result::SUCCESS
        }
        Err(e) => {
            // recreate it exactly as gamescope asked (no upscaling)
            eprintln!("numapilot-upscale: {e:#}; passing through");
            unsafe {
                (d.swap.destroy_swapchain_khr)(device, sc, alloc);
            }
            pass()
        }
    }
}

unsafe extern "system" fn numapilot_DestroySwapchainKHR(
    device: vk::Device,
    sc: vk::SwapchainKHR,
    alloc: *const vk::AllocationCallbacks<'_>,
) {
    let Some(d) = st().devices.get(&device.as_raw()).cloned() else {
        return;
    };
    let sw = st().swaps.remove(&sc.as_raw());
    if let Some(sw) = sw {
        unsafe {
            let _ = d.dev.device_wait_idle();
        }
        if let Ok(mut sw) = sw.lock() {
            unsafe { sw.chain.destroy(&d) };
        }
    }
    unsafe { (d.swap.destroy_swapchain_khr)(device, sc, alloc) };
}

unsafe extern "system" fn numapilot_GetSwapchainImagesKHR(
    device: vk::Device,
    sc: vk::SwapchainKHR,
    count: *mut u32,
    out: *mut vk::Image,
) -> vk::Result {
    let Some(d) = st().devices.get(&device.as_raw()).cloned() else {
        return vk::Result::ERROR_INITIALIZATION_FAILED;
    };
    let sw = st().swaps.get(&sc.as_raw()).cloned();
    let Some(sw) = sw else {
        return unsafe { (d.swap.get_swapchain_images_khr)(device, sc, count, out) };
    };
    let sw = sw.lock().unwrap_or_else(|e| e.into_inner());
    let imgs = sw.chain.images();
    unsafe {
        if out.is_null() {
            *count = imgs.len() as u32;
            return vk::Result::SUCCESS;
        }
        let n = (*count as usize).min(imgs.len());
        std::ptr::copy_nonoverlapping(imgs.as_ptr(), out, n);
        *count = n as u32;
        if n < imgs.len() {
            vk::Result::INCOMPLETE
        } else {
            vk::Result::SUCCESS
        }
    }
}

unsafe extern "system" fn numapilot_QueuePresentKHR(
    queue: vk::Queue,
    info: *const vk::PresentInfoKHR<'_>,
) -> vk::Result {
    let (dq, d) = {
        let s = st();
        let Some(&(dk, fam)) = s.queues.get(&queue.as_raw()) else {
            return vk::Result::ERROR_DEVICE_LOST;
        };
        let Some(d) = s.devices.get(&dk).cloned() else {
            return vk::Result::ERROR_DEVICE_LOST;
        };
        (fam, d)
    };
    let pi = unsafe { &*info };
    let scs = unsafe { std::slice::from_raw_parts(pi.p_swapchains, pi.swapchain_count as usize) };
    let idx =
        unsafe { std::slice::from_raw_parts(pi.p_image_indices, pi.swapchain_count as usize) };
    let tracked: Vec<(Arc<Mutex<Swap>>, u32)> = {
        let s = st();
        scs.iter()
            .zip(idx)
            .filter_map(|(sc, &i)| s.swaps.get(&sc.as_raw()).map(|w| (w.clone(), i)))
            .collect()
    };
    if tracked.is_empty() {
        return unsafe { (d.swap.queue_present_khr)(queue, info) };
    }
    let mut waits: Vec<vk::Semaphore> = if pi.wait_semaphore_count > 0 {
        unsafe {
            std::slice::from_raw_parts(pi.p_wait_semaphores, pi.wait_semaphore_count as usize)
                .to_vec()
        }
    } else {
        vec![]
    };
    let mut signals = vec![];
    for (sw, i) in tracked {
        let mut sw = sw.lock().unwrap_or_else(|e| e.into_inner());
        match unsafe { sw.chain.run(&d, queue, dq, i, &waits) } {
            Ok(sem) => {
                // later chains (several swapchains) wait on the previous one
                waits = vec![sem];
                signals.push(sem);
            }
            Err(e) => {
                eprintln!("numapilot-upscale: present: {e:#}");
                return vk::Result::ERROR_DEVICE_LOST;
            }
        }
    }
    // the present waits on the last chain, which waited on everything before it
    let last = [*signals.last().expect("one chain at least")];
    let mut pi2 = *pi;
    pi2.wait_semaphore_count = 1;
    pi2.p_wait_semaphores = last.as_ptr();
    unsafe { (d.swap.queue_present_khr)(queue, &pi2) }
}

impl DeviceData {
    /// Command pool for a queue family (made once).
    pub(crate) unsafe fn pool(&self, family: u32) -> ash::prelude::VkResult<vk::CommandPool> {
        let mut pools = self.pools.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = pools.get(&family) {
            return Ok(*p);
        }
        let p = unsafe {
            self.dev.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )
        }?;
        pools.insert(family, p);
        Ok(p)
    }

    /// A command buffer made by the layer needs the loader's dispatch table.
    pub(crate) unsafe fn set_dispatch(&self, cb: vk::CommandBuffer) {
        let device = self.dev.handle();
        unsafe {
            match self.set_loader_data {
                Some(f) => {
                    let _ = f(device, cb.as_raw() as *mut c_void);
                }
                None => {
                    *(cb.as_raw() as *mut *const c_void) =
                        *(device.as_raw() as *const *const c_void);
                }
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn instance(&self) -> vk::Instance {
        self.instance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processes_and_models() {
        assert!(proc_matches("gamescope", "gamescope,gamescope-wl"));
        assert!(proc_matches("gamescope-wl", "gamescope, gamescope-wl"));
        assert!(!proc_matches("skyrim.exe", "gamescope,gamescope-wl"));
        assert!(proc_matches("vkcube", "*"));
        assert!(!proc_matches("gamescope", ""));
        for m in [
            "anime4k-s",
            "anime4k-m",
            "anime4k-l",
            "bilinear",
            "artcnn-c4f16",
            "artcnn-c4f32",
        ] {
            assert!(model_shader(m).is_some(), "{m}");
        }
        assert!(model_shader("rife").is_none());
    }

    #[test]
    fn game_sizes() {
        // without NUMAPILOT_UPSCALE_SRC (not set in tests): half the window
        if std::env::var_os("NUMAPILOT_UPSCALE_SRC").is_none() {
            assert_eq!(game_size((2560, 1440)), (1280, 720));
            assert_eq!(game_size((1, 1)), (1, 1));
        }
    }
}
