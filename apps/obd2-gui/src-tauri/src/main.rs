#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::PathBuf,
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
    active_test_evidence_record, blocked_active_test_result, GmActiveTestCommand,
    GmActiveTestPrecondition, GmActiveTestResult,
};
use obd2_dash::gm_evidence::{GmEvidenceWriter, GmVehicleIdentity};
use obd2_dash::profiles::{
    acquire_identity, build_vehicle_context, builtin_profile, next_generation, select_into_state,
    validate_vin_charset, ActiveCommandProfile as ProfileActiveCommandProfile,
    ActiveTestDefinition, CapabilityId, Confidence as ProfileConfidence, CoverageMap, DecodedDtc,
    DiagnosticProfile, DispatchError, DispatchEvidence, EvidencePolicy as ProfileEvidencePolicy,
    FailurePolicy as ProfileFailurePolicy, IdentityConfidence, IdentityOutcome, ModuleKey,
    ModuleSafetyClass as ProfileModuleSafetyClass, PairRole as ProfilePairRole, ProfileDecodeError,
    ProfileEvidenceSink, ProfileRegistry, ProfileResponse, ProfileRuntime, ProfileState,
    Provenance as ProfileProvenance, RequestId, SafetyClass as ProfileSafetyClass, SelectedProfile,
    SignalCategory as ProfileSignalCategory, SignalComposition as ProfileSignalComposition,
    SignalDefinition, SignalDisplayDefinition, SignalDisplaySource, VehicleContext,
};
use serde::Serialize;
use tauri::{Manager, State};
use tokio::{sync::Mutex, time::Duration};

