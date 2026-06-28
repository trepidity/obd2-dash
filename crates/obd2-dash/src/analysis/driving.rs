use std::collections::VecDeque;
use std::time::Instant;

const HARD_BRAKE_THRESHOLD: f64 = -2.8; // m/s², ~0.29g
const JACKRABBIT_ACCEL_THRESHOLD: f64 = 2.8; // m/s²
const JACKRABBIT_THROTTLE_MIN: f64 = 65.0; // %
const JACKRABBIT_SPEED_MAX: f64 = 50.0; // km/h
const SMOOTHNESS_WINDOW: usize = 30; // last ~7.5s for score calc
const ACCEL_HISTORY_CAP: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EventState {
    Normal,
    Active,
}

pub struct DrivingBehavior {
    pub current_accel: f64,
    last_speed_kmh: Option<f64>,
    last_speed_time: Option<Instant>,
    pub accel_history: VecDeque<f64>,
    pub hard_brake_count: u32,
    pub jackrabbit_count: u32,
    pub smoothness_score: f64,
    braking_state: EventState,
    accel_state: EventState,
}

impl Default for DrivingBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl DrivingBehavior {
    pub fn new() -> Self {
        Self {
            current_accel: 0.0,
            last_speed_kmh: None,
            last_speed_time: None,
            accel_history: VecDeque::with_capacity(ACCEL_HISTORY_CAP),
            hard_brake_count: 0,
            jackrabbit_count: 0,
            smoothness_score: 100.0,
            braking_state: EventState::Normal,
            accel_state: EventState::Normal,
        }
    }

    pub fn update(&mut self, speed_kmh: f64, throttle_pct: f64) {
        let now = Instant::now();

        if let (Some(prev_speed), Some(prev_time)) = (self.last_speed_kmh, self.last_speed_time) {
            let dt = now.duration_since(prev_time).as_secs_f64();
            if dt > 0.01 {
                // Convert km/h to m/s: divide by 3.6
                let speed_ms = speed_kmh / 3.6;
                let prev_ms = prev_speed / 3.6;
                self.current_accel = (speed_ms - prev_ms) / dt;

                // Push to history
                if self.accel_history.len() >= ACCEL_HISTORY_CAP {
                    self.accel_history.pop_front();
                }
                self.accel_history.push_back(self.current_accel);

                // Hard brake detection with hysteresis
                match self.braking_state {
                    EventState::Normal => {
                        if self.current_accel < HARD_BRAKE_THRESHOLD {
                            self.braking_state = EventState::Active;
                            self.hard_brake_count += 1;
                        }
                    }
                    EventState::Active => {
                        if self.current_accel > HARD_BRAKE_THRESHOLD / 2.0 {
                            self.braking_state = EventState::Normal;
                        }
                    }
                }

                // Jackrabbit start detection with hysteresis
                match self.accel_state {
                    EventState::Normal => {
                        if self.current_accel > JACKRABBIT_ACCEL_THRESHOLD
                            && throttle_pct > JACKRABBIT_THROTTLE_MIN
                            && speed_kmh < JACKRABBIT_SPEED_MAX
                        {
                            self.accel_state = EventState::Active;
                            self.jackrabbit_count += 1;
                        }
                    }
                    EventState::Active => {
                        if self.current_accel < JACKRABBIT_ACCEL_THRESHOLD / 2.0 {
                            self.accel_state = EventState::Normal;
                        }
                    }
                }

                // Update smoothness score
                self.update_smoothness();
            }
        }

        self.last_speed_kmh = Some(speed_kmh);
        self.last_speed_time = Some(now);
    }

    fn update_smoothness(&mut self) {
        let window: Vec<f64> = self
            .accel_history
            .iter()
            .rev()
            .take(SMOOTHNESS_WINDOW)
            .copied()
            .collect();

        if window.len() < 2 {
            return;
        }

        let mean = window.iter().sum::<f64>() / window.len() as f64;
        let variance = window.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / window.len() as f64;
        let std_dev = variance.sqrt();

        self.smoothness_score = (100.0 - std_dev * 80.0).clamp(0.0, 100.0);
    }

    pub fn smoothness_label(&self) -> &str {
        if self.smoothness_score >= 90.0 {
            "Smooth"
        } else if self.smoothness_score >= 70.0 {
            "Good"
        } else if self.smoothness_score >= 50.0 {
            "Fair"
        } else if self.smoothness_score >= 30.0 {
            "Rough"
        } else {
            "Aggressive"
        }
    }

