pub mod config;
pub mod edit_mode;
pub mod renderers;

use serde::{Deserialize, Serialize};

/// All widget types available for the dashboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WidgetKind {
    // Composite panels (existing)
    GaugesAndEngine,
    TemperaturesPanel,
    FuelSystemPanel,
    SystemInfoPanel,
    DtcPanel,
    FuelEconomyPanel,

    // Individual engine/airflow widgets
    EngineRpmGauge,
    VehicleSpeedGauge,
    EngineLoadGauge,
    ThrottleGauge,
    IntakeMapDisplay,
    MafDisplay,
    FuelPressureDisplay,
    BoostPressureDisplay,
    OilPressureDisplay,

    // Individual fuel/emissions widgets
    FuelTankLevel,
    EngineFuelRate,
    FuelTrimBank1,
    FuelTrimBank2,

    // Individual temperature widgets
    CoolantTemp,
    OilTemp,
    TransmissionTemp,
    IntakeAirTemp,
    AmbientAirTemp,
    CatalystTemps,

    // Recording
    RecordingStatus,

    // Driving behavior
    DrivingBehavior,

    // Alerts
    AlertsPanel,
}

/// Widget size within a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WidgetSize {
    /// Takes roughly half the row width.
    Half,
    /// Takes the full row width.
    Full,
}

/// Categories for organizing widgets in the edit mode picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetCategory {
    EngineAirflow,
    FuelEmissions,
    Temperature,
    TransmissionChassis,
    Diagnostics,
    SystemVehicle,
    Recording,
    Driving,
}

impl WidgetCategory {
    pub fn all() -> &'static [WidgetCategory] {
        &[
            WidgetCategory::EngineAirflow,
            WidgetCategory::FuelEmissions,
            WidgetCategory::Temperature,
            WidgetCategory::TransmissionChassis,
            WidgetCategory::Diagnostics,
            WidgetCategory::SystemVehicle,
            WidgetCategory::Recording,
            WidgetCategory::Driving,
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            WidgetCategory::EngineAirflow => "Engine & Airflow",
            WidgetCategory::FuelEmissions => "Fuel & Emissions",
            WidgetCategory::Temperature => "Temperature",
            WidgetCategory::TransmissionChassis => "Transmission & Chassis",
            WidgetCategory::Diagnostics => "Error Codes & Diagnostics",
            WidgetCategory::SystemVehicle => "System & Vehicle Info",
            WidgetCategory::Recording => "Recording & Playback",
            WidgetCategory::Driving => "Driving Behavior",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            WidgetCategory::EngineAirflow => "RPM, speed, load, throttle, MAP, MAF, pressures",
            WidgetCategory::FuelEmissions => "Fuel tank, fuel rate, fuel trims",
            WidgetCategory::Temperature => "Coolant, oil, trans, intake, ambient, catalyst",
            WidgetCategory::TransmissionChassis => "Vehicle speed gauge",
            WidgetCategory::Diagnostics => "Diagnostic trouble codes panel",
            WidgetCategory::SystemVehicle => "Battery, VIN, vehicle details",
            WidgetCategory::Recording => "Recording status indicator",
            WidgetCategory::Driving => {
                "Smoothness score, acceleration, braking and jackrabbit detection"
            }
        }
    }
}

/// Metadata about a widget type.
pub struct WidgetMeta {
    pub kind: WidgetKind,
    pub title: &'static str,
    pub category: WidgetCategory,
    pub default_size: WidgetSize,
    pub description: &'static str,
}

