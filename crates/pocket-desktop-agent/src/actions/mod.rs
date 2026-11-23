//! The action registry and the [`Action`] trait.
//!
//! An action is a stable id, a label, typed parameters, an availability, and an async
//! `execute`. The registry validates every invocation before an action sees it:
//! id syntax → registered (`unknown_action`) → enabled in the configuration
//! (`not_permitted`) → available now (`unavailable`) → parameters match the declared
//! names and types (`invalid_params`). Actions report availability changes through
//! [`Changes`], and the server forwards them to the phone as `actions` messages.

pub mod media;
pub mod notify;

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pocket_dock_protocol::error_code;
use pocket_dock_protocol::message::{ActionInfo, ErrorInfo, ParamKind};
use serde_json::{Map, Value};
use tokio::sync::watch;

pub type Params = Map<String, Value>;
pub type ActionResult = Result<Value, ErrorInfo>;
pub type ActionFuture<'a> = Pin<Box<dyn Future<Output = ActionResult> + Send + 'a>>;

/// One desktop action. `info()` must be cheap (it is called for every action listing);
/// keep expensive availability checks in a background task and cache the result.
pub trait Action: Send + Sync {
    fn info(&self) -> ActionInfo;
    /// Runs the action. `params` has already been validated against `info().params`.
    fn execute<'a>(&'a self, params: &'a Params) -> ActionFuture<'a>;
}

/// Signals that the action list (availability) changed.
#[derive(Clone)]
pub struct Changes(Arc<watch::Sender<u64>>);

impl Changes {
    pub fn notify(&self) {
        self.0.send_modify(|n| *n += 1);
    }
}

pub struct Registry {
    actions: Vec<Arc<dyn Action>>,
    /// `None` = every registered action is enabled.
    enabled: Option<HashSet<String>>,
    changes: Arc<watch::Sender<u64>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            actions: Vec::new(),
            enabled: None,
            changes: Arc::new(watch::channel(0).0),
        }
    }

    /// Restricts the advertised and executable actions to `ids`.
    pub fn set_enabled(&mut self, ids: Option<impl IntoIterator<Item = String>>) {
        self.enabled = ids.map(|i| i.into_iter().collect());
    }

    /// Handle actions use to announce availability changes.
    pub fn changes(&self) -> Changes {
        Changes(self.changes.clone())
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    /// Adds an action. Panics on an invalid or duplicate id (a programming error).
    pub fn register(&mut self, action: impl Action + 'static) {
        let id = action.info().id;
        assert!(
            pocket_dock_protocol::is_valid_action_id(&id),
            "invalid action id {id:?}"
        );
        assert!(self.find(&id).is_none(), "duplicate action id {id:?}");
        self.actions.push(Arc::new(action));
    }

    fn find(&self, id: &str) -> Option<&Arc<dyn Action>> {
        self.actions.iter().find(|a| a.info().id == id)
    }

    fn is_enabled(&self, id: &str) -> bool {
        self.enabled.as_ref().is_none_or(|e| e.contains(id))
    }

    /// Enabled actions, in registration order.
    pub fn list(&self) -> Vec<ActionInfo> {
        self.actions
            .iter()
            .map(|a| a.info())
            .filter(|i| self.is_enabled(&i.id))
            .collect()
    }

    /// Validates and runs one invocation.
    pub async fn invoke(&self, id: &str, params: &Params) -> ActionResult {
        let Some(action) = self
            .find(id)
            .filter(|_| pocket_dock_protocol::is_valid_action_id(id))
        else {
            return Err(ErrorInfo::new(
                error_code::UNKNOWN_ACTION,
                format!("no action {id:?} on this computer"),
            ));
        };
        if !self.is_enabled(id) {
            return Err(ErrorInfo::new(
                error_code::NOT_PERMITTED,
                format!("action {id:?} is disabled in the agent configuration"),
            ));
        }
        let info = action.info();
        if !info.available {
            return Err(ErrorInfo::new(error_code::UNAVAILABLE, info.reason));
        }
        validate_params(&info, params)?;
        action.execute(params).await
    }
}

