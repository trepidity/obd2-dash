#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::{HashMap, HashSet},
    env,
};

use obd2_core::{
    adapter::{elm327::Elm327Adapter, Adapter},
    error::{NegativeResponse, Obd2Error},
    protocol::{dtc::Dtc, enhanced::Value, pid::Pid},
    session::Session,
    transport::{serial, LoggingTransport},
    vehicle::{ModuleId, PhysicalAddress, Protocol},
};
use obd2_dash::gm_active::{
    active_test_evidence_record, blocked_active_test_result, vgt_vane_control_definition,
    GmActiveTestCommand, GmActiveTestDefinition, GmActiveTestPrecondition, GmActiveTestResult,
};
use obd2_dash::gm_evidence::{GmEvidenceWriter, GmVehicleIdentity};
use obd2_dash::profiles::{
    acquire_identity, build_vehicle_context, next_generation, select_into_state,
    validate_vin_charset, CapabilityId, Confidence as ProfileConfidence, CoverageMap, DecodedDtc,
    DispatchError, DispatchEvidence, IdentityConfidence, IdentityOutcome, ModuleKey,
    ProfileDecodeError, ProfileEvidenceSink, ProfileRegistry, ProfileResponse, ProfileRuntime,
    ProfileState, Provenance as ProfileProvenance, RequestId, SelectedProfile, SignalDefinition,
    VehicleContext,
};
use serde::Serialize;
use tauri::State;
use tokio::{sync::Mutex, time::Duration};

const DEFAULT_BAUD: u32 = 115_200;
const POLL_MS: u16 = 250;
const GUI_EXTRA_VIN_READS: u8 = 2;
const LLY_PROFILE_ID: &str = "gm.gmt800.lly.class2";
const PROFILE_DTC_SCAN_CYCLE: u64 = 60;
const PSI_PER_KPA: f64 = 0.145_037_737_7;
const MPH_PER_KPH: f64 = 0.621_371;
const LLY_DESIRED_FUEL_PRESSURE_DID: u16 = 0x163D;
const LLY_ACTUAL_FUEL_PRESSURE_DID: u16 = 0x163E;
const LLY_BAROMETRIC_PRESSURE_DID: u16 = 0x1251;
// Live-probed on the 2004.5 LLY ECM. Public VPW docs for this DID were not found.
const LLY_DESIRED_MAP_DID: u16 = 0x1542;

#[derive(Debug, Clone, Serialize)]
struct StatusValue {
    label: String,
    value: String,
    state: String,
}

#[derive(Debug, Clone, Serialize)]
struct CylinderBalance {
    cylinder: u8,
    mm3: f32,
}

#[derive(Debug, Clone, Serialize)]
struct ModuleScan {
    module: String,
    standard: String,
    gm_all: String,
    gm_active: String,
}

