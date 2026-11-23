//! IPC contract shared between Pocket Core (`pocketd`) and its clients (the shell UI,
//! tools, tests).
//!
//! This crate is the *wire contract*: names, versions and value encodings. It stays free
//! of UI-framework, compositor and transport-library details; `pocketd` binds it to D-Bus
//! with zbus, and the QML bridge (`bridge/pocket-core-qml`) mirrors the same constants.
//! The contract is documented in `docs/decisions/0002-pocket-core-and-ipc.md`.

use std::fmt;
use std::str::FromStr;

/// Version of the Pocket IPC contract. The major is part of the D-Bus names (`Core1`);
/// bump it (and the names) on any incompatible change, bump the minor on additions.
/// History: 1.0 core state and modes (Stage 1); 1.1 adds the application object
/// [`dbus::apps`] (Stage 2); 1.2 adds the Dock object [`dbus::dock`] (Stage 3); 1.3 adds
/// the Dock state keys `retry_in_ms` and `connection_wanted` (Stage 3.1); 1.4 adds the
/// Dock platform members (deck, retry, live state, notifications; Stage 4,
/// docs/dock-mode/protocol.md); 1.5 adds `SetDeck`/`ResetDeck` (on-phone deck editing,
/// Stage 4.5).
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 5 };

/// D-Bus names of the core service (session bus).
pub mod dbus {
    /// Well-known bus name owned by `pocketd`.
    pub const BUS_NAME: &str = "org.pocketos.Core1";
    /// Object path of the core object.
    pub const OBJECT_PATH: &str = "/org/pocketos/Core1";
    /// Interface implemented by the core object.
    pub const INTERFACE: &str = "org.pocketos.Core1";
    /// Prefix of the D-Bus error names returned by the core.
    pub const ERROR_PREFIX: &str = "org.pocketos.Core1.Error";

    /// Keys of the `a{sv}` state dictionary returned by `GetState` and carried by
    /// `StateChanged`. Clients must ignore unknown keys (minor versions may add some).
    pub mod state_key {
        /// `s`: core version, e.g. `"0.1.0"`.
        pub const VERSION: &str = "version";
        /// `s`: IPC protocol version, `"<major>.<minor>"`.
        pub const PROTOCOL: &str = "protocol";
        /// `s`: current mode, a [`crate::Mode`] wire name.
        pub const MODE: &str = "mode";
        /// `b`: the core has finished starting and accepts requests.
        pub const READY: &str = "ready";
        /// `t`: state revision; strictly increases with every change within one core
        /// process. Restarts from 1 when the core restarts (a new bus-name owner).
        pub const REVISION: &str = "revision";
    }

    /// Installed applications and launching (since protocol 1.1). Served by `pocketd`
    /// on the same bus name as the core object.
    ///
    /// - `ListApps() -> aa{sv}`: the catalog, sorted by name (keys: [`app_key`]).
    /// - `Launch(s id) -> u pid`: starts the application; `pid` is 0 when it was
    ///   activated over D-Bus. Errors: `…Error.UnknownApp`, `…Error.LaunchFailed`.
    /// - `Reload()`: rescans the application directories now.
    /// - signal `AppsChanged()`: the catalog changed; call `ListApps` again.
    /// - signal `AppExited(s id, u pid, b success, s detail)`: a process started by
    ///   `Launch` ended (`detail`: `status=N` or `signal=N`).
    pub mod apps {
        pub const OBJECT_PATH: &str = "/org/pocketos/Core1/Apps";
        pub const INTERFACE: &str = "org.pocketos.Core1.Apps";

        /// Keys of one application dictionary. Clients must ignore unknown keys.
        pub mod app_key {
            /// `s`: desktop-file ID without `.desktop`.
            pub const ID: &str = "id";
            /// `s`: localised name.
            pub const NAME: &str = "name";
            /// `s`: localised generic name (may be empty).
            pub const GENERIC_NAME: &str = "generic_name";
            /// `s`: localised comment (may be empty).
            pub const COMMENT: &str = "comment";
            /// `s`: `Icon` value as written (may be empty).
            pub const ICON_NAME: &str = "icon_name";
            /// `s`: absolute path of the resolved icon file, empty if none was found.
            pub const ICON_PATH: &str = "icon_path";
            /// `as`: categories.
            pub const CATEGORIES: &str = "categories";
            /// `as`: localised keywords.
            pub const KEYWORDS: &str = "keywords";
            /// `s`: `StartupWMClass` (may be empty). Window app_ids match it or `id`.
            pub const STARTUP_WM_CLASS: &str = "startup_wm_class";
        }
    }

