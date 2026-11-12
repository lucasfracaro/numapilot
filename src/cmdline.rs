//! Parâmetros da linha do kernel, editados no arquivo do bootloader (que as atualizações do kernel
//! reaproveitam): Limine, systemd-boot (sdboot-manage), GRUB ou rEFInd. Sempre com backup.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Limine,
    SdbootManage,
    Grub,
    Refind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loader {
    pub kind: Kind,
    pub path: PathBuf,
}

impl Loader {
    pub fn name(&self) -> &'static str {
        match self.kind {
            Kind::Limine => "Limine",
            Kind::SdbootManage => "systemd-boot (sdboot-manage)",
            Kind::Grub => "GRUB",
            Kind::Refind => "rEFInd",
        }
    }
    /// Comando que regrava as entradas do boot depois da edição.
    pub fn regen(&self) -> Option<Vec<&'static str>> {
        match self.kind {
            Kind::Limine => Some(vec!["limine-mkinitcpio"]),
            Kind::SdbootManage => Some(vec!["sdboot-manage", "gen"]),
            Kind::Grub => Some(vec!["grub-mkconfig", "-o", "/boot/grub/grub.cfg"]),
            Kind::Refind => None,
        }
    }
}

fn root() -> PathBuf {
    std::env::var_os("NUMAPILOT_BOOT_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Descobre o bootloader pelo arquivo de configuração dos parâmetros.
pub fn detect() -> Option<Loader> {
    let r = root();
    [
        (Kind::Limine, "etc/default/limine"),
        (Kind::SdbootManage, "etc/sdboot-manage.conf"),
        (Kind::Grub, "etc/default/grub"),
        (Kind::Refind, "boot/refind_linux.conf"),
    ]
    .into_iter()
    .map(|(k, p)| Loader {
        kind: k,
        path: r.join(p),
    })
    .find(|l| l.path.is_file())
}

/// Parâmetros conhecidos: (parâmetro, título, explicação/custo).
pub const KNOWN: &[(&str, &str, &str)] = &[
    (
        "pci=realloc",
        "Realocar as janelas PCI",
        "Pode liberar o Resizable BAR e a VRAM inteira visível em placas X99/C612. Em algumas placas impede o boot: se acontecer, edite a entrada no menu do boot e tire o parâmetro.",
    ),
    (
        "intel_iommu=on",
        "IOMMU da Intel",
        "Necessário para passar uma GPU para uma máquina virtual (VFIO).",
    ),
    (
        "iommu=pt",
        "IOMMU em passthrough",
        "Com o IOMMU ligado, evita o custo de tradução no DMA das placas que ficam no Linux.",
    ),
    (
        "amd_iommu=off",
        "Desligar o IOMMU da AMD",
        "Recomendado na BC-250 (o IOMMU dela causa travamentos). Impede passar placas para máquinas virtuais.",
    ),
    (
        "nowatchdog",
        "Sem watchdog",
        "Menos interrupções periódicas (o CachyOS já costuma usar).",
    ),
    (
        "pcie_aspm=off",
        "Sem economia no PCIe (ASPM)",
        "Evita quedas e latência ao acordar o link das placas; consome um pouco mais.",
    ),
    (
        "intel_idle.max_cstate=1",
        "Limitar os estados de economia sempre",
        "Menor latência o tempo todo, mas mais calor e consumo parado. O numapilot já faz isso só enquanto um jogo roda.",
    ),
    (
        "amdgpu.dcdebugmask=0x10",
        "Sem Panel Self Refresh (AMD)",
        "Corrige telas externas (USB-C/MST) piscando quando o mouse está na tela do notebook. Gasta um pouco mais de bateria.",
    ),
];

/// Um grupo de mitigações do processador que se desliga junto.
pub struct Mitigation {
    pub title: &'static str,
    /// Arquivos em /sys/devices/system/cpu/vulnerabilities/
    pub vulns: &'static [&'static str],
    pub params: &'static [&'static str],
    /// Risco de deixar desligado num desktop (navegador, jogos, Steam).
    pub risk: &'static str,
    /// Ganho típico de desempenho ao desligar.
    pub gain: &'static str,
    pub note: &'static str,
    /// Das mais importantes (Spectre/Meltdown): fica ligada no "desligar as leves".
    pub heavy: bool,
}

