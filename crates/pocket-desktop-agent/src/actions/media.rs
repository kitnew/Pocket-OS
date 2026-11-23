//! `media.play_pause`: toggles playback of an MPRIS media player
//! (`org.mpris.MediaPlayer2.*` on the session bus).
//!
//! Available while at least one player is on the bus. The set of players is tracked
//! from `NameOwnerChanged` (no polling) and every change is announced to the phone.
//! Target: the first player that is `Playing`, otherwise the first one by name.
//! Result data: `{"player": "<bus name suffix>"}`.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use pocket_dock_protocol::error_code;
use pocket_dock_protocol::message::{ActionInfo, ErrorInfo};
use serde_json::json;
use tracing::{info, warn};
use zbus::OwnedVariant;

use super::{Action, ActionFuture, Changes, Params};

pub const ID: &str = "media.play_pause";
const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

pub struct MediaPlayPause {
    bus: Option<zbus::Connection>,
    players: Arc<Mutex<BTreeSet<String>>>,
}

impl MediaPlayPause {
    /// Starts tracking players in the background (needs a Tokio runtime when `bus` is
    /// set).
    pub fn new(bus: Option<zbus::Connection>, changes: Changes) -> Self {
        let players = Arc::new(Mutex::new(BTreeSet::new()));
        if let Some(bus) = bus.clone() {
            tokio::spawn(track(bus, players.clone(), changes));
        }
        MediaPlayPause { bus, players }
    }

    fn players(&self) -> Vec<String> {
        self.players
            .lock()
            .map(|p| p.iter().cloned().collect())
            .unwrap_or_default()
    }
}

async fn track(bus: zbus::Connection, players: Arc<Mutex<BTreeSet<String>>>, changes: Changes) {
    let Ok(proxy) = zbus::fdo::DBusProxy::new(&bus).await else {
        warn!("media: cannot create bus proxy; players are not tracked");
        return;
    };
    // Subscribe first, then list, so no change is missed.
    let mut stream = match proxy.receive_name_owner_changed().await {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "media: cannot watch the bus");
            return;
        }
    };
    if let Ok(names) = proxy.list_names().await {
        let found: BTreeSet<String> = names
            .iter()
            .map(|n| n.to_string())
            .filter(|n| n.starts_with(PREFIX))
            .collect();
        info!(players = ?found, "media players");
        if let Ok(mut p) = players.lock() {
            *p = found;
        }
        changes.notify();
    }
    while let Some(sig) = stream.next().await {
        let Ok(args) = sig.args() else { continue };
        let name = args.name().to_string();
        if !name.starts_with(PREFIX) {
            continue;
        }
        let present = args.new_owner().is_some();
        let changed = players.lock().is_ok_and(|mut p| {
            if present {
                p.insert(name.clone())
            } else {
                p.remove(&name)
            }
        });
        if changed {
            info!(player = %name, present, "media player changed");
            changes.notify();
        }
    }
}

impl Action for MediaPlayPause {
    fn info(&self) -> ActionInfo {
        let n = self.players().len();
        ActionInfo {
            id: ID.into(),
            label: "Play / Pause".into(),
            category: "media".into(),
            available: n > 0,
            reason: if self.bus.is_none() {
                "no desktop session bus".into()
            } else if n == 0 {
                "no media player is running".into()
            } else {
                String::new()
            },
            params: Vec::new(),
        }
    }

    fn execute<'a>(&'a self, _params: &'a Params) -> ActionFuture<'a> {
        Box::pin(async move {
            let bus = self
                .bus
                .as_ref()
                .ok_or_else(|| ErrorInfo::new(error_code::UNAVAILABLE, "no desktop session bus"))?;
            let players = self.players();
            let mut target = None;
            for p in &players {
                let status = bus
                    .call_method(
                        Some(p.as_str()),
                        PATH,
                        Some("org.freedesktop.DBus.Properties"),
                        "Get",
                        &(PLAYER, "PlaybackStatus"),
                    )
                    .await
                    .ok()
                    .and_then(|m| m.body().deserialize::<OwnedVariant>().ok())
                    .and_then(|v| String::try_from(v).ok());
                if status.as_deref() == Some("Playing") {
                    target = Some(p.clone());
                    break;
                }
            }
            let target = target.or_else(|| players.first().cloned()).ok_or_else(|| {
                ErrorInfo::new(error_code::UNAVAILABLE, "no media player is running")
            })?;
            bus.call_method(Some(target.as_str()), PATH, Some(PLAYER), "PlayPause", &())
                .await
                .map_err(|e| ErrorInfo::new(error_code::FAILED, format!("{target}: {e}")))?;
            let short = target.trim_start_matches(PREFIX).to_owned();
            info!(player = %short, "media play/pause");
            Ok(json!({ "player": short }))
        })
    }
}
