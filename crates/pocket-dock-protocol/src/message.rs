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
    },
    Ping {
        id: u64,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamInfo {
    pub name: String,
    pub kind: ParamKind,
    #[serde(default)]
    pub required: bool,
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
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"type": "result", "id": 7, "ok": false, "data": null,
                   "error": {"code": "unknown_action", "message": "nope"}})
        );
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
