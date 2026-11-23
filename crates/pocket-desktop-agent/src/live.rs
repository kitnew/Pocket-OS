//! Live host state (Pocket Dock 1.1, docs/dock-mode/protocol.md#topic-payloads).
//!
//! [`Live`] holds the current value of each topic with a per-topic revision. Producers
//! call [`Live::set`]; a value equal to the current one does not bump the revision.
//! Sessions subscribe, send a snapshot, then send a topic again whenever its revision
//! grew (the server coalesces bursts).
//!
//! Producers (all event-driven except the CPU/memory sampler):
//! - `media`: the shared MPRIS tracker ([`crate::mpris`]);
//! - `host`: `org.freedesktop.ScreenSaver` (`GetActive`, `ActiveChanged`) on the session
//!   bus and UPower's display device (`PropertiesChanged`) on the system bus;
//! - `system`: `/proc/stat` and `/proc/meminfo` every [`SYSTEM_INTERVAL`], **only while
//!   at least one phone is subscribed**;
//! - `notifications`: the forwarding policy (set by [`crate::notifications`]).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use pocket_dock_protocol::message::AgentMessage;
use pocket_dock_protocol::topic;
use serde_json::{Value, json};
use tokio::sync::{Notify, watch};
use tracing::{debug, info, warn};
use zbus::OwnedVariant;

pub const SYSTEM_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq)]
struct Topic {
    rev: u64,
    available: bool,
    reason: String,
    interval_ms: u64,
    data: Value,
}

struct Inner {
    topics: Mutex<BTreeMap<String, Topic>>,
    changed: watch::Sender<u64>,
    subscribers: AtomicUsize,
    wake: Notify,
}

#[derive(Clone)]
pub struct Live(Arc<Inner>);

impl Default for Live {
    fn default() -> Self {
        Self::new()
    }
}

/// Counts a subscribed session while alive (the sampler runs only while there is one).
pub struct Subscriber(Live);

