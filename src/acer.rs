//! Notebooks Acer Predator/Nitro com o módulo da comunidade Linuwu-Sense (linuwu_sense):
//! ventoinhas, limite de carga em 80%, overdrive da tela, carga USB desligado, luz do teclado
//! e RGB de 4 zonas. Os arquivos ficam em /sys/devices/platform/acer-wmi/{predator,nitro}_sense.
//! O perfil de energia (incluindo o Turbo) vem pelo platform_profile, que o numapilot já controla.

use crate::util::{read_trim, sys};
use std::path::PathBuf;

/// Ajustes conhecidos: chave, arquivo, nome, explicação.
pub const KNOBS: &[(&str, &str, &str, &str)] = &[
    (
        "fan_speed",
        "fan_speed",
        "Ventoinhas (CPU, GPU %)",
        "0,0 = automático; 100,100 = máximo",
    ),
    (
        "battery_limiter",
        "battery_limiter",
        "Parar de carregar em 80%",
        "aumenta a vida da bateria quando fica na tomada",
    ),
    (
        "lcd_override",
        "lcd_override",
        "Overdrive da tela",
        "menos rastro em movimento rápido",
    ),
    (
        "backlight_timeout",
        "backlight_timeout",
        "Apagar a luz do teclado",
        "depois de 30 s sem usar",
    ),
    (
        "usb_charging",
        "usb_charging",
        "Carregar USB desligado",
        "até a bateria chegar a 10, 20 ou 30% (0 = não)",
    ),
    (
        "boot_animation_sound",
        "boot_animation_sound",
        "Animação e som ao ligar",
        "",
    ),
];

/// Efeitos do RGB de 4 zonas (four_zone_mode).
pub const EFFECTS: &[(u8, &str)] = &[
    (0, "fixo"),
    (1, "respiração"),
    (2, "neon"),
    (3, "onda"),
    (4, "alternando"),
    (5, "zoom"),
    (6, "meteoro"),
    (7, "piscando"),
];

fn platform() -> PathBuf {
    sys("devices/platform/acer-wmi")
}

/// Pasta do Linuwu-Sense (predator_sense ou nitro_sense), se o módulo estiver carregado.
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

/// Ajustes que este notebook tem: (chave, valor atual).
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

/// Confere o formato antes de gravar no driver.
pub fn validate(key: &str, v: &str) -> Result<(), String> {
    let nums = |n: usize, max: &[u32]| -> Result<(), String> {
        let p: Vec<&str> = v.split(',').collect();
        if p.len() != n {
            return Err(crate::tf!(
                "{key}: esperava {n} números separados por vírgula",
                "{key}: expected {n} comma-separated numbers"
            ));
        }
        for (i, x) in p.iter().enumerate() {
            let x: u32 = x.trim().parse().map_err(|_| {
                crate::tf!("{key}: '{x}' não é número", "{key}: '{x}' is not a number")
            })?;
            if x > max[i.min(max.len() - 1)] {
                return Err(crate::tf!(
                    "{key}: {x} acima do máximo {}",
                    "{key}: {x} above the maximum {}",
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
                Err("usb_charging: use 0, 10, 20 ou 30".into())
            }
        }
        // modo, velocidade, brilho, direção, r, g, b
        "kb_effect" => nums(7, &[7, 9, 100, 2, 255]),
        // RRGGBB,RRGGBB,RRGGBB,RRGGBB,brilho
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
            "ajuste desconhecido: {key}",
            "unknown setting: {key}"
        )),
    }
}

pub fn set(key: &str, v: &str) -> Result<(), String> {
    validate(key, v)?;
    let p = path_for(key).ok_or_else(|| {
        crate::tf!(
            "{key}: o módulo linuwu_sense não expõe isso neste notebook",
            "{key}: the linuwu_sense module does not expose this on this laptop"
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
