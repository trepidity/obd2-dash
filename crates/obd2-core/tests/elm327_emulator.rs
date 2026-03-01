//! Integration test that spawns the Ircama ELM327-emulator with the Malibu 2020
//! scenario and exercises the full `Transport → Elm327 → init → VIN → PIDs → DTCs`
//! pipeline against it.
//!
//! Uses TCP mode (`-n PORT`) to avoid macOS PTY ioctl issues with tokio-serial.
//! A test-local `TcpTransport` implements the same `Transport` trait that
//! `SerialTransport` does, so `Elm327` is fully exercised.
//!
//! Gated with `#[ignore]` so `cargo test` doesn't require Python by default.
//! Run explicitly:
//!   cargo test -p obd2-core --test elm327_emulator -- --ignored

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

use obd2_core::obd2::elm327::Elm327;
use obd2_core::obd2::transport::Transport;
use obd2_core::{Obd2Connection, Obd2Error, Pid};

// ── TcpTransport: same contract as SerialTransport, over TCP ────────────────

const READ_TIMEOUT: Duration = Duration::from_secs(5);

struct TcpTransport {
    reader: BufReader<TcpStream>,
}

impl TcpTransport {
    fn new(stream: TcpStream) -> Self {
        Self {
            reader: BufReader::new(stream),
        }
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn send(&mut self, data: &[u8]) -> Result<(), Obd2Error> {
        let writer = self.reader.get_mut();
        writer.write_all(data).await.map_err(Obd2Error::Io)?;
        writer.flush().await.map_err(Obd2Error::Io)?;
        Ok(())
    }

    async fn read_line(&mut self) -> Result<String, Obd2Error> {
        let mut buf = Vec::new();
        loop {
            let byte = match tokio::time::timeout(READ_TIMEOUT, self.reader.read_u8()).await {
                Ok(result) => result.map_err(Obd2Error::Io)?,
                Err(_) => return Err(Obd2Error::Timeout),
            };
            match byte {
                b'\r' => break,
                b'\n' => continue,
                b'>' => {
                    buf.push(b'>');
                    break;
                }
                other => buf.push(other),
            }
        }
        Ok(String::from_utf8_lossy(&buf).to_string())
    }
}

// ── Test helpers ────────────────────────────────────────────────────────────

/// Locate the emulator scenario directory relative to the workspace root.
fn scenario_dir() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.join("..").join("..").join("emulator")
}

/// Check that the ELM327-emulator Python package is installed.
fn emulator_available() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import elm"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Find an available TCP port.
fn available_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().unwrap().port()
}