#[derive(Debug, Clone, Serialize)]
struct DtcSnapshot {
    code: String,
    module: String,
    status: String,
    description: Option<String>,
    notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SignalEvidence {
    key: String,
    label: String,
    module: String,
    node: String,
    request: String,
    source: String,
    confidence: String,
    status: String,
    unit: String,
    value: Option<f32>,
    response: Option<String>,
    notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct TemperatureSnapshot {
    coolant_f: f32,
    intake_air_f: f32,
    oil_f: Option<f32>,
    trans_f: Option<f32>,
    ambient_f: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
struct FuelRailSnapshot {
    actual_psi: f32,
    desired_psi: Option<f32>,
    delta_psi: Option<f32>,
}

#[derive(Debug, Clone, Serialize)]
struct VgtSnapshot {
    actual_pct: f32,
    desired_pct: f32,
    error_pct: f32,
}

#[derive(Debug, Clone, Serialize)]
struct VgtActiveTestSnapshot {
    definition: GmActiveTestDefinition,
    preconditions: Vec<GmActiveTestPrecondition>,
    last_result: Option<GmActiveTestResult>,
}

#[derive(Debug, Clone, Serialize)]
struct ActiveTestsSnapshot {
    vgt_vane: VgtActiveTestSnapshot,
}

#[derive(Debug, Clone, Serialize)]
struct DiagnosticSnapshot {
    vehicle: String,
    vin: String,
    protocol: String,
    connection: String,
    voltage: f32,
    rpm: u16,
    speed_mph: u16,
    poll_ms: u16,
    units: String,
    statuses: Vec<StatusValue>,
    alerts: Vec<String>,
    dtcs: Vec<DtcSnapshot>,
    modules: Vec<ModuleScan>,
    cylinders: Vec<CylinderBalance>,
    vgt: VgtSnapshot,
    fuel_rail: FuelRailSnapshot,
    temperatures: TemperatureSnapshot,
    map_psi: f32,
    desired_map_psi: Option<f32>,
    barometric_psi: Option<f32>,
    boost_psi: f32,
    maf_g_s: f32,
    source_confidence: Vec<SignalEvidence>,
    active_tests: ActiveTestsSnapshot,
}

struct GuiState {
    backend: Mutex<LiveBackend>,
}

struct LiveBackend {
    session: Option<Session<Elm327Adapter>>,
    port: Option<String>,
    baud: u32,
    vehicle: String,
    vin: String,
    last: DiagnosticSnapshot,
    cached_dtc_count: Option<usize>,
    cached_dtcs: Vec<DtcSnapshot>,
    cached_modules: Vec<ModuleScan>,
    profile_state: ProfileState,
    identity: IdentityOutcome,
    profile_context: Option<GuiProfileContextFingerprint>,
    last_active_test_result: Option<GmActiveTestResult>,
    cycle: u64,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct GuiProfileContextFingerprint {
    protocol: Protocol,
    vin: Option<String>,
    vin_confidence: IdentityConfidence,
    spec_identity: Option<String>,
    active_bus: Option<String>,
    discovered_modules: Vec<String>,
}

impl Default for LiveBackend {
    fn default() -> Self {
        Self {
            session: None,
            port: None,
            baud: configured_baud(),
            vehicle: "Live OBD-II".to_string(),
            vin: "--".to_string(),
            last: empty_snapshot("disconnected"),
            cached_dtc_count: None,
            cached_dtcs: Vec::new(),
            cached_modules: Vec::new(),
            profile_state: ProfileState::default(),
            identity: IdentityOutcome {
                vin: None,
                confidence: IdentityConfidence::Unread,
            },
            profile_context: None,
            last_active_test_result: None,
            cycle: 0,
        }
    }
}

impl LiveBackend {
    async fn snapshot(&mut self) -> DiagnosticSnapshot {
        match self.try_snapshot().await {
            Ok(snapshot) => {
                self.last = snapshot.clone();
                snapshot
            }
            Err(error) => {
                self.session = None;
                let mut snapshot = self.last.clone();
                snapshot.connection = format!("live error: {error}");
                snapshot.alerts = vec![format!("Live serial error: {error}")];
                snapshot.statuses = status_values(self.cached_dtc_count.unwrap_or(0), 0, false);
                snapshot
            }
        }
    }

    async fn try_snapshot(&mut self) -> Result<DiagnosticSnapshot, String> {
        if self.session.is_none() {
            self.connect().await?;
        }
        self.refresh_profile_selection_if_needed();

        let cycle = self.cycle;
        self.cycle = self.cycle.wrapping_add(1);

        let port = self.port.clone().unwrap_or_else(|| "unknown".to_string());
        let vehicle = self.vehicle.clone();
        let vin = self.vin.clone();
        let selected_lly = self.selected_lly_profile();
        let profile_context = self
            .session
            .as_ref()
            .and_then(|session| selected_lly.as_ref().map(|_| self.profile_context(session)));
        let gm_lly_enabled = selected_lly.is_some();
        if !gm_lly_enabled {
            self.cached_dtc_count = None;
            self.cached_dtcs.clear();
            self.cached_modules.clear();
        }
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| "live session missing after connect".to_string())?;

        let mut alerts = Vec::new();
        let mut source_confidence = Vec::new();
        let voltage = match session.battery_voltage().await {
            Ok(Some(value)) => value,
            Ok(None) => f64::from(self.last.voltage),
            Err(error) => {
                push_unexpected_error(&mut alerts, "Battery voltage", &error);
                f64::from(self.last.voltage)
            }
        };

        let rpm = read_scalar_pid(session, Pid::ENGINE_RPM, &mut alerts, "Engine RPM")
            .await
            .unwrap_or(f64::from(self.last.rpm));
        let speed_kph = read_scalar_pid(session, Pid::VEHICLE_SPEED, &mut alerts, "Vehicle speed")
            .await
            .unwrap_or(f64::from(self.last.speed_mph) / MPH_PER_KPH);
        let coolant_c = read_scalar_pid(
            session,
            Pid::COOLANT_TEMP,
            &mut alerts,
            "Coolant temperature",
        )
        .await
        .unwrap_or(f_to_c(f64::from(self.last.temperatures.coolant_f)));
        let intake_air_c = read_scalar_pid(
            session,
            Pid::INTAKE_AIR_TEMP,
            &mut alerts,
            "Intake air temperature",
        )
        .await
        .unwrap_or(f_to_c(f64::from(self.last.temperatures.intake_air_f)));
        let map_kpa = read_scalar_pid(session, Pid::INTAKE_MAP, &mut alerts, "Intake MAP")
            .await
            .unwrap_or(f64::from(self.last.map_psi) / PSI_PER_KPA);
        let baro_kpa = read_optional_scalar_pid(session, Pid::BAROMETRIC_PRESSURE).await;
        let enhanced_baro_kpa = if let (Some(context), Some(selected)) =
            (profile_context.as_ref(), selected_lly.as_ref())
        {
            let enhanced_baro = read_profile_signal_value(
                session,
                context,
                selected,
                LLY_BAROMETRIC_PRESSURE_DID,
                &mut alerts,
                "barometric_pressure_gm",
                "Barometer GM $22 1251 01 candidate",
                Some("Used when standard PID 01 33 is unavailable or stale."),
            )
            .await;
            let value = enhanced_baro.value;
            source_confidence.push(enhanced_baro.evidence);
            value
        } else {
            None
        };
        let desired_map_kpa = if let (Some(context), Some(selected)) =
            (profile_context.as_ref(), selected_lly.as_ref())
        {
            let desired_map = read_profile_signal_value(
                session,
                context,
                selected,
                LLY_DESIRED_MAP_DID,
                &mut alerts,
                "desired_map",
                "Desired MAP GM $22 1542 01 candidate",
                Some("Display as absolute pressure until cross-checked against a factory-equivalent tool."),
            )
            .await;
            let value = desired_map.value.or_else(|| {
                self.last
                    .desired_map_psi
                    .map(|value| f64::from(value) / PSI_PER_KPA)
            });
            source_confidence.push(desired_map.evidence);
            value
        } else {
            None
        };
        let maf_g_s = read_scalar_pid(session, Pid::MAF, &mut alerts, "MAF")
            .await
            .unwrap_or(f64::from(self.last.maf_g_s));
        let fuel_rail_kpa = read_scalar_pid(
            session,
            Pid::FUEL_RAIL_GAUGE_PRESSURE,
            &mut alerts,
            "Fuel rail pressure",
        )
        .await;
        let actual_fuel_rail_gm_psi = if let (Some(context), Some(selected)) =
            (profile_context.as_ref(), selected_lly.as_ref())
        {
            let actual_fuel_rail_gm = read_profile_signal_value(
                session,
                context,
                selected,
                LLY_ACTUAL_FUEL_PRESSURE_DID,
                &mut alerts,
                "fuel_rail_actual_gm",
                "Actual fuel rail GM $22 163E 01",
                Some(
                    "Standard PID 01 23 remains preferred for the displayed actual rail pressure.",
                ),
            )
            .await;
            let value = actual_fuel_rail_gm
                .value
                .and_then(|value| pressure_to_psi(value, &actual_fuel_rail_gm.evidence.unit));
            source_confidence.push(actual_fuel_rail_gm.evidence);
            value
        } else {
            None
        };
        let fuel_rail_actual_psi = fuel_rail_kpa
            .map(|value| value * PSI_PER_KPA)
            .or(actual_fuel_rail_gm_psi)
            .unwrap_or(f64::from(self.last.fuel_rail.actual_psi));
        source_confidence.push(standard_signal_evidence(
            "fuel_rail_actual",
            "Actual fuel rail SAE PID 01 23",
            "ECM",
            "01 23",
            "SAE standard PID",
            "standard; forced direct poll",
            "psi",
            Some(fuel_rail_actual_psi as f32),
            if fuel_rail_kpa.is_some() {
                "success"
            } else if actual_fuel_rail_gm_psi.is_some() {
                "fallback-gm"
            } else {
                "cached"
            },
            Some("Full-range gauge pressure; preferred over GM $22 163E X-Gauge scaling."),
        ));
        let desired_fuel_rail_psi = if let (Some(context), Some(selected)) =
            (profile_context.as_ref(), selected_lly.as_ref())
        {
            let desired_fuel_rail = read_profile_signal_value(
                session,
                context,
                selected,
                LLY_DESIRED_FUEL_PRESSURE_DID,
                &mut alerts,
                "fuel_rail_desired",
                "Desired fuel rail GM $22 163D 01",
                Some("Display scaling is retained from current live/probe code until persisted bytes are cross-checked."),
            )
            .await;
            let value = desired_fuel_rail
                .value
                .and_then(|value| pressure_to_psi(value, &desired_fuel_rail.evidence.unit))
                .or(self.last.fuel_rail.desired_psi.map(f64::from));
            source_confidence.push(desired_fuel_rail.evidence);
            value
        } else {
            None
        };

        let (vgt_actual, vgt_desired) = if let (Some(context), Some(selected)) =
            (profile_context.as_ref(), selected_lly.as_ref())
        {
            let actual = read_enhanced_scalar(
                session,
                context,
                selected,
                0x1543,
                &mut alerts,
                "VGT actual",
            )
            .await
            .unwrap_or(f64::from(self.last.vgt.actual_pct));
            let desired = read_enhanced_scalar(
                session,
                context,
                selected,
                0x1540,
                &mut alerts,
                "VGT desired",
            )
            .await
            .unwrap_or(f64::from(self.last.vgt.desired_pct));
            (actual, desired)
        } else {
            (0.0, 0.0)
        };

        let mut cylinders = Vec::with_capacity(8);
        if let (Some(context), Some(selected)) = (profile_context.as_ref(), selected_lly.as_ref()) {
            for (idx, did) in (0x162Fu16..=0x1636).enumerate() {
                let previous = self
                    .last
                    .cylinders
                    .get(idx)
                    .map(|reading| f64::from(reading.mm3))
                    .unwrap_or(0.0);
                let mm3 = read_enhanced_scalar(
                    session,
                    context,
                    selected,
                    did,
                    &mut alerts,
                    &format!("Injector balance cyl {}", idx + 1),
                )
                .await
                .unwrap_or(previous);
                cylinders.push(CylinderBalance {
                    cylinder: (idx + 1) as u8,
                    mm3: mm3 as f32,
                });
            }
        } else {
            cylinders.extend((1..=8).map(|cylinder| CylinderBalance { cylinder, mm3: 0.0 }));
        }

        if let (Some(context), Some(selected)) = (profile_context.as_ref(), selected_lly.as_ref()) {
            if cycle % 12 == 0 {
                let scan = scan_profile_dtcs(session, context, selected, &mut alerts).await;
                self.cached_dtc_count = Some(scan.dtcs.len());
                self.cached_dtcs = scan.dtcs;
                self.cached_modules = scan.modules;
            }
        }

        let dtc_count = self.cached_dtc_count.unwrap_or(0);
        let ecu_count = session
            .discovery()
            .map(|discovery| discovery.modules.len())
            .unwrap_or(0);
        let protocol = protocol_label(session.adapter_info().protocol).to_string();
        let coolant_f = c_to_f(coolant_c);
        let intake_air_f = c_to_f(intake_air_c);
        let speed_mph = (speed_kph * MPH_PER_KPH).max(0.0).round() as u16;
        let baro_display_kpa = baro_kpa.or(enhanced_baro_kpa).or_else(|| {
            if rpm < 1_200.0 && speed_kph < 1.0 {
                Some(map_kpa)
            } else {
                self.last
                    .barometric_psi
                    .map(|value| f64::from(value) / PSI_PER_KPA)
            }
        });
        let boost_psi = baro_display_kpa
            .map(|baro| ((map_kpa - baro) * PSI_PER_KPA).max(0.0))
            .unwrap_or(0.0);
        let module_scan = if !gm_lly_enabled {
            Vec::new()
        } else if self.cached_modules.is_empty() {
            build_pending_module_scan(session)
        } else {
            self.cached_modules.clone()
        };

        Ok(DiagnosticSnapshot {
            vehicle,
            vin,
            protocol,
            connection: format!("live {port}"),
            voltage: voltage as f32,
            rpm: rpm.max(0.0).round() as u16,
            speed_mph,
            poll_ms: POLL_MS,
            units: "US".to_string(),
            statuses: status_values(dtc_count, ecu_count, false),
            alerts,
            dtcs: self.cached_dtcs.clone(),
            modules: module_scan,
            cylinders,
            vgt: VgtSnapshot {
                actual_pct: vgt_actual as f32,
                desired_pct: vgt_desired as f32,
                error_pct: (vgt_actual - vgt_desired) as f32,
            },
            fuel_rail: FuelRailSnapshot {
                actual_psi: fuel_rail_actual_psi as f32,
                desired_psi: desired_fuel_rail_psi.map(|value| value as f32),
                delta_psi: desired_fuel_rail_psi
                    .map(|desired| (fuel_rail_actual_psi - desired) as f32),
            },
            temperatures: TemperatureSnapshot {
                coolant_f: coolant_f as f32,
                intake_air_f: intake_air_f as f32,
                oil_f: None,
                trans_f: None,
                ambient_f: None,
            },
            map_psi: (map_kpa * PSI_PER_KPA) as f32,
            desired_map_psi: desired_map_kpa.map(|value| (value * PSI_PER_KPA) as f32),
            barometric_psi: baro_display_kpa.map(|value| (value * PSI_PER_KPA) as f32),
            boost_psi: boost_psi as f32,
            maf_g_s: maf_g_s as f32,
            source_confidence,
            active_tests: active_tests_snapshot(
                rpm,
                speed_kph,
                coolant_f,
                voltage,
                self.last_active_test_result.clone(),
            ),
        })
    }

    async fn connect(&mut self) -> Result<(), String> {
        let port = select_port()?;
        let transport = serial::SerialTransport::new(&port, self.baud)
            .map_err(|error| format!("failed to open {port} at {} baud: {error}", self.baud))?;

        tokio::time::sleep(Duration::from_millis(500)).await;

        let logging = LoggingTransport::new(transport);
        let adapter = Elm327Adapter::new(Box::new(logging));
        let mut session = Session::new(adapter);
        session.set_raw_capture_enabled(false);
        let info = session
            .initialize()
            .await
            .map_err(|error| format!("initialize {port}: {error}"))?;

        let generation = next_generation();
        self.identity = acquire_identity(&mut session, GUI_EXTRA_VIN_READS).await;
        self.vin = self
            .identity
            .vin
            .clone()
            .unwrap_or_else(|| "--".to_string());
        self.vehicle = session
            .spec()
            .map(|spec| spec.identity.name.clone())
            .unwrap_or_else(|| "Live OBD-II".to_string());
        let context = build_vehicle_context(&session, generation, &self.identity);
        let registry = ProfileRegistry::with_builtins();
        self.profile_state = select_into_state(&registry, &context);
        self.profile_context = Some(GuiProfileContextFingerprint::from_context(&context));

        if self.identity.vin.is_none() {
            self.last.alerts = vec!["VIN/spec identification failed or unread".to_string()];
        }

        if !self.has_selected_lly_profile() {
            self.cached_dtc_count = None;
            self.cached_dtcs.clear();
            self.cached_modules.clear();
        }

        self.port = Some(port);
        self.last.protocol = protocol_label(info.protocol).to_string();
        self.session = Some(session);
        Ok(())
    }

    fn refresh_profile_selection_if_needed(&mut self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };

        sync_identity_from_session(session, &mut self.identity);
        let current_generation = self.profile_state.generation;
        let context = build_vehicle_context(session, current_generation, &self.identity);
        let current = GuiProfileContextFingerprint::from_context(&context);
        if self.profile_context.as_ref() == Some(&current) {
            return;
        }

        let generation = next_generation();
        let context = build_vehicle_context(session, generation, &self.identity);
        let registry = ProfileRegistry::with_builtins();
        self.profile_state = select_into_state(&registry, &context);
        self.profile_context = Some(GuiProfileContextFingerprint::from_context(&context));

        if !self.has_selected_lly_profile() {
            self.cached_dtc_count = None;
            self.cached_dtcs.clear();
            self.cached_modules.clear();
        }
    }

    fn selected_lly_profile(&self) -> Option<SelectedProfile> {
        let selected = self.profile_state.selected.as_ref()?;
        (selected.profile_id().as_str() == LLY_PROFILE_ID).then(|| selected.clone())
    }

    fn has_selected_lly_profile(&self) -> bool {
        self.profile_state
            .selected
            .as_ref()
            .is_some_and(|selected| selected.profile_id().as_str() == LLY_PROFILE_ID)
    }

    fn profile_context(&self, session: &Session<Elm327Adapter>) -> VehicleContext {
        build_vehicle_context(session, self.profile_state.generation, &self.identity)
    }

    fn request_active_test(
        &mut self,
        command: GmActiveTestCommand,
    ) -> Result<GmActiveTestResult, String> {
        let mut result = blocked_active_test_result(&command);
        match self.write_active_test_evidence(&command, &result) {
            Ok(path) => {
                result.evidence_path = Some(path.display().to_string());
            }
            Err(error) => {
                return Err(format!("active-test evidence write failed: {error}"));
            }
        }
        self.last_active_test_result = Some(result.clone());
        self.last.active_tests.vgt_vane.last_result = Some(result.clone());
        Ok(result)
    }

    fn write_active_test_evidence(
        &self,
        command: &GmActiveTestCommand,
        result: &GmActiveTestResult,
    ) -> std::io::Result<std::path::PathBuf> {
        let mut writer = GmEvidenceWriter::create_raw_capture("gm-active-test")?;
        let vehicle = if self.vin == "--" {
            None
        } else {
            Some(GmVehicleIdentity {
                vin: Some(self.vin.clone()),
                year: None,
                make: None,
                model: Some(self.vehicle.clone()),
                engine: None,
            })
        };
        let record = active_test_evidence_record(command, result)
            .with_adapter_context(self.port.clone(), Some(self.last.protocol.clone()))
            .with_vehicle(vehicle);

        writer.append(&record)?;
        writer.flush()?;
        Ok(writer.path().to_path_buf())
    }
}

impl GuiProfileContextFingerprint {
    fn from_context(ctx: &VehicleContext) -> Self {
        Self {
            protocol: ctx.protocol,
            vin: ctx.vin.clone(),
            vin_confidence: ctx.vin_confidence,
            spec_identity: ctx.spec.as_ref().map(|spec| {
                format!(
                    "{}:{}:{}:{}",
                    spec.identity.name,
                    spec.identity.engine.code,
                    spec.identity.model_years.0,
                    spec.identity.model_years.1
                )
            }),
            active_bus: ctx.active_bus.clone(),
            discovered_modules: ctx
                .discovered_modules
                .iter()
                .map(|module| module.0.clone())
                .collect(),
        }
    }
}

fn sync_identity_from_session<A: Adapter>(session: &Session<A>, identity: &mut IdentityOutcome) {
    let Some(vehicle) = session.vehicle() else {
        return;
    };
    if identity.vin.as_deref() == Some(vehicle.vin.as_str()) {
        return;
    }

    identity.vin = Some(vehicle.vin.clone());
    identity.confidence = if validate_vin_charset(&vehicle.vin) {
        IdentityConfidence::Single
    } else {
        IdentityConfidence::Corrupted
    };
}

#[tauri::command]
async fn diagnostic_snapshot(state: State<'_, GuiState>) -> Result<DiagnosticSnapshot, String> {
    let mut backend = state.backend.lock().await;
    Ok(backend.snapshot().await)
}

#[tauri::command]
async fn request_active_test(
    state: State<'_, GuiState>,
    command: GmActiveTestCommand,
) -> Result<GmActiveTestResult, String> {
    let mut backend = state.backend.lock().await;
    backend.request_active_test(command)
}

async fn read_scalar_pid(
    session: &mut Session<Elm327Adapter>,
    pid: Pid,
    alerts: &mut Vec<String>,
    label: &str,
) -> Option<f64> {
    match session.read_pid(pid).await {
        Ok(reading) => scalar_value(&reading.value),
        Err(error) => {
            push_unexpected_error(alerts, label, &error);
            None
        }
    }
}

async fn read_optional_scalar_pid(session: &mut Session<Elm327Adapter>, pid: Pid) -> Option<f64> {
    session
        .read_pid(pid)
        .await
        .ok()
        .and_then(|reading| scalar_value(&reading.value))
}

async fn read_enhanced_scalar(
    session: &mut Session<Elm327Adapter>,
    context: &VehicleContext,
    selected: &SelectedProfile,
    did: u16,
    alerts: &mut Vec<String>,
    label: &str,
) -> Option<f64> {
    read_profile_signal_value(session, context, selected, did, alerts, label, label, None)
        .await
        .value
}

#[derive(Debug, Clone)]
struct GmSignalReading<T> {
    value: Option<T>,
    evidence: SignalEvidence,
}

#[derive(Default)]
struct GuiDispatchSink {
    last: Option<GuiDispatchRecord>,
}

#[derive(Debug, Clone)]
struct GuiDispatchRecord {
    module: String,
    node: String,
    request: String,
    response: Option<String>,
}

impl ProfileEvidenceSink for GuiDispatchSink {
    fn record(&mut self, evidence: &DispatchEvidence<'_>) {
        self.last = Some(GuiDispatchRecord {
            module: evidence.route.module.canonical().to_string(),
            node: physical_node_label(&evidence.physical_address),
            request: dispatch_request_text(evidence),
            response: (!evidence.raw_payload.is_empty()).then(|| spaced_hex(evidence.raw_payload)),
        });
    }
}

async fn read_profile_signal_value(
    session: &mut Session<Elm327Adapter>,
    context: &VehicleContext,
    selected: &SelectedProfile,
    did: u16,
    alerts: &mut Vec<String>,
    key: &str,
    label: &str,
    notes: Option<&str>,
) -> GmSignalReading<f64> {
    let registry = ProfileRegistry::with_builtins();
    let Some(profile) = registry.get(selected.profile_id()) else {
        alerts.push(format!("{label}: selected profile is not registered"));
        return GmSignalReading {
            value: None,
            evidence: profile_signal_evidence(
                key,
                label,
                "unknown",
                "--",
                "--",
                "GM Mode 22 registry",
                "missing-profile",
                "",
                None,
                "error",
                None,
                Some("Selected manufacturer profile is not present in the registry."),
            ),
        };
    };

    let Some(signal) = profile_signal_by_did(profile.signals(), did) else {
        alerts.push(format!(
            "{label}: DID 0x{did:04X} is not owned by the selected profile"
        ));
        return GmSignalReading {
            value: None,
            evidence: profile_signal_evidence(
                key,
                label,
                "unknown",
                "--",
                "--",
                "selected profile signal registry",
                "missing-signal",
                "",
                None,
                "error",
                None,
                Some("DID is not present in the selected manufacturer profile."),
            ),
        };
    };

    let source = profile_signal_source(&signal);
    let confidence = profile_signal_confidence(&signal);
    let mut evidence = profile_signal_evidence(
        key,
        label,
        signal.route.module.canonical(),
        "--",
        signal.source_fields.txd,
        &source,
        &confidence,
        signal.unit,
        None,
        "pending",
        None,
        notes,
    );

    let mut sink = GuiDispatchSink::default();
    let runtime = ProfileRuntime::new(&registry);
    match runtime
        .execute_request(
            session,
            context,
            selected,
            CapabilityId::Signal(signal.key),
            RequestId::SINGLE,
            &mut sink,
        )
        .await
    {
        Ok(ProfileResponse::Signal(decoded)) => {
            if let Some(dispatch) = sink.last {
                evidence.module = dispatch.module;
                evidence.node = dispatch.node;
                evidence.request = dispatch.request;
                evidence.response = dispatch.response;
            }
            evidence.status = "success".to_string();
            evidence.value = Some(decoded.value as f32);
            if evidence.response.is_none() {
                evidence.response = Some(spaced_hex(&decoded.raw));
            }
            GmSignalReading {
                value: Some(decoded.value),
                evidence,
            }
        }
        Ok(ProfileResponse::Dtcs(_)) => {
            alerts.push(format!(
                "{label}: selected profile returned DTCs for a signal request"
            ));
            evidence.status = "error".to_string();
            evidence.notes = merge_note(
                evidence.notes.as_deref(),
                "Profile capability returned DTCs for a signal request.",
            );
            GmSignalReading {
                value: None,
                evidence,
            }
        }
        Err(error) => {
            push_profile_dispatch_error(alerts, label, &error);
            if let Some(dispatch) = sink.last {
                evidence.module = dispatch.module;
                evidence.node = dispatch.node;
                evidence.request = dispatch.request;
                evidence.response = dispatch.response;
            }
            evidence.status = profile_dispatch_error_label(&error).to_string();
            evidence.notes = merge_note(evidence.notes.as_deref(), &format!("{error:?}"));
            GmSignalReading {
                value: None,
                evidence,
            }
        }
    }
}

fn profile_signal_by_did(signals: &[SignalDefinition], did: u16) -> Option<SignalDefinition> {
    signals
        .iter()
        .copied()
        .find(|signal| signal_did(*signal) == Some(did))
}

fn signal_did(signal: SignalDefinition) -> Option<u16> {
    (signal.request_data.len() >= 2)
        .then(|| u16::from_be_bytes([signal.request_data[0], signal.request_data[1]]))
}

fn profile_signal_source(signal: &SignalDefinition) -> String {
    let mut source = String::from("selected manufacturer profile");
    if !signal.source_fields.txd.is_empty() {
        source.push_str(" TXD ");
        source.push_str(signal.source_fields.txd);
    }
    if let Some(rxf) = signal.source_fields.rxf {
        source.push_str(" RXF ");
        source.push_str(rxf);
    }
    if let Some(rxd) = signal.source_fields.rxd {
        source.push_str(" RXD ");
        source.push_str(rxd.raw);
    }
    if let Some(mth) = signal.source_fields.raw_mth {
        source.push_str(" MTH ");
        source.push_str(mth);
    }
    source
}

fn profile_signal_confidence(signal: &SignalDefinition) -> String {
    let mut label = profile_confidence_label(signal.confidence).to_string();
    for provenance in signal.provenance {
        label.push(';');
        label.push_str(profile_provenance_part(*provenance));
    }
    if matches!(
        signal.failure_policy,
        obd2_dash::profiles::FailurePolicy::CandidateOnly
    ) {
        label.push_str(";candidate-only");
    }
    label
}

fn physical_node_label(address: &PhysicalAddress) -> String {
    match address {
        PhysicalAddress::J1850 { node, .. } => format!("0x{node:02X}"),
        PhysicalAddress::Can11Bit { request_id, .. } => format!("0x{request_id:03X}"),
        PhysicalAddress::Can29Bit { request_id, .. } => format!("0x{request_id:08X}"),
        _ => "addressed".to_string(),
    }
}

fn dispatch_request_text(evidence: &DispatchEvidence<'_>) -> String {
    let mut bytes = Vec::with_capacity(4 + evidence.request_data.len());
    match evidence.physical_address {
        PhysicalAddress::J1850 { header, .. } => bytes.extend_from_slice(&header),
        PhysicalAddress::Can11Bit { .. } | PhysicalAddress::Can29Bit { .. } => {}
        _ => {}
    }
    bytes.push(evidence.service_id);
    bytes.extend_from_slice(evidence.request_data);
    spaced_hex(&bytes)
}

fn push_profile_dispatch_error(alerts: &mut Vec<String>, label: &str, error: &DispatchError) {
    match error {
        DispatchError::Transport(error) => push_unexpected_error(alerts, label, error),
        DispatchError::Decode(ProfileDecodeError::NegativeResponse { nrc, .. })
            if is_unsupported_nrc_byte(*nrc) => {}
        other => alerts.push(format!("{label}: {other:?}")),
    }
}

fn profile_dispatch_error_label(error: &DispatchError) -> &'static str {
    match error {
        DispatchError::Transport(error) => request_error_label(error),
        DispatchError::Decode(ProfileDecodeError::NegativeResponse { nrc, .. })
            if is_unsupported_nrc_byte(*nrc) =>
        {
            "unsupported"
        }
        DispatchError::Decode(_) => "decode error",
        DispatchError::CapabilityNotOwned { .. }
        | DispatchError::RouteNotOwnedByCapability { .. } => "unavailable",
        _ => "error",
    }
}

fn is_unsupported_nrc_byte(nrc: u8) -> bool {
    matches!(
        NegativeResponse::from_byte(nrc),
        Some(NegativeResponse::ServiceNotSupported | NegativeResponse::SubFunctionNotSupported)
    )
}

fn profile_confidence_label(confidence: ProfileConfidence) -> &'static str {
    match confidence {
        ProfileConfidence::Candidate => "candidate",
        ProfileConfidence::LiveObserved => "live-observed",
        ProfileConfidence::Community => "community",
        ProfileConfidence::Verified => "verified",
        ProfileConfidence::Rejected => "rejected",
    }
}

