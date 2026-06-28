#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;

#[derive(Debug, Serialize)]
struct StatusValue {
    label: &'static str,
    value: &'static str,
    state: &'static str,
}

#[derive(Debug, Serialize)]
struct CylinderBalance {
    cylinder: u8,
    mm3: f32,
}

#[derive(Debug, Serialize)]
struct ModuleScan {
    module: &'static str,
    stored: &'static str,
    pending: &'static str,
    permanent: &'static str,
}

#[derive(Debug, Serialize)]
struct TemperatureSnapshot {
    coolant_f: f32,
    intake_air_f: f32,
    oil_f: Option<f32>,
    trans_f: Option<f32>,
    ambient_f: Option<f32>,
}

#[derive(Debug, Serialize)]
struct FuelRailSnapshot {
    actual_psi: f32,
    desired_psi: Option<f32>,
    delta_psi: Option<f32>,
}

#[derive(Debug, Serialize)]
struct VgtSnapshot {
    actual_pct: f32,
    desired_pct: f32,
    error_pct: f32,
}

#[derive(Debug, Serialize)]
struct DiagnosticSnapshot {
    vehicle: &'static str,
    vin: &'static str,
    protocol: &'static str,
    connection: &'static str,
    voltage: f32,
    rpm: u16,
    speed_mph: u16,
    poll_ms: u16,
    units: &'static str,
    statuses: Vec<StatusValue>,
    alerts: Vec<&'static str>,
    modules: Vec<ModuleScan>,
    cylinders: Vec<CylinderBalance>,
    vgt: VgtSnapshot,
    fuel_rail: FuelRailSnapshot,
    temperatures: TemperatureSnapshot,
    map_psi: f32,
    boost_psi: f32,
    maf_lb_min: f32,
}

#[tauri::command]
fn diagnostic_snapshot() -> DiagnosticSnapshot {
    DiagnosticSnapshot {
        vehicle: "2004 GMC Sierra",
        vin: "1GTHK29294E391526",
        protocol: "J1850 VPW",
        connection: "mock",
        voltage: 13.8,
        rpm: 685,
        speed_mph: 0,
        poll_ms: 250,
        units: "US",
        statuses: vec![
            StatusValue {
                label: "DTCs",
                value: "0",
                state: "ok",
            },
            StatusValue {
                label: "ECUs",
                value: "5",
                state: "ok",
            },
            StatusValue {
                label: "MIL",
                value: "OFF",
                state: "ok",
            },
            StatusValue {
                label: "Record",
                value: "armed",
                state: "warn",
            },
        ],
        alerts: vec![
            "Desired fuel rail PID not verified on this ECM",
            "TCM enhanced DTC decoder pending live 59 payload",
        ],
        modules: vec![
            ModuleScan {
                module: "ECM",
                stored: "empty",
                pending: "empty",
                permanent: "unsup",
            },
            ModuleScan {
                module: "TCM",
                stored: "unsup",
                pending: "unsup",
                permanent: "unsup",
            },
            ModuleScan {
                module: "EBCM",
                stored: "no data",
                pending: "no data",
                permanent: "no data",
            },
            ModuleScan {
                module: "BCM",
                stored: "unsup",
                pending: "unsup",
                permanent: "unsup",
            },
            ModuleScan {
                module: "IPC",
                stored: "probe",
                pending: "probe",
                permanent: "probe",
            },
        ],
        cylinders: vec![
            CylinderBalance { cylinder: 1, mm3: 0.3 },
            CylinderBalance { cylinder: 2, mm3: -0.3 },
            CylinderBalance { cylinder: 3, mm3: -1.3 },
            CylinderBalance { cylinder: 4, mm3: -0.4 },
            CylinderBalance { cylinder: 5, mm3: -0.3 },
            CylinderBalance { cylinder: 6, mm3: 0.2 },
            CylinderBalance { cylinder: 7, mm3: 1.0 },
            CylinderBalance { cylinder: 8, mm3: 0.5 },
        ],
        vgt: VgtSnapshot {
            actual_pct: 88.2,
            desired_pct: 88.2,
            error_pct: 0.0,
        },
        fuel_rail: FuelRailSnapshot {
            actual_psi: 4260.0,
            desired_psi: None,
            delta_psi: None,
        },
        temperatures: TemperatureSnapshot {
            coolant_f: 170.6,
            intake_air_f: 91.4,
            oil_f: None,
            trans_f: None,
            ambient_f: None,
        },
        map_psi: 13.9,
        boost_psi: 0.0,
        maf_lb_min: 5.2,
    }
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![diagnostic_snapshot])
        .run(tauri::generate_context!())
        .expect("failed to run OBD2 Dash GUI");
}
