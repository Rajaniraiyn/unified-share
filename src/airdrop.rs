use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct AirDropProbe {
    pub schema_version: u8,
    pub adapter: &'static str,
    pub state: &'static str,
    pub safe_to_start: bool,
    pub interface: Option<String>,
    pub driver: Option<String>,
    pub active_wifi_connection: bool,
    pub monitor_mode_advertised: bool,
    pub frame_injection_verified: bool,
    pub opendrop: Option<String>,
    pub owl: Option<String>,
    pub awdl_interface_ready: bool,
    pub internet_preserving_backend: bool,
    pub blocking_reasons: Vec<String>,
}

impl AirDropProbe {
    pub fn detect(path: &OsStr) -> Self {
        let wifi = wireless_interface();
        let iw = first_command(path, &["iw"]);
        let opendrop = first_command(path, &["opendrop"]);
        let owl = first_command(path, &["owl", "owl-run"]);
        let monitor_mode_advertised = iw
            .as_ref()
            .and_then(|command| Command::new(command).arg("phy").output().ok())
            .filter(|output| output.status.success())
            .is_some_and(|output| advertises_monitor_mode(&output.stdout));
        let awdl_interface_ready = awdl_interface_has_link_local_ipv6();

        // `iw phy` can advertise monitor mode, but it cannot prove that the driver
        // acknowledges injected frames. A disruptive live injection test is never
        // appropriate during status discovery, so this remains explicitly false.
        let frame_injection_verified = false;
        // Upstream OWL currently takes exclusive control of the selected Wi-Fi
        // interface. Do not imply that it preserves an existing AP connection.
        let internet_preserving_backend = false;
        let active_wifi_connection = wifi
            .as_ref()
            .is_some_and(|value| interface_has_carrier(&value.name));

        let mut blocking_reasons = Vec::new();
        if iw.is_none() {
            blocking_reasons.push("The iw hardware inspection tool is not installed.".into());
        }
        if opendrop.is_none() {
            blocking_reasons.push("OpenDrop is not installed.".into());
        }
        if owl.is_none() {
            blocking_reasons.push("OWL is not installed.".into());
        }
        if iw.is_some() && !monitor_mode_advertised {
            blocking_reasons.push("The Wi-Fi PHY does not advertise monitor mode.".into());
        }
        if !frame_injection_verified {
            blocking_reasons.push(
                "Active monitor-mode frame injection has not been verified for this adapter."
                    .into(),
            );
        }
        if !awdl_interface_ready {
            blocking_reasons.push(
                "No ready awdl0 interface with a link-local IPv6 address exists; Unified Share will not create one automatically."
                    .into(),
            );
        }
        if active_wifi_connection && !internet_preserving_backend {
            blocking_reasons.push(
                "Upstream OWL cannot preserve the current Wi-Fi AP connection while it owns this interface."
                    .into(),
            );
        }

        // The core may use an already prepared AWDL interface, but must never take
        // over the user's primary Wi-Fi device merely because a chooser was opened.
        let safe_to_start = opendrop.is_some() && owl.is_some() && awdl_interface_ready;

        Self {
            schema_version: crate::SCHEMA_VERSION,
            adapter: "airdrop",
            state: if safe_to_start {
                "experimental"
            } else {
                "unavailable"
            },
            safe_to_start,
            interface: wifi.as_ref().map(|value| value.name.clone()),
            driver: wifi.and_then(|value| value.driver),
            active_wifi_connection,
            monitor_mode_advertised,
            frame_injection_verified,
            opendrop,
            owl,
            awdl_interface_ready,
            internet_preserving_backend,
            blocking_reasons,
        }
    }

    pub fn summary(&self) -> String {
        if self.safe_to_start {
            return "An externally prepared AWDL link is available; OpenDrop remains experimental and requires a live Apple-device test.".into();
        }

        let device = match (&self.interface, &self.driver) {
            (Some(interface), Some(driver)) => format!("{interface} ({driver})"),
            (Some(interface), None) => interface.clone(),
            _ => "the detected Wi-Fi hardware".into(),
        };
        format!(
            "Native AirDrop is unavailable on {device}: {}",
            self.blocking_reasons
                .first()
                .map(String::as_str)
                .unwrap_or("the AWDL link is not ready")
        )
    }
}

