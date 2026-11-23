//! JSON messages, one per encrypted frame. Both directions are internally tagged by
//! `"type"`. Unknown fields are ignored and unknown types decode to `Unknown`, so a
//! minor version can add either without breaking older peers.
//!
//! Client (phone) → agent:
//! - `hello {protocol, client, device_name}`: first message, exactly once.
//! - `invoke {id, action, params}`: run a registered action; answered by one `result`.
//! - `ping {id}`: liveness; answered by `pong`.
//!
//! Agent → client:
//! - `welcome {protocol, agent, host_name, actions}`: answer to `hello`.
//! - `result {id, ok, data, error}`: outcome of one `invoke`, sent after the action ran.
//! - `actions {actions}`: the action list changed (availability, e.g. a media player
//!   appeared). Always the full list.
//! - `pong {id}`.
//! - `error {code, message}`: fatal; the agent closes the connection after it.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub major: u16,
    pub minor: u16,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello {
        protocol: Version,
        /// Client software, e.g. `pocketd 0.1.0`.
        client: String,
        /// Human-readable phone name (hostname).
        #[serde(default)]
        device_name: String,
    },
    Invoke {
        id: u64,
        action: String,
        #[serde(default)]
        params: Map<String, Value>,
        /// Idempotency key (1.1): the same key means the same user intent; the agent
        /// returns the stored result instead of running it again.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key: Option<String>,
    },
    Ping {
        id: u64,
    },
    /// Live state topics the phone wants (1.1). Answered by one `state` per topic.
    Subscribe {
        topics: Vec<String>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    Welcome {
        protocol: Version,
        /// Agent software, e.g. `pocket-desktop-agent 0.1.0`.
        agent: String,
        /// Human-readable computer name.
        host_name: String,
        actions: Vec<ActionInfo>,
    },
    Result {
        id: u64,
        ok: bool,
        /// Action-specific result data (`null` when there is none).
        #[serde(default)]
        data: Value,
        /// Present when `ok` is false.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<ErrorInfo>,
        /// 1.1: not executed again; this is the stored result for the request key.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        duplicate: bool,
    },
    /// 1.1: a step of a running workflow (`id` of its invoke).
    Progress {
        id: u64,
        step: u32,
        total: u32,
        label: String,
    },
    /// 1.1: the full current value of one live topic (the first one is the snapshot).
    State {
        topic: String,
        /// Strictly increasing per topic within one agent process.
        rev: u64,
        available: bool,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        reason: String,
        /// > 0: sampled every `interval_ms`; stale after 3 × without an update.
        #[serde(default, skip_serializing_if = "is_zero")]
        interval_ms: u64,
        #[serde(default)]
        data: Value,
    },
    /// 1.1: a desktop notification (forwarded by the agent's policy).
    Notification(Notification),
    /// 1.1: the notification was closed on the computer.
    NotificationClosed {
        nid: String,
    },
    Actions {
        actions: Vec<ActionInfo>,
    },
    Pong {
        id: u64,
    },
    Error {
        code: String,
        message: String,
    },
    #[serde(other)]
    Unknown,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

fn is_basic(p: &Privilege) -> bool {
    *p == Privilege::Basic
}

/// A forwarded desktop notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    /// Agent-chosen id (stable for `notification_closed`).
    pub nid: String,
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub body: String,
    /// 0 low, 1 normal, 2 critical.
    #[serde(default)]
    pub urgency: u8,
    /// The body was withheld (e.g. the computer is locked).
    #[serde(default)]
    pub redacted: bool,
    /// Wall-clock time on the computer (ms since the epoch).
    #[serde(default)]
    pub time_ms: u64,
}

/// Authorization class of an action (1.1, ADR-0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Privilege {
    /// Harmless if any program on the phone triggers it; enabled by default.
    #[default]
    Basic,
    /// Changes visible desktop state; runs only when granted on the computer.
    Elevated,
}

impl Privilege {
    pub fn as_str(self) -> &'static str {
        match self {
            Privilege::Basic => "basic",
            Privilege::Elevated => "elevated",
        }
    }
}

/// One allowed value of a string parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    pub value: String,
    #[serde(default)]
    pub label: String,
}

/// One action advertised by the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionInfo {
    /// Stable id ([`crate::is_valid_action_id`]).
    pub id: String,
    /// Short label for a button.
    pub label: String,
    /// Grouping for the UI, e.g. `desktop`, `media`.
    #[serde(default)]
    pub category: String,
    /// Whether the action can run now.
    pub available: bool,
    /// Why it is unavailable (empty when available).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    /// Accepted parameters (all optional ones have defaults on the agent).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<ParamInfo>,
    /// 1.1: authorization class.
    #[serde(default, skip_serializing_if = "is_basic")]
    pub privilege: Privilege,
    /// 1.1: running it twice has the same effect as once.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub idempotent: bool,
    /// 1.1: the UI should ask before sending it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirm: bool,
}

impl ActionInfo {
    /// A basic, available action without parameters (fill in the rest with struct update).
    pub fn new(id: &str, label: &str, category: &str) -> Self {
        ActionInfo {
            id: id.into(),
            label: label.into(),
            category: category.into(),
            available: true,
            reason: String::new(),
            params: Vec::new(),
            privilege: Privilege::Basic,
            idempotent: false,
            confirm: false,
        }
    }

