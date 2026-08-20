use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize)]
struct Settings {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    device_name: String,
}

#[derive(Debug, Serialize)]
struct SettingsDocument {
    schema_version: u8,
    device_name: String,
}

pub fn device_name() -> String {
    env::var("UNIFIED_SHARE_DEVICE_NAME")
        .ok()
        .filter(|value| valid_device_name(value))
        .or_else(|| read().ok().map(|settings| settings.device_name))
        .filter(|value| valid_device_name(value))
        .or_else(|| {
            env::var("HOSTNAME")
                .ok()
                .filter(|value| valid_device_name(value))
        })
        .unwrap_or_else(|| "Omarchy".into())
}

pub fn set_device_name(value: &str) -> Result<()> {
    if !valid_device_name(value) {
        bail!("Device name must be 1–32 visible characters");
    }
    write(&Settings {
        device_name: value.to_owned(),
    })
}

pub fn print(json: bool) -> Result<()> {
    let value = device_name();
    if json {
        println!(
            "{}",
            serde_json::to_string(&SettingsDocument {
                schema_version: crate::SCHEMA_VERSION,
                device_name: value,
            })?
        );
    } else {
        println!("Device name: {value}");
    }
    Ok(())
}

fn valid_device_name(value: &str) -> bool {
    let length = value.chars().count();
    (1..=32).contains(&length) && !value.chars().any(char::is_control)
}

fn read() -> Result<Settings> {
    let path = settings_path();
    if !path.exists() {
        return Ok(Settings::default());
    }
    serde_json::from_slice(&fs::read(&path)?)
        .with_context(|| format!("could not parse {}", path.display()))
}

fn write(settings: &Settings) -> Result<()> {
    let path = settings_path();
    let parent = path.parent().expect("settings path has a parent");
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let temporary = path.with_extension("json.tmp");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    serde_json::to_writer_pretty(&mut file, settings)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn settings_path() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
        })
        .join("unified-share/settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_human_device_names() {
        assert!(valid_device_name("Raj's Omarchy"));
        assert!(!valid_device_name(""));
        assert!(!valid_device_name("bad\nname"));
        assert!(!valid_device_name(&"x".repeat(33)));
    }
}
