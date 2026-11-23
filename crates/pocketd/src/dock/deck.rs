//! Control Deck configuration (`deck.json`, schema v1; docs/dock-mode/architecture.md).
//!
//! The built-in default is `config/dock/deck.json` (compiled in, so a valid deck always
//! exists). A user file `<dock dir>/deck.json` replaces it. The file is validated
//! completely before use; an invalid user file is reported (`error`) and the default is
//! served instead. The result is a normalized JSON document: only known keys, defaults
//! filled in, so clients never see unvalidated input.
//!
//! Tiles name actions by id; whether the action exists, is granted and available is
//! decided at run time by the agent's advertisement, so one deck works with any computer.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

/// The built-in deck.
pub const DEFAULT: &str = include_str!("../../../../config/dock/deck.json");

pub const SCHEMA_VERSION: u64 = 1;
pub const DEFAULT_IDLE_TIMEOUT_S: u64 = 15;
pub const MAX_PAGES: usize = 12;
pub const MAX_TILES: usize = 24;
const MAX_TITLE: usize = 32;
const MAX_LABEL: usize = 40;
const MAX_PARAM_STRING: usize = 500;
const MAX_PARAMS: usize = 8;
/// Largest accepted file (bytes); a deck is a few KiB.
const MAX_FILE: u64 = 256 * 1024;

/// A loaded deck: the normalized document plus where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Deck {
    pub doc: Value,
    /// `"user"` or `"default"`.
    pub source: &'static str,
    /// Why the user file was not used (empty when it was, or when there is none).
    pub error: String,
}

impl Deck {
    /// The document served over D-Bus (`GetDeck`).
    pub fn to_json(&self) -> String {
        let mut d = self.doc.clone();
        if let Some(o) = d.as_object_mut() {
            o.insert("source".into(), Value::from(self.source));
            o.insert("error".into(), Value::from(self.error.clone()));
        }
        d.to_string()
    }
}

pub fn user_path(dir: &Path) -> PathBuf {
    dir.join("deck.json")
}

/// The built-in deck (always valid; checked by a unit test).
pub fn builtin() -> Deck {
    Deck {
        doc: validate(DEFAULT).expect("built-in deck.json is valid"),
        source: "default",
        error: String::new(),
    }
}

/// Loads the user deck if present and valid, else the built-in one.
pub fn load(dir: &Path) -> Deck {
    let path = user_path(dir);
    let text = match std::fs::metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return builtin(),
        Err(e) => return fallback(format!("{}: {e}", path.display())),
        Ok(m) if m.len() > MAX_FILE => {
            return fallback(format!("{}: larger than {MAX_FILE} bytes", path.display()));
        }
        Ok(_) => match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => return fallback(format!("{}: {e}", path.display())),
        },
    };
    match validate(&text) {
        Ok(doc) => Deck {
            doc,
            source: "user",
            error: String::new(),
        },
        Err(e) => fallback(format!("{}: {e}", path.display())),
    }
}

fn fallback(error: String) -> Deck {
    tracing::warn!(%error, "dock: invalid deck configuration; using the built-in deck");
    Deck { error, ..builtin() }
}

fn is_page_id(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some('a'..='z'))
        && s.len() <= 32
        && c.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_' | '-'))
}

fn is_icon(s: &str) -> bool {
    (1..=32).contains(&s.len()) && s.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-'))
}

fn text(o: &Map<String, Value>, key: &str, max: usize, at: &str) -> Result<String, String> {
    let v = o
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{at}: {key:?} must be a string"))?
        .trim();
    if v.is_empty() || v.chars().count() > max {
        return Err(format!("{at}: {key:?} must have 1–{max} characters"));
    }
    Ok(v.to_owned())
}

fn only_keys(o: &Map<String, Value>, allowed: &[&str], at: &str) -> Result<(), String> {
    match o.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(format!("{at}: unknown key {k:?}")),
        None => Ok(()),
    }
}

