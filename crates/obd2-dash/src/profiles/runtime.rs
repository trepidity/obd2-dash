use obd2_core::adapter::Adapter;
use obd2_core::error::Obd2Error;
use obd2_core::protocol::codec::BusFamily;
use obd2_core::protocol::service::Target;
use obd2_core::session::Session;
use obd2_core::vehicle::{ModuleId, PhysicalAddress, Protocol};

use crate::profiles::model::{
    AddressState, AddressTemplate, DecodedDtc, DecodedSignal, DiagnosticProfile,
    DtcServiceDefinition, ModuleDefinition, ModuleKey, ModuleMap, ProfileDecodeError, ProfileId,
    RouteDefinition, RouteScope, RouteSet, SelectedProfile, SignalDefinition, SourceFields,
    VehicleContext,
};
use crate::profiles::registry::ProfileRegistry;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityId {
    Signal(&'static str),
    DtcService(&'static str),
    ActiveTest(&'static str),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId(pub usize);

impl RequestId {
    pub const SINGLE: RequestId = RequestId(0);
}

#[derive(Debug, PartialEq)]
pub enum ProfileResponse {
    Signal(DecodedSignal),
    Dtcs(Vec<DecodedDtc>),
}

#[derive(Debug)]
pub enum DispatchError {
    UnknownProfile(ProfileId),
    StaleGeneration {
        token: u64,
        current: u64,
    },
    CapabilityNotOwned {
        profile: ProfileId,
        capability: CapabilityId,
    },
    RouteNotOwnedByCapability {
        capability: CapabilityId,
        request: RequestId,
    },
    ProtocolFamilyMismatch {
        route: BusFamily,
        live: Option<BusFamily>,
    },
    ActiveTestLocked {
        capability: CapabilityId,
    },
    MissingModuleMap {
        profile: ProfileId,
    },
    UnknownModule {
        module: ModuleKey,
    },
    UnknownBus {
        bus: &'static str,
    },
    AddressCandidate {
        module: ModuleKey,
        reason: &'static str,
    },
    AddressUnresolved {
        module: ModuleKey,
        reason: &'static str,
    },
    AddressBusMismatch {
        template: AddressTemplate,
        bus: BusFamily,
    },
    Transport(Obd2Error),
    Decode(ProfileDecodeError),
}

pub struct DispatchEvidence<'a> {
    pub profile_id: ProfileId,
    pub capability: CapabilityId,
    pub route: &'a RouteDefinition,
    pub service_id: u8,
    pub request_data: &'a [u8],
    pub physical_address: PhysicalAddress,
    pub raw_payload: &'a [u8],
    pub decoder_id: &'static str,
    pub label: Option<&'static str>,
    pub unit: Option<&'static str>,
    pub source_fields: SourceFields,
    pub outcome: Result<&'a ProfileResponse, &'a DispatchError>,
    pub is_probe: bool,
}

pub trait ProfileEvidenceSink {
    fn record(&mut self, evidence: &DispatchEvidence<'_>);
}

pub struct NullEvidenceSink;

impl ProfileEvidenceSink for NullEvidenceSink {
    fn record(&mut self, _evidence: &DispatchEvidence<'_>) {}
}

pub struct ProfileRuntime<'r> {
    pub(in crate::profiles) registry: &'r ProfileRegistry,
}

impl<'r> ProfileRuntime<'r> {
    pub fn new(registry: &'r ProfileRegistry) -> Self {
        Self { registry }
    }

    pub async fn execute_request<A: Adapter, E: ProfileEvidenceSink>(
        &self,
        session: &mut Session<A>,
        ctx: &VehicleContext,
        selected: &SelectedProfile,
        capability: CapabilityId,
        request: RequestId,
        evidence: &mut E,
    ) -> Result<ProfileResponse, DispatchError> {
        let profile = self
            .registry
            .get(selected.profile_id())
            .ok_or(DispatchError::UnknownProfile(selected.profile_id()))?;

        if selected.context_generation() != ctx.generation {
            return Err(DispatchError::StaleGeneration {
                token: selected.context_generation(),
                current: ctx.generation,
            });
        }

        let resolved_capability =
            resolve_capability(profile, capability).ok_or(DispatchError::CapabilityNotOwned {
                profile: selected.profile_id(),
                capability,
            })?;
        if matches!(resolved_capability, ResolvedCapability::ActiveTest) {
            return Err(DispatchError::ActiveTestLocked { capability });
        }

        let map = profile
            .module_map()
            .ok_or(DispatchError::MissingModuleMap {
                profile: selected.profile_id(),
            })?;

        let route = capability_route(map, ctx, &resolved_capability, request)?;
        let resolved = resolve_route(map, &route, ctx.protocol)?;

        match resolved_capability {
            ResolvedCapability::Signal(signal) => {
                let payload = match session
                    .raw_physical_request(
                        signal.service_id,
                        signal.request_data,
                        resolved.physical_address.clone(),
                    )
                    .await
                {
                    Ok(payload) => payload,
                    Err(err) => {
                        let dispatch = DispatchError::Transport(err);
                        record_dispatch(
                            evidence,
                            selected.profile_id(),
                            capability,
                            &route,
                            &resolved,
                            signal.service_id,
                            signal.request_data,
                            signal.decoder_id,
                            Some(signal.label),
                            Some(signal.unit),
                            signal.source_fields,
                            &[],
                            Err(&dispatch),
                        );
                        return Err(dispatch);
                    }
                };

                let decoded = match profile.decode_signal(signal, &payload) {
                    Ok(decoded) => decoded,
                    Err(err) => {
                        let dispatch = DispatchError::Decode(err);
                        record_dispatch(
                            evidence,
                            selected.profile_id(),
                            capability,
                            &route,
                            &resolved,
                            signal.service_id,
                            signal.request_data,
                            signal.decoder_id,
                            Some(signal.label),
                            Some(signal.unit),
                            signal.source_fields,
                            &payload,
                            Err(&dispatch),
                        );
                        return Err(dispatch);
                    }
                };
                let response = ProfileResponse::Signal(decoded);
                record_dispatch(
                    evidence,
                    selected.profile_id(),
                    capability,
                    &route,
                    &resolved,
                    signal.service_id,
                    signal.request_data,
                    signal.decoder_id,
                    Some(signal.label),
                    Some(signal.unit),
                    signal.source_fields,
                    &payload,
                    Ok(&response),
                );
                Ok(response)
            }
            ResolvedCapability::DtcService(service) => {
                let payload = match session
                    .raw_physical_request(
                        service.service_id,
                        service.request_data,
                        resolved.physical_address.clone(),
                    )
                    .await
                {
                    Ok(payload) => payload,
                    Err(err) => {
                        let dispatch = DispatchError::Transport(err);
                        record_dispatch(
                            evidence,
                            selected.profile_id(),
                            capability,
                            &route,
                            &resolved,
                            service.service_id,
                            service.request_data,
                            service.decoder_id,
                            Some(service.label),
                            None,
                            SourceFields::NONE,
                            &[],
                            Err(&dispatch),
                        );
                        return Err(dispatch);
                    }
                };

                let mut decoded = match profile.decode_dtc_response(service, &payload) {
                    Ok(decoded) => decoded,
                    Err(err) => {
                        let dispatch = DispatchError::Decode(err);
                        record_dispatch(
                            evidence,
                            selected.profile_id(),
                            capability,
                            &route,
                            &resolved,
                            service.service_id,
                            service.request_data,
                            service.decoder_id,
                            Some(service.label),
                            None,
                            SourceFields::NONE,
                            &payload,
                            Err(&dispatch),
                        );
                        return Err(dispatch);
                    }
                };
                let module = route.module.to_core_module_id();
                for dtc in &mut decoded {
                    if dtc.module.is_none() {
                        dtc.module = Some(module.clone());
                    }
                }
                let response = ProfileResponse::Dtcs(decoded);
                record_dispatch(
                    evidence,
                    selected.profile_id(),
                    capability,
                    &route,
                    &resolved,
                    service.service_id,
                    service.request_data,
                    service.decoder_id,
                    Some(service.label),
                    None,
                    SourceFields::NONE,
                    &payload,
                    Ok(&response),
                );
                Ok(response)
            }
            ResolvedCapability::ActiveTest => Err(DispatchError::ActiveTestLocked { capability }),
        }
    }
}

enum ResolvedCapability<'a> {
    Signal(&'a SignalDefinition),
    DtcService(&'a DtcServiceDefinition),
    ActiveTest,
}

fn resolve_capability(
    profile: &dyn DiagnosticProfile,
    capability: CapabilityId,
) -> Option<ResolvedCapability<'_>> {
    match capability {
        CapabilityId::Signal(key) => profile
            .signals()
            .iter()
            .find(|signal| signal.key == key)
            .map(ResolvedCapability::Signal),
        CapabilityId::DtcService(key) => profile
            .dtc_services()
            .iter()
            .find(|service| service.key == key)
            .map(ResolvedCapability::DtcService),
        CapabilityId::ActiveTest(_key) => Some(ResolvedCapability::ActiveTest),
    }
}

fn capability_route(
    map: &ModuleMap,
    ctx: &VehicleContext,
    capability: &ResolvedCapability<'_>,
    request: RequestId,
) -> Result<RouteDefinition, DispatchError> {
    match capability {
        ResolvedCapability::Signal(signal) => {
            if request == RequestId::SINGLE {
                Ok(signal.route)
            } else {
                Err(DispatchError::RouteNotOwnedByCapability {
                    capability: CapabilityId::Signal(signal.key),
                    request,
                })
            }
        }
        ResolvedCapability::DtcService(service) => route_from_set(
            map,
            ctx,
            service.route_set,
            request,
            CapabilityId::DtcService(service.key),
        ),
        ResolvedCapability::ActiveTest => Err(DispatchError::ActiveTestLocked {
            capability: CapabilityId::ActiveTest("active"),
        }),
    }
}

fn route_from_set(
    map: &ModuleMap,
    ctx: &VehicleContext,
    route_set: RouteSet,
    request: RequestId,
    capability: CapabilityId,
) -> Result<RouteDefinition, DispatchError> {
    match route_set.scope {
        RouteScope::Single(route) => {
            if request == RequestId::SINGLE {
                Ok(route)
            } else {
                Err(DispatchError::RouteNotOwnedByCapability {
                    capability,
                    request,
                })
            }
        }
        RouteScope::Explicit(routes) => {
            routes
                .get(request.0)
                .copied()
                .ok_or(DispatchError::RouteNotOwnedByCapability {
                    capability,
                    request,
                })
        }
        RouteScope::DiscoveredOnBus { bus } => {
            let mut index = 0usize;
            for module_id in &ctx.discovered_modules {
                let Some(module) = module_for_core_id(map, module_id) else {
                    continue;
                };
                if module.bus != bus {
                    continue;
                }
                if index == request.0 {
                    return Ok(RouteDefinition { module: module.key });
                }
                index += 1;
            }
            Err(DispatchError::RouteNotOwnedByCapability {
                capability,
                request,
            })
        }
    }
}

pub struct ResolvedRoute<'a> {
    pub route: &'a RouteDefinition,
    pub module: &'a ModuleDefinition,
    pub bus_family: BusFamily,
    pub physical_address: PhysicalAddress,
    pub target: Target,
}

pub fn resolve_route<'a>(
    map: &'a ModuleMap,
    route: &'a RouteDefinition,
    active: Protocol,
) -> Result<ResolvedRoute<'a>, DispatchError> {
    let module = map
        .modules
        .iter()
        .find(|module| module.key == route.module)
        .ok_or(DispatchError::UnknownModule {
            module: route.module,
        })?;

    let bus =
        map.buses
            .iter()
            .find(|bus| bus.key == module.bus)
            .ok_or(DispatchError::UnknownBus {
                bus: module.bus.as_str(),
            })?;

    let live_family = bus_family(active);
    if live_family != Some(bus.family) {
        return Err(DispatchError::ProtocolFamilyMismatch {
            route: bus.family,
            live: live_family,
        });
    }

    let template = match module.address {
        AddressState::Confirmed(template) => template,
        AddressState::Candidate { reason, .. } => {
            return Err(DispatchError::AddressCandidate {
                module: module.key,
                reason,
            })
        }
        AddressState::Unresolved { reason } => {
            return Err(DispatchError::AddressUnresolved {
                module: module.key,
                reason,
            })
        }
    };

    let template_family = address_template_family(template);
    if template_family != bus.family {
        return Err(DispatchError::AddressBusMismatch {
            template,
            bus: bus.family,
        });
    }

    let physical_address = match template {
        AddressTemplate::J1850 { node } => {
            let convention = bus.j1850.ok_or(DispatchError::AddressBusMismatch {
                template,
                bus: bus.family,
            })?;
            PhysicalAddress::J1850 {
                node,
                header: [convention.priority, node, convention.source],
            }
        }
        AddressTemplate::Can11 {
            request_id,
            response_id,
        } => PhysicalAddress::Can11Bit {
            request_id,
            response_id,
        },
        AddressTemplate::Can29 {
            request_id,
            response_id,
        } => PhysicalAddress::Can29Bit {
            request_id,
            response_id,
        },
    };

    Ok(ResolvedRoute {
        route,
        module,
        bus_family: bus.family,
        physical_address,
        target: resolve_route_target(route),
    })
}

pub fn resolve_route_target(route: &RouteDefinition) -> Target {
    Target::Module(route.module.canonical().to_string())
}

pub fn bus_family(protocol: Protocol) -> Option<BusFamily> {
    match protocol {
        Protocol::J1850Vpw | Protocol::J1850Pwm => Some(BusFamily::J1850),
        Protocol::Can11Bit500
        | Protocol::Can11Bit250
        | Protocol::Can29Bit500
        | Protocol::Can29Bit250 => Some(BusFamily::Can),
        Protocol::Iso9141(_) => Some(BusFamily::Iso9141),
        Protocol::Kwp2000(_) => Some(BusFamily::Kwp2000),
        Protocol::Auto => None,
        _ => None,
    }
}

fn address_template_family(template: AddressTemplate) -> BusFamily {
    match template {
        AddressTemplate::J1850 { .. } => BusFamily::J1850,
        AddressTemplate::Can11 { .. } | AddressTemplate::Can29 { .. } => BusFamily::Can,
    }
}

fn module_for_core_id<'a>(
    map: &'a ModuleMap,
    module_id: &ModuleId,
) -> Option<&'a ModuleDefinition> {
    map.modules
        .iter()
        .find(|module| module.key.canonical() == module_id.0)
}

