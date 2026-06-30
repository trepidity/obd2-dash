use std::{
    env,
    fs::{create_dir_all, File},
    io::{BufWriter, Write},
    time::{Duration, Instant},
};

use chrono::Local;
use obd2_core::adapter::elm327::Elm327Adapter;
use obd2_core::adapter::{Adapter, PhysicalTarget, RoutedRequest};
use obd2_core::transport::serial::SerialTransport;
use obd2_core::vehicle::PhysicalAddress;
use obd2_dash::gm_evidence::{GmEvidenceErrorKind, GmEvidenceRecord, GmEvidenceWriter};

const ECM_NODE: u8 = 0x10;
const MODE_22: u8 = 0x22;
const PSI_PER_KPA: f64 = 0.145_037_737_7;
const MPH_PER_KPH: f64 = 0.621_371;
const FUEL_PRESSURE_PSI_SCALE: f64 = 145.0 / 256.0;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port = env::args()
        .nth(1)
        .unwrap_or_else(|| "/dev/cu.usbserial-223230360830".to_string());
    let baud = env::args()
        .nth(2)
        .and_then(|arg| arg.parse::<u32>().ok())
        .unwrap_or(115_200);
    let samples = env::args()
        .nth(3)
        .and_then(|arg| arg.parse::<usize>().ok())
        .unwrap_or(240);

    create_dir_all("raw-captures")?;
    let path = format!(
        "raw-captures/gm-drive-{}.csv",
        Local::now().format("%Y%m%d-%H%M%S")
    );
    let mut csv = BufWriter::new(File::create(&path)?);
    let mut evidence = GmEvidenceWriter::create_raw_capture("gm-drive-evidence")?;

    let transport = SerialTransport::new(&port, baud)?;
    let mut adapter = Elm327Adapter::new(Box::new(transport));
    let report = adapter.initialize().await?;
    println!("protocol: {:?}", report.info.protocol);
    let protocol = format!("{:?}", report.info.protocol);
    println!("logging to {path}");
    println!("evidence: {}", evidence.path().display());
    println!("READY: start driving now. Do the pulls when safe.");

    writeln!(
        csv,
        "elapsed_ms,rpm,speed_mph,map_psi,baro_psi,boost_psi,desired_map_psi,desired_boost_psi,maf_g_s,fuel_actual_psi,fuel_desired_psi,fuel_delta_psi,vgt_actual_pct,vgt_desired_pct,vgt_error_pct,coolant_f,iat_f"
    )?;

    let start = Instant::now();
    for _ in 0..samples {
        let row = sample(&mut adapter, &mut evidence, &port, &protocol).await;
        let elapsed_ms = start.elapsed().as_millis();

        writeln!(
            csv,
            "{elapsed_ms},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            fmt(row.rpm),
            fmt(row.speed_mph),
            fmt(row.map_psi),
            fmt(row.baro_psi),
            fmt(row.boost_psi),
            fmt(row.desired_map_psi),
            fmt(row.desired_boost_psi),
            fmt(row.maf_g_s),
            fmt(row.fuel_actual_psi),
            fmt(row.fuel_desired_psi),
            fmt(row.fuel_delta_psi),
            fmt(row.vgt_actual_pct),
            fmt(row.vgt_desired_pct),
            fmt(row.vgt_error_pct),
            fmt(row.coolant_f),
            fmt(row.iat_f),
        )?;
        csv.flush()?;

        println!(
            "{:>6}ms rpm={} mph={} boost={} des_boost={} maf={} rail={}/{} vgt={}/{}",
            elapsed_ms,
            fmt(row.rpm),
            fmt(row.speed_mph),
            fmt(row.boost_psi),
            fmt(row.desired_boost_psi),
            fmt(row.maf_g_s),
            fmt(row.fuel_actual_psi),
            fmt(row.fuel_desired_psi),
            fmt(row.vgt_actual_pct),
            fmt(row.vgt_desired_pct),
        );

        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    evidence.flush()?;
    println!("done logging to {path}");
    Ok(())
}

