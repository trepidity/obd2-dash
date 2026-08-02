use super::snapshot::{DiscoveryOrigin, ModeState};
use super::ViewId;
use crate::gm_active::GmActiveTestCommand;
use tokio::sync::{mpsc, oneshot, watch};

/// The discrete control queue is intentionally small. Commands that cannot be
/// accepted now must be reported to the caller, not accumulated for a later
/// vehicle/session state.
pub const CONTROL_CHANNEL_CAPACITY: usize = 8;

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

/// Commands delivered to the asynchronous runner owner.
///
/// This is separate from [`RunnerCommand`], which remains the synchronous
/// state-machine input used by the lifecycle tests. The runner drains this
/// queue at request boundaries, acknowledges the command after applying its
/// mode-table decision, and never retains rejected work.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlCommand {
    RunDiagnostic,
    RescanVehicle,
    CancelForeground,
    /// Active tests retain the existing locked/evidence executor. This command
    /// only moves the request onto the single Session-owning runner.
    RequestActiveTest(GmActiveTestCommand),
    /// The acknowledgement is deliberately retained until the runner has
    /// dropped Session and flushed the last accepted persistence batch.
    Shutdown,
}

impl ControlCommand {
    pub fn reply_for(&self, mode: &ModeState) -> CommandReply {
        match self {
            Self::RunDiagnostic => reply_for(RunnerCommand::RunDiagnostic, mode),
            Self::RescanVehicle => reply_for(RunnerCommand::RescanVehicle, mode),
            Self::CancelForeground => reply_for(RunnerCommand::CancelForeground, mode),
            Self::RequestActiveTest(_) => match mode {
                ModeState::Telemetry => CommandReply::Accepted,
                ModeState::Diagnostic { .. }
                | ModeState::Discovering {
                    origin: DiscoveryOrigin::Rescan,
                    ..
                } => CommandReply::Busy,
                ModeState::ShuttingDown => CommandReply::Closed,
                _ => CommandReply::NotReady,
            },
            Self::Shutdown => reply_for(RunnerCommand::Shutdown, mode),
        }
    }
}

/// One queued control request and its acknowledgement route.
#[derive(Debug)]
pub struct QueuedCommand {
    command: ControlCommand,
    acknowledgement: oneshot::Sender<CommandReply>,
}

impl QueuedCommand {
    pub fn command(&self) -> &ControlCommand {
        &self.command
    }

    /// Reply exactly once. A dropped caller is harmless: runner ownership and
    /// the state transition remain authoritative.
    pub fn acknowledge(self, reply: CommandReply) {
        let _ = self.acknowledgement.send(reply);
    }

    /// Split command execution from acknowledgement timing. Shutdown uses
    /// this to defer its acknowledgement until resource release is complete.
    pub fn into_parts(self) -> (ControlCommand, oneshot::Sender<CommandReply>) {
        (self.command, self.acknowledgement)
    }
}

/// Result returned by the control receiver.
#[derive(Debug)]
pub enum ControlInput {
    Command(QueuedCommand),
    /// All control handles were dropped. Per the runner contract, this is a
    /// shutdown request, never a reconnect trigger.
    Closed,
}

/// Cloneable producer held by Tauri/TUI command surfaces.
#[derive(Debug, Clone)]
pub struct RunnerControl {
    command_tx: mpsc::Sender<QueuedCommand>,
    view_tx: watch::Sender<ViewId>,
}

impl RunnerControl {
    /// Submit without awaiting queue capacity. A full queue is acknowledged as
    /// `Busy` immediately; no task blocks a UI thread and no command waits in
    /// a hidden sender future.
    pub fn submit(&self, command: ControlCommand) -> oneshot::Receiver<CommandReply> {
        let (acknowledgement, receiver) = oneshot::channel();
        let queued = QueuedCommand {
            command,
            acknowledgement,
        };

        match self.command_tx.try_send(queued) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(queued)) => {
                queued.acknowledge(CommandReply::Busy);
            }
            Err(mpsc::error::TrySendError::Closed(queued)) => {
                queued.acknowledge(CommandReply::Closed);
            }
        }
        receiver
    }

    /// Replace, rather than queue, the active view. The runner reads this once
    /// per telemetry cycle, so rapid tab changes cannot replay stale views.
    pub fn set_active_view(&self, view: ViewId) {
        self.view_tx.send_replace(view);
    }

    pub fn active_view(&self) -> ViewId {
        self.view_tx.borrow().clone()
    }
}

/// Runner-owned halves of the bounded control plane.
#[derive(Debug)]
pub struct RunnerControlReceiver {
    command_rx: mpsc::Receiver<QueuedCommand>,
    view_rx: watch::Receiver<ViewId>,
}

impl RunnerControlReceiver {
    /// Wait for one discrete control input. `Closed` must start orderly
    /// shutdown; callers must not substitute reconnect behavior here.
    pub async fn recv(&mut self) -> ControlInput {
        match self.command_rx.recv().await {
            Some(command) if self.command_rx.is_closed() => {
                // A closed producer means its remaining buffered work is no
                // longer actionable. Resolve every acknowledgement rather
                // than leaving callers waiting while shutdown begins.
                command.acknowledge(CommandReply::Closed);
                self.reject_buffered_after_close();
                ControlInput::Closed
            }
            Some(command) => ControlInput::Command(command),
            None => ControlInput::Closed,
        }
    }

