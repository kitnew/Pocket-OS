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
//!   "enabled_actions": null }
//! ```
//! `listen` must be a specific address (the USB link), never `0.0.0.0`/`::`.
//! `enabled_actions: null` enables every built-in action; a list restricts them.

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
