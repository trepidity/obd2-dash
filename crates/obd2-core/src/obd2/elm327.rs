use async_trait::async_trait;

use super::dtc::Dtc;
use super::pid::Pid;
use super::transport::Transport;
use super::types::{AdapterInfo, Obd2Error, PidReading};
use super::Obd2Connection;

/// Real ELM327/STN adapter communicating over any Transport (serial, BLE, etc).
pub struct Elm327 {
    transport: Box<dyn Transport>,
    adapter_info: Option<AdapterInfo>,
}

impl Elm327 {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self {
            transport,
            adapter_info: None,
        }
    }

    /// Send an AT/OBD command and read response lines until the '>' prompt.
    async fn send_command(&mut self, cmd: &str) -> Result<Vec<String>, Obd2Error> {
        tracing::debug!(target: "obd2::elm327", "TX cmd: {}", cmd);
        self.transport.send(format!("{}\r", cmd).as_bytes()).await?;

        let lines = self.read_until_prompt().await?;
        tracing::debug!(target: "obd2::elm327", "RX {} lines: {:?}", lines.len(), lines);
        Ok(lines)
    }

    /// Read lines from the adapter until we see the '>' prompt character.
    async fn read_until_prompt(&mut self) -> Result<Vec<String>, Obd2Error> {
        let mut lines = Vec::new();

        loop {
            let buf = self.transport.read_line().await?;
            let trimmed = buf.trim();

            // The prompt character signals end of response
            if trimmed.contains('>') {
                let before_prompt = trimmed.replace('>', "").trim().to_string();
                if !before_prompt.is_empty() {
                    lines.push(before_prompt);
                }
                break;
            }

            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
            }
        }

        Ok(lines)
    }

    /// Parse a hex response line like "41 0C 0C 80" into bytes.
    pub fn parse_hex_response(line: &str) -> Result<Vec<u8>, Obd2Error> {
        line.split_whitespace()
            .map(|s| {
                u8::from_str_radix(s, 16)
                    .map_err(|_| Obd2Error::ParseError(format!("invalid hex byte: {s}")))
            })
            .collect()
    }
}

#[async_trait]
impl Obd2Connection for Elm327 {
    async fn initialize(&mut self) -> Result<(), Obd2Error> {
        tracing::debug!(target: "obd2::elm327", "Starting ELM327 initialization sequence");

        // Step 1: Reset — parse firmware string for chipset detection
        tracing::debug!(target: "obd2::elm327", "Step 1/7: ATZ (reset)");
        let atz_resp = self.send_command("ATZ").await?;
        let firmware_str = atz_resp
            .iter()
            .find(|l| {
                let u = l.to_uppercase();
                u.contains("ELM") || u.contains("STN") || u.contains("OBD")
            })
            .cloned()
            .unwrap_or_default();
        tracing::info!(target: "obd2::elm327", "Firmware: {:?}", firmware_str);

        // Step 2: Probe for STN chip (STI command)
        tracing::debug!(target: "obd2::elm327", "Step 2/7: STI (STN probe)");
        let sti_response = match self.send_command("STI").await {
            Ok(lines) => {
                let joined = lines.join(" ");
                let upper = joined.to_uppercase();
                if upper.contains('?') || upper.contains("ERROR") || upper.is_empty() {
                    None
                } else {
                    tracing::info!(target: "obd2::elm327", "STI response: {:?}", joined);
                    Some(joined)
                }
            }
            Err(_) => None,
        };

        // Detect chipset and capabilities
        let adapter_info = AdapterInfo::detect(&firmware_str, sti_response.as_deref());
        tracing::info!(
            target: "obd2::elm327",
            "Detected chipset: {} ({}), caps: clear_dtcs={}, dual_can={}, enhanced_diag={}, battery_voltage={}, adaptive_timing={}",
            adapter_info.chipset,
            adapter_info.firmware_version,
            adapter_info.caps.can_clear_dtcs,
            adapter_info.caps.dual_can,
            adapter_info.caps.enhanced_diag,
            adapter_info.caps.battery_voltage,
            adapter_info.caps.adaptive_timing,
        );
        self.adapter_info = Some(adapter_info);

        // Step 3: Echo off
        tracing::debug!(target: "obd2::elm327", "Step 3/7: ATE0 (echo off)");
        let _ = self.send_command("ATE0").await?;

        // Step 4: Linefeeds off
        tracing::debug!(target: "obd2::elm327", "Step 4/7: ATL0 (linefeeds off)");
        let _ = self.send_command("ATL0").await?;

        // Step 5: Headers off
        tracing::debug!(target: "obd2::elm327", "Step 5/7: ATH0 (headers off)");
        let _ = self.send_command("ATH0").await?;

        // Step 6: Auto-detect protocol
        tracing::debug!(target: "obd2::elm327", "Step 6/7: ATSP0 (auto-detect protocol)");
        let _ = self.send_command("ATSP0").await?;

        // Step 7: Check supported PIDs (also triggers protocol detection)
        tracing::debug!(target: "obd2::elm327", "Step 7/7: 0100 (supported PIDs / protocol detection)");
        let resp = self.send_command("0100").await?;

        // Check for errors
        for line in &resp {
            let upper = line.to_uppercase();
            if upper.contains("UNABLE TO CONNECT") || upper.contains("ERROR") {
                tracing::error!(target: "obd2::elm327", "Init failed: {}", line);
                return Err(Obd2Error::DeviceError(line.clone()));
            }
        }

        tracing::info!(target: "obd2::elm327", "ELM327 initialized successfully");
        Ok(())
    }

