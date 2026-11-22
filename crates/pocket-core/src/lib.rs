//! Pocket Core: framework-independent state and logic.
//!
//! This crate must never depend on Qt/QML, Quickshell, Phoc or any IPC transport.
//! System access goes through platform adapters (future `pocket-platform` crate);
//! clients reach the core through the `pocket-ipc` contract.
//!
//! Stage 0 contains only the mode state machine, which establishes the shape of the
//! core: commands in, state changes + events out, no side effects.

pub use pocket_ipc::Mode;

/// Commands the core accepts (from IPC clients or adapters).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    SetMode(Mode),
}

/// Events the core emits when its state changes; clients mirror state from these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    ModeChanged { from: Mode, to: Mode },
}

/// Authoritative core state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreState {
    mode: Mode,
}

impl CoreState {
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Applies a command. Returns the events produced; no-ops produce none.
    pub fn apply(&mut self, command: Command) -> Vec<Event> {
        match command {
            Command::SetMode(to) if to != self.mode => {
                let from = std::mem::replace(&mut self.mode, to);
                vec![Event::ModeChanged { from, to }]
            }
            Command::SetMode(_) => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_in_handheld_mode() {
        assert_eq!(CoreState::default().mode(), Mode::Handheld);
    }

    #[test]
    fn mode_change_emits_event_once() {
        let mut core = CoreState::default();
        assert_eq!(
            core.apply(Command::SetMode(Mode::Dock)),
            vec![Event::ModeChanged {
                from: Mode::Handheld,
                to: Mode::Dock
            }]
        );
        assert_eq!(core.mode(), Mode::Dock);
        assert!(core.apply(Command::SetMode(Mode::Dock)).is_empty());
    }
}
