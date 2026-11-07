//! Máquina virtual Windows com uma placa de vídeo só dela (VFIO), para os jogos com anti-cheat
//! que não rodam no Linux. O numapilot confere os requisitos, prende a placa escolhida no
//! vfio-pci no boot (sobrevive às atualizações do kernel) e cria a VM no libvirt com os núcleos
//! e a memória do socket da placa.
//!
//! Atenção: alguns anti-cheats (ex.: Vanguard, e jogos que configuram o EasyAntiCheat/BattlEye
//! para isso) recusam máquina virtual; nesses, nem a VM resolve.

use crate::topo::{Gpu, Topology};
use crate::util::{read_trim, sys};
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn vfio_conf() -> PathBuf {
    std::env::var_os("NUMAPILOT_VFIO_CONF")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/modprobe.d/numapilot-vfio.conf"))
}

const MKINITCPIO_CONF: &str = "/etc/mkinitcpio.conf.d/numapilot-vfio.conf";
const DRACUT_CONF: &str = "/etc/dracut.conf.d/numapilot-vfio.conf";

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub fix: Option<String>,
}

/// Funções PCI no mesmo grupo do IOMMU (precisam ir juntas para a VM).
pub fn iommu_group(pci: &str) -> Option<Vec<String>> {
    let g = std::fs::read_link(sys(&format!("bus/pci/devices/{pci}/iommu_group"))).ok()?;
    let name = g.file_name()?.to_string_lossy().to_string();
    let mut v: Vec<String> = std::fs::read_dir(sys(&format!("kernel/iommu_groups/{name}/devices")))
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    Some(v)
}

fn ids(pci: &str) -> Option<String> {
    let d = sys(&format!("bus/pci/devices/{pci}"));
    let v = read_trim(d.join("vendor"))?;
    let dv = read_trim(d.join("device"))?;
    Some(format!(
        "{}:{}",
        v.trim_start_matches("0x"),
        dv.trim_start_matches("0x")
    ))
}

fn class(pci: &str) -> String {
    read_trim(sys(&format!("bus/pci/devices/{pci}/class"))).unwrap_or_default()
}

/// A placa e as funções que vão junto (o áudio HDMI da mesma placa).
pub fn functions(pci: &str) -> Vec<String> {
    let slot = pci.rsplit_once('.').map(|x| x.0).unwrap_or(pci);
    let mut v: Vec<String> = std::fs::read_dir(sys("bus/pci/devices"))
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with(&format!("{slot}.")))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if v.is_empty() {
        v.push(pci.to_string());
    }
    v
}

/// Placas AMD com o defeito de reset (não voltam depois de desligar a VM sem o vendor-reset).
pub fn needs_vendor_reset(g: &Gpu) -> bool {
    g.vendor == 0x1002
        && matches!(
            g.device,
            // Polaris 10/11/12 (RX 470/480/570/580/550/560), Vega 10/20, Navi 10/12/14
            0x67df
                | 0x67ef
                | 0x67ff
                | 0x699f
                | 0x6987
                | 0x687f
                | 0x6863
                | 0x66a0
                | 0x66a1
                | 0x66af
                | 0x731f
                | 0x7340
                | 0x7360
                | 0x7362
        )
}

fn has(bin: &str) -> bool {
    crate::containers::which(bin)
}