const DEFAULT_BAUD: u32 = 115_200;
const POLL_MS: u16 = 250;
const GUI_EXTRA_VIN_READS: u8 = 2;
const PROFILE_DTC_SCAN_CYCLE: u64 = 60;
const PSI_PER_KPA: f64 = 0.145_037_737_7;
const MPH_PER_KPH: f64 = 0.621_371;
const MAX_RECORDING_FILE_BYTES: u64 = 256 * 1024 * 1024;

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
struct SignalRxdSource {
    raw: String,
    bit_width: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
struct SignalSourceFields {
    txd: String,
    rxf: Option<String>,
    rxd: Option<SignalRxdSource>,
    raw_mth: Option<String>,
    source_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SignalComposition {
    Scalar,
    Pair {
        group_key: String,
        group_label: Option<String>,
        role: String,
    },
    TableRow {
        table_key: String,
        table_label: Option<String>,
        row_index: u8,
        row_label: String,
    },
    Derived {
        group_key: String,
        group_label: Option<String>,
        formula_key: String,
        input_keys: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
struct SignalSnapshot {
    key: String,
    label: String,
    category: String,
    module: String,
    unit: String,
    value: Option<f32>,
    state: String,
    confidence: String,
    provenance: Vec<String>,
    source_fields: Option<SignalSourceFields>,
    request: Option<String>,
    decoder_id: Option<String>,
    evidence_policy: String,
    failure_policy: String,
    preferred_over: Option<String>,
    evidence: Option<SignalEvidence>,
    composition: SignalComposition,
}

#[derive(Debug, Clone, Serialize)]
struct CapabilitySection {
    id: String,
    category: String,
    label: String,
    signal_keys: Vec<String>,
    active_test_keys: Vec<String>,
    diagnostic_service_keys: Vec<String>,
    visible: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ActiveTestSnapshotV2 {
    key: String,
    label: String,
    safety_class: String,
    command_profile: String,
    actionable: bool,
    lock_reason: Option<String>,
    supported_modes: Vec<String>,
    safety_notes: Vec<String>,
    preconditions: Vec<GmActiveTestPrecondition>,
    timeout_ms: u64,
    cancel_available: bool,
    evidence_policy: String,
    last_result: Option<GmActiveTestResult>,
}

struct ActiveTestRuntimeValues {
    rpm: f64,
    speed_kph: f64,
    coolant_f: f64,
    voltage: f64,
    last_result: Option<GmActiveTestResult>,
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
    source_confidence: Vec<SignalEvidence>,
    signals: Vec<SignalSnapshot>,
    capability_sections: Vec<CapabilitySection>,
    active_tests_v2: Vec<ActiveTestSnapshotV2>,
}

#[derive(Debug, Clone)]
struct LastLiveValues {
    voltage: f32,
    rpm: u16,
    speed_mph: u16,
    coolant_f: f32,
    intake_air_f: f32,
    map_psi: f32,
    desired_map_psi: Option<f32>,
    barometric_psi: Option<f32>,
    maf_g_s: f32,
    fuel_rail_actual_psi: f32,
    fuel_rail_desired_psi: Option<f32>,
    vgt_actual_pct: f32,
    vgt_desired_pct: f32,
    cylinders: Vec<CylinderBalance>,
}

impl Default for LastLiveValues {
    fn default() -> Self {
        Self {
            voltage: 0.0,
            rpm: 0,
            speed_mph: 0,
            coolant_f: 0.0,
            intake_air_f: 0.0,
            map_psi: 0.0,
            desired_map_psi: None,
            barometric_psi: None,
            maf_g_s: 0.0,
            fuel_rail_actual_psi: 0.0,
            fuel_rail_desired_psi: None,
            vgt_actual_pct: 0.0,
            vgt_desired_pct: 0.0,
            cylinders: (1..=8)
                .map(|cylinder| CylinderBalance { cylinder, mm3: 0.0 })
                .collect(),
        }
    }
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
    last_values: LastLiveValues,
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
            last_values: LastLiveValues::default(),
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
        let selected_profile = self.profile_state.selected.clone();
        let selected_profile_definition = selected_profile
            .as_ref()
            .and_then(|selected| builtin_profile(selected.profile_id()));
        let profile_context = self
            .session
            .as_ref()
            .and_then(|session| selected_profile_definition.map(|_| self.profile_context(session)));
        let profile_enabled = selected_profile_definition.is_some();
        if !profile_enabled {
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
            Ok(None) => f64::from(self.last_values.voltage),
            Err(error) => {
                push_unexpected_error(&mut alerts, "Battery voltage", &error);
                f64::from(self.last_values.voltage)
            }
        };

        let rpm_read = read_scalar_pid(session, Pid::ENGINE_RPM, &mut alerts, "Engine RPM").await;
        let rpm = rpm_read.unwrap_or(f64::from(self.last_values.rpm));
        let speed_kph_read =
            read_scalar_pid(session, Pid::VEHICLE_SPEED, &mut alerts, "Vehicle speed").await;
        let speed_kph =
            speed_kph_read.unwrap_or(f64::from(self.last_values.speed_mph) / MPH_PER_KPH);
        let coolant_c_read = read_scalar_pid(
            session,
            Pid::COOLANT_TEMP,
            &mut alerts,
            "Coolant temperature",
        )
        .await;
        let coolant_c = coolant_c_read.unwrap_or(f_to_c(f64::from(self.last_values.coolant_f)));
        let intake_air_c_read = read_scalar_pid(
            session,
            Pid::INTAKE_AIR_TEMP,
            &mut alerts,
            "Intake air temperature",
        )
        .await;
        let intake_air_c =
            intake_air_c_read.unwrap_or(f_to_c(f64::from(self.last_values.intake_air_f)));
        let map_kpa_read =
            read_scalar_pid(session, Pid::INTAKE_MAP, &mut alerts, "Intake MAP").await;
        let map_kpa = map_kpa_read.unwrap_or(f64::from(self.last_values.map_psi) / PSI_PER_KPA);
        let baro_kpa = read_optional_scalar_pid(session, Pid::BAROMETRIC_PRESSURE).await;
        let maf_g_s_read = read_scalar_pid(session, Pid::MAF, &mut alerts, "MAF").await;
        let maf_g_s = maf_g_s_read.unwrap_or(f64::from(self.last_values.maf_g_s));
        let fuel_rail_kpa = read_scalar_pid(
            session,
            Pid::FUEL_RAIL_GAUGE_PRESSURE,
            &mut alerts,
            "Fuel rail pressure",
        )
        .await;

        let mut profile_readings = HashMap::new();
        if let (Some(context), Some(selected), Some(profile)) = (
            profile_context.as_ref(),
            selected_profile.as_ref(),
            selected_profile_definition,
        ) {
            for signal in profile_signals_to_read(profile) {
                let reading =
                    read_profile_signal(session, context, selected, signal, &mut alerts).await;
                source_confidence.push(reading.evidence.clone());
                profile_readings.insert(signal.key, reading);
            }
        }

        let enhanced_baro_kpa = profile_reading_value_by_key(&profile_readings, "lly.1251");
        let desired_map_kpa =
            profile_reading_value_by_key(&profile_readings, "lly.1542").or_else(|| {
                self.last_values
                    .desired_map_psi
                    .map(|value| f64::from(value) / PSI_PER_KPA)
            });
        let actual_fuel_rail_gm_psi = profile_reading_by_key(&profile_readings, "lly.163E")
            .and_then(|reading| {
                reading
                    .value
                    .and_then(|value| pressure_to_psi(value, &reading.evidence.unit))
            });
        let fuel_rail_actual_psi = fuel_rail_kpa
            .map(|value| value * PSI_PER_KPA)
            .or(actual_fuel_rail_gm_psi)
            .unwrap_or(f64::from(self.last_values.fuel_rail_actual_psi));
        let actual_fuel_rail_evidence = standard_signal_evidence(
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
        );
        source_confidence.push(actual_fuel_rail_evidence.clone());
        let desired_fuel_rail_psi = profile_reading_by_key(&profile_readings, "lly.163D")
            .and_then(|reading| {
                reading
                    .value
                    .and_then(|value| pressure_to_psi(value, &reading.evidence.unit))
            })
            .or(self.last_values.fuel_rail_desired_psi.map(f64::from));
        let vgt_actual = profile_reading_value_by_key(&profile_readings, "lly.1543")
            .unwrap_or(f64::from(self.last_values.vgt_actual_pct));
        let vgt_desired = profile_reading_value_by_key(&profile_readings, "lly.1540")
            .unwrap_or(f64::from(self.last_values.vgt_desired_pct));

        let (cylinders, cylinder_states) = profile_cylinder_values(
            selected_profile_definition,
            &profile_readings,
            &self.last_values.cylinders,
        );

        if let (Some(context), Some(selected), Some(_profile)) = (
            profile_context.as_ref(),
            selected_profile.as_ref(),
            selected_profile_definition,
        ) {
            if cycle.is_multiple_of(12) {
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
                self.last_values
                    .barometric_psi
                    .map(|value| f64::from(value) / PSI_PER_KPA)
            }
        });
        let boost_psi = baro_display_kpa
            .map(|baro| ((map_kpa - baro) * PSI_PER_KPA).max(0.0))
            .unwrap_or(0.0);
        let module_scan = if !profile_enabled {
            Vec::new()
        } else if self.cached_modules.is_empty() {
            build_pending_module_scan(session)
        } else {
            self.cached_modules.clone()
        };
        let map_psi = (map_kpa * PSI_PER_KPA) as f32;
        let desired_map_psi = desired_map_kpa.map(|value| (value * PSI_PER_KPA) as f32);
        let barometric_psi = baro_display_kpa.map(|value| (value * PSI_PER_KPA) as f32);
        let boost_psi = boost_psi as f32;
        let maf_g_s = maf_g_s as f32;
        let active_test_runtime = ActiveTestRuntimeValues {
            rpm,
            speed_kph,
            coolant_f,
            voltage,
            last_result: self.last_active_test_result.clone(),
        };
        let active_tests_v2 =
            build_active_tests_v2(selected_profile.as_ref(), &active_test_runtime);
        let profile_values = LivedataProfileValues {
            profile_readings: &profile_readings,
            cylinders: &cylinders,
            cylinder_states: &cylinder_states,
            desired_map_kpa,
        };
        let standard_values = StandardSignalValues {
            rpm,
            rpm_state: state_from_option(rpm_read),
            speed_mph: f64::from(speed_mph),
            speed_state: state_from_option(speed_kph_read),
            voltage,
            voltage_state: "ok",
            coolant_f,
            coolant_state: state_from_option(coolant_c_read),
            intake_air_f,
            intake_air_state: state_from_option(intake_air_c_read),
            map_psi,
            map_state: state_from_option(map_kpa_read),
            barometric_psi,
            barometric_state: if baro_kpa.is_some() {
                "ok"
            } else if baro_display_kpa.is_some() {
                "cached"
            } else {
                "unsupported"
            },
            boost_psi,
            boost_state: if baro_display_kpa.is_some() {
                "ok"
            } else {
                "waiting"
            },
            maf_g_s,
            maf_state: state_from_option(maf_g_s_read),
            fuel_rail_actual_psi: fuel_rail_actual_psi as f32,
            fuel_rail_actual_state: if fuel_rail_kpa.is_some() {
                "ok"
            } else {
                "cached"
            },
            fuel_rail_actual_evidence: actual_fuel_rail_evidence.clone(),
        };
        let signals =
            build_signal_snapshots(selected_profile.as_ref(), &standard_values, &profile_values);
        let diagnostic_service_keys = diagnostic_service_keys(selected_profile.as_ref());
        let active_test_keys = active_tests_v2
            .iter()
            .map(|test| test.key.clone())
            .collect::<Vec<_>>();
        let capability_sections = build_capability_sections(
            &signals,
            diagnostic_service_keys,
            active_test_keys,
            !self.cached_dtcs.is_empty() || !module_scan.is_empty(),
            !source_confidence.is_empty(),
        );
        self.last_values = LastLiveValues {
            voltage: voltage as f32,
            rpm: rpm.max(0.0).round() as u16,
            speed_mph,
            coolant_f: coolant_f as f32,
            intake_air_f: intake_air_f as f32,
            map_psi,
            desired_map_psi,
            barometric_psi,
            maf_g_s,
            fuel_rail_actual_psi: fuel_rail_actual_psi as f32,
            fuel_rail_desired_psi: desired_fuel_rail_psi.map(|value| value as f32),
            vgt_actual_pct: vgt_actual as f32,
            vgt_desired_pct: vgt_desired as f32,
            cylinders,
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
            source_confidence,
            signals,
            capability_sections,
            active_tests_v2,
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

        if !self.has_selected_profile() {
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

        if !self.has_selected_profile() {
            self.cached_dtc_count = None;
            self.cached_dtcs.clear();
            self.cached_modules.clear();
        }
    }

    fn has_selected_profile(&self) -> bool {
        self.profile_state
            .selected
            .as_ref()
            .is_some_and(|selected| builtin_profile(selected.profile_id()).is_some())
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
fn recordings_directory(app: tauri::AppHandle) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?
        .join("recordings");
    fs::create_dir_all(&dir).map_err(|error| {
        format!(
            "failed to create recordings directory {}: {error}",
            dir.display()
        )
    })?;
    Ok(dir.display().to_string())
}

#[tauri::command]
fn read_recording_file(path: String) -> Result<Vec<u8>, String> {
    let path = PathBuf::from(path);
    let metadata = fs::metadata(&path).map_err(|error| {
        format!(
            "failed to inspect recording file {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!("recording path is not a file: {}", path.display()));
    }
    if metadata.len() > MAX_RECORDING_FILE_BYTES {
        return Err(format!(
            "recording file is too large: {} bytes (limit {} bytes)",
            metadata.len(),
            MAX_RECORDING_FILE_BYTES
        ));
    }
    fs::read(&path)
        .map_err(|error| format!("failed to read recording file {}: {error}", path.display()))
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

#[derive(Debug, Clone)]
struct ProfileSignalReading<T> {
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

async fn read_profile_signal(
    session: &mut Session<Elm327Adapter>,
    context: &VehicleContext,
    selected: &SelectedProfile,
    signal: SignalDefinition,
    alerts: &mut Vec<String>,
) -> ProfileSignalReading<f64> {
    let source = profile_signal_source(&signal);
    let confidence = profile_signal_confidence(&signal);
    let mut evidence = profile_signal_evidence(
        signal.key,
        signal.label,
        signal.route.module.canonical(),
        "--",
        signal.source_fields.txd,
        &source,
        &confidence,
        signal.unit,
        None,
        "pending",
        None,
        profile_signal_note(signal).as_deref(),
    );

    let mut sink = GuiDispatchSink::default();
    let registry = ProfileRegistry::with_builtins();
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
            ProfileSignalReading {
                value: Some(decoded.value),
                evidence,
            }
        }
        Ok(ProfileResponse::Dtcs(_)) => {
            alerts.push(format!(
                "{}: selected profile returned DTCs for a signal request",
                signal.label
            ));
            evidence.status = "error".to_string();
            evidence.notes = merge_note(
                evidence.notes.as_deref(),
                "Profile capability returned DTCs for a signal request.",
            );
            ProfileSignalReading {
                value: None,
                evidence,
            }
        }
        Err(error) => {
            push_profile_dispatch_error(alerts, signal.label, &error);
            if let Some(dispatch) = sink.last {
                evidence.module = dispatch.module;
                evidence.node = dispatch.node;
                evidence.request = dispatch.request;
                evidence.response = dispatch.response;
            }
            evidence.status = profile_dispatch_error_label(&error).to_string();
            evidence.notes = merge_note(evidence.notes.as_deref(), &format!("{error:?}"));
            ProfileSignalReading {
                value: None,
                evidence,
            }
        }
    }
}

fn profile_signal_note(signal: SignalDefinition) -> Option<String> {
    match signal.failure_policy {
        ProfileFailurePolicy::PreferStandardPid => {
            Some("Standard PID remains preferred for the displayed value.".to_string())
        }
        ProfileFailurePolicy::CandidateOnly => {
            Some("Candidate profile signal; display until cross-checked against a factory-equivalent tool.".to_string())
        }
        _ => None,
    }
}

fn profile_signals_to_read(profile: &dyn DiagnosticProfile) -> Vec<SignalDefinition> {
    let mut keys = Vec::new();
    let display = profile.signal_display();
    if display.is_empty() {
        return profile
            .signals()
            .iter()
            .copied()
            .filter(signal_should_poll)
            .collect();
    }

    for definition in display {
        match definition.source {
            SignalDisplaySource::ProfileSignal(key) => push_unique_key(&mut keys, key),
            SignalDisplaySource::Derived { input_keys, .. } => {
                for key in input_keys {
                    if profile.signals().iter().any(|signal| signal.key == *key) {
                        push_unique_key(&mut keys, key);
                    }
                }
            }
            SignalDisplaySource::StandardPid(_) => {}
        }
    }

    keys.into_iter()
        .filter_map(|key| {
            profile
                .signals()
                .iter()
                .copied()
                .find(|signal| signal.key == key)
        })
        .filter(signal_should_poll)
        .collect()
}

fn signal_should_poll(signal: &SignalDefinition) -> bool {
    !matches!(signal.failure_policy, ProfileFailurePolicy::DoNotPoll)
}

fn push_unique_key(keys: &mut Vec<&'static str>, key: &'static str) {
    if !keys.contains(&key) {
        keys.push(key);
    }
}

#[cfg(test)]
fn profile_signal_by_did(signals: &[SignalDefinition], did: u16) -> Option<SignalDefinition> {
    signals
        .iter()
        .copied()
        .find(|signal| signal_did(*signal) == Some(did))
}

#[cfg(test)]
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

fn profile_reading_by_key<'a>(
    readings: &'a HashMap<&'static str, ProfileSignalReading<f64>>,
    key: &'static str,
) -> Option<&'a ProfileSignalReading<f64>> {
    readings.get(key)
}

fn profile_reading_value_by_key(
    readings: &HashMap<&'static str, ProfileSignalReading<f64>>,
    key: &'static str,
) -> Option<f64> {
    profile_reading_by_key(readings, key).and_then(|reading| reading.value)
}

fn profile_cylinder_values(
    profile: Option<&dyn DiagnosticProfile>,
    readings: &HashMap<&'static str, ProfileSignalReading<f64>>,
    previous: &[CylinderBalance],
) -> (Vec<CylinderBalance>, Vec<&'static str>) {
    let Some(profile) = profile else {
        return (
            (1..=8)
                .map(|cylinder| CylinderBalance { cylinder, mm3: 0.0 })
                .collect(),
            std::iter::repeat_n("waiting", 8).collect(),
        );
    };

    let mut rows = profile
        .signal_display()
        .iter()
        .filter_map(|display| {
            let SignalDisplaySource::ProfileSignal(key) = display.source else {
                return None;
            };
            let ProfileSignalComposition::TableRow {
                row_index,
                row_label,
                ..
            } = display.composition
            else {
                return None;
            };
            Some((row_index, row_label, key))
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|(row_index, _, _)| *row_index);

    if rows.is_empty() {
        return (
            previous.to_vec(),
            std::iter::repeat_n("cached", previous.len()).collect(),
        );
    }

    let mut cylinders = Vec::with_capacity(rows.len());
    let mut states = Vec::with_capacity(rows.len());
    for (idx, (row_index, row_label, key)) in rows.into_iter().enumerate() {
        let previous_value = previous
            .get(idx)
            .map(|reading| reading.mm3)
            .unwrap_or_default();
        let reading = readings.get(key);
        let value = reading
            .and_then(|reading| reading.value)
            .map(|value| value as f32)
            .unwrap_or(previous_value);
        let cylinder = row_label
            .parse::<u8>()
            .unwrap_or(row_index.saturating_add(1));
        cylinders.push(CylinderBalance {
            cylinder,
            mm3: value,
        });
        states.push(
            reading
                .map(profile_evidence_state_for_reading)
                .unwrap_or("cached"),
        );
    }
    (cylinders, states)
}

fn profile_evidence_state_for_reading(reading: &ProfileSignalReading<f64>) -> &'static str {
    profile_evidence_state(&reading.evidence)
}

fn label_matches_terms(label: &str, terms: &[&str]) -> bool {
    terms
        .iter()
        .all(|term| contains_ascii_ignore_case(label, term))
}

fn contains_ascii_ignore_case(haystack: &str, needle: &str) -> bool {
    let needle = needle.as_bytes();
    if needle.is_empty() {
        return true;
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle))
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

struct StandardSignalValues {
    rpm: f64,
    rpm_state: &'static str,
    speed_mph: f64,
    speed_state: &'static str,
    voltage: f64,
    voltage_state: &'static str,
    coolant_f: f64,
    coolant_state: &'static str,
    intake_air_f: f64,
    intake_air_state: &'static str,
    map_psi: f32,
    map_state: &'static str,
    barometric_psi: Option<f32>,
    barometric_state: &'static str,
    boost_psi: f32,
    boost_state: &'static str,
    maf_g_s: f32,
    maf_state: &'static str,
    fuel_rail_actual_psi: f32,
    fuel_rail_actual_state: &'static str,
    fuel_rail_actual_evidence: SignalEvidence,
}

struct LivedataProfileValues<'a> {
    profile_readings: &'a HashMap<&'static str, ProfileSignalReading<f64>>,
    cylinders: &'a [CylinderBalance],
    cylinder_states: &'a [&'static str],
    desired_map_kpa: Option<f64>,
}

fn state_from_option<T>(value: Option<T>) -> &'static str {
    if value.is_some() {
        "ok"
    } else {
        "cached"
    }
}

fn build_signal_snapshots(
    selected: Option<&SelectedProfile>,
    standard: &StandardSignalValues,
    profile_values: &LivedataProfileValues<'_>,
) -> Vec<SignalSnapshot> {
    let registry = ProfileRegistry::with_builtins();

    if let Some(profile) = selected.and_then(|selected| registry.get(selected.profile_id())) {
        let display = profile.signal_display();
        if !display.is_empty() {
            let mut signals = Vec::with_capacity(display.len());
            let mut display_values = HashMap::new();
            for definition in display {
                let snapshot = display_signal_snapshot(
                    *definition,
                    profile.signals(),
                    standard,
                    profile_values,
                    &display_values,
                );
                display_values.insert(
                    snapshot.key.clone(),
                    RuntimeSignalValue {
                        value: snapshot.value,
                        state: runtime_state_label(&snapshot.state),
                        unit: snapshot.unit.clone(),
                    },
                );
                signals.push(snapshot);
            }
            return signals;
        }
    }

    let mut signals = Vec::new();
    signals.extend(standard_signal_snapshots(standard));
    if let Some(profile) = selected.and_then(|selected| registry.get(selected.profile_id())) {
        signals.reserve(profile.signals().len());
        for signal in profile.signals() {
            signals.push(profile_signal_snapshot(
                *signal,
                profile_values,
                profile_signal_composition(*signal),
            ));
        }
    }
    signals.extend(derived_signal_snapshots(standard));
    signals
}

fn display_signal_snapshot(
    definition: SignalDisplayDefinition,
    profile_signals: &[SignalDefinition],
    standard: &StandardSignalValues,
    profile_values: &LivedataProfileValues<'_>,
    display_values: &HashMap<String, RuntimeSignalValue>,
) -> SignalSnapshot {
    match definition.source {
        SignalDisplaySource::ProfileSignal(key) => profile_signals
            .iter()
            .copied()
            .find(|signal| signal.key == key)
            .map(|signal| {
                profile_signal_snapshot(
                    signal,
                    profile_values,
                    signal_composition_from_profile(definition.composition),
                )
            })
            .map(|snapshot| apply_display_definition(snapshot, definition))
            .unwrap_or_else(|| {
                missing_display_signal_snapshot(definition, "missing profile signal")
            }),
        SignalDisplaySource::StandardPid(pid) => standard_signal_snapshot_for_pid(
            definition,
            pid,
            standard,
            signal_composition_from_profile(definition.composition),
        ),
        SignalDisplaySource::Derived {
            formula_key,
            input_keys,
        } => derived_display_signal_snapshot(
            definition,
            formula_key,
            input_keys,
            standard,
            profile_values,
            display_values,
            signal_composition_from_profile(definition.composition),
        ),
    }
}

#[derive(Clone)]
struct RuntimeSignalValue {
    value: Option<f32>,
    state: &'static str,
    unit: String,
}

fn apply_display_definition(
    mut signal: SignalSnapshot,
    definition: SignalDisplayDefinition,
) -> SignalSnapshot {
    signal.key = definition.key.to_string();
    signal.label = definition.label.to_string();
    signal.category = signal_category_label(definition.category).to_string();
    signal.unit = definition.unit.to_string();
    signal
}

fn missing_display_signal_snapshot(
    definition: SignalDisplayDefinition,
    reason: &'static str,
) -> SignalSnapshot {
    SignalSnapshot {
        key: definition.key.to_string(),
        label: definition.label.to_string(),
        category: signal_category_label(definition.category).to_string(),
        module: "profile".to_string(),
        unit: definition.unit.to_string(),
        value: None,
        state: "error".to_string(),
        confidence: "Rejected".to_string(),
        provenance: Vec::new(),
        source_fields: None,
        request: None,
        decoder_id: None,
        evidence_policy: "None".to_string(),
        failure_policy: "DoNotPoll".to_string(),
        preferred_over: None,
        evidence: None,
        composition: signal_composition_from_profile(definition.composition),
    }
    .with_error_note(reason)
}

trait SignalSnapshotErrorNote {
    fn with_error_note(self, reason: &'static str) -> Self;
}

impl SignalSnapshotErrorNote for SignalSnapshot {
    fn with_error_note(mut self, reason: &'static str) -> Self {
        self.evidence = Some(SignalEvidence {
            key: self.key.clone(),
            label: self.label.clone(),
            module: self.module.clone(),
            node: String::new(),
            request: String::new(),
            source: "profile display".to_string(),
            confidence: self.confidence.clone(),
            status: "error".to_string(),
            unit: self.unit.clone(),
            value: None,
            response: None,
            notes: Some(reason.to_string()),
        });
        self
    }
}

fn standard_signal_snapshots(standard: &StandardSignalValues) -> Vec<SignalSnapshot> {
    vec![
        standard_signal_snapshot(
            "sae.engine_rpm",
            "Engine RPM",
            ProfileSignalCategory::Powertrain,
            "ECM",
            "rpm",
            Some(standard.rpm as f32),
            standard.rpm_state,
            None,
            SignalComposition::Scalar,
        ),
        standard_signal_snapshot(
            "sae.vehicle_speed",
            "Vehicle speed",
            ProfileSignalCategory::Powertrain,
            "ECM",
            "mph",
            Some(standard.speed_mph as f32),
            standard.speed_state,
            None,
            SignalComposition::Scalar,
        ),
        standard_signal_snapshot(
            "sae.battery_voltage",
            "Battery voltage",
            ProfileSignalCategory::Powertrain,
            "Adapter",
            "V",
            Some(standard.voltage as f32),
            standard.voltage_state,
            None,
            SignalComposition::Scalar,
        ),
        standard_signal_snapshot(
            "sae.coolant_temp",
            "Coolant temperature",
            ProfileSignalCategory::Powertrain,
            "ECM",
            "deg F",
            Some(standard.coolant_f as f32),
            standard.coolant_state,
            None,
            SignalComposition::Scalar,
        ),
        standard_signal_snapshot(
            "sae.intake_air_temp",
            "Intake air temperature",
            ProfileSignalCategory::Powertrain,
            "ECM",
            "deg F",
            Some(standard.intake_air_f as f32),
            standard.intake_air_state,
            None,
            SignalComposition::Scalar,
        ),
        standard_signal_snapshot(
            "sae.intake_map",
            "Intake MAP",
            ProfileSignalCategory::Turbo,
            "ECM",
            "psi",
            Some(standard.map_psi),
            standard.map_state,
            None,
            SignalComposition::Pair {
                group_key: "map_pressure".to_string(),
                group_label: Some("MAP pressure".to_string()),
                role: "actual".to_string(),
            },
        ),
        standard_signal_snapshot(
            "sae.barometric_pressure",
            "Barometric pressure",
            ProfileSignalCategory::Turbo,
            "ECM",
            "psi",
            standard.barometric_psi,
            standard.barometric_state,
            None,
            SignalComposition::Scalar,
        ),
        standard_signal_snapshot(
            "sae.maf",
            "Mass air flow",
            ProfileSignalCategory::Turbo,
            "ECM",
            "g/s",
            Some(standard.maf_g_s),
            standard.maf_state,
            None,
            SignalComposition::Scalar,
        ),
        standard_signal_snapshot(
            "sae.fuel_rail_pressure",
            "Actual fuel rail pressure",
            ProfileSignalCategory::Fuel,
            "ECM",
            "psi",
            Some(standard.fuel_rail_actual_psi),
            standard.fuel_rail_actual_state,
            Some(standard.fuel_rail_actual_evidence.clone()),
            SignalComposition::Pair {
                group_key: "fuel_rail_pressure".to_string(),
                group_label: Some("Fuel rail pressure".to_string()),
                role: "actual".to_string(),
            },
        ),
    ]
}

fn standard_signal_snapshot_for_pid(
    definition: SignalDisplayDefinition,
    pid: u8,
    standard: &StandardSignalValues,
    composition: SignalComposition,
) -> SignalSnapshot {
    let (value, state, evidence, fallback_module) = standard_pid_runtime(pid, standard);
    let mut snapshot = standard_signal_snapshot(
        definition.key,
        definition.label,
        definition.category,
        fallback_module,
        definition.unit,
        value,
        state,
        evidence,
        composition,
    );
    snapshot.decoder_id = Some(format!("sae.pid.{pid:02X}"));
    snapshot
}

fn standard_pid_runtime(
    pid: u8,
    standard: &StandardSignalValues,
) -> (
    Option<f32>,
    &'static str,
    Option<SignalEvidence>,
    &'static str,
) {
    match pid {
        0x05 => (
            Some(standard.coolant_f as f32),
            standard.coolant_state,
            None,
            "ECM",
        ),
        0x0B => (Some(standard.map_psi), standard.map_state, None, "ECM"),
        0x0C => (Some(standard.rpm as f32), standard.rpm_state, None, "ECM"),
        0x0D => (
            Some(standard.speed_mph as f32),
            standard.speed_state,
            None,
            "ECM",
        ),
        0x0F => (
            Some(standard.intake_air_f as f32),
            standard.intake_air_state,
            None,
            "ECM",
        ),
        0x10 => (Some(standard.maf_g_s), standard.maf_state, None, "ECM"),
        0x23 => (
            Some(standard.fuel_rail_actual_psi),
            standard.fuel_rail_actual_state,
            Some(standard.fuel_rail_actual_evidence.clone()),
            "ECM",
        ),
        0x33 => (
            standard.barometric_psi,
            standard.barometric_state,
            None,
            "ECM",
        ),
        0x46 => (None, "unsupported", None, "ECM"),
        0x5C => (None, "unsupported", None, "ECM"),
        _ => (None, "unsupported", None, "ECM"),
    }
}

#[allow(clippy::too_many_arguments)]
fn standard_signal_snapshot(
    key: &str,
    label: &str,
    category: ProfileSignalCategory,
    module: &str,
    unit: &str,
    value: Option<f32>,
    state: &str,
    evidence: Option<SignalEvidence>,
    composition: SignalComposition,
) -> SignalSnapshot {
    SignalSnapshot {
        key: key.to_string(),
        label: label.to_string(),
        category: signal_category_label(category).to_string(),
        module: module.to_string(),
        unit: unit.to_string(),
        value,
        state: state.to_string(),
        confidence: "Verified".to_string(),
        provenance: vec!["SaeStandard".to_string()],
        source_fields: None,
        request: None,
        decoder_id: Some("sae.pid".to_string()),
        evidence_policy: "None".to_string(),
        failure_policy: "SurfaceUnavailable".to_string(),
        preferred_over: None,
        evidence,
        composition,
    }
}

fn profile_signal_snapshot(
    signal: SignalDefinition,
    values: &LivedataProfileValues<'_>,
    composition: SignalComposition,
) -> SignalSnapshot {
    let (value, state, evidence) = profile_signal_runtime(signal, values);
    SignalSnapshot {
        key: signal.key.to_string(),
        label: signal.label.to_string(),
        category: signal_category_label(signal.category).to_string(),
        module: signal.route.module.canonical().to_string(),
        unit: signal.unit.to_string(),
        value,
        state: state.to_string(),
        confidence: profile_confidence_variant(signal.confidence).to_string(),
        provenance: signal
            .provenance
            .iter()
            .map(|provenance| profile_provenance_variant(*provenance).to_string())
            .collect(),
        source_fields: Some(profile_source_fields(signal)),
        request: Some(profile_request_hex(signal)),
        decoder_id: Some(signal.decoder_id.to_string()),
        evidence_policy: profile_evidence_policy_label(signal.evidence_policy).to_string(),
        failure_policy: profile_failure_policy_label(signal.failure_policy).to_string(),
        preferred_over: signal.preferred_over.map(str::to_string),
        evidence,
        composition,
    }
}

fn profile_signal_runtime(
    signal: SignalDefinition,
    values: &LivedataProfileValues<'_>,
) -> (Option<f32>, &'static str, Option<SignalEvidence>) {
    if let Some(reading) = values.profile_readings.get(signal.key) {
        return profile_read_runtime(Some(reading), |value| value as f32);
    }

    match signal.composition_hint() {
        SignalRuntimeHint::InjectorBalanceRow(row_index) => {
            let idx = usize::from(row_index.saturating_sub(1));
            let value = values.cylinders.get(idx).map(|reading| reading.mm3);
            let state = values
                .cylinder_states
                .get(idx)
                .copied()
                .unwrap_or("waiting");
            (value, state, None)
        }
        SignalRuntimeHint::DesiredMapCandidate => (
            values.desired_map_kpa.map(|value| value as f32),
            if values.desired_map_kpa.is_some() {
                "cached"
            } else {
                "waiting"
            },
            None,
        ),
        SignalRuntimeHint::Scalar => (None, "waiting", None),
    }
}

fn profile_read_runtime(
    reading: Option<&ProfileSignalReading<f64>>,
    convert: impl Fn(f64) -> f32,
) -> (Option<f32>, &'static str, Option<SignalEvidence>) {
    match reading {
        Some(reading) => (
            reading.value.map(convert),
            profile_evidence_state(&reading.evidence),
            Some(reading.evidence.clone()),
        ),
        None => (None, "waiting", None),
    }
}

enum SignalRuntimeHint {
    Scalar,
    DesiredMapCandidate,
    InjectorBalanceRow(u8),
}

trait SignalRuntimeHints {
    fn composition_hint(&self) -> SignalRuntimeHint;
}

impl SignalRuntimeHints for SignalDefinition {
    fn composition_hint(&self) -> SignalRuntimeHint {
        if label_matches_terms(self.label, &["injector", "balance", "cyl"]) {
            let row = self
                .label
                .rsplit_once(' ')
                .and_then(|(_, suffix)| suffix.parse::<u8>().ok())
                .unwrap_or(0);
            return SignalRuntimeHint::InjectorBalanceRow(row);
        }
        if label_matches_terms(self.label, &["desired", "map"]) {
            return SignalRuntimeHint::DesiredMapCandidate;
        }
        SignalRuntimeHint::Scalar
    }
}

fn profile_evidence_state(evidence: &SignalEvidence) -> &'static str {
    match evidence.status.as_str() {
        "success" => "ok",
        "pending" => "waiting",
        "cached" | "fallback-gm" => "cached",
        "unsupported" | "unavailable" | "missing-signal" | "no data" => "unsupported",
        _ if evidence.value.is_some() => "ok",
        _ => "error",
    }
}

fn derived_signal_snapshots(standard: &StandardSignalValues) -> Vec<SignalSnapshot> {
    let mut derived = Vec::with_capacity(4);
    derived.push(derived_signal_snapshot(
        "derived.boost_pressure",
        "Boost pressure",
        ProfileSignalCategory::Turbo,
        "psi",
        Some(standard.boost_psi),
        standard.boost_state,
        "map_minus_baro",
        "map_pressure",
        vec!["sae.intake_map", "sae.barometric_pressure"],
    ));
    derived
}

fn derived_display_signal_snapshot(
    definition: SignalDisplayDefinition,
    formula_key: &'static str,
    input_keys: &'static [&'static str],
    standard: &StandardSignalValues,
    profile_values: &LivedataProfileValues<'_>,
    display_values: &HashMap<String, RuntimeSignalValue>,
    composition: SignalComposition,
) -> SignalSnapshot {
    let (value, state) = display_formula_runtime(
        formula_key,
        input_keys,
        definition.unit,
        standard,
        profile_values,
        display_values,
    );
    SignalSnapshot {
        key: definition.key.to_string(),
        label: definition.label.to_string(),
        category: signal_category_label(definition.category).to_string(),
        module: "derived".to_string(),
        unit: definition.unit.to_string(),
        value,
        state: state.to_string(),
        confidence: display_formula_confidence(formula_key, input_keys).to_string(),
        provenance: vec!["LocalFixture".to_string()],
        source_fields: None,
        request: None,
        decoder_id: Some("derived".to_string()),
        evidence_policy: "None".to_string(),
        failure_policy: "SurfaceUnavailable".to_string(),
        preferred_over: None,
        evidence: None,
        composition,
    }
}

fn display_formula_runtime(
    formula_key: &str,
    input_keys: &[&str],
    output_unit: &str,
    standard: &StandardSignalValues,
    profile_values: &LivedataProfileValues<'_>,
    display_values: &HashMap<String, RuntimeSignalValue>,
) -> (Option<f32>, &'static str) {
    match formula_key {
        "actual_minus_desired" if input_keys.len() == 2 => {
            let actual = resolve_formula_input(
                input_keys[0],
                output_unit,
                standard,
                profile_values,
                display_values,
            );
            let desired = resolve_formula_input(
                input_keys[1],
                output_unit,
                standard,
                profile_values,
                display_values,
            );
            (
                actual
                    .value
                    .zip(desired.value)
                    .map(|(actual, desired)| actual - desired),
                combine_binary_state(&actual, &desired),
            )
        }
        "first_available" => {
            for key in input_keys {
                let input = resolve_formula_input(
                    key,
                    output_unit,
                    standard,
                    profile_values,
                    display_values,
                );
                if input.value.is_some() {
                    return (input.value, input.state);
                }
            }
            (None, "waiting")
        }
        "max_zero_subtract" if input_keys.len() == 2 => {
            let minuend = resolve_formula_input(
                input_keys[0],
                output_unit,
                standard,
                profile_values,
                display_values,
            );
            let subtrahend = resolve_formula_input(
                input_keys[1],
                output_unit,
                standard,
                profile_values,
                display_values,
            );
            (
                minuend
                    .value
                    .zip(subtrahend.value)
                    .map(|(actual, desired)| (actual - desired).max(0.0)),
                combine_binary_state(&minuend, &subtrahend),
            )
        }
        "profile_desired_map_to_psi" if input_keys.len() == 1 => {
            let input = resolve_formula_input(
                input_keys[0],
                output_unit,
                standard,
                profile_values,
                display_values,
            );
            if input.value.is_some() {
                (input.value, input.state)
            } else {
                (
                    convert_unit(
                        profile_values.desired_map_kpa.map(|value| value as f32),
                        "kPa abs",
                        output_unit,
                    ),
                    if profile_values.desired_map_kpa.is_some() {
                        "cached"
                    } else {
                        input.state
                    },
                )
            }
        }
        _ => (None, "unsupported"),
    }
}

fn resolve_formula_input(
    key: &str,
    output_unit: &str,
    standard: &StandardSignalValues,
    profile_values: &LivedataProfileValues<'_>,
    display_values: &HashMap<String, RuntimeSignalValue>,
) -> RuntimeSignalValue {
    if let Some(value) = display_values.get(key) {
        return RuntimeSignalValue {
            value: convert_unit(value.value, &value.unit, output_unit),
            state: value.state,
            unit: output_unit.to_string(),
        };
    }

    if let Some(pid) = standard_key_pid(key) {
        let (value, state, _, _) = standard_pid_runtime(pid, standard);
        return RuntimeSignalValue {
            value: convert_unit(value, standard_pid_unit(pid), output_unit),
            state,
            unit: output_unit.to_string(),
        };
    }

    if let Some(reading) = profile_values.profile_readings.get(key) {
        return RuntimeSignalValue {
            value: convert_unit(
                reading.value.map(|value| value as f32),
                &reading.evidence.unit,
                output_unit,
            ),
            state: profile_evidence_state(&reading.evidence),
            unit: output_unit.to_string(),
        };
    }

    RuntimeSignalValue {
        value: None,
        state: "waiting",
        unit: output_unit.to_string(),
    }
}

fn standard_key_pid(key: &str) -> Option<u8> {
    let hex = key.strip_prefix("standard:")?;
    u8::from_str_radix(hex, 16).ok()
}

fn standard_pid_unit(pid: u8) -> &'static str {
    match pid {
        0x05 | 0x0F | 0x46 | 0x5C => "F",
        0x0B | 0x23 | 0x33 => "psi",
        0x0C => "rpm",
        0x0D => "mph",
        0x10 => "g/s",
        _ => "",
    }
}

fn convert_unit(value: Option<f32>, from_unit: &str, to_unit: &str) -> Option<f32> {
    let value = value?;
    if units_equivalent(from_unit, to_unit) {
        return Some(value);
    }
    match (from_unit.trim(), to_unit.trim()) {
        ("kPa", "psi") | ("kPa abs", "psi") => Some(value * PSI_PER_KPA as f32),
        ("psi", "kPa") | ("psi", "kPa abs") => Some(value / PSI_PER_KPA as f32),
        _ => Some(value),
    }
}

fn units_equivalent(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    left == right || matches!((left, right), ("F", "deg F") | ("deg F", "F"))
}

fn combine_binary_state(left: &RuntimeSignalValue, right: &RuntimeSignalValue) -> &'static str {
    if left.value.is_some() && right.value.is_some() {
        "ok"
    } else if matches!(left.state, "unsupported") || matches!(right.state, "unsupported") {
        "unsupported"
    } else {
        "waiting"
    }
}

