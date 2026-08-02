//! GUI-owned serial connector for the shared mode runner.
//!
//! A connector creates a new transport for every attempt.  The runner owns the
//! returned Session and drops it before retrying, so a failed ELM327 link is
//! never reused across reconnects.

use std::{env, time::Duration};

use async_trait::async_trait;
use obd2_core::{
    adapter::elm327::Elm327Adapter,
    session::Session,
    transport::{serial, LoggingTransport},
};
use obd2_dash::mode_runner::{ConnectError, NewSession, SessionConnector};

const DEFAULT_BAUD: u32 = 115_200;
const ADAPTER_SETTLE_DELAY: Duration = Duration::from_millis(500);

/// Opens the serial link used by the GUI's single [`obd2_dash::mode_runner::ModeRunner`].
///
/// `connect` intentionally does not cache a port or transport.  Reconnects
/// repeat port selection and open the device again, which releases stale OS
/// serial state after a cable, adapter, or vehicle-bus failure.
#[derive(Debug, Clone, Copy)]
pub struct SerialSessionConnector {
    baud: u32,
}

impl SerialSessionConnector {
    pub fn new(baud: u32) -> Self {
        Self { baud }
    }

    pub fn from_environment() -> Self {
        Self::new(configured_baud())
    }

    #[cfg(test)]
    pub fn baud(self) -> u32 {
        self.baud
    }
}

impl Default for SerialSessionConnector {
    fn default() -> Self {
        Self::from_environment()
    }
}

#[async_trait]
impl SessionConnector for SerialSessionConnector {
    type Adapter = Elm327Adapter;

    async fn connect(&self) -> Result<NewSession<Self::Adapter>, ConnectError> {
        let baud = self.baud;
        let (_port, transport) = tokio::task::spawn_blocking(move || {
            let port = select_port()?;
            let transport = serial::SerialTransport::new(&port, baud).map_err(|error| {
                ConnectError::Transport(format!("failed to open {port} at {baud} baud: {error}"))
            })?;
            Ok::<_, ConnectError>((port, transport))
        })
        .await
        .map_err(|error| {
            ConnectError::Transport(format!("serial connector worker terminated: {error}"))
        })??;

        // Preserve the adapter's post-open settle interval without blocking a
        // Tauri runtime worker.  Initialization itself remains runner-owned.
        tokio::time::sleep(ADAPTER_SETTLE_DELAY).await;

        let logging = LoggingTransport::new(transport);
        let adapter = Elm327Adapter::new(Box::new(logging));
        let mut session = Session::new(adapter);
        session.set_raw_capture_enabled(false);

        Ok(NewSession { session })
    }
}

fn select_port() -> Result<String, ConnectError> {
    let configured = env::var("OBD2_PORT").ok();
    let ports = serial::list_ports();
    select_port_from_candidates(configured.as_deref(), &ports).map_err(ConnectError::Transport)
}

fn select_port_from_candidates(
    configured: Option<&str>,
    ports: &[String],
) -> Result<String, String> {
    if let Some(port) = configured.map(str::trim).filter(|port| !port.is_empty()) {
        return Ok(port.to_owned());
    }

    let Some(first) = ports.first() else {
        return Err("no serial ports found; set OBD2_PORT=/dev/cu.usbserial-...".to_string());
    };

    Ok(ports
        .iter()
        .find(|port| is_likely_obd_port(port))
        .cloned()
        .unwrap_or_else(|| first.clone()))
}

fn is_likely_obd_port(port: &str) -> bool {
    let port = port.to_ascii_lowercase();
    port.contains("usbserial")
        || port.contains("usbmodem")
        || port.contains("ttyusb")
        || port.contains("slab_usbtouart")
        || port.contains("wchusbserial")
}

fn configured_baud() -> u32 {
    env::var("OBD2_BAUD")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_BAUD)
}

#[cfg(test)]
mod tests {
    use super::{select_port_from_candidates, SerialSessionConnector, DEFAULT_BAUD};

    #[test]
    fn configured_port_takes_precedence_and_is_trimmed() {
        let selected = select_port_from_candidates(
            Some("  /dev/tty.custom  "),
            &["/dev/cu.usbserial-1".to_string()],
        )
        .expect("configured port");
        assert_eq!(selected, "/dev/tty.custom");
    }

    #[test]
    fn likely_obd_port_beats_first_enumerated_port() {
        let selected = select_port_from_candidates(
            None,
            &[
                "/dev/cu.Bluetooth-Incoming-Port".to_string(),
                "/dev/cu.usbserial-1234".to_string(),
            ],
        )
        .expect("enumerated port");
        assert_eq!(selected, "/dev/cu.usbserial-1234");
    }

    #[test]
    fn no_port_reports_the_configuration_path() {
        let error = select_port_from_candidates(None, &[]).expect_err("no serial ports");
        assert!(error.contains("OBD2_PORT"));
    }

    #[test]
    fn connector_retains_requested_baud() {
        assert_eq!(SerialSessionConnector::new(38_400).baud(), 38_400);
        assert_eq!(DEFAULT_BAUD, 115_200);
    }
}
