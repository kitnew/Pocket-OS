//! `pocket-desktop-agent`: the Pocket Dock agent for the computer (runs as the desktop
//! user, never as root).
//!
//! ```text
//! pocket-desktop-agent init [--listen ADDR:PORT] [--name NAME]   create identity + config
//! pocket-desktop-agent show                                     public key, listen address, trusted phones
//! pocket-desktop-agent trust NAME PUBLIC_KEY                     trust a phone (pocketd dock show)
//! pocket-desktop-agent untrust NAME
//! pocket-desktop-agent grant ACTION | revoke ACTION              allow / forbid an elevated action
//! pocket-desktop-agent apps add|remove DESKTOP_ID                applications app.launch may start
//! pocket-desktop-agent notifications on|off                      forward desktop notifications
//! pocket-desktop-agent run                                       serve (foreground; systemd user unit)
//! ```
//! Configuration changes apply after `systemctl --user restart pocket-desktop-agent`.
//! ```text
//! ```
//! Config directory: `$POCKET_AGENT_DIR`, else `$XDG_CONFIG_HOME/pocket-desktop-agent`.
//! Logging: `POCKET_AGENT_LOG` (tracing env-filter, default `info`) to stderr.

use std::net::SocketAddr;
use std::os::unix::fs::MetadataExt;
use std::process::ExitCode;
use std::sync::Arc;

use pocket_desktop_agent::actions::{self, Desktop, Registry};
use pocket_desktop_agent::config::{self, Config, Paths, TrustedPeer};
use pocket_desktop_agent::live::Live;
use pocket_desktop_agent::mpris::Mpris;
use pocket_desktop_agent::notifications;
use pocket_desktop_agent::server::Server;
use pocket_dock_protocol::keys::{Identity, PublicKey};
use tokio::signal::unix::{SignalKind, signal};
use tracing::{error, info, warn};

const USAGE: &str = "usage: pocket-desktop-agent init [--listen ADDR:PORT] [--name NAME] | show | trust NAME PUBLIC_KEY | untrust NAME | grant ACTION | revoke ACTION | apps add|remove DESKTOP_ID | notifications on|off | run";

/// Elevated actions that `grant` accepts (ADR-0008).
const ELEVATED: [&str; 4] = [
    "app.launch",
    "desktop.shortcut",
    "session.lock",
    "workflow.run",
];

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("POCKET_AGENT_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let dir = config::default_dir();
    let paths = Paths::in_dir(&dir);
    let result = match args.as_slice() {
        ["init", rest @ ..] => init(&paths, rest),
        ["show"] => show(&paths),
        ["trust", name, key] => trust(&paths, name, key),
        ["untrust", name] => untrust(&paths, name),
        ["grant", id] => edit(&paths, |c| {
            if !ELEVATED.contains(id) {
                return Err(
                    format!("{id:?} is not an elevated action ({})", ELEVATED.join(", ")).into(),
                );
            }
            if !c.grants.iter().any(|g| g == id) {
                c.grants.push((*id).to_owned());
            }
            Ok(format!("granted {id}"))
        }),
        ["revoke", id] => edit(&paths, |c| {
            c.grants.retain(|g| g != id);
            Ok(format!("revoked {id}"))
        }),
        ["apps", "add", id] => edit(&paths, |c| {
            if !config::is_valid_desktop_id(id) {
                return Err(format!("{id:?} is not a desktop-file ID").into());
            }
            if !c.apps.iter().any(|a| a == id) {
                c.apps.push((*id).to_owned());
            }
            Ok(format!("app.launch may start {id}"))
        }),
        ["apps", "remove", id] => edit(&paths, |c| {
            c.apps.retain(|a| a != id);
            Ok(format!("removed {id}"))
        }),
        ["notifications", on @ ("on" | "off")] => edit(&paths, |c| {
            c.notifications.forward = *on == "on";
            Ok(format!("notification forwarding {on}"))
        }),
        ["run"] => run(&paths),
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pocket-desktop-agent: {e}");
            ExitCode::FAILURE
        }
    }
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn init(paths: &Paths, args: &[&str]) -> Result<()> {
    let mut listen: SocketAddr =
        format!("172.16.42.2:{}", pocket_dock_protocol::DEFAULT_PORT).parse()?;
    let mut name = String::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--listen" => listen = it.next().ok_or(USAGE)?.parse()?,
            "--name" => name = (*it.next().ok_or(USAGE)?).to_owned(),
            _ => return Err(USAGE.into()),
        }
    }
    if paths.identity.exists() {
        println!("identity exists: {}", paths.identity.display());
    } else {
        Identity::generate()?.save_new(&paths.identity)?;
        println!("created identity: {}", paths.identity.display());
    }
    let mut cfg = if paths.config.exists() {
        Config::load(&paths.config)?
    } else {
        Config {
            listen,
            host_name: String::new(),
            trusted: Vec::new(),
            enabled_actions: None,
            grants: Vec::new(),
            apps: Vec::new(),
            workflows: Vec::new(),
            notifications: Default::default(),
        }
    };
    if args.contains(&"--listen") {
        cfg.listen = listen;
    }
    if !name.is_empty() {
        cfg.host_name = name;
    }
    cfg.save(&paths.config)?;
    println!("config: {}", paths.config.display());
    show(paths)
}

