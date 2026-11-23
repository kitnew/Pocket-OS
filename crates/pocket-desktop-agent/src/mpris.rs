//! MPRIS media players on the user's session bus (`org.mpris.MediaPlayer2.*`).
//!
//! One tracker per agent, shared by the `media.*` actions and the `media` live topic.
//! Event-driven, no polling:
//! - players appear and disappear with `NameOwnerChanged`;
//! - a player's state is re-read (one `GetAll` + `Identity`) when it emits
//!   `PropertiesChanged` on `/org/mpris/MediaPlayer2`.
//!
//! The **active** player is the one that most recently started playing; if none ever
//! did, the first `Playing` one, else the first one by name. Commands target it.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use pocket_dock_protocol::error_code;
use pocket_dock_protocol::message::ErrorInfo;
use serde_json::{Value, json};
use tokio::sync::watch;
use tracing::{debug, info, warn};
use zbus::OwnedVariant;

use crate::actions::Changes;

pub const PREFIX: &str = "org.mpris.MediaPlayer2.";
pub const PATH: &str = "/org/mpris/MediaPlayer2";
pub const ROOT: &str = "org.mpris.MediaPlayer2";
pub const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Player {
    /// Bus name, e.g. `org.mpris.MediaPlayer2.elisa`.
    pub bus_name: String,
    pub identity: String,
    /// `Playing` | `Paused` | `Stopped` | "" (unknown).
    pub status: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub can_next: bool,
    pub can_previous: bool,
    pub can_play_pause: bool,
    /// Sequence number of the last switch to `Playing` (0 = never).
    pub started: u64,
}

impl Player {
    /// The short id the phone sees (`elisa`, `firefox.instance_1_134`).
    pub fn id(&self) -> &str {
        self.bus_name.trim_start_matches(PREFIX)
    }

    fn json(&self) -> Value {
        json!({
            "id": self.id(), "identity": self.identity, "status": self.status,
            "title": self.title, "artist": self.artist, "album": self.album,
            "can_next": self.can_next, "can_previous": self.can_previous,
            "can_play_pause": self.can_play_pause,
        })
    }
}

#[derive(Default)]
struct State {
    players: BTreeMap<String, Player>,
    /// unique bus name → player bus name
    owners: HashMap<String, String>,
    seq: u64,
}

impl State {
    fn active(&self) -> Option<&Player> {
        self.players
            .values()
            .filter(|p| p.status == "Playing")
            .max_by_key(|p| p.started)
            .or_else(|| {
                self.players
                    .values()
                    .filter(|p| p.started > 0)
                    .max_by_key(|p| p.started)
            })
            .or_else(|| self.players.values().next())
    }
}

#[derive(Clone)]
pub struct Mpris {
    bus: zbus::Connection,
    state: Arc<Mutex<State>>,
    revision: Arc<watch::Sender<u64>>,
    /// Action availability follows the set of players.
    changes: Option<Changes>,
}

impl Mpris {
    /// Starts tracking in the background (needs a Tokio runtime).
    pub fn start(bus: zbus::Connection, changes: Option<Changes>) -> Self {
        let m = Mpris {
            bus,
            state: Arc::default(),
            revision: Arc::new(watch::channel(0).0),
            changes,
        };
        tokio::spawn(m.clone().track());
        m
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Bumps on every observable change (players, status, metadata).
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.revision.subscribe()
    }

    pub fn player_count(&self) -> usize {
        self.lock().players.len()
    }

    pub fn active(&self) -> Option<Player> {
        self.lock().active().cloned()
    }

    /// The `media` topic value: `{active, players: [...]}`.
    pub fn snapshot(&self) -> Value {
        let s = self.lock();
        json!({
            "active": s.active().map(|p| p.id().to_owned()),
            "players": s.players.values().map(Player::json).collect::<Vec<_>>(),
        })
    }

    /// Calls `method` (`PlayPause`, `Next`, `Previous`) on the active player and returns
    /// its short id.
    pub async fn command(&self, method: &str) -> Result<String, ErrorInfo> {
        let target = self
            .active()
            .ok_or_else(|| ErrorInfo::new(error_code::UNAVAILABLE, "no media player is running"))?;
        self.bus
            .call_method(
                Some(target.bus_name.as_str()),
                PATH,
                Some(PLAYER),
                method,
                &(),
            )
            .await
            .map_err(|e| ErrorInfo::new(error_code::FAILED, format!("{}: {e}", target.id())))?;
        info!(player = %target.id(), method, "media command");
        Ok(target.id().to_owned())
    }

    fn changed(&self, players_changed: bool) {
        self.revision.send_modify(|n| *n += 1);
        if players_changed {
            if let Some(c) = &self.changes {
                c.notify();
            }
        }
    }

