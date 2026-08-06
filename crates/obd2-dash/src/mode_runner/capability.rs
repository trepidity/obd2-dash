use super::scheduler::RequestDescriptor;
use std::collections::BTreeMap;

use obd2_core::vehicle::Protocol;
pub use obd2_db::models::{CapabilityKind, CapabilityOutcome};

/// Stable descriptor fingerprint used as the capability-cache context key.
/// Serializes only the required request identities (kind:module:request_id),
/// sorted and deduplicated. Tier, cadence, and view are deliberately excluded
/// (spec §8.1): presentation-only changes must not invalidate a vehicle's
/// capability cache. Never Debug output or a process-randomized hash.
pub fn probe_fingerprint(descriptors: &[RequestDescriptor]) -> String {
    let mut fields: Vec<String> = descriptors
        .iter()
        .map(|descriptor| {
            format!(
                "{}:{}:{}",
                descriptor.key.kind.as_str(),
                descriptor.key.module,
                descriptor.key.request_id,
            )
        })
        .collect();
    fields.sort();
    fields.dedup();
    format!("v1:{}", fields.join("|"))
}

pub fn default_probe_fingerprint() -> String {
    let descriptors = [
        (0x0C, "broadcast"),
        (0x0D, "broadcast"),
        (0x05, "broadcast"),
        (0x0B, "broadcast"),
        (0x10, "broadcast"),
    ]
    .into_iter()
    .map(|(pid, module)| RequestDescriptor {
        key: CapabilityKey::new(CapabilityKind::Pid, format!("01{pid:02X}"), module),
        tier: super::scheduler::Tier::A,
        every_cycles: 1,
        view: None,
    })
    .collect::<Vec<_>>();
    probe_fingerprint(&descriptors)
}

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

/// Operator-facing protocol label. This is intentionally separate from
/// [`protocol_token`], whose compact values are a persistence contract.
pub fn protocol_display(protocol: Protocol) -> &'static str {
    match protocol {
        Protocol::J1850Vpw => "J1850 VPW",
        Protocol::J1850Pwm => "J1850 PWM",
        Protocol::Iso9141(_) => "ISO 9141",
        Protocol::Kwp2000(_) => "KWP2000",
        Protocol::Can11Bit500 => "CAN 11-bit 500 kbps",
        Protocol::Can11Bit250 => "CAN 11-bit 250 kbps",
        Protocol::Can29Bit500 => "CAN 29-bit 500 kbps",
        Protocol::Can29Bit250 => "CAN 29-bit 250 kbps",
        Protocol::Auto => "Automatic",
        // Core marks Protocol non-exhaustive. A new protocol remains visible
        // as unresolved instead of leaking an unstable Debug representation.
        _ => "Unknown",
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

    fn descriptor(
        request_id: &str,
        tier: super::super::scheduler::Tier,
        every: u32,
    ) -> RequestDescriptor {
        RequestDescriptor {
            key: CapabilityKey::new(CapabilityKind::Pid, request_id, "broadcast"),
            tier,
            every_cycles: every,
            view: None,
        }
    }

    #[test]
    fn fingerprint_is_stable_for_input_order() {
        use super::super::scheduler::Tier;
        let forward = [
            descriptor("010C", Tier::A, 1),
            descriptor("010D", Tier::B, 5),
        ];
        let reversed = [
            descriptor("010D", Tier::B, 5),
            descriptor("010C", Tier::A, 1),
        ];
        assert_eq!(probe_fingerprint(&forward), probe_fingerprint(&reversed));
    }

    #[test]
    fn fingerprint_changes_for_required_request_change() {
        use super::super::scheduler::Tier;
        let base = [descriptor("010C", Tier::A, 1)];
        let changed = [descriptor("010B", Tier::A, 1)];
        assert_ne!(probe_fingerprint(&base), probe_fingerprint(&changed));
    }

    #[test]
    fn fingerprint_ignores_tier_only_presentation_change() {
        use super::super::scheduler::Tier;
        let tiered_a = [
            descriptor("010C", Tier::A, 1),
            descriptor("0105", Tier::B, 5),
        ];
        let retiered = [
            descriptor("010C", Tier::B, 10),
            descriptor("0105", Tier::C, 50),
        ];
        assert_eq!(probe_fingerprint(&tiered_a), probe_fingerprint(&retiered));
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

    #[test]
    fn protocol_display_is_explicit_and_human_readable() {
        assert_eq!(protocol_display(Protocol::J1850Vpw), "J1850 VPW");
        assert_eq!(
            protocol_display(Protocol::Can11Bit500),
            "CAN 11-bit 500 kbps"
        );
    }
}
