# Architecture overview

Status: **Stage 2.5 (implemented)**: the Stage 2 mobile shell (home, launcher, window
switching, status bar, quick settings, on-screen keyboard, rotation, display power), plus
a supervised PipeWire audio stack with volume and mute, a usable landscape keyboard,
boot-image safety for package operations, and a soak-tested memory profile. Items marked
*planned* do not exist yet.
Decisions: [ADR-0001](../decisions/0001-initial-technology-stack.md) (stack),
[ADR-0002](../decisions/0002-pocket-core-and-ipc.md) (core + IPC),
[ADR-0003](../decisions/0003-graphical-session-lifecycle.md) (session lifecycle),
[ADR-0004](../decisions/0004-shell-ui-architecture.md) (shell UI),
[ADR-0005](../decisions/0005-applications-and-window-management.md) (apps, windows,
compositor capabilities), [ADR-0006](../decisions/0006-secure-lock-screen.md) (secure
lock screen: investigation, deferred). Audio: [`development/audio.md`](../development/audio.md).

## Layers

```
┌────────────────────────────────────────────────────────────────────────┐
│ L4  Pocket Shell   QML on Quickshell                       apps/shell   │
│       surfaces/ services/   (Quickshell-specific; services/AudioService │
│                             = Quickshell.Services.Pipewire)             │
│       state/Shell  features/  components/  theme/   (portable Qt Quick)  │
│     Pocket.Core      CoreClient, AppCatalog (QtDBus → pocketd)          │
│     Pocket.Platform  PowerKey, DisplayPower, OutputConfig (Wayland),    │
│                      Brightness, OrientationSensor, OnScreenKeyboard    │
├──────────── D-Bus session bus: org.pocketos.Core1 (+ /Apps) ───────────┤
│ L3  Pocket Core    pocketd (Rust, zbus)                crates/pocketd    │
│                    pocket-core: mode state machine     crates/pocket-core│
│                    pocket-platform: app catalog/launch crates/pocket-platform│
│                    pocket-ipc: contract                crates/pocket-ipc │
├────────────────────────────────────────────────────────────────────────┤
│ L2  Compositor & services  Phoc 0.59 · squeekboard · UPower ·           │
│     NetworkManager · BlueZ · iio-sensor-proxy · elogind · polkit ·      │
│     PipeWire 1.6 + WirePlumber 0.5 + pipewire-pulse (session)           │
├────────────────────────────────────────────────────────────────────────┤
│ L1  OS   postmarketOS edge / Alpine, OpenRC, Linux 7.2 sdm660, Mesa     │
└────────────────────────────────────────────────────────────────────────┘
```

## Components and responsibilities

| Component | Responsibility | Not responsible for |
|---|---|---|
| **Phoc** | Composition, input, layer-shell stacking, window activation/close, output power/transform/scale, input-method relay | Shell UI, policy |
| **Pocket Shell** (`apps/shell`) | All UI and UI-owned state (navigation, open panel, rotation mode, toasts) | Authoritative state; spawning processes; shell commands |
| **Pocket.Core** (`bridge/pocket-core-qml`) | `CoreClient` (core state, mode, reconnection); `AppCatalog`/`AppFilter` (catalog mirror, launch, exit reports) over the same connection | Logic |
| **Pocket.Platform** (`bridge/pocket-platform-qml`) | Shell-side adapters without a Quickshell integration: power-key grab (phosh_private), display power (wlr-output-power), rotation (wlr-output-management), backlight (logind), accelerometer (iio-sensor-proxy), OSK visibility (`sm.puri.OSK0`), sound-card presence | Policy (it lives in `state/Shell.qml`) |
| **pocketd** (`crates/pocketd`) | D-Bus service: Core1 (mode) and Core1/Apps (catalog, launch, exit signals, inotify rescans) | Logic (delegated to the crates below) |
| **pocket-core** | `StateManager`: mode, validation, events, revision. No I/O | Transport, system services |
| **pocket-platform** | Desktop Entry discovery, visibility rules, safe Exec expansion, icon lookup, launch command | D-Bus, Qt |
| **pocket-ipc** | Contract: names, protocol 1.1, keys | — |
| **Session layer** | Seat session, private bus, supervision of core/shell/osk/audio | Policy inside components |
| **Audio stack** (PipeWire, WirePlumber, pipewire-pulse) | Devices, routing, default sink, volumes and their persistence, Pulse API for apps | Pocket-specific state (none is kept in Core) |

## Process topology (one graphical session)

```
rc-service pocket-session (root, manual; not in any runlevel)
└─ openvt -c 7 ─ su -l <user>                  PAM/elogind: seat0, tty7 (active, local)
   └─ pocket-session                           log: ~/.local/state/pocket-os/session.log
      └─ dbus-run-session                      private session bus
         └─ pocket-session-run                 session root (lives as long as Phoc)
            ├─ pocket-supervise core ── pocketd           owns org.pocketos.Core1
            │                             └─ launched apps (own process groups)
            ├─ pocket-supervise audio ── pocket-audio   (unit: pipewire, wireplumber, pipewire-pulse)
            ├─ phoc -E ── pocket-supervise shell ── quickshell (Pocket Shell)
            └─ pocket-supervise osk ── squeekboard        (after Phoc's socket exists)
```

