//! Installed graphical applications (Freedesktop Desktop Entry Specification 1.5).
//!
//! [`Catalog::scan`] walks the XDG application directories in priority order
//! (`$XDG_DATA_HOME/applications`, then each `$XDG_DATA_DIRS/applications`), keeps the
//! first entry per desktop-file ID, and applies the visibility rules:
//!
//! - `Type=Application` only; `Hidden=true` and `NoDisplay=true` entries are skipped;
//! - `OnlyShowIn` / `NotShowIn` are matched against the current desktop names
//!   (`XDG_CURRENT_DESKTOP`, `Pocket` in a Pocket session), case-insensitively;
//! - `TryExec` must resolve to an executable file;
//! - `Terminal=true` entries are skipped (no terminal integration yet);
//! - `Exec` must parse into a command whose program resolves to an executable.
//!
//! The launch command is fixed when the catalog is built: `Exec` is unquoted and
//! tokenised by `freedesktop-desktop-entry` (spec quoting rules, no shell), file/URL
//! field codes expand to nothing (the launcher opens no documents), and entries with
//! leftover or unsupported field codes are rejected instead of being run with a
//! half-expanded command line. Nothing here passes a string to a shell.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use freedesktop_desktop_entry::{DesktopEntry, Iter};

/// Icon size requested from the icon theme (logical px; covers launcher tiles at scale 3).
const ICON_SIZE: u16 = 128;

/// One launchable application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// Desktop-file ID without the `.desktop` suffix, e.g. `org.gtk.Demo` or `foot`.
    pub id: String,
    /// Localised `Name`.
    pub name: String,
    pub generic_name: String,
    pub comment: String,
    /// `Icon` value as written (theme name or absolute path); may be empty.
    pub icon_name: String,
    /// Resolved icon file, if one was found.
    pub icon_path: Option<PathBuf>,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    /// `StartupWMClass`: the Wayland app_id some applications use instead of the ID.
    pub startup_wm_class: Option<String>,
    /// Activate through `org.freedesktop.Application` on the session bus first.
    pub dbus_activatable: bool,
    /// Fully expanded command (program + arguments).
    pub argv: Vec<String>,
    /// `Path`: working directory, if set.
    pub working_dir: Option<PathBuf>,
    /// The desktop file this entry was read from.
    pub source: PathBuf,
}

impl App {
    /// A `Command` ready to spawn: no shell, stdin closed, own process group (so session
    /// signals aimed at the launcher do not hit the application), inherited environment.
    pub fn command(&self) -> Command {
        self.command_for(&self.argv)
    }

    /// A `Command` that opens `targets` (files or URLs) with this application, through
    /// the entry's own `%f`/`%F`/`%u`/`%U` field code (re-read from [`App::source`]).
    /// Meant for targets chosen by trusted local configuration (Dock destinations), never
    /// for remote input. Errors if the entry cannot open files or URLs.
    pub fn open_command(&self, targets: &[&str], locales: &[String]) -> Result<Command, String> {
        let entry = DesktopEntry::from_path(&self.source, Some(locales))
            .map_err(|e| format!("{}: {e}", self.source.display()))?;
        let raw = entry.exec().ok_or("no Exec key")?.to_owned();
        let tokens: Vec<&str> = raw.split_ascii_whitespace().collect();
        if !tokens
            .iter()
            .any(|t| matches!(*t, "%f" | "%F" | "%u" | "%U"))
        {
            return Err(format!("{} cannot open files or links", self.name));
        }
        let mut entry = entry;
        if tokens.contains(&"%i") {
            let fixed: Vec<&str> = tokens.into_iter().filter(|t| *t != "%i").collect();
            entry.add_desktop_entry("Exec".into(), fixed.join(" "));
        }
        let argv = entry
            .parse_exec_with_uris(targets, locales)
            .map_err(|e| e.to_string())?;
        if argv.is_empty() || argv.iter().any(|a| has_field_code(a)) {
            return Err(format!("unsupported Exec in {}", self.source.display()));
        }
        Ok(self.command_for(&argv))
    }

