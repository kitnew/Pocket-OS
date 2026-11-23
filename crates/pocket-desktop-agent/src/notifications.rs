//! Notification bridge, laptop side (ADR-0008 §5).
//!
//! The desktop's notification server (Plasma) stays in charge. The agent opens a second
//! session-bus connection and turns it into a **monitor** (`BecomeMonitor`, allowed for
//! the bus owner's UID) that receives only:
//! - `Notify` method calls (the notification content),
//! - method returns sent by the notification server (the id it assigned),
//! - `NotificationClosed` signals.
//!
//! A monitor connection never sends anything. Each notification passes the policy
//! ([`Policy`]: opt-in, allow/deny lists by app name, no echo of the phone's own
//! notifications, body withheld while the screen is locked) and is broadcast to the
//! sessions that subscribed to `notifications`. Nothing is stored on disk. Markup is
//! stripped; texts are clipped.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use pocket_dock_protocol::message::{AgentMessage, Notification};
use pocket_dock_protocol::topic;
use serde_json::json;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use zbus::OwnedVariant;

use crate::config::NotificationPolicy;
use crate::live::Live;

const SERVER: &str = "org.freedesktop.Notifications";
const IFACE: &str = "org.freedesktop.Notifications";
/// The phone's own `desktop.notify` notifications are never sent back to it.
pub const OWN_APP: &str = "Pocket OS";
const MAX_PENDING: usize = 64;
const MAX_FORWARDED: usize = 256;

/// Applies the laptop owner's policy to one notification.
#[derive(Debug, Clone)]
pub struct Policy(pub NotificationPolicy);

impl Policy {
    /// `None` = not forwarded. `locked`: the screen locker is active.
    pub fn apply(&self, mut n: Notification, locked: Option<bool>) -> Option<Notification> {
        let p = &self.0;
        if !p.forward || n.app.eq_ignore_ascii_case(OWN_APP) {
            return None;
        }
        let matches = |list: &[String]| list.iter().any(|a| a.eq_ignore_ascii_case(&n.app));
        if matches(&p.deny_apps) || p.allow_apps.as_deref().is_some_and(|l| !matches(l)) {
            return None;
        }
        if p.redact_when_locked && locked != Some(false) && !n.body.is_empty() {
            // Unknown lock state counts as locked: never leak by default.
            n.body.clear();
            n.redacted = true;
        }
        Some(n)
    }
}