pub fn print_probe(json: bool) -> Result<()> {
    let path = env::var_os("PATH").unwrap_or_default();
    let probe = AirDropProbe::detect(&path);
    if json {
        println!("{}", serde_json::to_string(&probe)?);
    } else {
        println!("AirDrop: {}", probe.state);
        println!("  Wi-Fi: {}", wifi_label(&probe));
        println!(
            "  Monitor mode advertised: {}",
            yes_no(probe.monitor_mode_advertised)
        );
        println!(
            "  Frame injection verified: {}",
            yes_no(probe.frame_injection_verified)
        );
        println!(
            "  Existing AWDL link ready: {}",
            yes_no(probe.awdl_interface_ready)
        );
        println!(
            "  Preserves current Wi-Fi: {}",
            yes_no(probe.internet_preserving_backend)
        );
        if !probe.blocking_reasons.is_empty() {
            println!("  Blocked by:");
            for reason in probe.blocking_reasons {
                println!("    - {reason}");
            }
        }
    }
    Ok(())
}

fn wifi_label(probe: &AirDropProbe) -> String {
    match (&probe.interface, &probe.driver) {
        (Some(interface), Some(driver)) => format!("{interface} · {driver}"),
        (Some(interface), None) => interface.clone(),
        _ => "not detected".into(),
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

#[derive(Debug)]
struct WirelessInterface {
    name: String,
    driver: Option<String>,
}

fn wireless_interface() -> Option<WirelessInterface> {
    let entries = fs::read_dir("/sys/class/net").ok()?;
    for entry in entries.flatten() {
        let interface = entry.path();
        if !interface.join("wireless").exists() {
            continue;
        }
        let driver = fs::read_link(interface.join("device/driver"))
            .ok()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            });
        return Some(WirelessInterface {
            name: entry.file_name().to_string_lossy().into_owned(),
            driver,
        });
    }
    None
}

fn interface_has_carrier(name: &str) -> bool {
    fs::read_to_string(PathBuf::from("/sys/class/net").join(name).join("carrier"))
        .is_ok_and(|value| value.trim() == "1")
}

fn awdl_interface_has_link_local_ipv6() -> bool {
    fs::read_to_string("/proc/net/if_inet6")
        .ok()
        .is_some_and(|contents| parse_awdl_ipv6(&contents))
}

fn parse_awdl_ipv6(contents: &str) -> bool {
    contents.lines().any(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        fields.len() == 6 && fields[0].starts_with("fe80") && fields[5] == "awdl0"
    })
}

fn advertises_monitor_mode(output: &[u8]) -> bool {
    String::from_utf8_lossy(output)
        .lines()
        .any(|line| line.trim() == "* monitor")
}

fn first_command(path: &OsStr, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        env::split_paths(path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
            .map(|candidate| candidate.to_string_lossy().into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_monitor_mode_in_iw_output() {
        let fixture = b"Supported interface modes:\n\t * managed\n\t * monitor\n";
        assert!(advertises_monitor_mode(fixture));
        assert!(!advertises_monitor_mode(
            b"Supported interface modes:\n * managed\n"
        ));
    }

    #[test]
    fn requires_link_local_ipv6_on_awdl_zero() {
        let fixture = concat!(
            "00000000000000000000000000000001 01 80 10 80 lo\n",
            "fe80000000000000123456789abcdef0 0a 40 20 80 awdl0\n",
        );
        assert!(parse_awdl_ipv6(fixture));
        assert!(!parse_awdl_ipv6(
            "20010000000000000000000000000001 0a 40 00 80 awdl0\n"
        ));
        assert!(!parse_awdl_ipv6(
            "fe80000000000000123456789abcdef0 0a 40 20 80 wlan0\n"
        ));
    }
}
