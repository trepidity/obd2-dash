use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Known name prefixes for OBD2 BLE adapters (mirrors ble_transport.rs).
const ADAPTER_NAME_PATTERNS: &[&str] = &["OBDLink", "OBD", "ELM327", "STN", "OBDII", "Vgate"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeviceKind {
    Serial { port_path: String, baud: u32 },
    Ble { name: String },
}

#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub kind: DeviceKind,
    pub display_name: String,
    pub detail: Option<String>,
}

/// Events emitted by the scanner (decoupled from app::Message).
#[derive(Debug)]
pub enum ScanEvent {
    DeviceFound(DiscoveredDevice),
    ScanComplete,
}

/// Spawn a background scan that discovers serial ports and BLE adapters,
/// sending each as `ScanEvent::DeviceFound`. Sends `ScanEvent::ScanComplete`
/// when both sources finish. Abort the returned handle to cancel.
pub fn spawn_scan(
    tx: mpsc::UnboundedSender<ScanEvent>,
    default_baud: u32,
    ble_timeout: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        // ── Serial scan (synchronous, immediate) ─────────────────────
        if let Ok(ports) = serialport::available_ports() {
            for port in ports {
                let detail = match &port.port_type {
                    serialport::SerialPortType::UsbPort(info) => {
                        info.product.clone()
                    }
                    serialport::SerialPortType::BluetoothPort => {
                        Some("Bluetooth".into())
                    }
                    _ => None,
                };
                let display_name = if let Some(ref d) = detail {
                    format!("{} ({})", port.port_name, d)
                } else {
                    port.port_name.clone()
                };
                let dev = DiscoveredDevice {
                    kind: DeviceKind::Serial {
                        port_path: port.port_name.clone(),
                        baud: default_baud,
                    },
                    display_name,
                    detail,
                };
                if tx.send(ScanEvent::DeviceFound(dev)).is_err() {
                    return;
                }
            }
        }

        // ── BLE scan (async, up to ble_timeout) ─────────────────────
        if let Err(e) = scan_ble(&tx, ble_timeout).await {
            tracing::debug!("BLE scan ended: {}", e);
        }

        let _ = tx.send(ScanEvent::ScanComplete);
    })
}

async fn scan_ble(
    tx: &mpsc::UnboundedSender<ScanEvent>,
    scan_timeout: Duration,
) -> anyhow::Result<()> {
    use btleplug::api::{Central, CentralEvent, Manager as _, Peripheral as _, ScanFilter};
    use btleplug::platform::Manager;
    use futures::StreamExt;

    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no Bluetooth adapters found"))?;

    let mut events = adapter.events().await?;
    adapter.start_scan(ScanFilter::default()).await?;

    let _ = tokio::time::timeout(scan_timeout, async {
        while let Some(event) = events.next().await {
            if let CentralEvent::DeviceDiscovered(id) = event {
                if let Ok(peripheral) = adapter.peripheral(&id).await {
                    if let Ok(Some(props)) = peripheral.properties().await {
                        if let Some(name) = props.local_name {
                            if !name.is_empty() && is_ble_adapter(&name) {
                                let dev = DiscoveredDevice {
                                    kind: DeviceKind::Ble { name: name.clone() },
                                    display_name: name,
                                    detail: Some("BLE".into()),
                                };
                                if tx.send(ScanEvent::DeviceFound(dev)).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }
    })
    .await;

    adapter.stop_scan().await.ok();
    Ok(())
}

fn is_ble_adapter(name: &str) -> bool {
    let lower = name.to_lowercase();
    ADAPTER_NAME_PATTERNS
        .iter()
        .any(|p| lower.contains(&p.to_lowercase()))
}
