//! Interrupts on the right socket (PCs with 2 sockets): each card (GPU, NVMe, network) signals
//! the processor through interrupts; handled on the socket the card is connected to, the data
//! does not cross from one socket to the other. NVMe with kernel-managed queues already does
//! this by itself (the write is refused and that is fine).

use crate::util::{read_trim, sys};

#[derive(Debug, Clone, PartialEq)]
pub struct Dev {
    pub pci: String,
    /// "nvme" | "rede" | "gpu"
    pub kind: &'static str,
    pub node: usize,
    pub irqs: Vec<u32>,
}

/// NVMe disk and network cards with a known NUMA node.
pub fn devices() -> Vec<Dev> {
    let Ok(rd) = std::fs::read_dir(sys("bus/pci/devices")) else {
        return vec![];
    };
    let mut v = vec![];
    for e in rd.flatten() {
        let d = e.path();
        let class = read_trim(d.join("class")).unwrap_or_default();
        let kind = if class.starts_with("0x0108") {
            "nvme"
        } else if class.starts_with("0x02") {
            "rede"
        } else {
            continue;
        };
        let Some(node) = read_trim(d.join("numa_node"))
            .and_then(|n| n.parse::<i64>().ok())
            .filter(|n| *n >= 0)
        else {
            continue;
        };
        let mut irqs: Vec<u32> = std::fs::read_dir(d.join("msi_irqs"))
            .map(|rd| {
                rd.flatten()
                    .filter_map(|e| e.file_name().to_string_lossy().parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        irqs.sort_unstable();
        v.push(Dev {
            pci: e.file_name().to_string_lossy().to_string(),
            kind,
            node: node as usize,
            irqs,
        });
    }
    v.sort_by(|a, b| a.pci.cmp(&b.pci));
    v
}

fn irq_root() -> String {
    std::env::var("NUMAPILOT_IRQ_ROOT").unwrap_or_else(|_| "/proc/irq".into())
}

/// Points each card's interrupts to the CPUs of its node. Returns how many changed.
pub fn apply(topo: &crate::topo::Topology) -> usize {
    if topo.nodes.len() < 2 {
        return 0;
    }
    let mut n = 0;
    for d in devices() {
        let cpus = crate::util::format_cpulist(&topo.cpus_of(&[d.node]));
        if cpus.is_empty() {
            continue;
        }
        for irq in &d.irqs {
            let p = format!("{}/{irq}/smp_affinity_list", irq_root());
            if read_trim(&p).as_deref() != Some(cpus.as_str()) && std::fs::write(&p, &cpus).is_ok()
            {
                n += 1;
            }
        }
    }
    n
}

/// irqbalance redistributes the interrupts and undoes the tuning from time to time.
pub fn irqbalance_active() -> bool {
    std::process::Command::new("systemctl")
        .args(["is-active", "--quiet", "irqbalance"])
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    #[test]
    fn fake() {
        let root = std::env::temp_dir().join(format!("np-irq-{}", std::process::id()));
        let w = |p: &str, v: &str| {
            let f = root.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, v).unwrap();
        };
        w("bus/pci/devices/0000:81:00.0/class", "0x010802");
        w("bus/pci/devices/0000:81:00.0/numa_node", "1");
        w("bus/pci/devices/0000:81:00.0/msi_irqs/120", "msix");
        w("bus/pci/devices/0000:05:00.0/class", "0x020000");
        w("bus/pci/devices/0000:05:00.0/numa_node", "0");
        w("bus/pci/devices/0000:06:00.0/class", "0x030000");
        let _g = crate::util::test_sysfs_lock(&root);
        let d = super::devices();
        assert_eq!(d.len(), 2);
        assert_eq!(
            (d[1].kind, d[1].node, d[1].irqs.clone()),
            ("nvme", 1, vec![120])
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
