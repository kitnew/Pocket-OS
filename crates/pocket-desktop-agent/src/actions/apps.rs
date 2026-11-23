//! `app.launch` (elevated; ADR-0008): starts one of the applications the laptop owner
//! registered (`apps` in config.json, desktop-file IDs). The `app` parameter can only be
//! one of them (`choices`); the phone can never name a program, a path or arguments.
//!
//! The command comes from the desktop entry through `pocket-platform` (the same catalog
//! and safe `Exec` expansion the phone uses, ADR-0005): no shell, field codes for files
//! and URLs expand to nothing. The catalog is scanned in the background at start and
//! again when a registered app is not found. The process inherits the agent's (systemd
//! user) environment, which Plasma populates with the Wayland/X11 display.

use std::sync::{Arc, Mutex};

use pocket_dock_protocol::error_code;
use pocket_dock_protocol::message::{
    ActionInfo, Choice, ErrorInfo, ParamInfo, ParamKind, Privilege,
};
use pocket_platform::apps::{App, Catalog, ScanOptions};
use serde_json::json;
use tracing::{info, warn};

use super::{Action, ActionFuture, Changes, Params};

pub const ID: &str = "app.launch";

pub struct AppLaunch {
    registered: Vec<String>,
    catalog: Arc<Mutex<Option<Catalog>>>,
    changes: Changes,
}

fn scan() -> Catalog {
    let mut opts = ScanOptions::from_env();
    opts.resolve_icons = false;
    Catalog::scan(&opts).0
}

impl AppLaunch {
    /// Scans the catalog in the background when a Tokio runtime is running.
    pub fn new(registered: Vec<String>, changes: Changes) -> Self {
        let a = AppLaunch {
            registered,
            catalog: Arc::default(),
            changes,
        };
        if !a.registered.is_empty() && tokio::runtime::Handle::try_current().is_ok() {
            let (cat, ch) = (a.catalog.clone(), a.changes.clone());
            tokio::task::spawn_blocking(move || {
                let c = scan();
                info!(apps = c.len(), "apps: catalog scanned");
                if let Ok(mut slot) = cat.lock() {
                    *slot = Some(c);
                }
                ch.notify();
            });
        }
        a
    }

    /// For tests: a fixed catalog.
    pub fn with_catalog(registered: Vec<String>, catalog: Catalog, changes: Changes) -> Self {
        AppLaunch {
            registered,
            catalog: Arc::new(Mutex::new(Some(catalog))),
            changes,
        }
    }

    fn find(&self, id: &str) -> Option<App> {
        self.catalog.lock().ok()?.as_ref()?.get(id).cloned()
    }
}

impl Action for AppLaunch {
    fn info(&self) -> ActionInfo {
        let catalog = self.catalog.lock().ok().and_then(|c| c.clone());
        let mut p = ParamInfo::new("app", ParamKind::String, true);
        p.choices = self
            .registered
            .iter()
            .filter_map(|id| catalog.as_ref().and_then(|c| c.get(id)))
            .map(|a| Choice {
                value: a.id.clone(),
                label: a.name.clone(),
            })
            .collect();
        let reason = if self.registered.is_empty() {
            Some(
                "no applications registered (pocket-desktop-agent apps add <desktop-id>)"
                    .to_owned(),
            )
        } else if catalog.is_none() {
            Some("reading the installed applications…".to_owned())
        } else if p.choices.is_empty() {
            Some("none of the registered applications is installed".to_owned())
        } else {
            None
        };
        ActionInfo {
            params: vec![p],
            privilege: Privilege::Elevated,
            ..ActionInfo::new(ID, "Open application", "apps")
        }
        .unavailable_if(reason)
    }

    fn execute<'a>(&'a self, params: &'a Params) -> ActionFuture<'a> {
        Box::pin(async move {
            let id = params
                .get("app")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned();
            if !self.registered.contains(&id) {
                return Err(ErrorInfo::new(
                    error_code::NOT_PERMITTED,
                    format!("{id:?} is not registered"),
                ));
            }
            let app = match self.find(&id) {
                Some(a) => a,
                None => {
                    let cat = self.catalog.clone();
                    let fresh = tokio::task::spawn_blocking(scan).await.ok();
                    let found = fresh.as_ref().and_then(|c| c.get(&id).cloned());
                    if let (Some(c), Ok(mut slot)) = (fresh, cat.lock()) {
                        *slot = Some(c);
                    }
                    found.ok_or_else(|| {
                        ErrorInfo::new(error_code::UNAVAILABLE, format!("{id} is not installed"))
                    })?
                }
            };
            let mut cmd = tokio::process::Command::from(app.command());
            cmd.stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            let mut child = cmd
                .spawn()
                .map_err(|e| ErrorInfo::new(error_code::FAILED, format!("{}: {e}", app.name)))?;
            let pid = child.id().unwrap_or(0);
            info!(app = %app.id, pid, "apps: launched");
            let name = app.id.clone();
            tokio::spawn(async move {
                match child.wait().await {
                    Ok(s) if !s.success() => {
                        warn!(app = %name, status = %s, "apps: exited with failure")
                    }
                    Err(e) => warn!(app = %name, error = %e, "apps: wait failed"),
                    _ => {}
                }
            });
            Ok(json!({ "app": app.id, "name": app.name, "pid": pid }))
        })
    }
}
