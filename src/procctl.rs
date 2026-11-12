//! Encerrar programas pela interface (com proteção para o que é do sistema e da sessão
//! gráfica) e uso da GPU por processo, lido do fdinfo do DRM.

use std::collections::HashMap;
use std::fs;

/// Programas que nunca podem ser encerrados pela interface: a sessão gráfica (KDE, GNOME...),
/// som, portais, login e o próprio numapilot. Comparação pelo nome do processo (comm) ou
/// pelo nome do executável.
const PROTECTED: &[&str] = &[
    // KDE Plasma
    "plasmashell",
    "kwin_wayland",
    "kwin_x11",
    "kwin_wayland_wr",
    "ksmserver",
    "kded5",
    "kded6",
    "kglobalaccel",
    "kglobalacceld",
    "startplasma-wayland",
    "startplasma-x11",
    "plasma_session",
    "plasma-session",
    "ksplashqml",
    "kactivitymanagerd",
    "polkit-kde-authentication-agent-1",
    "org_kde_powerdevil",
    "powerdevil",
    "kscreen_backend_launcher",
    "xdg-desktop-portal-kde",
    "xembedsniproxy",
    "gmenudbusmenuproxy",
    "baloo_file",
    // GNOME e outros
    "gnome-shell",
    "gnome-session-binary",
    "gnome-session",
    "mutter",
    "gsd-xsettings",
    "Xwayland",
    "Xorg",
    "X",
    "gamescope-session",
    // sistema e sessão
    "systemd",
    "init",
    "dbus-daemon",
    "dbus-broker",
    "dbus-broker-launch",
    "pipewire",
    "pipewire-pulse",
    "wireplumber",
    "pulseaudio",
    "xdg-desktop-portal",
    "xdg-desktop-portal-gtk",
    "xdg-document-portal",
    "xdg-permission-store",
    "at-spi-bus-launcher",
    "at-spi2-registryd",
    "sddm",
    "sddm-helper",
    "gdm",
    "login",
    "agetty",
    "sshd",
    "polkitd",
    "upowerd",
    "udisksd",
    "NetworkManager",
    "bluetoothd",
    "gnome-keyring-daemon",
    "kwalletd5",
    "kwalletd6",
    "ssh-agent",
    "gpg-agent",
    // o próprio numapilot
    "numapilot",
    "numapilot-gui",
];

/// Motivo para NÃO deixar encerrar (None = pode). Lógica pura, para os testes.
/// `cgroup` é o conteúdo de /proc/PID/cgroup (cgroups v2: "0::/caminho").
pub fn protected_reason(
    pid: i32,
    comm: &str,
    exe: &str,
    uid: u32,
    my_uid: u32,
    cgroup: &str,
) -> Option<&'static str> {
    if pid <= 1 {
        return Some(crate::i18n::tr("processo do sistema"));
    }
    if exe.is_empty() {
        // threads do kernel não têm executável
        return Some(crate::i18n::tr("processo do kernel"));
    }
    if uid != my_uid {
        return Some(crate::i18n::tr(
            "não é seu (é do sistema ou de outro usuário)",
        ));
    }
    let exe_name = exe.rsplit('/').next().unwrap_or(exe);
    // "kwin_wayland (deleted)" depois de atualizar o pacote
    let exe_name = exe_name.trim_end_matches(" (deleted)");
    if PROTECTED
        .iter()
        .any(|p| comm == *p || exe_name == *p || exe_name.starts_with("startplasma"))
    {
        return Some(crate::i18n::tr("faz parte da sessão gráfica ou do sistema"));
    }
    let path = cgroup
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .unwrap_or("");
    // systemd: a sessão (KDE, GNOME) roda em session.slice; serviços do sistema em
    // system.slice; programas abertos pelo usuário ficam em app.slice
    if path.contains("/session.slice/")
        || path.starts_with("/system.slice/")
        || path == "/init.scope"
    {
        return Some(crate::i18n::tr("faz parte da sessão gráfica ou do sistema"));
    }
    None
}

/// Igual ao `protected_reason`, lendo tudo do /proc.
pub fn kill_blocked(pid: i32, my_uid: u32) -> Option<&'static str> {
    let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
    let exe = fs::read_link(format!("/proc/{pid}/exe"))
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let uid = fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0);
    let cg = fs::read_to_string(format!("/proc/{pid}/cgroup")).unwrap_or_default();
    if comm.is_empty() {
        return Some(crate::i18n::tr("o processo já terminou"));
    }
    protected_reason(pid, comm.trim(), &exe, uid, my_uid, &cg)
}

/// Encerra os processos (SIGTERM; `force` = SIGKILL). Pula os protegidos. Devolve quantos
/// receberam o sinal e o primeiro motivo de recusa.
pub fn terminate(pids: &[i32], my_uid: u32, force: bool) -> (usize, Option<String>) {
    let sig = if force { libc::SIGKILL } else { libc::SIGTERM };
    let mut sent = 0;
    let mut refused = None;
    for &pid in pids {
        if let Some(why) = kill_blocked(pid, my_uid) {
            refused.get_or_insert_with(|| format!("PID {pid}: {why}"));
            continue;
        }
        // SAFETY: kill só envia um sinal; o PID foi conferido acima
        if unsafe { libc::kill(pid, sig) } == 0 {
            sent += 1;
        } else {
            let e = std::io::Error::last_os_error();
            refused.get_or_insert_with(|| format!("PID {pid}: {e}"));
        }
    }
    (sent, refused)
}

