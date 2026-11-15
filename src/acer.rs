//! Acer Predator/Nitro laptops with the community module Linuwu-Sense (linuwu_sense):
//! fans, 80% charge limit, display overdrive, USB charging while off, keyboard light
//! and 4-zone RGB. The files live in /sys/devices/platform/acer-wmi/{predator,nitro}_sense.
//! The power profile (including Turbo) comes through platform_profile, which numapilot already
//! controls.

use crate::util::{read_trim, sys};
use std::path::PathBuf;

/// Known settings: key, file, name, explanation.
pub const KNOBS: &[(&str, &str, &str, &str)] = &[
    (
        "fan_speed",
        "fan_speed",
        "Fans (CPU, GPU %)",
        "0,0 = automatic; 100,100 = maximum",
    ),
    (
        "battery_limiter",
        "battery_limiter",
        "Stop charging at 80%",
        "extends battery life when it stays plugged in",
    ),
    (
        "lcd_override",
        "lcd_override",
        "Display overdrive",
        "less ghosting in fast motion",
    ),
    (
        "backlight_timeout",
        "backlight_timeout",
        "Turn off the keyboard light",
        "after 30 s of no use",
    ),
    (
        "usb_charging",
        "usb_charging",
        "USB charging while off",
        "until the battery reaches 10, 20 or 30% (0 = no)",
    ),
    (
        "boot_animation_sound",
        "boot_animation_sound",
        "Boot animation and sound",
        "",
    ),
];

/// 4-zone RGB effects (four_zone_mode).
pub const EFFECTS: &[(u8, &str)] = &[
    (0, "fixed"),
    (1, "breathing"),
    (2, "neon"),
    (3, "wave"),
    (4, "shifting"),
    (5, "zoom"),
    (6, "meteor"),
    (7, "twinkling"),
];

fn platform() -> PathBuf {
    sys("devices/platform/acer-wmi")
}

/// Linuwu-Sense folder (predator_sense or nitro_sense), if the module is loaded.
pub fn sense_dir() -> Option<PathBuf> {
    ["predator_sense", "nitro_sense"]
        .iter()
        .map(|d| platform().join(d))
        .find(|p| p.is_dir())
}

pub fn kb_dir() -> Option<PathBuf> {
    let p = platform().join("four_zoned_kb");
    p.is_dir().then_some(p)
}

pub fn is_acer() -> bool {
    read_trim(sys("class/dmi/id/sys_vendor")).is_some_and(|v| v.to_lowercase().contains("acer"))
}

/// The kernel side of an Acer laptop: which module drives it and what it exposes.
#[derive(Debug, Default, Clone)]
pub struct ModuleState {
    pub acer_wmi: bool,
    /// acer_wmi loaded with predator_v4 on (profiles including Turbo, fan readings)
    pub acer_wmi_pv4: bool,
    pub linuwu: bool,
    /// linuwu_sense forced to a model (nitro_v4 / predator_v4 parameter on)
    pub linuwu_forced: bool,
    /// nitro_sense / predator_sense / four_zoned_kb exist
    pub sense: bool,
    /// someone provides a power profile (platform_profile)
    pub profiles: bool,
    pub product: String,
    /// modprobe.d file that blacklists acer_wmi
    pub acer_wmi_blacklist: Option<String>,
    /// some modprobe.d line asks for acer_wmi predator_v4=1
    pub pv4_configured: bool,
}