    fn command_for(&self, argv: &[String]) -> Command {
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..]).stdin(Stdio::null()).process_group(0);
        match &self.working_dir {
            Some(dir) if dir.is_dir() => {
                cmd.current_dir(dir);
            }
            _ => {
                if let Some(home) = std::env::var_os("HOME") {
                    cmd.current_dir(home);
                }
            }
        }
        cmd
    }

    /// Case-insensitive match of a Wayland `app_id` against this entry.
    pub fn matches_app_id(&self, app_id: &str) -> bool {
        self.id.eq_ignore_ascii_case(app_id)
            || self
                .startup_wm_class
                .as_deref()
                .is_some_and(|c| c.eq_ignore_ascii_case(app_id))
    }
}

/// Why a desktop entry is not in the catalog (for diagnostics and tests).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    Unreadable(String),
    NotAnApplication,
    Hidden,
    NoDisplay,
    NotShownIn,
    TryExecMissing(String),
    Terminal,
    BadExec(String),
    ProgramMissing(String),
    /// Shadowed by an entry with the same ID in a higher-priority directory.
    Duplicate,
}

/// The installed applications, sorted by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    apps: Vec<App>,
}

/// Inputs of a scan; [`ScanOptions::from_env`] reads them from the process environment.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Application directories, highest priority first.
    pub dirs: Vec<PathBuf>,
    /// Current desktop names (lowercase), from `XDG_CURRENT_DESKTOP`.
    pub desktops: Vec<String>,
    /// Locales for `Name[xx]` etc., most preferred first.
    pub locales: Vec<String>,
    /// Directories searched for programs (`PATH`).
    pub path: Vec<PathBuf>,
    /// Resolve icons through the icon theme (off in tests).
    pub resolve_icons: bool,
    /// Preferred icon theme; `hicolor` is always the fallback.
    pub icon_theme: String,
}

impl ScanOptions {
    pub fn from_env() -> Self {
        let mut dirs = Vec::new();
        match std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
            Some(d) => dirs.push(PathBuf::from(d)),
            None => {
                if let Some(home) = std::env::var_os("HOME") {
                    dirs.push(PathBuf::from(home).join(".local/share"));
                }
            }
        }
        let data_dirs = std::env::var("XDG_DATA_DIRS")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
        dirs.extend(
            data_dirs
                .split(':')
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
        );
        let dirs = dirs.into_iter().map(|d| d.join("applications")).collect();

        let desktops = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter(|d| !d.is_empty())
            .map(str::to_ascii_lowercase)
            .collect();
        let path = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_else(|| vec!["/usr/local/bin".into(), "/usr/bin".into(), "/bin".into()]);
        let icon_theme = std::env::var("POCKET_ICON_THEME").unwrap_or_else(|_| "Adwaita".into());
        Self {
            dirs,
            desktops,
            locales: freedesktop_desktop_entry::get_languages_from_env(),
            path,
            resolve_icons: true,
            icon_theme,
        }
    }
}

impl Catalog {
    pub fn apps(&self) -> &[App] {
        &self.apps
    }

    pub fn get(&self, id: &str) -> Option<&App> {
        self.apps.iter().find(|a| a.id == id)
    }

