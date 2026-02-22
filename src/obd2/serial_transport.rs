use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_serial::SerialStream;

use super::transport::Transport;
use super::types::Obd2Error;

/// How long to wait for a single byte before declaring a read timeout.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Serial port transport for ELM327/STN adapters via tokio-serial.
pub struct SerialTransport {
    reader: BufReader<SerialStream>,
}

impl SerialTransport {
    pub fn new(stream: SerialStream) -> Self {
        Self {
            reader: BufReader::new(stream),
        }
    }
}

#[async_trait]
impl Transport for SerialTransport {
    async fn send(&mut self, data: &[u8]) -> Result<(), Obd2Error> {
        let data_str = String::from_utf8_lossy(data);
        tracing::debug!(target: "obd2::serial", "TX {} bytes: {:?}", data.len(), data_str.trim());
        let writer = self.reader.get_mut();
        writer.write_all(data).await.map_err(Obd2Error::Io)?;
        writer.flush().await.map_err(Obd2Error::Io)?;
        Ok(())
    }

    /// Read until CR (\r) or the ELM327 prompt character (>).
    ///
    /// ELM327 uses \r as its line terminator (not \n). The `>` prompt after
    /// a complete response has no trailing \r, so we must also break on it.
    /// Times out after 5 seconds if no data is received.
    async fn read_line(&mut self) -> Result<String, Obd2Error> {
        let mut buf = Vec::new();
        loop {
            let byte = match tokio::time::timeout(READ_TIMEOUT, self.reader.read_u8()).await {
                Ok(result) => result.map_err(Obd2Error::Io)?,
                Err(_) => {
                    tracing::warn!(target: "obd2::serial", "Read timed out after {}s", READ_TIMEOUT.as_secs());
                    return Err(Obd2Error::Timeout);
                }
            };
            match byte {
                b'\r' => break,
                b'\n' => continue, // skip LF if present
                b'>' => {
                    buf.push(b'>');
                    break;
                }
                other => buf.push(other),
            }
        }
        let line = String::from_utf8_lossy(&buf).to_string();
        if !line.is_empty() {
            tracing::debug!(target: "obd2::serial", "RX: {:?}", line);
        }
        Ok(line)
    }
}
