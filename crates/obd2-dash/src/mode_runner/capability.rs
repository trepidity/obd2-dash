use std::collections::BTreeMap;

pub use obd2_db::models::{CapabilityKind, CapabilityOutcome};

/// Stable capability namespace persisted by the runner.
/// A capability's stable request identity. `module` is never optional.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityKey {
    pub kind: CapabilityKind,
    pub request_id: String,
    pub module: String,
}

impl CapabilityKey {
    pub fn new(
        kind: CapabilityKind,
        request_id: impl Into<String>,
        module: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            request_id: request_id.into(),
            module: module.into(),
        }
    }
}

/// In-memory capability map. The ordered map makes fingerprints and tests
/// deterministic without requiring a clone of an unordered hash map.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilitySet {
    outcomes: BTreeMap<CapabilityKey, CapabilityOutcome>,
}

impl CapabilitySet {
    pub fn insert(&mut self, key: CapabilityKey, outcome: CapabilityOutcome) {
        self.outcomes.insert(key, outcome);
    }

    pub fn outcome(&self, key: &CapabilityKey) -> CapabilityOutcome {
        self.outcomes
            .get(key)
            .copied()
            .unwrap_or(CapabilityOutcome::Unverified)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&CapabilityKey, CapabilityOutcome)> {
        self.outcomes.iter().map(|(key, outcome)| (key, *outcome))
    }

    pub fn len(&self) -> usize {
        self.outcomes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.outcomes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_unsupported_is_pruned() {
        assert!(matches!(
            CapabilityOutcome::Unsupported,
            CapabilityOutcome::Unsupported
        ));
        assert!(!matches!(
            CapabilityOutcome::Unverified,
            CapabilityOutcome::Unsupported
        ));
        assert!(!matches!(
            CapabilityOutcome::Supported,
            CapabilityOutcome::Unsupported
        ));
    }

    #[test]
    fn missing_capabilities_are_unverified() {
        let set = CapabilitySet::default();
        let key = CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast");
        assert_eq!(set.outcome(&key), CapabilityOutcome::Unverified);
    }
}
