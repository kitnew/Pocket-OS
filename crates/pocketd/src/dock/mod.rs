//! Dock session: `org.pocketos.Core1.Dock` on `/org/pocketos/Core1/Dock` (protocol 1.2;
//! 1.4 adds the Stage 4 platform: deck, request keys and Retry, live state,
//! notifications — docs/dock-mode/protocol.md).
//!
//! Split:
//! - [`DockService`] (this file) is the D-Bus face: it validates requests against the
//!   current state and hands them to the link task. It never does network I/O.
//! - [`link`] owns the connection to the desktop agent (connect, retry with backoff,
//!   keepalive, request tracking) and publishes every state change.
//! - [`config`] reads the trusted-host configuration and implements `pocketd dock …`.
//!
//! - [`netwatch`] turns rtnetlink notifications into "the network changed" hints.
//!
//! The Dock connection state is independent of the core's operating mode (ADR-0007). It
//! is transient. What survives a core restart within the graphical session is the
//! user's *intent* (`connection_wanted`, see [`crate::recovery`]): Connect (or entering
//! Dock Mode) sets it, Disconnect clears it, and a restarted core that finds it set
//! connects again with a fresh authenticated session.

pub mod config;
pub mod deck;
pub mod link;
pub mod netwatch;
pub mod prefs;

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use pocket_dock_protocol::message::ActionInfo;
use pocket_ipc::DockState;
use pocket_ipc::dbus::dock::{action_key, pref_key, state_key};
use serde_json::{Map, Value};
use tokio::sync::mpsc;
use tokio::time::Instant;
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
    /// Deadline of the scheduled automatic attempt (owned by the link task).
    pub retry_at: Option<Instant>,
    /// The user wants a link (Connect / entering Dock Mode) or not (Disconnect).
    pub wanted: bool,
    pub revision: u64,
    pub actions: Vec<ActionInfo>,
    next_request: u64,
    /// Random per-process prefix of request keys (Core 1.4, protocol 1.1).
    key_prefix: String,
    /// Requests whose outcome is unknown (`timeout`, `connection_lost`), for `Retry`.
    pub uncertain: HashMap<u64, Uncertain>,
    /// Live topics of the current session (cleared when it ends).
    pub live: BTreeMap<String, LiveTopic>,
    pub prefs: prefs::Prefs,
}

/// How long an uncertain request can be retried with its original key (the agent keeps
/// results for the same time).
pub const UNCERTAIN_TTL: Duration = Duration::from_secs(600);
const MAX_UNCERTAIN: usize = 64;

/// A request whose outcome is unknown: everything needed to send it again, unchanged.
#[derive(Debug, Clone)]
pub struct Uncertain {
    pub action: String,
    pub params: Map<String, Value>,
    pub key: String,
    pub at: Instant,
}

/// The latest value of one live topic.
#[derive(Debug, Clone)]
pub struct LiveTopic {
    pub rev: u64,
    pub available: bool,
    pub reason: String,
    pub interval_ms: u64,
    pub data: Value,
    pub received: Instant,
}

impl LiveTopic {
    pub fn json(&self) -> Value {
        serde_json::json!({
            "rev": self.rev, "available": self.available, "reason": self.reason,
            "interval_ms": self.interval_ms, "data": self.data,
            "age_ms": self.received.elapsed().as_millis() as u64,
        })
    }
}

impl Shared {
    /// Time left until the scheduled attempt, computed when the state is read, so a
    /// client that reads it late (a restarted shell) still gets the right value. At
    /// least 1 ms while an attempt is scheduled (0 means none).
    pub fn retry_in_ms(&self) -> u32 {
        self.retry_at.map_or(0, |at| {
            let left = at.saturating_duration_since(Instant::now()).as_millis();
            left.clamp(1, u128::from(u32::MAX)) as u32
        })
    }