pub const MITIGATIONS: &[Mitigation] = &[
    Mitigation {
        title: "Meltdown (PTI)",
        vulns: &["meltdown"],
        params: &["pti=off"],
        risk: "alto",
        gain: "alto em disco, rede e chamadas ao sistema; médio em jogos",
        note: "Um programa pode ler a memória do kernel.",
        heavy: true,
    },
    Mitigation {
        title: "Spectre v1",
        vulns: &["spectre_v1"],
        params: &["nospectre_v1"],
        risk: "alto",
        gain: "baixo",
        note: "Barato de manter; quase não vale desligar.",
        heavy: true,
    },
    Mitigation {
        title: "Spectre v2 / BHI",
        vulns: &["spectre_v2"],
        params: &["spectre_v2=off", "spectre_v2_user=off", "spectre_bhi=off"],
        risk: "alto",
        gain: "médio (retpoline/IBRS em cada chamada indireta)",
        note: "Ataques entre programas e contra o kernel, inclusive por JavaScript.",
        heavy: true,
    },
    Mitigation {
        title: "Spectre v4 (SSB)",
        vulns: &["spec_store_bypass"],
        params: &["spec_store_bypass_disable=off"],
        risk: "médio",
        gain: "baixo (por padrão só vale para programas em sandbox)",
        note: "",
        heavy: true,
    },
    Mitigation {
        title: "Limpeza de buffers: MDS, TAA, MMIO, RFDS",
        vulns: &[
            "mds",
            "tsx_async_abort",
            "mmio_stale_data",
            "reg_file_data_sampling",
        ],
        params: &[
            "mds=off",
            "tsx_async_abort=off",
            "mmio_stale_data=off",
            "reg_file_data_sampling=off",
        ],
        risk: "médio",
        gain: "médio a alto (limpeza a cada troca de contexto)",
        note: "As quatro usam a mesma limpeza (VERW): só há ganho desligando todas juntas.",
        heavy: false,
    },
    Mitigation {
        title: "L1TF (Foreshadow)",
        vulns: &["l1tf"],
        params: &["l1tf=off"],
        risk: "baixo fora de máquinas virtuais",
        gain: "baixo fora de VMs",
        note: "",
        heavy: false,
    },
    Mitigation {
        title: "Retbleed",
        vulns: &["retbleed"],
        params: &["retbleed=off"],
        risk: "médio",
        gain: "alto quando a mitigação usa IBRS",
        note: "",
        heavy: false,
    },
    Mitigation {
        title: "Downfall (GDS)",
        vulns: &["gather_data_sampling"],
        params: &["gather_data_sampling=off"],
        risk: "médio",
        gain: "alto em código com AVX (compressão, IA na CPU)",
        note: "",
        heavy: false,
    },
    Mitigation {
        title: "SRBDS",
        vulns: &["srbds"],
        params: &["srbds=off"],
        risk: "baixo",
        gain: "baixo (RDRAND mais rápido)",
        note: "",
        heavy: false,
    },
    Mitigation {
        title: "ITS (Indirect Target Selection)",
        vulns: &["indirect_target_selection"],
        params: &["indirect_target_selection=off"],
        risk: "médio",
        gain: "baixo a médio",
        note: "",
        heavy: false,
    },
    Mitigation {
        title: "Só para máquinas virtuais (iTLB multihit, VMScape)",
        vulns: &["itlb_multihit", "vmscape"],
        params: &["kvm.nx_huge_pages=off", "vmscape=off"],
        risk: "baixo sem VMs de terceiros",
        gain: "só em VMs",
        note: "",
        heavy: false,
    },
    Mitigation {
        title: "AMD: SRSO e TSA",
        vulns: &["spec_rstack_overflow", "tsa"],
        params: &["spec_rstack_overflow=off", "tsa=off"],
        risk: "médio",
        gain: "médio",
        note: "",
        heavy: false,
    },
];