#[derive(Debug, Default)]
struct DriveSample {
    rpm: Option<f64>,
    speed_mph: Option<f64>,
    map_psi: Option<f64>,
    baro_psi: Option<f64>,
    boost_psi: Option<f64>,
    desired_map_psi: Option<f64>,
    desired_boost_psi: Option<f64>,
    maf_g_s: Option<f64>,
    fuel_actual_psi: Option<f64>,
    fuel_desired_psi: Option<f64>,
    fuel_delta_psi: Option<f64>,
    vgt_actual_pct: Option<f64>,
    vgt_desired_pct: Option<f64>,
    vgt_error_pct: Option<f64>,
    coolant_f: Option<f64>,
    iat_f: Option<f64>,
}

async fn sample(
    adapter: &mut Elm327Adapter,
    evidence: &mut GmEvidenceWriter,
    port: &str,
    protocol: &str,
) -> DriveSample {
    let rpm = read_pid_u16(adapter, 0x0C, evidence, port, protocol)
        .await
        .map(|raw| f64::from(raw) / 4.0);
    let speed_mph = read_pid_u8(adapter, 0x0D, evidence, port, protocol)
        .await
        .map(|value| f64::from(value) * MPH_PER_KPH);
    let map_kpa = read_pid_u8(adapter, 0x0B, evidence, port, protocol)
        .await
        .map(f64::from);
    let baro_kpa = read_mode22_u8(adapter, 0x1251, evidence, port, protocol)
        .await
        .map(f64::from);
    let desired_map_kpa = read_mode22_u8(adapter, 0x1542, evidence, port, protocol)
        .await
        .map(f64::from);
    let maf_g_s = read_pid_u16(adapter, 0x10, evidence, port, protocol)
        .await
        .map(|raw| f64::from(raw) / 100.0);
    let fuel_actual_psi = read_pid_u16(adapter, 0x23, evidence, port, protocol)
        .await
        .map(|raw| f64::from(raw) * 10.0 * PSI_PER_KPA);
    let fuel_desired_psi = read_mode22_u16(adapter, 0x163D, evidence, port, protocol)
        .await
        .map(|raw| f64::from(raw) * FUEL_PRESSURE_PSI_SCALE);
    let vgt_actual_pct = read_mode22_u8(adapter, 0x1543, evidence, port, protocol)
        .await
        .map(|raw| f64::from(raw) * 100.0 / 255.0);
    let vgt_desired_pct = read_mode22_u8(adapter, 0x1540, evidence, port, protocol)
        .await
        .map(|raw| f64::from(raw) * 100.0 / 255.0);
    let coolant_f = read_pid_u8(adapter, 0x05, evidence, port, protocol)
        .await
        .map(|raw| c_to_f(f64::from(raw) - 40.0));
    let iat_f = read_pid_u8(adapter, 0x0F, evidence, port, protocol)
        .await
        .map(|raw| c_to_f(f64::from(raw) - 40.0));

    let map_psi = map_kpa.map(|value| value * PSI_PER_KPA);
    let baro_psi = baro_kpa.map(|value| value * PSI_PER_KPA);
    let boost_psi = match (map_psi, baro_psi) {
        (Some(map), Some(baro)) => Some(map - baro),
        _ => None,
    };
    let desired_map_psi = desired_map_kpa.map(|value| value * PSI_PER_KPA);
    let desired_boost_psi = match (desired_map_psi, baro_psi) {
        (Some(map), Some(baro)) => Some(map - baro),
        _ => None,
    };
    let fuel_delta_psi = match (fuel_actual_psi, fuel_desired_psi) {
        (Some(actual), Some(desired)) => Some(actual - desired),
        _ => None,
    };
    let vgt_error_pct = match (vgt_actual_pct, vgt_desired_pct) {
        (Some(actual), Some(desired)) => Some(actual - desired),
        _ => None,
    };

    DriveSample {
        rpm,
        speed_mph,
        map_psi,
        baro_psi,
        boost_psi,
        desired_map_psi,
        desired_boost_psi,
        maf_g_s,
        fuel_actual_psi,
        fuel_desired_psi,
        fuel_delta_psi,
        vgt_actual_pct,
        vgt_desired_pct,
        vgt_error_pct,
        coolant_f,
        iat_f,
    }
}

