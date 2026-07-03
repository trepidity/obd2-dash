use std::env;

use obd2_core::adapter::elm327::Elm327Adapter;
use obd2_core::adapter::{Adapter, PhysicalTarget, RoutedRequest};
use obd2_core::transport::serial::SerialTransport;
use obd2_core::transport::Link;
use obd2_core::vehicle::PhysicalAddress;
use obd2_dash::gm_enhanced::find_lly_did;
use obd2_dash::gm_evidence::{
    GmDecodedEvidence, GmEvidenceErrorKind, GmEvidenceRecord, GmEvidenceWriter,
};

#[derive(Debug, Clone, Copy)]
struct Probe {
    label: &'static str,
    service: u8,
    data: &'static [u8],
    node: Option<u8>,
}

const PROBES: &[Probe] = &[
    Probe {
        label: "standard MAP 010B",
        service: 0x01,
        data: &[0x0B],
        node: None,
    },
    Probe {
        label: "standard BARO 0133",
        service: 0x01,
        data: &[0x33],
        node: None,
    },
    Probe {
        label: "standard fuel rail gauge 0123",
        service: 0x01,
        data: &[0x23],
        node: None,
    },
    Probe {
        label: "standard fuel rail absolute 0159",
        service: 0x01,
        data: &[0x59],
        node: None,
    },
    Probe {
        label: "GM VPW barometer V8 22119D01",
        service: 0x22,
        data: &[0x11, 0x9D, 0x01],
        node: Some(0x10),
    },
    Probe {
        label: "GM VPW barometer V6 22125101",
        service: 0x22,
        data: &[0x12, 0x51, 0x01],
        node: Some(0x10),
    },
    Probe {
        label: "LLY desired fuel pressure 22163D01",
        service: 0x22,
        data: &[0x16, 0x3D, 0x01],
        node: Some(0x10),
    },
    Probe {
        label: "LLY actual fuel pressure 22163E01",
        service: 0x22,
        data: &[0x16, 0x3E, 0x01],
        node: Some(0x10),
    },
    Probe {
        label: "old spec fuel pressure actual 221170",
        service: 0x22,
        data: &[0x11, 0x70],
        node: Some(0x10),
    },
    Probe {
        label: "old spec fuel pressure desired 221171",
        service: 0x22,
        data: &[0x11, 0x71],
        node: Some(0x10),
    },
    Probe {
        label: "known-good VGT desired 22154001",
        service: 0x22,
        data: &[0x15, 0x40, 0x01],
        node: Some(0x10),
    },
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
    let mut evidence = GmEvidenceWriter::create_raw_capture("gm-pressure-probe")?;
    println!("evidence: {}", evidence.path().display());

    for probe in PROBES {
        let target = match probe.node {
            Some(node) => PhysicalTarget::Addressed(PhysicalAddress::J1850 {
                node,
                header: [0x6C, node, 0xF1],
            }),
            None => PhysicalTarget::Broadcast,
        };
        let request = RoutedRequest {
            service_id: probe.service,
            data: probe.data.to_vec(),
            target,
        };

        match adapter.routed_request(&request).await {
            Ok(bytes) => {
                let mut record =
                    evidence_record(&port, &protocol, probe).with_response_bytes(bytes.clone());
                if let Some(decoded) = decoded_json(probe, &bytes) {
                    record = record.with_decoded(decoded, confidence_label(probe));
                }
                evidence.append(&record)?;
                println!(
                    "{:<42} {:02X}{} -> {}",
                    probe.label,
                    probe.service,
                    hex_bytes(probe.data),
                    hex_bytes(&bytes)
                );
                print_decode(probe, &bytes);
            }
            Err(error) => {
                evidence.append(
                    &evidence_record(&port, &protocol, probe)
                        .with_error(GmEvidenceErrorKind::Adapter, error.to_string()),
                )?;
                println!(
                    "{:<42} {:02X}{} -> ERR {}",
                    probe.label,
                    probe.service,
                    hex_bytes(probe.data),
                    error
                );
            }
        }
    }

    drop(adapter);

    println!("\nraw header capture:");
    let mut transport = SerialTransport::new(&port, baud)?;
    for command in [
        "ATZ",
        "ATE0",
        "ATL0",
        "ATH1",
        "ATS1",
        "ATSP2",
        "AT SH 6C10F1",
        "22163D01",
        "22163E01",
        "22119D01",
        "22125101",
        "22154001",
        "AT SH 686AF1",
        "010B",
        "0133",
        "0123",
    ] {
        let response = raw_command(&mut transport, command).await?;
        println!("{command:<14} -> {}", response.replace('\r', "\\r"));
    }

    evidence.flush()?;
    Ok(())
}

