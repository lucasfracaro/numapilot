//! Undervolt automático: desce a voltagem (AMD) ou desloca a curva com o clock travado (NVIDIA)
//! um passo por vez, roda um teste de carga em cada passo e para no primeiro erro. No fim volta
//! alguns passos de margem e grava. Roda como usuário: os ajustes vão pelo serviço em modo de
//! teste (não gravados no disco), então se o PC travar ele volta com o último valor bom.
//!
//! Erro = o teste de carga cair, o kernel registrar travamento/reset da GPU (amdgpu: ring
//! timeout, GPU reset, page fault; NVIDIA: Xid) ou a placa sumir.

use crate::ctl::{Request, send};
use crate::gpuctl::{self, GpuControl, GpuControls, OdSettings};
use crate::topo::{Gpu, Topology};
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Ctrl+C: para o teste e volta ao que era antes.
pub static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigint(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

pub fn catch_ctrl_c() {
    unsafe {
        libc::signal(libc::SIGINT, on_sigint as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_sigint as *const () as libc::sighandler_t);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Method {
    /// NVIDIA: clock máximo travado + deslocamento positivo da curva (MHz).
    NvOffset { clock_max: u32, limit: i32 },
    /// AMD RDNA2/3/4: deslocamento de voltagem negativo (mV).
    AmdOffset { limit: i32 },
    /// AMD Vega20/RDNA1: voltagem do ponto mais alto da curva (mV), mesmo clock.
    AmdCurve {
        mhz: u32,
        start_mv: u32,
        min_mv: u32,
    },
}

#[derive(Debug, Clone)]
pub struct Opts {
    pub secs: u64,
    /// Passos de margem no fim (1 = volta um passo do último que passou).
    pub margin: u32,
    /// NVIDIA: clock máximo para travar (None = 90% do máximo da placa).
    pub clock: Option<u32>,
    /// Tamanho do passo (MHz na NVIDIA, mV na AMD).
    pub step: Option<i32>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            secs: 60,
            margin: 1,
            clock: None,
            step: None,
        }
    }
}

pub fn method(g: &Gpu, base: &GpuControl, clock: Option<u32>) -> Result<Method, String> {
    if g.is_nvidia() {
        let max = crate::nvctl::get(&g.pci)
            .and_then(|n| n.clock_max_mhz)
            .unwrap_or(2000.0) as u32;
        let clock_max = clock
            .or(base.clock_max_mhz)
            .unwrap_or(max * 9 / 10 / 15 * 15);
        return Ok(Method::NvOffset {
            clock_max,
            limit: 300,
        });
    }
    let od = gpuctl::read_od(&g.pci).ok_or("OverDrive desligado: ligue em Configurações → Opções de boot (amdgpu.ppfeaturemask) e reinicie")?;
    if let Some((lo, _)) = od.range_vo {
        return Ok(Method::AmdOffset {
            limit: lo.max(-200),
        });
    }
    if let Some(&(_, mhz, mv)) = od.curve.last() {
        let min_mv = od
            .range_curve_volt
            .last()
            .map(|r| r.0)
            .unwrap_or(700)
            .max(mv.saturating_sub(250));
        let start_mv = base
            .od
            .as_ref()
            .and_then(|o| o.curve.last().map(|p| p.1))
            .unwrap_or(mv);
        return Ok(Method::AmdCurve {
            mhz,
            start_mv,
            min_mv,
        });
    }
    Err("esta placa não expõe voltagem (nem curva nem deslocamento) no pp_od_clk_voltage".into())
}

/// O ajuste do passo `n` (0 = ponto de partida).
pub fn control_at(base: &GpuControl, m: &Method, n: i32, step: i32) -> Option<GpuControl> {
    let mut c = base.clone();
    match m {
        Method::NvOffset { clock_max, limit } => {
            let start = base.nv_gpc_offset_mhz.unwrap_or(0).max(0);
            let v = start + n * step;
            if v > *limit {
                return None;
            }
            c.clock_max_mhz = Some(*clock_max);
            c.nv_gpc_offset_mhz = Some(v);
        }
        Method::AmdOffset { limit } => {
            let start = base
                .od
                .as_ref()
                .and_then(|o| o.voltage_offset_mv)
                .unwrap_or(0)
                .min(0);
            let v = start - n * step;
            if v < *limit {
                return None;
            }
            let mut od = base.od.clone().unwrap_or(OdSettings {
                sclk_max: None,
                mclk_max: None,
                curve: vec![],
                voltage_offset_mv: None,
            });
            od.voltage_offset_mv = Some(v);
            c.od = Some(od);
        }
        Method::AmdCurve {
            mhz,
            start_mv,
            min_mv,
        } => {
            let v = *start_mv as i32 - n * step;
            if v < *min_mv as i32 {
                return None;
            }
            let mut od = base.od.clone().unwrap_or(OdSettings {
                sclk_max: None,
                mclk_max: None,
                curve: vec![],
                voltage_offset_mv: None,
            });
            if od.curve.is_empty() {
                // os outros pontos ficam como estão na placa
                if let Some(o) = base.od.as_ref() {
                    od.curve = o.curve.clone();
                }
            }
            match od.curve.len() {
                0 => od.curve = vec![(*mhz, v as u32)],
                _ => {
                    let last = od.curve.len() - 1;
                    od.curve[last] = (*mhz, v as u32);
                }
            }
            c.od = Some(od);
        }
    }
    Some(c)
}

pub fn describe(c: &GpuControl, m: &Method) -> String {
    match m {
        Method::NvOffset { .. } => format!(
            "clock até {} MHz, curva {:+} MHz",
            c.clock_max_mhz.unwrap_or(0),
            c.nv_gpc_offset_mhz.unwrap_or(0)
        ),
        Method::AmdOffset { .. } => format!(
            "voltagem {:+} mV",
            c.od.as_ref().and_then(|o| o.voltage_offset_mv).unwrap_or(0)
        ),
        Method::AmdCurve { mhz, .. } => format!(
            "{mhz} MHz a {} mV",
            c.od.as_ref()
                .and_then(|o| o.curve.last())
                .map(|p| p.1)
                .unwrap_or(0)
        ),
    }
}

/// Linhas do kernel que indicam GPU instável.
pub fn gpu_errors(log: &str, g_pci: &str, nvidia: bool) -> Vec<String> {
    // "0000:03:00.0" -> "03:00" (o NVRM escreve "PCI:0000:03:00", sem a função)
    let short = g_pci
        .trim_start_matches("0000:")
        .split('.')
        .next()
        .unwrap_or(g_pci);
    log.lines()
        .filter(|l| {
            let lo = l.to_lowercase();
            if nvidia {
                lo.contains("nvrm: xid") && (lo.contains(short) || !lo.contains("pci:"))
            } else {
                (lo.contains(short) || !lo.contains("0000:"))
                    && lo.contains("amdgpu")
                    && (lo.contains("ring") && lo.contains("timeout")
                        || lo.contains("gpu reset")
                        || lo.contains("page fault")
                        || lo.contains("hang")
                        || lo.contains("failed to initialize"))
            }
        })
        .map(String::from)
        .collect()
}

fn kernel_log_since(t: std::time::SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Command::new("journalctl")
        .args([
            "-k",
            "--no-pager",
            "-o",
            "cat",
            "--since",
            &format!("@{secs}"),
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/// Programa de carga: vkmark sem janela (funciona até em placa sem monitor) ou glmark2.
pub fn stress_cmd() -> Option<Vec<String>> {
    if crate::containers::which("vkmark") {
        return Some(vec![
            "vkmark".into(),
            "--winsys".into(),
            "headless".into(),
            "-s".into(),
            "1920x1080".into(),
        ]);
    }
    if crate::containers::which("glmark2") {
        return Some(vec![
            "glmark2".into(),
            "--off-screen".into(),
            "--run-forever".into(),
            "-s".into(),
            "1920x1080".into(),
        ]);
    }
    None
}

fn stress_env(g: &Gpu, topo: &Topology) -> Vec<(String, String)> {
    let d = crate::decide::Decision {
        nodes: vec![],
        cpus: vec![],
        gpus: vec![g.clone()],
        kind: crate::decide::Kind::Graphics,
        mem: crate::config::MemSpec::Local,
        manual: true,
        limit_visible: false,
        priority: "normal".into(),
        needs_daemon: false,
        fg_gpu: None,
        reasons: vec![],
    };
    crate::decide::gpu_env(&d, topo).0
}

/// Roda a carga por `secs`. Ok se aguentou; Err com o motivo se caiu ou o kernel reclamou.
fn stress(g: &Gpu, topo: &Topology, secs: u64, out: &mut dyn FnMut(&str)) -> Result<(), String> {
    let cmd =
        stress_cmd().ok_or("instale o vkmark ou o glmark2 (sudo pacman -S vkmark glmark2)")?;
    let since = std::time::SystemTime::now();
    let env = stress_env(g, topo);
    let spawn = || {
        Command::new(&cmd[0])
            .args(&cmd[1..])
            .envs(env.clone())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{}: {e}", cmd[0]))
    };
    let mut ch = spawn()?;
    let t0 = Instant::now();
    let mut last = 0;
    let res = loop {
        std::thread::sleep(Duration::from_millis(500));
        if STOP.load(Ordering::SeqCst) {
            break Err("interrompido".into());
        }
        if let Ok(Some(st)) = ch.try_wait() {
            // terminou a rodada normalmente: começa outra
            if st.success() {
                ch = match spawn() {
                    Ok(c) => c,
                    Err(e) => break Err(e),
                };
            } else {
                break Err(format!("o teste de carga caiu ({st})"));
            }
        }
        let el = t0.elapsed().as_secs();
        if el / 10 != last {
            last = el / 10;
            let s = gpuctl::read_sensors(&g.pci);
            out(&format!(
                "    {el:>3}s  {}  {}",
                s.junction_c
                    .or(s.edge_c)
                    .map(|t| format!("{t:.0} °C"))
                    .unwrap_or_default(),
                s.power_w.map(|p| format!("{p:.0} W")).unwrap_or_default()
            ));
        }
        let errs = gpu_errors(&kernel_log_since(since), &g.pci, g.is_nvidia());
        if let Some(e) = errs.first() {
            break Err(format!("kernel: {e}"));
        }
        if el >= secs {
            break Ok(());
        }
    };
    let _ = ch.kill();
    let _ = ch.wait();
    std::thread::sleep(Duration::from_secs(1));
    if res.is_ok()
        && let Some(e) = gpu_errors(&kernel_log_since(since), &g.pci, g.is_nvidia()).first()
    {
        return Err(format!("kernel: {e}"));
    }
    res
}

fn set(pci: &str, c: &GpuControl, trial: bool) -> Result<String, String> {
    let r = send(&Request::SetGpuControl {
        pci: pci.into(),
        control: c.clone(),
        trial,
    })
    .map_err(|e| format!("serviço numapilot: {e}"))?;
    if r.ok { Ok(r.message) } else { Err(r.message) }
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub saved: GpuControl,
    pub description: String,
    pub steps_ok: u32,
}

pub fn run(
    topo: &Topology,
    g: &Gpu,
    opts: &Opts,
    out: &mut dyn FnMut(&str),
) -> Result<Outcome, String> {
    let base = GpuControls::load().get(&g.pci);
    let m = method(g, &base, opts.clock)?;
    let step = opts.step.unwrap_or(match m {
        Method::NvOffset { .. } => 15,
        _ => 10,
    });
    stress_cmd().ok_or("instale o vkmark ou o glmark2 (sudo pacman -S vkmark glmark2)")?;
    out(&format!(
        "{}: undervolt automático, {} s por passo",
        g.alias, opts.secs
    ));
    let mut good = base.clone();
    let mut ok_steps = 0u32;
    let mut n = 0;
    // Passo 0: o ponto de partida também precisa aguentar (na NVIDIA, já com o clock travado).
    loop {
        let Some(c) = control_at(&base, &m, n, step) else {
            out("  chegou ao limite seguro: parando aqui");
            break;
        };
        out(&format!("  passo {n}: {}", describe(&c, &m)));
        if let Err(e) = set(&g.pci, &c, true) {
            let _ = set(&g.pci, &good, true);
            if n == 0 {
                let _ = set(&g.pci, &base, false);
                return Err(e);
            }
            out(&format!("  a placa recusou: {e}"));
            break;
        }
        match stress(g, topo, opts.secs, out) {
            Ok(()) => {
                out("    passou");
                good = c;
                ok_steps = n as u32;
                n += 1;
            }
            Err(_) if STOP.load(Ordering::SeqCst) => {
                let _ = set(&g.pci, &base, false);
                return Err("interrompido: voltei ao que era antes".into());
            }
            Err(e) => {
                out(&format!("    falhou: {e}"));
                if n == 0 {
                    let _ = set(&g.pci, &base, false);
                    return Err("nem o ponto de partida aguentou: nada foi mudado".into());
                }
                break;
            }
        }
    }
    // Margem: volta alguns passos do último que passou.
    let keep = (ok_steps as i32 - opts.margin as i32).max(0);
    let mut final_c = control_at(&base, &m, keep, step).unwrap_or(good.clone());
    if keep == 0 && ok_steps == 0 {
        final_c = good.clone();
    }
    out(&format!(
        "  com {} passo(s) de margem: {}",
        opts.margin,
        describe(&final_c, &m)
    ));
    set(&g.pci, &final_c, true)?;
    out(&format!("  confirmação: {} s", opts.secs * 2));
    if let Err(e) = stress(g, topo, opts.secs * 2, out) {
        let _ = set(&g.pci, &base, false);
        if STOP.load(Ordering::SeqCst) {
            return Err("interrompido: voltei ao que era antes".into());
        }
        return Err(format!(
            "a confirmação falhou ({e}): voltei ao que era antes"
        ));
    }
    let msg = set(&g.pci, &final_c, false)?;
    out(&format!("  gravado: {msg}"));
    let _ = std::io::stdout().flush();
    Ok(Outcome {
        description: describe(&final_c, &m),
        saved: final_c,
        steps_ok: ok_steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps() {
        let base = GpuControl::default();
        let m = Method::NvOffset {
            clock_max: 1800,
            limit: 300,
        };
        let c = control_at(&base, &m, 3, 15).unwrap();
        assert_eq!(
            (c.clock_max_mhz, c.nv_gpc_offset_mhz),
            (Some(1800), Some(45))
        );
        assert!(control_at(&base, &m, 21, 15).is_none());
        let m = Method::AmdOffset { limit: -100 };
        let c = control_at(&base, &m, 4, 10).unwrap();
        assert_eq!(c.od.unwrap().voltage_offset_mv, Some(-40));
        assert!(control_at(&base, &m, 11, 10).is_none());
        let m = Method::AmdCurve {
            mhz: 1800,
            start_mv: 1100,
            min_mv: 950,
        };
        let c = control_at(&base, &m, 5, 10).unwrap();
        assert_eq!(c.od.unwrap().curve.last(), Some(&(1800, 1050)));
    }

    #[test]
    fn kernel_errors() {
        let log = "amdgpu 0000:03:00.0: amdgpu: ring gfx_0.0.0 timeout, signaled seq=1\nnada\namdgpu 0000:04:00.0: amdgpu: GPU reset begin!\n";
        assert_eq!(gpu_errors(log, "0000:03:00.0", false).len(), 1);
        assert_eq!(gpu_errors(log, "0000:04:00.0", false).len(), 1);
        let nv = "NVRM: Xid (PCI:0000:01:00): 79, pid=1, GPU has fallen off the bus.\n";
        assert_eq!(gpu_errors(nv, "0000:01:00.0", true).len(), 1);
        assert!(gpu_errors(nv, "0000:02:00.0", true).is_empty());
    }
}
