use std::fs;
use std::path::{Path, PathBuf};

const NEEDLES: &[&str] = &[
    "find_lly_did(",
    ".raw_request(",
    ".routed_request(",
    "class2_routed_request(",
    "class2_dtc_all_request(",
    "class2_dtc_active_request(",
    ".adapter_mut(",
];

const SCAN_FILES: &[&str] = &[
    "src/session_runner.rs",
    "src/app.rs",
    "src/main.rs",
    "src/domain.rs",
    "src/vehicle_data.rs",
    "src/mock_profile.rs",
];

const SCAN_DIRS: &[&str] = &["src/tui", "src/widget"];

const ALLOWLIST: &[(&str, &str, usize)] = &[("src/session_runner.rs", ".raw_request(", 1)];

// Regression guard: live-session LLY gates were moved behind selected-profile
// state and the profile scheduler. These counts should stay at zero.
const SESSION_RUNNER_LLY_GATE_ALLOWLIST: &[(&str, usize)] = &[
    ("is_lly_spec_identity", 0),
    ("lly_profile_matches", 0),
    ("fn session_matches_lly_profile", 0),
    ("session_matches_lly_profile(", 0),
    ("fn should_force_standard_poll", 0),
    ("should_force_standard_poll(", 0),
];

const RENDERER_PROFILE_MIGRATED_FILES: &[&str] = &["src/widget/renderers.rs", "src/tui/ui.rs"];

const FORBIDDEN_RENDERER_LLY_DIDS: &[&str] = &[
    "0x1251", "0x1542", "0x1170", "0x1171", "0x1540", "0x1543", "0x162E", "0x162F", "0x1636",
    "0x163D", "0x163E",
];
const FORBIDDEN_RENDERER_PROFILE_SYMBOLS: &[&str] = &["GM_LLY_CLASS2_PROFILE"];

#[test]
fn live_dashboard_has_no_new_raw_routed_callers() {
    for path in live_dashboard_files() {
        let rel = relative_source_path(&path);
        let content =
            fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {}: {err}", rel));
        for needle in NEEDLES {
            let actual = count_occurrences(&content, needle);
            let max = max_allowed(&rel, needle);
            assert!(
                actual <= max,
                "{} contains {} occurrences of `{}`; max is {}. to add a manufacturer-routed call site you must raise this bound deliberately (review required); to MOVE/REMOVE one you must LOWER the matching bound in the same commit. This is the Wave 0 freeze; do not delete this test.",
                rel,
                actual,
                needle,
                max
            );
        }
    }
}

#[test]
fn gm_library_modules_are_the_only_definers() {
    let expected = [
        ("fn class2_routed_request(", "src/gm_class2.rs"),
        ("fn class2_header(", "src/gm_class2.rs"),
        ("fn find_lly_did(", "src/gm_enhanced.rs"),
    ];

    let files = source_files(&manifest_dir().join("src"));
    for (needle, expected_path) in expected {
        let mut actual = Vec::new();
        for path in &files {
            let content = fs::read_to_string(path).unwrap_or_else(|err| {
                panic!("failed to read {}: {err}", relative_source_path(path))
            });
            if content.contains(needle) {
                actual.push(relative_source_path(path));
            }
        }
        assert_eq!(
            actual,
            vec![expected_path.to_string()],
            "`{needle}` definers"
        );
    }
}

#[test]
fn session_runner_does_not_own_gm_class2_dtc_decode() {
    let path = manifest_dir().join("src/session_runner.rs");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", relative_source_path(&path)));

    for needle in [
        "CLASS2_DTC_",
        "SERVICE_REPORT_DTCS_BY_STATUS",
        "decode_class2_dtcs",
    ] {
        assert!(
            !content.contains(needle),
            "session_runner.rs must not reference profile-owned GM Class 2 DTC symbol `{needle}`"
        );
    }
}

