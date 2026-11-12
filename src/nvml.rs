//! Pedaço da NVML (libnvidia-ml, que vem com o driver) carregado em tempo de execução:
//! deslocamento de clock da GPU e da memória. É o "undervolt" possível na NVIDIA pelo Linux:
//! com um deslocamento positivo, cada ponto da curva clock/voltagem sobe de clock, então o clock
//! desejado é alcançado com voltagem menor; junto com o clock máximo travado (nvidia-smi -lgc),
//! a placa roda no mesmo clock gastando menos. Funciona sem X (diferente do nvidia-settings).
//! Precisa de root.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::OnceLock;

type Ret = c_int;
type Dev = *mut c_void;

struct Lib {
    init: unsafe extern "C" fn() -> Ret,
    shutdown: unsafe extern "C" fn() -> Ret,
    by_pci: unsafe extern "C" fn(*const c_char, *mut Dev) -> Ret,
    err: unsafe extern "C" fn(Ret) -> *const c_char,
    get_gpc: Option<unsafe extern "C" fn(Dev, *mut c_int) -> Ret>,
    set_gpc: Option<unsafe extern "C" fn(Dev, c_int) -> Ret>,
    get_mem: Option<unsafe extern "C" fn(Dev, *mut c_int) -> Ret>,
    set_mem: Option<unsafe extern "C" fn(Dev, c_int) -> Ret>,
    gpc_range: Option<unsafe extern "C" fn(Dev, *mut c_int, *mut c_int) -> Ret>,
    mem_range: Option<unsafe extern "C" fn(Dev, *mut c_int, *mut c_int) -> Ret>,
}

unsafe impl Send for Lib {}
unsafe impl Sync for Lib {}

fn sym<T>(h: *mut c_void, name: &str) -> Option<T> {
    let c = CString::new(name).ok()?;
    let p = unsafe { libc::dlsym(h, c.as_ptr()) };
    (!p.is_null()).then(|| unsafe { std::mem::transmute_copy::<*mut c_void, T>(&p) })
}

fn lib() -> Result<&'static Lib, String> {
    static L: OnceLock<Result<Lib, String>> = OnceLock::new();
    L.get_or_init(|| {
        let name = std::env::var("NUMAPILOT_NVML").unwrap_or_else(|_| "libnvidia-ml.so.1".into());
        let c = CString::new(name.clone()).map_err(|e| e.to_string())?;
        let h = unsafe { libc::dlopen(c.as_ptr(), libc::RTLD_NOW) };
        if h.is_null() {
            return Err(crate::tf!(
                "{name} not found (NVIDIA driver installed?)",
                "{name} não encontrada (driver da NVIDIA instalado?)"
            ));
        }
        let l = Lib {
            init: sym(h, "nvmlInit_v2").ok_or(crate::i18n::tr("nvmlInit_v2 missing"))?,
            shutdown: sym(h, "nvmlShutdown").ok_or(crate::i18n::tr("nvmlShutdown missing"))?,
            by_pci: sym(h, "nvmlDeviceGetHandleByPciBusId_v2")
                .ok_or(crate::i18n::tr("nvmlDeviceGetHandleByPciBusId_v2 missing"))?,
            err: sym(h, "nvmlErrorString").ok_or(crate::i18n::tr("nvmlErrorString missing"))?,
            get_gpc: sym(h, "nvmlDeviceGetGpcClkVfOffset"),
            set_gpc: sym(h, "nvmlDeviceSetGpcClkVfOffset"),
            get_mem: sym(h, "nvmlDeviceGetMemClkVfOffset"),
            set_mem: sym(h, "nvmlDeviceSetMemClkVfOffset"),
            gpc_range: sym(h, "nvmlDeviceGetGpcClkMinMaxVfOffset"),
            mem_range: sym(h, "nvmlDeviceGetMemClkMinMaxVfOffset"),
        };
        Ok(l)
    })
    .as_ref()
    .map_err(|e| e.clone())
}

