use std::time::Instant;

use super::format::{RecordingFrame, FRAME_DTC, FRAME_PID, FRAME_VOLTAGE};
use super::index::SessionEntry;

/// Playback speed options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlaybackSpeed {
    Half,
    Normal,
    Double,
    Quad,
}

impl PlaybackSpeed {
    pub fn multiplier(&self) -> f64 {
        match self {
            PlaybackSpeed::Half => 0.5,
            PlaybackSpeed::Normal => 1.0,
            PlaybackSpeed::Double => 2.0,
            PlaybackSpeed::Quad => 4.0,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            PlaybackSpeed::Half => "0.5x",
            PlaybackSpeed::Normal => "1x",
            PlaybackSpeed::Double => "2x",
            PlaybackSpeed::Quad => "4x",
        }
    }

    pub fn next(&self) -> PlaybackSpeed {
        match self {
            PlaybackSpeed::Half => PlaybackSpeed::Normal,
            PlaybackSpeed::Normal => PlaybackSpeed::Double,
            PlaybackSpeed::Double => PlaybackSpeed::Quad,
            PlaybackSpeed::Quad => PlaybackSpeed::Half,
        }
    }
}

/// Controls playback of a recorded session.
pub struct ReplayController {
    pub session: SessionEntry,
    pub frames: Vec<RecordingFrame>,
    pub cursor: usize,
    pub playback_speed: PlaybackSpeed,
    pub paused: bool,
    /// When playback started (or resumed after pause).
    start_instant: Instant,
    /// Accumulated playback time in ms (accounts for pauses and seeks).
    elapsed_offset_ms: u64,
    /// Total duration of the recording in ms.
    pub total_duration_ms: u64,
}

impl ReplayController {
    pub fn new(session: SessionEntry, frames: Vec<RecordingFrame>) -> Self {
        let total_duration_ms = frames
            .last()
            .map(|f| f.offset_ms as u64)
            .unwrap_or(0);

        Self {
            session,
            frames,
            cursor: 0,
            playback_speed: PlaybackSpeed::Normal,
            paused: false,
            start_instant: Instant::now(),
            elapsed_offset_ms: 0,
            total_duration_ms,
        }
    }

    /// Get the current effective playback position in ms.
    pub fn current_position_ms(&self) -> u64 {
        if self.paused {
            self.elapsed_offset_ms
        } else {
            let real_elapsed = self.start_instant.elapsed().as_millis() as u64;
            let scaled = (real_elapsed as f64 * self.playback_speed.multiplier()) as u64;
            self.elapsed_offset_ms + scaled
        }
    }

    /// Get frames that should be emitted at the current playback position.
    /// Returns frames whose offset_ms <= current position, advancing the cursor.
    pub fn next_frames(&mut self) -> Vec<RecordingFrame> {
        if self.paused {
            return vec![];
        }

        let pos = self.current_position_ms();
        let mut result = Vec::new();

        while self.cursor < self.frames.len() {
            let frame = &self.frames[self.cursor];
            if (frame.offset_ms as u64) <= pos {
                result.push(frame.clone());
                self.cursor += 1;
            } else {
                break;
            }
        }

        result
    }

    /// Check if the replay has finished.
    pub fn is_finished(&self) -> bool {
        self.cursor >= self.frames.len()
    }

    /// Toggle pause/resume.
    pub fn toggle_pause(&mut self) {
        if self.paused {
            // Resuming: reset start_instant, keep elapsed_offset_ms
            self.start_instant = Instant::now();
            self.paused = false;
        } else {
            // Pausing: accumulate elapsed time
            self.elapsed_offset_ms = self.current_position_ms();
            self.paused = true;
        }
    }

    /// Seek forward by the given number of milliseconds.
    pub fn seek_forward(&mut self, ms: u64) {
        let new_pos = self.current_position_ms() + ms;
        self.seek_to(new_pos);
    }

    /// Seek backward by the given number of milliseconds.
    pub fn seek_backward(&mut self, ms: u64) {
        let current = self.current_position_ms();
        let new_pos = current.saturating_sub(ms);
        self.seek_to(new_pos);
    }

    /// Seek to an absolute position in ms.
    fn seek_to(&mut self, position_ms: u64) {
        let pos = position_ms.min(self.total_duration_ms);
        self.elapsed_offset_ms = pos;
        self.start_instant = Instant::now();

        // Find the cursor position for this offset
        self.cursor = self
            .frames
            .partition_point(|f| (f.offset_ms as u64) <= pos);
    }

    /// Cycle to the next playback speed.
    pub fn cycle_speed(&mut self) {
        // Preserve current position when changing speed
        self.elapsed_offset_ms = self.current_position_ms();
        self.start_instant = Instant::now();
        self.playback_speed = self.playback_speed.next();
    }

    /// Get the speed label for display.
    pub fn speed_label(&self) -> &'static str {
        self.playback_speed.label()
    }

    /// Get a progress text like "23:45 / 1:23:00".
    pub fn progress_text(&self) -> String {
        let current = self.current_position_ms();
        let total = self.total_duration_ms;
        format!(
            "{} / {}",
            format_duration_ms(current),
            format_duration_ms(total)
        )
    }

    /// Get progress as a ratio (0.0 to 1.0).
    pub fn progress_ratio(&self) -> f64 {
        if self.total_duration_ms == 0 {
            0.0
        } else {
            (self.current_position_ms() as f64 / self.total_duration_ms as f64).clamp(0.0, 1.0)
        }
    }

    /// Check if a frame is a PID frame.
    pub fn is_pid_frame(frame: &RecordingFrame) -> bool {
        frame.frame_type == FRAME_PID
    }

    /// Check if a frame is a voltage frame.
    pub fn is_voltage_frame(frame: &RecordingFrame) -> bool {
        frame.frame_type == FRAME_VOLTAGE
    }

    /// Check if a frame is a DTC frame.
    pub fn is_dtc_frame(frame: &RecordingFrame) -> bool {
        frame.frame_type == FRAME_DTC
    }
}

fn format_duration_ms(ms: u64) -> String {
    let total_secs = ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{}:{:02}", mins, secs)
    }
}
