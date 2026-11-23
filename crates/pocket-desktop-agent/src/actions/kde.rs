//! KDE Plasma integrations through KGlobalAccel (`org.kde.kglobalaccel`): the *named*
//! global shortcuts Plasma itself registers. Invoking one runs the same compositor or
//! Plasma action as pressing its key combination, without synthesizing any input and
//! independent of the user's key bindings (ADR-0008: no input injection).
//!
//! - `volume.up` / `volume.down` / `volume.mute` (basic): Plasma's audio shortcuts
//!   (component `kmix`, served by plasma-pa), with the usual on-screen display.
//! - `desktop.shortcut` (elevated): one of an approved list of KWin shortcuts, chosen by
//!   the `shortcut` parameter (`choices`).
//!
//! Availability: the component must exist (checked at start and whenever kglobalaccel
//! appears or leaves the bus). Without KDE the actions are listed as unavailable.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use pocket_dock_protocol::error_code;
use pocket_dock_protocol::message::{
    ActionInfo, Choice, ErrorInfo, ParamInfo, ParamKind, Privilege,
};
use serde_json::json;
use tracing::{info, warn};

use super::{Action, ActionFuture, Changes, Params};

const SERVICE: &str = "org.kde.kglobalaccel";
const COMPONENT_IFACE: &str = "org.kde.kglobalaccel.Component";

/// (action id, label, kglobalaccel shortcut name in component `kmix`)
pub const VOLUME: [(&str, &str, &str); 3] = [
    ("volume.up", "Volume up", "increase_volume"),
    ("volume.down", "Volume down", "decrease_volume"),
    ("volume.mute", "Mute", "mute"),
];

/// Approved `desktop.shortcut` values: (choice, label, KWin shortcut name).
pub const SHORTCUTS: [(&str, &str, &str); 5] = [
    ("overview", "Overview", "Overview"),
    ("show_desktop", "Show desktop", "Show Desktop"),
    ("desktop_next", "Next desktop", "Switch to Next Desktop"),
    (
        "desktop_previous",
        "Previous desktop",
        "Switch to Previous Desktop",
    ),
    ("grid_view", "Grid view", "Grid View"),
];

/// Shared view of which kglobalaccel components exist and their shortcut names.
#[derive(Clone)]
pub struct GlobalAccel {
    bus: Option<zbus::Connection>,
    components: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

impl GlobalAccel {
    /// Starts watching kglobalaccel in the background (needs a Tokio runtime when `bus`
    /// is set).
    pub fn new(bus: Option<zbus::Connection>, changes: Changes) -> Self {
        let g = GlobalAccel {
            bus,
            components: Arc::default(),
        };
        if let Some(bus) = g.bus.clone() {
            tokio::spawn(g.clone().watch(bus, changes));
        }
        g
    }

    async fn watch(self, bus: zbus::Connection, changes: Changes) {
        let Ok(dbus) = zbus::fdo::DBusProxy::new(&bus).await else {
            return;
        };
        let mut owners = match dbus.receive_name_owner_changed().await {
            Ok(s) => s,
            Err(e) => {
                warn!(error = %e, "kde: cannot watch the bus");
                return;
            }
        };
        self.refresh(&bus).await;
        changes.notify();
        while let Some(sig) = owners.next().await {
            if sig.args().is_ok_and(|a| a.name() == SERVICE) {
                self.refresh(&bus).await;
                changes.notify();
            }
        }
    }

    async fn refresh(&self, bus: &zbus::Connection) {
        let mut found = HashMap::new();
        for component in ["kmix", "kwin"] {
            let path = format!("/component/{component}");
            let names = bus
                .call_method(
                    Some(SERVICE),
                    path.as_str(),
                    Some(COMPONENT_IFACE),
                    "shortcutNames",
                    &(),
                )
                .await
                .ok()
                .and_then(|m| m.body().deserialize::<Vec<String>>().ok());
            if let Some(names) = names {
                found.insert(component.to_owned(), names);
            }
        }
        info!(components = ?found.keys().collect::<Vec<_>>(), "kde: global shortcuts");
        if let Ok(mut c) = self.components.lock() {
            *c = found;
        }
    }