fn profile_provenance_part(provenance: ProfileProvenance) -> &'static str {
    match provenance {
        ProfileProvenance::ScanGaugePublished => "scangauge-published",
        ProfileProvenance::LiveObserved => "live-observed",
        ProfileProvenance::LegacySpec => "legacy-spec",
        ProfileProvenance::LocalRejection => "local-rejection",
        ProfileProvenance::LocalFixture => "local-fixture",
    }
}

fn pressure_to_psi(value: f64, unit: &str) -> Option<f64> {
    match unit {
        "kPa" | "kPa abs" => Some(value * PSI_PER_KPA),
        "psi" => Some(value),
        _ => None,
    }
}

#[derive(Debug, Clone)]
struct GmClass2Scan {
    dtcs: Vec<DtcSnapshot>,
    modules: Vec<ModuleScan>,
}

#[derive(Debug, Clone)]
struct GenericDtcScan {
    dtcs: Vec<DtcSnapshot>,
    module_counts: HashMap<String, usize>,
    standard_label: String,
}

async fn scan_profile_dtcs(
    session: &mut Session<Elm327Adapter>,
    context: &VehicleContext,
    selected: &SelectedProfile,
    alerts: &mut Vec<String>,
) -> GmClass2Scan {
    let mut dtcs = Vec::new();
    let mut generic = scan_generic_dtcs(session, alerts).await;
    push_unique_dtcs(&mut dtcs, std::mem::take(&mut generic.dtcs));

    let registry = ProfileRegistry::with_builtins();
    let runtime = ProfileRuntime::new(&registry);
    let coverage = CoverageMap::new(Vec::new()).with_discovered_modules(
        context
            .discovered_modules
            .iter()
            .filter_map(profile_module_key)
            .collect(),
    );
    let plan = runtime.plan_poll_cycle(Some(selected), PROFILE_DTC_SCAN_CYCLE, &coverage);
    let mut modules = Vec::new();

    for planned in plan.requests {
        let CapabilityId::DtcService(key) = planned.capability else {
            continue;
        };

        let module = planned.route.module.to_core_module_id();
        let module_label = module.0.clone();
        let standard_label = generic_standard_label(&generic, &module_label);
        let row = ensure_module_scan(&mut modules, &module_label, standard_label);
        let label = execute_profile_dtc_request(
            session,
            &mut dtcs,
            &runtime,
            context,
            selected,
            planned.capability,
            planned.request,
            &module,
            alerts,
        )
        .await;

        match key {
            "lly.class2.dtc.all" => row.gm_all = label,
            "lly.class2.dtc.active" => row.gm_active = label,
            _ => {}
        }
    }

    if modules.is_empty() {
        modules = build_pending_module_scan(session);
    }

    GmClass2Scan { dtcs, modules }
}