    async fn track(self) {
        let Ok(dbus) = zbus::fdo::DBusProxy::new(&self.bus).await else {
            warn!("mpris: cannot create the bus proxy; players are not tracked");
            return;
        };
        // Subscribe before listing so no change is missed.
        let mut owners = match dbus.receive_name_owner_changed().await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "mpris: cannot watch the bus");
                return;
            }
        };
        let rule = zbus::MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface("org.freedesktop.DBus.Properties")
            .member("PropertiesChanged")
            .path(PATH)
            .build();
        let mut props = match rule {
            Ok(rule) => zbus::MessageStream::for_match_rule(rule, &self.bus, None)
                .await
                .ok(),
            Err(_) => None,
        };
        if props.is_none() {
            warn!("mpris: cannot watch player properties; only players are tracked");
        }
        if let Ok(names) = dbus.list_names().await {
            for n in names
                .iter()
                .map(|n| n.to_string())
                .filter(|n| n.starts_with(PREFIX))
            {
                let Ok(bus_name) = zbus::names::BusName::try_from(n.as_str()) else {
                    continue;
                };
                if let Ok(owner) = dbus.get_name_owner(bus_name).await {
                    self.lock().owners.insert(owner.to_string(), n.clone());
                }
                self.refresh(&n).await;
            }
            info!(players = ?self.lock().players.keys().collect::<Vec<_>>(), "mpris: players");
            self.changed(true);
        }
        loop {
            tokio::select! {
                sig = owners.next() => {
                    let Some(sig) = sig else { break };
                    let Ok(args) = sig.args() else { continue };
                    let name = args.name().to_string();
                    if !name.starts_with(PREFIX) {
                        continue;
                    }
                    match args.new_owner().as_ref() {
                        Some(owner) => {
                            self.lock().owners.insert(owner.to_string(), name.clone());
                            info!(player = %name, "mpris: player appeared");
                            self.refresh(&name).await;
                        }
                        None => {
                            let mut s = self.lock();
                            s.players.remove(&name);
                            s.owners.retain(|_, n| *n != name);
                            drop(s);
                            info!(player = %name, "mpris: player left");
                        }
                    }
                    self.changed(true);
                }
                Some(msg) = async { match props.as_mut() { Some(p) => p.next().await, None => std::future::pending().await } } => {
                    let Ok(msg) = msg else { continue };
                    let sender = msg.header().sender().map(|s| s.to_string());
                    let name = sender.and_then(|s| self.lock().owners.get(&s).cloned());
                    if let Some(name) = name {
                        debug!(player = %name, "mpris: properties changed");
                        self.refresh(&name).await;
                        self.changed(false);
                    }
                }
            }
        }
    }

    /// Re-reads one player's state.
    async fn refresh(&self, name: &str) {
        let all = self
            .bus
            .call_method(
                Some(name),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(PLAYER,),
            )
            .await
            .ok()
            .and_then(|m| m.body().deserialize::<HashMap<String, OwnedVariant>>().ok())
            .unwrap_or_default();
        let identity = self
            .bus
            .call_method(
                Some(name),
                PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Get",
                &(ROOT, "Identity"),
            )
            .await
            .ok()
            .and_then(|m| m.body().deserialize::<OwnedVariant>().ok())
            .and_then(|v| String::try_from(v).ok())
            .unwrap_or_default();
        let mut p = parse_player(name, all);
        p.identity = identity;
        let mut s = self.lock();
        let prev = s.players.get(name).map(|o| (o.status.clone(), o.started));
        p.started = match prev {
            Some((old, started)) if old == "Playing" => started,
            _ if p.status == "Playing" => {
                s.seq += 1;
                s.seq
            }
            Some((_, started)) => started,
            None => 0,
        };
        s.players.insert(name.to_owned(), p);
    }
}

/// Builds a player from `GetAll(org.mpris.MediaPlayer2.Player)` properties.
pub fn parse_player(bus_name: &str, mut all: HashMap<String, OwnedVariant>) -> Player {
    let mut take_bool = |k: &str| {
        all.remove(k)
            .and_then(|v| bool::try_from(v).ok())
            .unwrap_or(false)
    };
    let can_next = take_bool("CanGoNext");
    let can_previous = take_bool("CanGoPrevious");
    let can_play_pause = take_bool("CanPlay") || take_bool("CanPause");
    let status = all
        .remove("PlaybackStatus")
        .and_then(|v| String::try_from(v).ok())
        .unwrap_or_default();
    let mut meta: HashMap<String, OwnedVariant> = all
        .remove("Metadata")
        .and_then(|v| HashMap::try_from(v).ok())
        .unwrap_or_default();
    let mut text = |k: &str| {
        meta.remove(k)
            .and_then(|v| String::try_from(v).ok())
            .unwrap_or_default()
    };
    let title = text("xesam:title");
    let album = text("xesam:album");
    let artist = meta
        .remove("xesam:artist")
        .and_then(|v| Vec::<String>::try_from(v).ok())
        .map(|a| a.join(", "))
        .unwrap_or_default();
    let clip = |s: String, n: usize| s.chars().take(n).collect::<String>();
    Player {
        bus_name: bus_name.to_owned(),
        identity: String::new(),
        status,
        title: clip(title, 200),
        artist: clip(artist, 200),
        album: clip(album, 200),
        can_next,
        can_previous,
        can_play_pause,
        started: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(name: &str, status: &str, started: u64) -> Player {
        Player {
            bus_name: format!("{PREFIX}{name}"),
            status: status.into(),
            started,
            ..Default::default()
        }
    }

    #[test]
    fn the_most_recently_started_player_is_active() {
        let mut s = State::default();
        assert!(s.active().is_none());
        for p in [player("a", "Paused", 0), player("b", "Stopped", 0)] {
            s.players.insert(p.bus_name.clone(), p);
        }
        assert_eq!(
            s.active().unwrap().id(),
            "a",
            "first by name when none ever played"
        );
        for p in [player("a", "Playing", 1), player("b", "Playing", 2)] {
            s.players.insert(p.bus_name.clone(), p);
        }
        assert_eq!(s.active().unwrap().id(), "b", "the latest to start playing");
        s.players
            .insert(format!("{PREFIX}b"), player("b", "Paused", 2));
        assert_eq!(
            s.active().unwrap().id(),
            "a",
            "a playing one beats a paused one"
        );
        s.players
            .insert(format!("{PREFIX}a"), player("a", "Paused", 1));
        assert_eq!(
            s.active().unwrap().id(),
            "b",
            "nothing plays: the last one that did"
        );
    }
}
