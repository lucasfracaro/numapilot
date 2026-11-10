//! Conferir a escolha de placa: roda o `vulkaninfo`/`glxinfo` com o mesmo ambiente que um jogo
//! receberia e vê qual placa respondeu. Também descobre o nome Vulkan de cada placa, usado no
//! filtro do DXVK/VKD3D (DirectX 8–12) como reserva para quando o seletor do Mesa não está ativo.

use crate::containers::which;
use crate::topo::{Gpu, Topology};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, PartialEq)]
pub struct VkDev {
    pub vendor: u16,
    pub device: u16,
    pub name: String,
}

/// Lê os dispositivos do `vulkaninfo --summary`, na ordem em que o Vulkan os entrega.
pub fn parse_vk_summary(t: &str) -> Vec<VkDev> {
    let mut v = vec![];
    let (mut ven, mut dev) = (None, None);
    let hex = |s: &str| u16::from_str_radix(s.trim().trim_start_matches("0x"), 16).ok();
    for l in t.lines() {
        let Some((k, val)) = l.split_once('=') else {
            continue;
        };
        match k.trim() {
            "vendorID" => ven = hex(val),
            "deviceID" => dev = hex(val),
            "deviceName" => {
                if let (Some(vendor), Some(device)) = (ven.take(), dev.take()) {
                    v.push(VkDev {
                        vendor,
                        device,
                        name: val.trim().to_string(),
                    });
                }
            }
            _ => {}
        }
    }
    v
}

/// Placa que o OpenGL usou, pelo `glxinfo -B`: (id do dispositivo se o Mesa informar, nome).
pub fn parse_glxinfo(t: &str) -> Option<(Option<u16>, String)> {
    let line = |k: &str| {
        t.lines()
            .find_map(|l| l.trim().strip_prefix(k).map(|v| v.trim().to_string()))
    };
    if let Some(d) = line("Device:") {
        // "AMD Radeon RX 470 Graphics (radeonsi, polaris10, ...) (0x67df)"
        let id = d
            .rsplit_once("(0x")
            .and_then(|(_, r)| u16::from_str_radix(r.trim_end_matches(')'), 16).ok());
        return Some((id, d));
    }
    line("OpenGL renderer string:").map(|r| (None, r))
}

/// Parte estável do nome Vulkan ("AMD Radeon RX 470 Graphics (RADV POLARIS10)" → sem o parêntese):
/// o DXVK compara por trecho do nome, e o que vem entre parênteses muda com a versão do driver.
pub fn filter_name(vk_name: &str) -> String {
    vk_name
        .split(" (")
        .next()
        .unwrap_or(vk_name)
        .trim()
        .to_string()
}

fn run_env(argv: &[&str], set: &[(String, String)], unset: &[String]) -> Option<String> {
    let mut c = Command::new("timeout");
    c.arg("20").args(argv);
    for k in unset {
        c.env_remove(k);
    }
    for (k, v) in set {
        c.env(k, v);
    }
    let o = c.output().ok()?;
    Some(String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr))
}

#[derive(Serialize, Deserialize, Default)]
struct Cache {
    /// mtime da pasta dos drivers Vulkan quando os nomes foram lidos
    icd_stamp: u64,
    at: u64,
    names: HashMap<String, String>,
}

fn cache_path() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into())).join(".cache")
        })
        .join("numapilot/vk-names.json")
}

