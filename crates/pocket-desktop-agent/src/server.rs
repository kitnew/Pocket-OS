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
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

use crate::actions::Registry;

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
        }
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
            self.session_loop(&name, session, sender, receiver, &mut replaced_rx)
                .await;
        }

        if let Ok(mut s) = self.sessions.lock() {
            if s.get(&remote).is_some_and(|(id, _)| *id == session) {
                s.remove(&remote);
            }
        }
        info!(phone = %name, session, "phone disconnected");
    }

    async fn session_loop(
        &self,
        name: &str,
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
                m = in_rx.recv() => match m {
                    Some(Ok(Some(msg))) => {
                        idle.as_mut().reset(tokio::time::Instant::now() + self.idle_timeout);
                        match msg {
                            ClientMessage::Ping { id } => {
                                if sender.send(&AgentMessage::Pong { id }).await.is_err() {
                                    break None;
                                }
                            }
                            ClientMessage::Invoke { id, action, params } => {
                                let registry = self.registry.clone();
                                let out = out_tx.clone();
                                let name = name.to_owned();
                                tokio::spawn(async move {
                                    let started = Instant::now();
                                    let r = registry.invoke(&action, &params).await;
                                    let ms = started.elapsed().as_millis() as u64;
                                    let msg = match r {
                                        Ok(data) => {
                                            info!(phone = %name, session, id, %action, ms, "action ok");
                                            AgentMessage::Result { id, ok: true, data, error: None }
                                        }
                                        Err(e) => {
                                            warn!(phone = %name, session, id, %action, ms, code = %e.code, error = %e.message, "action failed");
                                            AgentMessage::Result { id, ok: false, data: Value::Null, error: Some(e) }
                                        }
                                    };
                                    let _ = out.send(msg).await;
                                });
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