    pub fn accel_display_history(&self) -> Vec<u64> {
        self.accel_history
            .iter()
            .map(|a| (a.abs() * 100.0) as u64)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: feed a sequence of (speed_kmh, throttle_pct) pairs with small time gaps.
    fn feed_updates(db: &mut DrivingBehavior, updates: &[(f64, f64)]) {
        for &(speed, throttle) in updates {
            db.update(speed, throttle);
            // Small sleep to ensure Instant::now() advances enough for dt > 0.01
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    #[test]
    fn test_initial_state() {
        let db = DrivingBehavior::new();
        assert_eq!(db.hard_brake_count, 0);
        assert_eq!(db.jackrabbit_count, 0);
        assert_eq!(db.smoothness_score, 100.0);
        assert_eq!(db.current_accel, 0.0);
    }

    #[test]
    fn test_steady_speed_is_smooth() {
        let mut db = DrivingBehavior::new();
        // Steady 60 km/h for several samples — should remain very smooth
        feed_updates(
            &mut db,
            &[
                (60.0, 30.0),
                (60.0, 30.0),
                (60.0, 30.0),
                (60.0, 30.0),
                (60.0, 30.0),
            ],
        );

        assert!(
            db.smoothness_score > 90.0,
            "steady speed should score high, got {}",
            db.smoothness_score
        );
        assert_eq!(db.hard_brake_count, 0);
        assert_eq!(db.jackrabbit_count, 0);
    }

    #[test]
    fn test_hard_brake_detection() {
        let mut db = DrivingBehavior::new();
        // Start at 80 km/h, then drop sharply (simulating emergency stop)
        db.update(80.0, 30.0);
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Drop to 0 in ~50ms → delta = -80 km/h = -22.2 m/s in 0.05s = -444 m/s²
        // This far exceeds the -2.8 m/s² threshold
        db.update(0.0, 0.0);

        assert!(
            db.hard_brake_count >= 1,
            "should detect a hard brake, got {}",
            db.hard_brake_count
        );
    }

    #[test]
    fn test_hard_brake_hysteresis() {
        let mut db = DrivingBehavior::new();
        // Trigger a hard brake
        db.update(80.0, 30.0);
        std::thread::sleep(std::time::Duration::from_millis(50));
        db.update(0.0, 0.0);
        assert_eq!(db.hard_brake_count, 1);

        // Still decelerating, but shouldn't count twice (hysteresis)
        std::thread::sleep(std::time::Duration::from_millis(50));
        db.update(0.0, 0.0);
        assert_eq!(
            db.hard_brake_count, 1,
            "hysteresis should prevent double-counting"
        );
    }

    #[test]
    fn test_jackrabbit_start_detection() {
        let mut db = DrivingBehavior::new();
        // Low speed, high throttle, rapid acceleration
        db.update(0.0, 70.0);
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Jump to 40 km/h in 50ms → 11.1 m/s in 0.05s = 222 m/s² (well above 2.8)
        db.update(40.0, 70.0);

        assert!(
            db.jackrabbit_count >= 1,
            "should detect jackrabbit start, got {}",
            db.jackrabbit_count
        );
    }

    #[test]
    fn test_no_jackrabbit_at_highway_speed() {
        let mut db = DrivingBehavior::new();
        // High speed (>50 km/h) — jackrabbit detection doesn't apply
        db.update(80.0, 70.0);
        std::thread::sleep(std::time::Duration::from_millis(50));
        db.update(120.0, 80.0);

        assert_eq!(
            db.jackrabbit_count, 0,
            "jackrabbit shouldn't trigger at highway speed"
        );
    }

    #[test]
    fn test_no_jackrabbit_at_low_throttle() {
        let mut db = DrivingBehavior::new();
        // Rapid acceleration from stop, but low throttle
        db.update(0.0, 20.0);
        std::thread::sleep(std::time::Duration::from_millis(50));
        db.update(40.0, 20.0);

        assert_eq!(
            db.jackrabbit_count, 0,
            "jackrabbit shouldn't trigger with low throttle"
        );
    }

    #[test]
    fn test_smoothness_label() {
        let mut db = DrivingBehavior::new();
        db.smoothness_score = 95.0;
        assert_eq!(db.smoothness_label(), "Smooth");

        db.smoothness_score = 75.0;
        assert_eq!(db.smoothness_label(), "Good");

        db.smoothness_score = 55.0;
        assert_eq!(db.smoothness_label(), "Fair");

        db.smoothness_score = 35.0;
        assert_eq!(db.smoothness_label(), "Rough");

        db.smoothness_score = 10.0;
        assert_eq!(db.smoothness_label(), "Aggressive");
    }

    #[test]
    fn test_accel_history_capacity() {
        let mut db = DrivingBehavior::new();
        // Feed more than ACCEL_HISTORY_CAP updates
        for i in 0..150 {
            db.update(i as f64, 30.0);
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        assert!(
            db.accel_history.len() <= ACCEL_HISTORY_CAP,
            "history should be capped at {}, got {}",
            ACCEL_HISTORY_CAP,
            db.accel_history.len()
        );
    }
}