fn runtime_state_label(state: &str) -> &'static str {
    match state {
        "ok" => "ok",
        "cached" => "cached",
        "unsupported" => "unsupported",
        "error" => "error",
        _ => "waiting",
    }
}

fn display_formula_confidence(formula_key: &str, _input_keys: &[&str]) -> &'static str {
    if formula_key == "profile_desired_map_to_psi" {
        "Candidate"
    } else {
        "LiveObserved"
    }
}

#[allow(clippy::too_many_arguments)]
fn derived_signal_snapshot(
    key: &str,
    label: &str,
    category: ProfileSignalCategory,
    unit: &str,
    value: Option<f32>,
    state: &str,
    formula_key: &str,
    group_key: &str,
    input_keys: Vec<&str>,
) -> SignalSnapshot {
    SignalSnapshot {
        key: key.to_string(),
        label: label.to_string(),
        category: signal_category_label(category).to_string(),
        module: "derived".to_string(),
        unit: unit.to_string(),
        value,
        state: state.to_string(),
        confidence: "LiveObserved".to_string(),
        provenance: vec!["LocalFixture".to_string()],
        source_fields: None,
        request: None,
        decoder_id: Some("derived".to_string()),
        evidence_policy: "None".to_string(),
        failure_policy: "SurfaceUnavailable".to_string(),
        preferred_over: None,
        evidence: None,
        composition: SignalComposition::Derived {
            group_key: group_key.to_string(),
            group_label: Some(title_from_group_key(group_key)),
            formula_key: formula_key.to_string(),
            input_keys: input_keys.into_iter().map(str::to_string).collect(),
        },
    }
}

