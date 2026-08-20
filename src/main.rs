use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

const SCHEMA_VERSION: u8 = 1;

#[derive(Parser, Debug)]
#[command(name = "unified-share", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Report the sharing routes that this machine can actually use.
    Status {
        /// Emit the stable machine-readable contract used by desktop plugins.
        #[arg(long)]
        json: bool,
    },
    /// Share one or more files or folders using an available adapter.
    Share {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        #[arg(long, value_enum, default_value_t = AdapterChoice::Auto)]
        via: AdapterChoice,
        /// Validate and select an adapter without launching it.
        #[arg(long)]
        dry_run: bool,
        /// Emit a JSON result instead of prose.
        #[arg(long)]
        json: bool,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum AdapterChoice {
    Auto,
    QuickShare,
    Browser,
    Bluetooth,
    AirDrop,
    LocalSend,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AdapterState {
    Ready,
    Experimental,
    Planned,
    Unavailable,
    Unsupported,
}

#[derive(Clone, Debug, Serialize)]
struct AdapterStatus {
    id: &'static str,
    name: &'static str,
    state: AdapterState,
    native_targets: Vec<&'static str>,
    detail: String,
    backend: Option<String>,
}

#[derive(Debug, Serialize)]
struct StatusDocument {
    schema_version: u8,
    version: &'static str,
    adapters: Vec<AdapterStatus>,
}

#[derive(Debug, Serialize)]
struct ShareResult {
    schema_version: u8,
    ok: bool,
    adapter: &'static str,
    message: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Status { json } => print_status(json),
        Commands::Share {
            paths,
            via,
            dry_run,
            json,
        } => share(paths, via, dry_run, json),
    }
}

fn print_status(json: bool) -> Result<()> {
    let document = status_document();
    if json {
        println!("{}", serde_json::to_string(&document)?);
    } else {
        println!("Unified Share {}", document.version);
        for adapter in document.adapters {
            println!(
                "{:<12} {:<13} {}",
                adapter.id,
                state_label(&adapter.state),
                adapter.detail
            );
        }
    }
    Ok(())
}

fn status_document() -> StatusDocument {
    let path = env::var_os("PATH").unwrap_or_default();
    StatusDocument {
        schema_version: SCHEMA_VERSION,
        version: env!("CARGO_PKG_VERSION"),
        adapters: detect_adapters(&path),
    }
}

fn detect_adapters(path: &std::ffi::OsStr) -> Vec<AdapterStatus> {
    let quick_backend = first_command(path, &["packet", "r-quick-share", "rquickshare"]);
    let localsend = first_command(path, &["localsend"]);
    let bluetoothctl = first_command(path, &["bluetoothctl"]);
    let obex = first_command(path, &["bluetooth-sendto", "blueman-sendto", "obexctl"]);
    let opendrop = first_command(path, &["opendrop"]);
    let owl = first_command(path, &["owl", "owl-run"]);
    let wifi_driver = wifi_driver();

    let quick_share = if let Some(command) = quick_backend {
        AdapterStatus {
            id: "quick_share",
            name: "Quick Share",
            state: AdapterState::Experimental,
            native_targets: vec!["Android", "Windows Quick Share"],
            detail: "A compatible backend is installed; the stable core integration is the next milestone.".into(),
            backend: Some(command),
        }
    } else {
        AdapterStatus {
            id: "quick_share",
            name: "Quick Share",
            state: AdapterState::Unavailable,
            native_targets: vec!["Android", "Windows Quick Share"],
            detail: "No Quick Share backend is installed yet.".into(),
            backend: None,
        }
    };

    let browser = AdapterStatus {
        id: "browser",
        name: "Browser / QR",
        state: AdapterState::Planned,
        native_targets: vec!["iOS", "Android", "Windows", "macOS", "Linux"],
        detail: "No-install, tokenized LAN transfer is planned as the universal fallback.".into(),
        backend: first_command(path, &["qrencode"]),
    };

    let bluetooth = match (bluetoothctl, obex) {
        (Some(_), Some(sender)) => AdapterStatus {
            id: "bluetooth",
            name: "Bluetooth OBEX",
            state: AdapterState::Experimental,
            native_targets: vec!["Android", "Windows", "Linux", "macOS"],
            detail: "BlueZ and an OBEX sender are present; adapter integration is not enabled yet."
                .into(),
            backend: Some(sender),
        },
        (Some(_), None) => AdapterStatus {
            id: "bluetooth",
            name: "Bluetooth OBEX",
            state: AdapterState::Unavailable,
            native_targets: vec!["Android", "Windows", "Linux", "macOS"],
            detail: "Bluetooth is present, but no OBEX file-transfer backend is installed.".into(),
            backend: None,
        },
        _ => AdapterStatus {
            id: "bluetooth",
            name: "Bluetooth OBEX",
            state: AdapterState::Unavailable,
            native_targets: vec!["Android", "Windows", "Linux", "macOS"],
            detail: "No BlueZ control tool was found.".into(),
            backend: None,
        },
    };

    let airdrop_ready = opendrop.is_some() && owl.is_some();
    let airdrop = if airdrop_ready {
        AdapterStatus {
            id: "airdrop",
            name: "AirDrop",
            state: AdapterState::Experimental,
            native_targets: vec!["iOS", "macOS"],
            detail: format!(
                "OpenDrop and OWL were found; compatibility still depends on Wi-Fi monitor mode (driver: {}).",
                wifi_driver.as_deref().unwrap_or("unknown")
            ),
            backend: opendrop,
        }
    } else {
        AdapterStatus {
            id: "airdrop",
            name: "AirDrop",
            state: AdapterState::Unsupported,
            native_targets: vec!["iOS", "macOS"],
            detail: format!(
                "Native AirDrop needs OpenDrop, OWL, and proven AWDL-capable monitor mode; this machine uses {}.",
                wifi_driver.as_deref().unwrap_or("an unknown Wi-Fi driver")
            ),
            backend: None,
        }
    };

    let localsend = match localsend {
        Some(command) => AdapterStatus {
            id: "localsend",
            name: "LocalSend",
            state: AdapterState::Ready,
            native_targets: vec!["LocalSend peers"],
            detail: "Installed and retained as the working fallback during migration.".into(),
            backend: Some(command),
        },
        None => AdapterStatus {
            id: "localsend",
            name: "LocalSend",
            state: AdapterState::Unavailable,
            native_targets: vec!["LocalSend peers"],
            detail: "Not installed.".into(),
            backend: None,
        },
    };

    vec![quick_share, browser, bluetooth, airdrop, localsend]
}

fn share(paths: Vec<PathBuf>, via: AdapterChoice, dry_run: bool, json: bool) -> Result<()> {
    let paths = validate_paths(paths)?;
    let selected = match via {
        AdapterChoice::Auto | AdapterChoice::LocalSend => "localsend",
        AdapterChoice::QuickShare => {
            bail!("Quick Share is not wired into the stable adapter contract yet")
        }
        AdapterChoice::Browser => bail!("Browser / QR sharing is planned but not implemented yet"),
        AdapterChoice::Bluetooth => bail!("Bluetooth OBEX sharing is not implemented yet"),
        AdapterChoice::AirDrop => bail!("AirDrop is unsupported on this machine's current stack"),
    };

    let path = env::var_os("PATH").unwrap_or_default();
    if first_command(&path, &["localsend"]).is_none() {
        bail!("LocalSend is not installed and no stable replacement adapter is ready");
    }

    if !dry_run {
        let mut command = Command::new("systemd-run");
        command.args([
            "--user",
            "--quiet",
            "--collect",
            "localsend",
            "--headless",
            "send",
        ]);
        command.args(&paths);
        let status = command
            .status()
            .context("failed to launch LocalSend through systemd-run")?;
        if !status.success() {
            bail!("LocalSend launcher exited with {status}");
        }
    }

    let result = ShareResult {
        schema_version: SCHEMA_VERSION,
        ok: true,
        adapter: selected,
        message: if dry_run {
            format!("Would share {} item(s) with LocalSend", paths.len())
        } else {
            format!("Opened LocalSend for {} item(s)", paths.len())
        },
    };
    if json {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        println!("{}", result.message);
    }
    Ok(())
}

fn validate_paths(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    paths
        .into_iter()
        .map(|path| {
            if !path.exists() {
                return Err(anyhow!("path does not exist: {}", path.display()));
            }
            fs::canonicalize(&path).with_context(|| format!("could not resolve {}", path.display()))
        })
        .collect()
}

fn first_command(path: &std::ffi::OsStr, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find(|name| command_exists_in(path, name))
        .map(|name| (*name).to_owned())
}

fn command_exists_in(path: &std::ffi::OsStr, name: &str) -> bool {
    env::split_paths(path).any(|directory| {
        let candidate = directory.join(name);
        candidate.is_file()
    })
}

fn wifi_driver() -> Option<String> {
    let entries = fs::read_dir("/sys/class/net").ok()?;
    for entry in entries.flatten() {
        let interface = entry.path();
        if !interface.join("wireless").exists() {
            continue;
        }
        let driver = fs::read_link(interface.join("device/driver")).ok()?;
        return driver
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
    }
    None
}

fn state_label(state: &AdapterState) -> &'static str {
    match state {
        AdapterState::Ready => "ready",
        AdapterState::Experimental => "experimental",
        AdapterState::Planned => "planned",
        AdapterState::Unavailable => "unavailable",
        AdapterState::Unsupported => "unsupported",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn finds_commands_in_an_explicit_path() {
        let path = std::ffi::OsStr::new("/definitely/missing:/usr/bin");
        assert!(command_exists_in(path, "sh"));
        assert!(!command_exists_in(path, "definitely-not-a-real-command"));
    }

    #[test]
    fn status_contract_has_stable_schema_and_order() {
        let document = status_document();
        assert_eq!(document.schema_version, 1);
        let ids: Vec<_> = document.adapters.iter().map(|adapter| adapter.id).collect();
        assert_eq!(
            ids,
            [
                "quick_share",
                "browser",
                "bluetooth",
                "airdrop",
                "localsend"
            ]
        );
    }

    #[test]
    fn rejects_missing_share_paths() {
        let result = validate_paths(vec![Path::new("/definitely/missing/file").into()]);
        assert!(result.is_err());
    }
}
