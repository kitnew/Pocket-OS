//! The link task: one connection to the trusted desktop agent.
//!
//! ```text
//!            Connect                 welcome
//! disconnected ──────► connecting ─────────────► connected
//!      ▲                 │   ▲                      │ closed / error / no pong
//!      │ Disconnect      │   │ timer (2,4,8,16,30,60 s)
//!      │ (any state)     ▼   │                      ▼
//!      └──────────────── error ◄────────────────────┘
//!                        (no timer for unauthorized, host_key_mismatch,
//!                         incompatible_protocol: these need the user)
//! ```
//!
//! Keepalive: a `ping` after [`PING_INTERVAL`] without sending anything; no answer
//! within [`PONG_TIMEOUT`] ends the session. Each accepted request gets exactly one
//! `ActionFinished`: the agent's result, or `timeout` / `connection_lost` /
//! `not_connected` from here. No polling.
//!
//! Network hints (issue #7, [`super::netwatch`]): while a retry is pending, a network
//! change (carrier up, new IPv4 address or route) pulls the attempt forward to
//! [`NET_SETTLE`] from now and restarts the backoff at 2 s, so a re-plugged cable does not
//! wait out a 60 s timer. Bounds: automatic attempts start at least [`MIN_GAP`] apart, a
//! hint takes effect at most once per [`NET_COOLDOWN`], hints are ignored unless the
//! user wants the link and the failure is transient, and only when the kernel has a
//! route to the configured address. Manual Connect ("Retry now") is not rate limited.
//!
//! Intent: `Connect` sets `connection_wanted`, `Disconnect` clears it; both are saved
//! for session recovery ([`crate::recovery`]).
//!
//! Stage 4 (protocol 1.1, Core 1.4): every `invoke` carries its request key; a request
//! that ends `timeout` / `connection_lost` is remembered as *uncertain* so the user can
//! `Retry` it with the same key (never automatically). With a 1.1 agent the link
//! subscribes to the live topics after `welcome`, keeps the latest value of each, and
//! clears them all when the session ends (no stale remote state). Notifications are
//! filtered by the phone's preferences (muted apps dropped, DND marks them silent) and
//! relayed without logging their content.

use std::collections::HashMap;
use std::time::Duration;

use pocket_dock_protocol::channel::{self, ChannelError};
use pocket_dock_protocol::message::{ActionInfo, AgentMessage, ClientMessage, Version};
use pocket_dock_protocol::{HANDSHAKE_TIMEOUT, PING_INTERVAL, PONG_TIMEOUT, PROTOCOL, error_code};
use pocket_ipc::DockState;
use serde_json::{Map, Value};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, sleep_until};
use tracing::{debug, info, warn};
use zbus::object_server::SignalEmitter;

use super::config::LinkConfig;
use super::{Cmd, DockService, LiveTopic, SharedState, Uncertain, lock};
use crate::recovery;

/// Time the computer has to confirm an action.
pub const ACTION_TIMEOUT: Duration = Duration::from_secs(15);

/// Automatic attempts never start closer together than this.
pub const MIN_GAP: Duration = Duration::from_secs(2);
/// Delay between a network hint and the attempt it triggers (lets a burst of changes,
/// and the computer's side of a re-plugged USB link, settle).
pub const NET_SETTLE: Duration = Duration::from_secs(1);
/// A network hint shortens the wait at most once per this interval.
pub const NET_COOLDOWN: Duration = Duration::from_secs(10);

/// Phone-side failure codes (in addition to the agent's [`error_code`]s).
pub mod code {
    pub const UNREACHABLE: &str = "unreachable";
    pub const TIMEOUT: &str = "timeout";
    pub const HANDSHAKE_FAILED: &str = "handshake_failed";
    pub const HOST_KEY_MISMATCH: &str = "host_key_mismatch";
    pub const CONNECTION_CLOSED: &str = "connection_closed";
    pub const CONNECTION_ERROR: &str = "connection_error";
    pub const CONNECTION_LOST: &str = "connection_lost";
    pub const NOT_CONNECTED: &str = "not_connected";
}