fn signal_composition_from_profile(composition: ProfileSignalComposition) -> SignalComposition {
    match composition {
        ProfileSignalComposition::Scalar => SignalComposition::Scalar,
        ProfileSignalComposition::Pair { group_key, role } => SignalComposition::Pair {
            group_key: group_key.to_string(),
            group_label: Some(title_from_group_key(group_key)),
            role: profile_pair_role_label(role).to_string(),
        },
        ProfileSignalComposition::TableRow {
            table_key,
            row_index,
            row_label,
        } => SignalComposition::TableRow {
            table_key: table_key.to_string(),
            table_label: Some(title_from_group_key(table_key)),
            row_index,
            row_label: row_label.to_string(),
        },
    }
}

fn profile_pair_role_label(role: ProfilePairRole) -> &'static str {
    match role {
        ProfilePairRole::Actual => "actual",
        ProfilePairRole::Desired => "desired",
        ProfilePairRole::Error => "error",
        ProfilePairRole::Delta => "delta",
    }
}

fn title_from_group_key(group_key: &str) -> String {
    match group_key {
        "fuel_rail_pressure" => "Fuel rail pressure".to_string(),
        "map_pressure" => "MAP pressure".to_string(),
        "vgt_vane_position" => "VGT vane position".to_string(),
        _ => titleize_key_segment(group_key),
    }
}

