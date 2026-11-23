//! The action registry and the [`Action`] trait.
//!
//! An action is a stable id, a label, typed parameters, a privilege class, an
//! availability, and an async `execute`. The registry validates and authorizes every
//! invocation before an action sees it (ADR-0007, ADR-0008):
//!
//! 1. id syntax and registered → else `unknown_action`
//! 2. enabled in the configuration → else `not_permitted`
//! 3. **granted** when the action is `elevated` → else `not_permitted`
//! 4. available now → else `unavailable`
//! 5. parameters: known names, declared types, required present, integer bounds →
//!    else `invalid_params`; string values outside a parameter's `choices` (the laptop's
//!    closed list: registered apps, approved shortcuts, workflows) → `not_permitted`
//!
//! Actions report availability changes through [`Changes`]; the server forwards them to
//! the phone as `actions` messages. Workflows (`workflow.run`) are run by the registry
//! itself: each step goes through the same checks, so a workflow can never do more than
//! its steps are allowed to.

pub mod apps;
pub mod kde;
pub mod media;
pub mod notify;
pub mod session;

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pocket_dock_protocol::error_code;
use pocket_dock_protocol::message::{
    ActionInfo, Choice, ErrorInfo, ParamInfo, ParamKind, Privilege,
};
use serde_json::{Map, Value, json};
use tokio::sync::{mpsc, watch};

use crate::config::WorkflowConfig;

pub type Params = Map<String, Value>;
pub type ActionResult = Result<Value, ErrorInfo>;
pub type ActionFuture<'a> = Pin<Box<dyn Future<Output = ActionResult> + Send + 'a>>;
/// Workflow progress: (step, total, label).
pub type Progress = mpsc::UnboundedSender<(u32, u32, String)>;

pub const WORKFLOW_ID: &str = "workflow.run";

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
    /// Elevated actions the laptop owner granted.
    grants: HashSet<String>,
    workflows: Vec<WorkflowConfig>,
    changes: Arc<watch::Sender<u64>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