/// Delay before automatic attempt `n` (0-based): 2, 4, 8, 16, 30, 60, 60, … seconds.
pub fn retry_delay(n: u32) -> Duration {
    const STEPS: [u64; 6] = [2, 4, 8, 16, 30, 60];
    Duration::from_secs(STEPS[(n as usize).min(STEPS.len() - 1)])
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: String,
    pub message: String,
    /// Retry automatically (transient) or wait for the user (needs re-pairing etc.).
    pub retry: bool,
}

impl Failure {
    fn transient(code: &str, message: impl Into<String>) -> Self {
        Failure {
            code: code.into(),
            message: message.into(),
            retry: true,
        }
    }

    /// A fatal `error` from the agent: retry unless it needs the user.
    fn from_agent(code: String, message: String) -> Self {
        let retry = !matches!(
            code.as_str(),
            error_code::UNAUTHORIZED | error_code::INCOMPATIBLE_PROTOCOL
        );
        Failure {
            code,
            message,
            retry,
        }
    }
}

struct Session {
    sender: channel::Sender<TcpStream>,
    receiver: channel::Receiver<TcpStream>,
    agent: String,
    host_name: String,
    actions: Vec<ActionInfo>,
    protocol: Version,
}

/// An accepted request waiting for the computer.
struct Pending {
    action: String,
    params: Map<String, Value>,
    key: String,
    deadline: Instant,
}

/// Outcome codes after which the request may or may not have run.
fn is_uncertain(code: &str) -> bool {
    code == code::TIMEOUT || code == code::CONNECTION_LOST
}

enum Next {
    Idle,
    Attempt(u32),
    Wait(u32, Instant),
    Exit,
}

enum End {
    User,
    Reconnect,
    Lost(Failure),
    Exit,
}

pub struct Link {
    conn: zbus::Connection,
    shared: SharedState,
    cmds: mpsc::UnboundedReceiver<Cmd>,
    cfg: Option<LinkConfig>,
    device_name: String,
    recovery: recovery::Handle,
    net: Option<watch::Receiver<u64>>,
    last_attempt: Option<Instant>,
    last_net_retry: Option<Instant>,
}

impl Link {
    pub fn new(
        conn: zbus::Connection,
        shared: SharedState,
        cmds: mpsc::UnboundedReceiver<Cmd>,
        recovery: recovery::Handle,
        net: watch::Receiver<u64>,
    ) -> Self {
        let device_name = std::fs::read_to_string("/etc/hostname")
            .map(|s| s.trim().to_owned())
            .unwrap_or_default();
        Link {
            conn,
            shared,
            cmds,
            cfg: None,
            device_name,
            recovery,
            net: Some(net),
            last_attempt: None,
            last_net_retry: None,
        }
    }

    pub async fn run(mut self) {
        let mut next = Next::Idle;
        loop {
            next = match next {
                Next::Exit => return,
                Next::Idle => match self.cmds.recv().await {
                    None => Next::Exit,
                    Some(Cmd::Connect(cfg)) => {
                        self.cfg = Some(*cfg);
                        self.set_wanted(true);
                        Next::Attempt(0)
                    }
                    Some(Cmd::Disconnect) => {
                        self.set_disconnected().await;
                        Next::Idle
                    }
                    Some(Cmd::Execute {
                        request, action, ..
                    }) => {
                        self.reject(request, &action).await;
                        Next::Idle
                    }
                },
                Next::Wait(n, at) => tokio::select! {
                    () = sleep_until(at) => Next::Attempt(n),
                    () = net_changed(&mut self.net) => {
                        match self.network_hint(at).await {
                            Some(sooner) => Next::Wait(0, sooner),
                            None => Next::Wait(n, at),
                        }
                    }
                    c = self.cmds.recv() => match c {
                        None => Next::Exit,
                        Some(Cmd::Connect(cfg)) => {
                            info!("dock: Connect while waiting; retrying now");
                            self.cfg = Some(*cfg);
                            self.set_wanted(true);
                            Next::Attempt(0)
                        }
                        Some(Cmd::Disconnect) => {
                            self.set_disconnected().await;
                            Next::Idle
                        }
                        Some(Cmd::Execute { request, action, .. }) => {
                            self.reject(request, &action).await;
                            Next::Wait(n, at)
                        }
                    },
                },
                Next::Attempt(n) => self.attempt(n).await,
            };
        }
    }

