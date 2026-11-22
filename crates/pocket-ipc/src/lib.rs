//! IPC contract types shared between Pocket Core and its clients (the shell UI, tools).
//!
//! These types are the *wire contract*: they must stay free of UI-framework, compositor
//! and transport details. The initial transport candidate is D-Bus (see
//! `docs/architecture/overview.md`); no transport is implemented yet.

use std::fmt;
use std::str::FromStr;

/// Version of the Pocket IPC contract. Bump the major on any incompatible change.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 0, minor: 1 };

/// Contract version, negotiated by clients before using the API.
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
        write!(f, "unknown mode {:?}", self.0)
    }
}

impl std::error::Error for UnknownMode {}

impl FromStr for Mode {
    type Err = UnknownMode;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "handheld" => Ok(Mode::Handheld),
            "dock" => Ok(Mode::Dock),
            other => Err(UnknownMode(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_round_trips_through_wire_names() {
        for mode in [Mode::Handheld, Mode::Dock] {
            assert_eq!(mode.as_str().parse::<Mode>(), Ok(mode));
        }
        assert!("desktop".parse::<Mode>().is_err());
    }

    #[test]
    fn version_compatibility() {
        let v = |major, minor| ProtocolVersion { major, minor };
        assert!(v(0, 1).is_compatible_with(v(0, 1)));
        assert!(v(0, 1).is_compatible_with(v(0, 2)));
        assert!(!v(0, 2).is_compatible_with(v(0, 1)));
        assert!(!v(1, 0).is_compatible_with(v(0, 9)));
        assert_eq!(PROTOCOL_VERSION.to_string(), "0.1");
    }
}
