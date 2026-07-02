use std::fmt;

use obd2_core::vehicle::Protocol;

#[cfg(any(test, feature = "proof-profile"))]
use super::fixture::FIXTURE_PROFILE;
use super::gm::GM_LLY_CLASS2_PROFILE;
use super::model::{
    DiagnosticProfile, IdentityConfidence, ProfileId, ProfileMatch, SelectedProfile, VehicleContext,
};
use super::selection::{PartialProfileMatch, ProfileAmbiguity, ProfileState};

pub struct ProfileRegistry {
    profiles: Vec<&'static dyn DiagnosticProfile>,
}

impl ProfileRegistry {
    pub fn new() -> Self {
        Self {
            profiles: Vec::new(),
        }
    }

    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(&GM_LLY_CLASS2_PROFILE);
        #[cfg(any(test, feature = "proof-profile"))]
        registry.register(&FIXTURE_PROFILE);
        registry
    }

    pub fn register(&mut self, profile: &'static dyn DiagnosticProfile) {
        self.profiles.push(profile);
    }

    pub fn profiles(&self) -> &[&'static dyn DiagnosticProfile] {
        &self.profiles
    }

    pub fn get(&self, id: ProfileId) -> Option<&dyn DiagnosticProfile> {
        self.profiles
            .iter()
            .find(|profile| profile.id() == id)
            .copied()
    }

    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }

    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    pub fn select(&self, ctx: &VehicleContext) -> ProfileState {
        let mut exact_matches = Vec::new();
        let mut partial_matches = Vec::new();

        for profile in &self.profiles {
            match profile.matches(ctx) {
                ProfileMatch::Exact { .. } => {
                    if let Some(reason) = exact_floor_failure(*profile, ctx) {
                        tracing::warn!(
                            profile_id = profile.id().as_str(),
                            reason,
                            "profile Exact downgraded by framework floor"
                        );
                        partial_matches.push(PartialProfileMatch {
                            profile_id: profile.id(),
                            reason,
                        });
                    } else {
                        exact_matches.push(profile.id());
                    }
                }
                ProfileMatch::Partial { reason } => {
                    partial_matches.push(PartialProfileMatch {
                        profile_id: profile.id(),
                        reason,
                    });
                }
                ProfileMatch::NoMatch => {}
            }
        }

        let ambiguity = if exact_matches.len() > 1 {
            let evidence = exact_matches
                .iter()
                .map(ProfileId::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            tracing::warn!(
                profile_ids = evidence.as_str(),
                generation = ctx.generation,
                "ambiguous profile selection"
            );
            Some(ProfileAmbiguity {
                profile_ids: exact_matches.clone(),
                evidence,
            })
        } else {
            None
        };

        let selected = if exact_matches.len() == 1 && ambiguity.is_none() {
            Some(SelectedProfile::seal(
                exact_matches[0],
                ctx.generation,
                false,
            ))
        } else {
            None
        };

        ProfileState {
            generation: ctx.generation,
            vin_confidence: Some(ctx.vin_confidence),
            selected,
            exact_matches,
            partial_matches,
            ambiguity,
        }
    }

    pub fn confirm_manual(
        &self,
        ctx: &VehicleContext,
        profile_id: ProfileId,
    ) -> Result<SelectedProfile, ManualConfirmError> {
        let profile = self
            .get(profile_id)
            .ok_or(ManualConfirmError::NotRegistered(profile_id))?;

        match profile.matches(ctx) {
            ProfileMatch::NoMatch => Err(ManualConfirmError::NotEvenPartial(profile_id)),
            ProfileMatch::Exact { .. } | ProfileMatch::Partial { .. } => {
                Ok(SelectedProfile::seal(profile_id, ctx.generation, true))
            }
        }
    }
}

pub fn builtin_profile(id: ProfileId) -> Option<&'static dyn DiagnosticProfile> {
    if id == GM_LLY_CLASS2_PROFILE.id() {
        return Some(&GM_LLY_CLASS2_PROFILE);
    }
    #[cfg(any(test, feature = "proof-profile"))]
    if id == FIXTURE_PROFILE.id() {
        return Some(&FIXTURE_PROFILE);
    }
    None
}