async fn scan_generic_dtcs(
    session: &mut Session<Elm327Adapter>,
    alerts: &mut Vec<String>,
) -> GenericDtcScan {
    match session.read_all_dtcs().await {
        Ok(dtcs) => {
            let dtcs = dtcs.into_iter().map(dtc_from_core).collect::<Vec<_>>();
            let mut module_counts = HashMap::new();
            for dtc in &dtcs {
                *module_counts.entry(dtc.module.clone()).or_insert(0) += 1;
            }
            GenericDtcScan {
                dtcs,
                module_counts,
                standard_label: "empty".to_string(),
            }
        }
        Err(error) => {
            push_unexpected_error(alerts, "SAE DTC scan", &error);
            GenericDtcScan {
                dtcs: Vec::new(),
                module_counts: HashMap::new(),
                standard_label: request_error_label(&error).to_string(),
            }
        }
    }
}

async fn execute_profile_dtc_request(
    session: &mut Session<Elm327Adapter>,
    dtcs: &mut Vec<DtcSnapshot>,
    runtime: &ProfileRuntime<'_>,
    context: &VehicleContext,
    selected: &SelectedProfile,
    capability: CapabilityId,
    request: RequestId,
    fallback_module: &ModuleId,
    alerts: &mut Vec<String>,
) -> String {
    let mut sink = GuiDispatchSink::default();
    match runtime
        .execute_request(session, context, selected, capability, request, &mut sink)
        .await
    {
        Ok(ProfileResponse::Dtcs(decoded)) => {
            let count = decoded.len();
            let snapshots = decoded
                .into_iter()
                .map(|decoded| dtc_from_profile_decoded(decoded, fallback_module))
                .collect::<Vec<_>>();
            push_unique_dtcs(dtcs, snapshots);
            count_label(count)
        }
        Ok(ProfileResponse::Signal(_)) => {
            alerts.push(format!(
                "Profile DTC capability {capability:?} returned a signal"
            ));
            "error".to_string()
        }
        Err(error) => profile_dtc_error_label(capability, error, alerts),
    }
}

