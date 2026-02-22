use std::time::Duration;

use async_trait::async_trait;
use btleplug::api::{
    Central, CentralEvent, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Manager, Peripheral};
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::time::timeout;

use super::transport::Transport;
use super::types::Obd2Error;

// ── GATT UUIDs ──────────────────────────────────────────────────────────────

/// OBDLink/STN custom UART service (FFF0).
#[allow(dead_code)]
const STN_SERVICE: uuid::Uuid = uuid::Uuid::from_u128(0x0000FFF0_0000_1000_8000_00805f9b34fb);
const STN_NOTIFY: uuid::Uuid = uuid::Uuid::from_u128(0x0000FFF1_0000_1000_8000_00805f9b34fb);
const STN_WRITE: uuid::Uuid = uuid::Uuid::from_u128(0x0000FFF2_0000_1000_8000_00805f9b34fb);

/// Nordic UART Service (NUS) — used by some generic ELM327 BLE adapters.
#[allow(dead_code)]
const NUS_SERVICE: uuid::Uuid = uuid::Uuid::from_u128(0x6E400001_b5a3_f393_e0a9_e50e24dcca9e);
const NUS_TX: uuid::Uuid = uuid::Uuid::from_u128(0x6E400003_b5a3_f393_e0a9_e50e24dcca9e); // notify
const NUS_RX: uuid::Uuid = uuid::Uuid::from_u128(0x6E400002_b5a3_f393_e0a9_e50e24dcca9e); // write

/// Known name prefixes for OBD2 BLE adapters.
const ADAPTER_NAME_PATTERNS: &[&str] = &["OBDLink", "OBD", "ELM327", "STN", "OBDII", "Vgate"];

/// Read timeout for BLE responses.
const BLE_READ_TIMEOUT: Duration = Duration::from_secs(5);

// ── Scanning ────────────────────────────────────────────────────────────────

/// Scan for a BLE OBD2 adapter. If `name_filter` is Some, only match peripherals
/// whose name contains that string (case-insensitive). Returns the first match.
pub async fn scan_for_adapter(
    name_filter: Option<&str>,
    scan_timeout: Duration,
) -> anyhow::Result<Peripheral> {
    let manager = Manager::new().await?;
    let adapters = manager.adapters().await?;
    let adapter = adapters
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("no Bluetooth adapters found"))?;

    // Subscribe to discovery events
    let mut events = adapter.events().await?;

    adapter.start_scan(ScanFilter::default()).await?;

    let found = timeout(scan_timeout, async {
        while let Some(event) = events.next().await {
            if let CentralEvent::DeviceDiscovered(id) = event {
                if let Ok(peripheral) = adapter.peripheral(&id).await {
                    if let Ok(Some(props)) = peripheral.properties().await {
                        let name = props.local_name.unwrap_or_default();
                        if !name.is_empty() {
                            tracing::info!("BLE device found: {} ({})", name, id);

                            if is_adapter_match(&name, name_filter) {
                                adapter.stop_scan().await.ok();
                                return Ok(peripheral);
                            }
                        }
                    }
                }
            }
        }
        Err(anyhow::anyhow!("BLE event stream ended"))
    })
    .await;

    adapter.stop_scan().await.ok();

    match found {
        Ok(result) => result,
        Err(_) => {
            // Timeout — check already-discovered peripherals as fallback
            let peripherals = adapter.peripherals().await?;
            for peripheral in peripherals {
                if let Ok(Some(props)) = peripheral.properties().await {
                    let name = props.local_name.unwrap_or_default();
                    if !name.is_empty() && is_adapter_match(&name, name_filter) {
                        return Ok(peripheral);
                    }
                }
            }
            anyhow::bail!(
                "no BLE OBD2 adapter found within {}s. Use --ble-name to specify the device name.",
                scan_timeout.as_secs()
            );
        }
    }
}

