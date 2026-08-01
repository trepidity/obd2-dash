use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::capability::{CapabilityKey, CapabilityOutcome};
use super::scheduler::{Tier, ViewId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeError {
    Unsupported,
    UnsupportedPid,
    ExplicitUnsupported,
    NoData,
    Timeout,
    Transport,
    Decode,
    Stale,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationResult {
    pub outcome: CapabilityOutcome,
    pub error_code: Option<&'static str>,
    pub retry_after: Option<Duration>,
}

#[derive(Debug, Clone)]
struct Entry {
    tier: Tier,
    view: Option<ViewId>,
    attempts: u8,
    no_data: u8,
    next_due: Instant,
    outcome: CapabilityOutcome,
}

#[derive(Debug, Clone, Default)]
pub struct Verifier {
    entries: BTreeMap<CapabilityKey, Entry>,
}

impl Verifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: CapabilityKey, tier: Tier, view: Option<ViewId>) {
        self.entries.insert(
            key,
            Entry {
                tier,
                view,
                attempts: 0,
                no_data: 0,
                next_due: Instant::now(),
                outcome: CapabilityOutcome::Unverified,
            },
        );
    }

    pub fn outcome(&self, key: &CapabilityKey) -> CapabilityOutcome {
        self.entries
            .get(key)
            .map_or(CapabilityOutcome::Unverified, |e| e.outcome)
    }

    pub fn remaining(&self, active_view: &ViewId) -> usize {
        self.entries
            .values()
            .filter(|e| e.outcome == CapabilityOutcome::Unverified && eligible(e, active_view))
            .count()
    }

    /// Entries that can still change outcome this session: unverified,
    /// eligible, and under the retry budget. When this reaches zero the
    /// verification pass is complete — `remaining()` may still be non-zero if
    /// exhausted entries stay unverified (the Degraded case).
    pub fn unresolved(&self, active_view: &ViewId) -> usize {
        self.entries
            .values()
            .filter(|e| {
                e.outcome == CapabilityOutcome::Unverified
                    && e.attempts < 3
                    && eligible(e, active_view)
            })
            .count()
    }

    /// Return at most one due request. Tier A/B always precede visible Tier C.
    pub fn next(&self, now: Instant, active_view: &ViewId) -> Option<&CapabilityKey> {
        self.entries
            .iter()
            .filter(|(_, e)| {
                e.outcome == CapabilityOutcome::Unverified
                    && e.attempts < 3
                    && e.next_due <= now
                    && eligible(e, active_view)
            })
            .min_by_key(|(key, e)| (tier_rank(e.tier), (*key).clone()))
            .map(|(key, _)| key)
    }

    pub fn classify(
        &mut self,
        key: &CapabilityKey,
        result: Result<(), ProbeError>,
        now: Instant,
    ) -> Option<VerificationResult> {
        let entry = self.entries.get_mut(key)?;
        entry.attempts = entry.attempts.saturating_add(1);
        let classified = match result {
            Ok(()) => VerificationResult {
                outcome: CapabilityOutcome::Supported,
                error_code: None,
                retry_after: None,
            },
            Err(
                ProbeError::Unsupported
                | ProbeError::UnsupportedPid
                | ProbeError::ExplicitUnsupported,
            ) => VerificationResult {
                outcome: CapabilityOutcome::Unsupported,
                error_code: Some("unsupported"),
                retry_after: None,
            },
            Err(ProbeError::NoData) => {
                entry.no_data = entry.no_data.saturating_add(1);
                if entry.no_data >= 2 {
                    VerificationResult {
                        outcome: CapabilityOutcome::Unsupported,
                        error_code: Some("no_data"),
                        retry_after: None,
                    }
                } else {
                    let d = backoff(entry.attempts);
                    VerificationResult {
                        outcome: CapabilityOutcome::Unverified,
                        error_code: Some("no_data"),
                        retry_after: Some(d),
                    }
                }
            }
            Err(error) => {
                let code = error_code(error);
                VerificationResult {
                    outcome: CapabilityOutcome::Unverified,
                    error_code: Some(code),
                    retry_after: Some(backoff(entry.attempts)),
                }
            }
        };
        entry.outcome = classified.outcome;
        if let Some(delay) = classified.retry_after {
            entry.next_due = now + delay;
        }
        Some(classified)
    }

    pub fn attempts(&self, key: &CapabilityKey) -> u8 {
        self.entries.get(key).map_or(0, |e| e.attempts)
    }

    /// Whether the verifier already tracks this key. Used on reconnect resume
    /// so re-staging does not reset a preserved entry's attempt counters.
    pub fn contains(&self, key: &CapabilityKey) -> bool {
        self.entries.contains_key(key)
    }

    pub fn exhausted(&self, key: &CapabilityKey) -> bool {
        self.entries
            .get(key)
            .is_some_and(|entry| entry.attempts >= 3)
    }
}

fn eligible(entry: &Entry, active: &ViewId) -> bool {
    entry.tier != Tier::C || entry.view.as_ref().is_none_or(|view| view == active)
}
fn tier_rank(tier: Tier) -> u8 {
    match tier {
        Tier::A => 0,
        Tier::B => 1,
        Tier::C => 2,
    }
}
fn backoff(attempt: u8) -> Duration {
    Duration::from_millis(250u64.saturating_mul(1u64 << attempt.min(4)))
}
fn error_code(error: ProbeError) -> &'static str {
    match error {
        ProbeError::Timeout => "timeout",
        ProbeError::Transport => "transport",
        ProbeError::Decode => "decode",
        ProbeError::Stale => "stale",
        ProbeError::Other => "error",
        _ => "unsupported",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use obd2_db::models::CapabilityKind;
    fn key(id: &str) -> CapabilityKey {
        CapabilityKey::new(CapabilityKind::Pid, id, "broadcast")
    }
    #[test]
    fn no_data_requires_separated_confirmation() {
        let mut v = Verifier::new();
        let k = key("010C");
        v.insert(k.clone(), Tier::A, None);
        let now = Instant::now();
        assert_eq!(
            v.classify(&k, Err(ProbeError::NoData), now)
                .unwrap()
                .outcome,
            CapabilityOutcome::Unverified
        );
        assert_eq!(
            v.classify(&k, Err(ProbeError::NoData), now + Duration::from_secs(1))
                .unwrap()
                .outcome,
            CapabilityOutcome::Unsupported
        );
    }
    #[test]
    fn ordering_and_view_gate_are_deterministic() {
        let mut v = Verifier::new();
        let c = key("c");
        let a = key("a");
        v.insert(c, Tier::C, Some(ViewId::Engine));
        v.insert(a.clone(), Tier::A, None);
        assert_eq!(v.next(Instant::now(), &ViewId::Gauges), Some(&a));
        assert_eq!(v.remaining(&ViewId::Gauges), 1);
    }
}
