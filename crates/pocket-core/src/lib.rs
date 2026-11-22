//! Pocket Core: framework-independent state and logic.
//!
//! This crate must never depend on Qt/QML, Quickshell, Phoc or an IPC transport. It is
//! driven by `pocketd`, which maps D-Bus calls to [`Command`]s and publishes the returned
//! [`Event`]s as D-Bus signals.
//!
//! Model: commands in, validated, applied atomically, events out. No side effects, no
//! I/O, no clocks, so every transition is unit-testable.

use std::fmt;

pub use pocket_ipc::{Mode, PROTOCOL_VERSION, ProtocolVersion};

/// Version of this core build (reported over IPC).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Commands the core accepts (from IPC clients or, later, platform adapters).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Switch the logical operating mode. Requesting the current mode is a no-op.
    RequestMode(Mode),
}

/// Events produced by a state change. Clients mirror state from these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    ModeChanged {
        from: Mode,
        to: Mode,
    },
    /// Emitted after every change, carrying the full new state.
    StateChanged(Snapshot),
}

/// Typed core errors. Every rejected command leaves the state untouched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// The core has not finished starting (see [`StateManager::mark_ready`]).
    NotReady,
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::NotReady => f.write_str("core is not ready yet"),
        }
    }
}

impl std::error::Error for CoreError {}

/// Immutable copy of the observable core state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub version: &'static str,
    pub protocol: ProtocolVersion,
    pub mode: Mode,
    pub ready: bool,
    /// Strictly increasing with every change (starts at 1 in a fresh core).
    pub revision: u64,
}

/// Authoritative core state. Owned by exactly one task in `pocketd`.
///
/// Holds only state that Pocket itself is the authority for; values owned by other
/// system services (battery, network, …) are not mirrored here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateManager {
    mode: Mode,
    ready: bool,
    revision: u64,
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl StateManager {
    /// Predictable initial state: handheld mode, not ready, revision 1.
    pub fn new() -> Self {
        Self {
            mode: Mode::default(),
            ready: false,
            revision: 1,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            version: VERSION,
            protocol: PROTOCOL_VERSION,
            mode: self.mode,
            ready: self.ready,
            revision: self.revision,
        }
    }

    /// Marks startup as complete (the daemon owns its bus name). Idempotent.
    pub fn mark_ready(&mut self) -> Vec<Event> {
        if self.ready {
            return Vec::new();
        }
        self.ready = true;
        self.changed(Vec::new())
    }

    /// Validates and applies a command. On success returns the produced events (empty
    /// for a no-op); on error the state is unchanged.
    pub fn apply(&mut self, command: Command) -> Result<Vec<Event>, CoreError> {
        if !self.ready {
            return Err(CoreError::NotReady);
        }
        match command {
            Command::RequestMode(to) if to == self.mode => Ok(Vec::new()),
            Command::RequestMode(to) => {
                let from = std::mem::replace(&mut self.mode, to);
                Ok(self.changed(vec![Event::ModeChanged { from, to }]))
            }
        }
    }

    fn changed(&mut self, mut events: Vec<Event>) -> Vec<Event> {
        self.revision += 1;
        events.push(Event::StateChanged(self.snapshot()));
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready() -> StateManager {
        let mut s = StateManager::new();
        s.mark_ready();
        s
    }

    #[test]
    fn initial_state_is_predictable() {
        let s = StateManager::new().snapshot();
        assert_eq!(s.mode, Mode::Handheld);
        assert!(!s.ready);
        assert_eq!(s.revision, 1);
        assert_eq!(s.version, VERSION);
        assert_eq!(s.protocol, PROTOCOL_VERSION);
    }

    #[test]
    fn commands_are_rejected_until_ready() {
        let mut s = StateManager::new();
        let before = s.clone();
        assert_eq!(
            s.apply(Command::RequestMode(Mode::Dock)),
            Err(CoreError::NotReady)
        );
        assert_eq!(s, before);
    }

    #[test]
    fn mark_ready_emits_state_once() {
        let mut s = StateManager::new();
        let events = s.mark_ready();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            Event::StateChanged(Snapshot {
                ready: true,
                revision: 2,
                ..
            })
        ));
        assert!(s.mark_ready().is_empty());
        assert_eq!(s.snapshot().revision, 2);
    }

    #[test]
    fn mode_change_emits_mode_then_state() {
        let mut s = ready();
        let rev = s.snapshot().revision;
        let events = s.apply(Command::RequestMode(Mode::Dock)).unwrap();
        assert_eq!(
            events,
            vec![
                Event::ModeChanged {
                    from: Mode::Handheld,
                    to: Mode::Dock
                },
                Event::StateChanged(s.snapshot()),
            ]
        );
        assert_eq!(s.mode(), Mode::Dock);
        assert_eq!(s.snapshot().revision, rev + 1);
    }

    #[test]
    fn requesting_current_mode_is_a_silent_no_op() {
        let mut s = ready();
        let before = s.clone();
        assert_eq!(s.apply(Command::RequestMode(Mode::Handheld)), Ok(vec![]));
        assert_eq!(s, before);
    }

    #[test]
    fn revision_strictly_increases_across_transitions() {
        let mut s = ready();
        let mut last = s.snapshot().revision;
        for to in [
            Mode::Dock,
            Mode::Handheld,
            Mode::Dock,
            Mode::Dock,
            Mode::Handheld,
        ] {
            s.apply(Command::RequestMode(to)).unwrap();
            let rev = s.snapshot().revision;
            assert!(rev >= last);
            last = rev;
            assert_eq!(s.mode(), to);
        }
        // 4 real changes after ready (rev 2): the repeated Dock was a no-op.
        assert_eq!(last, 6);
    }
}
