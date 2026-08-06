pub mod format;
pub mod index;
pub mod reader;
pub mod replay;
pub mod storage;
pub mod writer;

use std::time::Instant;

use replay::ReplayController;
use writer::RecordingWriter;

/// Top-level recording state machine.
pub enum RecordingState {
    /// Not recording or replaying.
    Idle,
    /// Actively recording live OBD2 data.
    Recording {
        writer: RecordingWriter,
        start_instant: Instant,
    },
    /// Playing back a recorded session.
    Replaying(ReplayController),
}

impl RecordingState {
    pub fn is_recording(&self) -> bool {
        matches!(self, RecordingState::Recording { .. })
    }

    pub fn is_replaying(&self) -> bool {
        matches!(self, RecordingState::Replaying(_))
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, RecordingState::Idle)
    }
}

impl std::fmt::Debug for RecordingState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecordingState::Idle => write!(f, "RecordingState::Idle"),
            RecordingState::Recording { .. } => write!(f, "RecordingState::Recording"),
            RecordingState::Replaying(_) => write!(f, "RecordingState::Replaying"),
        }
    }
}
