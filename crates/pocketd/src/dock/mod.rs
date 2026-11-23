//! Dock session: `org.pocketos.Core1.Dock` on `/org/pocketos/Core1/Dock` (protocol 1.2).
//!
//! Split:
//! - [`DockService`] (this file) is the D-Bus face: it validates requests against the
//!   current state and hands them to the link task. It never does network I/O.
//! - [`link`] owns the connection to the desktop agent (connect, retry with backoff,
//!   keepalive, request tracking) and publishes every state change.
//! - [`config`] reads the trusted-host configuration and implements `pocketd dock …`.
//!
//! The Dock connection state is independent of the core's operating mode (ADR-0007). It
//! is transient: a restarted core starts `disconnected` until a client calls `Connect`.

pub mod config;
pub mod link;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pocket_dock_protocol::message::ActionInfo;
use pocket_ipc::DockState;
use pocket_ipc::dbus::dock::{action_key, state_key};
use serde_json::{Map, Value};
use tokio::sync::mpsc;
use tracing::{info, warn};
use zbus::{OwnedVariant, Variant, interface, message::Header, object_server::SignalEmitter};

use crate::service::Error;

pub type Dict = HashMap<&'static str, Variant<'static>>;

/// Observable Dock state, shared by the D-Bus object and the link task. The lock is
/// never held across an `.await`.
#[derive(Debug, Default)]
pub struct Shared {
    pub state: DockState,
    pub configured: bool,
    pub host_name: String,
    pub host_id: String,
    pub address: String,
    pub agent: String,
    pub error_code: String,
    pub error: String,
    pub retry_in: u32,
    pub revision: u64,
    pub actions: Vec<ActionInfo>,
    next_request: u64,
}

impl Shared {
    pub fn dict(&self) -> Dict {
        HashMap::from([
            (
                state_key::STATE,
                Variant::from(self.state.as_str().to_owned()),
            ),
            (state_key::CONFIGURED, Variant::from(self.configured)),
            (state_key::HOST_NAME, Variant::from(self.host_name.clone())),
            (state_key::HOST_ID, Variant::from(self.host_id.clone())),
            (state_key::ADDRESS, Variant::from(self.address.clone())),
            (state_key::AGENT, Variant::from(self.agent.clone())),
            (
                state_key::ERROR_CODE,
                Variant::from(self.error_code.clone()),
            ),
            (state_key::ERROR, Variant::from(self.error.clone())),
            (state_key::RETRY_IN, Variant::from(self.retry_in)),
            (state_key::REVISION, Variant::from(self.revision)),
        ])
    }

    pub fn action_dicts(&self) -> Vec<Dict> {
        self.actions
            .iter()
            .map(|a| {
                HashMap::from([
                    (action_key::ID, Variant::from(a.id.clone())),
                    (action_key::LABEL, Variant::from(a.label.clone())),
                    (action_key::CATEGORY, Variant::from(a.category.clone())),
                    (action_key::AVAILABLE, Variant::from(a.available)),
                    (action_key::REASON, Variant::from(a.reason.clone())),
                ])
            })
            .collect()
    }

    pub fn set_host(&mut self, host: &config::Host) {
        self.configured = true;
        self.host_name = host.name.clone();
        self.host_id = host.key.short_id();
        self.address = host.address.to_string();
    }
}

/// Commands from the D-Bus object to the link task.
pub enum Cmd {
    Connect(Box<config::LinkConfig>),
    Disconnect,
    Execute {
        request: u64,
        action: String,
        params: Map<String, Value>,
    },
}

pub type SharedState = Arc<Mutex<Shared>>;

pub fn lock(s: &SharedState) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

pub struct DockService {
    shared: SharedState,
    cmd: mpsc::UnboundedSender<Cmd>,
    dir: PathBuf,
}

impl DockService {
    /// Returns the service and the link task's half (start it with [`link::Link::run`]).
    pub fn new(dir: PathBuf) -> (Self, SharedState, mpsc::UnboundedReceiver<Cmd>) {
        let shared: SharedState = Arc::default();
        {
            let mut s = lock(&shared);
            s.revision = 1;
            match config::load_host(&dir) {
                Ok(h) => s.set_host(&h),
                Err(e) => info!(reason = %e, "dock: no trusted computer configured"),
            }
        }
        let (tx, rx) = mpsc::unbounded_channel();
        (
            DockService {
                shared: shared.clone(),
                cmd: tx,
                dir,
            },
            shared,
            rx,
        )
    }

    fn send(&self, cmd: Cmd) -> Result<(), Error> {
        self.cmd
            .send(cmd)
            .map_err(|_| Error::NotReady("dock link task is not running".into()))
    }
}

