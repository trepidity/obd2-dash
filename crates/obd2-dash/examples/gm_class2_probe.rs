use std::env;

use obd2_core::adapter::elm327::Elm327Adapter;
use obd2_core::adapter::{Adapter, PhysicalTarget, RoutedRequest};
use obd2_core::transport::serial::SerialTransport;
use obd2_core::vehicle::PhysicalAddress;
use obd2_dash::gm_class2::{
    decode_class2_dtcs, hex_bytes, CLASS2_DTC_ACTIVE_REQUEST, CLASS2_DTC_ALL_REQUEST,
    DEFAULT_CLASS2_NODES, SERVICE_REPORT_DTCS_BY_STATUS,
};
use obd2_dash::gm_evidence::{
    GmDecodedEvidence, GmDtcEvidence, GmEvidenceErrorKind, GmEvidenceRecord, GmEvidenceWriter,
};

struct Probe {
    name: &'static str,
    service: u8,
    data: &'static [u8],
    decode_gm_class2: bool,
}

const PROBES: &[Probe] = &[
    Probe {
        name: "generic stored (03)",
        service: 0x03,
        data: &[],
        decode_gm_class2: false,
    },
    Probe {
        name: "Class2 $19 all DTCs (FF FF 00)",
        service: SERVICE_REPORT_DTCS_BY_STATUS,
        data: &CLASS2_DTC_ALL_REQUEST,
        decode_gm_class2: true,
    },
    Probe {
        name: "Class2 $19 active/history/current (92 FF 00)",
        service: SERVICE_REPORT_DTCS_BY_STATUS,
        data: &CLASS2_DTC_ACTIVE_REQUEST,
        decode_gm_class2: true,
    },
    Probe {
        name: "GMLAN $A9 fallback",
        service: 0xA9,
        data: &[0x81, 0xFF],
        decode_gm_class2: false,
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
    let mut evidence = GmEvidenceWriter::create_raw_capture("gm-class2-dtc")?;
    println!("evidence: {}", evidence.path().display());

    for node in DEFAULT_CLASS2_NODES {
        println!("\n[node {:02X}  {}]", node.node, node.label);
        for probe in PROBES {
            let request = RoutedRequest {
                service_id: probe.service,
                data: probe.data.to_vec(),
                target: PhysicalTarget::Addressed(PhysicalAddress::J1850 {
                    node: node.node,
                    header: [0x6C, node.node, 0xF1],
                }),
            };

            match adapter.routed_request(&request).await {
                Ok(bytes) => {
                    let mut record = GmEvidenceRecord::routed_request_outcome(
                        node.label,
                        node.node,
                        [0x6C, node.node, 0xF1],
                        probe.service,
                        probe.data.to_vec(),
                        if probe.decode_gm_class2 {
                            "gm-class2-dtc"
                        } else {
                            "raw-routed"
                        },
                    )
                    .with_adapter_context(Some(port.clone()), Some(protocol.clone()))
                    .with_response_bytes(bytes.clone());
                    println!(
                        "  {:02X}{} {:<44} -> {}",
                        probe.service,
                        hex_bytes(probe.data),
                        probe.name,
                        hex_bytes(&bytes)
                    );
                    if probe.decode_gm_class2 {
                        match decode_class2_dtcs(&bytes) {
                            Ok(records) if records.is_empty() => {
                                record = record.with_decoded(
                                    GmDecodedEvidence::Empty,
                                    Some("candidate".to_string()),
                                );
                                println!("      decoded: empty");
                            }
                            Ok(records) => {
                                let decoded = records
                                    .iter()
                                    .map(|record| GmDtcEvidence {
                                        code: record.dtc.code.clone(),
                                        gm_status_raw: record.status.raw,
                                        gm_status_flags: record
                                            .status
                                            .labels()
                                            .into_iter()
                                            .map(str::to_string)
                                            .collect(),
                                    })
                                    .collect::<Vec<_>>();
                                record = record.with_decoded(
                                    GmDecodedEvidence::Dtcs { records: decoded },
                                    Some("candidate".to_string()),
                                );
                                for record in records {
                                    println!(
                                        "      decoded: {} status=0x{:02X} {}",
                                        record.dtc.code,
                                        record.status.raw,
                                        record.status.display_flags()
                                    );
                                }
                            }
                            Err(err) => {
                                record =
                                    record.with_error(GmEvidenceErrorKind::Decode, err.to_string());
                                println!("      decode-error: {err}");
                            }
                        }
                    }
                    evidence.append(&record)?;
                }
                Err(err) => {
                    evidence.append(
                        &GmEvidenceRecord::routed_request_outcome(
                            node.label,
                            node.node,
                            [0x6C, node.node, 0xF1],
                            probe.service,
                            probe.data.to_vec(),
                            if probe.decode_gm_class2 {
                                "gm-class2-dtc"
                            } else {
                                "raw-routed"
                            },
                        )
                        .with_adapter_context(Some(port.clone()), Some(protocol.clone()))
                        .with_error(GmEvidenceErrorKind::Adapter, err.to_string()),
                    )?;
                    println!(
                        "  {:02X}{} {:<44} -> ERR {}",
                        probe.service,
                        hex_bytes(probe.data),
                        probe.name,
                        err
                    );
                }
            }
        }
    }

    evidence.flush()?;
    Ok(())
}
