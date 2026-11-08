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

/// Programas de carga possíveis: vkmark (sem janela e com janela) e a cena mais pesada do
/// glmark2 (terrain). Alguns caem ou puxam pouco com certos drivers: `pick_stress` mede e escolhe.
pub fn stress_cmds() -> Vec<Vec<String>> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<String>>();
    let mut v = vec![];
    if crate::containers::which("vkmark") {
        v.push(s(&["vkmark", "--winsys", "headless", "-s", "1920x1080"]));
        v.push(s(&["vkmark", "-s", "1920x1080"]));
    }
    let terrain = ["--run-forever", "-s", "1920x1080", "-b", "terrain"];
    if crate::containers::which("glmark2-wayland") {
        v.push(s(&[&["glmark2-wayland"][..], &terrain].concat()));
    }
    if crate::containers::which("glmark2") {
        v.push(s(&[&["glmark2"][..], &terrain].concat()));
        v.push(s(&[&["glmark2", "--off-screen"][..], &terrain].concat()));
    }
    v
}

/// Algum programa de carga instalado?
pub fn stress_cmd() -> Option<Vec<String>> {
    stress_cmds().into_iter().next()
}

/// Clock atual do núcleo (MHz).
fn clock_now(g: &Gpu) -> Option<f64> {
    if g.is_nvidia() {
        return crate::nvctl::get(&g.pci).and_then(|n| n.clock_mhz);
    }
    if let Some(c) = crate::gpuctl::read_sensors(&g.pci).clock_mhz {
        return Some(c);
    }
    let t = std::fs::read_to_string(crate::util::sys(&format!(
        "bus/pci/devices/{}/pp_dpm_sclk",
        g.pci
    )))
    .ok()?;
    t.lines()
        .find(|l| l.trim_end().ends_with('*'))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| {
            v.trim_end_matches("Mhz")
                .trim_end_matches("MHz")
                .parse()
                .ok()
        })
        .and_then(crate::gpuctl::plausible_clock)
}

/// Acha o programa de carga que mais puxa esta placa (cada um roda 8 s).
fn pick_stress(g: &Gpu, topo: &Topology, out: &mut dyn FnMut(&str)) -> Result<Vec<String>, String> {
    let env = stress_env(g, topo);
    let cands = stress_cmds();
    if cands.is_empty() {
        return Err("instale o vkmark ou o glmark2 (sudo pacman -S vkmark glmark2)".into());
    }
    out("  escolhendo o programa de carga que mais puxa a placa (8 s cada):");
    let mut best: Option<(u64, Vec<String>)> = None;
    for c in cands {
        if STOP.load(Ordering::SeqCst) {
            return Err("interrompido: nada foi mudado".into());
        }
        let Ok(mut ch) = Command::new(&c[0])
            .args(&c[1..])
            .envs(env.clone())
            .env_remove("VK_DRIVER_FILES")
            .env_remove("VK_ICD_FILENAMES")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        let t0 = Instant::now();
        let mut ok = true;
        let mut util = 0u64;
        let mut clock = None;
        while t0.elapsed() < Duration::from_secs(8) {
            std::thread::sleep(Duration::from_millis(500));
            if let Ok(Some(st)) = ch.try_wait() {
                ok = st.success();
                break;
            }
            // mede depois de esquentar
            if t0.elapsed() >= Duration::from_secs(4) {
                util = util.max(g.busy_percent().unwrap_or(0));
                clock = clock_now(g).or(clock);
            }
        }
        let _ = ch.kill();
        let _ = ch.wait();
        if !ok {
            out(&format!("    {}: caiu ao abrir", c.join(" ")));
            continue;
        }
        out(&format!(
            "    {}: {util}% de uso{}",
            c.join(" "),
            clock.map(|c| format!(", {c:.0} MHz")).unwrap_or_default()
        ));
        if best.as_ref().is_none_or(|(u, _)| util > *u) {
            best = Some((util, c));
        }
        // já puxa a placa toda: não precisa testar os outros
        if util >= 97 {
            break;
        }
    }
    let Some((util, c)) = best else {
        return Err(
            "nenhum programa de carga roda nesta placa (vkmark e glmark2 caíram ao abrir): nada foi mudado"
                .into(),
        );
    };
    if util < 80 {
        return Err(format!(
            "o programa de carga mais pesado só usou {util}% da placa: assim o teste não prova nada \
             (a placa fica em clock baixo, onde o ajuste não faz diferença). Nada foi mudado."
        ));
    }
    out(&format!("  teste de carga: {}", c.join(" ")));
    Ok(c)
}

/// Médias de um passo que aguentou.
#[derive(Debug, Clone, Default)]
pub struct StepStats {
    pub clock_mhz: Option<f64>,
    pub power_w: Option<f64>,
    pub util_pct: Option<f64>,
}

