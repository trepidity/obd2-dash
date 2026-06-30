use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GmActiveTestId {
    VgtVaneControl,
}

impl GmActiveTestId {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::VgtVaneControl => "vgt_vane_control",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GmActiveTestCommand {
    VgtTcLearn { enabled: bool },
    VgtManualPercent { percent: f32, hold_ms: u16 },
}

impl GmActiveTestCommand {
    pub fn test_id(&self) -> GmActiveTestId {
        match self {
            Self::VgtTcLearn { .. } | Self::VgtManualPercent { .. } => {
                GmActiveTestId::VgtVaneControl
            }
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::VgtTcLearn { enabled: true } => "TC Learn ON",
            Self::VgtTcLearn { enabled: false } => "TC Learn OFF",
            Self::VgtManualPercent { .. } => "Manual VGT vane percent",
        }
    }

    pub fn summary(&self) -> String {
        match self {
            Self::VgtTcLearn { enabled } => {
                format!("tc learn {}", if *enabled { "on" } else { "off" })
            }
            Self::VgtManualPercent { percent, hold_ms } => {
                format!("manual {percent:.1}% for {hold_ms} ms")
            }
        }
    }

    pub fn validate_shape(&self) -> Result<(), GmActiveTestBlockReason> {
        match self {
            Self::VgtTcLearn { .. } => Ok(()),
            Self::VgtManualPercent { percent, hold_ms } => {
                if !percent.is_finite() || !(0.0..=100.0).contains(percent) {
                    return Err(GmActiveTestBlockReason::InvalidValue);
                }
                if !(250..=5_000).contains(hold_ms) {
                    return Err(GmActiveTestBlockReason::InvalidDuration);
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GmActiveTestBlockReason {
    InvalidValue,
    InvalidDuration,
    UnverifiedCommandProfile,
}

impl GmActiveTestBlockReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidValue => "invalid_value",
            Self::InvalidDuration => "invalid_duration",
            Self::UnverifiedCommandProfile => "unverified_command_profile",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::InvalidValue => "manual VGT percent must be a finite value from 0.0 to 100.0",
            Self::InvalidDuration => "manual VGT hold duration must be 250-5000 ms",
            Self::UnverifiedCommandProfile => {
                "no verified GM Class 2 actuator command bytes are available for this test"
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmActiveTestPrecondition {
    pub label: String,
    pub satisfied: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmActiveTestDefinition {
    pub id: GmActiveTestId,
    pub label: String,
    pub locked: bool,
    pub lock_reason: String,
    pub command_profile: String,
    pub supported_modes: Vec<String>,
    pub safety_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GmActiveTestResult {
    pub test_id: GmActiveTestId,
    pub label: String,
    pub accepted: bool,
    pub status: String,
    pub detail: String,
    pub command: String,
    pub evidence_path: Option<String>,
    pub timestamp: DateTime<Utc>,
}

pub fn vgt_vane_control_definition() -> GmActiveTestDefinition {
    crate::profiles::gm::active::vgt_vane_control_definition()
}

pub fn blocked_active_test_result(command: &GmActiveTestCommand) -> GmActiveTestResult {
    crate::profiles::gm::active::blocked_active_test_result(command)
}

pub fn active_test_evidence_record(
    command: &GmActiveTestCommand,
    result: &GmActiveTestResult,
) -> crate::gm_evidence::GmEvidenceRecord {
    crate::profiles::gm::active::active_test_evidence_record(command, result)
}

pub fn write_active_test_evidence(
    command: &GmActiveTestCommand,
    result: &GmActiveTestResult,
) -> std::io::Result<std::path::PathBuf> {
    crate::profiles::gm::active::write_active_test_evidence(command, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_percent_shape_rejects_out_of_range_value() {
        let command = GmActiveTestCommand::VgtManualPercent {
            percent: 101.0,
            hold_ms: 1_000,
        };

        assert_eq!(
            command.validate_shape(),
            Err(GmActiveTestBlockReason::InvalidValue)
        );
    }

    #[test]
    fn blocked_result_defaults_to_unverified_profile_for_valid_shape() {
        let command = GmActiveTestCommand::VgtManualPercent {
            percent: 35.0,
            hold_ms: 1_000,
        };
        let result = blocked_active_test_result(&command);

        assert!(!result.accepted);
        assert_eq!(result.status, "unverified_command_profile");
    }

    #[test]
    fn evidence_record_marks_unverified_active_test() {
        let command = GmActiveTestCommand::VgtTcLearn { enabled: true };
        let result = blocked_active_test_result(&command);
        let record = active_test_evidence_record(&command, &result);

        assert_eq!(record.module_label, "ECM/PCM");
        assert_eq!(record.node, 0x10);
        assert_eq!(record.request_service, 0x00);
        assert!(matches!(
            record.error.as_ref().map(|error| &error.kind),
            Some(crate::gm_evidence::GmEvidenceErrorKind::UnverifiedCommand)
        ));
        assert!(matches!(
            record.decoded,
            Some(crate::gm_evidence::GmDecodedEvidence::ActiveTest {
                accepted: false,
                ..
            })
        ));
    }
}