/// (vulnerabilidade, situação) informadas pelo kernel.
pub fn vulnerabilities() -> Vec<(String, String)> {
    let Ok(rd) = std::fs::read_dir(crate::util::sys("devices/system/cpu/vulnerabilities")) else {
        return vec![];
    };
    let mut v: Vec<(String, String)> = rd
        .flatten()
        .filter_map(|e| {
            Some((
                e.file_name().to_string_lossy().to_string(),
                crate::util::read_trim(e.path())?,
            ))
        })
        .collect();
    v.sort();
    v
}

/// Grupos que se aplicam a este processador (algum item "afetado").
pub fn relevant_mitigations(vulns: &[(String, String)]) -> Vec<&'static Mitigation> {
    MITIGATIONS
        .iter()
        .filter(|m| {
            vulns
                .iter()
                .any(|(n, s)| m.vulns.contains(&n.as_str()) && !s.starts_with("Not affected"))
        })
        .collect()
}

/// Parâmetros em que só pode haver um valor (adicionar troca o antigo).
fn key_of(p: &str) -> &str {
    p.split('=').next().unwrap_or(p)
}
const SINGLE: &[&str] = &[
    "amdgpu.dcdebugmask",
    "mitigations",
    "iommu",
    "intel_iommu",
    "pcie_aspm",
    "intel_idle.max_cstate",
    "transparent_hugepage",
    "pti",
    "spectre_v2",
    "spectre_v2_user",
    "spectre_bhi",
    "spec_store_bypass_disable",
    "mds",
    "tsx_async_abort",
    "mmio_stale_data",
    "reg_file_data_sampling",
    "l1tf",
    "retbleed",
    "gather_data_sampling",
    "srbds",
    "indirect_target_selection",
    "kvm.nx_huge_pages",
    "vmscape",
    "spec_rstack_overflow",
    "tsa",
];

pub fn edit_params(cur: &str, add: &[String], remove: &[String]) -> String {
    let mut v: Vec<String> = cur.split_whitespace().map(String::from).collect();
    v.retain(|t| {
        !remove
            .iter()
            .any(|r| r == t || (!r.contains('=') && key_of(t) == r))
    });
    for a in add {
        if SINGLE.contains(&key_of(a)) {
            v.retain(|t| key_of(t) != key_of(a));
        }
        if !v.contains(a) {
            v.push(a.clone());
        }
    }
    v.join(" ")
}

/// Acha o texto entre aspas depois de `prefix` numa linha `PREFIX="..."` ou `PREFIX='...'`.
fn quoted_after(line: &str, eq_pos: usize) -> Option<(usize, usize)> {
    let rest = &line[eq_pos..];
    let q = rest.find(['"', '\''])?;
    let qc = rest.as_bytes()[q] as char;
    let start = eq_pos + q + 1;
    let end = start + line[start..].find(qc)?;
    Some((start, end))
}

