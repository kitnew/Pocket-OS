//! Session-scoped recovery of the user's choices across core restarts (issue #5).
//!
//! What is kept: the operating mode and whether the user wants the Dock link
//! (`dock_wanted`). Nothing else: no connection state, sockets, keys, requests or
//! retry timers. A restarted core rebuilds those itself.
//!
//! Where: `$XDG_RUNTIME_DIR/pocket-os/core-recovery.json`. That directory belongs to the
//! graphical session: `pocket-session-run` deletes it when a session starts and when it
//! ends, so the file lives exactly as long as the session. `pocketd` never creates the
//! directory: started outside a Pocket session (tests, `cargo run`), it has no recovery
//! and starts fresh, as before.
//!
//! Format (version 1, unknown fields ignored):
//! `{"version":1,"mode":"dock","dock_wanted":true}`. Writes are atomic (temporary file in
//! the same directory, mode 0600, then rename), so a crash leaves the old or the new
//! file, never a torn one. A missing, unreadable, truncated or invalid file means "fresh
//! start" (logged), never a failed start.

use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pocket_core::Mode;
use serde_json::{Value, json};
use tracing::{info, warn};

pub const FILE_NAME: &str = "core-recovery.json";
const VERSION: u64 = 1;

/// The recoverable choices. `Default` is a fresh session: handheld, no link wanted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Choices {
    pub mode: Mode,
    pub dock_wanted: bool,
}

impl Choices {
    fn to_json(self) -> String {
        json!({
            "version": VERSION,
            "mode": self.mode.as_str(),
            "dock_wanted": self.dock_wanted,
        })
        .to_string()
    }

    fn parse(text: &str) -> Result<Choices, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
        match v.get("version").and_then(Value::as_u64) {
            Some(VERSION) => {}
            other => return Err(format!("unsupported version {other:?}")),
        }
        let mode = v
            .get("mode")
            .and_then(Value::as_str)
            .ok_or("missing mode")?
            .parse::<Mode>()
            .map_err(|e| e.to_string())?;
        let dock_wanted = v
            .get("dock_wanted")
            .and_then(Value::as_bool)
            .ok_or("missing dock_wanted")?;
        Ok(Choices { mode, dock_wanted })
    }
}

/// Shared by the core object (mode) and the Dock object (link intent).
#[derive(Debug)]
pub struct Recovery {
    path: Option<PathBuf>,
    current: Mutex<Choices>,
}

pub type Handle = Arc<Recovery>;

/// `$XDG_RUNTIME_DIR/pocket-os/core-recovery.json`, if the session directory exists.
pub fn session_path() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join("pocket-os");
    dir.is_dir().then(|| dir.join(FILE_NAME))
}

impl Recovery {
    /// Reads the recovery file (if any) and returns the handle plus the recovered choices.
    pub fn open(path: Option<PathBuf>) -> (Handle, Choices) {
        let choices = match &path {
            None => {
                info!("recovery: no session runtime directory; starting fresh");
                Choices::default()
            }
            Some(p) => match std::fs::read_to_string(p) {
                Ok(text) => match Choices::parse(&text) {
                    Ok(c) => {
                        info!(mode = %c.mode, dock_wanted = c.dock_wanted, path = %p.display(), "recovery: restoring session choices");
                        c
                    }
                    Err(e) => {
                        warn!(path = %p.display(), error = %e, "recovery: ignoring invalid file; starting fresh");
                        Choices::default()
                    }
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    info!("recovery: new session; starting fresh");
                    Choices::default()
                }
                Err(e) => {
                    warn!(path = %p.display(), error = %e, "recovery: cannot read; starting fresh");
                    Choices::default()
                }
            },
        };
        (
            Arc::new(Recovery {
                path,
                current: Mutex::new(choices),
            }),
            choices,
        )
    }

    pub fn set_mode(&self, mode: Mode) {
        self.update(|c| c.mode = mode);
    }

    pub fn set_dock_wanted(&self, wanted: bool) {
        self.update(|c| c.dock_wanted = wanted);
    }

    fn update(&self, f: impl FnOnce(&mut Choices)) {
        let mut cur = self.current.lock().unwrap_or_else(|p| p.into_inner());
        let before = *cur;
        f(&mut cur);
        if *cur == before {
            return;
        }
        let Some(path) = &self.path else { return };
        // Written under the lock, so concurrent updates reach the file in order. The
        // file is a few dozen bytes on tmpfs.
        match write_atomic(path, cur.to_json().as_bytes()) {
            Ok(()) => info!(mode = %cur.mode, dock_wanted = cur.dock_wanted, "recovery: saved"),
            Err(e) => warn!(path = %path.display(), error = %e, "recovery: cannot save"),
        }
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    let result = (|| {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pocket-recovery-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn round_trip_and_permissions() {
        let d = tmpdir("rt");
        let p = d.join(FILE_NAME);
        let (r, c) = Recovery::open(Some(p.clone()));
        assert_eq!(c, Choices::default());
        assert!(!p.exists(), "nothing written until something changes");
        r.set_mode(Mode::Dock);
        r.set_dock_wanted(true);
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let (_, c) = Recovery::open(Some(p.clone()));
        assert_eq!(
            c,
            Choices {
                mode: Mode::Dock,
                dock_wanted: true
            }
        );
        // No temporary files left behind.
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn invalid_files_mean_a_fresh_start() {
        let d = tmpdir("bad");
        let p = d.join(FILE_NAME);
        for bad in [
            "",
            "{",
            "{\"version\":1,\"mode\":\"dock\"",
            "null",
            "{\"version\":2,\"mode\":\"dock\",\"dock_wanted\":true}",
            "{\"version\":1,\"mode\":\"desktop\",\"dock_wanted\":true}",
            "{\"version\":1,\"mode\":\"dock\"}",
            "{\"version\":1,\"mode\":\"dock\",\"dock_wanted\":\"yes\"}",
        ] {
            std::fs::write(&p, bad).unwrap();
            assert_eq!(
                Recovery::open(Some(p.clone())).1,
                Choices::default(),
                "{bad:?}"
            );
        }
        // Unknown fields are ignored (later versions may add some).
        std::fs::write(
            &p,
            "{\"version\":1,\"mode\":\"dock\",\"dock_wanted\":false,\"x\":1}",
        )
        .unwrap();
        assert_eq!(Recovery::open(Some(p.clone())).1.mode, Mode::Dock);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn without_a_session_directory_nothing_is_written() {
        let (r, c) = Recovery::open(None);
        assert_eq!(c, Choices::default());
        r.set_mode(Mode::Dock); // no panic, no file
    }
}