fn not_granted(id: &str) -> String {
    format!("not granted on this computer (pocket-desktop-agent grant {id})")
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            actions: Vec::new(),
            enabled: None,
            grants: HashSet::new(),
            workflows: Vec::new(),
            changes: Arc::new(watch::channel(0).0),
        }
    }

    /// Restricts the advertised and executable actions to `ids`.
    pub fn set_enabled(&mut self, ids: Option<impl IntoIterator<Item = String>>) {
        self.enabled = ids.map(|i| i.into_iter().collect());
    }

    /// The elevated actions this computer allows.
    pub fn set_grants(&mut self, ids: impl IntoIterator<Item = String>) {
        self.grants = ids.into_iter().collect();
    }

    /// Configured workflows (`workflow.run` is advertised when there is at least one).
    pub fn set_workflows(&mut self, workflows: Vec<WorkflowConfig>) {
        self.workflows = workflows;
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
        assert!(
            id != WORKFLOW_ID && self.find(&id).is_none(),
            "duplicate action id {id:?}"
        );
        self.actions.push(Arc::new(action));
    }

    fn find(&self, id: &str) -> Option<&Arc<dyn Action>> {
        self.actions.iter().find(|a| a.info().id == id)
    }

    fn is_enabled(&self, id: &str) -> bool {
        self.enabled.as_ref().is_none_or(|e| e.contains(id))
    }

    fn workflow_info(&self) -> Option<ActionInfo> {
        if self.workflows.is_empty() {
            return None;
        }
        let mut p = ParamInfo::new("workflow", ParamKind::String, true);
        p.choices = self
            .workflows
            .iter()
            .map(|w| Choice {
                value: w.id.clone(),
                label: if w.label.is_empty() {
                    w.id.clone()
                } else {
                    w.label.clone()
                },
            })
            .collect();
        Some(ActionInfo {
            params: vec![p],
            privilege: Privilege::Elevated,
            ..ActionInfo::new(WORKFLOW_ID, "Run workflow", "workflow")
        })
    }

    /// The action's advertisement with authorization applied (ungranted elevated actions
    /// are listed as unavailable, with the reason).
    fn authorized_info(&self, mut info: ActionInfo) -> ActionInfo {
        if info.privilege == Privilege::Elevated && !self.grants.contains(&info.id) {
            info.available = false;
            info.reason = not_granted(&info.id);
        }
        info
    }

    /// Enabled actions, in registration order (`workflow.run` last).
    pub fn list(&self) -> Vec<ActionInfo> {
        self.actions
            .iter()
            .map(|a| a.info())
            .chain(self.workflow_info())
            .filter(|i| self.is_enabled(&i.id))
            .map(|i| self.authorized_info(i))
            .collect()
    }

    /// Validates, authorizes and runs one invocation.
    pub async fn invoke(&self, id: &str, params: &Params) -> ActionResult {
        self.invoke_with_progress(id, params, None).await
    }

    pub async fn invoke_with_progress(
        &self,
        id: &str,
        params: &Params,
        progress: Option<&Progress>,
    ) -> ActionResult {
        let info = self.check(id, params)?;
        if info.id == WORKFLOW_ID {
            return self.run_workflow(params, progress).await;
        }
        match self.find(id) {
            Some(a) => a.execute(params).await,
            None => Err(ErrorInfo::new(
                error_code::UNKNOWN_ACTION,
                format!("no action {id:?}"),
            )),
        }
    }

    /// Steps 1–5 of the module documentation; returns the advertisement.
    fn check(&self, id: &str, params: &Params) -> Result<ActionInfo, ErrorInfo> {
        let info = if pocket_dock_protocol::is_valid_action_id(id) {
            if id == WORKFLOW_ID {
                self.workflow_info()
            } else {
                self.find(id).map(|a| a.info())
            }
        } else {
            None
        };
        let Some(info) = info else {
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
        if info.privilege == Privilege::Elevated && !self.grants.contains(id) {
            return Err(ErrorInfo::new(error_code::NOT_PERMITTED, not_granted(id)));
        }
        if !info.available {
            return Err(ErrorInfo::new(error_code::UNAVAILABLE, info.reason));
        }
        validate_params(&info, params)?;
        Ok(info)
    }

    async fn run_workflow(&self, params: &Params, progress: Option<&Progress>) -> ActionResult {
        let wid = params
            .get("workflow")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(w) = self.workflows.iter().find(|w| w.id == wid) else {
            return Err(ErrorInfo::new(
                error_code::NOT_PERMITTED,
                format!("no workflow {wid:?}"),
            ));
        };
        // Every step must be allowed before the first one runs: a workflow never stops
        // half-way because of a missing grant.
        for s in &w.steps {
            if s.action == WORKFLOW_ID {
                return Err(ErrorInfo::new(
                    error_code::NOT_PERMITTED,
                    "workflows cannot nest",
                ));
            }
            self.check(&s.action, &s.params).map_err(|e| {
                ErrorInfo::new(&e.code, format!("step {:?}: {}", s.action, e.message))
            })?;
        }
        let total = w.steps.len() as u32;
        for (i, s) in w.steps.iter().enumerate() {
            let n = i as u32 + 1;
            let label = self
                .find(&s.action)
                .map(|a| a.info().label)
                .unwrap_or_else(|| s.action.clone());
            if let Some(p) = progress {
                let _ = p.send((n, total, label.clone()));
            }
            if let Err(e) = Box::pin(self.invoke_with_progress(&s.action, &s.params, None)).await {
                return Err(ErrorInfo::new(
                    &e.code,
                    format!("step {n}/{total} ({label}) failed: {}", e.message),
                ));
            }
        }
        Ok(json!({ "workflow": w.id, "steps": total }))
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
        if let Some(n) = value.as_i64() {
            if p.min.is_some_and(|m| n < m) || p.max.is_some_and(|m| n > m) {
                return invalid(format!("parameter {name:?} is out of range"));
            }
        }
        if !p.choices.is_empty() {
            let v = value.as_str().unwrap_or_default();
            if !p.choices.iter().any(|c| c.value == v) {
                return Err(ErrorInfo::new(
                    error_code::NOT_PERMITTED,
                    format!("{v:?} is not an allowed {name} on this computer"),
                ));
            }
        }
    }
    for p in info.params.iter().filter(|p| p.required) {
        if !params.contains_key(&p.name) {
            return invalid(format!("missing parameter {:?}", p.name));
        }
    }
    Ok(())
}