async fn read_pid_u8(
    adapter: &mut Elm327Adapter,
    pid: u8,
    evidence: &mut GmEvidenceWriter,
    port: &str,
    protocol: &str,
) -> Option<u8> {
    read_request(
        adapter,
        0x01,
        &[pid],
        PhysicalTarget::Broadcast,
        evidence,
        port,
        protocol,
    )
    .await
    .and_then(|bytes| bytes.first().copied())
}

async fn read_pid_u16(
    adapter: &mut Elm327Adapter,
    pid: u8,
    evidence: &mut GmEvidenceWriter,
    port: &str,
    protocol: &str,
) -> Option<u16> {
    read_request(
        adapter,
        0x01,
        &[pid],
        PhysicalTarget::Broadcast,
        evidence,
        port,
        protocol,
    )
    .await
    .and_then(|bytes| {
        if bytes.len() >= 2 {
            Some(u16::from_be_bytes([bytes[0], bytes[1]]))
        } else {
            None
        }
    })
}

async fn read_mode22_u8(
    adapter: &mut Elm327Adapter,
    did: u16,
    evidence: &mut GmEvidenceWriter,
    port: &str,
    protocol: &str,
) -> Option<u8> {
    read_mode22(adapter, did, evidence, port, protocol)
        .await
        .and_then(|bytes| bytes.first().copied())
}

async fn read_mode22_u16(
    adapter: &mut Elm327Adapter,
    did: u16,
    evidence: &mut GmEvidenceWriter,
    port: &str,
    protocol: &str,
) -> Option<u16> {
    read_mode22(adapter, did, evidence, port, protocol)
        .await
        .and_then(|bytes| {
            if bytes.len() >= 2 {
                Some(u16::from_be_bytes([bytes[0], bytes[1]]))
            } else {
                None
            }
        })
}

async fn read_mode22(
    adapter: &mut Elm327Adapter,
    did: u16,
    evidence: &mut GmEvidenceWriter,
    port: &str,
    protocol: &str,
) -> Option<Vec<u8>> {
    let data = [(did >> 8) as u8, (did & 0xFF) as u8, 0x01];
    read_request(
        adapter,
        MODE_22,
        &data,
        PhysicalTarget::Addressed(PhysicalAddress::J1850 {
            node: ECM_NODE,
            header: [0x6C, ECM_NODE, 0xF1],
        }),
        evidence,
        port,
        protocol,
    )
    .await
}

async fn read_request(
    adapter: &mut Elm327Adapter,
    service_id: u8,
    data: &[u8],
    target: PhysicalTarget,
    evidence: &mut GmEvidenceWriter,
    port: &str,
    protocol: &str,
) -> Option<Vec<u8>> {
    let request = RoutedRequest {
        service_id,
        data: data.to_vec(),
        target: target.clone(),
    };
    let record = evidence_record(port, protocol, service_id, data, &target);
    match adapter.routed_request(&request).await {
        Ok(bytes) => {
            let _ = evidence.append(&record.with_response_bytes(bytes.clone()));
            Some(bytes)
        }
        Err(error) => {
            let _ = evidence
                .append(&record.with_error(GmEvidenceErrorKind::Adapter, error.to_string()));
            None
        }
    }
}

fn evidence_record(
    port: &str,
    protocol: &str,
    service_id: u8,
    data: &[u8],
    target: &PhysicalTarget,
) -> GmEvidenceRecord {
    let (module_label, node) = match target {
        PhysicalTarget::Broadcast => ("broadcast", 0),
        PhysicalTarget::Addressed(PhysicalAddress::J1850 { node, .. }) => ("ECM/PCM", *node),
        PhysicalTarget::Addressed(_) => ("addressed", 0),
    };
    GmEvidenceRecord::routed_request_outcome(
        module_label,
        node,
        match target {
            PhysicalTarget::Addressed(PhysicalAddress::J1850 { header, .. }) => *header,
            _ => [0x00, 0x00, 0x00],
        },
        service_id,
        data.to_vec(),
        "gm-drive-logger",
    )
    .with_adapter_context(Some(port.to_string()), Some(protocol.to_string()))
}

fn c_to_f(celsius: f64) -> f64 {
    celsius * 9.0 / 5.0 + 32.0
}

fn fmt(value: Option<f64>) -> String {
    value.map(|value| format!("{value:.1}")).unwrap_or_default()
}
