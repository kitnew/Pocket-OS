//! D-Bus binding of the application catalog: `org.pocketos.Core1.Apps` on
//! `/org/pocketos/Core1/Apps` (contract: `pocket_ipc::dbus::apps`).
//!
//! Discovery and command preparation live in `pocket_platform::apps`; this module only
//! serves the catalog, starts processes, reports their exit, and rescans when the
//! application directories change (inotify, debounced; no polling).
//!
//! Launched applications inherit the daemon's environment, which the session sets up
//! for graphical clients (`WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`,
//! `DBUS_SESSION_BUS_ADDRESS`, `XDG_SESSION_TYPE`, `XDG_CURRENT_DESKTOP`). They run in
//! their own process group, so restarting the core does not stop them; they end with the
//! graphical session.

use std::collections::HashMap;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::time::Duration;

use futures_util::StreamExt;
use inotify::{Inotify, WatchMask};
use pocket_ipc::dbus::apps::{OBJECT_PATH, app_key};
use pocket_platform::apps::{App, Catalog, ScanOptions};
use tracing::{debug, info, warn};
use zbus::{
    Connection, DBusError, Variant, interface, message::Header, object_server::SignalEmitter,
};

/// Time allowed for `org.freedesktop.Application.Activate` before falling back to Exec.
const ACTIVATE_TIMEOUT: Duration = Duration::from_secs(5);
/// Quiet period after a directory change before rescanning (package installs touch many files).
const RESCAN_DEBOUNCE: Duration = Duration::from_millis(700);

pub type AppDict = HashMap<&'static str, Variant<'static>>;

fn app_dict(a: &App) -> AppDict {
    let s = |v: &str| Variant::from(v.to_owned());
    HashMap::from([
        (app_key::ID, s(&a.id)),
        (app_key::NAME, s(&a.name)),
        (app_key::GENERIC_NAME, s(&a.generic_name)),
        (app_key::COMMENT, s(&a.comment)),
        (app_key::ICON_NAME, s(&a.icon_name)),
        (
            app_key::ICON_PATH,
            s(&a.icon_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()),
        ),
        (app_key::CATEGORIES, Variant::from(a.categories.clone())),
        (app_key::KEYWORDS, Variant::from(a.keywords.clone())),
        (
            app_key::STARTUP_WM_CLASS,
            s(a.startup_wm_class.as_deref().unwrap_or_default()),
        ),
    ])
}

/// Errors returned to D-Bus callers as `org.pocketos.Core1.Error.<Variant>`.
#[derive(Debug, DBusError)]
#[zbus(prefix = "org.pocketos.Core1.Error")]
pub enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    /// No application with this ID is in the catalog.
    UnknownApp(String),
    /// The application could not be started.
    LaunchFailed(String),
}

pub struct AppsService {
    opts: ScanOptions,
    catalog: Catalog,
}

impl AppsService {
    pub fn new() -> Self {
        let opts = ScanOptions::from_env();
        let catalog = scan(&opts);
        Self { opts, catalog }
    }

    pub fn dirs(&self) -> Vec<PathBuf> {
        self.opts.dirs.clone()
    }

    /// Rescans; returns whether the catalog changed.
    fn rescan(&mut self) -> bool {
        let catalog = scan(&self.opts);
        let changed = catalog != self.catalog;
        self.catalog = catalog;
        changed
    }
}

fn scan(opts: &ScanOptions) -> Catalog {
    let (catalog, skipped) = Catalog::scan(opts);
    for (path, why) in &skipped {
        debug!(path = %path.display(), reason = ?why, "desktop entry skipped");
    }
    info!(
        apps = catalog.len(),
        skipped = skipped.len(),
        "application catalog scanned"
    );
    catalog
}

/// D-Bus object path of an application for `org.freedesktop.Application` (spec: the ID
/// with `.` → `/`, `-` → `_`).
fn activation_path(id: &str) -> String {
    format!("/{}", id.replace('.', "/").replace('-', "_"))
}

async fn dbus_activate(conn: &Connection, id: &str) -> Result<(), String> {
    let platform_data: (HashMap<&str, Variant<'_>>,) = (HashMap::new(),);
    let path = activation_path(id);
    let call = conn.call_method(
        Some(id),
        path.as_str(),
        Some("org.freedesktop.Application"),
        "Activate",
        &platform_data,
    );
    match tokio::time::timeout(ACTIVATE_TIMEOUT, call).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("timed out".into()),
    }
}

