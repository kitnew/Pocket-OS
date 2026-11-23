# ADR-0002: Pocket Core daemon and IPC

- Status: **Accepted** (implemented and validated in Stage 1)
- Date: 2026-11-23
- Evidence: `docs/validation/stage1.md`

## Context

Stage 1 needs a real core process that owns shell state, and a real two-way link between
it and the QML shell running on Quickshell. ADR-0001 chose Rust for the core and the
D-Bus session bus as the transport. This ADR fixes the daemon design, the wire contract,
and how QML talks to it.

## Decision

### Daemon: `pocketd` (`crates/pocketd`)

- One Rust process, single-threaded Tokio runtime, **zbus 6.5** (pure Rust, no libdbus).
- `pocket-core` holds all logic (`StateManager`: commands in → validated → events out; no
  I/O). `pocketd/src/service.rs` only translates D-Bus ⇄ core. `pocket-ipc` holds the
  transport-free contract (names, version, `Mode` wire names, state keys).
- Lifecycle: connect → export object → request the name with `DO_NOT_QUEUE` (a second
  instance exits with status **3**) → mark ready (emits `StateChanged`) → serve until
  SIGTERM/SIGINT/SIGHUP, loss of the name, or bus disconnect → release the name → exit 0.
- State is in memory. A restarted core starts from the initial state; clients detect the
  restart through the bus-name owner change and re-read the state.
- Logging: `tracing` to stderr (collected in the session log), level via `POCKETD_LOG`.
- Build: cross-compiled on the host to a static `aarch64-unknown-linux-musl` binary with
  Rust's bundled `rust-lld` (`.cargo/config.toml`). No toolchain on the phone, ~1 min
  clean build, seconds incremental. Native builds on the phone were rejected as slower
  and needing ~1 GB of Rust packages; a C cross toolchain is unnecessary because every
  dependency is pure Rust.

### Contract: `org.pocketos.Core1` (session bus)

| | |
|---|---|
| Bus name | `org.pocketos.Core1` |
| Object path | `/org/pocketos/Core1` |
| Interface | `org.pocketos.Core1` |
| Protocol version | `1.1` (major is in the names; minor bumps only add). 1.0 = Stage 1; 1.1 adds the Apps object below |

| Member | Signature | Semantics |
|---|---|---|
| `GetVersion()` | `→ (s version, u protocol_major, u protocol_minor)` | Core build version and contract version |
| `GetState()` | `→ a{sv}` | Full state, see keys below |
| `GetMode()` | `→ s` | `"handheld"` or `"dock"` |
| `RequestMode(s mode)` | `→ ()` | Validates and applies. Same mode = success, no signals. Errors below |
| signal `ModeChanged` | `(s mode)` | New mode, after each real change |
| signal `StateChanged` | `(a{sv} state)` | Full new state, after every change (including becoming ready) |

State keys (`pocket_ipc::dbus::state_key`; clients ignore unknown keys):
`version` (s), `protocol` (s, `"1.1"`), `mode` (s), `ready` (b), `revision` (t — strictly
increasing per core process, restarts when the core restarts).

Errors: `org.pocketos.Core1.Error.InvalidMode` (unknown mode name; names are exact and
case-sensitive), `org.pocketos.Core1.Error.NotReady` (startup not complete).

### Since 1.1: `org.pocketos.Core1.Apps` on `/org/pocketos/Core1/Apps` (Stage 2)

Same bus name, its own object (the extension point foreseen above). Implementation:
`crates/pocket-platform` (catalog) + `crates/pocketd/src/apps.rs` (D-Bus). Decision
record: ADR-0005.

| Member | Signature | Semantics |
|---|---|---|
| `ListApps()` | `→ aa{sv}` | Visible installed applications sorted by name. Keys (`pocket_ipc::dbus::apps::app_key`): `id`, `name`, `generic_name`, `comment`, `icon_name`, `icon_path` (resolved file or empty), `categories` (as), `keywords` (as), `startup_wm_class` |
| `Launch(s id)` | `→ u pid` | Starts the app (D-Bus activation first for `DBusActivatable`, then pid 0). Errors `…Error.UnknownApp`, `…Error.LaunchFailed` |
| `Reload()` | `→ ()` | Rescans now. `AppsChanged` only if something changed |
| signal `AppsChanged` | `()` | Catalog changed (inotify on the application directories, debounced) |
| signal `AppExited` | `(s id, u pid, b success, s detail)` | A launched process ended (`status=N` / `signal=N`) |