// ── The test ────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_full_elm327_pipeline() {
    if !emulator_available() {
        eprintln!("SKIP: ELM327-emulator not installed (pip install ELM327-emulator)");
        return;
    }

    let emu_dir = scenario_dir();
    let port = available_port();
    let batch_out = std::env::temp_dir().join(format!("elm_test_{}.txt", std::process::id()));

    // Spawn emulator with both -b (batch) and -n (TCP) flags.
    // Batch mode processes stdin commands (merge + scenario) before entering
    // the "alive" state. TCP server starts at construction time.
    // We wait for "End of batch commands." in the output file to know
    // the scenario is loaded before we connect.
    let mut child = tokio::process::Command::new("python3")
        .args([
            "-m",
            "elm",
            "-n",
            &port.to_string(),
            "-b",
            batch_out.to_str().unwrap(),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .current_dir(&emu_dir)
        .spawn()
        .expect("failed to spawn ELM327-emulator");

    // Write merge + scenario commands to stdin, then close it so cmdloop exits batch mode.
    {
        let stdin = child.stdin.as_mut().expect("no stdin");
        stdin
            .write_all(b"merge malibu_2020\nscenario malibu_2020\n")
            .await
            .expect("write to stdin");
        stdin.flush().await.expect("flush stdin");
    }
    // Close stdin to signal end of batch input
    child.stdin.take();

    // Wait for the batch output file to signal completion
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(contents) = tokio::fs::read_to_string(&batch_out).await {
                if contents.contains("End of batch commands.") {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("emulator did not finish batch commands within 10 seconds");

    eprintln!("Emulator ready on TCP port {}", port);

    // Connect via TCP
    let addr = format!("127.0.0.1:{}", port);
    let stream = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match TcpStream::connect(&addr).await {
                Ok(s) => return s,
                Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
            }
        }
    })
    .await
    .expect("could not connect to emulator TCP port");

    eprintln!("Connected to emulator at {}", addr);

    let transport = TcpTransport::new(stream);
    let mut conn: Box<dyn Obd2Connection> = Box::new(Elm327::new(Box::new(transport)));

    // ── Initialize ───────────────────────────────────────────────────────────
    conn.initialize()
        .await
        .expect("ELM327 initialization failed");
    eprintln!("PASS: initialize()");

    // ── Read VIN ─────────────────────────────────────────────────────────────
    let vin = conn.read_vin().await.expect("read_vin failed");
    assert_eq!(vin, "1G1ZD5ST2LF000000", "VIN mismatch");
    eprintln!("PASS: read_vin() = {}", vin);

    // ── Query PIDs ───────────────────────────────────────────────────────────
    let rpm = conn
        .query_pid(Pid::EngineRpm)
        .await
        .expect("RPM query failed");
    assert!(
        (rpm.value - 800.0).abs() < 50.0,
        "RPM {} not near 800",
        rpm.value
    );
    eprintln!("PASS: EngineRpm = {:.1} {}", rpm.value, rpm.unit);

    let coolant = conn
        .query_pid(Pid::CoolantTemp)
        .await
        .expect("CoolantTemp query failed");
    assert!(
        (coolant.value - 90.0).abs() < 5.0,
        "CoolantTemp {} not near 90",
        coolant.value
    );
    eprintln!("PASS: CoolantTemp = {:.1} {}", coolant.value, coolant.unit);

    let speed = conn
        .query_pid(Pid::VehicleSpeed)
        .await
        .expect("Speed query failed");
    assert!(
        speed.value.abs() < 1.0,
        "Speed {} should be ~0",
        speed.value
    );
    eprintln!("PASS: VehicleSpeed = {:.1} {}", speed.value, speed.unit);

    let load = conn
        .query_pid(Pid::EngineLoad)
        .await
        .expect("EngineLoad query failed");
    assert!(
        (load.value - 20.0).abs() < 5.0,
        "EngineLoad {} not near 20",
        load.value
    );
    eprintln!("PASS: EngineLoad = {:.1} {}", load.value, load.unit);

    // ── Read Voltage ─────────────────────────────────────────────────────────
    let voltage = conn.read_voltage().await.expect("read_voltage failed");
    assert!(
        (voltage - 12.6).abs() < 0.5,
        "Voltage {} not near 12.6",
        voltage
    );
    eprintln!("PASS: read_voltage() = {:.1}V", voltage);

    // ── Read DTCs ────────────────────────────────────────────────────────────
    let dtcs = conn.read_dtcs().await.expect("read_dtcs failed");
    let dtc_codes: Vec<&str> = dtcs.iter().map(|d| d.code.as_str()).collect();
    assert!(
        dtc_codes.contains(&"P0171"),
        "DTCs {:?} missing P0171",
        dtc_codes
    );
    assert!(
        dtc_codes.contains(&"P0420"),
        "DTCs {:?} missing P0420",
        dtc_codes
    );
    eprintln!("PASS: read_dtcs() = {:?}", dtc_codes);

    // ── Cleanup ──────────────────────────────────────────────────────────────
    let _ = child.kill().await;
    let _ = tokio::fs::remove_file(&batch_out).await;

    eprintln!("ALL TESTS PASSED");
}
