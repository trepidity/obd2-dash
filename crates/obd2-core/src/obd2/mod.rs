pub mod ble_transport;
pub mod connection_prefs;
pub mod dtc;
pub mod elm327;
pub mod mock;
pub mod pid;
#[cfg(unix)]
pub mod pty_transport;
pub mod scanner;
pub mod serial_transport;
pub mod transport;
pub mod types;
pub mod vin;

pub use dtc::Dtc;
pub use pid::Pid;
pub use types::{AdapterInfo, Obd2Error, PidReading, VehicleData};

use std::collections::HashSet;

use async_trait::async_trait;

/// Trait abstracting OBD2 communication — real ELM327 or mock.
#[async_trait]
pub trait Obd2Connection: Send {
    /// Initialize the adapter (reset, configure, detect protocol).
    async fn initialize(&mut self) -> Result<(), Obd2Error>;

    /// Query a single PID and return the parsed reading.
    async fn query_pid(&mut self, pid: Pid) -> Result<PidReading, Obd2Error>;

    /// Read battery voltage via ATRV (ELM327-specific, but useful enough to include).
    async fn read_voltage(&mut self) -> Result<f64, Obd2Error>;

    /// Read stored Diagnostic Trouble Codes (Mode 03).
    async fn read_dtcs(&mut self) -> Result<Vec<Dtc>, Obd2Error>;

    /// Read the Vehicle Identification Number (Mode 09 PID 02).
    async fn read_vin(&mut self) -> Result<String, Obd2Error>;

    /// Return adapter chipset and capability info, if detected during init.
    fn adapter_info(&self) -> Option<&AdapterInfo>;

    /// PIDs the vehicle reported as supported (via Mode 01 PID 00/20/40 bitmaps).
    /// Empty set means the bitmap was not read — callers should try all PIDs.
    fn supported_pids(&self) -> &HashSet<u8>;
}
