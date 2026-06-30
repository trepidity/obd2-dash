use obd2_core::protocol::codec::BusFamily;
use obd2_core::protocol::dtc::{Dtc, DtcStatus};
use obd2_core::vehicle::{ModuleId, Protocol};

use crate::profiles::model::{
    ActiveTestDefinition, AddressState, AddressTemplate, BusDefinition, BusKey, Confidence,
    DecodedDtc, DecodedSignal, DiagnosticProfile, DtcServiceDefinition, EvidencePolicy,
    FailurePolicy, Manufacturer, MatchConfidence, ModuleDefinition, ModuleKey, ModuleMap,
    ModuleSafetyClass, PassiveMonitorDefinition, PollCadence, ProfileDecodeError, ProfileId,
    ProfileMatch, Provenance, RouteDefinition, RouteSet, SignalCategory, SignalDefinition,
    SourceFields, StandardPidOverride, StandardPidPolicy, VehicleContext,
};

pub const FIXTURE_PROFILE_ID: ProfileId = ProfileId::new("fixture.can11.readonly.v1");
pub const FIXTURE_VIN: &str = "0FICT1RE000000001";
pub const FIXTURE_SIGNAL_KEY: &str = "fixture.coolant_c";
pub const FIXTURE_DTC_KEY: &str = "fixture.dtc";

pub struct FixtureProfile;

pub static FIXTURE_PROFILE: FixtureProfile = FixtureProfile;

const CAN_BUS: BusKey = BusKey::new("can11-500");

const BUSES: &[BusDefinition] = &[BusDefinition {
    key: CAN_BUS,
    family: BusFamily::Can,
    protocol: Protocol::Can11Bit500,
    j1850: None,
    label: "Fixture CAN 11-bit 500k",
}];

const MODULES: &[ModuleDefinition] = &[ModuleDefinition {
    key: ModuleKey::Ecm,
    display_label: "Fixture Control Module",
    bus: CAN_BUS,
    address: AddressState::Confirmed(AddressTemplate::Can11 {
        request_id: 0x7E0,
        response_id: 0x7E8,
    }),
    safety_class: ModuleSafetyClass::Informational,
    coresident_with: None,
}];

pub const FIXTURE_MODULE_MAP: ModuleMap = ModuleMap {
    buses: BUSES,
    modules: MODULES,
};

const FIXTURE_SIGNALS: &[SignalDefinition] = &[SignalDefinition {
    key: FIXTURE_SIGNAL_KEY,
    label: "Fixture Coolant",
    category: SignalCategory::Powertrain,
    route: RouteDefinition {
        module: ModuleKey::Ecm,
    },
    service_id: 0x22,
    request_data: &[0xF0, 0x01],
    decoder_id: "fixture.scalar.u16_centi",
    unit: "C",
    cadence: PollCadence::Medium,
    confidence: Confidence::Verified,
    provenance: &[Provenance::LocalFixture],
    source_fields: SourceFields::NONE,
    evidence_policy: EvidencePolicy::BoundedLive,
    failure_policy: FailurePolicy::SurfaceUnavailable,
    preferred_over: None,
}];

const FIXTURE_DTC_SERVICES: &[DtcServiceDefinition] = &[DtcServiceDefinition {
    key: FIXTURE_DTC_KEY,
    label: "Fixture DTC",
    route_set: RouteSet::single(RouteDefinition {
        module: ModuleKey::Ecm,
    }),
    service_id: 0x19,
    request_data: &[0xFF, 0xFF, 0x00],
    decoder_id: "fixture.dtc.sae2byte",
    backoff_policy: crate::profiles::model::BackoffPolicy::NONE,
    cadence: PollCadence::Medium,
}];

impl DiagnosticProfile for FixtureProfile {
    fn id(&self) -> ProfileId {
        FIXTURE_PROFILE_ID
    }

    fn manufacturer(&self) -> Manufacturer {
        Manufacturer::Fixture
    }

    fn allowed_protocols(&self) -> &'static [Protocol] {
        &[Protocol::Can11Bit500]
    }

    fn module_map(&self) -> Option<&ModuleMap> {
        Some(&FIXTURE_MODULE_MAP)
    }

    fn matches(&self, ctx: &VehicleContext) -> ProfileMatch {
        if ctx.protocol == Protocol::Auto {
            return ProfileMatch::NoMatch;
        }
        if ctx.protocol != Protocol::Can11Bit500 {
            return ProfileMatch::NoMatch;
        }

        match ctx.vin.as_deref() {
            Some(FIXTURE_VIN) if ctx.vin_confidence.is_trusted() => ProfileMatch::Exact {
                confidence: MatchConfidence::VinExact,
            },
            Some(vin) if vin.starts_with("0FI") => ProfileMatch::Partial {
                reason: "fixture identity is present but VIN confidence is not trusted".into(),
            },
            _ => ProfileMatch::NoMatch,
        }
    }

    fn standard_pid_overrides(&self) -> &[StandardPidOverride] {
        &[]
    }

    fn standard_pid_policy(&self) -> StandardPidPolicy {
        StandardPidPolicy::EMPTY
    }

    fn signals(&self) -> &[SignalDefinition] {
        FIXTURE_SIGNALS
    }

    fn dtc_services(&self) -> &[DtcServiceDefinition] {
        FIXTURE_DTC_SERVICES
    }

    fn active_tests(&self) -> &[ActiveTestDefinition] {
        &[]
    }

    fn passive_monitors(&self) -> &[PassiveMonitorDefinition] {
        &[]
    }

    fn decode_signal(
        &self,
        signal: &SignalDefinition,
        payload: &[u8],
    ) -> Result<DecodedSignal, ProfileDecodeError> {
        match signal.decoder_id {
            "fixture.scalar.u16_centi" => decode_scalar_u16_centi(signal, payload),
            other => Err(ProfileDecodeError::UnknownDecoder(other)),
        }
    }

    fn decode_dtc_response(
        &self,
        service: &DtcServiceDefinition,
        payload: &[u8],
    ) -> Result<Vec<DecodedDtc>, ProfileDecodeError> {
        match service.decoder_id {
            "fixture.dtc.sae2byte" => decode_sae_pairs(payload),
            other => Err(ProfileDecodeError::UnknownDecoder(other)),
        }
    }
}