    /// Why `component`/`shortcut` cannot be invoked now (None = it can).
    fn missing(&self, component: &str, shortcut: &str) -> Option<String> {
        if self.bus.is_none() {
            return Some("no desktop session bus".into());
        }
        let c = self.components.lock().ok()?;
        match c.get(component) {
            None => Some(format!(
                "KDE global shortcuts ({component}) are not available"
            )),
            Some(names) if !names.iter().any(|n| n == shortcut) => {
                Some(format!("KDE has no {shortcut:?} shortcut"))
            }
            Some(_) => None,
        }
    }

    async fn invoke(&self, component: &str, shortcut: &str) -> Result<(), ErrorInfo> {
        if let Some(r) = self.missing(component, shortcut) {
            return Err(ErrorInfo::new(error_code::UNAVAILABLE, r));
        }
        let bus = self.bus.as_ref().expect("checked by missing()");
        let path = format!("/component/{component}");
        bus.call_method(
            Some(SERVICE),
            path.as_str(),
            Some(COMPONENT_IFACE),
            "invokeShortcut",
            &(shortcut,),
        )
        .await
        .map_err(|e| ErrorInfo::new(error_code::FAILED, format!("{shortcut}: {e}")))?;
        info!(component, shortcut, "kde: shortcut invoked");
        Ok(())
    }
}

pub struct VolumeAction {
    id: &'static str,
    label: &'static str,
    shortcut: &'static str,
    kde: GlobalAccel,
}

impl VolumeAction {
    pub fn new(v: (&'static str, &'static str, &'static str), kde: GlobalAccel) -> Self {
        VolumeAction {
            id: v.0,
            label: v.1,
            shortcut: v.2,
            kde,
        }
    }
}

impl Action for VolumeAction {
    fn info(&self) -> ActionInfo {
        ActionInfo {
            // Mute toggles; up/down are steps: none of them is idempotent.
            ..ActionInfo::new(self.id, self.label, "media")
        }
        .unavailable_if(self.kde.missing("kmix", self.shortcut))
    }

    fn execute<'a>(&'a self, _params: &'a Params) -> ActionFuture<'a> {
        Box::pin(async move {
            self.kde.invoke("kmix", self.shortcut).await?;
            Ok(json!({ "shortcut": self.shortcut }))
        })
    }
}

pub const SHORTCUT_ID: &str = "desktop.shortcut";

pub struct ShortcutAction {
    kde: GlobalAccel,
}

impl ShortcutAction {
    pub fn new(kde: GlobalAccel) -> Self {
        ShortcutAction { kde }
    }
}

impl Action for ShortcutAction {
    fn info(&self) -> ActionInfo {
        let mut p = ParamInfo::new("shortcut", ParamKind::String, true);
        p.choices = SHORTCUTS
            .iter()
            .map(|(v, l, _)| Choice {
                value: (*v).into(),
                label: (*l).into(),
            })
            .collect();
        ActionInfo {
            params: vec![p],
            privilege: Privilege::Elevated,
            ..ActionInfo::new(SHORTCUT_ID, "Desktop shortcut", "desktop")
        }
        .unavailable_if(self.kde.missing("kwin", "Overview"))
    }

    fn execute<'a>(&'a self, params: &'a Params) -> ActionFuture<'a> {
        Box::pin(async move {
            let choice = params
                .get("shortcut")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            // `choices` were checked by the registry; map to the KWin name.
            let Some((_, _, name)) = SHORTCUTS.iter().find(|(v, _, _)| *v == choice) else {
                return Err(ErrorInfo::new(
                    error_code::NOT_PERMITTED,
                    "unknown shortcut",
                ));
            };
            self.kde.invoke("kwin", name).await?;
            Ok(json!({ "shortcut": choice }))
        })
    }
}