#[test]
fn session_runner_lly_live_gates_stay_behind_selected_profile() {
    let path = manifest_dir().join("src/session_runner.rs");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", relative_source_path(&path)));

    for (needle, max) in SESSION_RUNNER_LLY_GATE_ALLOWLIST {
        let actual = count_occurrences(&content, needle);
        assert!(
            actual <= *max,
            "session_runner.rs contains {actual} occurrences of `{needle}`; max is {max}. LLY live gates must stay behind selected-profile state/scheduler."
        );
    }
}

#[test]
fn renderers_and_ui_do_not_own_lly_did_literals() {
    for rel in RENDERER_PROFILE_MIGRATED_FILES {
        let path = manifest_dir().join(rel);
        let content = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", relative_source_path(&path)));
        for needle in FORBIDDEN_RENDERER_LLY_DIDS {
            assert!(
                !content.contains(needle),
                "{rel} owns LLY DID literal `{needle}`; use profile display metadata, evidence keys, or profile helpers instead."
            );
        }
        for needle in FORBIDDEN_RENDERER_PROFILE_SYMBOLS {
            assert!(
                !content.contains(needle),
                "{rel} imports profile singleton `{needle}`; resolve display metadata from selected profile state instead."
            );
        }
    }
}

#[cfg(feature = "proof-profile")]
#[test]
fn fixture_module_does_not_reference_gm() {
    let path = manifest_dir().join("src/profiles/fixture/mod.rs");
    let content = fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", relative_source_path(&path)));

    for needle in [
        "gm_enhanced",
        "gm_class2",
        "gm_active",
        "gm_evidence",
        "find_lly_did",
        "LLY_",
        "class2_",
    ] {
        assert!(
            !content.contains(needle),
            "fixture proof profile must not reference `{needle}`"
        );
    }
}

fn live_dashboard_files() -> Vec<PathBuf> {
    let root = manifest_dir();
    let mut out = Vec::new();

    for rel in SCAN_FILES {
        out.push(root.join(rel));
    }
    for rel in SCAN_DIRS {
        out.extend(source_files(&root.join(rel)));
    }

    out.sort();
    out.dedup();
    out
}

fn source_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_source_files(root, &mut out);
    out.sort();
    out
}

fn collect_source_files(root: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(root).unwrap_or_else(|err| panic!("failed to scan {}: {err}", root.display()));
    for entry in entries {
        let entry = entry.expect("source directory entry");
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|err| panic!("failed to stat {}: {err}", path.display()));
        if file_type.is_dir() {
            collect_source_files(&path, out);
        } else if file_type.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("rs")
        {
            out.push(path);
        }
    }
}

fn max_allowed(path: &str, needle: &str) -> usize {
    ALLOWLIST
        .iter()
        .find(|(allowed_path, allowed_needle, _)| {
            *allowed_path == path && *allowed_needle == needle
        })
        .map(|(_, _, max)| *max)
        .unwrap_or(0)
}

fn count_occurrences(content: &str, needle: &str) -> usize {
    content.match_indices(needle).count()
}

fn relative_source_path(path: &Path) -> String {
    path.strip_prefix(manifest_dir())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// DTC service IDs may be composed only inside `mode_runner/diagnostic.rs`,
/// where `service_allowed` gates them by mode (OWL invariant 7). Any other
/// mode_runner file constructing an 0x03/0x07/0x0A request bypasses the gate.
#[test]
fn mode_runner_dtc_service_ids_live_only_in_diagnostic_module() {
    let root = manifest_dir().join("src/mode_runner");
    for path in source_files(&root) {
        let rel = relative_source_path(&path);
        if rel.ends_with("diagnostic.rs") {
            continue;
        }
        let content =
            fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {rel}: {err}"));
        for needle in [
            "0x03,",
            "service_id: 0x03",
            "raw_request(0x03",
            "raw_request(0x07",
            "raw_request(0x0A",
            "service_id: 0x07",
            "service_id: 0x0A",
        ] {
            assert!(
                !content.contains(needle),
                "{rel} composes DTC service bytes (`{needle}`); route through \
                 mode_runner/diagnostic.rs so service_allowed gates them"
            );
        }
    }
}