fn validate_params(info: &ActionInfo, params: &Params) -> Result<(), ErrorInfo> {
    let invalid = |m: String| Err(ErrorInfo::new(error_code::INVALID_PARAMS, m));
    for (name, value) in params {
        let Some(p) = info.params.iter().find(|p| &p.name == name) else {
            return invalid(format!("unknown parameter {name:?}"));
        };
        let ok = match p.kind {
            ParamKind::String => value.is_string(),
            ParamKind::Bool => value.is_boolean(),
            ParamKind::Integer => value.is_i64() || value.is_u64(),
        };
        if !ok {
            return invalid(format!("parameter {name:?} must be {:?}", p.kind));
        }
    }
    for p in info.params.iter().filter(|p| p.required) {
        if !params.contains_key(&p.name) {
            return invalid(format!("missing parameter {:?}", p.name));
        }
    }
    Ok(())
}

/// Registers the built-in desktop actions. `bus` is the user's session bus (`None` when
/// it is unavailable: the actions are then listed as unavailable).
pub fn builtin(registry: &mut Registry, bus: Option<zbus::Connection>) {
    registry.register(notify::DesktopNotify::new(bus.clone()));
    registry.register(media::MediaPlayPause::new(bus, registry.changes()));
}

/// Reads an optional string parameter with a default and a length limit (characters).
pub fn string_param<'a>(
    params: &'a Params,
    name: &str,
    default: &'a str,
    max_chars: usize,
) -> Result<&'a str, ErrorInfo> {
    let v = params.get(name).and_then(Value::as_str).unwrap_or(default);
    if v.chars().count() > max_chars {
        return Err(ErrorInfo::new(
            error_code::INVALID_PARAMS,
            format!("parameter {name:?} is longer than {max_chars} characters"),
        ));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pocket_dock_protocol::message::ParamInfo;
    use serde_json::json;

    struct Echo(bool);
    impl Action for Echo {
        fn info(&self) -> ActionInfo {
            ActionInfo {
                id: "test.echo".into(),
                label: "Echo".into(),
                category: "test".into(),
                available: self.0,
                reason: if self.0 { String::new() } else { "off".into() },
                params: vec![
                    ParamInfo {
                        name: "text".into(),
                        kind: ParamKind::String,
                        required: true,
                    },
                    ParamInfo {
                        name: "n".into(),
                        kind: ParamKind::Integer,
                        required: false,
                    },
                ],
            }
        }
        fn execute<'a>(&'a self, params: &'a Params) -> ActionFuture<'a> {
            Box::pin(async move { Ok(Value::Object(params.clone())) })
        }
    }

    fn p(v: Value) -> Params {
        v.as_object().unwrap().clone()
    }

    fn code(r: ActionResult) -> String {
        r.unwrap_err().code
    }

    #[tokio::test]
    async fn invocation_is_validated_before_execution() {
        let mut r = Registry::new();
        r.register(Echo(true));
        let ok = r
            .invoke("test.echo", &p(json!({"text": "hi", "n": 2})))
            .await;
        assert_eq!(ok.unwrap(), json!({"text": "hi", "n": 2}));
        assert_eq!(
            code(r.invoke("test.nope", &p(json!({}))).await),
            "unknown_action"
        );
        assert_eq!(
            code(r.invoke("../x", &p(json!({}))).await),
            "unknown_action"
        );
        assert_eq!(
            code(r.invoke("test.echo", &p(json!({}))).await),
            "invalid_params"
        );
        assert_eq!(
            code(r.invoke("test.echo", &p(json!({"text": 1}))).await),
            "invalid_params"
        );
        assert_eq!(
            code(
                r.invoke("test.echo", &p(json!({"text": "a", "cmd": "rm"})))
                    .await
            ),
            "invalid_params"
        );
        r.set_enabled(Some(Vec::<String>::new()));
        assert!(r.list().is_empty());
        assert_eq!(
            code(r.invoke("test.echo", &p(json!({"text": "a"}))).await),
            "not_permitted"
        );
    }

    #[tokio::test]
    async fn unavailable_actions_are_listed_but_refused() {
        let mut r = Registry::new();
        r.register(Echo(false));
        assert!(!r.list()[0].available);
        assert_eq!(
            code(r.invoke("test.echo", &p(json!({"text": "a"}))).await),
            "unavailable"
        );
    }
}