/// Check if a device name matches our adapter filter.
fn is_adapter_match(name: &str, name_filter: Option<&str>) -> bool {
    let lower = name.to_lowercase();
    if let Some(filter) = name_filter {
        lower.contains(&filter.to_lowercase())
    } else {
        ADAPTER_NAME_PATTERNS
            .iter()
            .any(|p| lower.contains(&p.to_lowercase()))
    }
}

// ── GATT Characteristic Discovery ───────────────────────────────────────────

struct UartChars {
    write_char: Characteristic,
    notify_char: Characteristic,
}

/// Discover the UART write+notify characteristics using a 3-tier strategy:
/// 1. OBDLink/STN UUIDs (FFF0/FFF1/FFF2)
/// 2. Nordic UART Service (NUS)
/// 3. Heuristic: any writable + notifiable pair
fn find_uart_characteristics(chars: &[Characteristic]) -> Result<UartChars, Obd2Error> {
    // Tier 1: STN/OBDLink
    let stn_write = chars.iter().find(|c| c.uuid == STN_WRITE);
    let stn_notify = chars.iter().find(|c| c.uuid == STN_NOTIFY);
    if let (Some(w), Some(n)) = (stn_write, stn_notify) {
        tracing::info!("Using OBDLink/STN GATT UUIDs (FFF0 service)");
        return Ok(UartChars {
            write_char: w.clone(),
            notify_char: n.clone(),
        });
    }

    // Tier 2: Nordic UART
    let nus_write = chars.iter().find(|c| c.uuid == NUS_RX);
    let nus_notify = chars.iter().find(|c| c.uuid == NUS_TX);
    if let (Some(w), Some(n)) = (nus_write, nus_notify) {
        tracing::info!("Using Nordic UART Service (NUS) UUIDs");
        return Ok(UartChars {
            write_char: w.clone(),
            notify_char: n.clone(),
        });
    }

    // Tier 3: heuristic — find any writable + notifiable pair in same service
    use btleplug::api::CharPropFlags;
    let writable = chars.iter().find(|c| {
        c.properties.contains(CharPropFlags::WRITE)
            || c.properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
    });
    let notifiable = chars.iter().find(|c| {
        c.properties.contains(CharPropFlags::NOTIFY) || c.properties.contains(CharPropFlags::INDICATE)
    });

    if let (Some(w), Some(n)) = (writable, notifiable) {
        tracing::warn!(
            "Using heuristic GATT characteristics: write={}, notify={}",
            w.uuid,
            n.uuid
        );
        return Ok(UartChars {
            write_char: w.clone(),
            notify_char: n.clone(),
        });
    }

    Err(Obd2Error::Ble(
        "no suitable UART characteristics found on BLE device".into(),
    ))
}

// ── BleTransport ────────────────────────────────────────────────────────────

/// BLE GATT transport for ELM327/STN adapters.
pub struct BleTransport {
    peripheral: Peripheral,
    write_char: Characteristic,
    /// Buffered bytes received from BLE notifications.
    rx: mpsc::Receiver<Vec<u8>>,
    /// Leftover bytes from previous reads that haven't formed a complete line yet.
    buf: Vec<u8>,
    /// Background notification listener handle.
    _listener: tokio::task::JoinHandle<()>,
}

