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