/// Everything the built-in actions need from the desktop.
pub struct Desktop {
    /// The user's session bus (`None`: the actions are listed as unavailable).
    pub bus: Option<zbus::Connection>,
    /// Shared MPRIS tracker (also feeds the `media` live topic).
    pub mpris: Option<crate::mpris::Mpris>,
    /// Desktop IDs `app.launch` may start.
    pub apps: Vec<String>,
}

/// Registers the built-in desktop actions.
pub fn builtin(registry: &mut Registry, desktop: Desktop) {
    let changes = registry.changes();
    registry.register(notify::DesktopNotify::new(desktop.bus.clone()));
    for cmd in media::Command::ALL {
        registry.register(media::MediaAction::new(cmd, desktop.mpris.clone()));
    }
    let kde = kde::GlobalAccel::new(desktop.bus.clone(), changes.clone());
    for v in kde::VOLUME {
        registry.register(kde::VolumeAction::new(v, kde.clone()));
    }
    registry.register(kde::ShortcutAction::new(kde));
    registry.register(apps::AppLaunch::new(desktop.apps, changes));
    registry.register(session::SessionLock::new(desktop.bus));
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
    use crate::config::WorkflowStep;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Echo {
        id: &'static str,
        available: bool,
        privilege: Privilege,
        runs: Arc<AtomicU32>,
    }
    impl Echo {
        fn new(id: &'static str) -> Self {
            Echo {
                id,
                available: true,
                privilege: Privilege::Basic,
                runs: Arc::default(),
            }
        }
    }
    impl Action for Echo {
        fn info(&self) -> ActionInfo {
            let mut text = ParamInfo::new("text", ParamKind::String, self.id == "test.echo");
            if self.id == "test.pick" {
                text.choices = vec![Choice {
                    value: "a".into(),
                    label: "A".into(),
                }];
            }
            let mut n = ParamInfo::new("n", ParamKind::Integer, false);
            n.min = Some(0);
            n.max = Some(10);
            ActionInfo {
                available: self.available,
                reason: if self.available {
                    String::new()
                } else {
                    "off".into()
                },
                params: vec![text, n],
                privilege: self.privilege,
                ..ActionInfo::new(self.id, "Echo", "test")
            }
        }
        fn execute<'a>(&'a self, params: &'a Params) -> ActionFuture<'a> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                if params.get("text").and_then(Value::as_str) == Some("fail") {
                    return Err(ErrorInfo::new(error_code::FAILED, "asked to fail"));
                }
                Ok(Value::Object(params.clone()))
            })
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
        r.register(Echo::new("test.echo"));
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
        assert_eq!(
            code(
                r.invoke("test.echo", &p(json!({"text": "a", "n": 11})))
                    .await
            ),
            "invalid_params",
            "integer above max"
        );
        assert_eq!(
            code(
                r.invoke("test.echo", &p(json!({"text": "a", "n": -1})))
                    .await
            ),
            "invalid_params",
            "integer below min"
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
        r.register(Echo {
            available: false,
            ..Echo::new("test.echo")
        });
        assert!(!r.list()[0].available);
        assert_eq!(
            code(r.invoke("test.echo", &p(json!({"text": "a"}))).await),
            "unavailable"
        );
    }

    #[tokio::test]
    async fn elevated_actions_need_a_grant() {
        let mut r = Registry::new();
        let e = Echo {
            privilege: Privilege::Elevated,
            ..Echo::new("test.elevated")
        };
        let runs = e.runs.clone();
        r.register(e);
        let info = &r.list()[0];
        assert!(
            !info.available,
            "advertised as unavailable while not granted"
        );
        assert!(
            info.reason
                .contains("pocket-desktop-agent grant test.elevated"),
            "{}",
            info.reason
        );
        let e = r.invoke("test.elevated", &p(json!({}))).await.unwrap_err();
        assert_eq!(e.code, "not_permitted");
        assert_eq!(runs.load(Ordering::SeqCst), 0, "never executed");
        r.set_grants(["test.elevated".to_owned()]);
        assert!(r.list()[0].available);
        assert!(r.invoke("test.elevated", &p(json!({}))).await.is_ok());
        assert_eq!(runs.load(Ordering::SeqCst), 1);
        // A grant does not bypass "disabled".
        r.set_enabled(Some(Vec::<String>::new()));
        assert_eq!(
            code(r.invoke("test.elevated", &p(json!({}))).await),
            "not_permitted"
        );
    }

    #[tokio::test]
    async fn values_outside_the_closed_list_are_refused() {
        let mut r = Registry::new();
        let a = Echo::new("test.pick");
        let runs = a.runs.clone();
        r.register(a);
        assert!(
            r.invoke("test.pick", &p(json!({"text": "a"})))
                .await
                .is_ok()
        );
        assert_eq!(
            code(
                r.invoke("test.pick", &p(json!({"text": "/usr/bin/rm"})))
                    .await
            ),
            "not_permitted"
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    fn workflow(id: &str, steps: &[(&str, Value)]) -> WorkflowConfig {
        WorkflowConfig {
            id: id.into(),
            label: "Flow".into(),
            steps: steps
                .iter()
                .map(|(a, v)| WorkflowStep {
                    action: (*a).into(),
                    params: p(v.clone()),
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn workflows_run_steps_in_order_with_progress_and_the_same_checks() {
        let mut r = Registry::new();
        let echo = Echo::new("test.echo");
        let runs = echo.runs.clone();
        r.register(echo);
        r.register(Echo {
            privilege: Privilege::Elevated,
            ..Echo::new("test.elevated")
        });
        r.set_workflows(vec![
            workflow(
                "two",
                &[
                    ("test.echo", json!({"text": "1"})),
                    ("test.echo", json!({"text": "2"})),
                ],
            ),
            workflow(
                "fails",
                &[
                    ("test.echo", json!({"text": "fail"})),
                    ("test.echo", json!({"text": "x"})),
                ],
            ),
            workflow(
                "needs-grant",
                &[
                    ("test.echo", json!({"text": "1"})),
                    ("test.elevated", json!({})),
                ],
            ),
        ]);
        let wf = r.list().into_iter().find(|a| a.id == WORKFLOW_ID).unwrap();
        assert_eq!(wf.privilege, Privilege::Elevated);
        assert_eq!(wf.params[0].choices.len(), 3);
        assert_eq!(
            code(r.invoke(WORKFLOW_ID, &p(json!({"workflow": "two"}))).await),
            "not_permitted",
            "workflow.run itself is elevated"
        );
        r.set_grants([WORKFLOW_ID.to_owned()]);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let ok = r
            .invoke_with_progress(WORKFLOW_ID, &p(json!({"workflow": "two"})), Some(&tx))
            .await;
        assert_eq!(ok.unwrap(), json!({"workflow": "two", "steps": 2}));
        assert_eq!(rx.recv().await.unwrap(), (1, 2, "Echo".to_owned()));
        assert_eq!(rx.recv().await.unwrap(), (2, 2, "Echo".to_owned()));
        assert_eq!(runs.load(Ordering::SeqCst), 2);

        let e = r
            .invoke(WORKFLOW_ID, &p(json!({"workflow": "fails"})))
            .await
            .unwrap_err();
        assert!(e.message.starts_with("step 1/2"), "{}", e.message);
        assert_eq!(runs.load(Ordering::SeqCst), 3, "stops at the failing step");

        let e = r
            .invoke(WORKFLOW_ID, &p(json!({"workflow": "needs-grant"})))
            .await
            .unwrap_err();
        assert_eq!(e.code, "not_permitted");
        assert_eq!(
            runs.load(Ordering::SeqCst),
            3,
            "nothing ran: checked before the first step"
        );
        assert_eq!(
            code(
                r.invoke(WORKFLOW_ID, &p(json!({"workflow": "other"})))
                    .await
            ),
            "not_permitted"
        );
    }
}
