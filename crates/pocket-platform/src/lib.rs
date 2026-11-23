//! Pocket OS platform adapters: narrowly scoped integrations with standard Linux
//! mechanisms that Pocket Core needs to act on the system.
//!
//! Stage 2 has one adapter, [`apps`]: discovery of installed applications from
//! Freedesktop Desktop Entries and preparing their launch command. Parsing of the key
//! file format, locales and `Exec` quoting is done by the `freedesktop-desktop-entry`
//! crate; icon lookup by `freedesktop-icons`. This crate does I/O but knows nothing
//! about D-Bus, Qt or the compositor; `pocketd` exposes it over IPC.

pub mod apps;
