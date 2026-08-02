use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn live_code_cannot_reach_gated_framing_helpers() {
    let root = repo_root();
    let src = root.join("crates/obd2-dash/src");
    let mut violations = Vec::new();

    for file in rust_files(&src) {
        let rel = file.strip_prefix(&root).unwrap();
        let rel_text = rel.to_string_lossy();
        if is_allowed_gated_file(&rel_text) {
            continue;
        }
        let text = fs::read_to_string(&file).unwrap();
        for (line_idx, line) in text.lines().enumerate() {
            for symbol in [
                "class2_routed_request",
                "class2_header",
                "class2_dtc_all_request",
                "class2_dtc_active_request",
            ] {
                if line.contains(symbol) {
                    violations.push(format!("{}:{}: {}", rel_text, line_idx + 1, symbol));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "gated raw framing helper reached live source:\n{}",
        violations.join("\n")
    );
}

#[test]
fn pending_migration_allowlist_is_tight() {
    let root = repo_root();
    let mut files = rust_files(&root.join("crates/obd2-dash/src"));
    files.push(root.join("apps/obd2-gui/src-tauri/src/main.rs"));

    assert_only_allowed(
        &root,
        &files,
        "find_lly_did",
        &[
            "crates/obd2-dash/src/gm_enhanced.rs",
            "crates/obd2-dash/src/profiles/gm/lly.rs",
        ],
        "GUI and live runners must read enhanced values through the selected profile runtime",
    );
    assert_only_allowed(
        &root,
        &files,
        "request_gm_node",
        &[],
        "GUI and live runners must not own GM node request helpers",
    );
    assert_only_allowed(
        &root,
        &files,
        ".adapter_mut(",
        &[],
        "GUI and live runners must not reach around the profile runtime",
    );
    assert_only_allowed(
        &root,
        &files,
        ".raw_request(",
        &[
            "crates/obd2-dash/src/mode_runner/diagnostic.rs",
            "crates/obd2-dash/src/profiles/runtime.rs",
            "crates/obd2-dash/src/session_runner.rs",
        ],
        "raw Session requests are restricted to legacy migration code, profile routing, and the runner's diagnostic gate",
    );
}

#[test]
fn gui_backend_cannot_reach_raw_gm_class2_helpers() {
    let root = repo_root();
    let path = root.join("apps/obd2-gui/src-tauri/src/main.rs");
    let text = fs::read_to_string(&path).unwrap();

    for needle in [
        "CLASS2_DTC_",
        "SERVICE_REPORT_DTCS_BY_STATUS",
        "DEFAULT_CLASS2_NODES",
        "decode_class2_dtcs",
        "class2_routed_request",
        "RoutedRequest",
        ".routed_request(",
        ".adapter_mut(",
        "request_gm_node",
        "find_lly_did",
    ] {
        assert!(
            !text.contains(needle),
            "GUI backend must not reach raw GM helper `{needle}`"
        );
    }
}

fn assert_only_allowed(root: &Path, files: &[PathBuf], needle: &str, allowed: &[&str], note: &str) {
    let mut violations = Vec::new();
    for file in files {
        let text = fs::read_to_string(file).unwrap();
        let rel = file.strip_prefix(root).unwrap().to_string_lossy();
        for (line_idx, line) in text.lines().enumerate() {
            if line.contains(needle) && !allowed.iter().any(|path| rel == *path) {
                violations.push(format!("{rel}:{}: {needle}", line_idx + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "{needle} outside pending-migration allowlist ({note}):\n{}",
        violations.join("\n")
    );
}

fn is_allowed_gated_file(rel: &str) -> bool {
    matches!(
        rel,
        "crates/obd2-dash/src/gm_class2.rs"
            | "crates/obd2-dash/src/gm_enhanced.rs"
            | "crates/obd2-dash/src/profiles/probe.rs"
            | "crates/obd2-dash/src/profiles/runtime.rs"
    )
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rust_files(root, &mut out);
    out
}

fn collect_rust_files(path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(path) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}