    async fn attempt(&mut self, n: u32) -> Next {
        let Some(cfg) = self.cfg.clone() else {
            return Next::Idle;
        };
        info!(host = %cfg.host.name, address = %cfg.host.address, attempt = n, "dock: connecting");
        self.last_attempt = Some(Instant::now());
        self.update(|s| {
            s.set_host(&cfg.host);
            s.state = DockState::Connecting;
            s.retry_at = None;
        })
        .await;
        let device_name = self.device_name.clone();
        let fut = establish(&cfg, &device_name);
        tokio::pin!(fut);
        let result = loop {
            tokio::select! {
                r = &mut fut => break r,
                c = self.cmds.recv() => match c {
                    None => return Next::Exit,
                    Some(Cmd::Connect(c)) => self.cfg = Some(*c), // used by the next attempt; already wanted
                    Some(Cmd::Disconnect) => {
                        self.set_disconnected().await;
                        return Next::Idle;
                    }
                    Some(Cmd::Execute { request, action, .. }) => self.reject(request, &action).await,
                },
            }
        };
        match result {
            Ok(session) => match self.session(session).await {
                End::User => {
                    self.set_disconnected().await;
                    Next::Idle
                }
                End::Reconnect => Next::Attempt(0),
                End::Exit => Next::Exit,
                End::Lost(f) => self.fail(f, 0).await,
            },
            Err(f) => self.fail(f, n).await,
        }
    }

    async fn fail(&mut self, f: Failure, n: u32) -> Next {
        let delay = f.retry.then(|| retry_delay(n));
        warn!(code = %f.code, error = %f.message, retry_in = ?delay, "dock: link failed");
        let now = Instant::now();
        // Never closer than MIN_GAP to the previous attempt's start (a session that ends
        // right after it began must not turn into a fast loop).
        let at = delay.map(|d| (now + d).max(self.last_attempt.map_or(now, |t| t + MIN_GAP)));
        self.update(|s| {
            s.state = DockState::Error;
            s.error_code = f.code.clone();
            s.error = f.message.clone();
            s.agent.clear();
            s.retry_at = at;
        })
        .await;
        // Only changes from now on count as hints for this wait.
        if let Some(net) = &mut self.net {
            net.borrow_and_update();
        }
        match at {
            Some(at) => Next::Wait(n + 1, at),
            None => Next::Idle,
        }
    }

    /// A network change while a retry is pending: returns the earlier deadline, if the
    /// hint is worth acting on (see the module documentation for the bounds).
    async fn network_hint(&mut self, at: Instant) -> Option<Instant> {
        let now = Instant::now();
        if self.last_net_retry.is_some_and(|t| now < t + NET_COOLDOWN) {
            debug!("dock: network changed; hint ignored (cooldown)");
            return None;
        }
        let addr = self.cfg.as_ref()?.host.address;
        if let Err(e) = route_exists(addr) {
            debug!(%addr, error = %e, "dock: network changed; no route to the computer yet");
            return None;
        }
        let sooner = (now + NET_SETTLE).max(self.last_attempt.map_or(now, |t| t + MIN_GAP));
        if sooner >= at {
            return None;
        }
        self.last_net_retry = Some(now);
        info!(
            %addr,
            was_in_ms = at.saturating_duration_since(now).as_millis() as u64,
            now_in_ms = sooner.saturating_duration_since(now).as_millis() as u64,
            "dock: network changed; retrying sooner"
        );
        self.update(|s| s.retry_at = Some(sooner)).await;
        Some(sooner)
    }

    /// Records the user's intent (shared state and the session recovery file).
    fn set_wanted(&self, wanted: bool) {
        lock(&self.shared).wanted = wanted;
        self.recovery.set_dock_wanted(wanted);
    }

