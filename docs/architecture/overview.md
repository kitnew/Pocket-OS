# Architecture overview

Status: **Stage 1 (implemented)**. Items marked *planned* do not exist yet.
Decisions: [ADR-0001](../decisions/0001-initial-technology-stack.md) (stack),
[ADR-0002](../decisions/0002-pocket-core-and-ipc.md) (core + IPC),
[ADR-0003](../decisions/0003-graphical-session-lifecycle.md) (session lifecycle).

## Layers

```
┌───────────────────────────────────────────────────────────────┐
│ L4  Pocket UI         Qt 6 / Qt Quick / QML on Quickshell      │  apps/shell
│                       + Pocket.Core QML module (QtDBus client) │  bridge/pocket-core-qml
├──────────── D-Bus session bus: org.pocketos.Core1 ────────────┤
│ L3  Pocket Core       pocketd (Rust, zbus): state, modes,      │  crates/pocketd
│                       validation, events                       │  crates/pocket-core
│                                                                │  crates/pocket-ipc
├───────────────────────────────────────────────────────────────┤
│ L2  Graphics & system Phoc (Wayland compositor, wlroots)       │
│                       D-Bus, elogind, NetworkManager, UPower … │
├───────────────────────────────────────────────────────────────┤
│ L1  OS                postmarketOS edge / Alpine, OpenRC,      │
│                       Linux 7.2 sdm660, DRM/KMS (msm), Mesa    │
└───────────────────────────────────────────────────────────────┘
```

## Components and responsibilities

| Component | Responsibility | Not responsible for |
|---|---|---|
| **Phoc** | Output, composition, input routing, layer-shell, output transform/scale | Shell UI, policy, state |
| **Pocket Shell** (`apps/shell`) | All UI. Mirrors core state, sends requests | Owning state; talking to system services; shell commands for system control |
| **Pocket.Core bridge** (`bridge/pocket-core-qml`) | QtDBus client exposed to QML as `CoreClient`: connection status, state mirror, `requestMode`, reconnection | Any business logic |
| **pocketd** (`crates/pocketd`) | D-Bus service: name ownership, request handling, signals, logging, graceful shutdown | Logic (delegated to `pocket-core`) |
| **pocket-core** | `StateManager`: initial state, command validation, transitions, revision, typed errors. No I/O; unit-tested | Transport, Qt, system services |
| **pocket-ipc** | The contract: bus names, protocol version, `Mode` wire names, state keys | Transport library details |
| **Session layer** (`packaging/openrc`, `scripts/device/pocket-*`) | Seat session, private bus, process topology, supervision, restart limits, shutdown | Policy inside components |
| **pocket-platform** *planned* | Adapters to system services when a feature needs one | — |

## Process topology (one graphical session)

```
rc-service pocket-session (root, manual; not in any runlevel)
└─ openvt -c 7 ─ su -l <user>                  PAM/elogind: session cN, seat0, tty7
   └─ pocket-session                           log: ~/.local/state/pocket-os/session.log
      └─ dbus-run-session                      private session bus for this session
         └─ pocket-session-run                 session root (lives as long as Phoc)
            ├─ pocket-supervise core ── pocketd          owns org.pocketos.Core1
            └─ phoc -E ── pocket-supervise shell ── quickshell (Pocket Shell)
```

- Shell, core and compositor are separate processes with separate lifetimes. A shell
  crash keeps Phoc and the core alive. A core crash keeps the UI alive. A Phoc exit ends
  the session.
- Runtime control state: `$XDG_RUNTIME_DIR/pocket-os/` (`env` with the session's
  `DBUS_SESSION_BUS_ADDRESS`/`WAYLAND_DISPLAY`, pid files, hold/restart/failed markers).
  `pocket-ctl` (user, no root) restarts, stops or starts single components.

## UI/Core separation and D-Bus boundary

