use std::{env, time::Duration};

use obd2_core::adapter::elm327::Elm327Adapter;
use obd2_core::adapter::{Adapter, PhysicalTarget, RoutedRequest};
use obd2_core::transport::serial::SerialTransport;
use obd2_core::vehicle::PhysicalAddress;

const ECM_NODE: u8 = 0x10;
const MODE_22: u8 = 0x22;
const PSI_PER_KPA: f64 = 0.145_037_737_7;
const CANDIDATE_DIDS: &[u16] = &[
    0x1251, 0x147C, 0x153F, 0x1541, 0x1542, 0x1544, 0x1545, 0x1546,
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
    let samples = env::args()
        .nth(3)
        .and_then(|arg| arg.parse::<usize>().ok())
        .unwrap_or(12);

    let transport = SerialTransport::new(&port, baud)?;
    let mut adapter = Elm327Adapter::new(Box::new(transport));
    let report = adapter.initialize().await?;
    println!("protocol: {:?}", report.info.protocol);
    println!("sample,rpm,map_kpa,map_psi,baro_1251");
    println!("       did values are raw u8 bytes; pressure-looking values also equal kPa if decoded that way");
    print!("       ");
    for did in CANDIDATE_DIDS {
        print!(" {did:04X}");
    }
    println!();

    for sample in 0..samples {
        let rpm = read_rpm(&mut adapter).await;
        let map = read_u8_pid(&mut adapter, 0x0B).await;

        print!(
            "{sample:>6},{:>4},{:>7},{:>7}",
            rpm.map(|value| format!("{value:.0}"))
                .unwrap_or_else(|| "--".to_string()),
            map.map(|value| value.to_string())
                .unwrap_or_else(|| "--".to_string()),
            map.map(|value| format!("{:.1}", f64::from(value) * PSI_PER_KPA))
                .unwrap_or_else(|| "--".to_string())
        );

        for did in CANDIDATE_DIDS {
            match request_mode_22_u8(&mut adapter, *did).await {
                Ok(value) => print!(" {value:>4}"),
                Err(_) => print!("   --"),
            }
        }
        println!();

        tokio::time::sleep(Duration::from_millis(350)).await;
    }

    Ok(())
}

async fn read_rpm(adapter: &mut Elm327Adapter) -> Option<f64> {
    let request = RoutedRequest {
        service_id: 0x01,
        data: vec![0x0C],
        target: PhysicalTarget::Broadcast,
    };
    let bytes = adapter.routed_request(&request).await.ok()?;
    if bytes.len() < 2 {
        return None;
    }
    Some(f64::from(u16::from_be_bytes([bytes[0], bytes[1]])) / 4.0)
}

async fn read_u8_pid(adapter: &mut Elm327Adapter, pid: u8) -> Option<u8> {
    let request = RoutedRequest {
        service_id: 0x01,
        data: vec![pid],
        target: PhysicalTarget::Broadcast,
    };
    adapter
        .routed_request(&request)
        .await
        .ok()
        .and_then(|bytes| bytes.first().copied())
}

async fn request_mode_22_u8(adapter: &mut Elm327Adapter, did: u16) -> Result<u8, String> {
    let data = vec![(did >> 8) as u8, (did & 0xFF) as u8, 0x01];
    let request = RoutedRequest {
        service_id: MODE_22,
        data,
        target: PhysicalTarget::Addressed(PhysicalAddress::J1850 {
            node: ECM_NODE,
            header: [0x6C, ECM_NODE, 0xF1],
        }),
    };

    let bytes = adapter
        .routed_request(&request)
        .await
        .map_err(|error| error.to_string())?;
    bytes
        .first()
        .copied()
        .ok_or_else(|| format!("empty response for DID {did:04X}"))
}
