//! Agent configuration directory (never in Git):
//!
//! ```text
//! $POCKET_AGENT_DIR | $XDG_CONFIG_HOME/pocket-desktop-agent | ~/.config/pocket-desktop-agent
//! ├── identity.key   this computer's Noise static key (0600, see pocket_dock_protocol::keys)
//! └── config.json    listen address, host name, trusted phones, optional action allow-list (0600)
//! ```
//!
//! `config.json`:
//! ```json
//! { "listen": "172.16.42.2:47130",
//!   "host_name": "fedora",
//!   "trusted": [ { "name": "lavender", "key": "<64 hex>" } ],
//!   "enabled_actions": null,
//!   "grants": ["app.launch", "session.lock"],
//!   "apps": ["org.kde.konsole", "org.kde.dolphin"],
//!   "workflows": [ { "id": "start-work", "label": "Start work", "steps": [
//!       { "action": "app.launch", "params": { "app": "org.kde.konsole" } },
//!       { "action": "desktop.shortcut", "params": { "shortcut": "desktop_next" } } ] } ],
//!   "notifications": { "forward": false, "allow_apps": null, "deny_apps": [],
//!                      "redact_when_locked": true } }
//! ```
//! `listen` must be a specific address (the USB link), never `0.0.0.0`/`::`.
//! `enabled_actions: null` enables every built-in action; a list restricts them.
//! Stage 4 (ADR-0008): `grants` lists the *elevated* actions the phone may run (none by
//! default); `apps` the desktop IDs `app.launch` may start; `workflows` named sequences
//! of registered actions; `notifications` the forwarding policy (off by default).

use std::fmt;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use pocket_dock_protocol::keys::{self, Identity, KeyError, PublicKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub name: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub listen: SocketAddr,
    #[serde(default)]
    pub host_name: String,
    #[serde(default)]
    pub trusted: Vec<TrustedPeer>,
    #[serde(default)]
    pub enabled_actions: Option<Vec<String>>,
    /// Elevated actions granted to trusted phones (ADR-0008).
    #[serde(default)]
    pub grants: Vec<String>,
    /// Desktop-file IDs `app.launch` may start.
    #[serde(default)]
    pub apps: Vec<String>,
    #[serde(default)]
    pub workflows: Vec<WorkflowConfig>,
    #[serde(default)]
    pub notifications: NotificationPolicy,
}

/// A named sequence of registered actions (`workflow.run`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowConfig {
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub action: String,
    #[serde(default)]
    pub params: serde_json::Map<String, serde_json::Value>,
}

/// Which desktop notifications are forwarded to the phone (ADR-0008 §5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationPolicy {
    /// Off by default: with it on, any program in the phone's session can read them (#8).
    #[serde(default)]
    pub forward: bool,
    /// Only these applications (by app name), if set.
    #[serde(default)]
    pub allow_apps: Option<Vec<String>>,
    #[serde(default)]
    pub deny_apps: Vec<String>,
    /// Withhold the body while the screen is locked.
    #[serde(default = "yes")]
    pub redact_when_locked: bool,
}

fn yes() -> bool {
    true
}

impl Default for NotificationPolicy {
    fn default() -> Self {
        NotificationPolicy {
            forward: false,
            allow_apps: None,
            deny_apps: Vec::new(),
            redact_when_locked: true,
        }
    }
}

