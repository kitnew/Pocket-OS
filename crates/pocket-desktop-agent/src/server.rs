//! Connection handling: Noise handshake → trust check → `hello`/`welcome` → session loop.
//!
//! - Untrusted static key: one fatal `error unauthorized`, then close. No action is ever
//!   reachable before the peer is authenticated *and* trusted.
//! - Protocol major mismatch: fatal `error incompatible_protocol`.
//! - One session per trusted phone: a new authenticated session replaces the old one
//!   (fatal `error replaced` on the old), so a phone that lost a half-open connection can
//!   reconnect immediately.
//! - Silence for `idle_timeout` closes the session (the phone pings every 20 s).
//! - Every `invoke` runs in its own task and gets exactly one `result` once the action
//!   finished.
//! - Request keys (1.1): results are cached per (phone, key) for [`DEDUP_TTL`] across
//!   sessions. A repeated key is answered with the stored result (`duplicate: true`),
//!   or waits for the original if it is still running; it never runs twice.
//! - `subscribe` (1.1): one `state` snapshot per topic, then the topic again whenever it
//!   changed (coalesced for [`COALESCE`]); laptop notifications when subscribed to
//!   `notifications`.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use pocket_dock_protocol::channel::{self, ChannelError, Established};
use pocket_dock_protocol::keys::{Identity, PublicKey};
use pocket_dock_protocol::message::{AgentMessage, ClientMessage, Version};
use pocket_dock_protocol::{HANDSHAKE_TIMEOUT, IDLE_TIMEOUT, PROTOCOL, error_code};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tracing::{debug, info, warn};

use crate::actions::Registry;
use crate::live::Live;

/// How long a request key's result is kept for duplicate detection.
pub const DEDUP_TTL: Duration = Duration::from_secs(600);
/// At most this many keys are kept (oldest dropped first).
pub const DEDUP_MAX: usize = 512;
/// Topic changes within this window are sent as one update.
pub const COALESCE: Duration = Duration::from_millis(100);

type Outcome = (
    bool,
    Value,
    Option<pocket_dock_protocol::message::ErrorInfo>,
);

struct DedupEntry {
    at: Instant,
    outcome: watch::Receiver<Option<Outcome>>,
}

enum Claim {
    /// First time: run it and publish the outcome here.
    Run(watch::Sender<Option<Outcome>>),
    /// Seen before: wait for (or take) the stored outcome.
    Duplicate(watch::Receiver<Option<Outcome>>),
}

pub struct Server {
    identity: Identity,
    trusted: Vec<(String, PublicKey)>,
    host_name: String,
    registry: Arc<Registry>,
    /// Protocol spoken by this agent (tests override it to check negotiation).
    pub protocol: Version,
    pub idle_timeout: Duration,
    sessions: Mutex<HashMap<PublicKey, (u64, oneshot::Sender<()>)>>,
    next_session: AtomicU64,
    dedup: Mutex<HashMap<(PublicKey, String), DedupEntry>>,
    /// Live topics served to subscribed phones.
    pub live: Live,
    /// Forwarded notifications (see `crate::notifications`).
    pub notifications: broadcast::Sender<AgentMessage>,
}

impl Server {
    pub fn new(
        identity: Identity,
        trusted: Vec<(String, PublicKey)>,
        host_name: String,
        registry: Arc<Registry>,
    ) -> Self {
        Server {
            identity,
            trusted,
            host_name,
            registry,
            protocol: PROTOCOL,
            idle_timeout: IDLE_TIMEOUT,
            sessions: Mutex::new(HashMap::new()),
            next_session: AtomicU64::new(1),
            dedup: Mutex::new(HashMap::new()),
            live: Live::new(),
            notifications: broadcast::channel(64).0,
        }
    }

