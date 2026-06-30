use std::sync::atomic::{AtomicU64, Ordering};

use obd2_core::adapter::Adapter;
use obd2_core::session::Session;

use super::model::{IdentityConfidence, ProfileId, SelectedProfile, VehicleContext};
use super::registry::ProfileRegistry;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartialProfileMatch {
    pub profile_id: ProfileId,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileAmbiguity {
    pub profile_ids: Vec<ProfileId>,
    pub evidence: String,
}

#[derive(Debug, Default)]
pub struct ProfileState {
    pub generation: u64,
    pub vin_confidence: Option<IdentityConfidence>,
    pub selected: Option<SelectedProfile>,
    pub exact_matches: Vec<ProfileId>,
    pub partial_matches: Vec<PartialProfileMatch>,
    pub ambiguity: Option<ProfileAmbiguity>,
}

impl ProfileState {
    pub fn snapshot(&self) -> ProfileStateSnapshot {
        ProfileStateSnapshot {
            generation: self.generation,
            selected_profile_id: self
                .selected
                .as_ref()
                .map(|selected| selected.profile_id().as_str()),
            manual_confirmed: self
                .selected
                .as_ref()
                .is_some_and(SelectedProfile::manual_confirmed),
            vin_confidence: self.vin_confidence,
            exact_matches: self.exact_matches.iter().map(ProfileId::as_str).collect(),
            partial_matches: self
                .partial_matches
                .iter()
                .map(|entry| (entry.profile_id.as_str(), entry.reason.clone()))
                .collect(),
            ambiguity: self.ambiguity.as_ref().map(|ambiguity| {
                ambiguity
                    .profile_ids
                    .iter()
                    .map(ProfileId::as_str)
                    .collect()
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileStateSnapshot {
    pub generation: u64,
    pub selected_profile_id: Option<&'static str>,
    pub manual_confirmed: bool,
    pub vin_confidence: Option<IdentityConfidence>,
    pub exact_matches: Vec<&'static str>,
    pub partial_matches: Vec<(&'static str, String)>,
    pub ambiguity: Option<Vec<&'static str>>,
}

static GENERATION: AtomicU64 = AtomicU64::new(1);

pub fn next_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::SeqCst)
}

pub fn validate_vin_charset(vin: &str) -> bool {
    vin.len() == 17
        && vin.bytes().all(
            |byte| matches!(byte, b'A'..=b'H' | b'J'..=b'N' | b'P' | b'R'..=b'Z' | b'0'..=b'9'),
        )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityOutcome {
    pub vin: Option<String>,
    pub confidence: IdentityConfidence,
}

pub async fn acquire_identity<A: Adapter>(
    session: &mut Session<A>,
    extra_reads: u8,
) -> IdentityOutcome {
    let mut accepted_vin = match session.identify_vehicle().await {
        Ok(profile) => Some(profile.vin),
        Err(err) => {
            tracing::warn!("vehicle identity read failed: {err}");
            None
        }
    };
    let mut confirmed = false;

    for _ in 0..extra_reads {
        match session.read_vin().await {
            Ok(vin) => match accepted_vin.as_ref() {
                Some(accepted) if accepted == &vin && validate_vin_charset(accepted) => {
                    confirmed = true;
                }
                None => {
                    accepted_vin = Some(vin);
                }
                Some(_) => {}
            },
            Err(err) => {
                tracing::debug!("corroborating VIN read failed: {err}");
            }
        }
    }

    let Some(vin) = accepted_vin else {
        return IdentityOutcome {
            vin: None,
            confidence: IdentityConfidence::Unread,
        };
    };

    let confidence = if !validate_vin_charset(&vin) {
        IdentityConfidence::Corrupted
    } else if confirmed {
        IdentityConfidence::Confirmed
    } else {
        IdentityConfidence::Single
    };

    IdentityOutcome {
        vin: Some(vin),
        confidence,
    }
}

pub fn build_vehicle_context<A: Adapter>(
    session: &Session<A>,
    generation: u64,
    identity: &IdentityOutcome,
) -> VehicleContext {
    let discovery = session.discovery();
    let protocol = discovery
        .map(|discovery| discovery.selected_protocol)
        .unwrap_or_else(|| session.adapter_info().protocol);
    let mut discovered_modules = discovery
        .map(|discovery| discovery.modules.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    discovered_modules.sort_by(|left, right| left.0.cmp(&right.0));
    let active_bus = discovery
        .and_then(|discovery| discovery.active_bus.as_ref())
        .map(|bus| bus.id.0.clone());

    VehicleContext {
        generation,
        protocol,
        vin: identity.vin.clone(),
        vin_confidence: identity.confidence,
        spec: session.spec().cloned(),
        discovered_modules,
        active_bus,
    }
}

pub fn select_into_state(registry: &ProfileRegistry, ctx: &VehicleContext) -> ProfileState {
    registry.select(ctx)
}

pub fn confidence_warning(confidence: IdentityConfidence) -> Option<String> {
    if confidence.is_trusted() {
        return None;
    }

    let label = match confidence {
        IdentityConfidence::Unread => "unread",
        IdentityConfidence::Corrupted => "corrupted",
        IdentityConfidence::Single => "single-read",
        IdentityConfidence::Confirmed => return None,
    };
    Some(format!(
        "VIN identity confidence is {label}; manufacturer profile selection remains unconfirmed"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vin_charset_rejects_illegal_letters() {
        assert!(validate_vin_charset("1GTHK29294E391526"));
        assert!(!validate_vin_charset("1GTHK29294E39152"));
        assert!(!validate_vin_charset("1GTHK29294E3915267"));
        assert!(!validate_vin_charset("1GTHK2I294E391526"));
        assert!(!validate_vin_charset("1GTHK2O294E391526"));
        assert!(!validate_vin_charset("1GTHK2Q294E391526"));
        assert!(!validate_vin_charset("1GTHK29294E39152é"));
    }

    #[test]
    fn next_generation_is_strictly_monotonic() {
        let first = next_generation();
        let second = next_generation();

        assert!(second > first);
    }
}
