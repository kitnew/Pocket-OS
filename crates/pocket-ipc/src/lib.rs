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
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };

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
        assert_eq!(PROTOCOL_VERSION.to_string(), "1.0");
    }

    #[test]
    fn dbus_names_carry_the_major_version() {
        let suffix = format!("Core{}", PROTOCOL_VERSION.major);
        assert!(dbus::BUS_NAME.ends_with(&suffix));
        assert!(dbus::INTERFACE.ends_with(&suffix));
        assert!(dbus::OBJECT_PATH.ends_with(&suffix));
    }
}