    /// Registers a request key (see the module documentation).
    fn claim(&self, phone: PublicKey, key: &str) -> Claim {
        let mut d = self.dedup.lock().unwrap_or_else(|p| p.into_inner());
        d.retain(|_, e| e.at.elapsed() < DEDUP_TTL);
        let k = (phone, key.to_owned());
        if let Some(e) = d.get(&k) {
            return Claim::Duplicate(e.outcome.clone());
        }
        while d.len() >= DEDUP_MAX {
            let Some(oldest) = d.iter().min_by_key(|(_, e)| e.at).map(|(k, _)| k.clone()) else {
                break;
            };
            d.remove(&oldest);
        }
        let (tx, rx) = watch::channel(None);
        d.insert(
            k,
            DedupEntry {
                at: Instant::now(),
                outcome: rx,
            },
        );
        Claim::Run(tx)
    }

    /// Accepts connections until the listener fails. Run it in a `select!` with the
    /// shutdown signal.
    pub async fn serve(self: Arc<Self>, listener: TcpListener) -> std::io::Result<()> {
        loop {
            let (stream, addr) = listener.accept().await?;
            let this = self.clone();
            tokio::spawn(async move { this.handle(stream, addr).await });
        }
    }

    async fn handle(self: Arc<Self>, stream: TcpStream, addr: SocketAddr) {
        let _ = stream.set_nodelay(true);
        let est =
            match tokio::time::timeout(HANDSHAKE_TIMEOUT, channel::respond(stream, &self.identity))
                .await
            {
                Ok(Ok(est)) => est,
                Ok(Err(e)) => {
                    warn!(peer = %addr, error = %e, "handshake failed");
                    return;
                }
                Err(_) => {
                    warn!(peer = %addr, "handshake timed out");
                    return;
                }
            };
        let Established {
            mut sender,
            mut receiver,
            remote,
        } = est;

        let Some(name) = self
            .trusted
            .iter()
            .find(|(_, k)| *k == remote)
            .map(|(n, _)| n.clone())
        else {
            warn!(peer = %addr, key = %remote, "rejected: phone key is not trusted");
            let _ = sender
                .send(&AgentMessage::Error {
                    code: error_code::UNAUTHORIZED.into(),
                    message: format!(
                        "this phone is not trusted by {}; on the computer run: pocket-desktop-agent trust <name> {remote}",
                        self.host_name
                    ),
                })
                .await;
            sender.close().await;
            return;
        };

        let hello = tokio::time::timeout(HANDSHAKE_TIMEOUT, receiver.recv::<ClientMessage>()).await;
        let (protocol, client, device) = match hello {
            Ok(Ok(Some(ClientMessage::Hello {
                protocol,
                client,
                device_name,
            }))) => (protocol, client, device_name),
            other => {
                warn!(peer = %addr, phone = %name, result = ?other.map(|r| r.map(|m| m.map(|_| "unexpected message"))), "no hello");
                let _ = sender
                    .send(&AgentMessage::Error {
                        code: error_code::PROTOCOL_ERROR.into(),
                        message: "expected hello".into(),
                    })
                    .await;
                sender.close().await;
                return;
            }
        };
        if protocol.major != self.protocol.major {
            warn!(phone = %name, client_protocol = %protocol, agent_protocol = %self.protocol, "incompatible protocol");
            let _ = sender
                .send(&AgentMessage::Error {
                    code: error_code::INCOMPATIBLE_PROTOCOL.into(),
                    message: format!(
                        "phone speaks Pocket Dock {protocol}, this agent speaks {}",
                        self.protocol
                    ),
                })
                .await;
            sender.close().await;
            return;
        }

        let session = self.next_session.fetch_add(1, Ordering::Relaxed);
        let (replaced_tx, mut replaced_rx) = oneshot::channel();
        if let Some((old, tx)) = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut s| s.insert(remote, (session, replaced_tx)))
        {
            info!(phone = %name, old_session = old, "replacing previous session");
            let _ = tx.send(());
        }
        info!(phone = %name, session, peer = %addr, %client, device = %device, %protocol, "phone connected");

        let welcome = AgentMessage::Welcome {
            protocol: self.protocol,
            agent: format!("pocket-desktop-agent {}", env!("CARGO_PKG_VERSION")),
            host_name: self.host_name.clone(),
            actions: self.registry.list(),
        };
        if sender.send(&welcome).await.is_ok() {
            self.session_loop(&name, remote, session, sender, receiver, &mut replaced_rx)
                .await;
        }