Core, shell, keyboard and audio restart independently (audio restarts as one unit). Applications are children of `pocketd`
in their own process groups, so they survive core and shell restarts. They end with the
logind session.

## Surfaces (layer-shell) and navigation

| Surface | Layer | Mapped |
|---|---|---|
| Status bar (time, Core, Dock, Bluetooth, Wi-Fi, battery) | top, exclusive zone | always, except while the OSK is shown in landscape |
| Navigation bar (Keyboard · Home · Open windows) | overlay, exclusive zone (always owns the bottom edge; ADR-0004) | always |
| Home: home page / launcher / switcher | top, between the bars | when requested or no windows exist |
| squeekboard | top (own client) | while a text field has focus or when toggled |
| Developer panel, Quick Settings, toasts | overlay | while open |

Phoc cannot minimise, so "Home" means showing the Home surface above the windows. Tapping
an app activates its existing window (matched by app_id = desktop ID or
`StartupWMClass`) or launches it through Core. Details:
[ADR-0004](../decisions/0004-shell-ui-architecture.md),
[ADR-0005](../decisions/0005-applications-and-window-management.md).

## State ownership

| Owner | State |
|---|---|
| Pocket Core | mode (`handheld`/`dock`), application catalog, launches |
| Compositor | windows, activation, output power, transform |
| System services | battery, network, Bluetooth, backlight, accelerometer, OSK visibility, audio devices/volume/mute (PipeWire/WirePlumber) |
| Shell (UI) | home visibility/page, open panel, rotation mode, toasts |

Nothing owned by the system or the compositor is mirrored into Core. UI state is
transient (reset by a shell restart).

## UI/Core separation and boundaries

1. `pocket-core`, `pocket-ipc` and `pocket-platform` depend on nothing UI- or
   transport-specific. `pocketd` is the only Rust code that knows D-Bus.
2. QML reaches Core only through `Pocket.Core`, and system/compositor interfaces only
   through Quickshell services (`services/`) or `Pocket.Platform`. No QML runs a process.
3. Quickshell-specific code is limited to `shell.qml`, `surfaces/` and `services/`.
   `state/`, `features/`, `components/` and `theme/` are plain Qt Quick.
4. Requests → validation → state change → signals. Clients never assume success: the
   mode label follows `StateChanged`, launches are confirmed by `Launch` replies,
   `AppExited` and the compositor's toplevel list.

Contract summary (ADR-0002): `org.pocketos.Core1` (`GetVersion`, `GetState`, `GetMode`,
`RequestMode`, `ModeChanged`, `StateChanged`) and, since 1.1, `org.pocketos.Core1.Apps`
on `/org/pocketos/Core1/Apps` (`ListApps`, `Launch`, `Reload`, `AppsChanged`,
`AppExited`). Errors: `org.pocketos.Core1.Error.{InvalidMode,NotReady,UnknownApp,LaunchFailed}`.

## Display, power key, rotation, security

- Power key short press: the shell grabs `XF86PowerOff` (phosh_private) and toggles
  wlr-output-power. Phoc ignores touch while the panel is off. elogind keeps
  `HandlePowerKey=ignore`, so a press never powers the phone off.
- Rotation: Portrait / Landscape / Auto (iio-sensor-proxy) → wlr-output-management
  transform. Phoc remaps touch.
- **There is no lock screen.** Phoc 0.59 (and Phoc `main`) lacks `ext-session-lock-v1`.
  Phosh's own lock is an overlay surface without a compositor guarantee. Display-off is
  power management, not security. Strategy and follow-up work: ADR-0006.

## Error handling and recovery

- Supervisors restart core/shell/osk/audio with growing delays and give up after 5 exits in
  60 s (`pocket-ctl start <name>` resumes).
- The shell survives Core loss: the catalog stays visible (marked stale), launching
  reports "Pocket Core is not connected", and everything resynchronises on a new owner.
- Launch failures (unknown app, spawn error, early non-zero exit, no window within 20 s)
  become toasts. Diagnostics are `pocket.shell.diag: event=…` lines in `session.log`.

## Extension points

- New UI: a feature view in `features/` + a surface in `surfaces/` (or a page in Home).
  Shared state and actions go in `state/Shell.qml`.
- New system integration: prefer a Quickshell service adapter in `services/`. If none
  exists, add a `Pocket.Platform` type (shell-side, compositor/D-Bus) or a
  `pocket-platform` adapter served by `pocketd` (authoritative actions).
- New Core domain: a new object under `/org/pocketos/Core1/…` (minor protocol bump).

There is no persistence layer, plugin loading or second daemon. Each would need an ADR.