    pub fn dict(&self) -> Dict {
        let ms = self.retry_in_ms();
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
            (state_key::RETRY_IN, Variant::from(ms.div_ceil(1000))),
            (state_key::RETRY_IN_MS, Variant::from(ms)),
            (state_key::CONNECTION_WANTED, Variant::from(self.wanted)),
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
                    (
                        action_key::PRIVILEGE,
                        Variant::from(a.privilege.as_str().to_owned()),
                    ),
                    (action_key::IDEMPOTENT, Variant::from(a.idempotent)),
                    (action_key::CONFIRM, Variant::from(a.confirm)),
                ])
            })
            .collect()
    }

    /// Accepts a request: a new id and a request key unique to this intent.
    fn new_request(&mut self) -> (u64, String) {
        self.next_request += 1;
        (
            self.next_request,
            format!("{}-{}", self.key_prefix, self.next_request),
        )
    }

    /// Records a request whose outcome is unknown (bounded, oldest dropped first).
    pub fn remember_uncertain(&mut self, request: u64, u: Uncertain) {
        self.uncertain.retain(|_, x| x.at.elapsed() < UNCERTAIN_TTL);
        while self.uncertain.len() >= MAX_UNCERTAIN {
            let Some(old) = self
                .uncertain
                .iter()
                .min_by_key(|(_, x)| x.at)
                .map(|(k, _)| *k)
            else {
                break;
            };
            self.uncertain.remove(&old);
        }
        self.uncertain.insert(request, u);
    }

    pub fn live_json(&self) -> String {
        let m: serde_json::Map<String, Value> = self
            .live
            .iter()
            .map(|(k, t)| (k.clone(), t.json()))
            .collect();
        Value::Object(m).to_string()
    }

    pub fn prefs_dict(&self) -> Dict {
        HashMap::from([
            (pref_key::DND, Variant::from(self.prefs.dnd)),
            (
                pref_key::MUTED_APPS,
                Variant::from(self.prefs.muted_apps.clone()),
            ),
        ])
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
        /// Request key (the same key on `Retry`).
        key: String,
    },
}

