use obd2_core::vehicle::Protocol;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuelClass {
    Gasoline,
    Diesel,
    Other,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticPhase {
    Dtc,
    FreezeFrames,
    Readiness,
    Mode05O2,
    ModuleRefresh,
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
}