    /// Dock session: the link to one trusted desktop agent (since protocol 1.2). Served by
    /// `pocketd` on the same bus name. The connection state is separate from the
    /// operating [`crate::Mode`]: Dock Mode renders without a connection, and a
    /// connection may exist in Handheld Mode. Decision record: ADR-0007.
    ///
    /// - `GetState() -> a{sv}`: [`state_key`].
    /// - `ListActions() -> aa{sv}`: actions advertised by the connected agent
    ///   ([`action_key`]); empty unless connected.
    /// - `Connect()`: (re)reads the host configuration and starts connecting; retries
    ///   transient failures with backoff until `Disconnect`; a pending retry is pulled
    ///   forward when the network comes back. Sets `connection_wanted`.
    ///   Error `…Error.NotConfigured`.
    /// - `Disconnect()`: closes the link, stops retrying, clears `connection_wanted`.
    /// - `Execute(s action, a{sv} params) -> t request`: *accepts* a request (connected,
    ///   advertised, available); the outcome arrives later as `ActionFinished` once the
    ///   computer confirmed it. Errors `…Error.NotConnected`, `…Error.UnknownAction`,
    ///   `…Error.ActionUnavailable`, `…Error.InvalidArgument`.
    /// - signal `StateChanged(a{sv})`: full new state.
    /// - signal `ActionsChanged(aa{sv})`: full new action list.
    /// - signal `ActionFinished(t request, s action, b ok, s error_code, s message,
    ///   s data_json)`: exactly once per accepted request. Since 1.4 a duplicate result
    ///   (the computer had already run this request key) is `ok` with `message =
    ///   "duplicate"`.
    ///
    /// Since 1.4 (Stage 4, `docs/dock-mode/protocol.md`): `Retry(t) -> t`, `GetDeck() -> s`,
    /// `ReloadDeck()`, `GetLive() -> s`, `GetPreferences() -> a{sv}`,
    /// `SetPreference(s, v)`; signals `DeckChanged()`, `LiveChanged(s topic, s json)`,
    /// `ActionProgress(t request, u step, u total, s label)`, `Notification(s json)`,
    /// `NotificationClosed(s nid)`. Error `…Error.UnknownRequest`.
    ///
    /// Since 1.5 (Stage 4.5): `SetDeck(s json)` (validated, persisted, emits
    /// `DeckChanged`; invalid → `InvalidArgument`, nothing changes) and `ResetDeck()`.
    pub mod dock {
        pub const OBJECT_PATH: &str = "/org/pocketos/Core1/Dock";
        pub const INTERFACE: &str = "org.pocketos.Core1.Dock";

        /// Keys of the Dock state dictionary. Clients must ignore unknown keys.
        pub mod state_key {
            /// `s`: a [`crate::DockState`] wire name.
            pub const STATE: &str = "state";
            /// `b`: a trusted host is configured on the phone.
            pub const CONFIGURED: &str = "configured";
            /// `s`: name of the configured (or connected) computer.
            pub const HOST_NAME: &str = "host_name";
            /// `s`: short form of the host's pinned public key.
            pub const HOST_ID: &str = "host_id";
            /// `s`: `address:port` of the agent.
            pub const ADDRESS: &str = "address";
            /// `s`: agent software and version (empty unless connected).
            pub const AGENT: &str = "agent";
            /// `s`: machine-readable reason of the last failure (empty when none).
            pub const ERROR_CODE: &str = "error_code";
            /// `s`: human-readable reason of the last failure.
            pub const ERROR: &str = "error";
            /// `u`: seconds until the next automatic attempt, rounded up, computed when
            /// the state is produced (0 = none scheduled).
            pub const RETRY_IN: &str = "retry_in";
            /// `u`: the same in milliseconds (at least 1 while an attempt is scheduled).
            /// Clients count down from the moment they receive it (since 1.3).
            pub const RETRY_IN_MS: &str = "retry_in_ms";
            /// `b`: the user wants this link: set by `Connect` and by entering Dock
            /// Mode, cleared by `Disconnect`. Survives core restarts within the
            /// graphical session; a restarted core reconnects when it is set (since 1.3).
            pub const CONNECTION_WANTED: &str = "connection_wanted";
            /// `t`: strictly increasing per core process.
            pub const REVISION: &str = "revision";
        }

