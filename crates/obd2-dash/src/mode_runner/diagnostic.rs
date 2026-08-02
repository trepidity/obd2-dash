use super::snapshot::ModeState;
use obd2_core::vehicle::Protocol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuelClass {
    Gasoline,
    Diesel,
    Other,
    Unknown,
}

/// A raw fuel label is one of three things: a recognized class, an explicit
/// "no claim" (the embedded generic spec ships `fuel_type: unknown`, and a
/// blank field means the same), or an unrecognized claim — a vocabulary gap.
enum FuelLabel {
    Class(FuelClass),
    Absent,
    Unrecognized,
}

/// Resolve fuel using the approved precedence: embedded session identity,
/// then exact-VIN database data, otherwise Unknown.
///
/// Precedence is by SOURCE, not by value: if the curated embedded spec makes
/// a claim we cannot parse, the answer is Unknown — the cached NHTSA row is
/// never consulted to overrule the authoritative source. Only an explicit
/// "unknown"/blank sentinel (spec makes no claim) falls through to the
/// database. Unrecognized labels never resolve to gasoline.
pub fn resolve_fuel(session: Option<&str>, database: Option<&str>) -> FuelClass {
    match session.map(classify_fuel_label) {
        Some(FuelLabel::Class(class)) => class,
        Some(FuelLabel::Unrecognized) => FuelClass::Unknown,
        Some(FuelLabel::Absent) | None => match database.map(classify_fuel_label) {
            Some(FuelLabel::Class(class)) => class,
            _ => FuelClass::Unknown,
        },
    }
}