async fn raw_command(
    transport: &mut SerialTransport,
    command: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut framed = String::with_capacity(command.len() + 1);
    framed.push_str(command);
    framed.push('\r');
    transport.write(framed.as_bytes()).await?;
    let bytes = transport.read().await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn print_decode(probe: &Probe, bytes: &[u8]) {
    match probe.label {
        "standard MAP 010B" | "standard BARO 0133" => {
            if let Some(value) = bytes.first() {
                println!(
                    "    {:.1} kPa / {:.1} psi",
                    value,
                    f64::from(*value) * 0.145_037_737_7
                );
            }
        }
        "standard fuel rail gauge 0123" | "standard fuel rail absolute 0159" => {
            if bytes.len() >= 2 {
                let raw = u16::from_be_bytes([bytes[0], bytes[1]]);
                let kpa = f64::from(raw) * 10.0;
                println!("    {:.0} kPa / {:.1} psi", kpa, kpa * 0.145_037_737_7);
            }
        }
        "GM VPW barometer V8 22119D01" | "GM VPW barometer V6 22125101" => {
            if let Some(value) = bytes.first() {
                println!(
                    "    {:.1} kPa / {:.1} psi",
                    value,
                    f64::from(*value) * 0.145_037_737_7
                );
            }
        }
        "LLY desired fuel pressure 22163D01" | "LLY actual fuel pressure 22163E01" => {
            if bytes.len() >= 2 {
                let raw = u16::from_be_bytes([bytes[0], bytes[1]]);
                println!(
                    "    raw={} / {:.1} psi",
                    raw,
                    f64::from(raw) * (145.0 / 256.0)
                );
            }
        }
        _ => {}
    }
}

fn evidence_record(port: &str, protocol: &str, probe: &Probe) -> GmEvidenceRecord {
    let module_label = if probe.node.is_some() {
        "ECM/PCM"
    } else {
        "broadcast"
    };
    let node = probe.node.unwrap_or(0);
    let header = probe
        .node
        .map(|node| [0x6C, node, 0xF1])
        .unwrap_or([0x00, 0x00, 0x00]);
    GmEvidenceRecord::routed_request_outcome(
        module_label,
        node,
        header,
        probe.service,
        probe.data.to_vec(),
        "gm-pressure-probe",
    )
    .with_adapter_context(Some(port.to_string()), Some(protocol.to_string()))
}

fn decoded_json(probe: &Probe, bytes: &[u8]) -> Option<GmDecodedEvidence> {
    match probe.label {
        "standard MAP 010B" | "standard BARO 0133" => {
            bytes.first().map(|value| GmDecodedEvidence::Value {
                signal: probe.label.to_string(),
                raw: u32::from(*value),
                value: f64::from(*value),
                unit: "kPa".to_string(),
            })
        }
        "standard fuel rail gauge 0123" | "standard fuel rail absolute 0159" => {
            if bytes.len() < 2 {
                return None;
            }
            let raw = u16::from_be_bytes([bytes[0], bytes[1]]);
            Some(GmDecodedEvidence::Value {
                signal: probe.label.to_string(),
                raw: u32::from(raw),
                value: f64::from(raw) * 10.0,
                unit: "kPa".to_string(),
            })
        }
        _ if probe.service == 0x22 && probe.data.len() >= 2 => {
            let did = u16::from_be_bytes([probe.data[0], probe.data[1]]);
            find_lly_did(did).and_then(|definition| {
                definition
                    .decode_value(bytes)
                    .ok()
                    .map(|decoded| GmDecodedEvidence::Value {
                        signal: definition.name.to_string(),
                        raw: decoded.selected_raw,
                        value: decoded.value,
                        unit: decoded.unit.to_string(),
                    })
            })
        }
        _ => None,
    }
}

fn confidence_label(probe: &Probe) -> Option<String> {
    if probe.service == 0x22 && probe.data.len() >= 2 {
        let did = u16::from_be_bytes([probe.data[0], probe.data[1]]);
        return find_lly_did(did).map(|definition| definition.confidence.as_str().to_string());
    }
    Some("verified".to_string())
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut out, "{byte:02X}").expect("write to string");
    }
    out
}
