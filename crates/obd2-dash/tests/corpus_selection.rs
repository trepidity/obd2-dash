use std::fs;
use std::path::{Path, PathBuf};

use obd2_core::adapter::mock::MockAdapter;
use obd2_core::session::Session;
use obd2_core::vehicle::Protocol;
use obd2_dash::profiles::{
    build_vehicle_context, IdentityConfidence, IdentityOutcome, ProfileRegistry,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct SelectionCase {
    name: String,
    protocol: String,
    vin: Option<String>,
    vin_confidence: String,
    use_embedded_spec: bool,
    expected: ExpectedSelection,
}

#[derive(Debug, Deserialize)]
struct ExpectedSelection {
    selected: bool,
    exact: Vec<String>,
    partial: Vec<String>,
}

#[tokio::test]
async fn corpus_cases_match_expected() {
    for path in corpus_files() {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let case: SelectionCase = serde_json::from_str(&text)
            .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));
        let context = context_from_case(&case).await;
        let state = ProfileRegistry::with_builtins().select(&context);

        assert_eq!(
            state.selected.is_some(),
            case.expected.selected,
            "{} selected",
            case.name
        );
        assert_eq!(
            state
                .exact_matches
                .iter()
                .map(|id| id.as_str().to_string())
                .collect::<Vec<_>>(),
            case.expected.exact,
            "{} exact matches",
            case.name
        );
        assert_eq!(
            state
                .partial_matches
                .iter()
                .map(|entry| entry.profile_id.as_str().to_string())
                .collect::<Vec<_>>(),
            case.expected.partial,
            "{} partial matches",
            case.name
        );
    }
}

#[tokio::test]
async fn no_two_profiles_exact_for_same_context() {
    for path in corpus_files() {
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let case: SelectionCase = serde_json::from_str(&text)
            .unwrap_or_else(|err| panic!("failed to parse {}: {err}", path.display()));
        let context = context_from_case(&case).await;
        let state = ProfileRegistry::with_builtins().select(&context);

        assert!(
            state.exact_matches.len() <= 1,
            "{} had overlapping exact profile matches",
            case.name
        );
    }
}

async fn context_from_case(case: &SelectionCase) -> obd2_dash::profiles::VehicleContext {
    let vin = case
        .vin
        .clone()
        .unwrap_or_else(|| "1GCHK23224F000001".to_string());
    let adapter = MockAdapter::with_vin(&vin);
    let mut session = Session::new(adapter);
    session.initialize().await.unwrap();
    if case.use_embedded_spec {
        let _ = session.identify_vehicle().await;
    }
    let identity = IdentityOutcome {
        vin: case.vin.clone(),
        confidence: parse_confidence(&case.vin_confidence),
    };
    let mut context = build_vehicle_context(&session, 31, &identity);
    context.protocol = parse_protocol(&case.protocol);
    context
}

fn parse_protocol(protocol: &str) -> Protocol {
    match protocol {
        "J1850Vpw" => Protocol::J1850Vpw,
        "Can11Bit500" => Protocol::Can11Bit500,
        "Auto" => Protocol::Auto,
        other => panic!("unknown protocol {other}"),
    }
}

fn parse_confidence(confidence: &str) -> IdentityConfidence {
    match confidence {
        "Unread" => IdentityConfidence::Unread,
        "Corrupted" => IdentityConfidence::Corrupted,
        "Single" => IdentityConfidence::Single,
        "Confirmed" => IdentityConfidence::Confirmed,
        other => panic!("unknown confidence {other}"),
    }
}

fn corpus_files() -> Vec<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus")
        .join("selection");
    let mut files = Vec::new();
    collect_json_files(&root, &mut files);
    files.sort();
    files
}

fn collect_json_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in
        fs::read_dir(dir).unwrap_or_else(|err| panic!("failed to scan {}: {err}", dir.display()))
    {
        let entry = entry.expect("corpus directory entry");
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, files);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            files.push(path);
        }
    }
}
