//! `desktop.notify`: shows a desktop notification through the session's notification
//! service (`org.freedesktop.Notifications`, e.g. Plasma or GNOME Shell).
//!
//! Parameters (all optional): `title` (≤ 120 characters), `body` (≤ 500). The body is
//! escaped, so it can never inject notification markup or links.
//! Result data: `{"notification_id": <u32>}` as returned by the notification server, i.e.
//! the server accepted and is showing the notification.

use std::collections::HashMap;

use pocket_dock_protocol::error_code;
use pocket_dock_protocol::message::{ActionInfo, ErrorInfo, ParamInfo, ParamKind};
use serde_json::json;
use tracing::info;
use zbus::Variant;

use super::{Action, ActionFuture, Params, string_param};

pub const ID: &str = "desktop.notify";

pub struct DesktopNotify {
    bus: Option<zbus::Connection>,
}

impl DesktopNotify {
    pub fn new(bus: Option<zbus::Connection>) -> Self {
        DesktopNotify { bus }
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl Action for DesktopNotify {
    fn info(&self) -> ActionInfo {
        ActionInfo {
            id: ID.into(),
            label: "Desktop notification".into(),
            category: "desktop".into(),
            available: self.bus.is_some(),
            reason: if self.bus.is_some() {
                String::new()
            } else {
                "no desktop session bus".into()
            },
            params: vec![
                ParamInfo {
                    name: "title".into(),
                    kind: ParamKind::String,
                    required: false,
                },
                ParamInfo {
                    name: "body".into(),
                    kind: ParamKind::String,
                    required: false,
                },
            ],
        }
    }

    fn execute<'a>(&'a self, params: &'a Params) -> ActionFuture<'a> {
        Box::pin(async move {
            let bus = self
                .bus
                .as_ref()
                .ok_or_else(|| ErrorInfo::new(error_code::UNAVAILABLE, "no desktop session bus"))?;
            let title = string_param(params, "title", "Pocket OS", 120)?;
            let body = escape(string_param(params, "body", "Hello from your phone", 500)?);
            let hints: HashMap<&str, Variant<'_>> =
                HashMap::from([("desktop-entry", Variant::from("pocket-desktop-agent"))]);
            let reply = bus
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "Notify",
                    &(
                        "Pocket OS",
                        0u32,
                        "phone",
                        title,
                        body.as_str(),
                        Vec::<&str>::new(),
                        hints,
                        -1i32,
                    ),
                )
                .await
                .map_err(|e| {
                    ErrorInfo::new(error_code::FAILED, format!("notification service: {e}"))
                })?;
            let id: u32 = reply.body().deserialize().map_err(|e| {
                ErrorInfo::new(error_code::FAILED, format!("unexpected reply: {e}"))
            })?;
            info!(notification_id = id, "desktop notification shown");
            Ok(json!({ "notification_id": id }))
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn markup_is_escaped() {
        assert_eq!(
            super::escape("<a href='x'>&</a>"),
            "&lt;a href='x'&gt;&amp;&lt;/a&gt;"
        );
    }
}