#[allow(clippy::too_many_arguments)]
fn record_dispatch<E: ProfileEvidenceSink>(
    evidence: &mut E,
    profile_id: ProfileId,
    capability: CapabilityId,
    route: &RouteDefinition,
    resolved: &ResolvedRoute<'_>,
    service_id: u8,
    request_data: &[u8],
    decoder_id: &'static str,
    label: Option<&'static str>,
    unit: Option<&'static str>,
    source_fields: SourceFields,
    raw_payload: &[u8],
    outcome: Result<&ProfileResponse, &DispatchError>,
) {
    let ev = DispatchEvidence {
        profile_id,
        capability,
        route,
        service_id,
        request_data,
        physical_address: resolved.physical_address.clone(),
        raw_payload,
        decoder_id,
        label,
        unit,
        source_fields,
        outcome,
        is_probe: false,
    };
    evidence.record(&ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::model::{
        AddressState, AddressTemplate, BusDefinition, BusKey, J1850HeaderConvention,
        ModuleSafetyClass,
    };

    const J1850: BusKey = BusKey::new("j1850vpw");
    const CAN: BusKey = BusKey::new("can11");

    const BUSES: &[BusDefinition] = &[
        BusDefinition {
            key: J1850,
            family: BusFamily::J1850,
            protocol: Protocol::J1850Vpw,
            j1850: Some(J1850HeaderConvention {
                priority: 0x6C,
                source: 0xF1,
            }),
            label: "J1850",
        },
        BusDefinition {
            key: CAN,
            family: BusFamily::Can,
            protocol: Protocol::Can11Bit500,
            j1850: None,
            label: "CAN",
        },
    ];

    const MODULES: &[ModuleDefinition] = &[
        ModuleDefinition {
            key: ModuleKey::Ecm,
            display_label: "ECM",
            bus: J1850,
            address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x10 }),
            safety_class: ModuleSafetyClass::Powertrain,
            coresident_with: None,
        },
        ModuleDefinition {
            key: ModuleKey::Tcm,
            display_label: "TCM",
            bus: J1850,
            address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x18 }),
            safety_class: ModuleSafetyClass::Powertrain,
            coresident_with: None,
        },
        ModuleDefinition {
            key: ModuleKey::Ebcm,
            display_label: "EBCM",
            bus: J1850,
            address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x29 }),
            safety_class: ModuleSafetyClass::WriteForbidden,
            coresident_with: None,
        },
        ModuleDefinition {
            key: ModuleKey::Bcm,
            display_label: "BCM",
            bus: CAN,
            address: AddressState::Confirmed(AddressTemplate::Can11 {
                request_id: 0x7E0,
                response_id: 0x7E8,
            }),
            safety_class: ModuleSafetyClass::Informational,
            coresident_with: None,
        },
    ];

    const MAP: ModuleMap = ModuleMap {
        buses: BUSES,
        modules: MODULES,
    };

    #[test]
    fn resolve_route_j1850_uses_module_map_header() {
        for (module, node) in [
            (ModuleKey::Ecm, 0x10),
            (ModuleKey::Tcm, 0x18),
            (ModuleKey::Ebcm, 0x29),
        ] {
            let route = RouteDefinition { module };
            let resolved = resolve_route(&MAP, &route, Protocol::J1850Vpw).unwrap();
            assert_eq!(
                resolved.target,
                Target::Module(module.canonical().to_string())
            );
            assert_eq!(
                resolved.physical_address,
                PhysicalAddress::J1850 {
                    node,
                    header: [0x6C, node, 0xF1],
                }
            );
        }
    }

    #[test]
    fn resolve_route_can_passthrough() {
        let route = RouteDefinition {
            module: ModuleKey::Bcm,
        };
        let resolved = resolve_route(&MAP, &route, Protocol::Can11Bit500).unwrap();
        assert_eq!(
            resolved.physical_address,
            PhysicalAddress::Can11Bit {
                request_id: 0x7E0,
                response_id: 0x7E8,
            }
        );
    }

    #[test]
    fn bus_family_rejects_auto() {
        assert_eq!(bus_family(Protocol::Auto), None);
        assert_eq!(bus_family(Protocol::J1850Vpw), Some(BusFamily::J1850));
        assert_eq!(bus_family(Protocol::Can11Bit500), Some(BusFamily::Can));
    }
}