fn titleize_key_segment(key: &str) -> String {
    let segment = key.rsplit('.').next().unwrap_or(key);
    let mut out = String::with_capacity(segment.len());
    let mut uppercase_next = true;
    for ch in segment.chars() {
        if matches!(ch, '_' | '-' | '.') {
            if !out.ends_with(' ') {
                out.push(' ');
            }
            uppercase_next = true;
        } else if uppercase_next {
            out.extend(ch.to_uppercase());
            uppercase_next = false;
        } else {
            out.extend(ch.to_lowercase());
        }
    }

    if out.is_empty() {
        "Signal group".to_string()
    } else {
        out
    }
}

fn profile_signal_composition(signal: SignalDefinition) -> SignalComposition {
    if label_matches_terms(signal.label, &["injector", "balance", "cyl"]) {
        let row_index = signal
            .label
            .rsplit_once(' ')
            .and_then(|(_, suffix)| suffix.parse::<u8>().ok())
            .unwrap_or(0);
        SignalComposition::TableRow {
            table_key: "injector_balance".to_string(),
            table_label: Some("Injector balance".to_string()),
            row_index,
            row_label: format!("Cylinder {row_index}"),
        }
    } else {
        SignalComposition::Scalar
    }
}

fn profile_source_fields(signal: SignalDefinition) -> SignalSourceFields {
    SignalSourceFields {
        txd: signal.source_fields.txd.to_string(),
        rxf: signal.source_fields.rxf.map(str::to_string),
        rxd: signal.source_fields.rxd.map(|rxd| SignalRxdSource {
            raw: rxd.raw.to_string(),
            bit_width: rxd.bit_width,
        }),
        raw_mth: signal.source_fields.raw_mth.map(str::to_string),
        source_ref: signal.source_fields.source_ref.map(str::to_string),
    }
}

fn profile_request_hex(signal: SignalDefinition) -> String {
    let mut bytes = Vec::with_capacity(1 + signal.request_data.len());
    bytes.push(signal.service_id);
    bytes.extend_from_slice(signal.request_data);
    spaced_hex(&bytes)
}