fn icd_stamp() -> u64 {
    ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"]
        .iter()
        .filter_map(|d| std::fs::metadata(d).ok()?.modified().ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .max()
        .unwrap_or(0)
}

/// Nome Vulkan desta placa com o ambiente dado, para o filtro do DXVK/VKD3D. Guarda por um dia
/// (ou até os drivers Vulkan instalados mudarem). None se não der para saber com certeza.
pub fn vk_name_for(g: &Gpu, set: &[(String, String)], unset: &[String]) -> Option<String> {
    if !which("vulkaninfo") {
        return None;
    }
    let key = format!("{:04x}:{:04x}", g.vendor, g.device);
    let now = crate::history::now();
    let stamp = icd_stamp();
    let mut cache: Cache = std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if cache.icd_stamp != stamp || now.saturating_sub(cache.at) > 86400 {
        cache = Cache {
            icd_stamp: stamp,
            at: now,
            names: HashMap::new(),
        };
    }
    if let Some(n) = cache.names.get(&key) {
        return (!n.is_empty()).then(|| n.clone());
    }
    let out = run_env(&["vulkaninfo", "--summary"], set, unset).unwrap_or_default();
    let name = parse_vk_summary(&out)
        .into_iter()
        .find(|d| d.vendor == g.vendor && d.device == g.device)
        .map(|d| filter_name(&d.name))
        .unwrap_or_default();
    cache.names.insert(key, name.clone());
    let p = cache_path();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(t) = serde_json::to_string(&cache) {
        let _ = std::fs::write(p, t);
    }
    (!name.is_empty()).then_some(name)
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum Verdict {
    Certa,
    Errada,
    NaoDeu,
}

#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub api: String,
    pub verdict: Verdict,
    pub detail: String,
}

/// Julga a saída do vulkaninfo para a placa pedida.
pub fn judge_vulkan(g: &Gpu, same_model: usize, out: &str) -> Line {
    // o renderizador por software (llvmpipe, vendorID 0x10005) não cabe em 16 bits e fica de fora
    let devs: Vec<VkDev> = parse_vk_summary(out);
    let (verdict, detail) = match devs.first() {
        None => (
            Verdict::NaoDeu,
            "o Vulkan não listou nenhuma placa com esta escolha".to_string(),
        ),
        Some(d) if d.vendor == g.vendor && d.device == g.device => (
            Verdict::Certa,
            if same_model > 1 {
                format!(
                    "{} (há {same_model} placas deste modelo: o modelo confere, mas por aqui não dá \
                     para saber qual delas)",
                    d.name
                )
            } else if devs.len() > 1 {
                format!(
                    "{} em primeiro; o programa ainda enxerga {} placa(s) a mais",
                    d.name,
                    devs.len() - 1
                )
            } else {
                d.name.clone()
            },
        ),
        Some(d) => (
            Verdict::Errada,
            format!("respondeu {} ({:04x}:{:04x})", d.name, d.vendor, d.device),
        ),
    };
    Line {
        api: "Vulkan (e DirectX pelo DXVK/VKD3D)".into(),
        verdict,
        detail,
    }
}

/// Julga a saída do glxinfo -B.
pub fn judge_gl(g: &Gpu, out: &str) -> Line {
    let (verdict, detail) = match parse_glxinfo(out) {
        None => (
            Verdict::NaoDeu,
            "o glxinfo não respondeu (precisa de uma sessão gráfica)".to_string(),
        ),
        Some((Some(id), name)) => (
            if id == g.device {
                Verdict::Certa
            } else {
                Verdict::Errada
            },
            name,
        ),
        Some((None, name)) => {
            let nv = name.to_uppercase().contains("NVIDIA");
            (
                if nv == g.is_nvidia() {
                    Verdict::Certa
                } else {
                    Verdict::Errada
                },
                name,
            )
        }
    };
    Line {
        api: "OpenGL (e DirectX antigo pelo WineD3D)".into(),
        verdict,
        detail,
    }
}

/// Testa a escolha desta placa com o ambiente que o `numapilot run --gpu` monta.
pub fn test(g: &Gpu, topo: &Topology, set: &[(String, String)], unset: &[String]) -> Vec<Line> {
    let mut v = vec![];
    let same = topo
        .gpus
        .iter()
        .filter(|o| o.vendor == g.vendor && o.device == g.device)
        .count();
    if which("vulkaninfo") {
        let out = run_env(&["vulkaninfo", "--summary"], set, unset).unwrap_or_default();
        v.push(judge_vulkan(g, same, &out));
    } else {
        v.push(Line {
            api: "Vulkan (e DirectX pelo DXVK/VKD3D)".into(),
            verdict: Verdict::NaoDeu,
            detail: "instale o vulkan-tools (sudo pacman -S vulkan-tools)".into(),
        });
    }
    if which("glxinfo") {
        let out = run_env(&["glxinfo", "-B"], set, unset).unwrap_or_default();
        v.push(judge_gl(g, &out));
    } else {
        v.push(Line {
            api: "OpenGL (e DirectX antigo pelo WineD3D)".into(),
            verdict: Verdict::NaoDeu,
            detail: "instale o mesa-utils (sudo pacman -S mesa-utils)".into(),
        });
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUMMARY: &str = "Devices:\n========\nGPU0:\n\tapiVersion = 1.4.3\n\tvendorID = 0x1002\n\tdeviceID = 0x67df\n\tdeviceType = PHYSICAL_DEVICE_TYPE_DISCRETE_GPU\n\tdeviceName = AMD Radeon RX 470 Graphics (RADV POLARIS10)\nGPU1:\n\tvendorID           = 0x1002\n\tdeviceID           = 0x66a1\n\tdeviceName         = AMD Radeon Instinct MI50 (RADV VEGA20)\n";

    fn gpu(vendor: u16, device: u16) -> Gpu {
        Gpu {
            alias: "g".into(),
            pci: "0000:01:00.0".into(),
            vendor,
            device,
            name: String::new(),
            numa_node: None,
            card: None,
            render: None,
            display_connected: false,
            gfx: None,
            rocr_id: None,
            role: crate::topo::GpuRole::Other,
        }
    }

    #[test]
    fn parses_summary() {
        let d = parse_vk_summary(SUMMARY);
        assert_eq!(d.len(), 2);
        assert_eq!((d[0].vendor, d[0].device), (0x1002, 0x67df));
        assert_eq!(filter_name(&d[0].name), "AMD Radeon RX 470 Graphics");
        assert_eq!(
            filter_name("NVIDIA GeForce RTX 3070 Ti Laptop GPU"),
            "NVIDIA GeForce RTX 3070 Ti Laptop GPU"
        );
    }

    #[test]
    fn judges() {
        assert_eq!(
            judge_vulkan(&gpu(0x1002, 0x67df), 1, SUMMARY).verdict,
            Verdict::Certa
        );
        assert_eq!(
            judge_vulkan(&gpu(0x1002, 0x66a1), 2, SUMMARY).verdict,
            Verdict::Errada
        );
        assert_eq!(
            judge_vulkan(&gpu(0x1002, 0x66a1), 2, "").verdict,
            Verdict::NaoDeu
        );
        let gl = "    Vendor: AMD (0x1002)\n    Device: AMD Radeon RX 470 Graphics (radeonsi, polaris10, ACO, DRM 3.64) (0x67df)\n";
        assert_eq!(judge_gl(&gpu(0x1002, 0x67df), gl).verdict, Verdict::Certa);
        assert_eq!(judge_gl(&gpu(0x1002, 0x66a1), gl).verdict, Verdict::Errada);
        let nv = "OpenGL renderer string: NVIDIA GeForce RTX 3070 Ti Laptop GPU/PCIe/SSE2\n";
        assert_eq!(judge_gl(&gpu(0x10de, 0x24a0), nv).verdict, Verdict::Certa);
        assert_eq!(judge_gl(&gpu(0x1002, 0x1681), nv).verdict, Verdict::Errada);
    }
}