    async fn query_pid(&mut self, pid: Pid) -> Result<PidReading, Obd2Error> {
        tracing::debug!(target: "obd2::elm327", "Query PID {:?} (0x{:02X})", pid, pid.code());
        let resp = self.send_command(&pid.command()).await?;

        // Find the response line starting with the expected service+PID bytes
        let expected_prefix = format!("{:02X} {:02X}", pid.response_id(), pid.code());

        for line in &resp {
            let upper = line.to_uppercase();
            if upper.contains("NO DATA") {
                tracing::debug!(target: "obd2::elm327", "PID {:?}: NO DATA", pid);
                return Err(Obd2Error::NoData);
            }
            if upper.starts_with(&expected_prefix) {
                let bytes = Self::parse_hex_response(&upper)?;
                // bytes[0] = service ID (0x41), bytes[1] = PID code, rest = data
                if bytes.len() < 3 {
                    return Err(Obd2Error::ParseError("response too short".into()));
                }
                let mut reading = pid.parse_value(&bytes[2..])?;
                reading.raw_bytes = Some(bytes[2..].to_vec());
                tracing::debug!(target: "obd2::elm327", "PID {:?} = {:.2} {}", pid, reading.value, reading.unit);
                return Ok(reading);
            }
        }

        Err(Obd2Error::ParseError(format!(
            "no matching response for PID {:#04x}",
            pid.code()
        )))
    }

    async fn read_voltage(&mut self) -> Result<f64, Obd2Error> {
        let resp = self.send_command("ATRV").await?;
        for line in &resp {
            // Response is like "12.4V" or "12.4"
            let cleaned = line.replace(['V', 'v'], "");
            if let Ok(v) = cleaned.trim().parse::<f64>() {
                tracing::debug!(target: "obd2::elm327", "Voltage: {:.1}V", v);
                return Ok(v);
            }
        }
        Err(Obd2Error::ParseError("could not parse voltage".into()))
    }

    fn adapter_info(&self) -> Option<&AdapterInfo> {
        self.adapter_info.as_ref()
    }