fn build_capability_sections(
    signals: &[SignalSnapshot],
    diagnostic_service_keys: Vec<String>,
    active_test_keys: Vec<String>,
    has_diagnostics: bool,
    has_evidence: bool,
) -> Vec<CapabilitySection> {
    let mut sections = Vec::new();
    for (category, label) in [
        ("Powertrain", "Powertrain"),
        ("Turbo", "Turbo"),
        ("Fuel", "Fuel"),
        ("Transmission", "Transmission"),
        ("Body", "Body"),
        ("Chassis", "Chassis"),
        ("Emissions", "Emissions"),
        ("Other", "Other"),
    ] {
        let signal_keys = signals
            .iter()
            .filter(|signal| normal_section_signal(signal, category))
            .map(|signal| signal.key.clone())
            .collect::<Vec<_>>();
        if !signal_keys.is_empty() {
            sections.push(CapabilitySection {
                id: category.to_ascii_lowercase(),
                category: category.to_string(),
                label: label.to_string(),
                signal_keys,
                active_test_keys: Vec::new(),
                diagnostic_service_keys: Vec::new(),
                visible: true,
            });
        }
    }

    let discovery_keys = signals
        .iter()
        .filter(|signal| signal.confidence == "Candidate")
        .map(|signal| signal.key.clone())
        .collect::<Vec<_>>();
    if !discovery_keys.is_empty() {
        sections.push(CapabilitySection {
            id: "discovery".to_string(),
            category: "Discovery".to_string(),
            label: "Discovery".to_string(),
            signal_keys: discovery_keys,
            active_test_keys: Vec::new(),
            diagnostic_service_keys: Vec::new(),
            visible: true,
        });
    }

    if has_diagnostics || !diagnostic_service_keys.is_empty() {
        sections.push(CapabilitySection {
            id: "diagnostics".to_string(),
            category: "Diagnostics".to_string(),
            label: "Diagnostics".to_string(),
            signal_keys: Vec::new(),
            active_test_keys: Vec::new(),
            diagnostic_service_keys,
            visible: true,
        });
    }
    if !active_test_keys.is_empty() {
        sections.push(CapabilitySection {
            id: "active-tests".to_string(),
            category: "ActiveTests".to_string(),
            label: "Active Tests".to_string(),
            signal_keys: Vec::new(),
            active_test_keys,
            diagnostic_service_keys: Vec::new(),
            visible: true,
        });
    }
    if has_evidence {
        sections.push(CapabilitySection {
            id: "evidence".to_string(),
            category: "Evidence".to_string(),
            label: "Evidence".to_string(),
            signal_keys: Vec::new(),
            active_test_keys: Vec::new(),
            diagnostic_service_keys: Vec::new(),
            visible: true,
        });
    }

    sections
}

fn normal_section_signal(signal: &SignalSnapshot, category: &str) -> bool {
    signal.category == category
        && signal.confidence != "Candidate"
        && signal.confidence != "Rejected"
        && signal.failure_policy != "DoNotPoll"
}

fn diagnostic_service_keys(selected: Option<&SelectedProfile>) -> Vec<String> {
    let registry = ProfileRegistry::with_builtins();
    selected
        .and_then(|selected| registry.get(selected.profile_id()))
        .map(|profile| {
            profile
                .dtc_services()
                .iter()
                .map(|service| service.key.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn build_active_tests_v2(
    selected: Option<&SelectedProfile>,
    runtime: &ActiveTestRuntimeValues,
) -> Vec<ActiveTestSnapshotV2> {
    let registry = ProfileRegistry::with_builtins();
    let Some(profile) = selected.and_then(|selected| registry.get(selected.profile_id())) else {
        return Vec::new();
    };

    profile
        .active_tests()
        .iter()
        .map(|definition| active_test_snapshot_v2(*definition, profile, runtime))
        .collect()
}

fn active_test_snapshot_v2(
    definition: ActiveTestDefinition,
    profile: &dyn obd2_dash::profiles::DiagnosticProfile,
    runtime: &ActiveTestRuntimeValues,
) -> ActiveTestSnapshotV2 {
    let command_profile = match definition.command_profile {
        ProfileActiveCommandProfile::Locked => "Locked",
        ProfileActiveCommandProfile::Verified(_) => "Verified",
    };
    let forbidden_module = active_test_write_forbidden_module(definition, profile);
    let actionable = matches!(
        definition.command_profile,
        ProfileActiveCommandProfile::Verified(_)
    ) && !matches!(definition.safety_class, ProfileSafetyClass::Locked)
        && forbidden_module.is_none();

    let preconditions = active_test_preconditions(definition, runtime);
    let last_result = runtime.last_result.clone();

    ActiveTestSnapshotV2 {
        key: definition.key.to_string(),
        label: definition.label.to_string(),
        safety_class: profile_safety_class_label(definition.safety_class).to_string(),
        command_profile: command_profile.to_string(),
        actionable,
        lock_reason: active_test_lock_reason(definition, forbidden_module),
        supported_modes: definition
            .supported_modes
            .iter()
            .map(|mode| (*mode).to_string())
            .collect(),
        safety_notes: active_test_safety_notes(definition),
        preconditions,
        timeout_ms: definition.timeout.as_millis() as u64,
        cancel_available: definition.cancel_command.is_some(),
        evidence_policy: profile_evidence_policy_label(definition.evidence_policy).to_string(),
        last_result,
    }
}

fn active_test_write_forbidden_module(
    definition: ActiveTestDefinition,
    profile: &dyn obd2_dash::profiles::DiagnosticProfile,
) -> Option<String> {
    let ProfileActiveCommandProfile::Verified(request) = definition.command_profile else {
        return None;
    };
    let module_key = request.route.module;
    let module = profile
        .module_map()?
        .modules
        .iter()
        .find(|module| module.key == module_key)?;
    matches!(
        module.safety_class,
        ProfileModuleSafetyClass::WriteForbidden
    )
    .then(|| module.display_label.to_string())
}

fn active_test_lock_reason(
    definition: ActiveTestDefinition,
    forbidden_module: Option<String>,
) -> Option<String> {
    if let Some(module) = forbidden_module {
        return Some(format!(
            "Resolved target module {module} is write-forbidden; command payload is disabled."
        ));
    }
    if let Some(reason) = definition.lock_reason {
        Some(reason.to_string())
    } else if matches!(
        definition.command_profile,
        ProfileActiveCommandProfile::Locked
    ) {
        Some("Verified command bytes are required before execution.".to_string())
    } else {
        None
    }
}

fn active_test_safety_notes(definition: ActiveTestDefinition) -> Vec<String> {
    if definition.safety_notes.is_empty() {
        return definition
            .preconditions
            .iter()
            .map(|precondition| precondition.detail.to_string())
            .collect();
    }
    definition
        .safety_notes
        .iter()
        .map(|note| (*note).to_string())
        .collect()
}

fn signal_category_label(category: ProfileSignalCategory) -> &'static str {
    match category {
        ProfileSignalCategory::Powertrain => "Powertrain",
        ProfileSignalCategory::Turbo => "Turbo",
        ProfileSignalCategory::Fuel => "Fuel",
        ProfileSignalCategory::Transmission => "Transmission",
        ProfileSignalCategory::Body => "Body",
        ProfileSignalCategory::Chassis => "Chassis",
        ProfileSignalCategory::Emissions => "Emissions",
        ProfileSignalCategory::Other => "Other",
    }
}

fn profile_confidence_variant(confidence: ProfileConfidence) -> &'static str {
    match confidence {
        ProfileConfidence::Candidate => "Candidate",
        ProfileConfidence::LiveObserved => "LiveObserved",
        ProfileConfidence::Community => "Community",
        ProfileConfidence::Verified => "Verified",
        ProfileConfidence::Rejected => "Rejected",
    }
}

fn profile_provenance_variant(provenance: ProfileProvenance) -> &'static str {
    match provenance {
        ProfileProvenance::ScanGaugePublished => "ScanGaugePublished",
        ProfileProvenance::LiveObserved => "LiveObserved",
        ProfileProvenance::LegacySpec => "LegacySpec",
        ProfileProvenance::LocalRejection => "LocalRejection",
        ProfileProvenance::LocalFixture => "LocalFixture",
    }
}

fn profile_failure_policy_label(policy: ProfileFailurePolicy) -> &'static str {
    match policy {
        ProfileFailurePolicy::SurfaceUnavailable => "SurfaceUnavailable",
        ProfileFailurePolicy::PreferStandardPid => "PreferStandardPid",
        ProfileFailurePolicy::CandidateOnly => "CandidateOnly",
        ProfileFailurePolicy::DoNotPoll => "DoNotPoll",
    }
}

fn profile_evidence_policy_label(policy: ProfileEvidencePolicy) -> &'static str {
    match policy {
        ProfileEvidencePolicy::None => "None",
        ProfileEvidencePolicy::OnError => "OnError",
        ProfileEvidencePolicy::OnDemand => "OnDemand",
        ProfileEvidencePolicy::BoundedLive => "BoundedLive",
        ProfileEvidencePolicy::Always => "Always",
    }
}

fn profile_safety_class_label(safety_class: ProfileSafetyClass) -> &'static str {
    match safety_class {
        ProfileSafetyClass::Passive => "Passive",
        ProfileSafetyClass::StationaryOnly => "StationaryOnly",
        ProfileSafetyClass::IdleOnly => "IdleOnly",
        ProfileSafetyClass::Locked => "Locked",
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
        let CapabilityId::DtcService(_key) = planned.capability else {
            continue;
        };

        let module = planned.route.module.to_core_module_id();
        let module_label = module.0.clone();
        let standard_label = generic_standard_label(&generic, &module_label);
        let row = ensure_module_scan(&mut modules, &module_label, standard_label);
        let request = GuiProfileDtcRequest {
            dtcs: &mut dtcs,
            runtime: &runtime,
            context,
            selected,
            capability: planned.capability,
            request: planned.request,
            fallback_module: &module,
            alerts,
        };
        let label = execute_profile_dtc_request(session, request).await;

        assign_profile_scan_result(row, label);
    }

    if modules.is_empty() {
        modules = build_pending_module_scan(session);
    }

    GmClass2Scan { dtcs, modules }
}

struct GuiProfileDtcRequest<'a, 'runtime> {
    dtcs: &'a mut Vec<DtcSnapshot>,
    runtime: &'a ProfileRuntime<'runtime>,
    context: &'a VehicleContext,
    selected: &'a SelectedProfile,
    capability: CapabilityId,
    request: RequestId,
    fallback_module: &'a ModuleId,
    alerts: &'a mut Vec<String>,
}