impl Default for ProfileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ManualConfirmError {
    NotRegistered(ProfileId),
    NotEvenPartial(ProfileId),
}

impl fmt::Display for ManualConfirmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ManualConfirmError::NotRegistered(id) => {
                write!(f, "profile {:?} is not registered", id)
            }
            ManualConfirmError::NotEvenPartial(id) => {
                write!(
                    f,
                    "profile {:?} returned NoMatch; manual confirm requires at least Partial",
                    id
                )
            }
        }
    }
}

impl std::error::Error for ManualConfirmError {}

fn exact_floor_failure(
    profile: &'static dyn DiagnosticProfile,
    ctx: &VehicleContext,
) -> Option<String> {
    if ctx.protocol == Protocol::Auto {
        return Some("Protocol::Auto cannot produce an Exact profile match".into());
    }
    if !profile.allowed_protocols().contains(&ctx.protocol) {
        return Some(format!(
            "protocol {:?} is not declared by profile",
            ctx.protocol
        ));
    }
    if !has_vin_identity_dimension(ctx) {
        return Some("no VIN-derived identity dimension is present".into());
    }
    if ctx.vin_confidence != IdentityConfidence::Confirmed {
        return Some(format!(
            "VIN confidence {:?} is below Confirmed",
            ctx.vin_confidence
        ));
    }
    if profile.module_map().is_some_and(|module_map| {
        module_map
            .buses
            .iter()
            .all(|bus| bus.protocol != ctx.protocol)
    }) {
        return Some("profile module map has no bus for selected protocol".into());
    }
    None
}

fn has_vin_identity_dimension(ctx: &VehicleContext) -> bool {
    ctx.vin.as_deref().is_some_and(|vin| {
        let vin = vin.trim();
        vin.len() == 17 && vin.is_ascii() && vin.as_bytes().get(7).is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::model::{
        ActiveTestDefinition, DecodedDtc, DecodedSignal, DtcServiceDefinition, Manufacturer,
        PassiveMonitorDefinition, ProfileDecodeError, ProfileMatch, SignalDefinition,
        StandardPidOverride,
    };

    struct TestProfile;

    impl DiagnosticProfile for TestProfile {
        fn id(&self) -> ProfileId {
            ProfileId::new("test.fixture")
        }

        fn manufacturer(&self) -> Manufacturer {
            Manufacturer::Generic
        }

        fn matches(&self, _ctx: &VehicleContext) -> ProfileMatch {
            ProfileMatch::NoMatch
        }

        fn standard_pid_overrides(&self) -> &[StandardPidOverride] {
            &[]
        }

        fn signals(&self) -> &[SignalDefinition] {
            &[]
        }

        fn dtc_services(&self) -> &[DtcServiceDefinition] {
            &[]
        }

        fn active_tests(&self) -> &[ActiveTestDefinition] {
            &[]
        }

        fn passive_monitors(&self) -> &[PassiveMonitorDefinition] {
            &[]
        }

        fn decode_signal(
            &self,
            _signal: &SignalDefinition,
            _payload: &[u8],
        ) -> Result<DecodedSignal, ProfileDecodeError> {
            Err(ProfileDecodeError::Decode("test".into()))
        }

        fn decode_dtc_response(
            &self,
            _service: &DtcServiceDefinition,
            _payload: &[u8],
        ) -> Result<Vec<DecodedDtc>, ProfileDecodeError> {
            Err(ProfileDecodeError::Decode("test".into()))
        }
    }

    static TEST_PROFILE: TestProfile = TestProfile;

    #[test]
    fn empty_registry_has_no_profiles() {
        let registry = ProfileRegistry::new();

        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(registry.profiles().is_empty());
    }

    #[test]
    fn register_increments_len_and_get_finds_by_id() {
        let mut registry = ProfileRegistry::new();

        registry.register(&TEST_PROFILE);

        assert_eq!(registry.len(), 1);
        assert!(registry.get(ProfileId::new("test.fixture")).is_some());
    }

    #[test]
    fn get_returns_none_for_unknown_id() {
        let mut registry = ProfileRegistry::new();

        registry.register(&TEST_PROFILE);

        assert!(registry.get(ProfileId::new("nope")).is_none());
    }
}
