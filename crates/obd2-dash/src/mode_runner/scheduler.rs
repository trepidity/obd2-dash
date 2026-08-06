use super::capability::{CapabilityKey, CapabilityOutcome, CapabilitySet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    A,
    B,
    C,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ViewId {
    Gauges,
    Engine,
    Diagnostics,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestDescriptor {
    pub key: CapabilityKey,
    pub tier: Tier,
    pub every_cycles: u32,
    pub view: Option<ViewId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestKey(pub CapabilityKey);

#[derive(Debug, Clone, Default)]
pub struct Scheduler {
    descriptors: Vec<RequestDescriptor>,
}

impl Scheduler {
    pub fn new(mut descriptors: Vec<RequestDescriptor>) -> Self {
        descriptors.sort_by(|left, right| left.key.cmp(&right.key));
        Self { descriptors }
    }

    /// Build one deterministic wire plan. Normal tiers include only supported
    /// capabilities; at most one unverified capability enters as verifier work.
    pub fn plan_cycle(
        &self,
        cycle: u64,
        active_view: &ViewId,
        capabilities: &CapabilitySet,
    ) -> Vec<RequestKey> {
        let mut plan = Vec::new();
        for descriptor in &self.descriptors {
            if capabilities.outcome(&descriptor.key) != CapabilityOutcome::Supported {
                continue;
            }
            if descriptor.every_cycles == 0
                || !cycle.is_multiple_of(u64::from(descriptor.every_cycles))
                || !view_matches(descriptor.view.as_ref(), active_view)
            {
                continue;
            }
            plan.push(RequestKey(descriptor.key.clone()));
        }

        if let Some(descriptor) = self.descriptors.iter().find(|descriptor| {
            capabilities.outcome(&descriptor.key) == CapabilityOutcome::Unverified
                && view_matches(descriptor.view.as_ref(), active_view)
        }) {
            plan.push(RequestKey(descriptor.key.clone()));
        }
        plan
    }
}

fn view_matches(required: Option<&ViewId>, active: &ViewId) -> bool {
    required.is_none_or(|required| required == active)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode_runner::capability::CapabilityKind;

    fn descriptor(
        id: &str,
        tier: Tier,
        every_cycles: u32,
        view: Option<ViewId>,
    ) -> RequestDescriptor {
        RequestDescriptor {
            key: CapabilityKey::new(CapabilityKind::Pid, id, "broadcast"),
            tier,
            every_cycles,
            view,
        }
    }

    #[test]
    fn unsupported_and_unverified_do_not_join_normal_tiers() {
        let scheduler = Scheduler::new(vec![
            descriptor("010C", Tier::A, 1, None),
            descriptor("010D", Tier::A, 1, None),
        ]);
        let mut capabilities = CapabilitySet::default();
        capabilities.insert(
            CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast"),
            CapabilityOutcome::Unsupported,
        );
        capabilities.insert(
            CapabilityKey::new(CapabilityKind::Pid, "010D", "broadcast"),
            CapabilityOutcome::Unverified,
        );

        assert!(
            scheduler
                .plan_cycle(1, &ViewId::Gauges, &capabilities)
                .len()
                <= 1
        );
        assert_eq!(
            scheduler.plan_cycle(1, &ViewId::Gauges, &capabilities)[0]
                .0
                .request_id,
            "010D"
        );
    }

    #[test]
    fn tier_b_is_due_every_fifth_cycle() {
        let scheduler = Scheduler::new(vec![descriptor("0105", Tier::B, 5, None)]);
        let mut capabilities = CapabilitySet::default();
        capabilities.insert(
            CapabilityKey::new(CapabilityKind::Pid, "0105", "broadcast"),
            CapabilityOutcome::Supported,
        );
        assert!(scheduler
            .plan_cycle(4, &ViewId::Gauges, &capabilities)
            .is_empty());
        assert_eq!(
            scheduler
                .plan_cycle(5, &ViewId::Gauges, &capabilities)
                .len(),
            1
        );
    }

    #[test]
    fn view_state_gates_tier_c_and_verifier() {
        let scheduler = Scheduler::new(vec![descriptor(
            "lly.1543",
            Tier::C,
            1,
            Some(ViewId::Engine),
        )]);
        let mut capabilities = CapabilitySet::default();
        capabilities.insert(
            CapabilityKey::new(CapabilityKind::Pid, "lly.1543", "broadcast"),
            CapabilityOutcome::Unverified,
        );
        assert!(scheduler
            .plan_cycle(1, &ViewId::Gauges, &capabilities)
            .is_empty());
        assert_eq!(
            scheduler
                .plan_cycle(1, &ViewId::Engine, &capabilities)
                .len(),
            1
        );
    }
}