pub fn check(topo: &Topology, gpu: Option<&Gpu>) -> Vec<Check> {
    let mut v = vec![];
    let mut add = |name: &str, ok: bool, detail: String, fix: Option<&str>| {
        v.push(Check {
            name: name.into(),
            ok,
            detail,
            fix: fix.map(String::from),
        });
    };
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let virt = cpuinfo.contains(" vmx") || cpuinfo.contains(" svm");
    add(
        "virtualização da CPU",
        virt,
        if virt {
            "VT-x/AMD-V ligado".into()
        } else {
            "desligada".into()
        },
        Some("ligue VT-x / SVM na BIOS"),
    );
    let groups = std::fs::read_dir(sys("kernel/iommu_groups"))
        .map(|r| r.count())
        .unwrap_or(0);
    let intel = cpuinfo.contains("GenuineIntel");
    add(
        "IOMMU",
        groups > 0,
        if groups > 0 {
            format!("{groups} grupos")
        } else {
            "desligado".into()
        },
        Some(if intel {
            "ligue VT-d na BIOS; numapilot vm prepare adiciona intel_iommu=on iommu=pt"
        } else {
            "ligue IOMMU/AMD-Vi na BIOS"
        }),
    );
    let qemu = has("qemu-system-x86_64");
    add(
        "QEMU",
        qemu,
        String::new(),
        Some("sudo pacman -S qemu-desktop"),
    );
    let lv = has("virsh");
    add(
        "libvirt",
        lv,
        String::new(),
        Some("sudo pacman -S libvirt dnsmasq && sudo systemctl enable --now libvirtd"),
    );
    add(
        "virt-install",
        has("virt-install"),
        String::new(),
        Some("sudo pacman -S virt-install"),
    );
    add(
        "virt-manager (janela da VM)",
        has("virt-manager"),
        String::new(),
        Some("sudo pacman -S virt-manager"),
    );
    let ovmf = [
        "/usr/share/edk2/x64/OVMF_CODE.4m.fd",
        "/usr/share/edk2-ovmf/x64/OVMF_CODE.fd",
        "/usr/share/OVMF/OVMF_CODE.fd",
    ]
    .iter()
    .any(|p| Path::new(p).exists());
    add(
        "UEFI da VM (OVMF)",
        ovmf,
        String::new(),
        Some("sudo pacman -S edk2-ovmf"),
    );
    add(
        "TPM da VM (Windows 11)",
        has("swtpm"),
        String::new(),
        Some("sudo pacman -S swtpm"),
    );
    let in_group = Command::new("id").arg("-nG").output().is_ok_and(|o| {
        String::from_utf8_lossy(&o.stdout)
            .split_whitespace()
            .any(|g| g == "libvirt")
    });
    add(
        "você no grupo libvirt",
        in_group,
        String::new(),
        Some("sudo usermod -aG libvirt $USER (e entre de novo)"),
    );
    if let Some(g) = gpu {
        let fns = functions(&g.pci);
        match iommu_group(&g.pci) {
            Some(grp) => {
                let extra: Vec<&String> = grp
                    .iter()
                    .filter(|d| !fns.contains(d) && !class(d).starts_with("0x0604"))
                    .collect();
                add(
                    &format!("{} isolada no IOMMU", g.alias),
                    extra.is_empty(),
                    if extra.is_empty() {
                        format!("grupo só com {}", fns.join(", "))
                    } else {
                        format!(
                            "divide o grupo com {}",
                            extra
                                .iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    },
                    Some(
                        "troque a placa de slot (os ligados direto na CPU costumam ficar isolados)",
                    ),
                );
            }
            None => add(
                &format!("{} no IOMMU", g.alias),
                false,
                "sem grupo (IOMMU desligado?)".into(),
                None,
            ),
        }
        let drv = std::fs::read_link(sys(&format!("bus/pci/devices/{}/driver", g.pci)))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
            .unwrap_or_default();
        add(
            &format!("{} presa no vfio-pci", g.alias),
            drv == "vfio-pci",
            format!(
                "driver agora: {}",
                if drv.is_empty() { "nenhum" } else { &drv }
            ),
            Some(&format!("sudo numapilot vm prepare {} e reinicie", g.alias)),
        );
        if g.display_connected {
            add(
                &format!("{} sem monitor do Linux", g.alias),
                false,
                "o monitor do Linux está nela; ligue-o em outra placa antes".into(),
                None,
            );
        }
        let same = topo
            .gpus
            .iter()
            .filter(|o| o.vendor == g.vendor && o.device == g.device)
            .count();
        add(
            "modelo único",
            same == 1,
            if same == 1 {
                String::new()
            } else {
                format!("{same} placas com o mesmo modelo: todas iriam para o vfio")
            },
            None,
        );
        if needs_vendor_reset(g) {
            let vr = Path::new(&sys("module/vendor_reset")).exists();
            add(
                "vendor-reset",
                vr,
                "este modelo não reinicia sozinho depois de desligar a VM".into(),
                Some(
                    "paru -S vendor-reset-dkms-git && echo vendor-reset | sudo tee /etc/modules-load.d/vendor-reset.conf",
                ),
            );
        }
    }
    v
}

/// Prende a placa no vfio-pci no boot. Precisa de root e reinício.
pub fn prepare(topo: &Topology, g: &Gpu, log: &mut dyn FnMut(&str)) -> Result<(), String> {
    if !crate::util::is_root() {
        return Err("precisa de root (sudo)".into());
    }
    if topo
        .gpus
        .iter()
        .filter(|o| o.vendor == g.vendor && o.device == g.device)
        .count()
        > 1
    {
        return Err("há outra placa com o mesmo modelo: as duas iriam para o vfio".into());
    }
    if topo.gpus.len() < 2 {
        return Err("é a única placa de vídeo: o Linux ficaria sem imagem".into());
    }
    let fns = functions(&g.pci);
    let idlist: Vec<String> = fns.iter().filter_map(|f| ids(f)).collect();
    let text = format!(
        "# Gerado pelo numapilot (vm prepare {}): estas funções ficam com o vfio-pci para a VM.\n\
         options vfio-pci ids={}\n\
         softdep amdgpu pre: vfio-pci\nsoftdep radeon pre: vfio-pci\nsoftdep nouveau pre: vfio-pci\n\
         softdep nvidia pre: vfio-pci\nsoftdep snd_hda_intel pre: vfio-pci\nsoftdep xhci_hcd pre: vfio-pci\n",
        g.alias,
        idlist.join(",")
    );
    std::fs::write(vfio_conf(), text).map_err(|e| format!("{}: {e}", vfio_conf().display()))?;
    log(&format!(
        "Gravado {} (ids {})",
        vfio_conf().display(),
        idlist.join(",")
    ));
    if Path::new("/etc/mkinitcpio.conf.d").exists() || has("mkinitcpio") {
        let _ = std::fs::create_dir_all("/etc/mkinitcpio.conf.d");
        std::fs::write(
            MKINITCPIO_CONF,
            "MODULES+=(vfio_pci vfio vfio_iommu_type1)\n",
        )
        .map_err(|e| e.to_string())?;
        log(&format!("Gravado {MKINITCPIO_CONF}"));
    }
    if has("dracut") {
        let _ = std::fs::create_dir_all("/etc/dracut.conf.d");
        std::fs::write(
            DRACUT_CONF,
            "force_drivers+=\" vfio_pci vfio vfio_iommu_type1 \"\n",
        )
        .map_err(|e| e.to_string())?;
        log(&format!("Gravado {DRACUT_CONF}"));
    }
    let intel = std::fs::read_to_string("/proc/cpuinfo")
        .unwrap_or_default()
        .contains("GenuineIntel");
    let mut params = vec!["iommu=pt".to_string()];
    if intel {
        params.push("intel_iommu=on".into());
    }
    let running = crate::cmdline::running();
    let missing: Vec<String> = params
        .into_iter()
        .filter(|p| !running.split_whitespace().any(|t| t == p))
        .collect();
    if !missing.is_empty() {
        crate::cmdline::apply(&missing, &[], log)?;
    }
    crate::bootcfg::regenerate(log)?;
    log("Pronto. Reinicie: a placa vai aparecer com o driver vfio-pci (numapilot vm check).");
    Ok(())
}

/// Devolve a placa ao Linux.
pub fn unprepare(log: &mut dyn FnMut(&str)) -> Result<(), String> {
    if !crate::util::is_root() {
        return Err("precisa de root (sudo)".into());
    }
    for f in [
        vfio_conf(),
        PathBuf::from(MKINITCPIO_CONF),
        PathBuf::from(DRACUT_CONF),
    ] {
        if f.exists() {
            std::fs::remove_file(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            log(&format!("Removido {}", f.display()));
        }
    }
    crate::bootcfg::regenerate(log)?;
    log("Pronto. Reinicie para a placa voltar ao Linux.");
    Ok(())
}

#[derive(Debug, Clone)]
pub struct NewVm {
    pub name: String,
    pub iso: PathBuf,
    pub mem_gb: u32,
    pub cpus: usize,
    pub disk_gb: u32,
}

/// Linha do virt-install: UEFI, TPM, CPUs e memória no socket da placa, placa e áudio dela.
pub fn virt_install_args(topo: &Topology, g: &Gpu, vm: &NewVm) -> Vec<String> {
    let node = g.numa_node.unwrap_or(0);
    let mut cpus = topo.cpus_of(&[node]);
    // deixa os primeiros núcleos do socket para o Linux
    let want = vm.cpus.min(cpus.len().saturating_sub(2)).max(2);
    let skip = cpus.len().saturating_sub(want);
    cpus = cpus.split_off(skip);
    let cpuset = crate::util::format_cpulist(&cpus);
    let mut a: Vec<String> = vec![
        "--connect".into(),
        "qemu:///system".into(),
        "--name".into(),
        vm.name.clone(),
        "--osinfo".into(),
        "win11".into(),
        "--machine".into(),
        "q35".into(),
        "--boot".into(),
        "uefi".into(),
        "--tpm".into(),
        "backend.type=emulator,backend.version=2.0,model=tpm-crb".into(),
        "--memory".into(),
        (vm.mem_gb * 1024).to_string(),
        "--vcpus".into(),
        format!("{want},cpuset={cpuset}"),
        "--cpu".into(),
        "host-passthrough,cache.mode=passthrough".into(),
        "--features".into(),
        "kvm_hidden=on,hyperv.relaxed.state=on,hyperv.vapic.state=on,hyperv.spinlocks.state=on,hyperv.spinlocks.retries=8191".into(),
        "--disk".into(),
        format!("size={},bus=sata,cache=none,discard=unmap", vm.disk_gb),
        "--cdrom".into(),
        vm.iso.display().to_string(),
        "--network".into(),
        "network=default,model=e1000e".into(),
        "--graphics".into(),
        "spice".into(),
        "--video".into(),
        "qxl".into(),
        "--noautoconsole".into(),
    ];
    if topo.nodes.len() > 1 {
        a.push("--numatune".into());
        a.push(format!("{node},mode=strict"));
    }
    for f in functions(&g.pci) {
        a.push("--hostdev".into());
        a.push(f);
    }
    a
}

pub fn create(topo: &Topology, g: &Gpu, vm: &NewVm) -> Result<(), String> {
    if !vm.iso.is_file() {
        return Err(format!("ISO não encontrada: {}", vm.iso.display()));
    }
    if !vm
        .name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("nome da VM: letras, números, - e _".into());
    }
    let a = virt_install_args(topo, g, vm);
    let st = Command::new("virt-install")
        .args(&a)
        .status()
        .map_err(|e| format!("virt-install: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("virt-install terminou com erro ({st})"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn functions_and_reset() {
        let root = std::env::temp_dir().join(format!("np-vm-{}", std::process::id()));
        for f in ["0000:03:00.0", "0000:03:00.1", "0000:04:00.0"] {
            std::fs::create_dir_all(root.join("bus/pci/devices").join(f)).unwrap();
        }
        std::fs::write(root.join("bus/pci/devices/0000:03:00.0/vendor"), "0x1002").unwrap();
        std::fs::write(root.join("bus/pci/devices/0000:03:00.0/device"), "0x67df").unwrap();
        let _g = crate::util::test_sysfs_lock(&root);
        assert_eq!(
            functions("0000:03:00.0"),
            vec!["0000:03:00.0", "0000:03:00.1"]
        );
        assert_eq!(ids("0000:03:00.0").as_deref(), Some("1002:67df"));
        let _ = std::fs::remove_dir_all(root);
    }
}
