//! `desktop.open` (elevated; issue #16): opens one of the destinations the laptop owner
//! configured (`destinations` in config.json: a project folder in the editor, a folder
//! in the file manager, a bookmarked page). The `destination` parameter can only be one
//! of their ids (`choices`); the phone never supplies a path, URL or program.
//!
//! The target is passed to the application through its desktop entry's own `%f`/`%u`
//! field code (`pocket_platform::apps::App::open_command`): no shell, no extra
//! arguments. Local targets must exist when opened.

use std::path::Path;
use std::sync::Arc;

use pocket_dock_protocol::error_code;
use pocket_dock_protocol::message::{
    ActionInfo, Choice, ErrorInfo, ParamInfo, ParamKind, Privilege,
};
use serde_json::json;
use tracing::info;

use super::apps::{SharedCatalog, find_app, spawn_logged};
use super::{Action, ActionFuture, Lists, Params};

pub const ID: &str = "desktop.open";

pub struct OpenDestination {
    lists: Arc<Lists>,
    catalog: SharedCatalog,
}

impl OpenDestination {
    pub fn new(lists: Arc<Lists>, catalog: SharedCatalog) -> Self {
        OpenDestination { lists, catalog }
    }
}

impl Action for OpenDestination {
    fn info(&self) -> ActionInfo {
        let dests = self.lists.destinations();
        let mut p = ParamInfo::new("destination", ParamKind::String, true);
        p.choices = dests
            .iter()
            .map(|d| Choice {
                value: d.id.clone(),
                label: d.label.clone(),
            })
            .collect();
        let reason = dests.is_empty().then(|| {
            "no destinations configured (destinations in the agent's config.json)".to_owned()
        });
        ActionInfo {
            params: vec![p],
            privilege: Privilege::Elevated,
            ..ActionInfo::new(ID, "Open destination", "desktop")
        }
        .unavailable_if(reason)
    }

    fn execute<'a>(&'a self, params: &'a Params) -> ActionFuture<'a> {
        Box::pin(async move {
            let id = params
                .get("destination")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let Some(d) = self.lists.destinations().into_iter().find(|d| d.id == id) else {
                return Err(ErrorInfo::new(
                    error_code::NOT_PERMITTED,
                    format!("no destination {id:?}"),
                ));
            };
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
            let target = d.resolved_target(home.as_deref());
            if target.starts_with('/') && !Path::new(&target).exists() {
                return Err(ErrorInfo::new(
                    error_code::UNAVAILABLE,
                    format!("{} does not exist", d.target),
                ));
            }
            let app = find_app(&self.catalog, &d.app).await?;
            let locales = freedesktop_locales();
            let cmd = app
                .open_command(&[target.as_str()], &locales)
                .map_err(|e| ErrorInfo::new(error_code::UNAVAILABLE, e))?;
            let pid = spawn_logged(cmd, &app.id)?;
            info!(destination = %d.id, app = %app.id, pid, "desktop.open: opened");
            Ok(json!({ "destination": d.id, "app": app.id, "name": app.name, "pid": pid }))
        })
    }
}

fn freedesktop_locales() -> Vec<String> {
    pocket_platform::apps::ScanOptions::from_env().locales
}
