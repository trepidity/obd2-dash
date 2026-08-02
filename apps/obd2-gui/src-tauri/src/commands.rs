//! Thin Tauri command surface over the shared mode-runner control plane.
//!
//! A GUI command may clone a cached snapshot or enqueue one bounded control
//! message.  It must never retain, create, or borrow the serial Session.

use crate::{runner_state::RunnerState, snapshot_dto::DiagnosticSnapshot};
use obd2_dash::{
    gm_active::GmActiveTestCommand,
    mode_runner::{CommandReply, ControlCommand, ViewId},
};
use serde::Serialize;
use tauri::{Manager, State};

/// Explicit acknowledgement result instead of treating a Busy or NotReady
/// state as a command failure.  The UI can report the runner's state without
/// guessing whether a request entered the bounded queue.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandReplyDto {
    Accepted,
    Busy,
    NotReady,
    NotRunning,
    Closed,
}

impl From<CommandReply> for CommandReplyDto {
    fn from(reply: CommandReply) -> Self {
        match reply {
            CommandReply::Accepted => Self::Accepted,
            CommandReply::Busy => Self::Busy,
            CommandReply::NotReady => Self::NotReady,
            CommandReply::NotRunning => Self::NotRunning,
            CommandReply::Closed => Self::Closed,
        }
    }
}

#[tauri::command]
pub async fn diagnostic_snapshot(
    state: State<'_, RunnerState>,
) -> Result<DiagnosticSnapshot, String> {
    Ok(DiagnosticSnapshot::from(&state.snapshot()))
}

#[tauri::command]
pub async fn run_diagnostic(state: State<'_, RunnerState>) -> Result<CommandReplyDto, String> {
    submit(&state, ControlCommand::RunDiagnostic).await
}

#[tauri::command]
pub async fn rescan_vehicle(state: State<'_, RunnerState>) -> Result<CommandReplyDto, String> {
    submit(&state, ControlCommand::RescanVehicle).await
}

#[tauri::command]
pub async fn cancel_foreground(state: State<'_, RunnerState>) -> Result<CommandReplyDto, String> {
    submit(&state, ControlCommand::CancelForeground).await
}

#[tauri::command]
pub async fn request_active_test(
    state: State<'_, RunnerState>,
    command: GmActiveTestCommand,
) -> Result<CommandReplyDto, String> {
    submit(&state, ControlCommand::RequestActiveTest(command)).await
}

#[tauri::command]
pub async fn start_recording(
    app: tauri::AppHandle,
    state: State<'_, RunnerState>,
) -> Result<String, String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?
        .join("recordings");
    state.start_recording(directory).await
}

#[tauri::command]
pub async fn stop_recording(state: State<'_, RunnerState>) -> Result<String, String> {
    state.stop_recording().await
}

async fn submit(state: &RunnerState, command: ControlCommand) -> Result<CommandReplyDto, String> {
    state
        .submit(command)
        .await
        .map(CommandReplyDto::from)
        .map_err(|_| "mode runner stopped before acknowledging the command".to_string())
}

#[tauri::command]
pub fn set_active_view(state: State<'_, RunnerState>, view: String) -> Result<(), String> {
    state.set_active_view(view_id(&view));
    Ok(())
}

fn view_id(view: &str) -> ViewId {
    match view {
        "overview" | "gauges" => ViewId::Gauges,
        "engine" => ViewId::Engine,
        "diagnostics" => ViewId::Diagnostics,
        other => ViewId::Other(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_names_map_to_scheduler_views_without_transport_access() {
        assert_eq!(view_id("overview"), ViewId::Gauges);
        assert_eq!(view_id("engine"), ViewId::Engine);
        assert_eq!(view_id("diagnostics"), ViewId::Diagnostics);
        assert_eq!(view_id("cap:turbo"), ViewId::Other("cap:turbo".to_string()));
    }

    #[test]
    fn command_replies_are_stable_json_tokens() {
        assert_eq!(
            serde_json::to_value(CommandReplyDto::Busy).unwrap(),
            serde_json::Value::String("busy".to_string())
        );
        assert_eq!(
            CommandReplyDto::from(CommandReply::NotReady),
            CommandReplyDto::NotReady
        );
    }
}