    /// Marks the action unavailable for `reason` (empty reason = available).
    pub fn unavailable_if(mut self, reason: Option<String>) -> Self {
        if let Some(r) = reason {
            self.available = false;
            self.reason = r;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamInfo {
    pub name: String,
    pub kind: ParamKind,
    #[serde(default)]
    pub required: bool,
    /// 1.1 (string params): the only accepted values.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<Choice>,
    /// 1.1 (integer params): inclusive bounds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<i64>,
}

impl ParamInfo {
    pub fn new(name: &str, kind: ParamKind, required: bool) -> Self {
        ParamInfo {
            name: name.into(),
            kind,
            required,
            choices: Vec::new(),
            min: None,
            max: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    String,
    Bool,
    Integer,
}

/// Structured error of a failed `invoke` (codes: [`crate::error_code`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
}

impl ErrorInfo {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn wire_format_is_stable() {
        let m = ClientMessage::Invoke {
            id: 7,
            action: "desktop.notify".into(),
            params: json!({"title": "x"}).as_object().unwrap().clone(),
            key: None,
        };
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({"type": "invoke", "id": 7, "action": "desktop.notify", "params": {"title": "x"}})
        );
        let r = AgentMessage::Result {
            id: 7,
            ok: false,
            data: Value::Null,
            error: Some(ErrorInfo::new(crate::error_code::UNKNOWN_ACTION, "nope")),
            duplicate: false,
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"type": "result", "id": 7, "ok": false, "data": null,
                   "error": {"code": "unknown_action", "message": "nope"}})
        );
    }

    #[test]
    fn version_1_1_fields_are_optional_and_compatible() {
        // A 1.0 action (no privilege/idempotent/confirm) decodes with safe defaults.
        let a: ActionInfo =
            serde_json::from_str(r#"{"id":"a.b","label":"x","available":true}"#).unwrap();
        assert_eq!(a.privilege, Privilege::Basic);
        assert!(!a.idempotent && !a.confirm);
        // A basic action serializes exactly as in 1.0.
        assert_eq!(
            serde_json::to_value(ActionInfo::new("a.b", "x", "c")).unwrap(),
            json!({"id": "a.b", "label": "x", "category": "c", "available": true})
        );
        let e = ActionInfo {
            privilege: Privilege::Elevated,
            confirm: true,
            ..ActionInfo::new("s.lock", "Lock", "session")
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(
            (v["privilege"].as_str(), v["confirm"].as_bool()),
            (Some("elevated"), Some(true))
        );
        // invoke key and duplicate results round-trip; a 1.0 result has no duplicate flag.
        let m: ClientMessage =
            serde_json::from_str(r#"{"type":"invoke","id":1,"action":"a","key":"k-1"}"#).unwrap();
        assert!(matches!(m, ClientMessage::Invoke { key: Some(k), .. } if k == "k-1"));
        let r: AgentMessage =
            serde_json::from_str(r#"{"type":"result","id":1,"ok":true}"#).unwrap();
        assert!(matches!(
            r,
            AgentMessage::Result {
                duplicate: false,
                ..
            }
        ));
        let n = AgentMessage::Notification(Notification {
            nid: "n1".into(),
            app: "Mail".into(),
            summary: "Hi".into(),
            body: String::new(),
            urgency: 1,
            redacted: true,
            time_ms: 5,
        });
        let v = serde_json::to_value(&n).unwrap();
        assert_eq!(v["type"], "notification");
        assert_eq!(serde_json::from_value::<AgentMessage>(v).unwrap(), n);
        let s: AgentMessage = serde_json::from_str(
            r#"{"type":"state","topic":"media","rev":3,"available":true,"data":{"x":1}}"#,
        )
        .unwrap();
        assert!(matches!(
            s,
            AgentMessage::State {
                interval_ms: 0,
                rev: 3,
                ..
            }
        ));
        assert!(crate::is_valid_request_key("a1_B-2"));
        assert!(!crate::is_valid_request_key("") && !crate::is_valid_request_key("a b"));
    }

    #[test]
    fn unknown_types_and_fields_are_tolerated() {
        let m: AgentMessage =
            serde_json::from_str(r#"{"type":"media_state","playing":true}"#).unwrap();
        assert_eq!(m, AgentMessage::Unknown);
        let m: ClientMessage =
            serde_json::from_str(r#"{"type":"ping","id":3,"future":1}"#).unwrap();
        assert_eq!(m, ClientMessage::Ping { id: 3 });
        let m: ClientMessage =
            serde_json::from_str(r#"{"type":"invoke","id":1,"action":"a"}"#).unwrap();
        assert!(matches!(m, ClientMessage::Invoke { params, .. } if params.is_empty()));
    }

    #[test]
    fn malformed_messages_are_errors() {
        for bad in [r#"{"id":1}"#, r#"{"type":"invoke"}"#, "[]", "not json"] {
            assert!(serde_json::from_str::<ClientMessage>(bad).is_err(), "{bad}");
        }
    }
}