/// 16 random hex characters (request keys must not repeat across core restarts).
fn random_prefix() -> String {
    let mut b = [0u8; 8];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = std::io::Read::read_exact(&mut f, &mut b);
    }
    if b == [0; 8] {
        // No urandom (never on Linux): time and pid still differ between processes.
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        b = (t ^ u64::from(std::process::id()).rotate_left(32)).to_le_bytes();
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub type SharedState = Arc<Mutex<Shared>>;

pub fn lock(s: &SharedState) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

/// The phone side of the Dock policy, shared by the D-Bus object and the core object
/// (entering Dock Mode connects). Cheap to clone.
#[derive(Clone)]
pub struct DockControl {
    shared: SharedState,
    cmd: mpsc::UnboundedSender<Cmd>,
    dir: PathBuf,
}

impl DockControl {
    fn send(&self, cmd: Cmd) -> Result<(), Error> {
        self.cmd
            .send(cmd)
            .map_err(|_| Error::NotReady("dock link task is not running".into()))
    }

    /// Re-reads the trusted-host configuration and asks the link task to connect. The
    /// link task records the intent (`connection_wanted`) when it handles the command.
    pub fn connect(&self, why: &str) -> Result<(), ConnectError> {
        let cfg = config::load(&self.dir).map_err(ConnectError::NotConfigured)?;
        info!(why, host = %cfg.host.name, address = %cfg.host.address, "dock Connect");
        lock(&self.shared).set_host(&cfg.host);
        self.send(Cmd::Connect(Box::new(cfg)))
            .map_err(ConnectError::Core)
    }

    /// Entering Dock Mode connects unless a link is already up or being retried
    /// (ADR-0007). This is the explicit user action that sets the intent again after
    /// a Disconnect; a restored mode after a core restart does not come through here.
    pub fn entered_dock_mode(&self) {
        let idle = lock(&self.shared).state == DockState::Disconnected;
        if !idle {
            return;
        }
        match self.connect("entered dock mode") {
            Ok(()) => {}
            Err(ConnectError::NotConfigured(e)) => {
                info!(reason = %e, "dock mode: no trusted computer to connect to")
            }
            Err(ConnectError::Core(e)) => warn!(error = ?e, "dock mode: cannot connect"),
        }
    }
}

pub enum ConnectError {
    NotConfigured(String),
    Core(Error),
}

pub struct DockService {
    control: DockControl,
    /// Control Deck configuration (re-read by `ReloadDeck`).
    deck: Mutex<deck::Deck>,
}

impl DockService {
    /// Returns the service, its control handle and the link task's half (start it with
    /// [`link::Link::run`]). `wanted` is the recovered connection intent.
    pub fn new(
        dir: PathBuf,
        wanted: bool,
    ) -> (Self, DockControl, SharedState, mpsc::UnboundedReceiver<Cmd>) {
        let shared: SharedState = Arc::default();
        {
            let mut s = lock(&shared);
            s.revision = 1;
            s.wanted = wanted;
            s.key_prefix = random_prefix();
            s.prefs = prefs::Prefs::load(&dir);
            match config::load_host(&dir) {
                Ok(h) => s.set_host(&h),
                Err(e) => info!(reason = %e, "dock: no trusted computer configured"),
            }
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let deck = deck::load(&dir);
        info!(source = deck.source, "dock: deck loaded");
        let control = DockControl {
            shared: shared.clone(),
            cmd: tx,
            dir,
        };
        (
            DockService {
                control: control.clone(),
                deck: Mutex::new(deck),
            },
            control,
            shared,
            rx,
        )
    }

    fn send(&self, cmd: Cmd) -> Result<(), Error> {
        self.control.send(cmd)
    }
}

/// Converts D-Bus parameters to JSON (strings, booleans and integers only).
fn json_params(params: HashMap<String, OwnedVariant>) -> Result<Map<String, Value>, Error> {
    let mut out = Map::new();
    for (k, v) in params {
        // `dbus-send` and some bindings wrap dict values once more (`v` in `v`).
        let inner = match &*v {
            Variant::Variant(b) => b.as_ref(),
            other => other,
        };
        let j = match inner {
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
        lock(&self.control.shared).dict()
    }

    /// `ListActions() -> aa{sv}` (empty unless connected).
    fn list_actions(&self) -> Vec<Dict> {
        lock(&self.control.shared).action_dicts()
    }

    /// `Connect()`: re-reads the configuration, records that the user wants the link,
    /// then connects (asynchronously; follow `StateChanged`). While a retry is pending
    /// it retries at once ("Retry now"). Already connected to the same host: no-op.
    async fn connect(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let sender = sender(&hdr);
        match self.control.connect(&format!("requested by {sender}")) {
            Ok(()) => Ok(()),
            Err(ConnectError::Core(e)) => Err(e),
            Err(ConnectError::NotConfigured(e)) => {
                warn!(%sender, error = %e, "dock Connect: not configured");
                let changed = {
                    let mut s = lock(&self.control.shared);
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

    /// `Disconnect()`: closes the link, cancels retries and records that the user does
    /// not want a link: nothing reconnects by itself (not a core or shell restart, not
    /// a network change) until `Connect` or entering Dock Mode again.
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
            let mut s = lock(&self.control.shared);
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
            s.new_request()
        };
        let (request, key) = request;
        info!(%sender, request, %action, "dock Execute accepted");
        self.send(Cmd::Execute {
            request,
            action: action.to_owned(),
            params,
            key,
        })?;
        Ok(request)
    }

    /// `Retry(t request) -> t new_request` (since 1.4): sends a request whose outcome is
    /// unknown (`timeout`, `connection_lost`, within 10 minutes) again with its original
    /// key. The computer returns the stored result if it already ran it (no second
    /// execution) or runs it once. Core never retries on its own.
    fn retry(&self, request: u64, #[zbus(header)] hdr: Header<'_>) -> Result<u64, Error> {
        let sender = sender(&hdr);
        let (new, u) = {
            let mut s = lock(&self.control.shared);
            s.uncertain.retain(|_, x| x.at.elapsed() < UNCERTAIN_TTL);
            let Some(u) = s.uncertain.get(&request).cloned() else {
                return Err(Error::UnknownRequest(format!(
                    "request {request} has no unknown outcome to retry (or it expired)"
                )));
            };
            if s.state != DockState::Connected {
                return Err(Error::NotConnected(format!("dock is {}", s.state)));
            }
            match s.actions.iter().find(|a| a.id == u.action) {
                None => {
                    return Err(Error::UnknownAction(format!(
                        "{:?} is no longer offered",
                        u.action
                    )));
                }
                Some(a) if !a.available => return Err(Error::ActionUnavailable(a.reason.clone())),
                Some(_) => {}
            }
            s.uncertain.remove(&request);
            let (new, _) = s.new_request();
            (new, u)
        };
        info!(%sender, request, new_request = new, action = %u.action, "dock Retry accepted (same key)");
        self.send(Cmd::Execute {
            request: new,
            action: u.action,
            params: u.params,
            key: u.key,
        })?;
        Ok(new)
    }

    /// `GetLive() -> s` (since 1.4): `{topic: {rev, available, reason, interval_ms, data,
    /// age_ms}}` of the current session; `{}` when not connected.
    fn get_live(&self) -> String {
        lock(&self.control.shared).live_json()
    }

    /// `GetPreferences() -> a{sv}` (since 1.4): `dnd b`, `muted_apps as`.
    fn get_preferences(&self) -> Dict {
        lock(&self.control.shared).prefs_dict()
    }

    /// `SetPreference(s key, v value)` (since 1.4): `dnd` (b), `mute_app` (s),
    /// `unmute_app` (s). Persisted before the call returns.
    fn set_preference(
        &self,
        key: &str,
        value: OwnedVariant,
        #[zbus(header)] hdr: Header<'_>,
    ) -> Result<(), Error> {
        let mut s = lock(&self.control.shared);
        let mut p = s.prefs.clone();
        match (key, &*value) {
            ("dnd", Variant::Bool(b)) => p.dnd = *b,
            ("mute_app", Variant::Str(a)) => p.mute(a.as_str()).map_err(Error::InvalidArgument)?,
            ("unmute_app", Variant::Str(a)) => p.unmute(a.as_str()),
            _ => {
                return Err(Error::InvalidArgument(format!(
                    "unknown preference {key:?} or wrong type {}",
                    value.value_signature()
                )));
            }
        }
        p.save(&self.control.dir)
            .map_err(|e| Error::InvalidArgument(format!("cannot save preferences: {e}")))?;
        info!(sender = %sender(&hdr), key, dnd = p.dnd, muted = p.muted_apps.len(), "dock: preference changed");
        s.prefs = p;
        Ok(())
    }

    /// `GetDeck() -> s`: the validated Control Deck configuration as JSON (since 1.4):
    /// `{version, navigation: {idle_timeout_s}, pages: [{id, title, tiles: [{id, action,
    /// label, icon, params}]}], source: "user"|"default", error}`.
    fn get_deck(&self) -> String {
        self.deck
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .to_json()
    }

    /// `ReloadDeck()`: re-reads `deck.json` and emits `DeckChanged` (since 1.4).
    async fn reload_deck(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let d = deck::load(&self.control.dir);
        info!(sender = %sender(&hdr), source = d.source, error = %d.error, "dock: deck reloaded");
        *self.deck.lock().unwrap_or_else(|p| p.into_inner()) = d;
        Self::deck_changed(&emitter).await?;
        Ok(())
    }

    /// `DeckChanged()`: call `GetDeck` again (since 1.4).
    #[zbus(signal)]
    pub async fn deck_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// `LiveChanged(s topic, s json)` (since 1.4): the topic's new value (as in
    /// `GetLive`); empty `json` = cleared (the session ended).
    #[zbus(signal)]
    pub async fn live_changed(
        emitter: &SignalEmitter<'_>,
        topic: &str,
        json: &str,
    ) -> zbus::Result<()>;

    /// `ActionProgress(t request, u step, u total, s label)` (since 1.4).
    #[zbus(signal)]
    pub async fn action_progress(
        emitter: &SignalEmitter<'_>,
        request: u64,
        step: u32,
        total: u32,
        label: &str,
    ) -> zbus::Result<()>;

    /// `Notification(s json)` (since 1.4): `{nid, app, summary, body, urgency, redacted,
    /// time_ms, silent}`.
    #[zbus(signal)]
    pub async fn notification(emitter: &SignalEmitter<'_>, json: &str) -> zbus::Result<()>;

    /// `NotificationClosed(s nid)` (since 1.4).
    #[zbus(signal)]
    pub async fn notification_closed(emitter: &SignalEmitter<'_>, nid: &str) -> zbus::Result<()>;

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
