//! `access.request` (basic; Stage 4.5): the phone asks the laptop owner to allow one
//! elevated action. The agent shows a desktop notification with **Allow** / **Deny** on
//! the laptop; only an *Allow* clicked there adds the grant (saved to config.json and
//! applied immediately). The phone can never grant anything itself: it can only ask,
//! and the answer comes from the laptop's own notification server, which only the
//! person at the laptop can click (ADR-0008, amended by ADR-0009).
//!
//! The request returns as soon as the notification is shown (`{"status": "asked"}`);
//! the phone learns the outcome from the next action advertisement. One request may be
//! open at a time; a denied action cannot be asked for again for a minute, so a program
//! on the phone cannot flood the laptop with prompts.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use pocket_dock_protocol::error_code;
use pocket_dock_protocol::message::ErrorInfo;
use serde_json::{Value, json};
use tokio::sync::watch;
use tracing::{info, warn};
use zbus::Variant;

use super::Policy;
use crate::config::Config;

pub const ID: &str = "access.request";
/// How long the laptop prompt waits for an answer.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(120);
/// After a Deny, the same action cannot be requested again for this long.
pub const DENY_COOLDOWN: Duration = Duration::from_secs(60);

/// What `access.request` needs from the agent.
#[derive(Clone)]
pub struct AccessConfig {
    /// The desktop session bus (the notification server).
    pub bus: zbus::Connection,
    /// config.json, updated when the owner allows (None: the grant lasts until restart).
    pub config_path: Option<PathBuf>,
    /// This computer's name, for the prompt.
    pub host_name: String,
}

#[derive(Default)]
struct State {
    /// The action whose prompt is open, and since when.
    open: Option<(String, Instant)>,
    denied: HashMap<String, Instant>,
}

pub struct Access {
    cfg: AccessConfig,
    state: Arc<Mutex<State>>,
}

impl Access {
    pub fn new(cfg: AccessConfig) -> Self {
        Access {
            cfg,
            state: Arc::default(),
        }
    }

    /// Why a request for `id` cannot be made right now.
    fn refused(&self, id: &str) -> Option<String> {
        let s = self.state.lock().ok()?;
        if let Some((open, since)) = &s.open
            && since.elapsed() < ANSWER_TIMEOUT
        {
            return Some(if open == id {
                "already waiting for an answer on the computer".into()
            } else {
                "another request is waiting for an answer on the computer".into()
            });
        }
        match s.denied.get(id) {
            Some(t) if t.elapsed() < DENY_COOLDOWN => {
                Some("denied on the computer; try again in a minute".into())
            }
            _ => None,
        }
    }

    /// Shows the prompt and returns; the answer is handled in the background.
    pub async fn request(
        &self,
        id: &str,
        label: &str,
        policy: Arc<RwLock<Policy>>,
        changes: Arc<watch::Sender<u64>>,
    ) -> Result<Value, ErrorInfo> {
        if let Some(r) = self.refused(id) {
            return Err(ErrorInfo::new(error_code::UNAVAILABLE, r));
        }
        let bus = self.cfg.bus.clone();
        // Subscribe before showing the prompt so a fast click cannot be missed.
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.Notifications")
            .build()
            .map_err(|e| ErrorInfo::new(error_code::FAILED, e.to_string()))?;
        let mut stream =
            zbus::MessageStream::for_match_rule(rule, &bus, std::num::NonZeroUsize::new(32))
                .await
                .map_err(|e| {
                    ErrorInfo::new(error_code::FAILED, format!("notification signals: {e}"))
                })?;
        let body = format!(
            "Your phone asks to use <b>{}</b> ({id}) on {}.\nAllow only if you asked for it. Any program on the phone's Pocket session could then use it (revoke: pocket-desktop-agent revoke {id}).",
            escape(label),
            escape(&self.cfg.host_name),
        );
        let hints: HashMap<&str, Variant<'_>> = HashMap::from([
            ("desktop-entry", Variant::from("pocket-desktop-agent")),
            ("urgency", Variant::from(1u8)),
            ("resident", Variant::from(false)),
        ]);
        let reply = bus
            .call_method(
                Some("org.freedesktop.Notifications"),
                "/org/freedesktop/Notifications",
                Some("org.freedesktop.Notifications"),
                "Notify",
                &(
                    super::notify::OWN_APP_NAME,
                    0u32,
                    "dialog-password",
                    "Allow phone access?",
                    body.as_str(),
                    vec!["deny", "Deny", "allow", "Allow"],
                    hints,
                    0i32, // never expires by itself; we close it after ANSWER_TIMEOUT
                ),
            )
            .await
            .map_err(|e| {
                ErrorInfo::new(error_code::FAILED, format!("notification service: {e}"))
            })?;
        let nid: u32 = reply
            .body()
            .deserialize()
            .map_err(|e| ErrorInfo::new(error_code::FAILED, format!("unexpected reply: {e}")))?;
        if let Ok(mut s) = self.state.lock() {
            s.open = Some((id.to_owned(), Instant::now()));
        }
        info!(
            action = id,
            notification = nid,
            "access: asked the laptop owner"
        );

        let result = json!({ "status": "asked", "action": id });
        let (state, cfg, id) = (self.state.clone(), self.cfg.clone(), id.to_owned());
        tokio::spawn(async move {
            let answer = tokio::time::timeout(ANSWER_TIMEOUT, async {
                while let Some(Ok(msg)) = stream.next().await {
                    let member = msg.header().member().map(|m| m.to_string());
                    match member.as_deref() {
                        Some("ActionInvoked") => {
                            if let Ok((n, key)) = msg.body().deserialize::<(u32, String)>()
                                && n == nid
                            {
                                return Some(key == "allow");
                            }
                        }
                        Some("NotificationClosed") => {
                            if let Ok((n, _)) = msg.body().deserialize::<(u32, u32)>()
                                && n == nid
                            {
                                return None; // dismissed without an answer
                            }
                        }
                        _ => {}
                    }
                }
                None
            })
            .await
            .unwrap_or(None);
            let allowed = answer == Some(true);
            if let Ok(mut s) = state.lock() {
                s.open = None;
                if answer == Some(false) {
                    s.denied.insert(id.clone(), Instant::now());
                }
            }
            // Close the prompt (no-op if the server already closed it).
            let _ = bus
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "CloseNotification",
                    &(nid,),
                )
                .await;
            if !allowed {
                info!(action = %id, answer = ?answer, "access: not allowed");
                changes.send_modify(|n| *n += 1); // the phone re-reads the state
                return;
            }
            if let Some(path) = &cfg.config_path {
                let saved = Config::load(path).and_then(|mut c| {
                    if !c.grants.contains(&id) {
                        c.grants.push(id.clone());
                    }
                    c.save(path)
                });
                if let Err(e) = saved {
                    warn!(error = %e, action = %id, "access: allowed, but config.json could not be updated (grant lasts until restart)");
                }
            }
            if let Ok(mut p) = policy.write() {
                p.grants.insert(id.clone());
            }
            changes.send_modify(|n| *n += 1);
            info!(action = %id, "access: allowed by the laptop owner");
        });
        Ok(result)
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
