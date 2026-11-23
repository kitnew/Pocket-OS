//! Notification preferences of the Dock (Core 1.4): Do Not Disturb and muted
//! applications, persisted in `<dock dir>/preferences.json` (0600, written atomically).
//!
//! ```json
//! { "version": 1, "dnd": false, "muted_apps": ["Thunderbird"], "reduce_motion": false }
//! ```
//! A missing or invalid file means the defaults (DND off, nothing muted); an invalid file
//! is logged and replaced on the next change.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

const MAX_MUTED: usize = 64;
const MAX_APP: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Prefs {
    pub dnd: bool,
    /// Lowercased for matching; kept in insertion order.
    pub muted_apps: Vec<String>,
    /// Dock UI: minimal animation (Core 1.5).
    pub reduce_motion: bool,
}

pub fn path(dir: &Path) -> PathBuf {
    dir.join("preferences.json")
}

impl Prefs {
    pub fn load(dir: &Path) -> Prefs {
        let p = path(dir);
        let Ok(text) = std::fs::read_to_string(&p) else {
            return Prefs::default();
        };
        match Self::parse(&text) {
            Some(prefs) => prefs,
            None => {
                tracing::warn!(path = %p.display(), "dock: invalid preferences; using defaults");
                Prefs::default()
            }
        }
    }

    fn parse(text: &str) -> Option<Prefs> {
        let v: Value = serde_json::from_str(text).ok()?;
        if v.get("version")?.as_u64()? != 1 {
            return None;
        }
        let mut p = Prefs {
            dnd: v.get("dnd").and_then(Value::as_bool).unwrap_or(false),
            muted_apps: Vec::new(),
            reduce_motion: v
                .get("reduce_motion")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        };
        for a in v
            .get("muted_apps")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            p.mute(a.as_str()?).ok()?;
        }
        Some(p)
    }

    pub fn to_json(&self) -> String {
        let mut s = json!({"version": 1, "dnd": self.dnd, "muted_apps": self.muted_apps,
                   "reduce_motion": self.reduce_motion})
        .to_string();
        s.push('\n');
        s
    }

    /// Atomically writes the file (directory 0700 created if needed, file 0600).
    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        pocket_dock_protocol::keys::write_private_file(&path(dir), self.to_json().as_bytes(), true)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }

    pub fn is_muted(&self, app: &str) -> bool {
        let a = app.to_lowercase();
        self.muted_apps.contains(&a)
    }

    pub fn mute(&mut self, app: &str) -> Result<(), String> {
        let a = app.trim().to_lowercase();
        if a.is_empty() || a.chars().count() > MAX_APP {
            return Err(format!("application name must have 1–{MAX_APP} characters"));
        }
        if !self.muted_apps.contains(&a) {
            if self.muted_apps.len() >= MAX_MUTED {
                return Err(format!("at most {MAX_MUTED} muted applications"));
            }
            self.muted_apps.push(a);
        }
        Ok(())
    }

    pub fn unmute(&mut self, app: &str) {
        let a = app.trim().to_lowercase();
        self.muted_apps.retain(|m| *m != a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_case_insensitive_muting() {
        let mut p = Prefs::default();
        p.mute("Thunderbird").unwrap();
        p.mute("thunderbird").unwrap();
        assert_eq!(p.muted_apps, ["thunderbird"]);
        assert!(p.is_muted("THUNDERBIRD"));
        p.dnd = true;
        p.reduce_motion = true;
        assert_eq!(Prefs::parse(&p.to_json()), Some(p.clone()));
        p.unmute("ThunderBird");
        assert!(!p.is_muted("thunderbird"));
        assert!(p.mute("").is_err());
        assert!(p.mute(&"x".repeat(65)).is_err());
    }

    #[test]
    fn invalid_files_mean_defaults() {
        for bad in [
            "",
            "{",
            "[]",
            r#"{"version":2}"#,
            r#"{"version":1,"muted_apps":[1]}"#,
        ] {
            assert_eq!(Prefs::parse(bad), None, "{bad}");
        }
        assert_eq!(Prefs::parse(r#"{"version":1}"#), Some(Prefs::default()));
    }

    #[test]
    fn saved_preferences_survive_a_reload() {
        let dir = std::env::temp_dir().join(format!("pocket-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(Prefs::load(&dir), Prefs::default());
        let p = Prefs {
            dnd: true,
            muted_apps: vec!["mail".into()],
            reduce_motion: false,
        };
        p.save(&dir).unwrap();
        assert_eq!(Prefs::load(&dir), p);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
