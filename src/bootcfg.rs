//! Opções que só valem a partir do boot, aplicadas de um jeito que sobrevive às atualizações do
//! kernel: opções de módulo em /etc/modprobe.d (não dependem do bootloader nem da versão do kernel)
//! e a imagem de boot (initramfs) refeita com a ferramenta da distro.

use std::path::PathBuf;

/// Arquivo gerenciado pelo numapilot.
pub fn conf_path() -> PathBuf {
    std::env::var_os("NUMAPILOT_MODPROBE_CONF")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/modprobe.d/numapilot.conf"))
}

/// Bit do OverDrive (PP_OVERDRIVE_MASK) no amdgpu.ppfeaturemask.
pub const OVERDRIVE_BIT: u64 = 0x4000;

fn parse_hex(s: &str) -> Option<u64> {
    let s = s.trim();
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => u64::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

/// Valor em uso agora pelo driver (None = amdgpu não carregado).
pub fn current_mask() -> Option<u64> {
    let p = crate::util::sys("module/amdgpu/parameters/ppfeaturemask");
    crate::util::read_trim(p).and_then(|s| parse_hex(&s))
}

/// Valor definido na linha do kernel (amdgpu.ppfeaturemask=...), que tem prioridade.
pub fn cmdline_mask() -> Option<u64> {
    let c = std::fs::read_to_string("/proc/cmdline").ok()?;
    c.split_whitespace()
        .find_map(|w| w.strip_prefix("amdgpu.ppfeaturemask="))
        .and_then(parse_hex)
}

/// Valor salvo no nosso arquivo.
pub fn saved_mask() -> Option<u64> {
    let s = std::fs::read_to_string(conf_path()).ok()?;
    s.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .flat_map(|l| l.split_whitespace())
        .find_map(|w| w.strip_prefix("ppfeaturemask="))
        .and_then(parse_hex)
}

#[derive(Debug, Clone, PartialEq)]
pub struct OverdriveState {
    /// Ativo agora (o driver carregou com o bit).
    pub active: bool,
    /// Vai estar ativo depois de reiniciar.
    pub on_next_boot: bool,
    /// A linha do kernel define o valor (nosso arquivo é ignorado).
    pub from_cmdline: bool,
}

pub fn overdrive_state() -> OverdriveState {
    let active = current_mask().is_some_and(|m| m & OVERDRIVE_BIT != 0);
    let cmd = cmdline_mask();
    let on_next_boot = match cmd {
        Some(m) => m & OVERDRIVE_BIT != 0,
        None => saved_mask().is_some_and(|m| m & OVERDRIVE_BIT != 0),
    };
    OverdriveState {
        active,
        on_next_boot,
        from_cmdline: cmd.is_some(),
    }
}

/// Máscara a gravar: a atual com só o bit do OverDrive ligado (não mexe em mais nada).
pub fn overdrive_mask(base: u64) -> u64 {
    base | OVERDRIVE_BIT
}

fn render(mask: Option<u64>) -> String {
    let mut s = String::from(
        "# Gerado pelo numapilot (Configurações → Opções de boot). Edite pela interface ou\n\
         # com `sudo numapilot boot ...`. Vale para qualquer kernel instalado.\n",
    );
    if let Some(m) = mask {
        s.push_str("# OverDrive das placas AMD (clock/voltagem: undervolt e overclock)\n");
        s.push_str(&format!("options amdgpu ppfeaturemask=0x{m:x}\n"));
    }
    s
}

/// Ferramenta que refaz a imagem de boot, na ordem de preferência.
pub fn initramfs_tool() -> Option<Vec<&'static str>> {
    let has = |b: &str| crate::containers::which(b);
    if has("limine-mkinitcpio") {
        Some(vec!["limine-mkinitcpio"])
    } else if has("mkinitcpio") {
        Some(vec!["mkinitcpio", "-P"])
    } else if has("dracut") {
        Some(vec!["dracut", "--regenerate-all", "--force"])
    } else if has("update-initramfs") {
        Some(vec!["update-initramfs", "-u", "-k", "all"])
    } else {
        None
    }
}

pub fn regenerate(log: &mut dyn FnMut(&str)) -> Result<(), String> {
    let Some(cmd) = initramfs_tool() else {
        log(
            "Nenhuma ferramenta de initramfs encontrada; o arquivo vale quando o módulo carregar do disco.",
        );
        return Ok(());
    };
    log(&format!("Refazendo a imagem de boot: {}", cmd.join(" ")));
    let st = std::process::Command::new(cmd[0])
        .args(&cmd[1..])
        .status()
        .map_err(|e| format!("{}: {e}", cmd[0]))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("{} terminou com erro ({st})", cmd[0]))
    }
}

/// Liga/desliga o OverDrive no boot. Precisa de root.
pub fn set_overdrive(on: bool, log: &mut dyn FnMut(&str)) -> Result<(), String> {
    if !crate::util::is_root() {
        return Err("precisa de root (use sudo, ou o botão da interface pede a senha)".into());
    }
    if let Some(m) = cmdline_mask() {
        log(&format!(
            "Aviso: a linha do kernel já define amdgpu.ppfeaturemask=0x{m:x}, e ela tem prioridade sobre este arquivo."
        ));
    }
    let new = if on {
        // Parte do valor em uso (padrão do driver) para não mudar outros recursos.
        let base = saved_mask().or_else(current_mask).unwrap_or(0xfff7_bfff);
        Some(overdrive_mask(base))
    } else {
        None
    };
    let path = conf_path();
    let old = std::fs::read_to_string(&path).ok();
    let text = render(new);
    if old.as_deref() == Some(text.as_str()) {
        log("Nada mudou no arquivo.");
    } else if new.is_none() && old.is_none() {
        log("Já estava desligado.");
        return Ok(());
    } else {
        if let Some(o) = &old {
            let _ = std::fs::write(path.with_extension("conf.bak"), o);
        }
        match new {
            None => {
                std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
                log(&format!("Removido {}", path.display()));
            }
            Some(m) => {
                if let Some(d) = path.parent() {
                    std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
                }
                std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
                log(&format!(
                    "Gravado {}: ppfeaturemask=0x{m:x}",
                    path.display()
                ));
            }
        }
    }
    regenerate(log)?;
    log("Pronto. Reinicie o computador para valer. Atualizações do kernel mantêm a opção.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks() {
        assert_eq!(parse_hex("0xfff7bfff"), Some(0xfff7bfff));
        assert_eq!(parse_hex("4294426623"), Some(0xfff7bfff));
        assert_eq!(overdrive_mask(0xfff7bfff), 0xfff7ffff);
        assert_eq!(overdrive_mask(0xfffd7fff), 0xfffd7fff);
        assert!(render(Some(0xfff7ffff)).contains("options amdgpu ppfeaturemask=0xfff7ffff"));
        assert!(!render(None).contains("options"));
    }
}
