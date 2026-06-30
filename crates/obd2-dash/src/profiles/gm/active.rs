use chrono::Utc;

use crate::gm_active::{
    GmActiveTestBlockReason, GmActiveTestCommand, GmActiveTestDefinition, GmActiveTestId,
    GmActiveTestResult,
};
use crate::gm_evidence::{
    GmDecodedEvidence, GmEvidenceErrorKind, GmEvidenceRecord, GmEvidenceWriter,
};
use crate::profiles::{
    ActiveCommandProfile, ActivePrecondition, ActiveTestDefinition, EvidencePolicy, SafetyClass,
};

pub const VGT_VANE_CONTROL_KEY: &str = "gm.lly.vgt_vane_control";
const MAX_ACTIVE_TEST_HOLD: std::time::Duration = std::time::Duration::from_millis(5_000);

const VGT_PRECONDITIONS: &[ActivePrecondition] = &[
    ActivePrecondition {
        label: "Verified command profile",
        detail: "No verified GM Class 2 actuator-control bytes are loaded.",
    },
    ActivePrecondition {
        label: "Stationary",
        detail: "Vehicle speed must be zero before any future enabled command can run.",
    },
    ActivePrecondition {
        label: "Idle speed",
        detail: "Engine must be idling; commands are refused while driving or under load.",
    },
    ActivePrecondition {
        label: "Warm coolant",
        detail: "Coolant must be warm enough to avoid cold-start actuator behavior.",
    },
    ActivePrecondition {
        label: "Battery voltage",
        detail: "Voltage must be stable before any actuator command is allowed.",
    },
    ActivePrecondition {
        label: "Park/Neutral and A/C off",
        detail: "Current data cannot observe this; an enabled command must require operator confirmation.",
    },
];

const VGT_ACTIVE_TESTS: &[ActiveTestDefinition] = &[ActiveTestDefinition {
    key: VGT_VANE_CONTROL_KEY,
    label: "VGT vane control",
    safety_class: SafetyClass::Locked,
    command_profile: ActiveCommandProfile::Locked,
    preconditions: VGT_PRECONDITIONS,
    timeout: MAX_ACTIVE_TEST_HOLD,
    cancel_command: None,
    evidence_policy: EvidencePolicy::Always,
}];

pub fn active_tests() -> &'static [ActiveTestDefinition] {
    VGT_ACTIVE_TESTS
}

pub fn vgt_vane_control_definition() -> GmActiveTestDefinition {
    GmActiveTestDefinition {
        id: GmActiveTestId::VgtVaneControl,
        label: "VGT vane control".to_string(),
        locked: true,
        lock_reason:
            "Verified Tech2/EFILive/Snap-on Class 2 command bytes are required before execution."
                .to_string(),
        command_profile: "missing verified GM Class 2 actuator-control profile".to_string(),
        supported_modes: vec![
            "TC Learn ON/OFF placeholder".to_string(),
            "Manual vane percent placeholder".to_string(),
        ],
        safety_notes: vec![
            "Stationary-only; never while driving.".to_string(),
            "Idle-only with warm coolant.".to_string(),
            "Future enabled commands must timeout and release control automatically.".to_string(),
            "Every attempt must be captured in the GM evidence stream.".to_string(),
        ],
    }
}

pub fn blocked_active_test_result(command: &GmActiveTestCommand) -> GmActiveTestResult {
    let reason = command
        .validate_shape()
        .err()
        .unwrap_or(GmActiveTestBlockReason::UnverifiedCommandProfile);

    GmActiveTestResult {
        test_id: command.test_id(),
        label: command.label().to_string(),
        accepted: false,
        status: reason.as_str().to_string(),
        detail: reason.detail().to_string(),
        command: command.summary(),
        evidence_path: None,
        timestamp: Utc::now(),
    }
}

pub fn active_test_evidence_record(
    command: &GmActiveTestCommand,
    result: &GmActiveTestResult,
) -> GmEvidenceRecord {
    let error_kind = if result.status == "unverified_command_profile" {
        GmEvidenceErrorKind::UnverifiedCommand
    } else {
        GmEvidenceErrorKind::InvalidCommand
    };

    GmEvidenceRecord::routed_request_outcome(
        "ECM/PCM",
        0x10,
        [0x6C, 0x10, 0xF1],
        0x00,
        Vec::new(),
        "gm-active-test-vgt-vane-control",
    )
    .with_decoded(
        GmDecodedEvidence::ActiveTest {
            test_id: command.test_id().as_str().to_string(),
            command: result.command.clone(),
            accepted: result.accepted,
            status: result.status.clone(),
        },
        Some("blocked".to_string()),
    )
    .with_error(error_kind, result.detail.clone())
}

pub fn write_active_test_evidence(
    command: &GmActiveTestCommand,
    result: &GmActiveTestResult,
) -> std::io::Result<std::path::PathBuf> {
    let mut writer = GmEvidenceWriter::create_raw_capture("gm-active-test")?;
    let record = active_test_evidence_record(command, result);
    writer.append(&record)?;
    writer.flush()?;
    Ok(writer.path().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vgt_active_test_is_locked_and_evidence_backed() {
        let test = &active_tests()[0];
        assert_eq!(test.key, VGT_VANE_CONTROL_KEY);
        assert_eq!(test.safety_class, SafetyClass::Locked);
        assert_eq!(test.command_profile, ActiveCommandProfile::Locked);
        assert_eq!(test.evidence_policy, EvidencePolicy::Always);
        assert_eq!(test.timeout, MAX_ACTIVE_TEST_HOLD);
    }

    #[test]
    fn blocked_result_preserves_invalid_shape_reason() {
        let command = GmActiveTestCommand::VgtManualPercent {
            percent: -1.0,
            hold_ms: 1_000,
        };
        let result = blocked_active_test_result(&command);

        assert_eq!(result.status, "invalid_value");
        assert!(!result.accepted);
    }
}