/// Uso de GPU de um processo, somando os clientes DRM dele (cada cliente aparece uma vez,
/// mesmo aberto em vários descritores).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GpuUse {
    /// Tempo total de GPU (ns) em todos os motores; a diferença entre duas leituras dá o uso.
    pub engine_ns: u64,
    /// Memória de vídeo (bytes).
    pub vram: u64,
}

/// Lê o fdinfo do DRM de um processo. None = não usa GPU (ou não dá para ler).
pub fn gpu_use(pid: i32) -> Option<GpuUse> {
    let dir = fs::read_dir(format!("/proc/{pid}/fdinfo")).ok()?;
    let mut clients: HashMap<String, GpuUse> = HashMap::new();
    for e in dir.flatten() {
        let Ok(t) = fs::read_to_string(e.path()) else {
            continue;
        };
        if !t.contains("drm-driver:") {
            continue;
        }
        if let Some((id, u)) = parse_drm_fdinfo(&t) {
            clients.insert(id, u);
        }
    }
    if clients.is_empty() {
        return None;
    }
    Some(clients.values().fold(GpuUse::default(), |a, u| GpuUse {
        engine_ns: a.engine_ns + u.engine_ns,
        vram: a.vram + u.vram,
    }))
}

/// Uso da GPU de um processo separado por placa (endereço PCI). O driver da NVIDIA não
/// publica fdinfo: processos só nela não aparecem aqui.
pub fn gpu_use_by_pci(pid: i32) -> HashMap<String, GpuUse> {
    let mut clients: HashMap<String, GpuUse> = HashMap::new();
    if let Ok(dir) = fs::read_dir(format!("/proc/{pid}/fdinfo")) {
        for e in dir.flatten() {
            let Ok(t) = fs::read_to_string(e.path()) else {
                continue;
            };
            if t.contains("drm-driver:")
                && let Some((id, u)) = parse_drm_fdinfo(&t)
            {
                clients.insert(id, u);
            }
        }
    }
    let mut by: HashMap<String, GpuUse> = HashMap::new();
    for (id, u) in clients {
        let pci = id.split('/').next().unwrap_or("").to_string();
        if pci.is_empty() {
            continue;
        }
        let e = by.entry(pci).or_default();
        e.engine_ns += u.engine_ns;
        e.vram += u.vram;
    }
    by
}

/// O programa pediu a placa `want` (PCI), mas está usando outra? Devolve o PCI da placa em
/// uso de verdade. Só afirma quando dá para ver: a placa pedida publica fdinfo (não é a
/// NVIDIA) e o processo tem memória de vídeo em outra placa e nenhuma na pedida.
pub fn wrong_gpu(by: &HashMap<String, GpuUse>, want: &str, want_is_nvidia: bool) -> Option<String> {
    if want_is_nvidia || by.is_empty() {
        return None;
    }
    let used = |u: &GpuUse| u.vram > 16 * 1024 * 1024 || u.engine_ns > 1_000_000_000;
    if by.get(want).is_some_and(used) {
        return None;
    }
    by.iter()
        .filter(|(p, u)| p.as_str() != want && used(u))
        .max_by_key(|(_, u)| u.vram)
        .map(|(p, _)| p.clone())
}

/// Um fdinfo do DRM: (pdev + drm-client-id, uso). Formato do kernel em
/// Documentation/gpu/drm-usage-stats.rst.
pub fn parse_drm_fdinfo(t: &str) -> Option<(String, GpuUse)> {
    let mut client = None;
    let mut pdev = "";
    let mut u = GpuUse::default();
    // o amdgpu repete a VRAM em drm-memory-vram (antigo) e drm-resident-vram (atual):
    // usa o atual se existir, senão o antigo
    let (mut resident, mut legacy) = (None::<u64>, None::<u64>);
    for l in t.lines() {
        let Some((k, v)) = l.split_once(':') else {
            continue;
        };
        let v = v.trim();
        match k.trim() {
            "drm-client-id" => client = Some(v.to_string()),
            "drm-pdev" => pdev = v,
            k if k.starts_with("drm-engine-") && !k.starts_with("drm-engine-capacity") => {
                if let Some(ns) = v
                    .split_whitespace()
                    .next()
                    .and_then(|n| n.parse::<u64>().ok())
                {
                    u.engine_ns += ns;
                }
            }
            "drm-memory-vram" => legacy = Some(parse_size(v)),
            // amdgpu: drm-resident-vram; xe: drm-resident-vram0, vram1...
            k if k.starts_with("drm-resident-vram") => {
                *resident.get_or_insert(0) += parse_size(v);
            }
            _ => {}
        }
    }
    u.vram = resident.or(legacy).unwrap_or(0);
    Some((format!("{pdev}/{}", client?), u))
}