fn classify_fuel_label(raw: &str) -> FuelLabel {
    match raw.trim().to_ascii_lowercase().as_str() {
        "gasoline" => FuelLabel::Class(FuelClass::Gasoline),
        "diesel" => FuelLabel::Class(FuelClass::Diesel),
        "other" => FuelLabel::Class(FuelClass::Other),
        "" | "unknown" => FuelLabel::Absent,
        _ => FuelLabel::Unrecognized,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticPhase {
    Dtc,
    FreezeFrames,
    Readiness,
    Mode05O2,
    ModuleRefresh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticRequest {
    pub phase: DiagnosticPhase,
    pub service: u8,
}

/// Service eligibility inputs for one diagnostic pass. Spec §11: readiness
/// is skipped only when its cached service row is already `unsupported`
/// (`unverified` is attempted — the operator explicitly asked); Mode-05 has
/// its own fail-closed gate.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServiceGates {
    pub cached_mode05_unsupported: bool,
    pub cached_readiness_unsupported: bool,
    pub is_lly_profile: bool,
}

pub fn request_plan(
    fuel: FuelClass,
    protocol: Protocol,
    gates: ServiceGates,
) -> Vec<DiagnosticRequest> {
    let mut requests = vec![
        DiagnosticRequest {
            phase: DiagnosticPhase::Dtc,
            service: 0x03,
        },
        DiagnosticRequest {
            phase: DiagnosticPhase::Dtc,
            service: 0x07,
        },
        DiagnosticRequest {
            phase: DiagnosticPhase::Dtc,
            service: 0x0A,
        },
        DiagnosticRequest {
            phase: DiagnosticPhase::FreezeFrames,
            service: 0x02,
        },
    ];
    if !gates.cached_readiness_unsupported {
        requests.push(DiagnosticRequest {
            phase: DiagnosticPhase::Readiness,
            service: 0x01,
        });
    }
    if mode05_allowed(
        fuel,
        protocol,
        gates.cached_mode05_unsupported,
        gates.is_lly_profile,
    ) {
        requests.push(DiagnosticRequest {
            phase: DiagnosticPhase::Mode05O2,
            service: 0x05,
        });
    }
    requests.push(DiagnosticRequest {
        phase: DiagnosticPhase::ModuleRefresh,
        service: 0x01,
    });
    requests
}

impl DiagnosticPhase {
    pub const ORDER: [Self; 5] = [
        Self::Dtc,
        Self::FreezeFrames,
        Self::Readiness,
        Self::Mode05O2,
        Self::ModuleRefresh,
    ];
}

/// Mode-05 is intentionally fail-closed. No profile/display heuristic may
/// make this request eligible; only explicit gasoline permits it, and only
/// on a positively identified legacy (non-CAN) protocol — `Auto` means the
/// protocol is unresolved, and `Protocol` is non-exhaustive, so both deny by
/// default rather than slipping past a CAN deny-list.
pub fn mode05_allowed(
    fuel: FuelClass,
    protocol: Protocol,
    cached_unsupported: bool,
    is_lly_profile: bool,
) -> bool {
    matches!(fuel, FuelClass::Gasoline)
        && matches!(
            protocol,
            Protocol::J1850Vpw | Protocol::J1850Pwm | Protocol::Iso9141(_) | Protocol::Kwp2000(_)
        )
        && !cached_unsupported
        && !is_lly_profile
}

pub fn phases() -> &'static [DiagnosticPhase; 5] {
    &DiagnosticPhase::ORDER
}

/// Gate diagnostic service IDs at the request boundary. Telemetry and every
/// non-diagnostic foreground state are denied; Mode-06 is never permitted.
pub fn service_allowed(mode: &ModeState, service: u8) -> bool {
    if !matches!(mode, ModeState::Diagnostic { .. }) {
        return false;
    }
    matches!(service, 0x03 | 0x07 | 0x0A)
}

#[cfg(test)]
mod tests {
    use super::*;
    use obd2_core::vehicle::KLineInit;

    #[test]
    fn diagnostic_phases_have_stable_five_phase_order() {
        assert_eq!(phases().len(), 5);
        assert_eq!(phases()[0], DiagnosticPhase::Dtc);
        assert_eq!(phases()[4], DiagnosticPhase::ModuleRefresh);
    }

    #[test]
    fn mode05_requires_explicit_gasoline_and_non_can() {
        assert!(mode05_allowed(
            FuelClass::Gasoline,
            Protocol::Iso9141(KLineInit::SlowInit),
            false,
            false
        ));
        assert!(!mode05_allowed(
            FuelClass::Unknown,
            Protocol::Iso9141(KLineInit::SlowInit),
            false,
            false
        ));
        assert!(!mode05_allowed(
            FuelClass::Gasoline,
            Protocol::Can11Bit500,
            false,
            false
        ));
        assert!(!mode05_allowed(
            FuelClass::Gasoline,
            Protocol::Iso9141(KLineInit::SlowInit),
            true,
            false
        ));
        assert!(!mode05_allowed(
            FuelClass::Gasoline,
            Protocol::Iso9141(KLineInit::SlowInit),
            false,
            true
        ));
    }

    #[test]
    fn mode05_denies_unresolved_and_unknown_protocols() {
        // Auto = protocol not yet identified: fail closed.
        assert!(!mode05_allowed(
            FuelClass::Gasoline,
            Protocol::Auto,
            false,
            false
        ));
        // The gate is a positive allow-list, so every non-legacy variant —
        // including ones core adds later — denies by default.
        for legacy in [
            Protocol::J1850Vpw,
            Protocol::J1850Pwm,
            Protocol::Iso9141(KLineInit::FastInit),
            Protocol::Kwp2000(KLineInit::SlowInit),
        ] {
            assert!(mode05_allowed(FuelClass::Gasoline, legacy, false, false));
        }
    }

    #[test]
    fn fuel_resolution_is_exact_and_precedence_ordered() {
        // The recognized session claim always wins.
        assert_eq!(
            resolve_fuel(Some("Diesel"), Some("Gasoline")),
            FuelClass::Diesel
        );
        // An unparseable session CLAIM resolves Unknown — the cached NHTSA
        // row never overrules the curated spec (no value shopping).
        assert_eq!(
            resolve_fuel(Some("mild gasoline blend"), Some("Gasoline")),
            FuelClass::Unknown
        );
        assert_eq!(
            resolve_fuel(Some("bio-diesel b20"), Some("Gasoline")),
            FuelClass::Unknown
        );
        // The embedded generic spec ships fuel_type: unknown — an explicit
        // "no claim" that falls through to the database, like a blank field.
        assert_eq!(
            resolve_fuel(Some("unknown"), Some("Gasoline")),
            FuelClass::Gasoline
        );
        assert_eq!(resolve_fuel(Some("  "), Some("Diesel")), FuelClass::Diesel);
        // Shipped embedded specs use lowercase labels.
        assert_eq!(resolve_fuel(Some("diesel"), None), FuelClass::Diesel);
        // Absent session falls through; unrecognized database stays Unknown.
        assert_eq!(resolve_fuel(None, Some("Other")), FuelClass::Other);
        assert_eq!(resolve_fuel(None, Some("unlisted")), FuelClass::Unknown);
        assert_eq!(resolve_fuel(None, None), FuelClass::Unknown);
    }

    #[test]
    fn diagnostic_services_are_impossible_before_command_and_mode06_is_locked() {
        assert!(!service_allowed(&ModeState::Telemetry, 0x03));
        assert!(!service_allowed(&ModeState::Connecting, 0x07));
        assert!(!service_allowed(
            &ModeState::Diagnostic {
                phase: 0,
                phase_total: 5,
                step: 0,
                total: 0,
            },
            0x06
        ));
        assert!(service_allowed(
            &ModeState::Diagnostic {
                phase: 0,
                phase_total: 5,
                step: 0,
                total: 0,
            },
            0x03
        ));
    }

    #[test]
    fn request_plan_preserves_phase_order_and_excludes_mode06() {
        let plan = request_plan(
            FuelClass::Gasoline,
            Protocol::Iso9141(KLineInit::SlowInit),
            ServiceGates::default(),
        );
        assert_eq!(plan[0].service, 0x03);
        assert_eq!(plan[1].service, 0x07);
        assert_eq!(plan[2].service, 0x0A);
        assert_eq!(plan[3].service, 0x02);
        assert!(plan.iter().any(|request| request.service == 0x05));
        assert!(plan.iter().all(|request| request.service != 0x06));
        let diesel = request_plan(
            FuelClass::Diesel,
            Protocol::Iso9141(KLineInit::SlowInit),
            ServiceGates::default(),
        );
        assert!(diesel.iter().all(|request| request.service != 0x05));
    }

    #[test]
    fn readiness_skips_only_when_cached_unsupported() {
        let skipped = request_plan(
            FuelClass::Diesel,
            Protocol::J1850Vpw,
            ServiceGates {
                cached_readiness_unsupported: true,
                ..ServiceGates::default()
            },
        );
        assert!(skipped
            .iter()
            .all(|request| request.phase != DiagnosticPhase::Readiness));
        // Unverified (i.e. not cached-unsupported) is attempted: the
        // operator explicitly requested diagnostics.
        let attempted = request_plan(
            FuelClass::Diesel,
            Protocol::J1850Vpw,
            ServiceGates::default(),
        );
        assert!(attempted
            .iter()
            .any(|request| request.phase == DiagnosticPhase::Readiness));
    }
}