1. `pocket-core` and `pocket-ipc` depend on nothing UI- or transport-specific;
   `pocketd` is the only place that knows D-Bus on the Rust side.
2. QML reaches the core only through `CoreClient`. QML never spawns processes for system
   control.
3. Pattern: clients send **requests**; the core validates, applies atomically, and
   broadcasts **`ModeChanged` + `StateChanged`** (full `a{sv}` state with a revision).
   Clients never assume a request succeeded; the UI's mode label changes only on core
   state. All subscribers receive the same signals (D-Bus broadcast); there is no polling.
4. Quickshell-specific code stays in `apps/shell/shell.qml` and `apps/shell/surfaces/`.
   `components/`, `features/` and `theme/` are plain Qt Quick plus the `Pocket.Core`
   module, so they run under any Qt Quick host.

Contract summary (full table in ADR-0002): `GetVersion() → (s,u,u)`, `GetState() → a{sv}`,
`GetMode() → s`, `RequestMode(s)`, signals `ModeChanged(s)`, `StateChanged(a{sv})`, errors
`org.pocketos.Core1.Error.{InvalidMode,NotReady}`. State keys: `version`, `protocol`,
`mode`, `ready`, `revision`.

## Session lifecycle

| Action | Mechanism |
|---|---|
| Start | `scripts/session.sh start` → `rc-service pocket-session start` |
| Stop | `scripts/session.sh stop` → `loginctl terminate-session`. Supervisors stop children, the bus ends, VT → tty1 |
| Restart shell / core only | `scripts/session.sh shell|core restart` → `pocket-ctl restart …` |
| Hold a component stopped | `pocket-ctl stop core` (e.g. to test the UI's disconnected state), `pocket-ctl start core` |

## Error handling and recovery

- Components: the supervisor restarts after unrequested exits with a 1, 2, 3… s delay,
  and gives up after 5 exits within a sliding 60 s window. It keeps
  `~/.local/state/pocket-os/<name>-gave-up.log` and waits for `pocket-ctl start <name>`.
  No reboot is used as recovery.
- UI ↔ core: `CoreClient` shows Connecting/Connected/Disconnected. It follows the
  bus-name owner (no polling while the core is absent), re-reads state on every new
  owner, and backs off (1 s → 30 s) only for transport errors. Invalid commands return
  typed D-Bus errors that the UI logs.
- Core: rejected commands leave state untouched. `pocketd` exits non-zero on startup
  failure (3 = name already owned) and exits 0 after releasing its name on SIGTERM.
- Diagnostics use stable line prefixes in `session.log`: `pocket.session:`,
  `pocket.supervise:`, `pocketd`, `pocket.core.client`, `pocket.shell.diag: event=…`.
  Fetch them with `scripts/logs.sh`.
- The graphical session never starts at boot during development. SSH (USB network) is
  independent of it, and `scripts/session.sh stop` always returns to the tty1 console.
- Power key: a short press is ignored (elogind drop-in, development policy). A long
  hardware reset still works.

## Handheld and Dock modes

`Mode` (`handheld` | `dock`) is owned by the core. In Stage 1 it is a logical mode only:
the UI displays it and can request a switch. Selecting a surface composition per mode
(the real Dock Mode UI) is Stage 2+. Layouts respond to size, not to a fixed orientation.

## Extension points

- New core state: add a key to the state dictionary (protocol minor bump), and a
  command plus event in `pocket-core`.
- New domain (settings, notifications…): a new interface/object under
  `/org/pocketos/Core1/…` served by the same `pocketd`, mirrored by a new QML type in the
  bridge module.
- System integration: a `pocket-platform` adapter behind a core-owned trait, added only
  when a feature needs it. Values owned by system services (battery, network) are not
  mirrored into core state.
- New UI: a directory under `apps/shell/features/`, composed by `surfaces/`.

There is no plugin loading, persistence layer or second daemon. Each one needs an ADR.
