use std::time::Instant;

use super::format::{RecordingFrame, FRAME_DTC, FRAME_ENHANCED, FRAME_O2, FRAME_PID, FRAME_VOLTAGE};
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
        let total_duration_ms = frames.last().map(|f| f.offset_ms as u64).unwrap_or(0);

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
        self.cursor = self.frames.partition_point(|f| (f.offset_ms as u64) <= pos);
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

    /// Check if a frame is an enhanced PID frame.
    pub fn is_enhanced_frame(frame: &RecordingFrame) -> bool {
        frame.frame_type == FRAME_ENHANCED
    }

    /// Check if a frame is an O2 monitoring frame.
    pub fn is_o2_frame(frame: &RecordingFrame) -> bool {
        frame.frame_type == FRAME_O2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recording::index::SessionEntry;
    use chrono::Utc;
    use std::path::PathBuf;

    fn sample_entry() -> SessionEntry {
        SessionEntry {
            session_id: "test-session".into(),
            start_time: Utc::now(),
            vin: Some("TESTVIN1234567890".into()),
            vehicle_name: Some("Test Car".into()),
            duration_secs: 60,
            frame_count: 100,
            file_path: PathBuf::from("test.obd2rec"),
            file_size_bytes: 1400,
            compressed: false,
        }
    }

    fn sample_frames() -> Vec<RecordingFrame> {
        (0..100)
            .map(|i| RecordingFrame {
                frame_type: FRAME_PID,
                offset_ms: i * 100, // 100ms apart, 0..9900ms
                pid_code: 0x0C,
                value: 2000.0 + i as f64,
                raw_bytes: vec![],
            })
            .collect()
    }

    #[test]
    fn test_new_replay_controller() {
        let ctrl = ReplayController::new(sample_entry(), sample_frames());
        assert_eq!(ctrl.cursor, 0);
        assert!(!ctrl.paused);
        assert_eq!(ctrl.playback_speed, PlaybackSpeed::Normal);
        assert_eq!(ctrl.total_duration_ms, 9900);
        assert!(!ctrl.is_finished());
    }

    #[test]
    fn test_empty_frames() {
        let ctrl = ReplayController::new(sample_entry(), vec![]);
        assert_eq!(ctrl.total_duration_ms, 0);
        assert!(ctrl.is_finished());
    }

    #[test]
    fn test_toggle_pause() {
        let mut ctrl = ReplayController::new(sample_entry(), sample_frames());
        assert!(!ctrl.paused);

        ctrl.toggle_pause();
        assert!(ctrl.paused);

        ctrl.toggle_pause();
        assert!(!ctrl.paused);
    }

    #[test]
    fn test_paused_returns_no_frames() {
        let mut ctrl = ReplayController::new(sample_entry(), sample_frames());
        ctrl.toggle_pause(); // Pause immediately

        let frames = ctrl.next_frames();
        assert!(frames.is_empty(), "paused controller should return no frames");
    }

    #[test]
    fn test_cycle_speed() {
        let mut ctrl = ReplayController::new(sample_entry(), sample_frames());
        assert_eq!(ctrl.playback_speed, PlaybackSpeed::Normal);

        ctrl.cycle_speed();
        assert_eq!(ctrl.playback_speed, PlaybackSpeed::Double);

        ctrl.cycle_speed();
        assert_eq!(ctrl.playback_speed, PlaybackSpeed::Quad);

        ctrl.cycle_speed();
        assert_eq!(ctrl.playback_speed, PlaybackSpeed::Half);

        ctrl.cycle_speed();
        assert_eq!(ctrl.playback_speed, PlaybackSpeed::Normal);
    }

    #[test]
    fn test_speed_labels() {
        assert_eq!(PlaybackSpeed::Half.label(), "0.5x");
        assert_eq!(PlaybackSpeed::Normal.label(), "1x");
        assert_eq!(PlaybackSpeed::Double.label(), "2x");
        assert_eq!(PlaybackSpeed::Quad.label(), "4x");
    }

    #[test]
    fn test_speed_multipliers() {
        assert!((PlaybackSpeed::Half.multiplier() - 0.5).abs() < 0.001);
        assert!((PlaybackSpeed::Normal.multiplier() - 1.0).abs() < 0.001);
        assert!((PlaybackSpeed::Double.multiplier() - 2.0).abs() < 0.001);
        assert!((PlaybackSpeed::Quad.multiplier() - 4.0).abs() < 0.001);
    }

    #[test]
    fn test_seek_forward() {
        let mut ctrl = ReplayController::new(sample_entry(), sample_frames());
        ctrl.toggle_pause(); // Pause to get deterministic position
        assert_eq!(ctrl.current_position_ms(), 0);

        ctrl.seek_forward(5000);
        assert_eq!(ctrl.current_position_ms(), 5000);

        // Cursor should advance past the 5000ms mark
        // Frames at 0, 100, 200, ..., 4900 ms should be behind cursor
        assert_eq!(ctrl.cursor, 51); // frames 0..=50 have offset_ms <= 5000
    }

    #[test]
    fn test_seek_backward() {
        let mut ctrl = ReplayController::new(sample_entry(), sample_frames());
        ctrl.toggle_pause();
        ctrl.seek_forward(8000);
        assert_eq!(ctrl.current_position_ms(), 8000);

        ctrl.seek_backward(3000);
        assert_eq!(ctrl.current_position_ms(), 5000);
    }

    #[test]
    fn test_seek_backward_saturates_at_zero() {
        let mut ctrl = ReplayController::new(sample_entry(), sample_frames());
        ctrl.toggle_pause();
        ctrl.seek_forward(2000);
        ctrl.seek_backward(5000); // Would go negative without saturation
        assert_eq!(ctrl.current_position_ms(), 0);
    }

    #[test]
    fn test_seek_forward_clamped_at_end() {
        let mut ctrl = ReplayController::new(sample_entry(), sample_frames());
        ctrl.toggle_pause();
        ctrl.seek_forward(999_999);
        assert_eq!(ctrl.current_position_ms(), ctrl.total_duration_ms);
    }

    #[test]
    fn test_progress_ratio() {
        let mut ctrl = ReplayController::new(sample_entry(), sample_frames());
        ctrl.toggle_pause();
        assert!((ctrl.progress_ratio() - 0.0).abs() < 0.01);

        ctrl.seek_forward(ctrl.total_duration_ms);
        assert!((ctrl.progress_ratio() - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_progress_text_format() {
        let mut ctrl = ReplayController::new(sample_entry(), sample_frames());
        ctrl.toggle_pause();
        let text = ctrl.progress_text();
        assert!(text.contains("/"), "progress text should contain '/', got '{}'", text);
    }

    #[test]
    fn test_frame_type_checks() {
        let pid_frame = RecordingFrame {
            frame_type: FRAME_PID, offset_ms: 0, pid_code: 0, value: 0.0,
            raw_bytes: vec![],
        };
        let voltage_frame = RecordingFrame {
            frame_type: FRAME_VOLTAGE, offset_ms: 0, pid_code: 0, value: 0.0,
            raw_bytes: vec![],
        };
        let dtc_frame = RecordingFrame {
            frame_type: FRAME_DTC, offset_ms: 0, pid_code: 0, value: 0.0,
            raw_bytes: vec![],
        };
        let enhanced_frame = RecordingFrame {
            frame_type: FRAME_ENHANCED, offset_ms: 0, pid_code: 0, value: 0.0,
            raw_bytes: vec![],
        };
        let o2_frame = RecordingFrame {
            frame_type: FRAME_O2, offset_ms: 0, pid_code: 0, value: 0.0,
            raw_bytes: vec![],
        };

        assert!(ReplayController::is_pid_frame(&pid_frame));
        assert!(ReplayController::is_voltage_frame(&voltage_frame));
        assert!(ReplayController::is_dtc_frame(&dtc_frame));
        assert!(ReplayController::is_enhanced_frame(&enhanced_frame));
        assert!(ReplayController::is_o2_frame(&o2_frame));

        assert!(!ReplayController::is_pid_frame(&voltage_frame));
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