pub fn module_state() -> ModuleState {
    let on = |rel: &str| matches!(read_trim(sys(rel)).as_deref(), Some("Y") | Some("1"));
    let mut s = ModuleState {
        acer_wmi: sys("module/acer_wmi").exists(),
        acer_wmi_pv4: on("module/acer_wmi/parameters/predator_v4"),
        linuwu: sys("module/linuwu_sense").exists(),
        linuwu_forced: on("module/linuwu_sense/parameters/nitro_v4")
            || on("module/linuwu_sense/parameters/predator_v4"),
        sense: sense_dir().is_some() || kb_dir().is_some(),
        profiles: !crate::notebook::profile_handlers().is_empty()
            || sys("firmware/acpi/platform_profile").exists(),
        product: read_trim(sys("class/dmi/id/product_name")).unwrap_or_default(),
        ..Default::default()
    };
    let files: Vec<(String, String)> = std::fs::read_dir("/etc/modprobe.d")
        .map(|rd| {
            rd.flatten()
                .map(|e| {
                    (
                        e.path().display().to_string(),
                        std::fs::read_to_string(e.path()).unwrap_or_default(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    apply_modprobe(&mut s, &files);
    s
}

/// Reads acer_wmi blacklists and predator_v4 options from modprobe.d files (path, text).
pub fn apply_modprobe(s: &mut ModuleState, files: &[(String, String)]) {
    for (path, text) in files {
        for l in text.lines().map(str::trim).filter(|l| !l.starts_with('#')) {
            let w: Vec<&str> = l.split_whitespace().collect();
            let names_acer = |i: usize| w.get(i).is_some_and(|m| m.replace('-', "_") == "acer_wmi");
            if w.first() == Some(&"blacklist") && names_acer(1)
                || w.first() == Some(&"install") && names_acer(1) && l.contains("/bin/false")
            {
                s.acer_wmi_blacklist = Some(path.clone());
            }
            if w.first() == Some(&"options") && names_acer(1) && l.contains("predator_v4=1") {
                s.pv4_configured = true;
            }
        }
    }
}

/// Linuwu-Sense parameter that forces this model: Nitro or Predator.
pub fn model_param(product: &str) -> &'static str {
    if product.to_lowercase().contains("nitro") {
        "nitro_v4"
    } else {
        "predator_v4"
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Ok,
    Info,
    Warn,
}

/// What the doctor says about the Acer side: (name, verdict, message, fix).
pub fn findings(s: &ModuleState) -> Vec<(&'static str, Verdict, String, Option<String>)> {
    let mut v = vec![];
    let param = model_param(&s.product);
    let force = crate::tf!(
        "echo 'options linuwu_sense {param}=1' | sudo tee /etc/modprobe.d/linuwu-sense.conf, then sudo modprobe -r linuwu_sense && sudo modprobe linuwu_sense (stop DAMX or another manager first)",
        "echo 'options linuwu_sense {param}=1' | sudo tee /etc/modprobe.d/linuwu-sense.conf, depois sudo modprobe -r linuwu_sense && sudo modprobe linuwu_sense (pare o DAMX ou outro gerenciador antes)"
    );
    // Linuwu-Sense: fans, 80% limit, RGB
    if s.sense {
        v.push((
            "Acer (Linuwu-Sense)",
            Verdict::Ok,
            crate::tf!(
                "{} settings available",
                "{} ajustes disponíveis",
                knobs().len()
            ),
            None,
        ));
    } else if s.linuwu {
        v.push((
            "Acer (Linuwu-Sense)",
            Verdict::Warn,
            crate::tf!(
                "linuwu_sense is loaded but did not recognize this laptop ({}): no manual fan, 80% limit, RGB or power profile",
                "o linuwu_sense está carregado mas não reconheceu este notebook ({}): sem ventoinha manual, limite de 80%, RGB e perfil de energia",
                s.product
            ),
            Some(if s.linuwu_forced {
                crate::tf!(
                    "the model parameter is already on and nothing shows up: this model may not be supported yet (https://github.com/0x7375646F/Linuwu-Sense)",
                    "o parâmetro de modelo já está ligado e nada aparece: talvez este modelo ainda não seja suportado (https://github.com/0x7375646F/Linuwu-Sense)"
                )
            } else {
                force.clone()
            }),
        ));
    } else {
        v.push((
            "Acer (Linuwu-Sense)",
            Verdict::Info,
            crate::i18n::tr("module not loaded: no manual fan, 80% limit or RGB").into(),
            Some("https://github.com/0x7375646F/Linuwu-Sense".into()),
        ));
    }
    // acer_wmi predator_v4: only what is actually loaded counts, not the modprobe.d line.
    if s.acer_wmi && s.acer_wmi_pv4 {
        v.push((
            "acer_wmi predator_v4",
            Verdict::Ok,
            crate::i18n::tr("on: power profiles and fans through the kernel (numapilot uses it)")
                .into(),
            None,
        ));
    } else if s.pv4_configured && !s.acer_wmi && !s.linuwu {
        v.push((
            "acer_wmi predator_v4",
            Verdict::Warn,
            match &s.acer_wmi_blacklist {
                Some(f) => crate::tf!(
                    "configured, but acer_wmi is not loaded: {f} blocks it",
                    "configurado, mas o acer_wmi não está carregado: {f} bloqueia ele"
                ),
                None => crate::tf!(
                    "configured, but acer_wmi is not loaded",
                    "configurado, mas o acer_wmi não está carregado"
                ),
            },
            Some(crate::tf!(
                "remove the blacklist (or install Linuwu-Sense, which replaces acer_wmi) and run sudo modprobe acer_wmi",
                "tire o bloqueio (ou instale o Linuwu-Sense, que substitui o acer_wmi) e rode sudo modprobe acer_wmi"
            )),
        ));
    } else if s.pv4_configured && s.acer_wmi {
        v.push((
            "acer_wmi predator_v4",
            Verdict::Warn,
            crate::tf!(
                "configured, but the loaded acer_wmi has it off",
                "configurado, mas o acer_wmi carregado está com ele desligado"
            ),
            Some("sudo modprobe -r acer_wmi && sudo modprobe acer_wmi".into()),
        ));
    }
    // Without a power profile the machine modes cannot change the laptop's profile.
    if !s.profiles {
        v.push((
            crate::t2!("Acer: power profile", "Acer: perfil de energia"),
            Verdict::Warn,
            crate::tf!(
                "no power profile provider (/sys/class/platform-profile is empty): the machine modes do not change the laptop's profile",
                "nenhum fornecedor de perfil de energia (/sys/class/platform-profile vazio): os modos da máquina não mudam o perfil do notebook"
            ),
            if s.linuwu && !s.sense && !s.linuwu_forced {
                Some(force)
            } else if !s.linuwu && !s.acer_wmi {
                Some(crate::tf!(
                    "sudo modprobe acer_wmi predator_v4=1 (after removing any blacklist of acer_wmi)",
                    "sudo modprobe acer_wmi predator_v4=1 (depois de tirar o bloqueio do acer_wmi, se houver)"
                ))
            } else {
                None
            },
        ));
    }
    v
}

/// Settings this laptop has: (key, current value).
pub fn knobs() -> Vec<(&'static str, Option<String>)> {
    let Some(d) = sense_dir() else { return vec![] };
    KNOBS
        .iter()
        .filter(|k| d.join(k.1).exists())
        .map(|k| (k.0, read_trim(d.join(k.1))))
        .collect()
}

fn path_for(key: &str) -> Option<PathBuf> {
    match key {
        "kb_effect" => Some(kb_dir()?.join("four_zone_mode")),
        "kb_zones" => Some(kb_dir()?.join("per_zone_mode")),
        k => {
            let f = KNOBS.iter().find(|x| x.0 == k)?.1;
            Some(sense_dir()?.join(f))
        }
    }
}

pub fn read(key: &str) -> Option<String> {
    read_trim(path_for(key)?)
}

/// Checks the format before writing to the driver.
pub fn validate(key: &str, v: &str) -> Result<(), String> {
    let nums = |n: usize, max: &[u32]| -> Result<(), String> {
        let p: Vec<&str> = v.split(',').collect();
        if p.len() != n {
            return Err(crate::tf!(
                "{key}: expected {n} comma-separated numbers",
                "{key}: esperava {n} números separados por vírgula"
            ));
        }
        for (i, x) in p.iter().enumerate() {
            let x: u32 = x.trim().parse().map_err(|_| {
                crate::tf!("{key}: '{x}' is not a number", "{key}: '{x}' não é número")
            })?;
            if x > max[i.min(max.len() - 1)] {
                return Err(crate::tf!(
                    "{key}: {x} above the maximum {}",
                    "{key}: {x} acima do máximo {}",
                    max[i.min(max.len() - 1)]
                ));
            }
        }
        Ok(())
    };
    match key {
        "fan_speed" => nums(2, &[100]),
        "battery_limiter" | "lcd_override" | "backlight_timeout" | "boot_animation_sound" => {
            nums(1, &[1])
        }
        "usb_charging" => {
            if ["0", "10", "20", "30"].contains(&v) {
                Ok(())
            } else {
                Err("usb_charging: use 0, 10, 20 or 30".into())
            }
        }
        // mode, speed, brightness, direction, r, g, b
        "kb_effect" => nums(7, &[7, 9, 100, 2, 255]),
        // RRGGBB,RRGGBB,RRGGBB,RRGGBB,brightness
        "kb_zones" => {
            let p: Vec<&str> = v.split(',').collect();
            let hex_ok = p.len() == 5
                && p[..4]
                    .iter()
                    .all(|z| z.len() == 6 && z.chars().all(|c| c.is_ascii_hexdigit()));
            let b_ok = p
                .get(4)
                .and_then(|b| b.parse::<u32>().ok())
                .is_some_and(|b| b <= 100);
            if hex_ok && b_ok {
                Ok(())
            } else {
                Err("kb_zones: use RRGGBB,RRGGBB,RRGGBB,RRGGBB,brilho".into())
            }
        }
        _ => Err(crate::tf!(
            "unknown setting: {key}",
            "ajuste desconhecido: {key}"
        )),
    }
}

pub fn set(key: &str, v: &str) -> Result<(), String> {
    validate(key, v)?;
    let p = path_for(key).ok_or_else(|| {
        crate::tf!(
            "{key}: the linuwu_sense module does not expose this on this laptop",
            "{key}: o módulo linuwu_sense não expõe isso neste notebook"
        )
    })?;
    std::fs::write(&p, v).map_err(|e| format!("{}: {e}", p.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert!(validate("fan_speed", "50,70").is_ok());
        assert!(validate("fan_speed", "50,170").is_err());
        assert!(validate("battery_limiter", "2").is_err());
        assert!(validate("usb_charging", "20").is_ok());
        assert!(validate("kb_effect", "3,1,100,0,255,0,0").is_ok());
        assert!(validate("kb_zones", "ff0000,00ff00,0000ff,ffffff,80").is_ok());
        assert!(validate("kb_zones", "ff0000,00ff00,0000ff,80").is_err());
        assert!(validate("x", "1").is_err());
    }

    #[test]
    fn fake() {
        let root = std::env::temp_dir().join(format!("np-acer-{}", std::process::id()));
        let d = root.join("devices/platform/acer-wmi/predator_sense");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("fan_speed"), "0,0\n").unwrap();
        std::fs::write(d.join("battery_limiter"), "0\n").unwrap();
        let _g = crate::util::test_sysfs_lock(&root);
        let k = knobs();
        assert_eq!(k.len(), 2);
        set("battery_limiter", "1").unwrap();
        assert_eq!(read("battery_limiter").as_deref(), Some("1"));
        assert!(set("kb_effect", "0,1,100,0,255,0,0").is_err());
        let _ = std::fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;

    fn kinds(s: &ModuleState) -> Vec<(&'static str, Verdict)> {
        findings(s).into_iter().map(|f| (f.0, f.1)).collect()
    }

    #[test]
    fn modprobe_lines() {
        let mut s = ModuleState::default();
        apply_modprobe(
            &mut s,
            &[
                (
                    "/etc/modprobe.d/acer-wmi.conf".into(),
                    "options acer_wmi predator_v4=1\n".into(),
                ),
                (
                    "/etc/modprobe.d/blacklist-acer_wmi.conf".into(),
                    "# x\nblacklist acer_wmi\n".into(),
                ),
            ],
        );
        assert!(s.pv4_configured);
        assert_eq!(
            s.acer_wmi_blacklist.as_deref(),
            Some("/etc/modprobe.d/blacklist-acer_wmi.conf")
        );
        let mut s = ModuleState::default();
        apply_modprobe(
            &mut s,
            &[(
                "a".into(),
                "# blacklist acer_wmi\noptions acer_wmi predator_v4=0\n".into(),
            )],
        );
        assert!(!s.pv4_configured && s.acer_wmi_blacklist.is_none());
    }

    #[test]
    fn nitro_with_linuwu_not_recognized() {
        // The Nitro AN515-46 of the QA round: acer_wmi blocked, linuwu_sense loaded with no model
        // parameter, nothing exposed. The doctor said "ok" because of the modprobe.d line.
        let s = ModuleState {
            linuwu: true,
            product: "Nitro AN515-46".into(),
            acer_wmi_blacklist: Some("/etc/modprobe.d/blacklist-acer_wmi.conf".into()),
            pv4_configured: true,
            ..Default::default()
        };
        let k = kinds(&s);
        assert!(!k.iter().any(|(_, v)| *v == Verdict::Ok), "{k:?}");
        assert!(k.contains(&("Acer (Linuwu-Sense)", Verdict::Warn)));
        assert!(k.contains(&("Acer: power profile", Verdict::Warn)));
        let fix = findings(&s)[0].3.clone().unwrap();
        assert!(fix.contains("nitro_v4=1"), "{fix}");
    }

    #[test]
    fn working_setups() {
        let pv4 = ModuleState {
            acer_wmi: true,
            acer_wmi_pv4: true,
            profiles: true,
            pv4_configured: true,
            ..Default::default()
        };
        assert!(kinds(&pv4).contains(&("acer_wmi predator_v4", Verdict::Ok)));
        assert!(!kinds(&pv4).iter().any(|(_, v)| *v == Verdict::Warn));
        let linuwu = ModuleState {
            linuwu: true,
            sense: true,
            profiles: true,
            product: "Predator PHN16-71".into(),
            ..Default::default()
        };
        assert_eq!(kinds(&linuwu), vec![("Acer (Linuwu-Sense)", Verdict::Ok)]);
        assert_eq!(model_param("Predator PHN16-71"), "predator_v4");
    }

    #[test]
    fn blacklisted_without_linuwu() {
        let s = ModuleState {
            pv4_configured: true,
            acer_wmi_blacklist: Some("/etc/modprobe.d/b.conf".into()),
            ..Default::default()
        };
        let f = findings(&s);
        let pv4 = f.iter().find(|x| x.0 == "acer_wmi predator_v4").unwrap();
        assert_eq!(pv4.1, Verdict::Warn);
        assert!(pv4.2.contains("/etc/modprobe.d/b.conf"));
    }
}