        /// Keys of one action dictionary. Clients must ignore unknown keys.
        pub mod action_key {
            pub const ID: &str = "id";
            pub const LABEL: &str = "label";
            pub const CATEGORY: &str = "category";
            /// `b`
            pub const AVAILABLE: &str = "available";
            /// `s`: why it is unavailable.
            pub const REASON: &str = "reason";
            /// `s`: `basic` | `elevated` (since 1.4; ADR-0008).
            pub const PRIVILEGE: &str = "privilege";
            /// `b`: running it twice has the same effect as once (since 1.4).
            pub const IDEMPOTENT: &str = "idempotent";
            /// `b`: the UI asks before sending it (since 1.4).
            pub const CONFIRM: &str = "confirm";
            /// `s`: the parameters as a JSON array of `{name, kind, required, choices:
            /// [{value, label}], min, max}` (protocol.md; since 1.5). The phone UI uses the
            /// `choices` (the laptop's closed lists) to configure tiles.
            pub const PARAMS: &str = "params";
        }

        /// Keys of `GetPreferences` (since 1.4). `SetPreference` keys: `dnd` (b),
        /// `mute_app` (s), `unmute_app` (s).
        pub mod pref_key {
            /// `b`: Do Not Disturb (no banners; history still fills).
            pub const DND: &str = "dnd";
            /// `as`: applications whose notifications are dropped.
            pub const MUTED_APPS: &str = "muted_apps";
            /// `b`: the Dock UI uses minimal animation (since 1.5).
            pub const REDUCE_MOTION: &str = "reduce_motion";
        }
    }
}

/// Contract version, checked by clients before using the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    /// A client built against `self` can talk to a server speaking `server`
    /// when the majors match and the server is at least as new.
    pub fn is_compatible_with(self, server: ProtocolVersion) -> bool {
        self.major == server.major && self.minor <= server.minor
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Top-level interaction mode of the device. Both modes share the same core and
/// UI components; only layout and enabled surfaces differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mode {
    /// Portrait, touch-first phone usage.
    #[default]
    Handheld,
    /// Landscape desktop-control surface ("Dock Mode").
    Dock,
}

impl Mode {
    pub const ALL: [Mode; 2] = [Mode::Handheld, Mode::Dock];

    /// Wire name (D-Bus `s`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Handheld => "handheld",
            Mode::Dock => "dock",
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// State of the link to the desktop agent (not a [`Mode`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DockState {
    /// No link and no attempt scheduled (initial, or after `Disconnect`).
    #[default]
    Disconnected,
    /// An attempt is in progress.
    Connecting,
    /// Authenticated session with the agent.
    Connected,
    /// The last attempt or session failed; see `error_code`. A retry may be scheduled
    /// (`retry_in` > 0), or the failure needs user action (e.g. `unauthorized`).
    Error,
}

impl DockState {
    pub const fn as_str(self) -> &'static str {
        match self {
            DockState::Disconnected => "disconnected",
            DockState::Connecting => "connecting",
            DockState::Connected => "connected",
            DockState::Error => "error",
        }
    }
}

impl fmt::Display for DockState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error for an unrecognised mode name on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownMode(pub String);

impl fmt::Display for UnknownMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown mode {:?} (expected \"handheld\" or \"dock\")",
            self.0
        )
    }
}

impl std::error::Error for UnknownMode {}

impl FromStr for Mode {
    type Err = UnknownMode;

    /// Wire names are exact and case-sensitive.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Mode::ALL
            .into_iter()
            .find(|m| m.as_str() == s)
            .ok_or_else(|| UnknownMode(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_round_trips_through_wire_names() {
        for mode in Mode::ALL {
            assert_eq!(mode.as_str().parse::<Mode>(), Ok(mode));
        }
        for bad in ["desktop", "", "Dock", " dock"] {
            assert_eq!(bad.parse::<Mode>(), Err(UnknownMode(bad.to_owned())));
        }
    }

    #[test]
    fn version_compatibility() {
        let v = |major, minor| ProtocolVersion { major, minor };
        assert!(v(1, 0).is_compatible_with(v(1, 0)));
        assert!(v(1, 0).is_compatible_with(v(1, 2)));
        assert!(!v(1, 2).is_compatible_with(v(1, 0)));
        assert!(!v(2, 0).is_compatible_with(v(1, 9)));
        assert_eq!(PROTOCOL_VERSION.to_string(), "1.5");
    }

    #[test]
    fn dbus_names_carry_the_major_version() {
        let suffix = format!("Core{}", PROTOCOL_VERSION.major);
        assert!(dbus::BUS_NAME.ends_with(&suffix));
        assert!(dbus::INTERFACE.ends_with(&suffix));
        assert!(dbus::OBJECT_PATH.ends_with(&suffix));
    }
}
