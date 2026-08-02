use std::path::PathBuf;

fn gui_source(name: &str) -> String {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("src");
    path.push(name);
    std::fs::read_to_string(path).expect("read GUI source")
}

#[test]
fn only_the_serial_connector_owns_live_transport_types() {
    for name in [
        "main.rs",
        "commands.rs",
        "runner_state.rs",
        "snapshot_dto.rs",
    ] {
        let source = gui_source(name);
        assert!(
            !source.contains("obd2_core::session"),
            "{name} imports a session boundary"
        );
        assert!(
            !source.contains("Elm327Adapter"),
            "{name} imports the serial adapter"
        );
        assert!(
            !source.contains("SerialTransport"),
            "{name} imports serial transport"
        );
        assert!(
            !source.contains("raw_request"),
            "{name} can issue a raw request"
        );
    }

    let connector = gui_source("serial_connector.rs");
    assert!(connector.contains("Elm327Adapter"));
    assert!(connector.contains("SerialTransport"));
}

#[test]
fn legacy_live_backend_is_not_present_in_the_gui_source_tree() {
    for name in [
        "main.rs",
        "commands.rs",
        "runner_state.rs",
        "serial_connector.rs",
        "snapshot_dto.rs",
    ] {
        assert!(
            !gui_source(name).contains("LiveBackend"),
            "{name} still references the removed inline backend"
        );
    }
}