fn dtc_from_profile_decoded(decoded: DecodedDtc, fallback_module: &ModuleId) -> DtcSnapshot {
    let mut dtc = Dtc::from_code(&decoded.code);
    dtc.status = decoded.status;
    dtc.source_module = Some(
        decoded
            .module
            .map(|module| module.0)
            .unwrap_or_else(|| fallback_module.0.clone()),
    );
    dtc.notes = decoded.notes;
    dtc_from_core(dtc)
}

fn profile_dtc_error_label(
    capability: CapabilityId,
    error: DispatchError,
    alerts: &mut Vec<String>,
) -> String {
    match error {
        DispatchError::Transport(Obd2Error::NoData) => "no data".to_string(),
        DispatchError::Transport(Obd2Error::NegativeResponse { nrc, .. })
            if is_unsupported_nrc(nrc) =>
        {
            "unsup".to_string()
        }
        DispatchError::Decode(ProfileDecodeError::NegativeResponse { nrc, .. })
            if is_unsupported_nrc_byte(nrc) =>
        {
            "unsup".to_string()
        }
        DispatchError::Decode(ProfileDecodeError::NegativeResponse { service, nrc }) => {
            alerts.push(format!(
                "Profile DTC {capability:?}: negative response service 0x{service:02X}, {}",
                negative_response_label(nrc)
            ));
            "error".to_string()
        }
        DispatchError::Decode(ProfileDecodeError::Decode(message))
        | DispatchError::Decode(ProfileDecodeError::Other(message)) => {
            alerts.push(format!("Profile DTC {capability:?}: {message}"));
            "error".to_string()
        }
        other => {
            alerts.push(format!("Profile DTC {capability:?}: {other:?}"));
            "error".to_string()
        }
    }
}