/// Validates a deck document and returns its normalized form.
pub fn validate(text_in: &str) -> Result<Value, String> {
    let root: Value = serde_json::from_str(text_in).map_err(|e| format!("not JSON: {e}"))?;
    let root = root.as_object().ok_or("the deck must be a JSON object")?;
    only_keys(root, &["version", "navigation", "pages"], "deck")?;
    if root.get("version").and_then(Value::as_u64) != Some(SCHEMA_VERSION) {
        return Err(format!("\"version\" must be {SCHEMA_VERSION}"));
    }

    let mut idle = DEFAULT_IDLE_TIMEOUT_S;
    if let Some(nav) = root.get("navigation") {
        let nav = nav.as_object().ok_or("\"navigation\" must be an object")?;
        only_keys(nav, &["idle_timeout_s"], "navigation")?;
        if let Some(v) = nav.get("idle_timeout_s") {
            idle = v
                .as_u64()
                .filter(|s| *s == 0 || (5..=600).contains(s))
                .ok_or("navigation.idle_timeout_s must be 0 (never) or 5–600")?;
        }
    }

    let pages = root
        .get("pages")
        .and_then(Value::as_array)
        .ok_or("\"pages\" must be an array")?;
    if pages.is_empty() || pages.len() > MAX_PAGES {
        return Err(format!("\"pages\" must have 1–{MAX_PAGES} entries"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out_pages = Vec::new();
    for (pi, page) in pages.iter().enumerate() {
        let at = format!("pages[{pi}]");
        let page = page.as_object().ok_or(format!("{at} must be an object"))?;
        only_keys(page, &["id", "title", "tiles"], &at)?;
        let id = page
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| is_page_id(s))
            .ok_or(format!("{at}.id must match [a-z][a-z0-9_-]{{0,31}}"))?;
        if !seen.insert(id.to_owned()) {
            return Err(format!("{at}: duplicate page id {id:?}"));
        }
        let title = text(page, "title", MAX_TITLE, &at)?;
        let tiles = page
            .get("tiles")
            .and_then(Value::as_array)
            .ok_or(format!("{at}.tiles must be an array"))?;
        if tiles.len() > MAX_TILES {
            return Err(format!("{at}: at most {MAX_TILES} tiles"));
        }
        let mut out_tiles = Vec::new();
        for (ti, tile) in tiles.iter().enumerate() {
            let at = format!("{at}.tiles[{ti}]");
            let tile = tile.as_object().ok_or(format!("{at} must be an object"))?;
            only_keys(tile, &["action", "label", "icon", "params"], &at)?;
            let action = tile
                .get("action")
                .and_then(Value::as_str)
                .filter(|a| pocket_dock_protocol::is_valid_action_id(a))
                .ok_or(format!("{at}.action must be a valid action id"))?;
            let label = text(tile, "label", MAX_LABEL, &at)?;
            let icon = match tile.get("icon") {
                None => String::new(),
                Some(Value::String(s)) if is_icon(s) => s.clone(),
                Some(_) => return Err(format!("{at}.icon must match [a-z0-9-]{{1,32}}")),
            };
            let mut params = Map::new();
            if let Some(p) = tile.get("params") {
                let p = p
                    .as_object()
                    .ok_or(format!("{at}.params must be an object"))?;
                if p.len() > MAX_PARAMS {
                    return Err(format!("{at}.params: at most {MAX_PARAMS} parameters"));
                }
                for (k, v) in p {
                    if !pocket_dock_protocol::is_valid_action_id(k) {
                        return Err(format!("{at}.params: invalid parameter name {k:?}"));
                    }
                    let ok = match v {
                        Value::String(s) => s.chars().count() <= MAX_PARAM_STRING,
                        Value::Bool(_) => true,
                        Value::Number(n) => n.is_i64() || n.is_u64(),
                        _ => false,
                    };
                    if !ok {
                        return Err(format!(
                            "{at}.params.{k}: only strings (≤ {MAX_PARAM_STRING} chars), booleans and integers"
                        ));
                    }
                    params.insert(k.clone(), v.clone());
                }
            }
            out_tiles.push(json!({
                "id": format!("{id}/{ti}"),
                "action": action,
                "label": label,
                "icon": icon,
                "params": params,
            }));
        }
        out_pages.push(json!({ "id": id, "title": title, "tiles": out_tiles }));
    }
    Ok(json!({
        "version": SCHEMA_VERSION,
        "navigation": { "idle_timeout_s": idle },
        "pages": out_pages,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(pages: Value) -> String {
        json!({"version": 1, "pages": pages}).to_string()
    }

    #[test]
    fn builtin_deck_is_valid_and_has_the_default_pages() {
        let d = builtin();
        let ids: Vec<&str> = d.doc["pages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["general", "development", "media"]);
        assert_eq!(d.doc["navigation"]["idle_timeout_s"], 15);
        assert_eq!(d.doc["pages"][0]["tiles"][0]["id"], "general/0");
    }

    #[test]
    fn normalizes_defaults() {
        let d = validate(&deck(json!([{"id": "a", "title": " A ", "tiles": [
            {"action": "x.y", "label": "Go"}]}])))
        .unwrap();
        assert_eq!(d["navigation"]["idle_timeout_s"], 15);
        assert_eq!(d["pages"][0]["title"], "A");
        assert_eq!(d["pages"][0]["tiles"][0]["icon"], "");
        assert_eq!(d["pages"][0]["tiles"][0]["params"], json!({}));
    }

    #[test]
    fn rejects_invalid_documents() {
        let tile = |t: Value| deck(json!([{"id": "a", "title": "A", "tiles": [t]}]));
        let cases = [
            "not json".to_owned(),
            "[]".to_owned(),
            json!({"version": 2, "pages": []}).to_string(),
            json!({"version": 1, "pages": [], "extra": 1}).to_string(),
            deck(json!([])),
            deck(json!([{"id": "A", "title": "x", "tiles": []}])),
            deck(json!([{"id": "a", "title": "", "tiles": []}])),
            deck(json!([{"id": "a", "title": "x", "tiles": []}, {"id": "a", "title": "y", "tiles": []}])),
            json!({"version": 1, "navigation": {"idle_timeout_s": 3}, "pages": [{"id": "a", "title": "x", "tiles": []}]}).to_string(),
            tile(json!({"action": "rm -rf", "label": "x"})),
            tile(json!({"action": "a.b", "label": ""})),
            tile(json!({"action": "a.b", "label": "x", "icon": "../etc"})),
            tile(json!({"action": "a.b", "label": "x", "params": {"p": 1.5}})),
            tile(json!({"action": "a.b", "label": "x", "params": {"p": {"nested": 1}}})),
            tile(json!({"action": "a.b", "label": "x", "params": {"p": "x".repeat(501)}})),
            tile(json!({"action": "a.b", "label": "x", "cmd": "sh"})),
        ];
        for c in cases {
            assert!(validate(&c).is_err(), "accepted: {c}");
        }
        let many: Vec<Value> = (0..25)
            .map(|_| json!({"action": "a.b", "label": "x"}))
            .collect();
        assert!(validate(&deck(json!([{"id": "a", "title": "A", "tiles": many}]))).is_err());
        // 0 = never return automatically
        assert!(validate(&json!({"version": 1, "navigation": {"idle_timeout_s": 0}, "pages": [{"id": "a", "title": "x", "tiles": []}]}).to_string()).is_ok());
    }

    #[test]
    fn user_file_replaces_the_default_and_bad_files_fall_back() {
        let dir = std::env::temp_dir().join(format!("pocket-deck-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(load(&dir).source, "default");
        std::fs::write(
            user_path(&dir),
            deck(json!([{"id": "mine", "title": "Mine", "tiles": []}])),
        )
        .unwrap();
        let d = load(&dir);
        assert_eq!(
            (d.source, d.doc["pages"][0]["id"].as_str()),
            ("user", Some("mine"))
        );
        std::fs::write(user_path(&dir), "{ broken").unwrap();
        let d = load(&dir);
        assert_eq!(d.source, "default");
        assert!(d.error.contains("not JSON"), "{}", d.error);
        let v: Value = serde_json::from_str(&d.to_json()).unwrap();
        assert_eq!(v["source"], "default");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
