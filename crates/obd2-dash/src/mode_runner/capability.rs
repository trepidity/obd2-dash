use std::collections::BTreeMap;

use obd2_core::vehicle::Protocol;
pub use obd2_db::models::{CapabilityKind, CapabilityOutcome};

/// Stable persistence token for a negotiated protocol (spec §8.1). Cache
/// context rows must never carry `Debug` or display formatting; these tokens
/// are a compatibility contract with existing `vehicle_capability_sets` rows.
/// K-line init flavor is deliberately collapsed — it does not change which
/// requests a vehicle supports.
pub fn protocol_token(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::J1850Vpw => "j1850_vpw",
        Protocol::J1850Pwm => "j1850_pwm",
        Protocol::Iso9141(_) => "iso9141",
        Protocol::Kwp2000(_) => "kwp2000",
        Protocol::Can11Bit500 => "can_11bit_500",
        Protocol::Can11Bit250 => "can_11bit_250",
        Protocol::Can29Bit500 => "can_29bit_500",
        Protocol::Can29Bit250 => "can_29bit_250",
        Protocol::Auto => "auto",
        // Core marks Protocol non-exhaustive. A future variant lands here and
        // simply mismatches stored contexts (structural invalidation) until a
        // real token is added — never a Debug string.
        _ => "unknown",
    }
}

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

    #[test]
    fn protocol_tokens_are_stable_and_never_debug_formatted() {
        use obd2_core::vehicle::KLineInit;
        let expected = [
            (Protocol::J1850Vpw, "j1850_vpw"),
            (Protocol::J1850Pwm, "j1850_pwm"),
            (Protocol::Iso9141(KLineInit::SlowInit), "iso9141"),
            (Protocol::Kwp2000(KLineInit::FastInit), "kwp2000"),
            (Protocol::Can11Bit500, "can_11bit_500"),
            (Protocol::Can11Bit250, "can_11bit_250"),
            (Protocol::Can29Bit500, "can_29bit_500"),
            (Protocol::Can29Bit250, "can_29bit_250"),
            (Protocol::Auto, "auto"),
        ];
        for (protocol, token) in expected {
            assert_eq!(protocol_token(protocol), token);
            assert_ne!(protocol_token(protocol), format!("{protocol:?}"));
        }
        // K-line init flavor never changes the persisted context token.
        assert_eq!(
            protocol_token(Protocol::Iso9141(KLineInit::FastInit)),
            protocol_token(Protocol::Iso9141(KLineInit::SlowInit)),
        );
    }
}