/// Get metadata for all registered widget types.
pub fn widget_registry() -> Vec<WidgetMeta> {
    vec![
        // Composite panels
        WidgetMeta {
            kind: WidgetKind::GaugesAndEngine,
            title: "Gauges + Engine",
            category: WidgetCategory::EngineAirflow,
            default_size: WidgetSize::Half,
            description: "RPM/Speed gauges with sparklines and engine data panel",
        },
        WidgetMeta {
            kind: WidgetKind::TemperaturesPanel,
            title: "Temperatures",
            category: WidgetCategory::Temperature,
            default_size: WidgetSize::Half,
            description: "All temperature readings in a list",
        },
        WidgetMeta {
            kind: WidgetKind::FuelSystemPanel,
            title: "Fuel System",
            category: WidgetCategory::FuelEmissions,
            default_size: WidgetSize::Half,
            description: "Fuel tank level, fuel rate, and fuel trims",
        },
        WidgetMeta {
            kind: WidgetKind::SystemInfoPanel,
            title: "System / Vehicle",
            category: WidgetCategory::SystemVehicle,
            default_size: WidgetSize::Half,
            description: "Battery voltage, vehicle info, VIN",
        },
        WidgetMeta {
            kind: WidgetKind::DtcPanel,
            title: "DTCs",
            category: WidgetCategory::Diagnostics,
            default_size: WidgetSize::Half,
            description: "Stored diagnostic trouble codes",
        },
        WidgetMeta {
            kind: WidgetKind::FuelEconomyPanel,
            title: "Fuel Economy",
            category: WidgetCategory::FuelEmissions,
            default_size: WidgetSize::Full,
            description: "Dual MPG display: ECU gold standard + speed-density calculated",
        },
        // Individual engine/airflow
        WidgetMeta {
            kind: WidgetKind::EngineRpmGauge,
            title: "Engine RPM",
            category: WidgetCategory::EngineAirflow,
            default_size: WidgetSize::Half,
            description: "RPM gauge with sparkline history",
        },
        WidgetMeta {
            kind: WidgetKind::VehicleSpeedGauge,
            title: "Vehicle Speed",
            category: WidgetCategory::TransmissionChassis,
            default_size: WidgetSize::Half,
            description: "Speed gauge with sparkline history",
        },
        WidgetMeta {
            kind: WidgetKind::EngineLoadGauge,
            title: "Engine Load",
            category: WidgetCategory::EngineAirflow,
            default_size: WidgetSize::Half,
            description: "Engine load percentage gauge",
        },
        WidgetMeta {
            kind: WidgetKind::ThrottleGauge,
            title: "Throttle Position",
            category: WidgetCategory::EngineAirflow,
            default_size: WidgetSize::Half,
            description: "Throttle position percentage gauge",
        },
        WidgetMeta {
            kind: WidgetKind::IntakeMapDisplay,
            title: "Intake MAP",
            category: WidgetCategory::EngineAirflow,
            default_size: WidgetSize::Half,
            description: "Intake manifold absolute pressure",
        },
        WidgetMeta {
            kind: WidgetKind::MafDisplay,
            title: "MAF",
            category: WidgetCategory::EngineAirflow,
            default_size: WidgetSize::Half,
            description: "Mass air flow sensor reading",
        },
        WidgetMeta {
            kind: WidgetKind::FuelPressureDisplay,
            title: "Fuel Pressure",
            category: WidgetCategory::EngineAirflow,
            default_size: WidgetSize::Half,
            description: "Fuel rail pressure",
        },
        WidgetMeta {
            kind: WidgetKind::BoostPressureDisplay,
            title: "Boost Pressure",
            category: WidgetCategory::EngineAirflow,
            default_size: WidgetSize::Half,
            description: "Derived boost pressure (MAP - Barometric)",
        },
        WidgetMeta {
            kind: WidgetKind::OilPressureDisplay,
            title: "Oil Pressure",
            category: WidgetCategory::EngineAirflow,
            default_size: WidgetSize::Half,
            description: "Engine oil pressure",
        },
        // Individual fuel/emissions
        WidgetMeta {
            kind: WidgetKind::FuelTankLevel,
            title: "Fuel Tank Level",
            category: WidgetCategory::FuelEmissions,
            default_size: WidgetSize::Half,
            description: "Fuel tank fill percentage",
        },
        WidgetMeta {
            kind: WidgetKind::EngineFuelRate,
            title: "Engine Fuel Rate",
            category: WidgetCategory::FuelEmissions,
            default_size: WidgetSize::Half,
            description: "Fuel consumption rate (L/h)",
        },
        WidgetMeta {
            kind: WidgetKind::FuelTrimBank1,
            title: "Fuel Trim Bank 1",
            category: WidgetCategory::FuelEmissions,
            default_size: WidgetSize::Half,
            description: "Short and long term fuel trim for bank 1",
        },
        WidgetMeta {
            kind: WidgetKind::FuelTrimBank2,
            title: "Fuel Trim Bank 2",
            category: WidgetCategory::FuelEmissions,
            default_size: WidgetSize::Half,
            description: "Short and long term fuel trim for bank 2",
        },
        // Individual temperatures
        WidgetMeta {
            kind: WidgetKind::CoolantTemp,
            title: "Coolant Temp",
            category: WidgetCategory::Temperature,
            default_size: WidgetSize::Half,
            description: "Engine coolant temperature",
        },
        WidgetMeta {
            kind: WidgetKind::OilTemp,
            title: "Oil Temp",
            category: WidgetCategory::Temperature,
            default_size: WidgetSize::Half,
            description: "Engine oil temperature",
        },
        WidgetMeta {
            kind: WidgetKind::TransmissionTemp,
            title: "Trans Temp",
            category: WidgetCategory::Temperature,
            default_size: WidgetSize::Half,
            description: "Transmission fluid temperature",
        },
        WidgetMeta {
            kind: WidgetKind::IntakeAirTemp,
            title: "Intake Air Temp",
            category: WidgetCategory::Temperature,
            default_size: WidgetSize::Half,
            description: "Intake air temperature",
        },
        WidgetMeta {
            kind: WidgetKind::AmbientAirTemp,
            title: "Ambient Air Temp",
            category: WidgetCategory::Temperature,
            default_size: WidgetSize::Half,
            description: "Outside ambient air temperature",
        },
        WidgetMeta {
            kind: WidgetKind::CatalystTemps,
            title: "Catalyst Temps",
            category: WidgetCategory::Temperature,
            default_size: WidgetSize::Half,
            description: "All 4 catalyst temperature sensors",
        },
        // Recording
        WidgetMeta {
            kind: WidgetKind::RecordingStatus,
            title: "Recording Status",
            category: WidgetCategory::Recording,
            default_size: WidgetSize::Half,
            description: "Current recording status and storage info",
        },
        // Driving behavior
        WidgetMeta {
            kind: WidgetKind::DrivingBehavior,
            title: "Driving Behavior",
            category: WidgetCategory::Driving,
            default_size: WidgetSize::Half,
            description: "Smoothness score, acceleration, braking/jackrabbit events",
        },
        // Alerts
        WidgetMeta {
            kind: WidgetKind::AlertsPanel,
            title: "Alerts",
            category: WidgetCategory::Diagnostics,
            default_size: WidgetSize::Half,
            description: "Active threshold alerts and errors",
        },
    ]
}

/// Get widgets belonging to a specific category.
#[allow(dead_code)]
pub fn widgets_for_category(category: WidgetCategory) -> Vec<&'static WidgetMeta> {
    // Leak the registry into a static so we can return references.
    // This is fine — it's called once per edit session at most.
    let registry: &'static Vec<WidgetMeta> = Box::leak(Box::new(widget_registry()));
    registry.iter().filter(|m| m.category == category).collect()
}