/// Aplica `f` aos parâmetros das linhas certas do arquivo e devolve o novo texto
/// (e os parâmetros encontrados na primeira linha, antes da mudança).
pub fn rewrite(kind: Kind, text: &str, f: &dyn Fn(&str) -> String) -> (String, Option<String>) {
    let mut first = None;
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        let target = match kind {
            _ if t.starts_with('#') => None,
            Kind::Limine => {
                // KERNEL_CMDLINE[default]="..."  (todas as entradas, menos a de emergência "fallback")
                if t.starts_with("KERNEL_CMDLINE[") && !t.contains("fallback") {
                    line.find('=').and_then(|e| quoted_after(line, e))
                } else {
                    None
                }
            }
            Kind::SdbootManage => t
                .starts_with("LINUX_OPTIONS=")
                .then(|| line.find('=').and_then(|e| quoted_after(line, e)))
                .flatten(),
            Kind::Grub => t
                .starts_with("GRUB_CMDLINE_LINUX_DEFAULT=")
                .then(|| line.find('=').and_then(|e| quoted_after(line, e)))
                .flatten(),
            Kind::Refind => {
                // "Rótulo"  "opções": edita o segundo texto entre aspas
                quoted_after(line, 0).and_then(|(_, e1)| quoted_after(line, e1 + 1))
            }
        };
        match target {
            Some((a, b)) => {
                let cur = &line[a..b];
                if first.is_none() {
                    first = Some(cur.to_string());
                }
                out.push(format!("{}{}{}", &line[..a], f(cur), &line[b..]));
            }
            None => out.push(line.to_string()),
        }
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    (s, first)
}

/// Parâmetros configurados no bootloader agora (para mostrar na interface).
pub fn configured() -> Option<(Loader, String)> {
    let l = detect()?;
    let text = std::fs::read_to_string(&l.path).ok()?;
    let (_, first) = rewrite(l.kind, &text, &|s| s.to_string());
    Some((l, first.unwrap_or_default()))
}

