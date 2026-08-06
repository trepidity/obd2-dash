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
