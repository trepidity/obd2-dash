use super::snapshot::{DiscoveryOrigin, ModeState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandReply {
    Accepted,
    Busy,
    NotReady,
    NotRunning,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerCommand {
    RunDiagnostic,
    RescanVehicle,
    CancelForeground,
    Shutdown,
}

pub fn reply_for(command: RunnerCommand, mode: &ModeState) -> CommandReply {
    match command {
        RunnerCommand::RunDiagnostic | RunnerCommand::RescanVehicle => match mode {
            ModeState::Telemetry => CommandReply::Accepted,
            ModeState::Diagnostic { .. }
            | ModeState::Discovering {
                origin: DiscoveryOrigin::Rescan,
                ..
            } => CommandReply::Busy,
            ModeState::ShuttingDown => CommandReply::Closed,
            _ => CommandReply::NotReady,
        },
        RunnerCommand::CancelForeground => match mode {
            ModeState::Diagnostic { .. }
            | ModeState::Discovering {
                origin: DiscoveryOrigin::Rescan,
                ..
            } => CommandReply::Accepted,
            ModeState::ShuttingDown => CommandReply::Closed,
            _ => CommandReply::NotRunning,
        },
        RunnerCommand::Shutdown => match mode {
            ModeState::ShuttingDown => CommandReply::Closed,
            _ => CommandReply::Accepted,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_table_matches_spec_modes() {
        assert_eq!(
            reply_for(RunnerCommand::RunDiagnostic, &ModeState::Connecting),
            CommandReply::NotReady
        );
        assert_eq!(
            reply_for(RunnerCommand::RunDiagnostic, &ModeState::Telemetry),
            CommandReply::Accepted
        );
        assert_eq!(
            reply_for(
                RunnerCommand::RescanVehicle,
                &ModeState::Discovering {
                    origin: DiscoveryOrigin::Rescan,
                    step: 0,
                    total: 1
                }
            ),
            CommandReply::Busy
        );
        assert_eq!(
            reply_for(RunnerCommand::CancelForeground, &ModeState::Telemetry),
            CommandReply::NotRunning
        );
    }
}