    pub fn len(&self) -> usize {
        self.apps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    /// Scans the application directories. Returns the catalog and the skipped entries
    /// (desktop file, reason).
    pub fn scan(opts: &ScanOptions) -> (Catalog, Vec<(PathBuf, Skip)>) {
        let mut seen = HashSet::new();
        let mut apps = Vec::new();
        let mut skipped = Vec::new();
        for path in Iter::new(opts.dirs.iter().cloned()) {
            let id = desktop_id(&opts.dirs, &path);
            if !seen.insert(id.clone()) {
                skipped.push((path, Skip::Duplicate));
                continue;
            }
            match load(&path, id, opts) {
                Ok(app) => apps.push(app),
                Err(skip) => skipped.push((path, skip)),
            }
        }
        apps.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        (Catalog { apps }, skipped)
    }
}

/// Desktop-file ID: path below the applications directory, `/` replaced by `-`, without
/// the `.desktop` suffix (spec "Desktop File ID").
fn desktop_id(dirs: &[PathBuf], path: &Path) -> String {
    let rel = dirs
        .iter()
        .filter_map(|d| {
            let d = d.canonicalize().unwrap_or_else(|_| d.clone());
            path.strip_prefix(&d).ok().map(Path::to_path_buf)
        })
        .next()
        .unwrap_or_else(|| path.file_name().map(PathBuf::from).unwrap_or_default());
    let s = rel.to_string_lossy().replace('/', "-");
    s.strip_suffix(".desktop").unwrap_or(&s).to_owned()
}

fn load(path: &Path, id: String, opts: &ScanOptions) -> Result<App, Skip> {
    let mut entry = DesktopEntry::from_path(path, Some(&opts.locales))
        .map_err(|e| Skip::Unreadable(e.to_string()))?;
    entry.appid = id.clone();

    if entry.type_() != Some("Application") {
        return Err(Skip::NotAnApplication);
    }
    if entry.hidden() {
        return Err(Skip::Hidden);
    }
    if entry.no_display() {
        return Err(Skip::NoDisplay);
    }
    if !shown_in(&entry, &opts.desktops) {
        return Err(Skip::NotShownIn);
    }
    if let Some(try_exec) = entry.try_exec()
        && find_program(try_exec, &opts.path).is_none()
    {
        return Err(Skip::TryExecMissing(try_exec.to_owned()));
    }
    if entry.terminal() {
        return Err(Skip::Terminal);
    }

    let argv = expand_exec(&mut entry, &opts.locales)?;
    if find_program(&argv[0], &opts.path).is_none() {
        return Err(Skip::ProgramMissing(argv[0].clone()));
    }

    let icon_name = entry.icon().unwrap_or_default().to_owned();
    let icon_path = if opts.resolve_icons {
        resolve_icon(&icon_name, &opts.icon_theme)
    } else {
        None
    };
    let list = |v: Option<Vec<&str>>| -> Vec<String> {
        v.unwrap_or_default()
            .into_iter()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect()
    };
    Ok(App {
        name: entry
            .name(&opts.locales)
            .map(|n| n.into_owned())
            .unwrap_or_else(|| id.clone()),
        generic_name: entry
            .generic_name(&opts.locales)
            .map(|n| n.into_owned())
            .unwrap_or_default(),
        comment: entry
            .comment(&opts.locales)
            .map(|n| n.into_owned())
            .unwrap_or_default(),
        categories: list(entry.categories()),
        keywords: entry
            .keywords(&opts.locales)
            .unwrap_or_default()
            .into_iter()
            .map(|k| k.into_owned())
            .filter(|k| !k.is_empty())
            .collect(),
        startup_wm_class: entry.startup_wm_class().map(str::to_owned),
        dbus_activatable: entry.dbus_activatable(),
        working_dir: entry.path().filter(|p| !p.is_empty()).map(PathBuf::from),
        source: path.to_path_buf(),
        icon_name,
        icon_path,
        argv,
        id,
    })
}

/// `OnlyShowIn` / `NotShowIn` against the current desktop names.
fn shown_in(entry: &DesktopEntry, desktops: &[String]) -> bool {
    let current = |list: Vec<&str>| {
        list.iter()
            .any(|d| desktops.iter().any(|c| c.eq_ignore_ascii_case(d)))
    };
    if let Some(only) = entry.only_show_in()
        && !current(only)
    {
        return false;
    }
    if let Some(not) = entry.not_show_in()
        && current(not)
    {
        return false;
    }
    true
}

/// Expands `Exec` into argv with no files/URLs. Rejects malformed values and field codes
/// the parser leaves unexpanded.
fn expand_exec(entry: &mut DesktopEntry, locales: &[String]) -> Result<Vec<String>, Skip> {
    let raw = entry
        .exec()
        .ok_or_else(|| Skip::BadExec("no Exec key".into()))?
        .to_owned();
    // The spec expands an unquoted `%i` to two arguments (`--icon <Icon>`); the parser
    // would insert only the icon name. Launcher-started apps need no icon override, so
    // drop the standalone field code before parsing.
    if raw.split_ascii_whitespace().any(|t| t == "%i") {
        let fixed: Vec<&str> = raw
            .split_ascii_whitespace()
            .filter(|t| *t != "%i")
            .collect();
        entry.add_desktop_entry("Exec".into(), fixed.join(" "));
    }
    let argv = entry
        .parse_exec_with_uris(&[], locales)
        .map_err(|e| Skip::BadExec(e.to_string()))?;
    // Anything still containing a field code was not expanded by the parser (e.g. a code
    // embedded in a larger argument, or a deprecated one): do not run it half-expanded.
    if let Some(bad) = argv.iter().find(|a| has_field_code(a)) {
        return Err(Skip::BadExec(format!("unsupported field code in {bad:?}")));
    }
    // Deprecated/unknown standalone codes are dropped by the parser; detect that too.
    let dropped = raw.split_ascii_whitespace().any(|t| {
        t.len() == 2
            && t.starts_with('%')
            && !matches!(t, "%f" | "%F" | "%u" | "%U" | "%c" | "%k" | "%i")
    });
    if dropped {
        return Err(Skip::BadExec(format!("unsupported field code in {raw:?}")));
    }
    Ok(argv)
}

fn has_field_code(arg: &str) -> bool {
    let b = arg.as_bytes();
    b.windows(2)
        .any(|w| w[0] == b'%' && w[1].is_ascii_alphabetic())
}

/// Resolves a program name like `execvp` would: absolute/relative paths as-is, bare
/// names through `path`. Returns the executable file.
pub fn find_program(program: &str, path: &[PathBuf]) -> Option<PathBuf> {
    let is_exec = |p: &Path| {
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if program.contains('/') {
        let p = PathBuf::from(program);
        return is_exec(&p).then_some(p);
    }
    path.iter().map(|d| d.join(program)).find(|p| is_exec(p))
}

/// Icon theme lookup (theme → hicolor → pixmaps) or an absolute path as written.
fn resolve_icon(icon: &str, theme: &str) -> Option<PathBuf> {
    if icon.is_empty() {
        return None;
    }
    let p = Path::new(icon);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    // An icon name never has an extension (spec); tolerate entries that add one.
    let name = match p.extension().and_then(OsStr::to_str) {
        Some("png" | "svg" | "xpm") => p.file_stem()?.to_str()?,
        _ => icon,
    };
    freedesktop_icons::lookup(name)
        .with_theme(theme)
        .with_size(ICON_SIZE)
        .with_cache()
        .find()
        .filter(|p| p.extension().is_some_and(|e| e != "xpm")) // Qt Quick needs a plugin for XPM
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn tempdir() -> Dir {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "pocket-platform-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    /// Layout: <root>/high/applications, <root>/low/applications, <root>/bin with
    /// executables `app`, `other`.
    struct Fixture {
        root: Dir,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempdir();
            for d in ["high/applications", "low/applications", "bin"] {
                fs::create_dir_all(root.0.join(d)).unwrap();
            }
            let f = Fixture { root };
            f.program("app");
            f.program("other");
            f
        }
        fn program(&self, name: &str) {
            let p = self.root.0.join("bin").join(name);
            fs::write(&p, "#!/bin/sh\n").unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }
        fn entry(&self, dir: &str, file: &str, body: &str) {
            let p = self.root.0.join(dir).join("applications").join(file);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
        fn opts(&self) -> ScanOptions {
            ScanOptions {
                dirs: vec![
                    self.root.0.join("high/applications"),
                    self.root.0.join("low/applications"),
                ],
                desktops: vec!["pocket".into()],
                locales: vec!["de".into()],
                path: vec![self.root.0.join("bin")],
                resolve_icons: false,
                icon_theme: "hicolor".into(),
            }
        }
        fn scan(&self) -> (Catalog, Vec<(PathBuf, Skip)>) {
            Catalog::scan(&self.opts())
        }
        fn skip_of(&self, file: &str) -> Option<Skip> {
            self.scan()
                .1
                .into_iter()
                .find(|(p, _)| p.ends_with(file))
                .map(|(_, s)| s)
        }
    }

    fn app(name: &str, extra: &str) -> String {
        format!("[Desktop Entry]\nType=Application\nName={name}\nExec=app\n{extra}\n")
    }

    #[test]
    fn reads_fields_with_locale_and_sorts_by_name() {
        let f = Fixture::new();
        f.entry(
            "low",
            "org.example.Zeta.desktop",
            "[Desktop Entry]\nType=Application\nName=Zeta\nName[de]=Zett\nGenericName=Thing\n\
             Comment=Does things\nIcon=zeta\nCategories=Utility;GTK;\nKeywords=a;b;\n\
             StartupWMClass=zeta-app\nExec=app --flag \"two words\"\nPath=/tmp\n",
        );
        f.entry("low", "alpha.desktop", &app("alpha", ""));
        let (cat, _) = f.scan();
        let names: Vec<_> = cat.apps().iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["alpha", "Zett"]);
        let z = cat.get("org.example.Zeta").unwrap();
        assert_eq!(z.generic_name, "Thing");
        assert_eq!(z.comment, "Does things");
        assert_eq!(z.icon_name, "zeta");
        assert_eq!(z.categories, ["Utility", "GTK"]);
        assert_eq!(z.keywords, ["a", "b"]);
        assert_eq!(z.argv, ["app", "--flag", "two words"]);
        assert_eq!(z.working_dir.as_deref(), Some(Path::new("/tmp")));
        assert!(z.matches_app_id("ZETA-APP"));
        assert!(z.matches_app_id("org.example.zeta"));
        assert!(!z.matches_app_id("zeta"));
    }

    #[test]
    fn first_directory_wins_for_the_same_id() {
        let f = Fixture::new();
        f.entry("high", "dup.desktop", &app("High", ""));
        f.entry("low", "dup.desktop", &app("Low", ""));
        let (cat, skipped) = f.scan();
        assert_eq!(cat.len(), 1);
        assert_eq!(cat.get("dup").unwrap().name, "High");
        assert!(skipped.iter().any(|(_, s)| *s == Skip::Duplicate));
    }

    #[test]
    fn a_hidden_override_masks_the_lower_entry() {
        let f = Fixture::new();
        f.entry("high", "masked.desktop", &app("Masked", "Hidden=true"));
        f.entry("low", "masked.desktop", &app("Masked", ""));
        assert!(f.scan().0.is_empty());
    }

    #[test]
    fn subdirectories_form_prefixed_ids() {
        let f = Fixture::new();
        f.entry("low", "vendor/tool.desktop", &app("Tool", ""));
        assert!(f.scan().0.get("vendor-tool").is_some());
    }

    #[test]
    fn visibility_rules() {
        let f = Fixture::new();
        f.entry("low", "nodisplay.desktop", &app("a", "NoDisplay=true"));
        f.entry("low", "hidden.desktop", &app("a", "Hidden=true"));
        f.entry(
            "low",
            "link.desktop",
            "[Desktop Entry]\nType=Link\nName=l\nURL=x\n",
        );
        f.entry("low", "only.desktop", &app("a", "OnlyShowIn=GNOME;KDE;"));
        f.entry(
            "low",
            "onlyus.desktop",
            &app("Only us", "OnlyShowIn=GNOME;Pocket;"),
        );
        f.entry("low", "not.desktop", &app("a", "NotShowIn=Pocket;"));
        f.entry(
            "low",
            "tryexec.desktop",
            &app("a", "TryExec=missing-binary"),
        );
        f.entry("low", "tryok.desktop", &app("Try ok", "TryExec=other"));
        f.entry("low", "term.desktop", &app("a", "Terminal=true"));
        f.entry(
            "low",
            "noprog.desktop",
            "[Desktop Entry]\nType=Application\nName=n\nExec=not-installed\n",
        );
        assert_eq!(f.skip_of("nodisplay.desktop"), Some(Skip::NoDisplay));
        assert_eq!(f.skip_of("hidden.desktop"), Some(Skip::Hidden));
        assert_eq!(f.skip_of("link.desktop"), Some(Skip::NotAnApplication));
        assert_eq!(f.skip_of("only.desktop"), Some(Skip::NotShownIn));
        assert_eq!(f.skip_of("not.desktop"), Some(Skip::NotShownIn));
        assert_eq!(
            f.skip_of("tryexec.desktop"),
            Some(Skip::TryExecMissing("missing-binary".into()))
        );
        assert_eq!(f.skip_of("term.desktop"), Some(Skip::Terminal));
        assert_eq!(
            f.skip_of("noprog.desktop"),
            Some(Skip::ProgramMissing("not-installed".into()))
        );
        let (cat, _) = f.scan();
        let ids: Vec<_> = cat.apps().iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["onlyus", "tryok"]);
    }

    #[test]
    fn exec_field_codes_and_quoting() {
        let f = Fixture::new();
        let exec = |name: &str, exec: &str| {
            f.entry(
                "low",
                &format!("{name}.desktop"),
                &format!("[Desktop Entry]\nType=Application\nName={name}\nIcon=ic\nExec={exec}\n"),
            )
        };
        exec("files", "app %U --new-window %f");
        exec("named", "app --class %c");
        exec("icon", "app %i -x");
        // Key-file string escapes apply first: `\\"` in the file is `\"` in the value.
        exec("quoted", r#"app "a \\"q\\" \\$HOME" 'single'"#);
        exec("badescape", r#"app "a \"q\"""#);
        exec("shell", "app; rm -rf ~");
        exec("deprecated", "app %d");
        exec("embedded", "app --file=%f");
        exec("unterminated", r#"app "open"#);
        exec("env", "FOO=1 app");
        let (cat, _) = f.scan();
        let argv = |id: &str| cat.get(id).map(|a| a.argv.clone());
        assert_eq!(argv("files").unwrap(), ["app", "--new-window"]);
        assert_eq!(argv("named").unwrap(), ["app", "--class", "named"]);
        assert_eq!(argv("icon").unwrap(), ["app", "-x"]);
        assert_eq!(
            argv("quoted").unwrap(),
            ["app", r#"a "q" $HOME"#, "'single'"]
        );
        // No shell: ';' is part of an argument, the program `app;` does not exist.
        assert_eq!(
            f.skip_of("shell.desktop"),
            Some(Skip::ProgramMissing("app;".into()))
        );
        assert!(matches!(
            f.skip_of("deprecated.desktop"),
            Some(Skip::BadExec(_))
        ));
        assert!(matches!(
            f.skip_of("embedded.desktop"),
            Some(Skip::BadExec(_))
        ));
        assert!(matches!(
            f.skip_of("unterminated.desktop"),
            Some(Skip::BadExec(_))
        ));
        assert!(matches!(f.skip_of("env.desktop"), Some(Skip::BadExec(_))));
        assert!(matches!(
            f.skip_of("badescape.desktop"),
            Some(Skip::Unreadable(_))
        ));
    }

    #[test]
    fn command_has_no_shell_and_runs() {
        let f = Fixture::new();
        let marker = f.root.0.join("ran");
        let script = f.root.0.join("bin/writer");
        fs::write(
            &script,
            format!("#!/bin/sh\necho \"$@\" > '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        f.entry(
            "low",
            "writer.desktop",
            &format!(
                "[Desktop Entry]\nType=Application\nName=w\nExec={} \"$(id)\" x\n",
                script.display()
            ),
        );
        let (cat, _) = f.scan();
        let status = cat.get("writer").unwrap().command().status().unwrap();
        assert!(status.success());
        assert_eq!(fs::read_to_string(marker).unwrap(), "$(id) x\n");
    }

    #[test]
    fn open_command_expands_targets_through_the_entry() {
        let f = Fixture::new();
        let marker = f.root.0.join("opened");
        let script = f.root.0.join("bin/opener");
        fs::write(
            &script,
            format!("#!/bin/sh\necho \"$@\" > '{}'\n", marker.display()),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        f.entry(
            "low",
            "opener.desktop",
            &format!(
                "[Desktop Entry]\nType=Application\nName=o\nExec={} --new %F\n",
                script.display()
            ),
        );
        f.entry(
            "low",
            "plain.desktop",
            &format!(
                "[Desktop Entry]\nType=Application\nName=p\nExec={}\n",
                script.display()
            ),
        );
        let (cat, _) = f.scan();
        let app = cat.get("opener").unwrap();
        // The launcher command still opens nothing.
        assert!(!app.argv.iter().any(|a| a.contains("%")));
        let status = app
            .open_command(&["/tmp/my project"], &[])
            .unwrap()
            .status()
            .unwrap();
        assert!(status.success());
        assert_eq!(
            fs::read_to_string(&marker).unwrap(),
            "--new /tmp/my project\n"
        );
        assert!(
            cat.get("plain")
                .unwrap()
                .open_command(&["/tmp"], &[])
                .is_err()
        );
    }

    #[test]
    fn find_program_requires_an_executable_file() {
        let f = Fixture::new();
        let bin = f.root.0.join("bin");
        fs::write(bin.join("plain"), "").unwrap();
        assert!(find_program("app", std::slice::from_ref(&bin)).is_some());
        assert!(find_program("plain", std::slice::from_ref(&bin)).is_none());
        assert!(find_program("bin", std::slice::from_ref(&f.root.0)).is_none()); // a directory
        let abs = bin.join("app");
        assert_eq!(find_program(abs.to_str().unwrap(), &[]), Some(abs));
    }
}