/// Desktop-file IDs: `[A-Za-z0-9._-]`, 1–255 characters, no path separators.
pub fn is_valid_desktop_id(id: &str) -> bool {
    (1..=255).contains(&id.len())
        && !id.starts_with('.')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

#[derive(Debug)]
pub enum ConfigError {
    Key(KeyError),
    Io(std::io::Error),
    Parse(String),
    Invalid(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Key(e) => write!(f, "identity: {e}"),
            ConfigError::Io(e) => write!(f, "{e}"),
            ConfigError::Parse(e) => write!(f, "config.json: {e}"),
            ConfigError::Invalid(e) => write!(f, "config.json: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<KeyError> for ConfigError {
    fn from(e: KeyError) -> Self {
        ConfigError::Key(e)
    }
}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}

pub fn default_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("POCKET_AGENT_DIR") {
        return d.into();
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("pocket-desktop-agent")
}

pub struct Paths {
    pub identity: PathBuf,
    pub config: PathBuf,
}

impl Paths {
    pub fn in_dir(dir: &Path) -> Self {
        Paths {
            identity: dir.join("identity.key"),
            config: dir.join("config.json"),
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        keys::check_private_file(path)?;
        let text = std::fs::read_to_string(path)?;
        let c: Config =
            serde_json::from_str(&text).map_err(|e| ConfigError::Parse(e.to_string()))?;
        c.validate()?;
        Ok(c)
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        self.validate()?;
        let mut text =
            serde_json::to_string_pretty(self).map_err(|e| ConfigError::Parse(e.to_string()))?;
        text.push('\n');
        keys::write_private_file(path, text.as_bytes(), true)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.listen.ip().is_unspecified() {
            return Err(ConfigError::Invalid(format!(
                "listen address {} would accept connections on every interface; use the USB link address (e.g. 172.16.42.2:{})",
                self.listen,
                pocket_dock_protocol::DEFAULT_PORT
            )));
        }
        for t in &self.trusted {
            t.key.parse::<PublicKey>().map_err(|_| {
                ConfigError::Invalid(format!("trusted peer {:?}: malformed key", t.name))
            })?;
        }
        if let Some(ids) = &self.enabled_actions {
            if let Some(bad) = ids
                .iter()
                .find(|i| !pocket_dock_protocol::is_valid_action_id(i))
            {
                return Err(ConfigError::Invalid(format!("invalid action id {bad:?}")));
            }
        }
        if let Some(bad) = self
            .grants
            .iter()
            .find(|i| !pocket_dock_protocol::is_valid_action_id(i))
        {
            return Err(ConfigError::Invalid(format!(
                "grants: invalid action id {bad:?}"
            )));
        }
        if let Some(bad) = self.apps.iter().find(|a| !is_valid_desktop_id(a)) {
            return Err(ConfigError::Invalid(format!(
                "apps: invalid desktop ID {bad:?}"
            )));
        }
        let mut ids = std::collections::HashSet::new();
        for w in &self.workflows {
            let at = format!("workflow {:?}", w.id);
            if !pocket_dock_protocol::is_valid_action_id(&w.id) || !ids.insert(w.id.clone()) {
                return Err(ConfigError::Invalid(format!(
                    "{at}: invalid or duplicate id"
                )));
            }
            if w.steps.is_empty() || w.steps.len() > 10 {
                return Err(ConfigError::Invalid(format!("{at}: needs 1–10 steps")));
            }
            for s in &w.steps {
                if !pocket_dock_protocol::is_valid_action_id(&s.action)
                    || s.action == "workflow.run"
                {
                    return Err(ConfigError::Invalid(format!(
                        "{at}: invalid step action {:?} (workflows cannot nest)",
                        s.action
                    )));
                }
                if s.params
                    .values()
                    .any(|v| v.is_object() || v.is_array() || v.is_null() || v.is_f64())
                {
                    return Err(ConfigError::Invalid(format!(
                        "{at}: step parameters must be strings, booleans or integers"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Trusted peers as (name, key) pairs (validated by `load`).
    pub fn trusted_keys(&self) -> Vec<(String, PublicKey)> {
        self.trusted
            .iter()
            .filter_map(|t| t.key.parse().ok().map(|k| (t.name.clone(), k)))
            .collect()
    }

    /// This computer's name for the phone: the configured one, else the hostname.
    pub fn display_name(&self) -> String {
        if !self.host_name.is_empty() {
            return self.host_name.clone();
        }
        std::fs::read_to_string("/etc/hostname")
            .ok()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "computer".into())
    }
}

pub fn load_identity(paths: &Paths) -> Result<Identity, ConfigError> {
    Ok(Identity::load(&paths.identity)?)
}