    async fn session(&mut self, s: Session) -> End {
        let Session {
            mut sender,
            mut receiver,
            agent,
            host_name,
            actions,
            protocol,
        } = s;
        let host_key = self.cfg.as_ref().map(|c| c.host.clone());
        info!(%agent, host = %host_name, actions = ?actions.iter().map(|a| &a.id).collect::<Vec<_>>(), "dock: connected");
        self.update(|st| {
            st.state = DockState::Connected;
            st.agent = agent.clone();
            if !host_name.is_empty() {
                st.host_name = host_name.clone();
            }
            st.error_code.clear();
            st.error.clear();
            st.retry_at = None;
            st.actions = actions;
        })
        .await;
        self.publish_actions().await;

        let (in_tx, mut in_rx) = mpsc::channel(16);
        let mut last_tx = Instant::now();
        if protocol.minor >= 1 {
            let topics = pocket_dock_protocol::topic::ALL
                .iter()
                .map(|t| (*t).to_owned())
                .collect();
            if let Err(e) = sender.send(&ClientMessage::Subscribe { topics }).await {
                warn!(error = %e, "dock: cannot subscribe to live state");
            }
            last_tx = Instant::now();
        } else {
            info!(%protocol, "dock: agent predates live state (protocol 1.1)");
        }
        let reader = tokio::spawn(async move {
            loop {
                let m = receiver.recv::<AgentMessage>().await;
                let end = !matches!(m, Ok(Some(_)));
                if in_tx.send(m).await.is_err() || end {
                    break;
                }
            }
        });

        let mut pending: HashMap<u64, Pending> = HashMap::new();
        let mut pong_deadline: Option<Instant> = None;
        let mut ping_id = 0u64;

        let end = loop {
            let mut deadline = pong_deadline.unwrap_or(last_tx + PING_INTERVAL);
            if let Some(d) = pending.values().map(|p| p.deadline).min() {
                deadline = deadline.min(d);
            }
            tokio::select! {
                c = self.cmds.recv() => match c {
                    None => break End::Exit,
                    Some(Cmd::Disconnect) => break End::User,
                    Some(Cmd::Connect(cfg)) => {
                        let changed = host_key.as_ref() != Some(&cfg.host);
                        self.cfg = Some(*cfg);
                        if changed {
                            info!("dock: host configuration changed; reconnecting");
                            break End::Reconnect;
                        }
                    }
                    Some(Cmd::Execute { request, action, params, key }) => {
                        // Keys go only to agents that understand them (1.1).
                        let wire_key = (protocol.minor >= 1).then(|| key.clone());
                        let msg = ClientMessage::Invoke { id: request, action: action.clone(), params: params.clone(), key: wire_key };
                        let p = Pending { action, params, key, deadline: Instant::now() + ACTION_TIMEOUT };
                        if let Err(e) = sender.send(&msg).await {
                            self.finish_pending(request, p, code::CONNECTION_LOST, "the connection failed while sending; the action may or may not have run").await;
                            break End::Lost(Failure::transient(code::CONNECTION_ERROR, e.to_string()));
                        }
                        last_tx = Instant::now();
                        pending.insert(request, p);
                    }
                },
                m = in_rx.recv() => {
                    pong_deadline = None; // any message proves the agent is alive
                    match m {
                        Some(Ok(Some(msg))) => match msg {
                            AgentMessage::Result { id, ok, data, error, duplicate } => {
                                match pending.remove(&id) {
                                    Some(p) => {
                                        if duplicate {
                                            info!(request = id, action = %p.action, "dock: the computer had already run this request (duplicate key)");
                                        }
                                        let r = if ok {
                                            Ok(data)
                                        } else {
                                            let e = error.unwrap_or_else(|| pocket_dock_protocol::message::ErrorInfo::new(error_code::FAILED, "failed"));
                                            Err((e.code, e.message))
                                        };
                                        self.finish_result(id, &p.action, r, duplicate).await;
                                    }
                                    None => debug!(id, "dock: result for an unknown or expired request"),
                                }
                            }
                            AgentMessage::Progress { id, step, total, label } => {
                                if pending.contains_key(&id) {
                                    if let Some(e) = self.emitter() {
                                        let _ = DockService::action_progress(&e, id, step, total, &label).await;
                                    }
                                }
                            }
                            AgentMessage::State { topic, rev, available, reason, interval_ms, data } => {
                                if pocket_dock_protocol::topic::ALL.contains(&topic.as_str()) {
                                    let t = LiveTopic { rev, available, reason, interval_ms, data, received: Instant::now() };
                                    let json = t.json().to_string();
                                    lock(&self.shared).live.insert(topic.clone(), t);
                                    debug!(%topic, rev, available, "dock: live state");
                                    if let Some(e) = self.emitter() {
                                        let _ = DockService::live_changed(&e, &topic, &json).await;
                                    }
                                }
                            }
                            AgentMessage::Notification(n) => self.notification(n).await,
                            AgentMessage::NotificationClosed { nid } => {
                                if let Some(e) = self.emitter() {
                                    let _ = DockService::notification_closed(&e, &nid).await;
                                }
                            }
                            AgentMessage::Actions { actions } => {
                                info!(available = ?actions.iter().filter(|a| a.available).map(|a| &a.id).collect::<Vec<_>>(), "dock: actions changed");
                                self.update(|s| s.actions = actions).await;
                                self.publish_actions().await;
                            }
                            AgentMessage::Error { code, message } => break End::Lost(Failure::from_agent(code, message)),
                            AgentMessage::Pong { .. } | AgentMessage::Welcome { .. } | AgentMessage::Unknown => {}
                        },
                        Some(Ok(None)) | None => break End::Lost(Failure::transient(code::CONNECTION_CLOSED, "the computer closed the connection")),
                        Some(Err(ChannelError::Decode(e))) => break End::Lost(Failure::transient(error_code::PROTOCOL_ERROR, format!("invalid message from the agent: {e}"))),
                        Some(Err(e)) => break End::Lost(Failure::transient(code::CONNECTION_ERROR, e.to_string())),
                    }
                }
                () = sleep_until(deadline) => {
                    let now = Instant::now();
                    if pong_deadline.is_some_and(|d| d <= now) {
                        break End::Lost(Failure::transient(code::TIMEOUT, "the computer stopped answering"));
                    }
                    if pong_deadline.is_none() && last_tx + PING_INTERVAL <= now {
                        ping_id += 1;
                        if let Err(e) = sender.send(&ClientMessage::Ping { id: ping_id }).await {
                            break End::Lost(Failure::transient(code::CONNECTION_ERROR, e.to_string()));
                        }
                        last_tx = now;
                        pong_deadline = Some(now + PONG_TIMEOUT);
                    }
                    let expired: Vec<u64> = pending.iter().filter(|(_, p)| p.deadline <= now).map(|(id, _)| *id).collect();
                    for id in expired {
                        if let Some(p) = pending.remove(&id) {
                            self.finish_pending(id, p, code::TIMEOUT, "the computer did not confirm the action in time; it may or may not have run").await;
                        }
                    }
                }
            }
        };

        sender.close().await;
        reader.abort();
        for (id, p) in pending {
            self.finish_pending(
                id,
                p,
                code::CONNECTION_LOST,
                "the connection was lost before the computer confirmed; the action may or may not have run",
            )
            .await;
        }
        self.update(|s| {
            s.actions.clear();
            s.agent.clear();
        })
        .await;
        self.publish_actions().await;
        // Remote state ends with the session: never show it as current afterwards.
        let topics: Vec<String> = std::mem::take(&mut lock(&self.shared).live)
            .into_keys()
            .collect();
        if let Some(e) = self.emitter() {
            for t in topics {
                let _ = DockService::live_changed(&e, &t, "").await;
            }
        }
        info!("dock: session ended");
        end
    }

