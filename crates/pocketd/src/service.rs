//! D-Bus binding of the core: `org.pocketos.Core1` on `/org/pocketos/Core1`.
//!
//! This module only translates between D-Bus and `pocket-core`; all validation and state
//! transitions live in `pocket_core::StateManager`. The contract is documented in
//! `docs/decisions/0002-pocket-core-and-ipc.md`.

use std::collections::HashMap;

use pocket_core::{Command, CoreError, Event, Mode, Snapshot, StateManager};
use pocket_ipc::dbus::state_key;

use crate::dock::DockControl;
use crate::recovery;
use tracing::{info, warn};
use zbus::{DBusError, Variant, interface, message::Header, object_server::SignalEmitter};

/// The `a{sv}` state dictionary (keys: `pocket_ipc::dbus::state_key`).
pub type StateDict = HashMap<&'static str, Variant<'static>>;

pub fn state_dict(s: &Snapshot) -> StateDict {
    HashMap::from([
        (state_key::VERSION, Variant::from(s.version.to_owned())),
        (state_key::PROTOCOL, Variant::from(s.protocol.to_string())),
        (state_key::MODE, Variant::from(s.mode.as_str().to_owned())),
        (state_key::READY, Variant::from(s.ready)),
        (state_key::REVISION, Variant::from(s.revision)),
    ])
}

/// Errors returned to D-Bus callers as `org.pocketos.Core1.Error.<Variant>`.
#[derive(Debug, DBusError)]
#[zbus(prefix = "org.pocketos.Core1.Error")]
pub enum Error {
    #[zbus(error)]
    ZBus(zbus::Error),
    /// The requested mode name is not a known mode.
    InvalidMode(String),
    /// The core has not finished starting.
    NotReady(String),
    /// Dock: no trusted computer is configured on the phone (since 1.2).
    NotConfigured(String),
    /// Dock: the request needs a connected computer (since 1.2).
    NotConnected(String),
    /// Dock: the connected computer does not offer this action (since 1.2).
    UnknownAction(String),
    /// Dock: the action is offered but cannot run now (since 1.2).
    ActionUnavailable(String),
    /// Dock: a malformed argument, e.g. an unsupported parameter type (since 1.2).
    InvalidArgument(String),
    /// Dock: `Retry` of a request that is not (or no longer) uncertain (since 1.4).
    UnknownRequest(String),
}

impl From<CoreError> for Error {
    fn from(e: CoreError) -> Self {
        match e {
            CoreError::NotReady => Error::NotReady(e.to_string()),
        }
    }
}

pub struct CoreService {
    state: StateManager,
    recovery: recovery::Handle,
    dock: DockControl,
}

impl CoreService {
    /// `mode` is the recovered mode (handheld in a new session).
    pub fn new(mode: Mode, recovery: recovery::Handle, dock: DockControl) -> Self {
        Self {
            state: StateManager::restored(mode),
            recovery,
            dock,
        }
    }

    /// Completes startup once the bus name is owned, announcing the ready state.
    pub async fn mark_ready(&mut self, emitter: &SignalEmitter<'_>) -> zbus::Result<()> {
        let events = self.state.mark_ready();
        publish(emitter, &events).await
    }
}

/// Emits the D-Bus signals for core events, in order.
async fn publish(emitter: &SignalEmitter<'_>, events: &[Event]) -> zbus::Result<()> {
    for event in events {
        match event {
            Event::ModeChanged { from, to } => {
                info!(%from, %to, "mode changed");
                CoreService::mode_changed(emitter, to.as_str()).await?;
            }
            Event::StateChanged(s) => {
                info!(mode = %s.mode, ready = s.ready, revision = s.revision, "state changed");
                CoreService::state_changed(emitter, state_dict(s)).await?;
            }
        }
    }
    Ok(())
}

fn sender(hdr: &Header<'_>) -> String {
    hdr.sender().map_or_else(|| "?".into(), |s| s.to_string())
}

#[interface(name = "org.pocketos.Core1")]
impl CoreService {
    /// `GetVersion() -> (s core_version, u protocol_major, u protocol_minor)`
    fn get_version(&self) -> (String, u32, u32) {
        let s = self.state.snapshot();
        (
            s.version.to_owned(),
            s.protocol.major.into(),
            s.protocol.minor.into(),
        )
    }

    /// `GetState() -> a{sv}`
    fn get_state(&self) -> StateDict {
        state_dict(&self.state.snapshot())
    }

    /// `GetMode() -> s`
    fn get_mode(&self) -> String {
        self.state.mode().as_str().to_owned()
    }

    /// `RequestMode(s mode)`. Replies once the change is applied; the change itself is
    /// announced by `ModeChanged` + `StateChanged`. Requesting the current mode succeeds
    /// without signals. Errors: `…Error.InvalidMode`, `…Error.NotReady`.
    async fn request_mode(
        &mut self,
        mode: &str,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> Result<(), Error> {
        let sender = sender(&hdr);
        let mode: Mode = match mode.parse() {
            Ok(m) => m,
            Err(e) => {
                warn!(%sender, requested = mode, "rejected RequestMode: invalid mode");
                return Err(Error::InvalidMode(e.to_string()));
            }
        };
        info!(%sender, requested = %mode, "RequestMode");
        let events = self
            .state
            .apply(Command::RequestMode(mode))
            .inspect_err(|e| {
                warn!(%sender, error = %e, "rejected RequestMode");
            })?;
        for event in &events {
            if let Event::ModeChanged { to, .. } = event {
                self.recovery.set_mode(*to);
                if *to == Mode::Dock {
                    self.dock.entered_dock_mode();
                }
            }
        }
        publish(&emitter, &events).await?;
        Ok(())
    }

    /// `ModeChanged(s mode)`: the new mode.
    #[zbus(signal)]
    async fn mode_changed(emitter: &SignalEmitter<'_>, mode: &str) -> zbus::Result<()>;

    /// `StateChanged(a{sv} state)`: the full new state.
    #[zbus(signal)]
    async fn state_changed(emitter: &SignalEmitter<'_>, state: StateDict) -> zbus::Result<()>;
}