The QML side is `AppCatalog` (list model) + `AppFilter` (search) in `Pocket.Core`. It
uses the `CoreClient`'s connection, so a shell keeps one logical core link.

Why this shape: `a{sv}` maps directly to `QVariantMap` in QtDBus and to a JS object in
QML, so adding a key later needs no client change; plain methods + signals avoid
`org.freedesktop.DBus.Properties` round-trips and keep the client small. The full-state
signal makes clients stateless mirrors: no diffing, no ordering assumptions beyond the
revision.

**Session bus** (not system bus): the core is per-user shell state, its lifetime is the
graphical session, and it needs no root or bus policy file. Each graphical session gets
its own bus from `dbus-run-session` (see ADR-0003).

### QML client: `Pocket.Core` module (`bridge/pocket-core-qml`)

A small C++ QML module (~330 lines) (`CoreClient`, `QML_ELEMENT`) on **QtDBus**, built natively on
the device with CMake against the device's Qt 6.12 (`scripts/build.sh bridge`, ~30 s)
and loaded through `QML_IMPORT_PATH`. Public Qt API only, so it does not need a rebuild
on Qt patch upgrades (unlike Quickshell).

- Properties: `status` (`Connecting`/`Connected`/`Disconnected`), `statusText`, `mode`,
  `coreVersion`, `protocol`, `ready`, `revision`, `owner`, `connections`, `lastError`,
  `requestPending`. Method `requestMode(mode)`; signal `requestFinished(mode, ok, error)`.
- Recovery without polling: `QDBusServiceWatcher` on the name; on a new owner the
  revision baseline is reset and `GetState` is re-read. While the core is absent the
  client only waits for the owner-change notification. Other failures (timeouts, no bus)
  retry with backoff 1 s → 30 s.
- It subscribes to `StateChanged` before reading state; stale replies (previous owner)
  and stale revisions are dropped. Incompatible protocol majors are refused.
- The UI never applies a mode locally: the label changes only when the core reports it.

## Alternatives considered

| Alternative | Why not |
|---|---|
| Quickshell built-in D-Bus | Quickshell 0.3.2 has internal D-Bus helpers for its own services (UPower, SNI, dbusmenu) but no generic QML D-Bus client |
| `Process` + `dbus-send`/`busctl`/`gdbus` from QML | Forbidden by the architecture (no shell-outs for system control); no signal subscription without a long-running monitor process; fragile parsing |
| Custom Unix socket / JSON protocol | Re-implements what D-Bus already gives (activation-free discovery, owner tracking, introspection, typed errors); every system service we integrate with is on D-Bus anyway |
| Varlink | Not available in Qt; no broadcast signals |
| D-Bus properties + `PropertiesChanged` | Works, but more round-trips and a heavier client; revisit if generic tooling needs it |
| Making the bridge a Quickshell plugin | Ties the bridge to Quickshell's private-API rebuild cycle; a plain QML module also works under `qmlscene`/a standalone Qt Quick host (ADR-0001 fallback) |
| D-Bus activation of `pocketd` | Activation only starts on a call; restart after a crash and give-up policy still need a supervisor, so it adds a second start path without removing one |
| zbus 5 / libdbus-sys | zbus 6.5 is current and pure Rust (static musl binary, no C toolchain) |

## Consequences

- The contract is defined in three places that must agree: `crates/pocket-ipc`
  (authoritative), `bridge/pocket-core-qml/coreclient.cpp` (constants), and this ADR. The
  black-box tests (`crates/pocketd/tests/dbus_contract.rs`,
  `bridge/pocket-core-qml/tests/tst_coreclient.cpp`) catch drift.
- The bridge must be built on the device (needs target Qt headers). It uses only public
  Qt API.
- Core state is lost on core restart (Stage 1 has nothing worth persisting; the mode
  resets to `handheld`). Persistence is a later, explicit decision.
- New features add methods/keys under `Core1` (minor bump) or a new interface/object
  (e.g. `/org/pocketos/Core1/Settings`); incompatible changes require `Core2`.
