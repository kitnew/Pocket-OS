//! `media.play_pause`, `media.next`, `media.previous` (basic, ADR-0008): transport
//! controls of the active MPRIS player (see [`crate::mpris`] for how it is chosen).
//!
//! Available while at least one player is on the bus; availability changes are announced
//! to the phone. Result data: `{"player": "<short id>"}`.

use pocket_dock_protocol::error_code;
use pocket_dock_protocol::message::{ActionInfo, ErrorInfo};
use serde_json::json;

use super::{Action, ActionFuture, Params};
use crate::mpris::Mpris;

pub const ID: &str = "media.play_pause";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    Next,
    Previous,
}

impl Command {
    pub const ALL: [Command; 3] = [Command::PlayPause, Command::Next, Command::Previous];

    fn id(self) -> &'static str {
        match self {
            Command::PlayPause => "media.play_pause",
            Command::Next => "media.next",
            Command::Previous => "media.previous",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Command::PlayPause => "Play / Pause",
            Command::Next => "Next track",
            Command::Previous => "Previous track",
        }
    }

    fn method(self) -> &'static str {
        match self {
            Command::PlayPause => "PlayPause",
            Command::Next => "Next",
            Command::Previous => "Previous",
        }
    }
}

pub struct MediaAction {
    cmd: Command,
    mpris: Option<Mpris>,
}

impl MediaAction {
    pub fn new(cmd: Command, mpris: Option<Mpris>) -> Self {
        MediaAction { cmd, mpris }
    }
}

impl Action for MediaAction {
    fn info(&self) -> ActionInfo {
        let reason = match &self.mpris {
            None => Some("no desktop session bus".to_owned()),
            Some(m) if m.player_count() == 0 => Some("no media player is running".to_owned()),
            Some(_) => None,
        };
        ActionInfo::new(self.cmd.id(), self.cmd.label(), "media").unavailable_if(reason)
    }

    fn execute<'a>(&'a self, _params: &'a Params) -> ActionFuture<'a> {
        Box::pin(async move {
            let m = self
                .mpris
                .as_ref()
                .ok_or_else(|| ErrorInfo::new(error_code::UNAVAILABLE, "no desktop session bus"))?;
            let player = m.command(self.cmd.method()).await?;
            Ok(json!({ "player": player }))
        })
    }
}