    // ── state publication ────────────────────────────────────────────────────────

    fn emitter(&self) -> Option<SignalEmitter<'_>> {
        SignalEmitter::new(&self.conn, pocket_ipc::dbus::dock::OBJECT_PATH).ok()
    }

    /// Applies `f`, bumps the revision and emits `StateChanged`.
    async fn update(&self, f: impl FnOnce(&mut super::Shared)) {
        let dict = {
            let mut s = lock(&self.shared);
            f(&mut s);
            s.revision += 1;
            debug!(state = %s.state, revision = s.revision, "dock: state");
            s.dict()
        };
        if let Some(e) = self.emitter() {
            if let Err(err) = DockService::state_changed(&e, dict).await {
                warn!(error = %err, "dock: cannot emit StateChanged");
            }
        }
    }

    async fn publish_actions(&self) {
        let list = lock(&self.shared).action_dicts();
        if let Some(e) = self.emitter() {
            let _ = DockService::actions_changed(&e, list).await;
        }
    }

    /// Handles `Disconnect` (its only caller): the user no longer wants the link.
    async fn set_disconnected(&self) {
        info!("dock: disconnected");
        self.set_wanted(false);
        self.update(|s| {
            s.state = DockState::Disconnected;
            s.error_code.clear();
            s.error.clear();
            s.retry_at = None;
            s.agent.clear();
            s.actions.clear();
        })
        .await;
    }

    async fn reject(&self, request: u64, action: &str) {
        self.finish(
            request,
            action,
            Err((code::NOT_CONNECTED, "the computer is not connected".into())),
        )
        .await;
    }

    async fn finish(&self, request: u64, action: &str, r: Result<Value, (&str, String)>) {
        self.finish_owned(request, action, r.map_err(|(c, m)| (c.to_owned(), m)))
            .await;
    }

    /// Ends a pending request with a phone-side failure; an uncertain outcome is kept
    /// for `Retry`.
    async fn finish_pending(&self, request: u64, p: Pending, code: &str, message: &str) {
        if is_uncertain(code) {
            lock(&self.shared).remember_uncertain(
                request,
                Uncertain {
                    action: p.action.clone(),
                    params: p.params,
                    key: p.key,
                    at: Instant::now(),
                },
            );
        }
        self.finish(request, &p.action, Err((code, message.to_owned())))
            .await;
    }

    async fn notification(&self, mut n: pocket_dock_protocol::message::Notification) {
        let (muted, dnd) = {
            let s = lock(&self.shared);
            (s.prefs.is_muted(&n.app), s.prefs.dnd)
        };
        if muted {
            debug!(nid = %n.nid, "dock: notification from a muted application dropped");
            return;
        }
        n.summary = n.summary.chars().take(200).collect();
        n.body = n.body.chars().take(500).collect();
        let mut v = serde_json::to_value(&n).unwrap_or_default();
        if let Some(o) = v.as_object_mut() {
            o.remove("type");
            o.insert("silent".into(), Value::from(dnd));
        }
        // No content in the log: notification text is private.
        info!(nid = %n.nid, silent = dnd, redacted = n.redacted, "dock: notification");
        if let Some(e) = self.emitter() {
            let _ = DockService::notification(&e, &v.to_string()).await;
        }
    }

    async fn finish_result(
        &self,
        request: u64,
        action: &str,
        r: Result<Value, (String, String)>,
        duplicate: bool,
    ) {
        self.finish_inner(request, action, r, duplicate).await
    }

    async fn finish_owned(&self, request: u64, action: &str, r: Result<Value, (String, String)>) {
        self.finish_inner(request, action, r, false).await
    }

    async fn finish_inner(
        &self,
        request: u64,
        action: &str,
        r: Result<Value, (String, String)>,
        duplicate: bool,
    ) {
        let (ok, code, message, data) = match r {
            Ok(d) => (
                true,
                String::new(),
                if duplicate {
                    "duplicate".to_owned()
                } else {
                    String::new()
                },
                d,
            ),
            Err((c, m)) => (false, c, m, Value::Null),
        };
        if ok {
            info!(request, %action, data = %data, "dock: action confirmed by the computer");
        } else {
            warn!(request, %action, %code, %message, "dock: action failed");
        }
        if let Some(e) = self.emitter() {
            let _ = DockService::action_finished(
                &e,
                request,
                action,
                ok,
                &code,
                &message,
                &data.to_string(),
            )
            .await;
        }
    }
}

