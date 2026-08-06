use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapabilityKind {
    Pid,
    ProfileSignal,
    Service,
}

impl CapabilityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pid => "pid",
            Self::ProfileSignal => "profile_signal",
            Self::Service => "service",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pid" => Some(Self::Pid),
            "profile_signal" => Some(Self::ProfileSignal),
            "service" => Some(Self::Service),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CapabilityOutcome {
    Supported,
    Unsupported,
    Unverified,
}

impl CapabilityOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Unsupported => "unsupported",
            Self::Unverified => "unverified",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "supported" => Some(Self::Supported),
            "unsupported" => Some(Self::Unsupported),
            "unverified" => Some(Self::Unverified),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityContext {
    pub protocol: String,
    pub profile_id: String,
    pub probe_schema_version: i64,
    pub probe_fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRecord {
    pub kind: CapabilityKind,
    pub request_id: String,
    pub module: String,
    pub outcome: CapabilityOutcome,
    pub observation_seq: i64,
    pub rtt_ms: Option<i64>,
    pub attempted_at: String,
    pub error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehicleCapabilitySet {
    pub vin: String,
    pub set_id: String,
    pub context: CapabilityContext,
    pub completed_at: String,
    pub records: Vec<CapabilityRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilitySetReplacement {
    pub vin: String,
    pub context: CapabilityContext,
    pub completed_at: String,
    pub records: Vec<CapabilityRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityLoad {
    Hit(VehicleCapabilitySet),
    Miss,
    ContextMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeUpdate {
    Applied,
    StaleSet,
}

/// Alert severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertLevel {
    Warning,
    Critical,
}

impl fmt::Display for AlertLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AlertLevel::Warning => write!(f, "WARN"),
            AlertLevel::Critical => write!(f, "CRIT"),
        }
    }
}

/// An active alert for a PID reading outside its threshold.
#[derive(Debug, Clone)]
pub struct Alert {
    pub pid_code: u8,
    pub pid_name: String,
    pub level: AlertLevel,
    pub value: f64,
    pub limit: f64,
    pub direction: AlertDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertDirection {
    Low,
    High,
}

impl fmt::Display for Alert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dir = match self.direction {
            AlertDirection::Low => "LOW",
            AlertDirection::High => "HIGH",
        };
        write!(
            f,
            "[{}] {} {}: {:.1} (limit: {:.1})",
            self.level, self.pid_name, dir, self.value, self.limit
        )
    }
}

/// Vehicle identification from the database.
#[derive(Debug, Clone)]
pub struct VehicleInfo {
    pub vin: String,
    pub year: Option<i32>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub trim: Option<String>,
    pub engine_family_id: Option<i64>,
    pub engine_family_code: Option<String>,
    pub transmission_type: Option<String>,
    pub drive_type: Option<String>,
    pub fuel_type: Option<String>,
    pub displacement_l: Option<f64>,
    pub cylinders: Option<i32>,
}

impl VehicleInfo {
    /// Short display name like "2006 MINI Cooper S".
    pub fn display_name(&self) -> String {
        let year = self.year.map(|y| y.to_string()).unwrap_or_default();
        let make = self.make.as_deref().unwrap_or("");
        let model = self.model.as_deref().unwrap_or("");
        let trim = self.trim.as_deref().unwrap_or("");
        format!("{} {} {} {}", year, make, model, trim)
            .trim()
            .to_string()
    }
}

/// Engine family grouping with shared operating characteristics.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EngineFamily {
    pub id: Option<i64>,
    pub manufacturer: String,
    pub family_code: String,
    pub displacement_l: f64,
    pub cylinders: i32,
    pub layout: String,
    pub aspiration: String,
    pub fuel_type: String,
    pub compression_ratio: Option<f64>,
    pub redline_rpm: Option<i32>,
    pub idle_rpm_cold: Option<i32>,
    pub idle_rpm_warm: Option<i32>,
    pub max_power_kw: Option<f64>,
    pub max_torque_nm: Option<f64>,
}

/// Universal safe range for a PID.
#[derive(Debug, Clone)]
pub struct DefaultThreshold {
    pub pid_code: u8,
    pub min_value: f64,
    pub max_value: f64,
    pub low_warning: Option<f64>,
    pub high_warning: Option<f64>,
    pub low_critical: Option<f64>,
    pub high_critical: Option<f64>,
    pub unit: String,
}

/// Scoped override for a PID threshold.
#[derive(Debug, Clone)]
pub struct PidThreshold {
    pub scope_type: String,
    pub scope_id: String,
    pub pid_code: u8,
    pub min_value: Option<f64>,
    pub max_value: Option<f64>,
    pub low_warning: Option<f64>,
    pub high_warning: Option<f64>,
    pub low_critical: Option<f64>,
    pub high_critical: Option<f64>,
    pub notes: Option<String>,
}

/// Fully resolved threshold for a PID (after priority chain).
#[derive(Debug, Clone)]
pub struct ResolvedThreshold {
    pub pid_code: u8,
    pub min_value: f64,
    pub max_value: f64,
    pub low_warning: Option<f64>,
    pub high_warning: Option<f64>,
    pub low_critical: Option<f64>,
    pub high_critical: Option<f64>,
    pub unit: String,
}

impl ResolvedThreshold {
    /// Check a value against this threshold and return an alert if violated.
    pub fn check(&self, value: f64, pid_name: &str) -> Option<Alert> {
        // Critical checks first
        if let Some(lc) = self.low_critical {
            if value < lc {
                return Some(Alert {
                    pid_code: self.pid_code,
                    pid_name: pid_name.to_string(),
                    level: AlertLevel::Critical,
                    value,
                    limit: lc,
                    direction: AlertDirection::Low,
                });
            }
        }
        if let Some(hc) = self.high_critical {
            if value > hc {
                return Some(Alert {
                    pid_code: self.pid_code,
                    pid_name: pid_name.to_string(),
                    level: AlertLevel::Critical,
                    value,
                    limit: hc,
                    direction: AlertDirection::High,
                });
            }
        }
        // Warning checks
        if let Some(lw) = self.low_warning {
            if value < lw {
                return Some(Alert {
                    pid_code: self.pid_code,
                    pid_name: pid_name.to_string(),
                    level: AlertLevel::Warning,
                    value,
                    limit: lw,
                    direction: AlertDirection::Low,
                });
            }
        }
        if let Some(hw) = self.high_warning {
            if value > hw {
                return Some(Alert {
                    pid_code: self.pid_code,
                    pid_name: pid_name.to_string(),
                    level: AlertLevel::Warning,
                    value,
                    limit: hw,
                    direction: AlertDirection::High,
                });
            }
        }
        None
    }
}
