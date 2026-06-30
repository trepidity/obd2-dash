use obd2_dash::gm_active::{
    active_test_evidence_record, blocked_active_test_result, vgt_vane_control_definition,
    GmActiveTestCommand,
};
use obd2_dash::profiles::gm::active::VGT_VANE_CONTROL_KEY;
use obd2_dash::profiles::gm::GM_LLY_CLASS2_PROFILE;
use obd2_dash::profiles::{ActiveCommandProfile, DiagnosticProfile, EvidencePolicy, SafetyClass};

#[test]
fn lly_profile_publishes_locked_vgt_active_test() {
    let tests = GM_LLY_CLASS2_PROFILE.active_tests();
    let vgt = tests
        .iter()
        .find(|test| test.key == VGT_VANE_CONTROL_KEY)
        .expect("LLY profile should declare VGT active test");

    assert_eq!(vgt.label, "VGT vane control");
    assert_eq!(vgt.safety_class, SafetyClass::Locked);
    assert_eq!(vgt.command_profile, ActiveCommandProfile::Locked);
    assert_eq!(vgt.evidence_policy, EvidencePolicy::Always);
    assert!(vgt.cancel_command.is_none());
    assert!(!vgt.preconditions.is_empty());
}

#[test]
fn public_gm_active_definition_matches_locked_profile_state() {
    let definition = vgt_vane_control_definition();

    assert!(definition.locked);
    assert_eq!(definition.id.as_str(), "vgt_vane_control");
    assert!(definition
        .command_profile
        .contains("missing verified GM Class 2"));
    assert!(definition
        .supported_modes
        .iter()
        .any(|mode| mode.contains("Manual vane percent")));
}

#[test]
fn active_test_attempts_are_evidence_records_even_when_refused() {
    let command = GmActiveTestCommand::VgtManualPercent {
        percent: 35.0,
        hold_ms: 1_000,
    };
    let result = blocked_active_test_result(&command);
    let record = active_test_evidence_record(&command, &result);

    assert!(!result.accepted);
    assert_eq!(result.status, "unverified_command_profile");
    assert_eq!(record.decoder, "gm-active-test-vgt-vane-control");
    assert_eq!(record.node, 0x10);
    assert!(record.error.is_some());
}