/// Resolves on the next network hint; never, if there is no watcher.
async fn net_changed(net: &mut Option<watch::Receiver<u64>>) {
    if let Some(rx) = net {
        if rx.changed().await.is_ok() {
            return;
        }
        *net = None; // watcher gone: fall back to the backoff alone
    }
    std::future::pending::<()>().await
}

/// Asks the kernel for a route to `addr` (a UDP "connect" sends nothing).
fn route_exists(addr: std::net::SocketAddr) -> std::io::Result<()> {
    let any: std::net::SocketAddr = if addr.is_ipv4() {
        (std::net::Ipv4Addr::UNSPECIFIED, 0).into()
    } else {
        (std::net::Ipv6Addr::UNSPECIFIED, 0).into()
    };
    std::net::UdpSocket::bind(any)?.connect(addr)
}

/// Connect + handshake + pin check + hello/welcome, within [`HANDSHAKE_TIMEOUT`].
async fn establish(cfg: &LinkConfig, device_name: &str) -> Result<Session, Failure> {
    let addr = cfg.host.address;
    let work = async {
        let stream = TcpStream::connect(addr).await.map_err(|e| {
            Failure::transient(code::UNREACHABLE, format!("cannot reach {addr}: {e}"))
        })?;
        let _ = stream.set_nodelay(true);
        let est = channel::initiate(stream, &cfg.identity)
            .await
            .map_err(|e| Failure::transient(code::HANDSHAKE_FAILED, format!("{addr}: {e}")))?;
        if est.remote != cfg.host.key {
            return Err(Failure {
                code: code::HOST_KEY_MISMATCH.into(),
                message: format!(
                    "{addr} presented key {} but {} ({}) is trusted; re-pair if the computer was reinstalled",
                    est.remote.short_id(),
                    cfg.host.name,
                    cfg.host.key.short_id()
                ),
                retry: false,
            });
        }
        let (mut sender, mut receiver) = (est.sender, est.receiver);
        sender
            .send(&ClientMessage::Hello {
                protocol: PROTOCOL,
                client: format!("pocketd {}", pocket_core::VERSION),
                device_name: device_name.to_owned(),
            })
            .await
            .map_err(|e| Failure::transient(code::CONNECTION_ERROR, e.to_string()))?;
        match receiver.recv::<AgentMessage>().await {
            Ok(Some(AgentMessage::Welcome {
                protocol,
                agent,
                host_name,
                actions,
            })) => {
                if protocol.major != PROTOCOL.major {
                    return Err(Failure {
                        code: error_code::INCOMPATIBLE_PROTOCOL.into(),
                        message: format!(
                            "agent speaks Pocket Dock {protocol}, the phone {PROTOCOL}"
                        ),
                        retry: false,
                    });
                }
                let actions = actions
                    .into_iter()
                    .filter(|a| pocket_dock_protocol::is_valid_action_id(&a.id))
                    .collect();
                Ok(Session {
                    sender,
                    receiver,
                    agent,
                    host_name,
                    actions,
                    protocol,
                })
            }
            Ok(Some(AgentMessage::Error { code, message })) => {
                Err(Failure::from_agent(code, message))
            }
            Ok(Some(other)) => Err(Failure::transient(
                error_code::PROTOCOL_ERROR,
                format!("expected welcome, got {other:?}"),
            )),
            Ok(None) => Err(Failure::transient(
                code::CONNECTION_CLOSED,
                "the agent closed the connection during the handshake",
            )),
            Err(e) => Err(Failure::transient(code::CONNECTION_ERROR, e.to_string())),
        }
    };
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, work).await {
        Ok(r) => r,
        Err(_) => Err(Failure::transient(
            code::TIMEOUT,
            format!(
                "no answer from {addr} within {} s",
                HANDSHAKE_TIMEOUT.as_secs()
            ),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_bounded_and_never_fast() {
        let d: Vec<u64> = (0..9).map(|n| retry_delay(n).as_secs()).collect();
        assert_eq!(d, [2, 4, 8, 16, 30, 60, 60, 60, 60]);
    }

    #[test]
    fn user_action_failures_are_not_retried() {
        assert!(!Failure::from_agent("unauthorized".into(), String::new()).retry);
        assert!(!Failure::from_agent("incompatible_protocol".into(), String::new()).retry);
        assert!(Failure::from_agent("replaced".into(), String::new()).retry);
        assert!(Failure::from_agent("protocol_error".into(), String::new()).retry);
    }
}