fn decode_scalar_u16_centi(
    signal: &SignalDefinition,
    payload: &[u8],
) -> Result<DecodedSignal, ProfileDecodeError> {
    if payload.len() > 2 {
        return Err(ProfileDecodeError::MismatchedResponse);
    }
    let raw = payload
        .get(0..2)
        .ok_or(ProfileDecodeError::PayloadTooShort {
            expected: 2,
            got: payload.len(),
        })?;
    let value = f64::from(u16::from_be_bytes([raw[0], raw[1]])) / 100.0;
    Ok(DecodedSignal {
        key: signal.key,
        value,
        unit: signal.unit,
        raw: payload.to_vec(),
        selected_raw: raw.to_vec(),
        module: ModuleId::new(ModuleKey::Ecm.canonical()),
        confidence: signal.confidence,
    })
}

fn decode_sae_pairs(payload: &[u8]) -> Result<Vec<DecodedDtc>, ProfileDecodeError> {
    if payload.len() % 2 != 0 {
        return Err(ProfileDecodeError::PayloadTooShort {
            expected: payload.len() + 1,
            got: payload.len(),
        });
    }

    let mut out = Vec::new();
    for pair in payload.chunks_exact(2) {
        if pair == [0, 0] {
            continue;
        }
        let dtc = Dtc::from_bytes(pair[0], pair[1]);
        out.push(DecodedDtc {
            code: dtc.code,
            status: DtcStatus::Stored,
            status_raw: None,
            status_flags: Vec::new(),
            raw: pair.to_vec(),
            module: Some(ModuleId::new(ModuleKey::Ecm.canonical())),
            notes: Some("fixture sae2byte".to_string()),
        });
    }
    Ok(out)
}

pub fn profile_tabs(profile: &dyn DiagnosticProfile) -> Vec<SignalCategory> {
    let mut out = Vec::new();
    for signal in profile.signals() {
        if !out.contains(&signal.category) {
            out.push(signal.category);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(protocol: Protocol, vin: Option<&str>, trusted: bool) -> VehicleContext {
        VehicleContext {
            generation: 1,
            protocol,
            vin: vin.map(str::to_string),
            vin_confidence: if trusted {
                crate::profiles::IdentityConfidence::Confirmed
            } else {
                crate::profiles::IdentityConfidence::Corrupted
            },
            spec: None,
            discovered_modules: Vec::new(),
            active_bus: None,
        }
    }

    #[test]
    fn fixture_decode_scalar_roundtrip() {
        let decoded = FIXTURE_PROFILE
            .decode_signal(&FIXTURE_SIGNALS[0], &[0x13, 0x88])
            .unwrap();

        assert_eq!(decoded.value, 50.0);
        assert_eq!(decoded.unit, "C");
        assert_eq!(decoded.selected_raw, vec![0x13, 0x88]);
    }

    #[test]
    fn fixture_decode_short_payload_errors() {
        let err = FIXTURE_PROFILE
            .decode_signal(&FIXTURE_SIGNALS[0], &[0x13])
            .unwrap_err();

        assert_eq!(
            err,
            ProfileDecodeError::PayloadTooShort {
                expected: 2,
                got: 1
            }
        );
    }

    #[test]
    fn fixture_decode_dtc_pairs() {
        let decoded = FIXTURE_PROFILE
            .decode_dtc_response(&FIXTURE_DTC_SERVICES[0], &[0x01, 0x23, 0x00, 0x00])
            .unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].code, "P0123");
    }

    #[test]
    fn fixture_match_exact_only_for_synthetic_identity() {
        assert!(matches!(
            FIXTURE_PROFILE.matches(&context(Protocol::Can11Bit500, Some(FIXTURE_VIN), true)),
            ProfileMatch::Exact {
                confidence: MatchConfidence::VinExact
            }
        ));
        assert_eq!(
            FIXTURE_PROFILE.matches(&context(Protocol::J1850Vpw, Some(FIXTURE_VIN), true)),
            ProfileMatch::NoMatch
        );
        assert_eq!(
            FIXTURE_PROFILE.matches(&context(
                Protocol::Can11Bit500,
                Some("1GCHK23224F000001"),
                true
            )),
            ProfileMatch::NoMatch
        );
        assert_eq!(
            FIXTURE_PROFILE.matches(&context(Protocol::Auto, Some(FIXTURE_VIN), true)),
            ProfileMatch::NoMatch
        );
    }

    #[test]
    fn fixture_match_floor_rejects_corrupted_vin() {
        let result =
            FIXTURE_PROFILE.matches(&context(Protocol::Can11Bit500, Some(FIXTURE_VIN), false));

        assert!(matches!(result, ProfileMatch::Partial { .. }));
    }

    #[test]
    fn fixture_has_no_active_tests() {
        assert!(FIXTURE_PROFILE.active_tests().is_empty());
    }
}