fn is_unsupported_nrc(nrc: NegativeResponse) -> bool {
    matches!(
        nrc,
        NegativeResponse::ServiceNotSupported | NegativeResponse::SubFunctionNotSupported
    )
}

fn negative_response_label(nrc: u8) -> String {
    NegativeResponse::from_byte(nrc)
        .map(|nrc| nrc.to_string())
        .unwrap_or_else(|| format!("NRC 0x{nrc:02X}"))
}

fn generic_standard_label(generic: &GenericDtcScan, module: &str) -> String {
    generic
        .module_counts
        .get(module)
        .map(|count| count_label(*count))
        .unwrap_or_else(|| generic.standard_label.clone())
}

fn ensure_module_scan<'a>(
    modules: &'a mut Vec<ModuleScan>,
    module: &str,
    standard: String,
) -> &'a mut ModuleScan {
    if let Some(idx) = modules.iter().position(|row| row.module == module) {
        return &mut modules[idx];
    }
    modules.push(ModuleScan {
        module: module.to_string(),
        standard,
        gm_all: "pending".to_string(),
        gm_active: "pending".to_string(),
    });
    modules.last_mut().expect("module scan row just pushed")
}

fn profile_module_key(module: &ModuleId) -> Option<ModuleKey> {
    match module.0.as_str() {
        "ecm" => Some(ModuleKey::Ecm),
        "tcm" => Some(ModuleKey::Tcm),
        "ficm" => Some(ModuleKey::Ficm),
        "bcm" => Some(ModuleKey::Bcm),
        "abs" => Some(ModuleKey::Ebcm),
        "ipc" => Some(ModuleKey::Ipc),
        "airbag" => Some(ModuleKey::Sdm),
        "hvac" => Some(ModuleKey::Hvac),
        _ => None,
    }
}

