//! `session.lock` (elevated, confirm, idempotent; ADR-0008): locks the desktop session
//! through `org.freedesktop.ScreenSaver.Lock` (implemented by KDE Plasma's ksmserver and
//! other desktops). Locking an already locked session changes nothing.

use pocket_dock_protocol::error_code;
use pocket_dock_protocol::message::{ActionInfo, ErrorInfo, Privilege};
use serde_json::json;
use tracing::info;

use super::{Action, ActionFuture, Params};

pub const ID: &str = "session.lock";

pub struct SessionLock {
    bus: Option<zbus::Connection>,
}

impl SessionLock {
    pub fn new(bus: Option<zbus::Connection>) -> Self {
        SessionLock { bus }
    }
}

impl Action for SessionLock {
    fn info(&self) -> ActionInfo {
        ActionInfo {
            privilege: Privilege::Elevated,
            idempotent: true,
            confirm: true,
            ..ActionInfo::new(ID, "Lock screen", "session")
        }
        .unavailable_if(self.bus.is_none().then(|| "no desktop session bus".into()))
    }

    fn execute<'a>(&'a self, _params: &'a Params) -> ActionFuture<'a> {
        Box::pin(async move {
            let bus = self
                .bus
                .as_ref()
                .ok_or_else(|| ErrorInfo::new(error_code::UNAVAILABLE, "no desktop session bus"))?;
            bus.call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/org/freedesktop/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "Lock",
                &(),
            )
            .await
            .map_err(|e| ErrorInfo::new(error_code::FAILED, format!("screen locker: {e}")))?;
            info!("session locked");
            Ok(json!({ "locked": true }))
        })
    }
}