impl BleTransport {
    /// Connect to a BLE peripheral, discover UART characteristics, and subscribe
    /// to notifications. Returns a ready-to-use transport.
    pub async fn connect(peripheral: Peripheral) -> Result<Self, Obd2Error> {
        peripheral
            .connect()
            .await
            .map_err(|e| Obd2Error::Ble(format!("connect failed: {e}")))?;

        peripheral
            .discover_services()
            .await
            .map_err(|e| Obd2Error::Ble(format!("service discovery failed: {e}")))?;

        let chars: Vec<Characteristic> = peripheral
            .services()
            .iter()
            .flat_map(|s| s.characteristics.clone())
            .collect();

        tracing::debug!(
            "BLE services discovered: {} characteristics total",
            chars.len()
        );
        for c in &chars {
            tracing::debug!("  char {} props={:?} service={}", c.uuid, c.properties, c.service_uuid);
        }

        let uart = find_uart_characteristics(&chars)?;

        // Subscribe to notifications on the notify characteristic
        peripheral
            .subscribe(&uart.notify_char)
            .await
            .map_err(|e| Obd2Error::Ble(format!("subscribe failed: {e}")))?;

        // Spawn background task to forward notifications into an mpsc channel
        let (tx, rx) = mpsc::channel::<Vec<u8>>(256);
        let notif_peripheral = peripheral.clone();
        let listener = tokio::spawn(async move {
            let Ok(mut stream) = notif_peripheral.notifications().await else {
                return;
            };
            while let Some(notif) = stream.next().await {
                if tx.send(notif.value).await.is_err() {
                    break; // receiver dropped
                }
            }
        });

        let name = peripheral
            .properties()
            .await
            .ok()
            .flatten()
            .and_then(|p| p.local_name)
            .unwrap_or_else(|| "unknown".into());
        tracing::info!("BLE transport connected to: {}", name);

        Ok(Self {
            peripheral,
            write_char: uart.write_char,
            rx,
            buf: Vec::with_capacity(512),
            _listener: listener,
        })
    }
}

#[async_trait]
impl Transport for BleTransport {
    async fn send(&mut self, data: &[u8]) -> Result<(), Obd2Error> {
        // BLE has a max write size (typically 20 bytes for BLE 4.x, 512 for 5.x).
        // Chunk if necessary, though most OBD commands are short.
        for chunk in data.chunks(20) {
            self.peripheral
                .write(&self.write_char, chunk, WriteType::WithResponse)
                .await
                .map_err(|e| Obd2Error::Ble(format!("write failed: {e}")))?;
        }
        Ok(())
    }

    async fn read_line(&mut self) -> Result<String, Obd2Error> {
        // Check if we already have a complete line in the buffer
        if let Some(line) = extract_line(&mut self.buf) {
            return Ok(line);
        }

        // Read more data from BLE notifications until we get a line
        loop {
            let chunk = timeout(BLE_READ_TIMEOUT, self.rx.recv())
                .await
                .map_err(|_| Obd2Error::Timeout)?
                .ok_or(Obd2Error::Ble("notification stream ended".into()))?;

            self.buf.extend_from_slice(&chunk);

            if let Some(line) = extract_line(&mut self.buf) {
                return Ok(line);
            }
        }
    }
}

impl Drop for BleTransport {
    fn drop(&mut self) {
        let peripheral = self.peripheral.clone();
        // Fire-and-forget disconnect
        tokio::spawn(async move {
            if let Err(e) = peripheral.disconnect().await {
                tracing::warn!("BLE disconnect error: {e}");
            }
        });
    }
}

/// Try to extract one line from the buffer. A "line" ends at `\r`, `\n`, or the
/// `>` prompt character. Returns the line content (without delimiter) and drains
/// the consumed bytes from `buf`.
fn extract_line(buf: &mut Vec<u8>) -> Option<String> {
    for (i, &byte) in buf.iter().enumerate() {
        match byte {
            b'\r' | b'\n' => {
                let line: Vec<u8> = buf.drain(..=i).collect();
                // Skip any consecutive \r\n
                while buf.first() == Some(&b'\r') || buf.first() == Some(&b'\n') {
                    buf.remove(0);
                }
                let s = String::from_utf8_lossy(&line[..line.len() - 1]).to_string();
                if s.trim().is_empty() {
                    // Skip empty lines, try again
                    return extract_line(buf);
                }
                return Some(s);
            }
            b'>' => {
                // The `>` prompt — return it as its own "line" so read_until_prompt sees it
                let line: Vec<u8> = buf.drain(..=i).collect();
                let s = String::from_utf8_lossy(&line).to_string();
                return Some(s);
            }
            _ => {}
        }
    }
    None
}
