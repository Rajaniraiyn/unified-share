use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rqs_lib::channel::{Message, TransferKind};
use rqs_lib::{EndpointInfo, OutboundPayload, RQS, SendInfo, TransferState, Visibility};
use serde::Serialize;
use tokio::runtime::Runtime;
use tokio::sync::broadcast;
use tokio::time::{Instant, timeout_at};

use crate::SCHEMA_VERSION;

#[derive(Debug, Serialize)]
struct DiscoveryDocument {
    schema_version: u8,
    adapter: &'static str,
    devices: Vec<Device>,
}

#[derive(Clone, Debug, Serialize)]
struct Device {
    id: String,
    name: String,
    device_type: String,
    address: String,
}

#[derive(Debug, Serialize)]
struct TransferDocument {
    schema_version: u8,
    ok: bool,
    adapter: &'static str,
    device_id: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pin: Option<String>,
}

pub fn discover(timeout_seconds: u64, json: bool) -> Result<()> {
    Runtime::new()?.block_on(discover_async(timeout_seconds, json))
}

async fn discover_async(timeout_seconds: u64, json: bool) -> Result<()> {
    let mut service = RQS::new(Visibility::Invisible, None, None, Some(host_name()));
    service.run().await.context("could not start Quick Share")?;
    let (sender, mut receiver) = broadcast::channel::<EndpointInfo>(32);
    service
        .discovery(sender)
        .context("could not start Quick Share discovery")?;

    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    let mut devices = BTreeMap::new();
    loop {
        match timeout_at(deadline, receiver.recv()).await {
            Ok(Ok(endpoint)) if endpoint.present == Some(true) => {
                if let Some(device) = endpoint_to_device(endpoint) {
                    devices.insert(device.id.clone(), device);
                }
            }
            Ok(Ok(endpoint)) => {
                devices.remove(&endpoint.id);
            }
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => break,
        }
    }
    service.stop().await;

    let document = DiscoveryDocument {
        schema_version: SCHEMA_VERSION,
        adapter: "quick_share",
        devices: devices.into_values().collect(),
    };
    if json {
        println!("{}", serde_json::to_string(&document)?);
    } else if document.devices.is_empty() {
        println!("No Quick Share devices found");
    } else {
        for device in document.devices {
            println!("{}\t{}\t{}", device.id, device.name, device.device_type);
        }
    }
    Ok(())
}

pub fn send(
    paths: &[PathBuf],
    device_id: &str,
    device_name: &str,
    timeout_seconds: u64,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    for path in paths {
        if !path.metadata()?.is_file() {
            bail!(
                "Quick Share currently accepts regular files only: {}",
                path.display()
            );
        }
    }
    if dry_run {
        return print_transfer(
            TransferDocument {
                schema_version: SCHEMA_VERSION,
                ok: true,
                adapter: "quick_share",
                device_id: device_id.to_owned(),
                message: format!("Would send {} file(s) to {device_name}", paths.len()),
                pin: None,
            },
            json,
        );
    }
    Runtime::new()?.block_on(send_async(
        paths,
        device_id,
        device_name,
        timeout_seconds,
        json,
    ))
}

async fn send_async(
    paths: &[PathBuf],
    device_id: &str,
    device_name: &str,
    timeout_seconds: u64,
    json: bool,
) -> Result<()> {
    let files = paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect();
    let mut service = RQS::new(Visibility::Invisible, None, None, Some(host_name()));
    let (sender, _) = service.run().await.context("could not start Quick Share")?;
    let mut events = service.message_sender.subscribe();
    sender
        .send(SendInfo {
            id: device_id.to_owned(),
            name: device_name.to_owned(),
            addr: device_id.to_owned(),
            ob: OutboundPayload::Files(files),
        })
        .await
        .context("Quick Share transfer worker stopped")?;

    let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
    let mut pin = None;
    let result = loop {
        let event = match timeout_at(deadline, events.recv()).await {
            Ok(Ok(event)) => event,
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(broadcast::error::RecvError::Closed)) => {
                break Err(anyhow!("Quick Share event stream closed"));
            }
            Err(_) => {
                break Err(anyhow!(
                    "Quick Share timed out waiting for consent or transfer"
                ));
            }
        };
        if event.id != device_id {
            continue;
        }
        let Message::Client(client) = event.msg else {
            continue;
        };
        if client.kind != TransferKind::Outbound {
            continue;
        }
        if let Some(metadata) = client.metadata
            && metadata.pin_code.is_some()
            && metadata.pin_code != pin
        {
            pin = metadata.pin_code;
            if let Some(value) = &pin {
                eprintln!("Quick Share confirmation code: {value}");
            }
        }
        match client.state {
            Some(TransferState::Finished) => break Ok(()),
            Some(TransferState::Rejected) => break Err(anyhow!("Quick Share was rejected")),
            Some(TransferState::Cancelled) => break Err(anyhow!("Quick Share was cancelled")),
            Some(TransferState::Disconnected) => {
                break Err(anyhow!("Quick Share device disconnected"));
            }
            _ => {}
        }
    };
    service.stop().await;
    result?;

    print_transfer(
        TransferDocument {
            schema_version: SCHEMA_VERSION,
            ok: true,
            adapter: "quick_share",
            device_id: device_id.to_owned(),
            message: format!("Sent {} file(s) to {device_name}", paths.len()),
            pin,
        },
        json,
    )
}

fn endpoint_to_device(endpoint: EndpointInfo) -> Option<Device> {
    let address = format!("{}:{}", endpoint.ip?, endpoint.port?);
    Some(Device {
        id: endpoint.id,
        name: endpoint
            .name
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "Unknown device".into()),
        device_type: format!(
            "{:?}",
            endpoint.rtype.unwrap_or(rqs_lib::DeviceType::Unknown)
        )
        .to_lowercase(),
        address,
    })
}

fn print_transfer(document: TransferDocument, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(&document)?);
    } else {
        println!("{}", document.message);
    }
    Ok(())
}

fn host_name() -> String {
    crate::settings::device_name()
}