fn spaced_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(3).saturating_sub(1));
    for (idx, byte) in bytes.iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        use std::fmt::Write;
        write!(&mut out, "{byte:02X}").expect("write to string");
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn profile_signal_evidence(
    key: &str,
    label: &str,
    module: &str,
    node: &str,
    request: &str,
    source: &str,
    confidence: &str,
    unit: &str,
    value: Option<f32>,
    status: &str,
    response: Option<&str>,
    notes: Option<&str>,
) -> SignalEvidence {
    SignalEvidence {
        key: key.to_string(),
        label: label.to_string(),
        module: module.to_string(),
        node: node.to_string(),
        request: request.to_string(),
        source: source.to_string(),
        confidence: confidence.to_string(),
        status: status.to_string(),
        unit: unit.to_string(),
        value,
        response: response.map(str::to_string),
        notes: notes.map(str::to_string),
    }
}

#[allow(clippy::too_many_arguments)]
fn standard_signal_evidence(
    key: &str,
    label: &str,
    module: &str,
    request: &str,
    source: &str,
    confidence: &str,
    unit: &str,
    value: Option<f32>,
    status: &str,
    notes: Option<&str>,
) -> SignalEvidence {
    SignalEvidence {
        key: key.to_string(),
        label: label.to_string(),
        module: module.to_string(),
        node: "broadcast".to_string(),
        request: request.to_string(),
        source: source.to_string(),
        confidence: confidence.to_string(),
        status: status.to_string(),
        unit: unit.to_string(),
        value,
        response: None,
        notes: notes.map(str::to_string),
    }
}

fn merge_note(existing: Option<&str>, detail: &str) -> Option<String> {
    match existing {
        Some(existing) if !existing.is_empty() => Some(format!("{existing} {detail}")),
        _ if !detail.is_empty() => Some(detail.to_string()),
        _ => None,
    }
}

fn request_error_label(error: &Obd2Error) -> &'static str {
    match error {
        Obd2Error::NoData => "no data",
        Obd2Error::UnsupportedPid { .. } => "unsupported",
        Obd2Error::NegativeResponse {
            nrc: NegativeResponse::ServiceNotSupported,
            ..
        }
        | Obd2Error::NegativeResponse {
            nrc: NegativeResponse::SubFunctionNotSupported,
            ..
        } => "unsupported",
        _ => "error",
    }
}

fn dtc_from_core(dtc: Dtc) -> DtcSnapshot {
    let status = dtc
        .notes
        .as_deref()
        .and_then(|notes| notes.strip_prefix("GM Class 2 status "))
        .unwrap_or(match dtc.status {
            obd2_core::protocol::dtc::DtcStatus::Pending => "pending",
            obd2_core::protocol::dtc::DtcStatus::Permanent => "permanent",
            obd2_core::protocol::dtc::DtcStatus::Stored => "stored",
        })
        .to_string();

    DtcSnapshot {
        code: dtc.code,
        module: dtc.source_module.unwrap_or_else(|| "unknown".to_string()),
        status,
        description: dtc.description,
        notes: dtc.notes,
    }
}

fn count_label(count: usize) -> String {
    if count == 0 {
        "empty".to_string()
    } else {
        format!("{count} dtc")
    }
}

fn push_unique_dtcs(dst: &mut Vec<DtcSnapshot>, new_dtcs: Vec<DtcSnapshot>) {
    let mut seen: HashSet<(String, String, String)> = dst
        .iter()
        .map(|dtc| (dtc.module.clone(), dtc.code.clone(), dtc.status.clone()))
        .collect();
    for dtc in new_dtcs {
        if seen.insert((dtc.module.clone(), dtc.code.clone(), dtc.status.clone())) {
            dst.push(dtc);
        }
    }
}

fn scalar_value(value: &Value) -> Option<f64> {
    match value {
        Value::Scalar(value) => Some(*value),
        _ => None,
    }
}

fn push_unexpected_error(alerts: &mut Vec<String>, label: &str, error: &Obd2Error) {
    if is_quiet_no_data(error) {
        return;
    }
    alerts.push(format!("{label}: {error}"));
}

fn is_quiet_no_data(error: &Obd2Error) -> bool {
    matches!(
        error,
        Obd2Error::NoData
            | Obd2Error::UnsupportedPid { .. }
            | Obd2Error::NegativeResponse {
                nrc: obd2_core::error::NegativeResponse::ServiceNotSupported,
                ..
            }
            | Obd2Error::NegativeResponse {
                nrc: obd2_core::error::NegativeResponse::SubFunctionNotSupported,
                ..
            }
    )
}

fn build_pending_module_scan(session: &Session<Elm327Adapter>) -> Vec<ModuleScan> {
    let Some(discovery) = session.discovery() else {
        return Vec::new();
    };

    let mut modules: Vec<String> = discovery
        .modules
        .keys()
        .map(|module| module.0.clone())
        .collect();
    modules.sort();
    modules
        .into_iter()
        .map(|module| ModuleScan {
            module,
            standard: "probe".to_string(),
            gm_all: "probe".to_string(),
            gm_active: "probe".to_string(),
        })
        .collect()
}

fn status_values(dtc_count: usize, ecu_count: usize, mil_on: bool) -> Vec<StatusValue> {
    vec![
        status(
            "DTCs",
            dtc_count.to_string(),
            if dtc_count == 0 { "ok" } else { "warn" },
        ),
        status("ECUs", ecu_count.to_string(), "ok"),
        status(
            "MIL",
            if mil_on { "ON" } else { "OFF" },
            if mil_on { "crit" } else { "ok" },
        ),
        status("Record", "ready", "warn"),
    ]
}

fn status(
    label: impl Into<String>,
    value: impl Into<String>,
    state: impl Into<String>,
) -> StatusValue {
    StatusValue {
        label: label.into(),
        value: value.into(),
        state: state.into(),
    }
}

fn active_tests_snapshot(
    rpm: f64,
    speed_kph: f64,
    coolant_f: f64,
    voltage: f64,
    last_result: Option<GmActiveTestResult>,
) -> ActiveTestsSnapshot {
    let preconditions = vec![
        active_precondition(
            "Verified command profile",
            false,
            "No verified GM Class 2 actuator-control bytes are loaded.",
        ),
        active_precondition(
            "Stationary",
            speed_kph < 0.5,
            format!("{:.1} mph", speed_kph * MPH_PER_KPH),
        ),
        active_precondition(
            "Idle speed",
            (500.0..=900.0).contains(&rpm),
            format!("{rpm:.0} rpm"),
        ),
        active_precondition(
            "Warm coolant",
            coolant_f >= 104.0,
            format!("{coolant_f:.1} F"),
        ),
        active_precondition(
            "Battery voltage",
            voltage >= 12.0,
            format!("{voltage:.1} V"),
        ),
        active_precondition(
            "Park/Neutral and A/C off",
            false,
            "Not observable through current data; future enabled command must require operator confirmation.",
        ),
    ];

    ActiveTestsSnapshot {
        vgt_vane: VgtActiveTestSnapshot {
            definition: vgt_vane_control_definition(),
            preconditions,
            last_result,
        },
    }
}

