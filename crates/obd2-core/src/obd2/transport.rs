use async_trait::async_trait;

use super::types::Obd2Error;

/// Trait abstracting byte-level I/O for ELM327/STN command transport.
/// Implemented by SerialTransport (tokio-serial) and BleTransport (btleplug GATT).
#[async_trait]
pub trait Transport: Send {
    /// Send raw bytes to the adapter (command + \r terminator).
    async fn send(&mut self, data: &[u8]) -> Result<(), Obd2Error>;

    /// Read a single line from the adapter. Returns the line content (without line ending).
    /// Blocks until a line delimiter (\r, \n) or the `>` prompt is encountered.
    async fn read_line(&mut self) -> Result<String, Obd2Error>;
}