/// Plain text from notification markup (`<b>`, `<a href>`, entities), clipped.
pub fn plain(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    out.trim().chars().take(max).collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Starts the bridge on the user's session bus; forwarded messages go to `tx`. Publishes
/// the `notifications` topic.
pub fn start(policy: Policy, live: Live, tx: broadcast::Sender<AgentMessage>) {
    start_on(None, policy, live, tx)
}

/// [`start`] on the bus at `address` (`None` = the session bus; tests use a private bus).
pub fn start_on(
    address: Option<String>,
    policy: Policy,
    live: Live,
    tx: broadcast::Sender<AgentMessage>,
) {
    if !policy.0.forward {
        live.set(
            topic::NOTIFICATIONS,
            true,
            "",
            0,
            json!({"forwarding": false,
                   "reason": "disabled on the computer (pocket-desktop-agent notifications on)"}),
        );
        return;
    }
    tokio::spawn(async move {
        loop {
            match run(address.as_deref(), &policy, &live, &tx).await {
                Ok(()) => info!("notifications: server changed; restarting the monitor"),
                Err(e) => {
                    warn!(error = %e, "notifications: monitor failed; retrying in 10 s");
                    live.set(
                        topic::NOTIFICATIONS,
                        false,
                        &format!("monitor failed: {e}"),
                        0,
                        json!({"forwarding": false}),
                    );
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            }
        }
    });
}

struct Pending {
    note: Notification,
    at: std::time::Instant,
}

/// One monitor lifetime: until the notification server's owner changes (Ok) or an error.
fn builder(address: Option<&str>) -> zbus::connection::Builder<'_> {
    match address {
        Some(a) => zbus::connection::Builder::address(a),
        None => zbus::connection::Builder::session(),
    }
}

async fn run(
    address: Option<&str>,
    policy: &Policy,
    live: &Live,
    tx: &broadcast::Sender<AgentMessage>,
) -> zbus::Result<()> {
    let control = builder(address).build().await?;
    let dbus = zbus::fdo::DBusProxy::new(&control).await?;
    let mut owner_changes = dbus.receive_name_owner_changed().await?;
    let server = loop {
        match dbus.get_name_owner(SERVER.try_into()?).await {
            Ok(o) => break o.to_string(),
            Err(_) => {
                live.set(
                    topic::NOTIFICATIONS,
                    false,
                    "no notification server is running",
                    0,
                    json!({"forwarding": false}),
                );
                // Wait for it to appear.
                while let Some(sig) = owner_changes.next().await {
                    if sig
                        .args()
                        .is_ok_and(|a| a.name() == SERVER && a.new_owner().is_some())
                    {
                        break;
                    }
                }
            }
        }
    };

    let monitor = builder(address).build().await?;
    let rules = [
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::MethodCall)
            .interface(IFACE)
            .member("Notify")
            .build()?,
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::MethodReturn)
            .sender(server.as_str())
            .build()?,
        zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(IFACE)
            .member("NotificationClosed")
            .build()?,
    ];
    zbus::fdo::MonitoringProxy::new(&monitor)
        .await?
        .become_monitor(&rules, 0)
        .await?;
    let mut stream = zbus::MessageStream::from(&monitor);
    info!(%server, "notifications: monitoring the notification server");
    live.set(
        topic::NOTIFICATIONS,
        true,
        "",
        0,
        json!({"forwarding": true, "reason": ""}),
    );

    let mut pending: HashMap<(String, u32), Pending> = HashMap::new();
    let mut forwarded: VecDeque<String> = VecDeque::new();
    let mut forwarded_set: HashSet<String> = HashSet::new();

    loop {
        tokio::select! {
            sig = owner_changes.next() => {
                match sig {
                    Some(sig) if sig.args().is_ok_and(|a| a.name() == SERVER) => return Ok(()),
                    Some(_) => continue,
                    None => return Err(zbus::Error::Failure("bus closed".into())),
                }
            }
            msg = stream.next() => {
                let Some(msg) = msg else { return Err(zbus::Error::Failure("monitor closed".into())) };
                let Ok(msg) = msg else { continue };
                let h = msg.header();
                match msg.message_type() {
                    zbus::message::Type::MethodCall => {
                        type Args = (String, u32, String, String, String, Vec<String>, HashMap<String, OwnedVariant>, i32);
                        let Ok((app, replaces, _icon, summary, body, _actions, mut hints, _timeout)) =
                            msg.body().deserialize::<Args>() else { continue };
                        let urgency = hints.remove("urgency").and_then(|v| u8::try_from(v).ok()).unwrap_or(1).min(2);
                        let note = Notification {
                            nid: if replaces != 0 { format!("n{replaces}") } else { String::new() },
                            app: plain(&app, 64),
                            summary: plain(&summary, 200),
                            body: plain(&body, 500),
                            urgency,
                            redacted: false,
                            time_ms: now_ms(),
                        };
                        let Some(sender) = h.sender().map(|s| s.to_string()) else { continue };
                        pending.retain(|_, p| p.at.elapsed() < Duration::from_secs(10));
                        if pending.len() >= MAX_PENDING {
                            continue;
                        }
                        pending.insert((sender, h.primary().serial_num().get()), Pending { note, at: std::time::Instant::now() });
                    }
                    zbus::message::Type::MethodReturn => {
                        let (Some(dest), Some(serial)) = (h.destination().map(|d| d.to_string()), h.reply_serial()) else { continue };
                        let Some(p) = pending.remove(&(dest, serial.get())) else { continue };
                        let Ok(id) = msg.body().deserialize::<u32>() else { continue };
                        let mut note = p.note;
                        if note.nid.is_empty() {
                            note.nid = format!("n{id}");
                        }
                        let Some(note) = policy.apply(note, live.host_locked()) else {
                            debug!("notifications: filtered by policy");
                            continue;
                        };
                        // Log no content: notification text is private.
                        info!(nid = %note.nid, redacted = note.redacted, "notifications: forwarding");
                        if forwarded_set.insert(note.nid.clone()) {
                            forwarded.push_back(note.nid.clone());
                            if forwarded.len() > MAX_FORWARDED {
                                if let Some(old) = forwarded.pop_front() {
                                    forwarded_set.remove(&old);
                                }
                            }
                        }
                        let _ = tx.send(AgentMessage::Notification(note));
                    }
                    zbus::message::Type::Signal => {
                        let Ok((id, _reason)) = msg.body().deserialize::<(u32, u32)>() else { continue };
                        let nid = format!("n{id}");
                        if forwarded_set.remove(&nid) {
                            forwarded.retain(|n| *n != nid);
                            let _ = tx.send(AgentMessage::NotificationClosed { nid });
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(app: &str, body: &str) -> Notification {
        Notification {
            nid: "n1".into(),
            app: app.into(),
            summary: "s".into(),
            body: body.into(),
            urgency: 1,
            redacted: false,
            time_ms: 0,
        }
    }

    fn policy(f: impl FnOnce(&mut NotificationPolicy)) -> Policy {
        let mut p = NotificationPolicy {
            forward: true,
            ..Default::default()
        };
        f(&mut p);
        Policy(p)
    }

    #[test]
    fn policy_is_opt_in_and_filters_by_app() {
        assert!(
            Policy(NotificationPolicy::default())
                .apply(note("Mail", "x"), Some(false))
                .is_none()
        );
        let p = policy(|_| {});
        assert!(p.apply(note("Mail", "x"), Some(false)).is_some());
        assert!(
            p.apply(note("pocket os", "x"), Some(false)).is_none(),
            "no echo"
        );
        let p = policy(|p| p.deny_apps = vec!["mail".into()]);
        assert!(p.apply(note("Mail", "x"), Some(false)).is_none());
        let p = policy(|p| p.allow_apps = Some(vec!["Chat".into()]));
        assert!(p.apply(note("Mail", "x"), Some(false)).is_none());
        assert!(p.apply(note("chat", "x"), Some(false)).is_some());
    }

    #[test]
    fn bodies_are_withheld_unless_the_screen_is_known_unlocked() {
        let p = policy(|_| {});
        let n = p.apply(note("Mail", "secret"), Some(true)).unwrap();
        assert!(n.redacted && n.body.is_empty());
        let n = p.apply(note("Mail", "secret"), None).unwrap();
        assert!(n.redacted, "unknown lock state counts as locked");
        let n = p.apply(note("Mail", "secret"), Some(false)).unwrap();
        assert_eq!(n.body, "secret");
        let p = policy(|p| p.redact_when_locked = false);
        assert_eq!(
            p.apply(note("Mail", "secret"), Some(true)).unwrap().body,
            "secret"
        );
    }

    #[test]
    fn markup_is_stripped() {
        assert_eq!(
            plain("<b>Hi</b> &amp; <a href='x'>bye</a>", 100),
            "Hi & bye"
        );
        assert_eq!(plain("abcdef", 3), "abc");
    }
}
