use async_trait::async_trait;
use serde::Serialize;

use obd2_db::models::VehicleInfo;

/// Sensor health status relative to thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SensorStatus {
    Ok,
    Warning,
    Critical,
    NoData,
}

impl SensorStatus {
    pub fn label(self) -> &'static str {
        match self {
            SensorStatus::Ok => "OK",
            SensorStatus::Warning => "WARN",
            SensorStatus::Critical => "CRIT",
            SensorStatus::NoData => "N/A",
        }
    }
}

/// A snapshot of a single sensor reading with threshold status.
#[derive(Debug, Clone, Serialize)]
pub struct SensorSnapshot {
    pub pid_code: u8,
    pub name: &'static str,
    pub value: Option<f64>,
    pub unit: &'static str,
    pub status: SensorStatus,
}

/// Serializable subset of VehicleInfo for diagnostic context.
#[derive(Debug, Clone, Serialize)]
pub struct VehicleInfoSummary {
    pub year: Option<i32>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub engine_code: Option<String>,
    pub displacement_l: Option<f64>,
    pub cylinders: Option<i32>,
}

impl From<&VehicleInfo> for VehicleInfoSummary {
    fn from(v: &VehicleInfo) -> Self {
        Self {
            year: v.year,
            make: v.make.clone(),
            model: v.model.clone(),
            engine_code: v.engine_family_code.clone(),
            displacement_l: v.displacement_l,
            cylinders: v.cylinders,
        }
    }
}

/// Full context passed to a diagnostic provider.
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticContext {
    pub dtc_code: String,
    pub dtc_description: String,
    pub dtc_category: String,
    pub related_sensors: Vec<SensorSnapshot>,
    pub other_dtcs: Vec<(String, String)>,
    pub vehicle_info: Option<VehicleInfoSummary>,
}

/// A titled section within a diagnostic result.
pub struct DiagnosticSection {
    pub heading: String,
    pub lines: Vec<String>,
}

/// The result returned by a diagnostic provider.
pub struct DiagnosticResult {
    pub sections: Vec<DiagnosticSection>,
}

impl DiagnosticResult {
    /// Flatten sections into popup-ready lines with `--- Heading ---` separators.
    pub fn to_popup_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for section in &self.sections {
            if section.lines.is_empty() {
                continue;
            }
            out.push(String::new());
            out.push(format!("--- {} ---", section.heading));
            for line in &section.lines {
                out.push(line.clone());
            }
        }
        out
    }
}

/// Trait for diagnostic analysis providers (local rules, OpenAI, Claude, etc.).
#[async_trait]
pub trait DiagnosticProvider: Send + Sync {
    async fn diagnose(&self, context: &DiagnosticContext) -> Option<DiagnosticResult>;
    fn name(&self) -> &str;
}

/// Built-in provider using static correlation tables and guidance text.
pub struct LocalDiagnosticProvider;

impl LocalDiagnosticProvider {
    /// Synchronous diagnostic analysis using local rules.
    pub fn diagnose_sync(&self, context: &DiagnosticContext) -> Option<DiagnosticResult> {
        let mut sections = Vec::new();

        // Section 1: Related Sensors
        if !context.related_sensors.is_empty() {
            let lines: Vec<String> = context
                .related_sensors
                .iter()
                .map(|s| {
                    let val_str = match s.value {
                        Some(v) => format!("{:>6.1} {:<4}", v, s.unit),
                        None => format!("{:>6} {:<4}", "--", s.unit),
                    };
                    format!("  {:<16} {}  {}", s.name, val_str, s.status.label())
                })
                .collect();
            sections.push(DiagnosticSection {
                heading: "Related Sensors".to_string(),
                lines,
            });
        }

        // Section 2: Other Active DTCs
        if !context.other_dtcs.is_empty() {
            let lines: Vec<String> = context
                .other_dtcs
                .iter()
                .map(|(code, desc)| format!("  {}  {}", code, desc))
                .collect();
            sections.push(DiagnosticSection {
                heading: "Other Active DTCs".to_string(),
                lines,
            });
        }

        // Section 3: Common Causes
        if let Some(causes) = super::correlation::common_causes(&context.dtc_code) {
            let lines: Vec<String> = causes.iter().map(|c| format!("  * {}", c)).collect();
            sections.push(DiagnosticSection {
                heading: "Common Causes".to_string(),
                lines,
            });
        }

        // Section 4: Suggested Actions
        if let Some(actions) = super::correlation::suggested_actions(&context.dtc_code) {
            let lines: Vec<String> = actions
                .iter()
                .enumerate()
                .map(|(i, a)| format!("  {}. {}", i + 1, a))
                .collect();
            sections.push(DiagnosticSection {
                heading: "Suggested Actions".to_string(),
                lines,
            });
        }

        if sections.is_empty() {
            None
        } else {
            Some(DiagnosticResult { sections })
        }
    }
}

#[async_trait]
impl DiagnosticProvider for LocalDiagnosticProvider {
    async fn diagnose(&self, context: &DiagnosticContext) -> Option<DiagnosticResult> {
        self.diagnose_sync(context)
    }

    fn name(&self) -> &str {
        "local"
    }
}