    /// Drain work already accepted at the current request boundary. Returning
    /// `Closed` distinguishes a disconnected producer from an empty queue.
    pub fn try_recv(&mut self) -> Option<ControlInput> {
        if self.command_rx.is_closed() {
            self.reject_buffered_after_close();
            return Some(ControlInput::Closed);
        }
        match self.command_rx.try_recv() {
            Ok(command) => Some(ControlInput::Command(command)),
            Err(mpsc::error::TryRecvError::Disconnected) => Some(ControlInput::Closed),
            Err(mpsc::error::TryRecvError::Empty) => None,
        }
    }

    /// Read exactly once while building a cycle; intermediate view changes are
    /// intentionally collapsed by the watch channel.
    pub fn active_view(&mut self) -> ViewId {
        self.view_rx.borrow_and_update().clone()
    }

    fn reject_buffered_after_close(&mut self) {
        while let Ok(command) = self.command_rx.try_recv() {
            command.acknowledge(CommandReply::Closed);
        }
    }
}

/// Construct the runner control plane. `Gauges` is the deterministic initial
/// view used by the existing scheduler.
pub fn control_channel() -> (RunnerControl, RunnerControlReceiver) {
    let (command_tx, command_rx) = mpsc::channel(CONTROL_CHANNEL_CAPACITY);
    let (view_tx, view_rx) = watch::channel(ViewId::Gauges);
    (
        RunnerControl {
            command_tx,
            view_tx,
        },
        RunnerControlReceiver {
            command_rx,
            view_rx,
        },
    )
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

    #[tokio::test]
    async fn full_queue_returns_busy_without_waiting_for_capacity() {
        let (control, mut receiver) = control_channel();
        let mut accepted = Vec::new();
        for _ in 0..CONTROL_CHANNEL_CAPACITY {
            accepted.push(control.submit(ControlCommand::RunDiagnostic));
        }

        let overflow = control.submit(ControlCommand::RescanVehicle);
        assert_eq!(overflow.await, Ok(CommandReply::Busy));

        let ControlInput::Command(command) = receiver.recv().await else {
            panic!("control producer is still alive");
        };
        command.acknowledge(CommandReply::Accepted);
        assert_eq!(accepted.remove(0).await, Ok(CommandReply::Accepted));
    }

    #[tokio::test]
    async fn duplicate_foreground_command_is_rejected_at_consumer_without_queueing() {
        let (control, mut receiver) = control_channel();
        let first = control.submit(ControlCommand::RunDiagnostic);
        let second = control.submit(ControlCommand::RescanVehicle);

        let ControlInput::Command(first_command) = receiver.recv().await else {
            panic!("first command must be available");
        };
        assert_eq!(
            first_command.command().reply_for(&ModeState::Telemetry),
            CommandReply::Accepted
        );
        first_command.acknowledge(CommandReply::Accepted);

        let foreground = ModeState::Diagnostic {
            phase: 0,
            phase_total: 5,
            step: 0,
            total: 0,
        };
        let ControlInput::Command(second_command) = receiver.recv().await else {
            panic!("second command must be available");
        };
        assert_eq!(
            second_command.command().reply_for(&foreground),
            CommandReply::Busy
        );
        second_command.acknowledge(CommandReply::Busy);

        assert_eq!(first.await, Ok(CommandReply::Accepted));
        assert_eq!(second.await, Ok(CommandReply::Busy));
        assert!(receiver.try_recv().is_none());
    }

    #[test]
    fn active_view_is_latest_value_not_a_discrete_command_queue() {
        let (control, mut receiver) = control_channel();
        control.set_active_view(ViewId::Engine);
        control.set_active_view(ViewId::Diagnostics);
        control.set_active_view(ViewId::Other("profile".to_owned()));

        assert_eq!(receiver.active_view(), ViewId::Other("profile".to_owned()));
        assert!(receiver.try_recv().is_none());
    }

    #[tokio::test]
    async fn command_channel_close_is_shutdown_signal() {
        let (control, mut receiver) = control_channel();
        let queued = control.submit(ControlCommand::RunDiagnostic);
        drop(control);

        assert!(matches!(receiver.recv().await, ControlInput::Closed));
        assert_eq!(queued.await, Ok(CommandReply::Closed));
        assert!(matches!(receiver.try_recv(), Some(ControlInput::Closed)));
    }

    #[test]
    fn active_test_uses_same_mode_table_as_other_foreground_work() {
        let command =
            ControlCommand::RequestActiveTest(GmActiveTestCommand::VgtTcLearn { enabled: true });
        assert_eq!(
            command.reply_for(&ModeState::Connecting),
            CommandReply::NotReady
        );
        assert_eq!(
            command.reply_for(&ModeState::Telemetry),
            CommandReply::Accepted
        );
        assert_eq!(
            command.reply_for(&ModeState::Diagnostic {
                phase: 0,
                phase_total: 5,
                step: 0,
                total: 0,
            }),
            CommandReply::Busy
        );
    }
}