fn active_precondition(
    label: impl Into<String>,
    satisfied: bool,
    detail: impl Into<String>,
) -> GmActiveTestPrecondition {
    GmActiveTestPrecondition {
        label: label.into(),
        satisfied,
        detail: detail.into(),
    }
}

fn select_port() -> Result<String, String> {
    if let Ok(port) = env::var("OBD2_PORT") {
        let trimmed = port.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    let ports = serial::list_ports();
    if ports.is_empty() {
        return Err("no serial ports found; set OBD2_PORT=/dev/cu.usbserial-...".to_string());
    }

    ports
        .iter()
        .find(|port| {
            let lower = port.to_lowercase();
            lower.contains("usbserial")
                || lower.contains("usbmodem")
                || lower.contains("ttyusb")
                || lower.contains("slab_usbtouart")
                || lower.contains("wchusbserial")
        })
        .cloned()
        .or_else(|| ports.first().cloned())
        .ok_or_else(|| "no usable serial port found".to_string())
}

fn configured_baud() -> u32 {
    env::var("OBD2_BAUD")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_BAUD)
}

fn protocol_label(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::J1850Vpw => "J1850 VPW",
        Protocol::J1850Pwm => "J1850 PWM",
        Protocol::Iso9141(_) => "ISO 9141",
        Protocol::Kwp2000(_) => "KWP2000",
        Protocol::Can11Bit500 => "CAN 11/500",
        Protocol::Can11Bit250 => "CAN 11/250",
        Protocol::Can29Bit500 => "CAN 29/500",
        Protocol::Can29Bit250 => "CAN 29/250",
        Protocol::Auto => "auto",
        _ => "unknown",
    }
}

fn c_to_f(value: f64) -> f64 {
    value * 9.0 / 5.0 + 32.0
}

fn f_to_c(value: f64) -> f64 {
    (value - 32.0) * 5.0 / 9.0
}

fn empty_snapshot(connection: impl Into<String>) -> DiagnosticSnapshot {
    DiagnosticSnapshot {
        vehicle: "Live OBD-II".to_string(),
        vin: "--".to_string(),
        protocol: "--".to_string(),
        connection: connection.into(),
        voltage: 0.0,
        rpm: 0,
        speed_mph: 0,
        poll_ms: POLL_MS,
        units: "US".to_string(),
        statuses: status_values(0, 0, false),
        alerts: Vec::new(),
        dtcs: Vec::new(),
        modules: Vec::new(),
        cylinders: (1..=8)
            .map(|cylinder| CylinderBalance { cylinder, mm3: 0.0 })
            .collect(),
        vgt: VgtSnapshot {
            actual_pct: 0.0,
            desired_pct: 0.0,
            error_pct: 0.0,
        },
        fuel_rail: FuelRailSnapshot {
            actual_psi: 0.0,
            desired_psi: None,
            delta_psi: None,
        },
        temperatures: TemperatureSnapshot {
            coolant_f: 0.0,
            intake_air_f: 0.0,
            oil_f: None,
            trans_f: None,
            ambient_f: None,
        },
        map_psi: 0.0,
        desired_map_psi: None,
        barometric_psi: None,
        boost_psi: 0.0,
        maf_g_s: 0.0,
        source_confidence: Vec::new(),
        active_tests: active_tests_snapshot(0.0, 0.0, 0.0, 0.0, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use obd2_dash::profiles::ProfileId;

    fn decode_profile_signal(did: u16, payload: &[u8]) -> obd2_dash::profiles::DecodedSignal {
        let registry = ProfileRegistry::with_builtins();
        let profile = registry
            .get(ProfileId::new(LLY_PROFILE_ID))
            .expect("LLY profile registered");
        let signal = profile_signal_by_did(profile.signals(), did).expect("profile signal");
        profile
            .decode_signal(&signal, payload)
            .expect("profile signal decode")
    }

    #[test]
    fn decodes_lly_fuel_pressure_from_profile_stripped_payload() {
        let decoded = decode_profile_signal(LLY_DESIRED_FUEL_PRESSURE_DID, &[0x26, 0x00]);

        assert_eq!(decoded.selected_raw, vec![0x26]);
        assert!((decoded.value - 551.0).abs() < 0.1);
        assert_eq!(decoded.unit, "psi");
    }

    #[test]
    fn decodes_lly_fuel_pressure_from_profile_full_mode_22_payload() {
        let decoded = decode_profile_signal(
            LLY_DESIRED_FUEL_PRESSURE_DID,
            &[0x62, 0x16, 0x3D, 0x26, 0x00],
        );

        assert_eq!(decoded.selected_raw, vec![0x26]);
        assert!((decoded.value - 551.0).abs() < 0.1);
    }

    #[test]
    fn decodes_gm_mode22_u8_from_profile_stripped_payload() {
        let decoded = decode_profile_signal(LLY_BAROMETRIC_PRESSURE_DID, &[0x61]);

        assert_eq!(decoded.selected_raw, vec![97]);
        assert_eq!(decoded.value, 97.0);
    }

    #[test]
    fn decodes_gm_mode22_u8_from_profile_full_mode_22_payload() {
        let decoded = decode_profile_signal(LLY_BAROMETRIC_PRESSURE_DID, &[0x62, 0x12, 0x51, 0x61]);

        assert_eq!(decoded.selected_raw, vec![97]);
        assert_eq!(decoded.value, 97.0);
    }

    #[test]
    fn decodes_desired_map_candidate_from_mode_22_payload() {
        let decoded = decode_profile_signal(LLY_DESIRED_MAP_DID, &[0x62, 0x15, 0x42, 0x67]);

        assert_eq!(decoded.selected_raw, vec![103]);
        assert_eq!(decoded.value, 103.0);
    }

    #[test]
    fn decodes_lly_fuel_pressure_with_echoed_selector_byte() {
        let decoded = decode_profile_signal(
            LLY_DESIRED_FUEL_PRESSURE_DID,
            &[0x62, 0x16, 0x3D, 0x01, 0x26, 0x00],
        );

        assert_eq!(decoded.selected_raw, vec![0x26]);
        assert!((decoded.value - 551.0).abs() < 0.1);
    }

    #[test]
    fn rejects_short_lly_fuel_pressure_payload() {
        let registry = ProfileRegistry::with_builtins();
        let profile = registry
            .get(ProfileId::new(LLY_PROFILE_ID))
            .expect("LLY profile registered");
        let signal = profile_signal_by_did(profile.signals(), LLY_DESIRED_FUEL_PRESSURE_DID)
            .expect("profile signal");
        let err = profile.decode_signal(&signal, &[0x01]).unwrap_err();

        assert!(matches!(
            err,
            ProfileDecodeError::PayloadTooShort { .. }
                | ProfileDecodeError::Decode(_)
                | ProfileDecodeError::MismatchedResponse
        ));
    }
}

fn main() {
    tauri::Builder::default()
        .manage(GuiState {
            backend: Mutex::new(LiveBackend::default()),
        })
        .invoke_handler(tauri::generate_handler![
            diagnostic_snapshot,
            request_active_test
        ])
        .run(tauri::generate_context!())
        .expect("failed to run OBD2 Dash GUI");
}
