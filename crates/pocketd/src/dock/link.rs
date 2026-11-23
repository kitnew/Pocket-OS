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
//! `not_connected` from here. No polling and no retries faster than 2 s.

use std::collections::HashMap;
use std::time::Duration;

use pocket_dock_protocol::channel::{self, ChannelError};
use pocket_dock_protocol::message::{ActionInfo, AgentMessage, ClientMessage};
use pocket_dock_protocol::{HANDSHAKE_TIMEOUT, PING_INTERVAL, PONG_TIMEOUT, PROTOCOL, error_code};
use pocket_ipc::DockState;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until};
use tracing::{debug, info, warn};
use zbus::object_server::SignalEmitter;

use super::config::LinkConfig;
use super::{Cmd, DockService, SharedState, lock};

/// Time the computer has to confirm an action.
pub const ACTION_TIMEOUT: Duration = Duration::from_secs(15);

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
}

impl Link {
    pub fn new(
        conn: zbus::Connection,
        shared: SharedState,
        cmds: mpsc::UnboundedReceiver<Cmd>,
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
                    c = self.cmds.recv() => match c {
                        None => Next::Exit,
                        Some(Cmd::Connect(cfg)) => {
                            info!("dock: Connect while waiting; retrying now");
                            self.cfg = Some(*cfg);
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
        self.update(|s| {
            s.set_host(&cfg.host);
            s.state = DockState::Connecting;
            s.retry_in = 0;
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
                    Some(Cmd::Connect(c)) => self.cfg = Some(*c), // used by the next attempt
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
        self.update(|s| {
            s.state = DockState::Error;
            s.error_code = f.code.clone();
            s.error = f.message.clone();
            s.agent.clear();
            s.retry_in = delay.map_or(0, |d| d.as_secs() as u32);
        })
        .await;
        match delay {
            Some(d) => Next::Wait(n + 1, Instant::now() + d),
            None => Next::Idle,
        }
    }

    async fn session(&mut self, s: Session) -> End {
        let Session {
            mut sender,
            mut receiver,
            agent,
            host_name,
            actions,
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
            st.retry_in = 0;
            st.actions = actions;
        })
        .await;
        self.publish_actions().await;

        let (in_tx, mut in_rx) = mpsc::channel(16);
        let reader = tokio::spawn(async move {
            loop {
                let m = receiver.recv::<AgentMessage>().await;
                let end = !matches!(m, Ok(Some(_)));
                if in_tx.send(m).await.is_err() || end {
                    break;
                }
            }
        });

        let mut pending: HashMap<u64, (String, Instant)> = HashMap::new();
        let mut last_tx = Instant::now();
        let mut pong_deadline: Option<Instant> = None;
        let mut ping_id = 0u64;

        let end = loop {
            let mut deadline = pong_deadline.unwrap_or(last_tx + PING_INTERVAL);
            if let Some(d) = pending.values().map(|(_, d)| *d).min() {
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
                    Some(Cmd::Execute { request, action, params }) => {
                        let msg = ClientMessage::Invoke { id: request, action: action.clone(), params };
                        if let Err(e) = sender.send(&msg).await {
                            self.finish(request, &action, Err((code::CONNECTION_LOST, "the connection failed while sending".into()))).await;
                            break End::Lost(Failure::transient(code::CONNECTION_ERROR, e.to_string()));
                        }
                        last_tx = Instant::now();
                        pending.insert(request, (action, Instant::now() + ACTION_TIMEOUT));
                    }
                },
                m = in_rx.recv() => {
                    pong_deadline = None; // any message proves the agent is alive
                    match m {
                        Some(Ok(Some(msg))) => match msg {
                            AgentMessage::Result { id, ok, data, error } => {
                                match pending.remove(&id) {
                                    Some((action, _)) => {
                                        let r = if ok {
                                            Ok(data)
                                        } else {
                                            let e = error.unwrap_or_else(|| pocket_dock_protocol::message::ErrorInfo::new(error_code::FAILED, "failed"));
                                            Err((e.code, e.message))
                                        };
                                        self.finish_owned(id, &action, r).await;
                                    }
                                    None => debug!(id, "dock: result for an unknown or expired request"),
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
                    let expired: Vec<u64> = pending.iter().filter(|(_, (_, d))| *d <= now).map(|(id, _)| *id).collect();
                    for id in expired {
                        if let Some((action, _)) = pending.remove(&id) {
                            self.finish(id, &action, Err((code::TIMEOUT, "the computer did not confirm the action in time".into()))).await;
                        }
                    }
                }
            }
        };

        sender.close().await;
        reader.abort();
        for (id, (action, _)) in pending {
            self.finish(
                id,
                &action,
                Err((
                    code::CONNECTION_LOST,
                    "the connection was lost before the computer confirmed; the action may or may not have run".into(),
                )),
            )
            .await;
        }
        self.update(|s| {
            s.actions.clear();
            s.agent.clear();
        })
        .await;
        self.publish_actions().await;
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

    async fn set_disconnected(&self) {
        info!("dock: disconnected");
        self.update(|s| {
            s.state = DockState::Disconnected;
            s.error_code.clear();
            s.error.clear();
            s.retry_in = 0;
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

    async fn finish_owned(&self, request: u64, action: &str, r: Result<Value, (String, String)>) {
        let (ok, code, message, data) = match r {
            Ok(d) => (true, String::new(), String::new(), d),
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
