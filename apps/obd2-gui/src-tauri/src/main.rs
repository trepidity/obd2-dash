#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Tauri shell for the shared OBD mode runner.
//!
//! This binary deliberately has no `Session`, adapter, or serial transport
//! imports.  `serial_connector` constructs a fresh session for the background
//! runner; command handlers only read its cached snapshots or enqueue bounded
//! controls.

mod commands;
mod runner_state;
mod serial_connector;
mod snapshot_dto;

use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use tauri::Manager;

const MAX_RECORDING_FILE_BYTES: u64 = 256 * 1024 * 1024;

#[tauri::command]
fn recordings_directory(app: tauri::AppHandle) -> Result<String, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?
        .join("recordings");
    fs::create_dir_all(&dir).map_err(|error| {
        format!(
            "failed to create recordings directory {}: {error}",
            dir.display()
        )
    })?;
    Ok(dir.display().to_string())
}

/// Recording inspection is local file work, independent of the live runner.
#[tauri::command]
fn read_recording_file(path: String) -> Result<Vec<u8>, String> {
    let path = PathBuf::from(path);
    let metadata = fs::metadata(&path).map_err(|error| {
        format!(
            "failed to inspect recording file {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!("recording path is not a file: {}", path.display()));
    }
    if metadata.len() > MAX_RECORDING_FILE_BYTES {
        return Err(format!(
            "recording file is too large: {} bytes (limit {} bytes)",
            metadata.len(),
            MAX_RECORDING_FILE_BYTES
        ));
    }
    fs::read(&path)
        .map_err(|error| format!("failed to read recording file {}: {error}", path.display()))
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let capability_db_path = app
                .path()
                .app_data_dir()
                .map_err(|error| format!("resolve app data directory: {error}"))?
                .join("capabilities.sqlite");
            let state = runner_state::RunnerState::bootstrap(
                serial_connector::SerialSessionConnector::from_environment(),
                runner_state::RunnerStateConfig::new(capability_db_path),
            );
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::diagnostic_snapshot,
            commands::run_diagnostic,
            commands::rescan_vehicle,
            commands::cancel_foreground,
            commands::set_active_view,
            commands::request_active_test,
            commands::start_recording,
            commands::stop_recording,
            recordings_directory,
            read_recording_file
        ])
        .build(tauri::generate_context!())
        .expect("failed to run OBD2 Dash GUI");

    // The event loop normally terminates immediately after ExitRequested.
    // Hold that first exit request until the runner has released its session
    // and the recording worker has flushed its append-only file.
    let exiting = Arc::new(AtomicBool::new(false));
    app.run(move |handle, event| {
        if let tauri::RunEvent::ExitRequested { api, .. } = event {
            if exiting.swap(true, Ordering::AcqRel) {
                return;
            }
            api.prevent_exit();
            let state = handle.state::<runner_state::RunnerState>().inner().clone();
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let _ = state.shutdown().await;
                handle.exit(0);
            });
        }
    });
}