fn show(paths: &Paths) -> Result<()> {
    let id = config::load_identity(paths)?;
    let cfg = Config::load(&paths.config)?;
    println!("public_key={}", id.public());
    println!("listen={}", cfg.listen);
    println!("host_name={}", cfg.display_name());
    for t in &cfg.trusted {
        println!("trusted={} {}", t.name, t.key);
    }
    println!("grants={}", cfg.grants.join(","));
    println!("apps={}", cfg.apps.join(","));
    println!(
        "workflows={}",
        cfg.workflows
            .iter()
            .map(|w| w.id.as_str())
            .collect::<Vec<_>>()
            .join(",")
    );
    println!("notifications.forward={}", cfg.notifications.forward);
    if !cfg.grants.is_empty() || cfg.notifications.forward {
        println!(
            "note: any program in the phone's Pocket session can use granted actions and read forwarded notifications (issue #8)"
        );
    }
    Ok(())
}

/// Loads config.json, applies `f`, validates and saves it.
fn edit(paths: &Paths, f: impl FnOnce(&mut Config) -> Result<String>) -> Result<()> {
    let mut cfg = Config::load(&paths.config)?;
    let msg = f(&mut cfg)?;
    cfg.save(&paths.config)?;
    println!("{msg}; restart the agent to apply (systemctl --user restart pocket-desktop-agent)");
    Ok(())
}

fn trust(paths: &Paths, name: &str, key: &str) -> Result<()> {
    let key: PublicKey = key.parse()?;
    let mut cfg = Config::load(&paths.config)?;
    cfg.trusted
        .retain(|t| t.name != name && t.key != key.to_string());
    cfg.trusted.push(TrustedPeer {
        name: name.to_owned(),
        key: key.to_string(),
    });
    cfg.save(&paths.config)?;
    println!(
        "trusted {name} ({}); restart the agent to apply",
        key.short_id()
    );
    Ok(())
}

fn untrust(paths: &Paths, name: &str) -> Result<()> {
    let mut cfg = Config::load(&paths.config)?;
    let before = cfg.trusted.len();
    cfg.trusted.retain(|t| t.name != name);
    if cfg.trusted.len() == before {
        return Err(format!("no trusted phone named {name:?}").into());
    }
    cfg.save(&paths.config)?;
    println!("removed {name}; restart the agent to apply");
    Ok(())
}

/// Binds the listener to one address. IP_FREEBIND lets the agent start before the USB
/// link (and its address) exists; connections arrive once the phone is plugged in.
fn bind(addr: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    use socket2::{Domain, Socket, Type};
    let s = Socket::new(Domain::for_address(addr), Type::STREAM, None)?;
    s.set_reuse_address(true)?;
    if addr.is_ipv4() {
        s.set_freebind_v4(true)?;
    } else {
        s.set_freebind_v6(true)?;
    }
    s.set_nonblocking(true)?;
    s.bind(&addr.into())?;
    s.listen(8)?;
    tokio::net::TcpListener::from_std(s.into())
}

fn run(paths: &Paths) -> Result<()> {
    if std::fs::metadata("/proc/self")?.uid() == 0 {
        return Err("refusing to run as root; run it as the desktop user".into());
    }
    let identity = config::load_identity(paths)?;
    let cfg = Config::load(&paths.config)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let bus = match zbus::Connection::session().await {
            Ok(c) => Some(c),
            Err(e) => {
                warn!(error = %e, "no desktop session bus: desktop actions are unavailable");
                None
            }
        };
        let mut registry = Registry::new();
        let mpris = bus.clone().map(|b| Mpris::start(b, Some(registry.changes())));
        actions::builtin(
            &mut registry,
            Desktop { bus: bus.clone(), mpris: mpris.clone(), apps: cfg.apps.clone() },
        );
        registry.set_enabled(cfg.enabled_actions.clone());
        registry.set_grants(cfg.grants.clone());
        registry.set_workflows(cfg.workflows.clone());
        let live = Live::new();
        pocket_desktop_agent::live::start(&live, bus.clone(), mpris);
        let trusted = cfg.trusted_keys();
        if trusted.is_empty() {
            warn!("no trusted phone configured: every connection will be rejected (pocket-desktop-agent trust NAME KEY)");
        }
        let mut server = Server::new(
            identity.clone(),
            trusted.clone(),
            cfg.display_name(),
            Arc::new(registry),
        );
        server.live = live.clone();
        notifications::start(
            notifications::Policy(cfg.notifications.clone()),
            live,
            server.notifications.clone(),
        );
        let server = Arc::new(server);
        let listener = bind(cfg.listen)?;
        info!(
            version = env!("CARGO_PKG_VERSION"),
            listen = %cfg.listen,
            key = %identity.public().short_id(),
            trusted = ?trusted.iter().map(|(n, k)| format!("{n}:{}", k.short_id())).collect::<Vec<_>>(),
            host_name = %cfg.display_name(),
            grants = ?cfg.grants,
            notifications = cfg.notifications.forward,
            "pocket-desktop-agent listening"
        );
        let mut term = signal(SignalKind::terminate())?;
        let mut int = signal(SignalKind::interrupt())?;
        tokio::select! {
            r = server.serve(listener) => {
                if let Err(e) = r {
                    error!(error = %e, "listener failed");
                    return Err(e.into());
                }
            }
            _ = term.recv() => info!("SIGTERM: stopping"),
            _ = int.recv() => info!("SIGINT: stopping"),
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    })
}