impl StepStats {
    pub fn line(&self) -> String {
        [
            self.clock_mhz.map(|v| format!("{v:.0} MHz")),
            self.power_w.map(|v| format!("{v:.0} W")),
            self.util_pct.map(|v| format!("{v:.0}% de uso")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ")
    }
}

fn avg(v: &[f64]) -> Option<f64> {
    (!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

fn stress_env(g: &Gpu, topo: &Topology) -> Vec<(String, String)> {
    crate::decide::env_for_gpu(g, topo)
}

/// Roda a carga por `secs`. Ok (com as médias) se aguentou; Err com o motivo se caiu ou o kernel reclamou.
fn stress(
    g: &Gpu,
    topo: &Topology,
    cmd: &[String],
    secs: u64,
    out: &mut dyn FnMut(&str),
) -> Result<StepStats, String> {
    let since = std::time::SystemTime::now();
    let env = stress_env(g, topo);
    let spawn = || {
        Command::new(&cmd[0])
            .args(&cmd[1..])
            .envs(env.clone())
            .env_remove("VK_DRIVER_FILES")
            .env_remove("VK_ICD_FILENAMES")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{}: {e}", cmd[0]))
    };
    let mut ch = spawn()?;
    let t0 = Instant::now();
    let mut last = 0;
    let mut crashes = 0;
    let (mut clocks, mut watts, mut utils) = (vec![], vec![], vec![]);
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
            } else if crashes == 0
                && gpu_errors(&kernel_log_since(since), &g.pci, g.is_nvidia()).is_empty()
            {
                // caiu sem a placa reclamar no kernel: pode ser o programa; dá mais uma chance
                crashes += 1;
                out(&format!(
                    "    o programa de teste caiu ({st}) sem erro da placa; tentando de novo"
                ));
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
            let ck = clock_now(g);
            let u = g.busy_percent();
            clocks.extend(ck);
            watts.extend(s.power_w);
            utils.extend(u.map(|v| v as f64));
            out(&format!(
                "    {el:>3}s  {}  {}  {}  {}",
                s.junction_c
                    .or(s.edge_c)
                    .map(|t| format!("{t:.0} °C"))
                    .unwrap_or_default(),
                s.power_w.map(|p| format!("{p:.0} W")).unwrap_or_default(),
                ck.map(|c| format!("{c:.0} MHz")).unwrap_or_default(),
                u.map(|u| format!("{u}% uso")).unwrap_or_default()
            ));
        }
        let errs = gpu_errors(&kernel_log_since(since), &g.pci, g.is_nvidia());
        if let Some(e) = errs.first() {
            break Err(format!("kernel: {e}"));
        }
        if el >= secs {
            break Ok(StepStats {
                clock_mhz: avg(&clocks),
                power_w: avg(&watts),
                util_pct: avg(&utils),
            });
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
    let cmd = pick_stress(g, topo, out)?;
    out(&format!(
        "{}: undervolt automático, {} s por passo",
        g.alias, opts.secs
    ));
    if let Method::NvOffset { clock_max, .. } = m {
        out(&format!(
            "  Na NVIDIA o Linux não deixa escolher a voltagem: o clock fica travado em até {clock_max} MHz \
             e a curva sobe a cada passo, então o mesmo clock roda com voltagem menor. Compare os passos: \
             menos watts no mesmo clock ou, se a placa está no limite de potência, mais clock nos mesmos watts. \
             A temperatura sobe com o tempo de carga (a placa vai esquentando); compare depois de alguns passos."
        ));
    }
    let mut first: Option<StepStats> = None;
    let mut last_ok: Option<StepStats> = None;
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
        match stress(g, topo, &cmd, opts.secs, out) {
            Ok(st) => {
                out(&format!("    passou ({})", st.line()));
                if first.is_none() {
                    first = Some(st.clone());
                }
                last_ok = Some(st);
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
                    return Err(if e.starts_with("kernel:") {
                        "nem o ponto de partida aguentou (a placa deu erro no kernel): nada foi mudado".into()
                    } else {
                        format!(
                            "o programa de teste ({}) caiu já no ponto de partida, sem erro da placa no kernel: \
                             o problema parece ser o programa, não a placa. Nada foi mudado.",
                            cmd[0]
                        )
                    });
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
    if let Err(e) = stress(g, topo, &cmd, opts.secs * 2, out) {
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
    if let (Some(a), Some(b)) = (&first, &last_ok) {
        out(&format!("  sem ajuste:      {}", a.line()));
        out(&format!("  último que passou: {}", b.line()));
    }
    crate::notify::event(
        "jobs",
        &format!("uv-{}", g.pci),
        &format!("Undervolt de {} pronto", g.alias),
        &format!(
            "{} ({} passo(s) estáveis)",
            describe(&final_c, &m),
            ok_steps
        ),
        3,
        Duration::from_secs(1),
    );
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