/// Parâmetros com que o kernel atual iniciou.
pub fn running() -> String {
    std::fs::read_to_string("/proc/cmdline")
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Edita (com backup) e regrava as entradas. Precisa de root.
pub fn apply(add: &[String], remove: &[String], log: &mut dyn FnMut(&str)) -> Result<(), String> {
    for p in add.iter().chain(remove) {
        if p.is_empty()
            || p.chars().any(|c| {
                c.is_whitespace() || c == '"' || c == '\'' || c == '\\' || c == '$' || c == '`'
            })
        {
            return Err(crate::tf!(
                "parâmetro inválido: {p:?}",
                "invalid parameter: {p:?}"
            ));
        }
    }
    if !crate::util::is_root() {
        return Err(crate::i18n::tr(
            "precisa de root (use sudo, ou o botão da interface pede a senha)",
        )
        .into());
    }
    let l = detect().ok_or(crate::i18n::tr(
        "não reconheci o bootloader (Limine, systemd-boot/sdboot-manage, GRUB ou rEFInd)",
    ))?;
    let text =
        std::fs::read_to_string(&l.path).map_err(|e| format!("{}: {e}", l.path.display()))?;
    let (new, first) = rewrite(l.kind, &text, &|cur| edit_params(cur, add, remove));
    if first.is_none() {
        return Err(crate::tf!(
            "não achei a linha dos parâmetros em {}",
            "could not find the parameters line in {}",
            l.path.display()
        ));
    }
    if new == text {
        log(crate::i18n::tr("Nada a mudar."));
        return Ok(());
    }
    let ts = crate::history::now();
    let bak = (0..)
        .map(|i| {
            l.path.with_extension(if i == 0 {
                format!("numapilot-bak-{ts}")
            } else {
                format!("numapilot-bak-{ts}-{i}")
            })
        })
        .find(|p| !p.exists())
        .unwrap();
    std::fs::write(&bak, &text).map_err(|e| format!("backup: {e}"))?;
    std::fs::write(&l.path, &new).map_err(|e| format!("{}: {e}", l.path.display()))?;
    log(&crate::tf!(
        "{} ({}): backup em {}",
        "{} ({}): backup in {}",
        l.name(),
        l.path.display(),
        bak.display()
    ));
    let (_, after) = rewrite(l.kind, &new, &|s| s.to_string());
    log(&crate::tf!(
        "Parâmetros: {}",
        "Parameters: {}",
        after.unwrap_or_default()
    ));
    if let Some(cmd) = l.regen() {
        if crate::containers::which(cmd[0]) {
            log(&crate::tf!(
                "Regravando as entradas: {}",
                "Rewriting the entries: {}",
                cmd.join(" ")
            ));
            let st = std::process::Command::new(cmd[0])
                .args(&cmd[1..])
                .status()
                .map_err(|e| e.to_string())?;
            if !st.success() {
                // Volta o arquivo para não deixar meio aplicado.
                let _ = std::fs::write(&l.path, &text);
                return Err(crate::tf!(
                    "{} falhou ({st}); arquivo original restaurado",
                    "{} failed ({st}); original file restored",
                    cmd[0]
                ));
            }
        } else {
            log(&crate::tf!(
                "Aviso: {} não encontrado; rode o equivalente para regravar o boot.",
                "Warning: {} not found; run the equivalent to rewrite the boot entries.",
                cmd[0]
            ));
        }
    }
    log(crate::i18n::tr("Pronto. Reinicie para valer."));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(kind: Kind, text: &str, add: &[&str], rm: &[&str]) -> String {
        let add: Vec<String> = add.iter().map(|s| s.to_string()).collect();
        let rm: Vec<String> = rm.iter().map(|s| s.to_string()).collect();
        rewrite(kind, text, &|c| edit_params(c, &add, &rm)).0
    }

    #[test]
    fn formats() {
        let lim = "# comentário\nKERNEL_CMDLINE[default]=\"quiet splash\"\nKERNEL_CMDLINE[\"linux-cachyos\"]=\"quiet\"\nKERNEL_CMDLINE[fallback]=\"quiet\"\n";
        let out = ed(Kind::Limine, lim, &["pci=realloc"], &[]);
        assert!(out.contains("KERNEL_CMDLINE[default]=\"quiet splash pci=realloc\""));
        assert!(out.contains("KERNEL_CMDLINE[\"linux-cachyos\"]=\"quiet pci=realloc\""));
        assert!(
            out.contains("KERNEL_CMDLINE[fallback]=\"quiet\""),
            "a entrada de emergência fica intacta"
        );
        assert!(out.starts_with("# comentário\n") && out.ends_with('\n'));

        let sd = "LINUX_OPTIONS=\"zswap.enabled=0 nowatchdog quiet splash\"\nLINUX_FALLBACK_OPTIONS=\"\"\n";
        let out = ed(
            Kind::SdbootManage,
            sd,
            &["mitigations=off"],
            &["nowatchdog"],
        );
        assert!(out.starts_with("LINUX_OPTIONS=\"zswap.enabled=0 quiet splash mitigations=off\""));

        let grub = "GRUB_DEFAULT=0\nGRUB_CMDLINE_LINUX_DEFAULT='loglevel=3 quiet mitigations=auto'\nGRUB_CMDLINE_LINUX=\"\"\n";
        let out = ed(Kind::Grub, grub, &["mitigations=off"], &[]);
        assert!(out.contains("GRUB_CMDLINE_LINUX_DEFAULT='loglevel=3 quiet mitigations=off'"));
        assert!(out.contains("GRUB_CMDLINE_LINUX=\"\""));

        let rf = "\"Boot using default options\"     \"root=PARTUUID=abc rw quiet\"\n\"Boot to terminal\"   \"root=PARTUUID=abc rw systemd.unit=multi-user.target\"\n";
        let out = ed(Kind::Refind, rf, &["iommu=pt"], &[]);
        assert!(out.contains("\"root=PARTUUID=abc rw quiet iommu=pt\""));
        assert!(out.contains("\"Boot using default options\""));

        assert_eq!(
            edit_params("a pci=realloc b", &[], &["pci=realloc".into()]),
            "a b"
        );
        assert_eq!(
            edit_params("a mds=full", &["mds=off".into()], &[]),
            "a mds=off"
        );
        let v = vec![
            ("meltdown".to_string(), "Mitigation: PTI".to_string()),
            ("srbds".into(), "Not affected".into()),
        ];
        let r = relevant_mitigations(&v);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].params, &["pti=off"]);
        assert_eq!(
            edit_params("a", &["iommu=pt".into(), "iommu=pt".into()], &[]),
            "a iommu=pt"
        );
    }
}
