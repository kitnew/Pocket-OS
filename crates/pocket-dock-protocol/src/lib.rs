//! Pocket Dock protocol: the contract between Pocket Core on the phone (`pocketd`, the
//! client) and the Pocket Desktop Agent on a computer (the server).
//!
//! Layers (ADR-0007, `docs/decisions/0007-dock-architecture.md`):
//!
//! 1. **TCP**, normally over the USB network link (phone `172.16.42.1` → host
//!    `172.16.42.2`, port [`DEFAULT_PORT`]).
//! 2. **Noise** [`NOISE_PARAMS`] handshake (mutual authentication with pinned static
//!    X25519 keys, then ChaCha20-Poly1305 encryption), see [`channel`]. Frames are a
//!    big-endian `u16` length followed by one Noise message.
//! 3. **JSON messages** ([`message`]), one per frame. The client opens with `hello`
//!    (protocol version), the agent answers `welcome` (its version and actions) or a fatal
//!    `error`. Requests carry ids; every `invoke` gets exactly one `result`.
//!
//! Keys live in small files outside the repository ([`keys`]); trust is established by
//! copying public keys over an already-trusted channel (SSH), see the development guide.

pub mod channel;
pub mod keys;
pub mod message;

use std::time::Duration;

pub use message::Version;

/// Version of this protocol. Majors must match; minors only add (unknown message types
/// and fields are ignored by both sides).
pub const PROTOCOL: Version = Version { major: 1, minor: 0 };

/// Noise protocol name. Changing it is a new protocol major.
pub const NOISE_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

/// Noise prologue (both sides must use the same bytes, or the handshake fails).
pub const PROLOGUE: &[u8] = b"pocket-dock/1";

/// Default TCP port of the desktop agent.
pub const DEFAULT_PORT: u16 = 47130;

/// Largest Noise message (the frame length is a `u16`).
pub const MAX_FRAME: usize = 65535;

/// Largest plaintext in one frame (frame minus the 16-byte AEAD tag).
pub const MAX_PAYLOAD: usize = MAX_FRAME - 16;

/// Time allowed for the TCP connect, the Noise handshake and hello/welcome together.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// The client sends a `ping` after this much silence; the agent closes a connection that
/// stays silent for [`IDLE_TIMEOUT`].
pub const PING_INTERVAL: Duration = Duration::from_secs(20);

/// Maximum time the client waits for a `pong` before it treats the link as dead.
pub const PONG_TIMEOUT: Duration = Duration::from_secs(8);

/// The agent closes a connection without any message for this long (3 missed pings).
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Stable machine-readable error codes (`result.error.code` and fatal `error.code`).
pub mod error_code {
    /// Fatal: the client's static key is not trusted by the agent.
    pub const UNAUTHORIZED: &str = "unauthorized";
    /// Fatal: protocol majors differ.
    pub const INCOMPATIBLE_PROTOCOL: &str = "incompatible_protocol";
    /// Fatal: malformed or out-of-order message.
    pub const PROTOCOL_ERROR: &str = "protocol_error";
    /// Fatal: another session from the same peer replaced this one.
    pub const REPLACED: &str = "replaced";
    /// The action id is not registered on the agent.
    pub const UNKNOWN_ACTION: &str = "unknown_action";
    /// The action exists but is disabled in the agent configuration.
    pub const NOT_PERMITTED: &str = "not_permitted";
    /// The action is registered but cannot run now (e.g. no media player).
    pub const UNAVAILABLE: &str = "unavailable";
    /// Missing, unknown or ill-typed parameters.
    pub const INVALID_PARAMS: &str = "invalid_params";
    /// The action ran and failed on the desktop.
    pub const FAILED: &str = "failed";
}

/// Action ids are stable, lowercase, dot-separated identifiers such as
/// `desktop.notify`: 1–64 characters of `a-z 0-9 . _ -`, starting with a letter.
pub fn is_valid_action_id(id: &str) -> bool {
    let mut chars = id.chars();
    matches!(chars.next(), Some('a'..='z'))
        && id.len() <= 64
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '.' | '_' | '-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_ids() {
        for ok in ["desktop.notify", "media.play_pause", "a", "x-1.y_2"] {
            assert!(is_valid_action_id(ok), "{ok}");
        }
        for bad in [
            "",
            "Desktop.notify",
            "1abc",
            ".x",
            "a b",
            "a/b",
            "rm -rf",
            "ä",
            &"a".repeat(65),
        ] {
            assert!(!is_valid_action_id(bad), "{bad}");
        }
    }
}
