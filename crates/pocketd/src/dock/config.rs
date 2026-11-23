//! Phone-side Dock configuration (never in Git, never in QML):
//!
//! ```text
//! $POCKET_DOCK_DIR | $XDG_CONFIG_HOME/pocket-os/dock | ~/.config/pocket-os/dock   (0700)
//! ├── identity.key   this phone's Noise static key (0600)
//! └── host.json      the one trusted computer (0600):
//!                    { "name": "fedora", "address": "172.16.42.2:47130", "key": "<64 hex>" }
//! ```
//!
//! Managed with `pocketd dock init | show | set-host NAME ADDRESS KEY | forget-host`
//! (`scripts/dock-pair.sh` runs the whole pairing from the development host over SSH).
//! Only static configuration lives here; connection state is never persisted.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use pocket_dock_protocol::keys::{self, Identity, PublicKey};

pub fn dir() -> PathBuf {
    if let Some(d) = std::env::var_os("POCKET_DOCK_DIR") {
        return d.into();
    }
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pocket-os/dock")
}

/// The trusted computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host {
    pub name: String,
    pub address: SocketAddr,
    pub key: PublicKey,
}

/// Everything needed to connect.
#[derive(Debug, Clone)]
pub struct LinkConfig {
    pub identity: Identity,
    pub host: Host,
}

fn identity_path(dir: &Path) -> PathBuf {
    dir.join("identity.key")
}

fn host_path(dir: &Path) -> PathBuf {
    dir.join("host.json")
}

pub fn load_host(dir: &Path) -> Result<Host, String> {
    let path = host_path(dir);
    if !path.exists() {
        return Err(format!(
            "no trusted computer configured ({} missing; pair with scripts/dock-pair.sh)",
            path.display()
        ));
    }
    keys::check_private_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let field = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .ok_or_else(|| format!("{}: missing {k:?}", path.display()))
    };
    Ok(Host {
        name: field("name")?.to_owned(),
        address: field("address")?
            .parse()
            .map_err(|e| format!("{}: address: {e}", path.display()))?,
        key: field("key")?
            .parse()
            .map_err(|e| format!("{}: key: {e}", path.display()))?,
    })
}

pub fn load(dir: &Path) -> Result<LinkConfig, String> {
    let host = load_host(dir)?;
    let path = identity_path(dir);
    let identity = Identity::load(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(LinkConfig { identity, host })
}

/// `pocketd dock …` (prints to stdout; returns the exit status).
pub fn cli(args: &[String]) -> u8 {
    let dir = dir();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let r = match args.as_slice() {
        ["init"] => init(&dir),
        ["show"] => show(&dir),
        ["set-host", name, address, key] => set_host(&dir, name, address, key),
        ["forget-host"] => std::fs::remove_file(host_path(&dir)).map_err(|e| e.to_string()),
        _ => Err(
            "usage: pocketd dock init | show | set-host NAME ADDRESS:PORT PUBLIC_KEY | forget-host"
                .into(),
        ),
    };
    match r {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("pocketd dock: {e}");
            1
        }
    }
}

fn init(dir: &Path) -> Result<(), String> {
    let path = identity_path(dir);
    if path.exists() {
        println!("identity exists: {}", path.display());
    } else {
        Identity::generate()
            .and_then(|i| i.save_new(&path))
            .map_err(|e| e.to_string())?;
        println!("created identity: {}", path.display());
    }
    show(dir)
}

fn show(dir: &Path) -> Result<(), String> {
    let path = identity_path(dir);
    let id = Identity::load(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("public_key={}", id.public());
    match load_host(dir) {
        Ok(h) => println!("host={} {} {}", h.name, h.address, h.key),
        Err(e) => println!("host=none ({e})"),
    }
    Ok(())
}

fn set_host(dir: &Path, name: &str, address: &str, key: &str) -> Result<(), String> {
    let address: SocketAddr = address.parse().map_err(|e| format!("address: {e}"))?;
    let key: PublicKey = key.parse().map_err(|e| format!("key: {e}"))?;
    if name.is_empty() || name.len() > 64 {
        return Err("name must be 1–64 characters".into());
    }
    let text = serde_json::to_string_pretty(&serde_json::json!({
        "name": name, "address": address.to_string(), "key": key.to_string()
    }))
    .map_err(|e| e.to_string())?;
    keys::write_private_file(&host_path(dir), format!("{text}\n").as_bytes(), true)
        .map_err(|e| e.to_string())?;
    println!("trusted computer: {name} at {address} ({})", key.short_id());
    println!("takes effect on the next Connect (Dock Mode → Connect)");
    Ok(())
}