fn assign_profile_scan_result(row: &mut ModuleScan, label: String) {
    if row.gm_all == "pending" {
        row.gm_all = label;
    } else if row.gm_active == "pending" {
        row.gm_active = label;
    }
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
    request: GuiProfileDtcRequest<'_, '_>,
) -> String {
    let GuiProfileDtcRequest {
        dtcs,
        runtime,
        context,
        selected,
        capability,
        request,
        fallback_module,
        alerts,
    } = request;
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

fn active_test_preconditions(
    definition: ActiveTestDefinition,
    runtime: &ActiveTestRuntimeValues,
) -> Vec<GmActiveTestPrecondition> {
    definition
        .preconditions
        .iter()
        .map(|precondition| {
            if label_matches_terms(precondition.label, &["verified", "command"]) {
                active_precondition(
                    precondition.label,
                    matches!(
                        definition.command_profile,
                        ProfileActiveCommandProfile::Verified(_)
                    ),
                    precondition.detail,
                )
            } else if label_matches_terms(precondition.label, &["stationary"]) {
                active_precondition(
                    precondition.label,
                    runtime.speed_kph < 0.5,
                    format!("{:.1} mph", runtime.speed_kph * MPH_PER_KPH),
                )
            } else if label_matches_terms(precondition.label, &["idle"]) {
                active_precondition(
                    precondition.label,
                    (500.0..=900.0).contains(&runtime.rpm),
                    format!("{:.0} rpm", runtime.rpm),
                )
            } else if label_matches_terms(precondition.label, &["coolant"])
                || label_matches_terms(precondition.label, &["warm"])
            {
                active_precondition(
                    precondition.label,
                    runtime.coolant_f >= 104.0,
                    format!("{:.1} F", runtime.coolant_f),
                )
            } else if label_matches_terms(precondition.label, &["voltage"])
                || label_matches_terms(precondition.label, &["battery"])
            {
                active_precondition(
                    precondition.label,
                    runtime.voltage >= 12.0,
                    format!("{:.1} V", runtime.voltage),
                )
            } else {
                active_precondition(precondition.label, false, precondition.detail)
            }
        })
        .collect()
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
        source_confidence: Vec::new(),
        signals: Vec::new(),
        capability_sections: Vec::new(),
        active_tests_v2: Vec::new(),
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(GuiState {
            backend: Mutex::new(LiveBackend::default()),
        })
        .invoke_handler(tauri::generate_handler![
            diagnostic_snapshot,
            recordings_directory,
            read_recording_file,
            request_active_test
        ])
        .run(tauri::generate_context!())
        .expect("failed to run OBD2 Dash GUI");
}

#[cfg(test)]
mod tests {
    use super::*;
    use obd2_core::protocol::codec::BusFamily;
    use obd2_core::vehicle::Protocol as CoreProtocol;
    use obd2_dash::profiles::{
        ActivePrecondition, AddressState, AddressTemplate, BusDefinition, BusKey,
        DiagnosticProfile, Manufacturer, ModuleDefinition, ModuleMap, ModuleSafetyClass, ProfileId,
        ProfileMatch, ProfileRequestDefinition, RouteDefinition, StandardPidOverride,
        StandardPidPolicy,
    };

    const TEST_LLY_PROFILE_ID: &str = "gm.gmt800.lly.class2";
    const TEST_LLY_DESIRED_FUEL_PRESSURE_DID: u16 = 0x163D;
    const TEST_LLY_BAROMETRIC_PRESSURE_DID: u16 = 0x1251;
    const TEST_LLY_DESIRED_MAP_DID: u16 = 0x1542;

    static TEST_BUSES: &[BusDefinition] = &[BusDefinition {
        key: BusKey::new("test"),
        family: BusFamily::J1850,
        protocol: CoreProtocol::J1850Vpw,
        j1850: None,
        label: "test bus",
    }];
    static TEST_MODULES: &[ModuleDefinition] = &[ModuleDefinition {
        key: ModuleKey::Sdm,
        display_label: "Sensing and Diagnostic Module",
        bus: BusKey::new("test"),
        address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x58 }),
        safety_class: ModuleSafetyClass::WriteForbidden,
        coresident_with: None,
    }];
    static TEST_MODULE_MAP: ModuleMap = ModuleMap {
        buses: TEST_BUSES,
        modules: TEST_MODULES,
    };
    static TEST_ACTIVE_PRECONDITIONS: &[ActivePrecondition] = &[ActivePrecondition {
        label: "fixture",
        detail: "fixture precondition",
    }];
    static TEST_FORBIDDEN_ACTIVE_TESTS: &[ActiveTestDefinition] = &[ActiveTestDefinition {
        key: "test.write_forbidden",
        label: "Write forbidden command",
        safety_class: ProfileSafetyClass::StationaryOnly,
        command_profile: ProfileActiveCommandProfile::Verified(ProfileRequestDefinition {
            route: RouteDefinition {
                module: ModuleKey::Sdm,
            },
            service_id: 0x31,
            request_data: &[0x01],
        }),
        preconditions: TEST_ACTIVE_PRECONDITIONS,
        lock_reason: Some("fixture lock reason"),
        supported_modes: &["fixture mode"],
        safety_notes: &["fixture safety note"],
        timeout: std::time::Duration::from_millis(500),
        cancel_command: None,
        evidence_policy: ProfileEvidencePolicy::OnError,
    }];

    struct WriteForbiddenProfile;

    impl DiagnosticProfile for WriteForbiddenProfile {
        fn id(&self) -> ProfileId {
            ProfileId::new("test.write-forbidden")
        }

        fn manufacturer(&self) -> Manufacturer {
            Manufacturer::Fixture
        }

        fn module_map(&self) -> Option<&ModuleMap> {
            Some(&TEST_MODULE_MAP)
        }

        fn matches(&self, _ctx: &VehicleContext) -> ProfileMatch {
            ProfileMatch::NoMatch
        }

        fn standard_pid_overrides(&self) -> &[StandardPidOverride] {
            &[]
        }

        fn standard_pid_policy(&self) -> StandardPidPolicy {
            StandardPidPolicy::EMPTY
        }

        fn signals(&self) -> &[SignalDefinition] {
            &[]
        }

        fn dtc_services(&self) -> &[obd2_dash::profiles::DtcServiceDefinition] {
            &[]
        }

        fn active_tests(&self) -> &[ActiveTestDefinition] {
            TEST_FORBIDDEN_ACTIVE_TESTS
        }

        fn passive_monitors(&self) -> &[obd2_dash::profiles::PassiveMonitorDefinition] {
            &[]
        }

        fn decode_signal(
            &self,
            _signal: &SignalDefinition,
            _payload: &[u8],
        ) -> Result<obd2_dash::profiles::DecodedSignal, ProfileDecodeError> {
            Err(ProfileDecodeError::Other("fixture".to_string()))
        }

        fn decode_dtc_response(
            &self,
            _service: &obd2_dash::profiles::DtcServiceDefinition,
            _payload: &[u8],
        ) -> Result<Vec<DecodedDtc>, ProfileDecodeError> {
            Ok(Vec::new())
        }
    }

    fn embedded_lly_context() -> VehicleContext {
        let spec = obd2_core::specs::embedded::load_embedded_specs()
            .into_iter()
            .find(|spec| spec.identity.engine.code == "LLY")
            .expect("embedded LLY spec");
        VehicleContext {
            generation: 42,
            protocol: Protocol::J1850Vpw,
            vin: Some("1GTHK29294E391526".into()),
            vin_confidence: IdentityConfidence::Confirmed,
            spec: Some(spec),
            discovered_modules: vec![ModuleId::new("ecm")],
            active_bus: Some("j1850vpw".into()),
        }
    }

    fn test_standard_values() -> StandardSignalValues {
        StandardSignalValues {
            rpm: 725.0,
            rpm_state: "ok",
            speed_mph: 0.0,
            speed_state: "ok",
            voltage: 13.9,
            voltage_state: "ok",
            coolant_f: 170.0,
            coolant_state: "ok",
            intake_air_f: 95.0,
            intake_air_state: "ok",
            map_psi: 13.9,
            map_state: "ok",
            barometric_psi: Some(14.2),
            barometric_state: "ok",
            boost_psi: 0.0,
            boost_state: "ok",
            maf_g_s: 38.0,
            maf_state: "ok",
            fuel_rail_actual_psi: 5_500.0,
            fuel_rail_actual_state: "ok",
            fuel_rail_actual_evidence: standard_signal_evidence(
                "fuel_rail_actual",
                "Fuel Rail Pressure",
                "ecm",
                "0123",
                "SAE PID 01 23",
                "SAE standard",
                "psi",
                Some(5_500.0),
                "ok",
                None,
            ),
        }
    }

    fn test_reading(
        key: &'static str,
        label: &str,
        unit: &str,
        value: f64,
    ) -> ProfileSignalReading<f64> {
        ProfileSignalReading {
            value: Some(value),
            evidence: profile_signal_evidence(
                key,
                label,
                "ECM/PCM",
                "0x10",
                "fixture",
                "fixture",
                "verified",
                unit,
                Some(value as f32),
                "success",
                None,
                None,
            ),
        }
    }

    fn signal_display_string(signal: &SignalSnapshot) -> String {
        let Some(value) = signal.value else {
            return match signal.state.as_str() {
                "unsupported" => "unsupported".to_string(),
                "error" => "ERR".to_string(),
                _ => "--".to_string(),
            };
        };
        match signal.unit.trim() {
            "psi" => format!("{value:.1} psi"),
            "kPa" | "kPa abs" => format!("{:.1} psi", value / 6.894_757),
            "F" => format!("{value:.1} F"),
            "g/s" => format!("{value:.1} g/s"),
            "%" => format!("{value:.1}%"),
            "V" => format!("{value:.1} V"),
            "rpm" => format!("{value:.0} rpm"),
            "mph" => format!("{value:.1} mph"),
            "mm3" => {
                let rounded = format!("{value:.1}");
                if rounded == "-0.0" || rounded == "0.0" {
                    "0.0 mm3".to_string()
                } else if value > 0.0 {
                    format!("+{rounded} mm3")
                } else {
                    format!("{rounded} mm3")
                }
            }
            "" => format!("{value:.1}"),
            unit => format!("{value:.1} {unit}"),
        }
    }

    fn display_for<'a>(signals: &'a [SignalSnapshot], key: &str) -> &'a SignalSnapshot {
        signals
            .iter()
            .find(|signal| signal.key == key)
            .unwrap_or_else(|| panic!("missing signal {key}"))
    }

    fn decode_profile_signal(did: u16, payload: &[u8]) -> obd2_dash::profiles::DecodedSignal {
        let registry = ProfileRegistry::with_builtins();
        let profile = registry
            .get(ProfileId::new(TEST_LLY_PROFILE_ID))
            .expect("LLY profile registered");
        let signal = profile_signal_by_did(profile.signals(), did).expect("profile signal");
        profile
            .decode_signal(&signal, payload)
            .expect("profile signal decode")
    }

    #[test]
    fn decodes_lly_fuel_pressure_from_profile_stripped_payload() {
        let decoded = decode_profile_signal(TEST_LLY_DESIRED_FUEL_PRESSURE_DID, &[0x26, 0x00]);

        assert_eq!(decoded.selected_raw, vec![0x26]);
        assert!((decoded.value - 551.0).abs() < 0.1);
        assert_eq!(decoded.unit, "psi");
    }

    #[test]
    fn decodes_lly_fuel_pressure_from_profile_full_mode_22_payload() {
        let decoded = decode_profile_signal(
            TEST_LLY_DESIRED_FUEL_PRESSURE_DID,
            &[0x62, 0x16, 0x3D, 0x26, 0x00],
        );

        assert_eq!(decoded.selected_raw, vec![0x26]);
        assert!((decoded.value - 551.0).abs() < 0.1);
    }

    #[test]
    fn decodes_gm_mode22_u8_from_profile_stripped_payload() {
        let decoded = decode_profile_signal(TEST_LLY_BAROMETRIC_PRESSURE_DID, &[0x61]);

        assert_eq!(decoded.selected_raw, vec![97]);
        assert_eq!(decoded.value, 97.0);
    }

    #[test]
    fn decodes_gm_mode22_u8_from_profile_full_mode_22_payload() {
        let decoded =
            decode_profile_signal(TEST_LLY_BAROMETRIC_PRESSURE_DID, &[0x62, 0x12, 0x51, 0x61]);

        assert_eq!(decoded.selected_raw, vec![97]);
        assert_eq!(decoded.value, 97.0);
    }

    #[test]
    fn decodes_desired_map_candidate_from_mode_22_payload() {
        let decoded = decode_profile_signal(TEST_LLY_DESIRED_MAP_DID, &[0x62, 0x15, 0x42, 0x67]);

        assert_eq!(decoded.selected_raw, vec![103]);
        assert_eq!(decoded.value, 103.0);
    }

    #[test]
    fn decodes_lly_fuel_pressure_with_echoed_selector_byte() {
        let decoded = decode_profile_signal(
            TEST_LLY_DESIRED_FUEL_PRESSURE_DID,
            &[0x62, 0x16, 0x3D, 0x01, 0x26, 0x00],
        );

        assert_eq!(decoded.selected_raw, vec![0x26]);
        assert!((decoded.value - 551.0).abs() < 0.1);
    }

    #[test]
    fn rejects_short_lly_fuel_pressure_payload() {
        let registry = ProfileRegistry::with_builtins();
        let profile = registry
            .get(ProfileId::new(TEST_LLY_PROFILE_ID))
            .expect("LLY profile registered");
        let signal = profile_signal_by_did(profile.signals(), TEST_LLY_DESIRED_FUEL_PRESSURE_DID)
            .expect("profile signal");
        let err = profile.decode_signal(&signal, &[0x01]).unwrap_err();

        assert!(matches!(
            err,
            ProfileDecodeError::PayloadTooShort { .. }
                | ProfileDecodeError::Decode(_)
                | ProfileDecodeError::MismatchedResponse
        ));
    }

    #[test]
    fn diagnostic_snapshot_serializes_generic_capability_shape_only() {
        let snapshot = empty_snapshot("test");
        let value = serde_json::to_value(&snapshot).expect("snapshot serializes");
        let object = value.as_object().expect("snapshot is an object");

        for field in [
            "source_confidence",
            "signals",
            "capability_sections",
            "active_tests_v2",
            "dtcs",
            "modules",
            "statuses",
            "alerts",
        ] {
            assert!(object.contains_key(field), "missing generic field {field}");
        }

        for legacy in [
            "cylinders",
            "vgt",
            "fuel_rail",
            "temperatures",
            "map_psi",
            "desired_map_psi",
            "barometric_psi",
            "boost_psi",
            "maf_g_s",
            "active_tests",
        ] {
            assert!(
                !object.contains_key(legacy),
                "legacy field {legacy} must not be serialized"
            );
        }
    }

    #[test]
    fn verified_active_test_targeting_write_forbidden_module_is_disabled() {
        let runtime = ActiveTestRuntimeValues {
            rpm: 700.0,
            speed_kph: 0.0,
            coolant_f: 170.0,
            voltage: 13.8,
            last_result: None,
        };
        let profile = WriteForbiddenProfile;

        let snapshot = active_test_snapshot_v2(TEST_FORBIDDEN_ACTIVE_TESTS[0], &profile, &runtime);

        assert_eq!(snapshot.command_profile, "Verified");
        assert!(!snapshot.actionable);
        assert!(snapshot
            .lock_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("write-forbidden")));
    }

    #[test]
    fn selected_lly_snapshots_follow_profile_display_contract() {
        let registry = ProfileRegistry::with_builtins();
        let state = registry.select(&embedded_lly_context());
        let selected = state.selected.expect("LLY profile selected");
        let profile = registry
            .get(selected.profile_id())
            .expect("selected profile registered");
        let cylinders: [CylinderBalance; 0] = [];
        let cylinder_states: [&'static str; 0] = [];
        let profile_readings = HashMap::new();
        let profile_values = LivedataProfileValues {
            profile_readings: &profile_readings,
            cylinders: &cylinders,
            cylinder_states: &cylinder_states,
            desired_map_kpa: Some(103.0),
        };

        let signals =
            build_signal_snapshots(Some(&selected), &test_standard_values(), &profile_values);
        let actual_keys = signals
            .iter()
            .map(|signal| signal.key.as_str())
            .collect::<Vec<_>>();
        let expected_keys = profile
            .signal_display()
            .iter()
            .map(|definition| definition.key)
            .collect::<Vec<_>>();
        assert_eq!(actual_keys, expected_keys);

        let actual_map = signals
            .iter()
            .find(|signal| signal.key == "standard:0B")
            .expect("actual MAP signal");
        assert_eq!(actual_map.unit, "psi");
        assert!(matches!(
            &actual_map.composition,
            SignalComposition::Pair {
                group_key,
                role,
                ..
            } if group_key == "lly.map_pressure" && role == "actual"
        ));

        let desired_map = signals
            .iter()
            .find(|signal| signal.key == "lly.desired_map")
            .expect("desired MAP display signal");
        assert!(desired_map
            .value
            .is_some_and(|value| (value - 14.9).abs() < 0.05));
        assert_eq!(desired_map.confidence, "Candidate");
        assert!(matches!(
            &desired_map.composition,
            SignalComposition::Pair {
                group_key,
                role,
                ..
            } if group_key == "lly.map_pressure" && role == "desired"
        ));

        let injector_rows = signals
            .iter()
            .filter(|signal| {
                matches!(
                    &signal.composition,
                    SignalComposition::TableRow { table_key, .. }
                        if table_key == "lly.injector_balance"
                )
            })
            .count();
        assert_eq!(injector_rows, 8);
        assert!(!signals
            .iter()
            .any(|signal| signal.key == "derived.desired_map"));
    }

    #[test]
    fn lly_generic_signal_graph_matches_legacy_display_strings() {
        let registry = ProfileRegistry::with_builtins();
        let state = registry.select(&embedded_lly_context());
        let selected = state.selected.expect("LLY profile selected");
        let mut profile_readings = HashMap::new();
        profile_readings.insert(
            "lly.1543",
            test_reading("lly.1543", "VGT actual", "%", 88.2),
        );
        profile_readings.insert(
            "lly.1540",
            test_reading("lly.1540", "VGT desired", "%", 88.6),
        );
        profile_readings.insert(
            "lly.163D",
            test_reading("lly.163D", "Desired fuel rail", "psi", 5_510.0),
        );
        profile_readings.insert(
            "lly.1251",
            test_reading("lly.1251", "Barometer", "kPa abs", 98.0),
        );
        let cylinders = [
            CylinderBalance {
                cylinder: 1,
                mm3: 0.3,
            },
            CylinderBalance {
                cylinder: 2,
                mm3: -0.3,
            },
            CylinderBalance {
                cylinder: 3,
                mm3: -1.3,
            },
            CylinderBalance {
                cylinder: 4,
                mm3: -0.4,
            },
            CylinderBalance {
                cylinder: 5,
                mm3: -0.3,
            },
            CylinderBalance {
                cylinder: 6,
                mm3: 0.2,
            },
            CylinderBalance {
                cylinder: 7,
                mm3: 1.0,
            },
            CylinderBalance {
                cylinder: 8,
                mm3: 0.5,
            },
        ];
        let cylinder_states = ["ok"; 8];
        let profile_values = LivedataProfileValues {
            profile_readings: &profile_readings,
            cylinders: &cylinders,
            cylinder_states: &cylinder_states,
            desired_map_kpa: Some(103.0),
        };
        let signals =
            build_signal_snapshots(Some(&selected), &test_standard_values(), &profile_values);

        for (key, expected) in [
            ("lly.1543", "88.2%"),
            ("lly.1540", "88.6%"),
            ("lly.vgt_vane.error", "-0.4%"),
            ("lly.fuel_rail.actual", "5500.0 psi"),
            ("lly.163D", "5510.0 psi"),
            ("lly.fuel_rail.delta", "-10.0 psi"),
            ("standard:0B", "13.9 psi"),
            ("lly.desired_map", "14.9 psi"),
            ("lly.barometric_pressure", "14.2 psi"),
            ("lly.boost_pressure", "0.0 psi"),
            ("standard:10", "38.0 g/s"),
            ("standard:05", "170.0 F"),
            ("standard:0F", "95.0 F"),
            ("lly.162F", "+0.3 mm3"),
            ("lly.1630", "-0.3 mm3"),
            ("lly.1631", "-1.3 mm3"),
            ("lly.1632", "-0.4 mm3"),
            ("lly.1633", "-0.3 mm3"),
            ("lly.1634", "+0.2 mm3"),
            ("lly.1635", "+1.0 mm3"),
            ("lly.1636", "+0.5 mm3"),
        ] {
            assert_eq!(
                signal_display_string(display_for(&signals, key)),
                expected,
                "display parity for {key}"
            );
        }
    }
}