    async fn read_vin(&mut self) -> Result<String, Obd2Error> {
        tracing::debug!(target: "obd2::elm327", "Reading VIN (mode 09 PID 02)");
        let resp = self.send_command("0902").await?;

        // Collect all data bytes from the response.
        // Two formats are possible:
        //
        // Legacy (non-CAN): each line starts with "49 02 NN"
        //   49 02 01 13 31 47 31    (seq 01, byte count 0x13, then VIN data)
        //   49 02 02 5A 44 35 ...   (seq 02, continuation data)
        //   bytes[2]=sequence, bytes[3]=byte count (first seq only), data from [4..]
        //
        // CAN ISO-TP (STN/OBDLink): lines have "N:" sequence prefixes
        //   014                      (byte count line — skip)
        //   0: 49 02 01 31 47 31    (service=49, pid=02, msg_count=01, then VIN data)
        //   1: 5A 44 35 53 54 32 4C (continuation — pure data)
        //   2: 46 30 38 38 38 34 39 (continuation — pure data)
        //   bytes[2]=msg count, data starts from [3..] (byte count is on separate line)
        let mut data_bytes: Vec<u8> = Vec::new();
        let mut seen_header = false;
        let mut is_can_format = false;

        for line in &resp {
            let upper = line.to_uppercase();
            if upper.contains("NO DATA") || upper.contains("ERROR") {
                tracing::debug!(target: "obd2::elm327", "VIN: NO DATA");
                return Err(Obd2Error::NoData);
            }

            // Strip CAN ISO-TP sequence prefix ("0: ", "1: ", etc.)
            let had_prefix = upper.len() >= 2
                && upper.as_bytes()[0].is_ascii_digit()
                && upper.as_bytes()[1] == b':';
            let stripped = if had_prefix {
                upper[2..].trim()
            } else {
                upper.trim()
            };

            if stripped.starts_with("49 02") {
                let bytes = Self::parse_hex_response(stripped)?;
                // bytes[0]=0x49, bytes[1]=0x02, bytes[2]=count/sequence
                if bytes.len() > 3 {
                    if !seen_header {
                        seen_header = true;
                        is_can_format = had_prefix;
                        if is_can_format {
                            // CAN: bytes[2] is message count (01), data from [3..]
                            // (byte count was on separate "014" line)
                            data_bytes.extend_from_slice(&bytes[3..]);
                        } else {
                            // Legacy: bytes[2] is sequence, bytes[3] is byte count — skip both
                            data_bytes.extend_from_slice(&bytes[4..]);
                        }
                    } else {
                        // Subsequent "49 02 NN" lines (legacy multi-line format)
                        data_bytes.extend_from_slice(&bytes[3..]);
                    }
                }
            } else if seen_header && is_can_format {
                // CAN continuation frame: pure hex data bytes (no "49 02" prefix)
                // Skip non-hex lines (like the byte count "014")
                if let Ok(bytes) = Self::parse_hex_response(stripped) {
                    if !bytes.is_empty() && bytes.iter().all(|&b| b >= 0x20 || b == 0x00) {
                        data_bytes.extend_from_slice(&bytes);
                    }
                }
            }
        }

        if data_bytes.is_empty() {
            return Err(Obd2Error::ParseError("no VIN data in response".into()));
        }

        // Convert to ASCII, taking up to 17 characters
        let vin: String = data_bytes
            .iter()
            .take(17)
            .filter(|&&b| (0x20..=0x7E).contains(&b))
            .map(|&b| b as char)
            .collect();

        if vin.len() != 17 {
            return Err(Obd2Error::ParseError(format!(
                "VIN length {} (expected 17): {:?}",
                vin.len(),
                vin
            )));
        }

        tracing::info!(target: "obd2::elm327", "VIN: {}", vin);
        Ok(vin)
    }

    async fn read_dtcs(&mut self) -> Result<Vec<Dtc>, Obd2Error> {
        tracing::debug!(target: "obd2::elm327", "Reading DTCs (mode 03)");
        let resp = self.send_command("03").await?;
        let mut dtcs = Vec::new();

        for line in &resp {
            let upper = line.to_uppercase();
            if upper.contains("NO DATA") {
                tracing::debug!(target: "obd2::elm327", "DTCs: NO DATA (no stored codes)");
                return Ok(vec![]);
            }
            // Mode 03 response starts with "43"
            if !upper.starts_with("43") {
                continue;
            }
            let bytes = Self::parse_hex_response(&upper)?;
            // bytes[0] = 0x43 (service response), then pairs of DTC bytes
            let dtc_bytes = &bytes[1..];
            for pair in dtc_bytes.chunks_exact(2) {
                // Skip 00 00 padding
                if pair[0] == 0x00 && pair[1] == 0x00 {
                    continue;
                }
                dtcs.push(Dtc::from_bytes(pair[0], pair[1]));
            }
        }

        tracing::debug!(target: "obd2::elm327", "DTCs found: {} codes {:?}", dtcs.len(), dtcs.iter().map(|d| &d.code).collect::<Vec<_>>());
        Ok(dtcs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_response() {
        let bytes = Elm327::parse_hex_response("41 0C 0C 80").unwrap();
        assert_eq!(bytes, vec![0x41, 0x0C, 0x0C, 0x80]);
    }

    #[test]
    fn test_parse_hex_response_single() {
        let bytes = Elm327::parse_hex_response("41 05 82").unwrap();
        assert_eq!(bytes, vec![0x41, 0x05, 0x82]);
    }

    #[test]
    fn test_parse_hex_response_invalid() {
        assert!(Elm327::parse_hex_response("41 ZZ").is_err());
    }
}
