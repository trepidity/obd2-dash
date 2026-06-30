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