fn errstr(l: &Lib, r: Ret) -> String {
    let p = unsafe { (l.err)(r) };
    if p.is_null() {
        return crate::tf!("error {r}", "erro {r}");
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().to_string();
    match r {
        3 => crate::tf!(
            "{s} (this model/driver does not allow changing the offset)",
            "{s} (este modelo/driver não deixa mudar o deslocamento)"
        ),
        4 => crate::tf!("{s} (needs root)", "{s} (precisa de root)"),
        _ => s,
    }
}

fn check(l: &Lib, r: Ret) -> Result<(), String> {
    if r == 0 { Ok(()) } else { Err(errstr(l, r)) }
}

fn dev(l: &Lib, pci: &str) -> Result<Dev, String> {
    let c = CString::new(pci).map_err(|e| e.to_string())?;
    let mut d: Dev = std::ptr::null_mut();
    check(l, unsafe { (l.by_pci)(c.as_ptr(), &mut d) })?;
    Ok(d)
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Offsets {
    pub gpc_mhz: Option<i32>,
    pub mem_mhz: Option<i32>,
    pub gpc_range: Option<(i32, i32)>,
    pub mem_range: Option<(i32, i32)>,
}

/// NVML aberta só durante a chamada. Aberta o tempo todo, ela segura /dev/nvidia* e a placa
/// do notebook não pode ser desligada ("NVIDIA em uso: numapilot").
struct Session(&'static Lib);

impl Drop for Session {
    fn drop(&mut self) {
        unsafe { (self.0.shutdown)() };
    }
}

fn session() -> Result<Session, String> {
    let l = lib()?;
    let r = unsafe { (l.init)() };
    if r != 0 {
        return Err(format!("nvmlInit: {}", errstr(l, r)));
    }
    Ok(Session(l))
}

pub fn offsets(pci: &str) -> Result<Offsets, String> {
    let s = session()?;
    let l = s.0;
    let d = dev(l, pci)?;
    let one = |f: Option<unsafe extern "C" fn(Dev, *mut c_int) -> Ret>| {
        let f = f?;
        let mut v: c_int = 0;
        (unsafe { f(d, &mut v) } == 0).then_some(v)
    };
    let two = |f: Option<unsafe extern "C" fn(Dev, *mut c_int, *mut c_int) -> Ret>| {
        let f = f?;
        let (mut a, mut b): (c_int, c_int) = (0, 0);
        (unsafe { f(d, &mut a, &mut b) } == 0).then_some((a, b))
    };
    Ok(Offsets {
        gpc_mhz: one(l.get_gpc),
        mem_mhz: one(l.get_mem),
        gpc_range: two(l.gpc_range),
        mem_range: two(l.mem_range),
    })
}

/// Deslocamento do clock da GPU (MHz). 0 = de fábrica.
pub fn set_gpc_offset(pci: &str, mhz: i32) -> Result<(), String> {
    let s = session()?;
    let l = s.0;
    let f = l.set_gpc.ok_or(crate::i18n::tr(
        "this driver has no nvmlDeviceSetGpcClkVfOffset",
    ))?;
    check(l, unsafe { f(dev(l, pci)?, mhz as c_int) })
}

/// Deslocamento do clock da memória (MHz). 0 = de fábrica.
pub fn set_mem_offset(pci: &str, mhz: i32) -> Result<(), String> {
    let s = session()?;
    let l = s.0;
    let f = l.set_mem.ok_or(crate::i18n::tr(
        "this driver has no nvmlDeviceSetMemClkVfOffset",
    ))?;
    check(l, unsafe { f(dev(l, pci)?, mhz as c_int) })
}

/// O limite que a NVML aceita para um valor (algumas placas devolvem faixas enormes; o
/// numapilot usa no máximo +300 na GPU e +1500 na memória).
pub fn clamp(v: i32, range: Option<(i32, i32)>, hard: i32) -> i32 {
    let (lo, hi) = range.unwrap_or((-hard, hard));
    v.clamp(lo.max(-hard), hi.min(hard))
}

#[cfg(test)]
mod tests {
    #[test]
    fn clamp() {
        assert_eq!(super::clamp(500, Some((-1000, 1000)), 300), 300);
        assert_eq!(super::clamp(-50, Some((0, 200)), 300), 0);
        assert_eq!(super::clamp(120, None, 300), 120);
    }
}

#[cfg(test)]
mod stub_tests {
    /// Com a biblioteca de mentira de /tmp/nvmlstub (gcc), se existir.
    #[test]
    fn stub() {
        let p = "/tmp/nvmlstub/libnvml.so";
        if !std::path::Path::new(p).exists() {
            return;
        }
        unsafe { std::env::set_var("NUMAPILOT_NVML", p) };
        let o = super::offsets("0000:01:00.0").unwrap();
        assert_eq!(o.gpc_range, Some((-200, 1000)));
        super::set_gpc_offset("0000:01:00.0", super::clamp(500, o.gpc_range, 300)).unwrap();
        assert_eq!(super::offsets("0000:01:00.0").unwrap().gpc_mhz, Some(300));
        let e = super::set_mem_offset("0000:01:00.0", 100).unwrap_err();
        assert!(e.contains("Not Supported"), "{e}");
        assert!(super::offsets("0000:09:00.0").is_err());
    }
}