impl Drop for Subscriber {
    fn drop(&mut self) {
        self.0.0.subscribers.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Live {
    pub fn new() -> Self {
        Live(Arc::new(Inner {
            topics: Mutex::default(),
            changed: watch::channel(0).0,
            subscribers: AtomicUsize::new(0),
            wake: Notify::new(),
        }))
    }

    /// Sets a topic. Unchanged values keep their revision (no message is sent).
    pub fn set(&self, name: &str, available: bool, reason: &str, interval_ms: u64, data: Value) {
        let mut t = self.0.topics.lock().unwrap_or_else(|p| p.into_inner());
        let entry = t.entry(name.to_owned()).or_insert(Topic {
            rev: 0,
            available: !available,
            reason: String::new(),
            interval_ms,
            data: Value::Null,
        });
        // A sampled topic is re-sent even when equal: the phone uses it as a heartbeat.
        if interval_ms == 0
            && entry.available == available
            && entry.reason == reason
            && entry.data == data
        {
            return;
        }
        entry.rev += 1;
        entry.available = available;
        entry.reason = reason.to_owned();
        entry.interval_ms = interval_ms;
        entry.data = data;
        drop(t);
        self.0.changed.send_modify(|n| *n += 1);
    }

    /// Bumps whenever any topic changed.
    pub fn changes(&self) -> watch::Receiver<u64> {
        self.0.changed.subscribe()
    }

    /// The `state` message for a topic and its revision (unknown topics: unavailable).
    pub fn message(&self, name: &str) -> (u64, AgentMessage) {
        let t = self.0.topics.lock().unwrap_or_else(|p| p.into_inner());
        match t.get(name) {
            Some(x) => (
                x.rev,
                AgentMessage::State {
                    topic: name.to_owned(),
                    rev: x.rev,
                    available: x.available,
                    reason: x.reason.clone(),
                    interval_ms: x.interval_ms,
                    data: x.data.clone(),
                },
            ),
            None => (
                0,
                AgentMessage::State {
                    topic: name.to_owned(),
                    rev: 0,
                    available: false,
                    reason: "not provided by this agent".into(),
                    interval_ms: 0,
                    data: Value::Null,
                },
            ),
        }
    }

    pub fn rev(&self, name: &str) -> u64 {
        let t = self.0.topics.lock().unwrap_or_else(|p| p.into_inner());
        t.get(name).map_or(0, |x| x.rev)
    }

    /// The `data` of a topic (Null when unknown).
    pub fn data(&self, name: &str) -> Value {
        let t = self.0.topics.lock().unwrap_or_else(|p| p.into_inner());
        t.get(name).map_or(Value::Null, |x| x.data.clone())
    }

    pub fn subscriber(&self) -> Subscriber {
        self.0.subscribers.fetch_add(1, Ordering::SeqCst);
        self.0.wake.notify_one();
        Subscriber(self.clone())
    }

    /// `host.locked` as last reported (None = unknown).
    pub fn host_locked(&self) -> Option<bool> {
        self.data(topic::HOST)
            .get("locked")
            .and_then(Value::as_bool)
    }
}

/// Starts every producer (needs a Tokio runtime).
pub fn start(live: &Live, session: Option<zbus::Connection>, mpris: Option<crate::mpris::Mpris>) {
    match mpris {
        Some(m) => {
            tokio::spawn(media(live.clone(), m));
        }
        None => live.set(
            topic::MEDIA,
            false,
            "no desktop session bus",
            0,
            Value::Null,
        ),
    }
    tokio::spawn(host(live.clone(), session));
    tokio::spawn(system(live.clone()));
}

async fn media(live: Live, m: crate::mpris::Mpris) {
    let mut rx = m.subscribe();
    loop {
        live.set(topic::MEDIA, true, "", 0, m.snapshot());
        if rx.changed().await.is_err() {
            return;
        }
        // Coalesce a burst (a track change emits several PropertiesChanged).
        tokio::time::sleep(Duration::from_millis(150)).await;
        rx.borrow_and_update();
    }
}

#[derive(Default, Clone, PartialEq)]
struct HostState {
    locked: Option<bool>,
    battery: Option<(f64, bool)>,
}

impl HostState {
    fn json(&self) -> Value {
        json!({
            "locked": self.locked,
            "battery": self.battery.map(|(p, c)| json!({"percent": p.round(), "charging": c})),
        })
    }
}

async fn host(live: Live, session: Option<zbus::Connection>) {
    let state = Arc::new(Mutex::new(HostState::default()));
    let publish = |live: &Live, s: &HostState| live.set(topic::HOST, true, "", 0, s.json());
    publish(&live, &state.lock().unwrap());
    let lock_task = session.map(|bus| {
        let (live, state) = (live.clone(), state.clone());
        tokio::spawn(async move {
            let rule = zbus::MatchRule::builder()
                .msg_type(zbus::message::Type::Signal)
                .interface("org.freedesktop.ScreenSaver")
                .member("ActiveChanged")
                .build();
            let mut stream = match rule {
                Ok(r) => zbus::MessageStream::for_match_rule(r, &bus, None)
                    .await
                    .ok(),
                Err(_) => None,
            };
            let active = bus
                .call_method(
                    Some("org.freedesktop.ScreenSaver"),
                    "/org/freedesktop/ScreenSaver",
                    Some("org.freedesktop.ScreenSaver"),
                    "GetActive",
                    &(),
                )
                .await
                .ok()
                .and_then(|m| m.body().deserialize::<bool>().ok());
            if let Ok(mut s) = state.lock() {
                s.locked = active;
                publish(&live, &s);
            }
            info!(locked = ?active, "host: screen locker");
            let Some(stream) = stream.as_mut() else {
                return;
            };
            while let Some(Ok(msg)) = stream.next().await {
                if let Ok(a) = msg.body().deserialize::<bool>() {
                    debug!(locked = a, "host: screen locker changed");
                    if let Ok(mut s) = state.lock() {
                        s.locked = Some(a);
                        publish(&live, &s);
                    }
                }
            }
        })
    });
    // Battery: UPower's display device on the system bus (absent on desktops).
    let battery = async {
        let Ok(bus) = zbus::Connection::system().await else {
            return;
        };
        const DEV: &str = "/org/freedesktop/UPower/devices/DisplayDevice";
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .member("PropertiesChanged")
            .path(DEV)
            .build();
        let mut stream = match rule {
            Ok(r) => zbus::MessageStream::for_match_rule(r, &bus, None)
                .await
                .ok(),
            Err(_) => None,
        };
        loop {
            let props = bus
                .call_method(
                    Some("org.freedesktop.UPower"),
                    DEV,
                    Some("org.freedesktop.DBus.Properties"),
                    "GetAll",
                    &("org.freedesktop.UPower.Device",),
                )
                .await
                .ok()
                .and_then(|m| {
                    m.body()
                        .deserialize::<std::collections::HashMap<String, OwnedVariant>>()
                        .ok()
                });
            let b = props.and_then(|mut p| {
                let present = p.remove("IsPresent").and_then(|v| bool::try_from(v).ok())?;
                let pct = p.remove("Percentage").and_then(|v| f64::try_from(v).ok())?;
                // UPower states: 1 charging, 4 fully charged, 5 pending charge.
                let st = p
                    .remove("State")
                    .and_then(|v| u32::try_from(v).ok())
                    .unwrap_or(0);
                present.then_some((pct, matches!(st, 1 | 5)))
            });
            if let Ok(mut s) = state.lock() {
                if s.battery != b {
                    s.battery = b;
                    publish(&live, &s);
                }
            }
            match stream.as_mut() {
                Some(st) => {
                    if st.next().await.is_none() {
                        return;
                    }
                }
                None => return,
            }
        }
    };
    battery.await;
    if let Some(t) = lock_task {
        let _ = t.await;
    }
}

/// CPU busy share since the previous sample, from the first line of `/proc/stat`.
fn cpu_times() -> Option<(u64, u64)> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().next()?;
    let v: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if v.len() < 4 {
        return None;
    }
    let idle = v[3] + v.get(4).copied().unwrap_or(0); // idle + iowait
    Some((v.iter().take(8).sum(), idle))
}

/// Used memory share from `/proc/meminfo` (MemTotal − MemAvailable).
fn mem_percent() -> Option<f64> {
    let info = std::fs::read_to_string("/proc/meminfo").ok()?;
    let get = |k: &str| {
        info.lines()
            .find(|l| l.starts_with(k))?
            .split_whitespace()
            .nth(1)?
            .parse::<f64>()
            .ok()
    };
    let (total, avail) = (get("MemTotal:")?, get("MemAvailable:")?);
    (total > 0.0).then(|| 100.0 * (total - avail) / total)
}

async fn system(live: Live) {
    let interval_ms = SYSTEM_INTERVAL.as_millis() as u64;
    let mut prev = cpu_times();
    loop {
        if live.0.subscribers.load(Ordering::SeqCst) == 0 {
            live.0.wake.notified().await;
            prev = cpu_times();
            tokio::time::sleep(Duration::from_millis(500)).await; // a first meaningful delta
        }
        let now = cpu_times();
        let cpu = match (prev, now) {
            (Some((t0, i0)), Some((t1, i1))) if t1 > t0 => {
                100.0 * (1.0 - (i1.saturating_sub(i0)) as f64 / (t1 - t0) as f64)
            }
            _ => 0.0,
        };
        prev = now;
        match mem_percent() {
            Some(mem) => live.set(
                topic::SYSTEM,
                true,
                "",
                interval_ms,
                json!({"cpu_percent": (cpu * 10.0).round() / 10.0, "mem_percent": (mem * 10.0).round() / 10.0}),
            ),
            None => {
                warn!("system: /proc/meminfo is not readable");
                live.set(topic::SYSTEM, false, "system statistics unavailable", 0, Value::Null);
                return;
            }
        }
        tokio::time::sleep(SYSTEM_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_values_keep_their_revision() {
        let l = Live::new();
        assert_eq!(l.rev("media"), 0);
        l.set("media", true, "", 0, json!({"a": 1}));
        l.set("media", true, "", 0, json!({"a": 1}));
        assert_eq!(l.rev("media"), 1);
        l.set("media", true, "", 0, json!({"a": 2}));
        assert_eq!(l.rev("media"), 2);
        // sampled topics are heartbeats: every sample is a new revision
        l.set("system", true, "", 5000, json!({"cpu_percent": 1}));
        l.set("system", true, "", 5000, json!({"cpu_percent": 1}));
        assert_eq!(l.rev("system"), 2);
        let (_, m) = l.message("nope");
        assert!(matches!(
            m,
            AgentMessage::State {
                available: false,
                ..
            }
        ));
    }

    #[test]
    fn proc_readers_work_on_linux() {
        assert!(cpu_times().is_some());
        let m = mem_percent().unwrap();
        assert!((0.0..=100.0).contains(&m));
    }

    #[test]
    fn host_lock_state_is_read_from_the_topic() {
        let l = Live::new();
        assert_eq!(l.host_locked(), None);
        l.set(
            topic::HOST,
            true,
            "",
            0,
            HostState {
                locked: Some(true),
                battery: None,
            }
            .json(),
        );
        assert_eq!(l.host_locked(), Some(true));
    }
}
