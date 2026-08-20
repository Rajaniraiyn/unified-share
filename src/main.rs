use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;

mod browser;
mod history;
mod quick_share;
mod settings;

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
    /// Discover Android and Windows Quick Share devices for a bounded time.
    Discover {
        /// Number of seconds to listen for nearby Quick Share devices.
        #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u64).range(1..=60))]
        timeout_seconds: u64,
        /// Emit the stable machine-readable device list.
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
        /// Lifetime of a Browser / QR link in seconds.
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(30..=86400))]
        timeout_seconds: u64,
        /// Quick Share device id returned by `discover`.
        #[arg(long)]
        device: Option<String>,
        /// Friendly Quick Share device name shown during consent.
        #[arg(long)]
        device_name: Option<String>,
        /// Maximum time to wait for Quick Share consent and transfer.
        #[arg(long, default_value_t = 120, value_parser = clap::value_parser!(u64).range(10..=3600))]
        transfer_timeout_seconds: u64,
    },
    /// Internal entry point for the isolated Browser / QR server.
    #[command(hide = true)]
    BrowserServe {
        #[arg(long)]
        state: PathBuf,
        #[arg(long)]
        ready: PathBuf,
    },
    /// Stop an active Browser / QR transfer before it expires.
    Stop {
        transfer_id: String,
        /// Emit a JSON result instead of prose.
        #[arg(long)]
        json: bool,
    },
    /// Show or clear the private on-device transfer history.
    History {
        /// Remove all saved history entries.
        #[arg(long)]
        clear: bool,
        /// Maximum number of newest entries to return.
        #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=200))]
        limit: u64,
        /// Emit the stable machine-readable history contract.
        #[arg(long)]
        json: bool,
    },
    /// Read or change how this computer appears to nearby devices.
    Config {
        /// Set the Quick Share sender/receiver display name.
        #[arg(long)]
        device_name: Option<String>,
        /// Emit the stable machine-readable settings contract.
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

impl AdapterChoice {
    fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::QuickShare => "quick_share",
            Self::Browser => "browser",
            Self::Bluetooth => "bluetooth",
            Self::AirDrop => "airdrop",
            Self::LocalSend => "localsend",
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AdapterState {
    Ready,
    Experimental,
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
    device_name: String,
    adapters: Vec<AdapterStatus>,
}

#[derive(Debug, Serialize)]
struct ShareResult {
    schema_version: u8,
    ok: bool,
    adapter: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at_unix: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transfer_id: Option<String>,
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
        Commands::Discover {
            timeout_seconds,
            json,
        } => quick_share::discover(timeout_seconds, json),
        Commands::Share {
            paths,
            via,
            dry_run,
            json,
            timeout_seconds,
            device,
            device_name,
            transfer_timeout_seconds,
        } => share(
            paths,
            ShareOptions {
                via,
                dry_run,
                json,
                timeout_seconds,
                device,
                device_name,
                transfer_timeout_seconds,
            },
        ),
        Commands::BrowserServe { state, ready } => browser::serve_from_state(&state, &ready),
        Commands::Stop { transfer_id, json } => {
            browser::stop(&transfer_id)?;
            print_share_result(
                ShareResult {
                    schema_version: SCHEMA_VERSION,
                    ok: true,
                    adapter: "browser",
                    message: format!("Stopped Browser / QR transfer {transfer_id}"),
                    url: None,
                    expires_at_unix: None,
                    transfer_id: Some(transfer_id),
                },
                json,
            )
        }
        Commands::History { clear, limit, json } => {
            if clear {
                history::clear()?;
            }
            history::print(limit as usize, json)
        }
        Commands::Config { device_name, json } => {
            if let Some(value) = device_name {
                settings::set_device_name(&value)?;
            }
            settings::print(json)
        }
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
        device_name: settings::device_name(),
        adapters: detect_adapters(&path),
    }
}

fn detect_adapters(path: &std::ffi::OsStr) -> Vec<AdapterStatus> {
    let localsend = first_command(path, &["localsend"]);
    let bluetoothctl = first_command(path, &["bluetoothctl"]);
    let obex = first_command(path, &["bluetooth-sendto", "blueman-sendto", "obexctl"]);
    let opendrop = first_command(path, &["opendrop"]);
    let owl = first_command(path, &["owl", "owl-run"]);
    let wifi_driver = wifi_driver();

    let quick_share = AdapterStatus {
        id: "quick_share",
        name: "Quick Share",
        state: AdapterState::Experimental,
        native_targets: vec!["Android", "Windows Quick Share"],
        detail: "The native on-demand rqs engine is installed; live device interoperability still needs verification.".into(),
        backend: Some("native-rqs".into()),
    };

    let browser = match browser::availability() {
        Ok(address) => AdapterStatus {
            id: "browser",
            name: "Browser / QR",
            state: AdapterState::Ready,
            native_targets: vec!["iOS", "Android", "Windows", "macOS", "Linux"],
            detail: format!(
                "Native expiring LAN links are ready on {address}; recipients only need a browser."
            ),
            backend: Some("native".into()),
        },
        Err(detail) => AdapterStatus {
            id: "browser",
            name: "Browser / QR",
            state: AdapterState::Unavailable,
            native_targets: vec!["iOS", "Android", "Windows", "macOS", "Linux"],
            detail,
            backend: None,
        },
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

struct ShareOptions {
    via: AdapterChoice,
    dry_run: bool,
    json: bool,
    timeout_seconds: u64,
    device: Option<String>,
    device_name: Option<String>,
    transfer_timeout_seconds: u64,
}

fn share(paths: Vec<PathBuf>, options: ShareOptions) -> Result<()> {
    let route = options.via.id();
    let target = options
        .device_name
        .clone()
        .unwrap_or_else(|| route.replace('_', " "));
    let item_count = paths.len();
    let dry_run = options.dry_run;
    let result = share_inner(paths, options);
    if !dry_run {
        match &result {
            Ok(()) => history::record(
                route,
                &target,
                item_count,
                "completed",
                "Transfer completed",
            ),
            Err(_) => history::record(route, &target, item_count, "failed", "Transfer failed"),
        }
    }
    result
}

fn share_inner(paths: Vec<PathBuf>, options: ShareOptions) -> Result<()> {
    let paths = validate_paths(paths)?;
    let selected = match options.via {
        AdapterChoice::Auto | AdapterChoice::LocalSend => "localsend",
        AdapterChoice::QuickShare => "quick_share",
        AdapterChoice::Browser => "browser",
        AdapterChoice::Bluetooth => bail!("Bluetooth OBEX sharing is not implemented yet"),
        AdapterChoice::AirDrop => bail!("AirDrop is unsupported on this machine's current stack"),
    };

    if selected == "quick_share" {
        let device = options
            .device
            .context("Quick Share needs --device DEVICE_ID from `unified-share discover --json`")?;
        return quick_share::send(
            &paths,
            &device,
            options
                .device_name
                .as_deref()
                .unwrap_or("Quick Share device"),
            options.transfer_timeout_seconds,
            options.dry_run,
            options.json,
        );
    }

    if selected == "browser" {
        let launch = browser::launch(&paths, options.timeout_seconds, options.dry_run)?;
        let result = ShareResult {
            schema_version: SCHEMA_VERSION,
            ok: true,
            adapter: selected,
            message: if options.dry_run {
                format!(
                    "Would create an expiring browser link for {} file(s)",
                    paths.len()
                )
            } else {
                format!("Browser link ready for {} file(s)", paths.len())
            },
            url: launch.as_ref().map(|value| value.url.clone()),
            expires_at_unix: launch.as_ref().map(|value| value.expires_at_unix),
            transfer_id: launch.map(|value| value.transfer_id),
        };
        return print_share_result(result, options.json);
    }

    let path = env::var_os("PATH").unwrap_or_default();
    if first_command(&path, &["localsend"]).is_none() {
        bail!("LocalSend is not installed and no stable replacement adapter is ready");
    }

    if !options.dry_run {
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
        message: if options.dry_run {
            format!("Would share {} item(s) with LocalSend", paths.len())
        } else {
            format!("Opened LocalSend for {} item(s)", paths.len())
        },
        url: None,
        expires_at_unix: None,
        transfer_id: None,
    };
    print_share_result(result, options.json)
}

fn print_share_result(result: ShareResult, json: bool) -> Result<()> {
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
