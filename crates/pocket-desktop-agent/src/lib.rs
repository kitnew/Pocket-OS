//! Pocket Desktop Agent: the computer side of Pocket Dock (ADR-0007).
//!
//! It accepts Noise-authenticated connections from one or more *trusted* phones
//! ([`config`]), advertises its registered actions, executes them and returns structured
//! results ([`server`]). Actions are explicit Rust types behind the [`actions::Action`]
//! trait; there is no shell-command or scripting endpoint. Adding an action means adding
//! one type and one `register` call, without touching networking.

pub mod actions;
pub mod config;
pub mod server;