#[interface(name = "org.pocketos.Core1.Apps")]
impl AppsService {
    /// `ListApps() -> aa{sv}`
    fn list_apps(&self) -> Vec<AppDict> {
        self.catalog.apps().iter().map(app_dict).collect()
    }

    /// `Launch(s id) -> u pid` (0 when activated over D-Bus).
    async fn launch(
        &self,
        id: &str,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<u32, Error> {
        let sender = hdr.sender().map_or_else(|| "?".into(), |s| s.to_string());
        let Some(app) = self.catalog.get(id).cloned() else {
            warn!(%sender, id, "rejected Launch: unknown app");
            return Err(Error::UnknownApp(format!("no application {id:?}")));
        };
        info!(%sender, id, argv = ?app.argv, dbus = app.dbus_activatable, "Launch");

        if app.dbus_activatable {
            match dbus_activate(conn, &app.id).await {
                Ok(()) => {
                    info!(id, "activated over D-Bus");
                    return Ok(0);
                }
                Err(e) => warn!(id, error = %e, "D-Bus activation failed; using Exec"),
            }
        }

        let mut child = tokio::process::Command::from(app.command())
            .kill_on_drop(false)
            .spawn()
            .map_err(|e| {
                warn!(id, error = %e, "launch failed");
                Error::LaunchFailed(format!("cannot start {}: {e}", app.argv[0]))
            })?;
        let pid = child.id().unwrap_or(0);
        info!(id, pid, "launched");

        let emitter = emitter.to_owned();
        let id = app.id;
        tokio::spawn(async move {
            let (success, detail) = match child.wait().await {
                Ok(st) => match (st.code(), st.signal()) {
                    (Some(c), _) => (c == 0, format!("status={c}")),
                    (None, Some(s)) => (false, format!("signal={s}")),
                    _ => (false, "unknown".into()),
                },
                Err(e) => (false, format!("wait failed: {e}")),
            };
            info!(id, pid, success, detail, "app exited");
            if let Err(e) = Self::app_exited(&emitter, &id, pid, success, &detail).await {
                warn!(error = %e, "cannot emit AppExited");
            }
        });
        Ok(pid)
    }

    /// `Reload()`: rescan now; emits `AppsChanged` if the catalog changed.
    async fn reload(
        &mut self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        if self.rescan() {
            Self::apps_changed(&emitter).await?;
        }
        Ok(())
    }

    /// `AppsChanged()`
    #[zbus(signal)]
    async fn apps_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;

    /// `AppExited(s id, u pid, b success, s detail)`
    #[zbus(signal)]
    async fn app_exited(
        emitter: &SignalEmitter<'_>,
        id: &str,
        pid: u32,
        success: bool,
        detail: &str,
    ) -> zbus::Result<()>;
}

/// Watches the application directories and rescans after changes. Runs until the
/// connection is dropped. Directories that do not exist are not watched.
pub async fn watch(conn: Connection, dirs: Vec<PathBuf>) {
    let inotify = match Inotify::init() {
        Ok(i) => i,
        Err(e) => {
            warn!(error = %e, "inotify unavailable; application changes need Reload()");
            return;
        }
    };
    let mask = WatchMask::CREATE
        | WatchMask::DELETE
        | WatchMask::MOVED_FROM
        | WatchMask::MOVED_TO
        | WatchMask::CLOSE_WRITE
        | WatchMask::ATTRIB;
    let mut watched = 0;
    for d in &dirs {
        if d.is_dir() {
            match inotify.watches().add(d, mask) {
                Ok(_) => watched += 1,
                Err(e) => warn!(dir = %d.display(), error = %e, "cannot watch"),
            }
        }
    }
    debug!(watched, "watching application directories");
    let mut events = match inotify.into_event_stream([0u8; 4096]) {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "inotify stream failed");
            return;
        }
    };
    while events.next().await.is_some() {
        // Debounce: drain until the directories are quiet.
        while let Ok(Some(_)) = tokio::time::timeout(RESCAN_DEBOUNCE, events.next()).await {}
        let Ok(iface) = conn
            .object_server()
            .interface::<_, AppsService>(OBJECT_PATH)
            .await
        else {
            return;
        };
        let changed = iface.get_mut().await.rescan();
        if changed {
            info!("application catalog changed");
            if let Err(e) = AppsService::apps_changed(iface.signal_emitter()).await {
                warn!(error = %e, "cannot emit AppsChanged");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_paths_follow_the_spec() {
        assert_eq!(activation_path("org.gnome.Maps"), "/org/gnome/Maps");
        assert_eq!(activation_path("org.example.my-app"), "/org/example/my_app");
    }
}
