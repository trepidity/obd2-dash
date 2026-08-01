//! Scripted transport harness for mode-runner tests.
//!
//! The harness wraps [`MockAdapter`] rather than faking `Session`, so tests
//! exercise the same request boundary used by a real transport. Responses can
//! be queued per service/PID and requests can be held behind a `Notify` gate.

use async_trait::async_trait;
use obd2_core::adapter::mock::MockAdapter;
use obd2_core::adapter::{Adapter, AdapterEvent, AdapterInfo, InitializationReport};
use obd2_core::error::Obd2Error;
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::service::ServiceRequest;
use obd2_core::session::Session;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

/// A response injected at the adapter request boundary.
#[derive(Debug, Clone)]
pub enum ScriptedResponse {
    Bytes(Vec<u8>),
    NoData,
    Timeout,
    Transport(String),
    Unsupported(u8),
}

impl ScriptedResponse {
    fn into_result(self) -> Result<Vec<u8>, Obd2Error> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            Self::NoData => Err(Obd2Error::NoData),
            Self::Timeout => Err(Obd2Error::Timeout),
            Self::Transport(detail) => Err(Obd2Error::Transport(detail)),
            Self::Unsupported(pid) => Err(Obd2Error::UnsupportedPid { pid }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RequestKey {
    service: u8,
    pid: Option<u8>,
}

#[derive(Default)]
struct ScriptState {
    responses: HashMap<RequestKey, VecDeque<ScriptedResponse>>,
    requests: Vec<(u8, Option<u8>)>,
    gate: Option<Arc<Notify>>,
}

/// Shared script state. Clone this handle to inspect requests while a runner
/// is active or to release a gated request.
#[derive(Clone, Default)]
pub struct RequestScript {
    state: Arc<Mutex<ScriptState>>,
}

impl RequestScript {
    pub fn push(&self, service: u8, pid: Option<u8>, response: ScriptedResponse) {
        let state = Arc::clone(&self.state);
        // Scripts are configured before a runner starts; avoid exposing an
        // async setup API while still keeping runtime access synchronized.
        match state.try_lock() {
            Ok(mut state) => state
                .responses
                .entry(RequestKey { service, pid })
                .or_default()
                .push_back(response),
            Err(_) => panic!("request script is currently in use"),
        };
    }

    pub async fn requests(&self) -> Vec<(u8, Option<u8>)> {
        self.state.lock().await.requests.clone()
    }

    /// Gate all subsequent requests until the returned notifier is notified.
    pub async fn gate(&self) -> Arc<Notify> {
        let notify = Arc::new(Notify::new());
        self.state.lock().await.gate = Some(Arc::clone(&notify));
        notify
    }
}

/// Adapter wrapper that applies a [`RequestScript`] before delegating to the
/// realistic `MockAdapter` behavior.
pub struct ScriptedAdapter {
    inner: MockAdapter,
    script: RequestScript,
}

impl std::fmt::Debug for ScriptedAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScriptedAdapter").finish_non_exhaustive()
    }
}

#[async_trait]
impl Adapter for ScriptedAdapter {
    async fn initialize(&mut self) -> Result<InitializationReport, Obd2Error> {
        self.inner.initialize().await
    }

    async fn request(&mut self, req: &ServiceRequest) -> Result<Vec<u8>, Obd2Error> {
        let key = RequestKey {
            service: req.service_id,
            pid: req.data.first().copied(),
        };
        let (gate, scripted) = {
            let mut state = self.script.state.lock().await;
            state.requests.push((key.service, key.pid));
            let gate = state.gate.clone();
            let scripted = state.responses.get_mut(&key).and_then(VecDeque::pop_front);
            (gate, scripted)
        };
        if let Some(gate) = gate {
            gate.notified().await;
        }
        match scripted {
            Some(response) => response.into_result(),
            None => self.inner.request(req).await,
        }
    }

    async fn supported_pids(&mut self) -> Result<std::collections::HashSet<Pid>, Obd2Error> {
        // The mask walk goes through this dedicated method, not request(), so
        // scripted errors queued under (0x01, 0x00) apply here — tests use
        // this to force the conservative mask-failure fallback.
        let key = RequestKey {
            service: 0x01,
            pid: Some(0x00),
        };
        let scripted = {
            let mut state = self.script.state.lock().await;
            state.requests.push((key.service, key.pid));
            state.responses.get_mut(&key).and_then(VecDeque::pop_front)
        };
        if let Some(response) = scripted {
            response.into_result()?;
        }
        self.inner.supported_pids().await
    }

    async fn battery_voltage(&mut self) -> Result<Option<f64>, Obd2Error> {
        self.inner.battery_voltage().await
    }

    fn info(&self) -> &AdapterInfo {
        self.inner.info()
    }

    fn drain_events(&mut self) -> Vec<AdapterEvent> {
        self.inner.drain_events()
    }
}

/// Connector that creates a fresh scripted adapter on every call. The VIN is
/// switchable between connects so reconnect tests can simulate moving the
/// adapter to a different vehicle.
pub struct ScriptedConnector {
    pub calls: Arc<AtomicUsize>,
    vin: Arc<std::sync::Mutex<String>>,
    pub script: RequestScript,
}

impl ScriptedConnector {
    pub fn new(vin: impl Into<String>) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            vin: Arc::new(std::sync::Mutex::new(vin.into())),
            script: RequestScript::default(),
        }
    }

    pub fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    /// Handle for changing the VIN the next connect reports.
    pub fn vin_handle(&self) -> Arc<std::sync::Mutex<String>> {
        Arc::clone(&self.vin)
    }
}

#[async_trait]
impl super::SessionConnector for ScriptedConnector {
    type Adapter = ScriptedAdapter;

    async fn connect(&self) -> Result<super::NewSession<Self::Adapter>, super::ConnectError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let vin = self.vin.lock().expect("vin mutex poisoned").clone();
        Ok(super::NewSession {
            session: Session::new(ScriptedAdapter {
                inner: MockAdapter::with_vin(&vin),
                script: self.script.clone(),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode_runner::SessionConnector;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn scripted_response_is_consumed_at_request_boundary() {
        let connector = ScriptedConnector::new("1GCHK23224F000001");
        connector
            .script
            .push(0x01, Some(0x0C), ScriptedResponse::Timeout);
        let mut session = connector.connect().await.unwrap().session;
        session.initialize().await.unwrap();
        let error = session.read_pid(Pid(0x0C)).await.unwrap_err();
        assert!(matches!(error, Obd2Error::Timeout));
        assert_eq!(connector.script.requests().await, vec![(0x01, Some(0x0C))]);
    }

    #[tokio::test]
    async fn gate_holds_request_until_released() {
        let connector = ScriptedConnector::new("1GCHK23224F000001");
        let release = connector.script.gate().await;
        let mut session = connector.connect().await.unwrap().session;
        session.initialize().await.unwrap();
        let request = tokio::spawn(async move { session.read_pid(Pid(0x0C)).await });
        assert!(timeout(Duration::from_millis(20), request).await.is_err());
        release.notify_waiters();
    }
}
