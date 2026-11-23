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

use super::{Action, ActionFuture, Changes, Lists, Params};

/// The installed-applications catalog, scanned in the background and shared by
/// `app.launch` and `desktop.open`.
pub type SharedCatalog = Arc<Mutex<Option<Catalog>>>;

pub const ID: &str = "app.launch";

pub struct AppLaunch {
    lists: Arc<Lists>,
    catalog: SharedCatalog,
}

pub fn scan() -> Catalog {
    let mut opts = ScanOptions::from_env();
    opts.resolve_icons = false;
    Catalog::scan(&opts).0
}

impl AppLaunch {
    /// Scans the catalog in the background when a Tokio runtime is running.
    pub fn new(lists: Arc<Lists>, changes: Changes) -> Self {
        let a = AppLaunch {
            lists,
            catalog: Arc::default(),
        };
        if tokio::runtime::Handle::try_current().is_ok() {
            let (cat, ch) = (a.catalog.clone(), changes);
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
    pub fn with_catalog(registered: Vec<String>, catalog: Catalog, _changes: Changes) -> Self {
        let lists = Arc::new(Lists::default());
        if let Ok(mut a) = lists.apps.write() {
            *a = registered;
        }
        AppLaunch {
            lists,
            catalog: Arc::new(Mutex::new(Some(catalog))),
        }
    }

    /// The catalog this action scanned (shared with `desktop.open`).
    pub fn catalog(&self) -> SharedCatalog {
        self.catalog.clone()
    }
}

/// Finds `id` in the catalog, rescanning once if it is missing (newly installed).
pub async fn find_app(catalog: &SharedCatalog, id: &str) -> Result<App, ErrorInfo> {
    let cached = catalog
        .lock()
        .ok()
        .and_then(|c| c.as_ref().and_then(|c| c.get(id).cloned()));
    if let Some(a) = cached {
        return Ok(a);
    }
    let fresh = tokio::task::spawn_blocking(scan).await.ok();
    let found = fresh.as_ref().and_then(|c| c.get(id).cloned());
    if let (Some(c), Ok(mut slot)) = (fresh, catalog.lock()) {
        *slot = Some(c);
    }
    found.ok_or_else(|| ErrorInfo::new(error_code::UNAVAILABLE, format!("{id} is not installed")))
}

static SYSTEMD_RUN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Start applications as transient systemd user services (`systemd-run --user`) instead
/// of children of the agent. Set by `pocket-desktop-agent run` when a user manager is
/// present. The agent itself runs sandboxed (read-only home, private /tmp); an app
/// started as its child would inherit that sandbox and its cgroup (it could not save
/// files, and restarting the agent would kill it). A transient unit starts it like any
/// desktop launcher does: the user manager's environment, its own cgroup, no sandbox.
pub fn use_systemd_run(on: bool) {
    SYSTEMD_RUN.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// `cmd` wrapped in `systemd-run --user` (same program, arguments and directory; still
/// no shell).
fn transient(cmd: &std::process::Command, what: &str) -> std::process::Command {
    let safe: String = what
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0);
    let mut run = std::process::Command::new("systemd-run");
    run.args(["--user", "--collect", "--quiet", "--no-block"])
        .arg(format!("--unit=app-pocket-dock-{safe}-{nonce}"))
        .arg(format!("--description={what} (Pocket Dock)"));
    if let Some(dir) = cmd.get_current_dir() {
        run.arg(format!("--working-directory={}", dir.display()));
    }
    run.arg("--").arg(cmd.get_program()).args(cmd.get_args());
    run.stdin(std::process::Stdio::null());
    run
}

/// Starts `cmd` (as a transient user service when enabled, else as a detached child) and
/// logs a failing exit. Returns the pid of the child it spawned.
pub fn spawn_logged(cmd: std::process::Command, what: &str) -> Result<u32, ErrorInfo> {
    let systemd = SYSTEMD_RUN.load(std::sync::atomic::Ordering::Relaxed);
    let cmd = if systemd { transient(&cmd, what) } else { cmd };
    let mut cmd = tokio::process::Command::from(cmd);
    cmd.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| ErrorInfo::new(error_code::FAILED, format!("{what}: {e}")))?;
    let pid = child.id().unwrap_or(0);
    let name = what.to_owned();
    tokio::spawn(async move {
        match child.wait().await {
            Ok(s) if !s.success() => warn!(app = %name, status = %s, "apps: exited with failure"),
            Err(e) => warn!(app = %name, error = %e, "apps: wait failed"),
            _ => {}
        }
    });
    Ok(pid)
}

impl Action for AppLaunch {
    fn info(&self) -> ActionInfo {
        let catalog = self.catalog.lock().ok().and_then(|c| c.clone());
        let registered = self.lists.apps();
        let mut p = ParamInfo::new("app", ParamKind::String, true);
        p.choices = registered
            .iter()
            .filter_map(|id| catalog.as_ref().and_then(|c| c.get(id)))
            .map(|a| Choice {
                value: a.id.clone(),
                label: a.name.clone(),
            })
            .collect();
        let reason = if registered.is_empty() {
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
            if !self.lists.apps().contains(&id) {
                return Err(ErrorInfo::new(
                    error_code::NOT_PERMITTED,
                    format!("{id:?} is not registered"),
                ));
            }
            let app = find_app(&self.catalog, &id).await?;
            let pid = spawn_logged(app.command(), &app.id)?;
            info!(app = %app.id, pid, "apps: launched");
            Ok(json!({ "app": app.id, "name": app.name, "pid": pid }))
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn transient_units_keep_argv_without_a_shell() {
        let mut c = std::process::Command::new("/usr/bin/code");
        c.args(["--new-window", "/tmp/a b;c"])
            .current_dir("/home/u");
        let t = super::transient(&c, "code/x");
        assert_eq!(t.get_program(), "systemd-run");
        let args: Vec<_> = t
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            args[4].starts_with("--unit=app-pocket-dock-code_x-"),
            "{args:?}"
        );
        assert_eq!(args[6], "--working-directory=/home/u");
        assert_eq!(
            &args[7..],
            ["--", "/usr/bin/code", "--new-window", "/tmp/a b;c"]
        );
    }
}