/// Converts D-Bus parameters to JSON (strings, booleans and integers only).
fn json_params(params: HashMap<String, OwnedVariant>) -> Result<Map<String, Value>, Error> {
    let mut out = Map::new();
    for (k, v) in params {
        let j = match &*v {
            Variant::Str(s) => Value::from(s.as_str()),
            Variant::Bool(b) => Value::from(*b),
            Variant::I32(i) => Value::from(*i),
            Variant::I64(i) => Value::from(*i),
            Variant::U32(i) => Value::from(*i),
            Variant::U64(i) => Value::from(*i),
            Variant::I16(i) => Value::from(*i),
            Variant::U16(i) => Value::from(*i),
            Variant::U8(i) => Value::from(*i),
            other => {
                return Err(Error::InvalidArgument(format!(
                    "parameter {k:?}: unsupported type {}",
                    other.value_signature()
                )));
            }
        };
        out.insert(k, j);
    }
    Ok(out)
}

fn sender(hdr: &Header<'_>) -> String {
    hdr.sender().map_or_else(|| "?".into(), |s| s.to_string())
}

#[interface(name = "org.pocketos.Core1.Dock")]
impl DockService {
    /// `GetState() -> a{sv}`
    fn get_state(&self) -> Dict {
        lock(&self.shared).dict()
    }

    /// `ListActions() -> aa{sv}` (empty unless connected).
    fn list_actions(&self) -> Vec<Dict> {
        lock(&self.shared).action_dicts()
    }

    /// `Connect()`: re-reads the configuration, then connects (asynchronously; follow
    /// `StateChanged`). Already connected to the same host: no-op.
    async fn connect(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let sender = sender(&hdr);
        match config::load(&self.dir) {
            Ok(cfg) => {
                info!(%sender, host = %cfg.host.name, address = %cfg.host.address, "dock Connect");
                // The link task publishes the resulting state (connecting/connected).
                lock(&self.shared).set_host(&cfg.host);
                self.send(Cmd::Connect(Box::new(cfg)))
            }
            Err(e) => {
                warn!(%sender, error = %e, "dock Connect: not configured");
                let changed = {
                    let mut s = lock(&self.shared);
                    let was = s.configured;
                    s.configured = false;
                    if was {
                        s.revision += 1;
                        Some(s.dict())
                    } else {
                        None
                    }
                };
                if let Some(d) = changed {
                    let _ = Self::state_changed(&emitter, d).await;
                }
                Err(Error::NotConfigured(e))
            }
        }
    }

    /// `Disconnect()`: closes the link and cancels retries.
    fn disconnect(&self, #[zbus(header)] hdr: Header<'_>) -> Result<(), Error> {
        info!(sender = %sender(&hdr), "dock Disconnect");
        self.send(Cmd::Disconnect)
    }

    /// `Execute(s action, a{sv} params) -> t request`: accepts the request; the outcome
    /// is `ActionFinished(request, …)` after the computer answered.
    fn execute(
        &self,
        action: &str,
        params: HashMap<String, OwnedVariant>,
        #[zbus(header)] hdr: Header<'_>,
    ) -> Result<u64, Error> {
        let sender = sender(&hdr);
        let params = json_params(params)?;
        let request = {
            let mut s = lock(&self.shared);
            if s.state != DockState::Connected {
                warn!(%sender, %action, state = %s.state, "dock Execute rejected: not connected");
                return Err(Error::NotConnected(format!("dock is {}", s.state)));
            }
            let Some(info) = s
                .actions
                .iter()
                .find(|a| a.id == action)
                .filter(|_| pocket_dock_protocol::is_valid_action_id(action))
            else {
                warn!(%sender, %action, "dock Execute rejected: unknown action");
                return Err(Error::UnknownAction(format!(
                    "{action:?} is not offered by {}",
                    s.host_name
                )));
            };
            if !info.available {
                return Err(Error::ActionUnavailable(info.reason.clone()));
            }
            s.next_request += 1;
            s.next_request
        };
        info!(%sender, request, %action, "dock Execute accepted");
        self.send(Cmd::Execute {
            request,
            action: action.to_owned(),
            params,
        })?;
        Ok(request)
    }

    /// `StateChanged(a{sv} state)`
    #[zbus(signal)]
    pub async fn state_changed(emitter: &SignalEmitter<'_>, state: Dict) -> zbus::Result<()>;

    /// `ActionsChanged(aa{sv} actions)`
    #[zbus(signal)]
    pub async fn actions_changed(
        emitter: &SignalEmitter<'_>,
        actions: Vec<Dict>,
    ) -> zbus::Result<()>;

    /// `ActionFinished(t request, s action, b ok, s error_code, s message, s data_json)`
    #[zbus(signal)]
    #[allow(clippy::too_many_arguments)]
    pub async fn action_finished(
        emitter: &SignalEmitter<'_>,
        request: u64,
        action: &str,
        ok: bool,
        error_code: &str,
        message: &str,
        data_json: &str,
    ) -> zbus::Result<()>;
}