fn parse_size(v: &str) -> u64 {
    let mut it = v.split_whitespace();
    let n: u64 = it.next().and_then(|n| n.parse().ok()).unwrap_or(0);
    match it.next() {
        Some("KiB") => n * 1024,
        Some("MiB") => n * 1024 * 1024,
        Some("GiB") => n * 1024 * 1024 * 1024,
        _ => n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP: &str =
        "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-steam@x.service\n";
    const SESSION: &str = "0::/user.slice/user-1000.slice/user@1000.service/session.slice/plasma-plasmashell.service\n";

    #[test]
    fn pode_encerrar_programa_do_usuario() {
        assert_eq!(
            protected_reason(4242, "steam", "/usr/lib/steam/steam", 1000, 1000, APP),
            None
        );
        assert_eq!(
            protected_reason(4243, "obs", "/usr/bin/obs", 1000, 1000, APP),
            None
        );
    }

    #[test]
    fn protege_sessao_sistema_e_kernel() {
        // KDE pelo nome, mesmo fora do session.slice
        assert!(
            protected_reason(10, "plasmashell", "/usr/bin/plasmashell", 1000, 1000, APP).is_some()
        );
        assert!(
            protected_reason(
                11,
                "kwin_wayland",
                "/usr/bin/kwin_wayland (deleted)",
                1000,
                1000,
                ""
            )
            .is_some()
        );
        // qualquer coisa no session.slice
        assert!(protected_reason(12, "algo", "/usr/bin/algo", 1000, 1000, SESSION).is_some());
        // de outro usuário / root
        assert!(
            protected_reason(
                13,
                "dockerd",
                "/usr/bin/dockerd",
                0,
                1000,
                "0::/system.slice/docker.service"
            )
            .is_some()
        );
        // thread do kernel (sem exe)
        assert!(protected_reason(14, "kworker/0:1", "", 0, 1000, "0::/").is_some());
        assert!(
            protected_reason(1, "systemd", "/usr/lib/systemd/systemd", 1000, 1000, "").is_some()
        );
        // o próprio numapilot
        assert!(
            protected_reason(
                15,
                "numapilot-gui",
                "/usr/local/bin/numapilot-gui",
                1000,
                1000,
                APP
            )
            .is_some()
        );
    }

    #[test]
    fn fdinfo_amdgpu() {
        let t = "pos:\t0\nflags:\t02100002\ndrm-driver:\tamdgpu\ndrm-client-id:\t42\n\
                 drm-pdev:\t0000:03:00.0\ndrm-resident-vram:\t35840 KiB\ndrm-purgeable-vram:\t4 MiB\ndrm-memory-vram:\t35840 KiB\ndrm-memory-gtt:\t2048 KiB\n\
                 drm-engine-gfx:\t1000 ns\ndrm-engine-compute:\t500 ns\ndrm-engine-capacity-gfx:\t2\n";
        let (id, u) = parse_drm_fdinfo(t).unwrap();
        assert_eq!(id, "0000:03:00.0/42");
        assert_eq!(u.engine_ns, 1500);
        assert_eq!(u.vram, 35840 * 1024);
    }

    #[test]
    fn fdinfo_antigo_so_memory_vram() {
        let t = "drm-driver:\tamdgpu\ndrm-client-id:\t7\ndrm-memory-vram:\t1 MiB\n";
        assert_eq!(parse_drm_fdinfo(t).unwrap().1.vram, 1024 * 1024);
    }

    #[test]
    fn encerra_processo_proprio() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        let me = unsafe { libc::getuid() };
        if let Some(why) = kill_blocked(pid, me) {
            // ex.: a CI roda como serviço (system.slice): lá é protegido, e está certo
            eprintln!("pulei: {why}");
            let _ = child.kill();
            let _ = child.wait();
            return;
        }
        let (n, refused) = terminate(&[pid], me, false);
        assert_eq!((n, refused), (1, None));
        let st = child.wait().unwrap();
        assert!(!st.success());
    }

    #[test]
    fn fdinfo_sem_cliente() {
        assert!(parse_drm_fdinfo("drm-driver:\tamdgpu\n").is_none());
    }
}

#[cfg(test)]
mod wrong_gpu_tests {
    use super::*;

    #[test]
    fn detects_other_card() {
        let big = GpuUse {
            engine_ns: 5_000_000_000,
            vram: 900 << 20,
        };
        let mut by = HashMap::new();
        by.insert("0000:03:00.0".to_string(), big.clone());
        assert_eq!(
            wrong_gpu(&by, "0000:04:00.0", false),
            Some("0000:03:00.0".into())
        );
        assert_eq!(wrong_gpu(&by, "0000:03:00.0", false), None);
        // NVIDIA pedida: não dá para ver, não afirma
        assert_eq!(wrong_gpu(&by, "0000:01:00.0", true), None);
        // usa as duas (apresenta por uma, renderiza na pedida): certo
        by.insert("0000:04:00.0".to_string(), big);
        assert_eq!(wrong_gpu(&by, "0000:04:00.0", false), None);
        assert_eq!(wrong_gpu(&HashMap::new(), "0000:04:00.0", false), None);
    }
}