        if let Ok(mut s) = self.sessions.lock() {
            if s.get(&remote).is_some_and(|(id, _)| *id == session) {
                s.remove(&remote);
            }
        }
        info!(phone = %name, session, "phone disconnected");
    }

    #[allow(clippy::too_many_arguments)]
    async fn session_loop(
        &self,
        name: &str,
        phone: PublicKey,
        session: u64,
        mut sender: channel::Sender<TcpStream>,
        mut receiver: channel::Receiver<TcpStream>,
        replaced: &mut oneshot::Receiver<()>,
    ) {
        // The receiver is not cancel-safe: read in a task, select on its channel.
        let (in_tx, mut in_rx) = mpsc::channel(16);
        let reader = tokio::spawn(async move {
            loop {
                let m = receiver.recv::<ClientMessage>().await;
                let end = !matches!(m, Ok(Some(_)));
                if in_tx.send(m).await.is_err() || end {
                    break;
                }
            }
        });
        let (out_tx, mut out_rx) = mpsc::channel::<AgentMessage>(16);
        let mut changes = self.registry.subscribe();
        changes.mark_unchanged();
        let idle = tokio::time::sleep(self.idle_timeout);
        tokio::pin!(idle);
        // Live state (after `subscribe`): topics, revision last sent per topic.
        let mut topics: Vec<String> = Vec::new();
        let mut sent: HashMap<String, u64> = HashMap::new();
        let mut _subscriber = None;
        let mut live_rx = self.live.changes();
        let mut flush_at: Option<tokio::time::Instant> = None;
        let mut notes: Option<broadcast::Receiver<AgentMessage>> = None;

        let fatal = loop {
            tokio::select! {
                _ = &mut *replaced => break Some((error_code::REPLACED, "a newer session from this phone replaced this one")),
                () = &mut idle => {
                    info!(phone = %name, session, "idle timeout");
                    break None;
                }
                Ok(()) = changes.changed() => {
                    let actions = self.registry.list();
                    info!(phone = %name, session, available = ?actions.iter().filter(|a| a.available).map(|a| &a.id).collect::<Vec<_>>(), "actions changed");
                    if sender.send(&AgentMessage::Actions { actions }).await.is_err() {
                        break None;
                    }
                }
                Some(out) = out_rx.recv() => {
                    if sender.send(&out).await.is_err() {
                        break None;
                    }
                }
                Ok(()) = live_rx.changed(), if !topics.is_empty() => {
                    flush_at.get_or_insert(tokio::time::Instant::now() + COALESCE);
                }
                () = async { tokio::time::sleep_until(flush_at.unwrap_or_else(tokio::time::Instant::now)).await }, if flush_at.is_some() => {
                    flush_at = None;
                    live_rx.borrow_and_update();
                    let mut failed = false;
                    for t in &topics {
                        let (rev, msg) = self.live.message(t);
                        if sent.get(t).copied() != Some(rev) {
                            sent.insert(t.clone(), rev);
                            if sender.send(&msg).await.is_err() {
                                failed = true;
                                break;
                            }
                        }
                    }
                    if failed {
                        break None;
                    }
                }
                n = async { match notes.as_mut() { Some(r) => r.recv().await, None => std::future::pending().await } } => {
                    match n {
                        Ok(msg) => {
                            if sender.send(&msg).await.is_err() {
                                break None;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(missed)) => warn!(phone = %name, session, missed, "notifications dropped (phone too slow)"),
                        Err(broadcast::error::RecvError::Closed) => notes = None,
                    }
                }
                m = in_rx.recv() => match m {
                    Some(Ok(Some(msg))) => {
                        idle.as_mut().reset(tokio::time::Instant::now() + self.idle_timeout);
                        match msg {
                            ClientMessage::Ping { id } => {
                                if sender.send(&AgentMessage::Pong { id }).await.is_err() {
                                    break None;
                                }
                            }
                            ClientMessage::Invoke { id, action, params, key } => {
                                let key = key.filter(|k| pocket_dock_protocol::is_valid_request_key(k));
                                let claim = key.as_deref().map(|k| self.claim(phone, k));
                                let registry = self.registry.clone();
                                let out = out_tx.clone();
                                let name = name.to_owned();
                                tokio::spawn(async move {
                                    if let Some(Claim::Duplicate(mut rx)) = claim {
                                        // Never run twice: wait for (or take) the stored outcome.
                                        let outcome = loop {
                                            if let Some(o) = rx.borrow_and_update().clone() {
                                                break Some(o);
                                            }
                                            if rx.changed().await.is_err() {
                                                break None;
                                            }
                                        };
                                        info!(phone = %name, session, id, %action, "duplicate request key: not run again");
                                        let (ok, data, error) = outcome.unwrap_or((false, Value::Null, Some(pocket_dock_protocol::message::ErrorInfo::new(error_code::FAILED, "the original request was abandoned"))));
                                        let _ = out.send(AgentMessage::Result { id, ok, data, error, duplicate: true }).await;
                                        return;
                                    }
                                    let (ptx, mut prx) = mpsc::unbounded_channel();
                                    let pout = out.clone();
                                    let progress = tokio::spawn(async move {
                                        while let Some((step, total, label)) = prx.recv().await {
                                            let _ = pout.send(AgentMessage::Progress { id, step, total, label }).await;
                                        }
                                    });
                                    let started = Instant::now();
                                    let r = registry.invoke_with_progress(&action, &params, Some(&ptx)).await;
                                    drop(ptx);
                                    let _ = progress.await; // progress is sent before the result
                                    let ms = started.elapsed().as_millis() as u64;
                                    let outcome: Outcome = match r {
                                        Ok(data) => {
                                            info!(phone = %name, session, id, %action, ms, "action ok");
                                            (true, data, None)
                                        }
                                        Err(e) => {
                                            warn!(phone = %name, session, id, %action, ms, code = %e.code, error = %e.message, "action failed");
                                            (false, Value::Null, Some(e))
                                        }
                                    };
                                    if let Some(Claim::Run(tx)) = claim {
                                        let _ = tx.send(Some(outcome.clone()));
                                    }
                                    let (ok, data, error) = outcome;
                                    let _ = out.send(AgentMessage::Result { id, ok, data, error, duplicate: false }).await;
                                });
                            }
                            ClientMessage::Subscribe { topics: wanted } => {
                                let known: Vec<String> = wanted.into_iter()
                                    .filter(|t| pocket_dock_protocol::topic::ALL.contains(&t.as_str()))
                                    .collect();
                                debug!(phone = %name, session, topics = ?known, "subscribe");
                                if known.iter().any(|t| t == pocket_dock_protocol::topic::NOTIFICATIONS) {
                                    notes = Some(self.notifications.subscribe());
                                }
                                if known.iter().any(|t| t == pocket_dock_protocol::topic::SYSTEM) && _subscriber.is_none() {
                                    _subscriber = Some(self.live.subscriber());
                                }
                                live_rx.borrow_and_update();
                                let mut failed = false;
                                for t in &known {
                                    let (rev, msg) = self.live.message(t);
                                    sent.insert(t.clone(), rev);
                                    if sender.send(&msg).await.is_err() {
                                        failed = true;
                                        break;
                                    }
                                }
                                topics = known;
                                if failed {
                                    break None;
                                }
                            }
                            ClientMessage::Hello { .. } => break Some((error_code::PROTOCOL_ERROR, "duplicate hello")),
                            ClientMessage::Unknown => warn!(phone = %name, session, "ignoring unknown message type"),
                        }
                    }
                    Some(Ok(None)) | None => break None,
                    Some(Err(ChannelError::Decode(e))) => {
                        warn!(phone = %name, session, error = %e, "malformed message");
                        break Some((error_code::PROTOCOL_ERROR, "malformed message"));
                    }
                    Some(Err(e)) => {
                        warn!(phone = %name, session, error = %e, "connection error");
                        break None;
                    }
                },
            }
        };
        if let Some((code, message)) = fatal {
            let _ = sender
                .send(&AgentMessage::Error {
                    code: code.into(),
                    message: message.into(),
                })
                .await;
        }
        sender.close().await;
        reader.abort();
    }
}
