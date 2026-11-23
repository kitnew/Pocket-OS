//! `pocketd`: the Pocket Core daemon.
//!
//! Lifecycle: connect to the session bus → export `/org/pocketos/Core1`, the
//! application catalog `/org/pocketos/Core1/Apps` and the Dock session
//! `/org/pocketos/Core1/Dock` → own
//! `org.pocketos.Core1` (fail fast if another core owns it) → mark ready (emits
//! `StateChanged`) → serve until SIGTERM/SIGINT, loss of the name, or bus disconnect →
//! release the name and exit. State is in memory; clients resynchronise on the bus-name
//! owner change. Within a graphical session the user's mode and Dock link intent survive
//! a restart (`recovery`, issue #5): the new core starts in the recovered mode and
//! reconnects the Dock link if it was wanted.
//!
//! Logging: `POCKETD_LOG` (tracing env-filter syntax, default `info`) to stderr.

mod apps;
mod dock;
mod recovery;
mod service;

use std::process::ExitCode;
use std::time::Duration;

use apps::AppsService;
use futures_util::StreamExt;
use pocket_ipc::dbus::{BUS_NAME, OBJECT_PATH};
use service::CoreService;
use tokio::signal::unix::{SignalKind, signal};
use tracing::{error, info, warn};
use zbus::{connection, fdo::RequestNameFlags, fdo::RequestNameReply};

/// Exit status when another process already owns the bus name.
const EXIT_NAME_TAKEN: u8 = 3;

fn main() -> ExitCode {
    // `pocketd dock …`: trusted-host setup (docs/development/dock.md); no daemon.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("dock") {
        return ExitCode::from(dock::config::cli(&args[1..]));
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("POCKETD_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            error!(error = %e, "cannot create runtime");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run())
}

enum Stop {
    Signal(&'static str),
    NameLost,
    BusClosed,
}

async fn run() -> ExitCode {
    info!(
        version = pocket_core::VERSION,
        protocol = %pocket_core::PROTOCOL_VERSION,
        pid = std::process::id(),
        "pocketd starting"
    );

    let (Ok(mut sigterm), Ok(mut sigint), Ok(mut sighup)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
        signal(SignalKind::hangup()),
    ) else {
        error!("cannot install signal handlers");
        return ExitCode::FAILURE;
    };

    let (recovery, recovered) = recovery::Recovery::open(recovery::session_path());
    let apps = AppsService::new();
    let app_dirs = apps.dirs();
    let (dock, dock_control, dock_state, dock_cmds) =
        dock::DockService::new(dock::config::dir(), recovered.dock_wanted);
    let conn = match connection::Builder::session()
        .serve_at(
            OBJECT_PATH,
            CoreService::new(recovered.mode, recovery.clone(), dock_control.clone()),
        )
        .serve_at(pocket_ipc::dbus::apps::OBJECT_PATH, apps)
        .serve_at(pocket_ipc::dbus::dock::OBJECT_PATH, dock)
        .build()
        .await
    {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "cannot connect to the session bus");
            return ExitCode::FAILURE;
        }
    };

    let mut name_lost = match zbus::fdo::DBusProxy::new(&conn).await {
        Ok(p) => match p.receive_name_lost().await {
            Ok(s) => s,
            Err(e) => {
                error!(error = %e, "cannot subscribe to NameLost");
                return ExitCode::FAILURE;
            }
        },
        Err(e) => {
            error!(error = %e, "cannot create bus proxy");
            return ExitCode::FAILURE;
        }
    };

    // DoNotQueue without AllowReplacement: exactly one core per session.
    match conn
        .request_name_with_flags(BUS_NAME, RequestNameFlags::DO_NOT_QUEUE)
        .await
    {
        Ok(RequestNameReply::PrimaryOwner) => {}
        Ok(reply) => {
            error!(
                name = BUS_NAME,
                ?reply,
                "bus name is owned by another process (pocketd already running?)"
            );
            return ExitCode::from(EXIT_NAME_TAKEN);
        }
        Err(e) => {
            error!(name = BUS_NAME, error = %e, "cannot own bus name");
            return ExitCode::from(EXIT_NAME_TAKEN);
        }
    }

    if let Err(e) = mark_ready(&conn).await {
        error!(error = %e, "cannot publish ready state");
        return ExitCode::FAILURE;
    }
    let watcher = tokio::spawn(apps::watch(conn.clone(), app_dirs));
    let dock_link = tokio::spawn(
        dock::link::Link::new(
            conn.clone(),
            dock_state,
            dock_cmds,
            recovery,
            dock::netwatch::spawn(),
        )
        .run(),
    );
    info!(name = BUS_NAME, path = OBJECT_PATH, unique = ?conn.unique_name().map(|n| n.as_str()), mode = %recovered.mode, "pocketd ready");
    // A fresh authenticated session to the trusted computer, if the user wanted one
    // before the restart. Nothing about the old session is reused.
    if recovered.dock_wanted {
        match dock_control.connect("restoring the link after a core restart") {
            Ok(()) => {}
            Err(dock::ConnectError::NotConfigured(e)) => {
                warn!(reason = %e, "dock: link was wanted but no computer is configured")
            }
            Err(dock::ConnectError::Core(e)) => warn!(error = ?e, "dock: cannot restore the link"),
        }
    }

    let stop = tokio::select! {
        _ = sigterm.recv() => Stop::Signal("SIGTERM"),
        _ = sigint.recv() => Stop::Signal("SIGINT"),
        _ = sighup.recv() => Stop::Signal("SIGHUP"),
        _ = conn.closed() => Stop::BusClosed,
        _ = async {
            while let Some(sig) = name_lost.next().await {
                if sig.args().is_ok_and(|a| a.name() == BUS_NAME) {
                    return;
                }
            }
            // The stream also ends when the bus goes away (session stop); that is not
            // a lost name, so leave it to the `closed()` branch.
            std::future::pending::<()>().await
        } => Stop::NameLost,
    };

    match stop {
        Stop::Signal(sig) => {
            info!(signal = sig, "pocketd stopping");
            // Background tasks hold connection references; stop them first. Aborting the
            // Dock link closes its TCP session (the agent sees the phone disconnect).
            dock_link.abort();
            watcher.abort();
            let _ = dock_link.await;
            let _ = watcher.await;
            match conn.release_name(BUS_NAME).await {
                Ok(_) => info!(name = BUS_NAME, "bus name released"),
                Err(e) => warn!(error = %e, "cannot release bus name"),
            }
            // The NameLost stream holds a connection reference; drop it so the shutdown
            // can complete, and do not let a misbehaving peer block the exit.
            drop(name_lost);
            if tokio::time::timeout(Duration::from_secs(2), conn.graceful_shutdown())
                .await
                .is_err()
            {
                warn!("graceful shutdown timed out");
            }
            info!("pocketd stopped");
            ExitCode::SUCCESS
        }
        Stop::NameLost => {
            error!(name = BUS_NAME, "bus name lost; exiting");
            ExitCode::FAILURE
        }
        Stop::BusClosed => {
            warn!("session bus closed; exiting");
            ExitCode::SUCCESS
        }
    }
}

async fn mark_ready(conn: &zbus::Connection) -> zbus::Result<()> {
    let iface = conn
        .object_server()
        .interface::<_, CoreService>(OBJECT_PATH)
        .await?;
    let emitter = iface.signal_emitter().clone();
    iface.get_mut().await.mark_ready(&emitter).await
}
