use std::collections::BTreeSet;
use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::Utc;
use obd2_core::protocol::codec::decode_j1850_headers_on;
use obd2_core::transport::serial::SerialTransport;
use obd2_core::transport::{CaptureMetadata, Link, LoggingTransport};
use obd2_dash::gm_evidence::{
    GmDecodedEvidence, GmEvidenceErrorKind, GmEvidenceRecord, GmEvidenceWriter,
};

const ECM_NODE: u8 = 0x10;
const BROADCAST_HEADER: [u8; 3] = [0x68, 0x6A, 0xF1];
const ECM_HEADER: [u8; 3] = [0x6C, ECM_NODE, 0xF1];
const MODE_01: u8 = 0x01;
const MODE_22: u8 = 0x22;
const MODE_22_SELECTOR: u8 = 0x01;
const DISCOVERY_START: u16 = 0x1100;
const DISCOVERY_END: u16 = 0x11FF;
const REQUEST_INTERVAL: Duration = Duration::from_millis(500);
const RUN_TIME_CAP: Duration = Duration::from_secs(5 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(2);
const EXPECTED_REQUEST_COUNT: usize = 281;
const MAX_CONSECUTIVE_TRANSPORT_FAILURES: u8 = 3;
const MAX_POSITIVE_DATA_BYTES: usize = 8;
const MAX_RPM_FRACTION_CHANGE: f64 = 0.15;

const CONTROL_PIDS: &[(u8, &'static str)] = &[
    (0x0C, "RPM control"),
    (0x05, "coolant-temperature control"),
    (0x0F, "intake-air-temperature control"),
    (0x23, "standard rail-pressure control"),
];

const DISCOVERY_CHUNKS: &[(u16, u16)] = &[
    (0x1140, 0x117F),
    (0x1180, 0x11BF),
    (0x1100, 0x113F),
    (0x11C0, 0x11FF),
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum RequestKind {
    Control,
    Candidate { did: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeRequest {
    label: String,
    service: u8,
    data: Vec<u8>,
    target_header: [u8; 3],
    kind: RequestKind,
}

impl ProbeRequest {
    fn command(&self) -> Result<String, String> {
        validate_request(self)?;
        let mut command = format!("{:02X}", self.service);
        for byte in &self.data {
            write!(&mut command, "{byte:02X}").expect("write to string");
        }
        Ok(command)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ParsedOutcome {
    Positive { full_frame: Vec<u8>, data: Vec<u8> },
    Negative { full_frame: Vec<u8>, nrc: u8 },
    TransportFailure(String),
}

#[derive(Debug, Default)]
struct RunGuard {
    baseline_rpm: Option<f64>,
    consecutive_transport_failures: u8,
}

impl RunGuard {
    fn observe_control_rpm(&mut self, rpm: f64) -> Result<(), String> {
        let Some(baseline) = self.baseline_rpm else {
            self.baseline_rpm = Some(rpm);
            return Ok(());
        };

        let baseline_running = baseline > 0.0;
        let current_running = rpm > 0.0;
        if baseline_running != current_running {
            return Err(format!(
                "RPM state changed during discovery: baseline={baseline:.1}, current={rpm:.1}"
            ));
        }
        if baseline_running {
            let change = (rpm - baseline).abs() / baseline;
            if change > MAX_RPM_FRACTION_CHANGE {
                return Err(format!(
                    "RPM changed {:.1}% during discovery: baseline={baseline:.1}, current={rpm:.1}",
                    change * 100.0
                ));
            }
        }
        Ok(())
    }

    fn observe_transport_success(&mut self) {
        self.consecutive_transport_failures = 0;
    }

    fn observe_transport_failure(&mut self, detail: &str) -> Result<(), String> {
        self.consecutive_transport_failures = self.consecutive_transport_failures.saturating_add(1);
        if self.consecutive_transport_failures >= MAX_CONSECUTIVE_TRANSPORT_FAILURES {
            return Err(format!(
                "{} consecutive transport failures; last={detail}",
                self.consecutive_transport_failures
            ));
        }
        Ok(())
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::args()
        .nth(1)
        .unwrap_or_else(|| "/dev/cu.usbserial-223230360830".to_string());
    let baud = env::args()
        .nth(2)
        .and_then(|arg| arg.parse::<u32>().ok())
        .unwrap_or(115_200);
    let plan = build_plan();
    validate_plan(&plan)?;

    let capture_dir = PathBuf::from("raw-captures");
    fs::create_dir_all(&capture_dir)?;
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let raw_path = capture_dir.join(format!("gm-fuel-temp-discovery-{timestamp}.obd2raw"));
    let jsonl_path = capture_dir.join(format!("gm-fuel-temp-discovery-{timestamp}.jsonl"));

    let serial = SerialTransport::new(&port, baud)?;
    let mut transport = LoggingTransport::new(serial);
    transport.start_capture(
        &raw_path,
        &CaptureMetadata {
            transport_type: "serial-j1850-vpw".to_string(),
            port_or_device: port.clone(),
            baud_rate: Some(baud),
        },
    )?;
    initialize_read_only(&mut transport).await?;

    let mut evidence = GmEvidenceWriter::create(&jsonl_path)?;
    let run_started = Instant::now();
    let mut last_request_started: Option<tokio::time::Instant> = None;
    let mut current_header = None;
    let mut guard = RunGuard::default();
    let mut positives = Vec::new();

    println!("raw evidence: {}", raw_path.display());
    println!("JSONL evidence: {}", jsonl_path.display());
    println!(
        "bounded discovery: {} requests, DIDs 0x{DISCOVERY_START:04X}-0x{DISCOVERY_END:04X}, max 2 requests/s, 5 minute cap",
        plan.len()
    );

    for (index, request) in plan.iter().enumerate() {
        if run_started.elapsed() >= RUN_TIME_CAP {
            return abort(
                &mut transport,
                &mut evidence,
                format!(
                    "5-minute wall-clock cap reached before request {}",
                    index + 1
                ),
            );
        }

        if current_header != Some(request.target_header) {
            set_header(&mut transport, request.target_header).await?;
            current_header = Some(request.target_header);
        }

        if let Some(previous) = last_request_started {
            tokio::time::sleep_until(previous + REQUEST_INTERVAL).await;
        }
        last_request_started = Some(tokio::time::Instant::now());

        let command = request.command()?;
        let remaining = RUN_TIME_CAP.saturating_sub(run_started.elapsed());
        let timeout = REQUEST_TIMEOUT.min(remaining);
        let raw_response =
            match tokio::time::timeout(timeout, raw_command(&mut transport, &command)).await {
                Ok(Ok(response)) => response,
                Ok(Err(error)) => {
                    let detail = error.to_string();
                    record_outcome(
                        &mut evidence,
                        &port,
                        request,
                        &command,
                        "",
                        &ParsedOutcome::TransportFailure(detail.clone()),
                    )?;
                    if matches!(request.kind, RequestKind::Control) {
                        return abort(
                            &mut transport,
                            &mut evidence,
                            format!("known control failed: {}: {detail}", request.label),
                        );
                    }
                    if let Err(reason) = guard.observe_transport_failure(&detail) {
                        return abort(&mut transport, &mut evidence, reason);
                    }
                    continue;
                }
                Err(_) => {
                    let detail = format!("request timeout after {} ms", timeout.as_millis());
                    record_outcome(
                        &mut evidence,
                        &port,
                        request,
                        &command,
                        "",
                        &ParsedOutcome::TransportFailure(detail.clone()),
                    )?;
                    if matches!(request.kind, RequestKind::Control) {
                        return abort(
                            &mut transport,
                            &mut evidence,
                            format!("known control timed out: {}", request.label),
                        );
                    }
                    if let Err(reason) = guard.observe_transport_failure(&detail) {
                        return abort(&mut transport, &mut evidence, reason);
                    }
                    continue;
                }
            };

        let outcome = parse_response(request, &raw_response);
        record_outcome(
            &mut evidence,
            &port,
            request,
            &command,
            &raw_response,
            &outcome,
        )?;

        match outcome {
            ParsedOutcome::Positive { ref data, .. } => {
                guard.observe_transport_success();
                if data.len() > MAX_POSITIVE_DATA_BYTES {
                    return abort(
                        &mut transport,
                        &mut evidence,
                        format!(
                            "unexpected positive response length {} for {}",
                            data.len(),
                            request.label
                        ),
                    );
                }
                if matches!(request.kind, RequestKind::Control) {
                    if let Err(reason) = validate_control(request, data, &mut guard) {
                        return abort(&mut transport, &mut evidence, reason);
                    }
                } else if let RequestKind::Candidate { did } = request.kind {
                    positives.push((did, data.clone()));
                    println!("positive candidate 0x{did:04X}: {}", hex_bytes(data));
                }
            }
            ParsedOutcome::Negative { nrc, .. }
                if matches!(request.kind, RequestKind::Candidate { .. }) =>
            {
                if nrc != 0x12 && nrc != 0x31 {
                    return abort(
                        &mut transport,
                        &mut evidence,
                        format!("unexpected NRC 0x{nrc:02X} for {}", request.label),
                    );
                }
                guard.observe_transport_success();
            }
            ParsedOutcome::Negative { nrc, .. } => {
                return abort(
                    &mut transport,
                    &mut evidence,
                    format!("known control returned NRC 0x{nrc:02X}: {}", request.label),
                );
            }
            ParsedOutcome::TransportFailure(ref detail) => {
                if matches!(request.kind, RequestKind::Control) {
                    return abort(
                        &mut transport,
                        &mut evidence,
                        format!("known control failed: {}: {detail}", request.label),
                    );
                }
                if let Err(reason) = guard.observe_transport_failure(detail) {
                    return abort(&mut transport, &mut evidence, reason);
                }
            }
        }
    }

    evidence.flush()?;
    transport.stop_capture()?;
    println!(
        "completed safely in {:.1}s; {} positive DIDs",
        run_started.elapsed().as_secs_f64(),
        positives.len()
    );
    for (did, data) in positives {
        println!("  0x{did:04X}: {}", hex_bytes(&data));
    }
    Ok(())
}

fn build_plan() -> Vec<ProbeRequest> {
    let mut plan = Vec::with_capacity(EXPECTED_REQUEST_COUNT);
    append_controls(&mut plan);
    for &(start, end) in DISCOVERY_CHUNKS {
        for did in start..=end {
            plan.push(ProbeRequest {
                label: format!("candidate DID 0x{did:04X}"),
                service: MODE_22,
                data: vec![(did >> 8) as u8, did as u8, MODE_22_SELECTOR],
                target_header: ECM_HEADER,
                kind: RequestKind::Candidate { did },
            });
        }
        append_controls(&mut plan);
    }
    plan
}

fn append_controls(plan: &mut Vec<ProbeRequest>) {
    for &(pid, label) in CONTROL_PIDS {
        plan.push(ProbeRequest {
            label: label.to_string(),
            service: MODE_01,
            data: vec![pid],
            target_header: BROADCAST_HEADER,
            kind: RequestKind::Control,
        });
    }
    plan.push(ProbeRequest {
        label: "E60 rail-pressure control".to_string(),
        service: MODE_22,
        data: vec![0x16, 0x3E, MODE_22_SELECTOR],
        target_header: ECM_HEADER,
        kind: RequestKind::Control,
    });
}

fn validate_plan(plan: &[ProbeRequest]) -> Result<(), String> {
    if plan.len() != EXPECTED_REQUEST_COUNT {
        return Err(format!(
            "request-count invariant failed: expected {EXPECTED_REQUEST_COUNT}, got {}",
            plan.len()
        ));
    }
    let mut candidates = BTreeSet::new();
    let mut controls = 0usize;
    for request in plan {
        validate_request(request)?;
        match request.kind {
            RequestKind::Control => controls += 1,
            RequestKind::Candidate { did } => {
                candidates.insert(did);
            }
        }
    }
    let expected: BTreeSet<u16> = (DISCOVERY_START..=DISCOVERY_END).collect();
    if candidates != expected {
        return Err("candidate DID set is not exactly 0x1100-0x11FF".to_string());
    }
    if controls != 25 {
        return Err(format!("expected 25 controls, got {controls}"));
    }
    Ok(())
}

fn validate_request(request: &ProbeRequest) -> Result<(), String> {
    match request.kind {
        RequestKind::Candidate { did } => {
            if request.service != MODE_22
                || request.target_header != ECM_HEADER
                || !(DISCOVERY_START..=DISCOVERY_END).contains(&did)
                || request.data != [(did >> 8) as u8, did as u8, MODE_22_SELECTOR]
            {
                return Err(format!("candidate request escaped whitelist: {request:?}"));
            }
        }
        RequestKind::Control => {
            let standard_control = request.service == MODE_01
                && request.target_header == BROADCAST_HEADER
                && request.data.len() == 1
                && CONTROL_PIDS.iter().any(|(pid, _)| request.data == [*pid]);
            let e60_control = request.service == MODE_22
                && request.target_header == ECM_HEADER
                && request.data == [0x16, 0x3E, MODE_22_SELECTOR];
            if !standard_control && !e60_control {
                return Err(format!("control request escaped whitelist: {request:?}"));
            }
        }
    }
    Ok(())
}

async fn initialize_read_only(
    transport: &mut LoggingTransport<SerialTransport>,
) -> Result<(), Box<dyn std::error::Error>> {
    for command in ["ATZ", "ATE0", "ATL0", "ATH1", "ATS1", "ATSP2"] {
        let response = raw_command(transport, command).await?;
        if command != "ATZ" && !response.contains("OK") {
            return Err(
                format!("adapter initialization failed for {command}: {response:?}").into(),
            );
        }
    }
    Ok(())
}

async fn set_header(
    transport: &mut LoggingTransport<SerialTransport>,
    header: [u8; 3],
) -> Result<(), Box<dyn std::error::Error>> {
    let command = format!("AT SH {}", hex_bytes(&header));
    let response = raw_command(transport, &command).await?;
    if !response.contains("OK") {
        return Err(format!("adapter header change failed for {command}: {response:?}").into());
    }
    Ok(())
}

async fn raw_command(
    transport: &mut impl Link,
    command: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    transport.annotate_raw_capture(&format!("command={command}"));
    let mut framed = String::with_capacity(command.len() + 1);
    framed.push_str(command);
    framed.push('\r');
    transport.write(framed.as_bytes()).await?;
    let bytes = transport.read().await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn parse_response(request: &ProbeRequest, raw: &str) -> ParsedOutcome {
    let upper = raw.to_ascii_uppercase();
    if upper.contains("BUS ERROR")
        || upper.contains("DATA ERROR")
        || upper.contains("RX ERROR")
        || upper.contains("STOPPED")
        || upper.contains("LV RESET")
        || upper.contains("UNABLE TO CONNECT")
    {
        return ParsedOutcome::TransportFailure(raw.trim().to_string());
    }
    if upper.contains("NO DATA") {
        return ParsedOutcome::TransportFailure("NO DATA".to_string());
    }

    let expected_positive = request.service.wrapping_add(0x40);
    let expected_echo_len = if request.service == MODE_22 { 2 } else { 1 };
    let expected_echo = &request.data[..expected_echo_len];

    for line in upper.split(['\r', '\n']) {
        let line = line.trim().trim_end_matches('>').trim();
        if line.is_empty() || line.eq_ignore_ascii_case(&request.command().unwrap_or_default()) {
            continue;
        }
        let Ok(frame) = decode_j1850_headers_on(line) else {
            continue;
        };
        if frame.source != ECM_NODE {
            continue;
        }
        let mut full_frame = vec![frame.priority, frame.target, frame.source];
        full_frame.extend_from_slice(&frame.payload);
        if let Some(checksum) = frame.checksum {
            full_frame.push(checksum);
        }

        if frame.payload.first().copied() == Some(expected_positive)
            && frame.payload.get(1..1 + expected_echo.len()) == Some(expected_echo)
        {
            return ParsedOutcome::Positive {
                full_frame,
                data: frame.payload[1 + expected_echo.len()..].to_vec(),
            };
        }
        if frame.payload.starts_with(&[0x7F, request.service]) {
            let Some(nrc) = frame.payload.last().copied() else {
                return ParsedOutcome::TransportFailure(
                    "malformed negative response without NRC".to_string(),
                );
            };
            return ParsedOutcome::Negative { full_frame, nrc };
        }
    }

    ParsedOutcome::TransportFailure(format!(
        "no matching header-inclusive response: {}",
        raw.trim()
    ))
}

fn validate_control(
    request: &ProbeRequest,
    data: &[u8],
    guard: &mut RunGuard,
) -> Result<(), String> {
    match (request.service, request.data.as_slice()) {
        (MODE_01, [0x0C]) if data.len() >= 2 => {
            let raw = u16::from_be_bytes([data[0], data[1]]);
            guard.observe_control_rpm(f64::from(raw) / 4.0)
        }
        (MODE_01, [0x05] | [0x0F]) if !data.is_empty() => Ok(()),
        (MODE_01, [0x23]) | (MODE_22, [0x16, 0x3E, MODE_22_SELECTOR]) if data.len() >= 2 => Ok(()),
        _ => Err(format!(
            "malformed known-control response for {}: {}",
            request.label,
            hex_bytes(data)
        )),
    }
}

fn record_outcome(
    evidence: &mut GmEvidenceWriter,
    port: &str,
    request: &ProbeRequest,
    command: &str,
    raw_response: &str,
    outcome: &ParsedOutcome,
) -> std::io::Result<()> {
    let module_label = if request.target_header == ECM_HEADER {
        "ECM/PCM"
    } else {
        "broadcast"
    };
    let mut record = GmEvidenceRecord::routed_request_outcome(
        module_label,
        if request.target_header == ECM_HEADER {
            ECM_NODE
        } else {
            0
        },
        request.target_header,
        request.service,
        request.data.clone(),
        "gm-fuel-temp-discovery-raw-j1850",
    )
    .with_adapter_context(Some(port.to_string()), Some("J1850Vpw".to_string()))
    .with_raw_text(command, raw_response);

    match outcome {
        ParsedOutcome::Positive { full_frame, data } => {
            record = record.with_response_bytes(full_frame.clone());
            if let Some(decoded) = decoded_evidence(request, data) {
                record = record.with_decoded(
                    decoded,
                    Some(
                        match request.kind {
                            RequestKind::Control => "live-observed",
                            RequestKind::Candidate { .. } => "candidate",
                        }
                        .to_string(),
                    ),
                );
            }
        }
        ParsedOutcome::Negative { full_frame, nrc } => {
            record = record.with_response_bytes(full_frame.clone()).with_error(
                GmEvidenceErrorKind::NegativeResponse,
                format!("NRC 0x{nrc:02X}"),
            );
        }
        ParsedOutcome::TransportFailure(detail) => {
            record = record.with_error(GmEvidenceErrorKind::Transport, detail.clone());
        }
    }
    evidence.append(&record)
}

fn decoded_evidence(request: &ProbeRequest, data: &[u8]) -> Option<GmDecodedEvidence> {
    let (value, unit) = match (request.service, request.data.as_slice()) {
        (MODE_01, [0x0C]) if data.len() >= 2 => (
            f64::from(u16::from_be_bytes([data[0], data[1]])) / 4.0,
            "rpm",
        ),
        (MODE_01, [0x05] | [0x0F]) if !data.is_empty() => (f64::from(data[0]) - 40.0, "deg C"),
        (MODE_01, [0x23]) if data.len() >= 2 => (
            f64::from(u16::from_be_bytes([data[0], data[1]])) * 10.0,
            "kPa",
        ),
        (MODE_22, [0x16, 0x3E, MODE_22_SELECTOR]) if data.len() >= 2 => (
            f64::from(u16::from_be_bytes([data[0], data[1]])) * (145.0 / 256.0),
            "psi",
        ),
        _ if matches!(request.kind, RequestKind::Candidate { .. }) => {
            let raw = data
                .iter()
                .take(4)
                .fold(0u32, |value, byte| (value << 8) | u32::from(*byte));
            (f64::from(raw), "raw")
        }
        _ => return None,
    };

    let raw = data
        .iter()
        .take(4)
        .fold(0u32, |value, byte| (value << 8) | u32::from(*byte));
    Some(GmDecodedEvidence::Value {
        signal: request.label.clone(),
        raw,
        value,
        unit: unit.to_string(),
    })
}

fn abort<T>(
    transport: &mut LoggingTransport<SerialTransport>,
    evidence: &mut GmEvidenceWriter,
    reason: String,
) -> Result<T, Box<dyn std::error::Error>> {
    let _ = evidence.flush();
    let _ = transport.stop_capture();
    Err(format!("discovery aborted safely: {reason}").into())
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut out, "{byte:02X}").expect("write to string");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(did: u16) -> ProbeRequest {
        ProbeRequest {
            label: format!("candidate DID 0x{did:04X}"),
            service: MODE_22,
            data: vec![(did >> 8) as u8, did as u8, MODE_22_SELECTOR],
            target_header: ECM_HEADER,
            kind: RequestKind::Candidate { did },
        }
    }

    #[test]
    fn plan_is_exactly_the_approved_281_reads() {
        let plan = build_plan();
        validate_plan(&plan).unwrap();
        assert_eq!(plan.len(), 281);
        assert_eq!(
            plan.iter()
                .filter(|request| matches!(request.kind, RequestKind::Candidate { .. }))
                .count(),
            256
        );
        assert!(plan
            .iter()
            .all(|request| matches!(request.service, MODE_01 | MODE_22)));
    }

    #[test]
    fn candidate_order_matches_the_approved_chunks() {
        let dids: Vec<u16> = build_plan()
            .into_iter()
            .filter_map(|request| match request.kind {
                RequestKind::Candidate { did } => Some(did),
                RequestKind::Control => None,
            })
            .collect();
        assert_eq!(&dids[0..64], &(0x1140..=0x117F).collect::<Vec<_>>());
        assert_eq!(&dids[64..128], &(0x1180..=0x11BF).collect::<Vec<_>>());
        assert_eq!(&dids[128..192], &(0x1100..=0x113F).collect::<Vec<_>>());
        assert_eq!(&dids[192..256], &(0x11C0..=0x11FF).collect::<Vec<_>>());
    }

    #[test]
    fn whitelist_rejects_active_services_and_out_of_range_dids() {
        let mut active = candidate(0x1140);
        active.service = 0x2F;
        assert!(validate_request(&active).is_err());

        assert!(validate_request(&candidate(0x1200)).is_err());

        let mut wrong_selector = candidate(0x1140);
        wrong_selector.data[2] = 0x00;
        assert!(validate_request(&wrong_selector).is_err());
    }

    #[test]
    fn parses_header_inclusive_positive_and_negative_responses() {
        let positive = parse_response(&candidate(0x1193), "6C F1 10 62 11 93 00 80 00\r\r>");
        assert!(matches!(
            positive,
            ParsedOutcome::Positive { data, .. } if data == [0x00, 0x80]
        ));

        let negative = parse_response(&candidate(0x1174), "6C F1 10 7F 22 11 74 01 31 5D\r\r>");
        assert!(matches!(
            negative,
            ParsedOutcome::Negative { nrc: 0x31, .. }
        ));
    }

    #[test]
    fn guard_aborts_engine_state_and_large_rpm_changes() {
        let mut stopped = RunGuard::default();
        stopped.observe_control_rpm(0.0).unwrap();
        assert!(stopped.observe_control_rpm(650.0).is_err());

        let mut running = RunGuard::default();
        running.observe_control_rpm(650.0).unwrap();
        assert!(running.observe_control_rpm(700.0).is_ok());
        assert!(running.observe_control_rpm(800.0).is_err());
    }

    #[test]
    fn guard_aborts_after_three_consecutive_transport_failures() {
        let mut guard = RunGuard::default();
        assert!(guard.observe_transport_failure("one").is_ok());
        assert!(guard.observe_transport_failure("two").is_ok());
        assert!(guard.observe_transport_failure("three").is_err());
        guard.observe_transport_success();
        assert_eq!(guard.consecutive_transport_failures, 0);
    }

    #[test]
    fn timing_caps_are_fixed_to_the_approved_values() {
        assert_eq!(REQUEST_INTERVAL, Duration::from_millis(500));
        assert_eq!(RUN_TIME_CAP, Duration::from_secs(300));
        assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(2));
    }
}
