use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    time::Duration,
};

use obd2_core::adapter::elm327::Elm327Adapter;
use obd2_core::adapter::{Adapter, PhysicalTarget, RoutedRequest};
use obd2_core::error::{NegativeResponse, Obd2Error};
use obd2_core::transport::serial::SerialTransport;
use obd2_core::vehicle::PhysicalAddress;
use obd2_dash::gm_enhanced::find_lly_did;
use obd2_dash::gm_evidence::{
    GmDecodedEvidence, GmEvidenceErrorKind, GmEvidenceRecord, GmEvidenceWriter,
};

const ECM_NODE: u8 = 0x10;
const MODE_22: u8 = 0x22;
const PSI_PER_KPA: f64 = 0.145_037_737_7;

const EXACT_DIDS: &[(u16, &str)] = &[
    (0x119D, "public GM VPW barometer V8"),
    (0x1251, "public GM VPW barometer V6"),
    (0x1470, "public LB7/LLY oil pressure"),
    (0x1540, "known desired vane"),
    (0x1543, "known actual vane"),
    (0x163D, "known desired fuel pressure"),
    (0x163E, "known actual fuel pressure"),
];

const RANGES: &[(u16, u16, &str)] = &[
    (0x1180, 0x11AF, "general GM engine VPW range"),
    (0x1240, 0x1260, "public barometer neighborhood"),
    (0x1460, 0x1480, "pressure neighborhood"),
    (0x1530, 0x1560, "VGT/boost neighborhood"),
    (0x1620, 0x1645, "LLY diesel pressure/balance neighborhood"),
];

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::args()
        .nth(1)
        .unwrap_or_else(|| "/dev/cu.usbserial-223230360830".to_string());
    let baud = env::args()
        .nth(2)
        .and_then(|arg| arg.parse::<u32>().ok())
        .unwrap_or(115_200);

    let transport = SerialTransport::new(&port, baud)?;
    let mut adapter = Elm327Adapter::new(Box::new(transport));
    let report = adapter.initialize().await?;
    println!("protocol: {:?}", report.info.protocol);
    let protocol = format!("{:?}", report.info.protocol);
    let mut evidence = GmEvidenceWriter::create_raw_capture("gm-desired-map-probe")?;
    println!("evidence: {}", evidence.path().display());
    print_standard_context(&mut adapter, &mut evidence, &port, &protocol).await?;

    println!("\nExact known/public probes:");
    let mut seen = BTreeSet::new();
    for (did, label) in EXACT_DIDS {
        seen.insert(*did);
        probe_did(
            &mut adapter,
            &mut evidence,
            &port,
            &protocol,
            *did,
            label,
            true,
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    println!("\nBounded Mode 22 scan for pressure-like positives:");
    for (start, end, label) in RANGES {
        let mut summary = BTreeMap::<String, usize>::new();
        let mut positives = 0usize;
        println!("\n[{label}: {start:04X}..={end:04X}]");

        for did in *start..=*end {
            if seen.contains(&did) {
                continue;
            }

            match request_mode_22(&mut adapter, did, true).await {
                Ok(bytes) => {
                    evidence.append(
                        &mode22_evidence(&port, &protocol, did, true, "gm-mode22-scan")
                            .with_response_bytes(bytes.clone())
                            .with_decoded(decode_json(did, &bytes), confidence_label(did)),
                    )?;
                    positives += 1;
                    println!(
                        "  {did:04X} -> {} {}",
                        hex_bytes(&bytes),
                        decode_notes(&bytes)
                    );
                }
                Err(error) => {
                    evidence.append(
                        &mode22_evidence(&port, &protocol, did, true, "gm-mode22-scan")
                            .with_error(GmEvidenceErrorKind::Adapter, error_bucket(&error)),
                    )?;
                    *summary.entry(error_bucket(&error)).or_default() += 1;
                }
            }

            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        println!("  positives: {positives}");
        for (kind, count) in summary {
            println!("  {kind}: {count}");
        }
    }

    evidence.flush()?;
    Ok(())
}

async fn print_standard_context(
    adapter: &mut Elm327Adapter,
    evidence: &mut GmEvidenceWriter,
    port: &str,
    protocol: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("standard context:");
    for (service_id, data, label) in [
        (0x01, &[0x0B][..], "MAP"),
        (0x01, &[0x33][..], "BARO"),
        (0x01, &[0x0C][..], "RPM"),
        (0x01, &[0x0D][..], "speed"),
    ] {
        let request = RoutedRequest {
            service_id,
            data: data.to_vec(),
            target: PhysicalTarget::Broadcast,
        };
        match adapter.routed_request(&request).await {
            Ok(bytes) => {
                evidence.append(
                    &GmEvidenceRecord::routed_request_outcome(
                        "broadcast",
                        0,
                        [0x00, 0x00, 0x00],
                        service_id,
                        data.to_vec(),
                        "standard-context",
                    )
                    .with_adapter_context(Some(port.to_string()), Some(protocol.to_string()))
                    .with_response_bytes(bytes.clone())
                    .with_decoded(standard_json(data[0], &bytes), Some("verified".to_string())),
                )?;
                println!(
                    "  {label:<5} -> {} {}",
                    hex_bytes(&bytes),
                    standard_notes(data[0], &bytes)
                );
            }
            Err(error) => {
                evidence.append(
                    &GmEvidenceRecord::routed_request_outcome(
                        "broadcast",
                        0,
                        [0x00, 0x00, 0x00],
                        service_id,
                        data.to_vec(),
                        "standard-context",
                    )
                    .with_adapter_context(Some(port.to_string()), Some(protocol.to_string()))
                    .with_error(GmEvidenceErrorKind::Adapter, error.to_string()),
                )?;
                println!("  {label:<5} -> ERR {error}");
            }
        }
    }
    Ok(())
}

async fn probe_did(
    adapter: &mut Elm327Adapter,
    evidence: &mut GmEvidenceWriter,
    port: &str,
    protocol: &str,
    did: u16,
    label: &str,
    selector: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    match request_mode_22(adapter, did, selector).await {
        Ok(bytes) => {
            evidence.append(
                &mode22_evidence(port, protocol, did, selector, label)
                    .with_response_bytes(bytes.clone())
                    .with_decoded(decode_json(did, &bytes), confidence_label(did)),
            )?;
            println!(
                "  {did:04X} {label:<34} -> {} {}",
                hex_bytes(&bytes),
                decode_notes(&bytes)
            );
        }
        Err(error) => {
            evidence.append(
                &mode22_evidence(port, protocol, did, selector, label)
                    .with_error(GmEvidenceErrorKind::Adapter, error_bucket(&error)),
            )?;
            println!("  {did:04X} {label:<34} -> ERR {error}");
        }
    }
    Ok(())
}

async fn request_mode_22(
    adapter: &mut Elm327Adapter,
    did: u16,
    selector: bool,
) -> Result<Vec<u8>, Obd2Error> {
    let mut data = Vec::with_capacity(3);
    data.push((did >> 8) as u8);
    data.push((did & 0xFF) as u8);
    if selector {
        data.push(0x01);
    }

    let request = RoutedRequest {
        service_id: MODE_22,
        data,
        target: PhysicalTarget::Addressed(PhysicalAddress::J1850 {
            node: ECM_NODE,
            header: [0x6C, ECM_NODE, 0xF1],
        }),
    };

    adapter.routed_request(&request).await
}

fn standard_notes(pid: u8, bytes: &[u8]) -> String {
    match (pid, bytes) {
        (0x0B | 0x33, [value, ..]) => format!(
            "({:.1} kPa / {:.1} psi abs)",
            f64::from(*value),
            f64::from(*value) * PSI_PER_KPA
        ),
        (0x0C, [a, b, ..]) => {
            let rpm = f64::from(u16::from_be_bytes([*a, *b])) / 4.0;
            format!("({rpm:.0} rpm)")
        }
        (0x0D, [value, ..]) => format!("({value} km/h)"),
        _ => String::new(),
    }
}

fn standard_json(pid: u8, bytes: &[u8]) -> GmDecodedEvidence {
    match (pid, bytes) {
        (0x0B | 0x33, [value, ..]) => GmDecodedEvidence::Value {
            signal: format!("standard PID {pid:02X}"),
            raw: u32::from(*value),
            value: f64::from(*value),
            unit: "kPa".to_string(),
        },
        (0x0C, [a, b, ..]) => {
            let raw = u16::from_be_bytes([*a, *b]);
            GmDecodedEvidence::Value {
                signal: "standard RPM".to_string(),
                raw: u32::from(raw),
                value: f64::from(raw) / 4.0,
                unit: "rpm".to_string(),
            }
        }
        (0x0D, [value, ..]) => GmDecodedEvidence::Value {
            signal: "standard speed".to_string(),
            raw: u32::from(*value),
            value: f64::from(*value),
            unit: "km/h".to_string(),
        },
        _ => GmDecodedEvidence::Empty,
    }
}

fn mode22_evidence(
    port: &str,
    protocol: &str,
    did: u16,
    selector: bool,
    decoder: &str,
) -> GmEvidenceRecord {
    let mut data = vec![(did >> 8) as u8, (did & 0xFF) as u8];
    if selector {
        data.push(0x01);
    }
    GmEvidenceRecord::routed_request_outcome(
        "ECM/PCM",
        ECM_NODE,
        [0x6C, ECM_NODE, 0xF1],
        MODE_22,
        data,
        decoder,
    )
    .with_adapter_context(Some(port.to_string()), Some(protocol.to_string()))
}

fn decode_json(did: u16, bytes: &[u8]) -> GmDecodedEvidence {
    if let Some(definition) = find_lly_did(did) {
        if let Ok(decoded) = definition.decode_value(bytes) {
            return GmDecodedEvidence::Value {
                signal: definition.name.to_string(),
                raw: decoded.selected_raw,
                value: decoded.value,
                unit: decoded.unit.to_string(),
            };
        }
    }
    GmDecodedEvidence::Empty
}

fn confidence_label(did: u16) -> Option<String> {
    find_lly_did(did).map(|definition| definition.confidence.as_str().to_string())
}

fn decode_notes(bytes: &[u8]) -> String {
    match bytes {
        [] => String::new(),
        [value] => format!(
            "(u8={} => {:.1} kPa / {:.1} psi abs, pct={:.1})",
            value,
            f64::from(*value),
            f64::from(*value) * PSI_PER_KPA,
            f64::from(*value) * 100.0 / 255.0
        ),
        [a, b, ..] => {
            let raw = u16::from_be_bytes([*a, *b]);
            format!(
                "(u16={} raw*kPa={:.1} psi, raw/4 kPa={:.1} psi, fuel-scale={:.1} psi)",
                raw,
                f64::from(raw) * PSI_PER_KPA,
                (f64::from(raw) / 4.0) * PSI_PER_KPA,
                f64::from(raw) * (145.0 / 256.0)
            )
        }
    }
}

fn error_bucket(error: &Obd2Error) -> String {
    match error {
        Obd2Error::NoData => "no data".to_string(),
        Obd2Error::NegativeResponse {
            nrc: NegativeResponse::RequestOutOfRange,
            ..
        } => "NRC 31 request out of range".to_string(),
        Obd2Error::NegativeResponse {
            nrc: NegativeResponse::SubFunctionNotSupported,
            ..
        } => "NRC 12 subfunction not supported".to_string(),
        Obd2Error::NegativeResponse {
            nrc: NegativeResponse::ServiceNotSupported,
            ..
        } => "NRC 11 service not supported".to_string(),
        Obd2Error::NegativeResponse { nrc, .. } => format!("NRC {:02X} {nrc}", nrc.code()),
        other => other.to_string(),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut out, "{byte:02X}").expect("write to string");
    }
    out
}
