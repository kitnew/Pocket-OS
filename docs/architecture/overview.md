# Architecture overview

Status: **baseline (Stage 0)**. Items marked *planned* do not exist yet.

## Layers

```
┌───────────────────────────────────────────────────────────────┐
│ L4  Pocket UI         Qt 6 / Qt Quick / QML on Quickshell      │  apps/shell
│                       surfaces · features · components · theme │
├──────────────────────────── IPC (D-Bus, versioned) ───────────┤
│ L3  Pocket Core       Rust. State, modes, actions, config,     │  crates/pocket-core
│                       app management; adapters to the system   │  crates/pocket-ipc
│                                                                │  pocketd (planned)
├───────────────────────────────────────────────────────────────┤
│ L2  Graphics & system Phoc (Wayland compositor, wlroots)       │
│                       D-Bus, elogind, NetworkManager, BlueZ,   │
│                       UPower, PipeWire …                       │
├───────────────────────────────────────────────────────────────┤
│ L1  OS                postmarketOS edge / Alpine, OpenRC,      │
│                       Linux 7.2 sdm660, DRM/KMS (msm), Mesa    │
└───────────────────────────────────────────────────────────────┘
```

## Components and responsibilities

| Component | Responsibility | Not responsible for |
|---|---|---|
| **Phoc** | Output/mode setting, rendering & composition, input routing (touch/keys), window stacking, layer-shell, output transform/scale, idle protocol | Shell UI, policy, system state |
| **Pocket Shell** (`apps/shell`) | All shell UI: home, status bar, quick settings, launcher, notifications UI, Dock Mode layout (from Stage 1+) | Talking to system services directly; running shell commands for system control |
| **Pocket Core** (`crates/pocket-core`, `pocketd` *planned*) | Authoritative state (mode, settings, app registry…), command handling, policy, config | Rendering; anything Qt/Phoc specific |
| **Platform adapters** (`pocket-platform` *planned*) | Translate core intents to system services (NetworkManager, UPower, BlueZ, elogind, PipeWire, wlr-output-management…) and system events back into core events | Policy decisions |
| **IPC contract** (`crates/pocket-ipc`) | Versioned types shared by core and clients | Transport details |
| **pocket-session** (OpenRC service + `scripts/device/pocket-session`) | Opens an elogind seat0 session on a VT, starts Phoc, which starts the shell | Autostart (deliberately manual in Stage 0) |

## Process boundaries

```
OpenRC: pocket-session (root, manual start)
  └─ openvt -c 7 ─ su -l <user>          ← PAM: pam_elogind → session cN, seat0, vt7
       └─ dbus-run-session                ← per-session D-Bus bus
            └─ phoc -E pocket-shell       ← DRM/input via libseat → logind
                 └─ quickshell (Pocket Shell)   Wayland client, layer-shell surfaces
pocketd (planned): user service on the session bus; shell talks to it over D-Bus
```

- The shell and the core are **separate processes**. A shell crash must not lose core
  state; a core restart must be survivable by the shell (it re-reads state on reconnect).
- Phoc is the session leader's child: if the shell (`-E` client) exits, Phoc exits and
  the session ends. Stage 1 can decouple this (supervised shell restart) if needed.

## Core/UI separation rules

1. `pocket-core` and `pocket-ipc` have no dependency on Qt, QML, Quickshell or Phoc
   (enforced by their `Cargo.toml`; reviewed on every dependency change).
2. QML never spawns processes for system control. System actions become IPC commands to
   the core, which uses an adapter.
3. Quickshell-specific code is confined to `apps/shell/shell.qml` and `apps/shell/surfaces/`.
   `components/`, `features/` and `theme/` are plain Qt Quick, so they survive a runtime
   change (e.g. to a standalone Qt Quick app with layer-shell-qt).

## Wayland compositor responsibilities

Pocket relies on Phoc for everything it already does well and uses standard protocols to
reach it. Verified on the device (see `docs/validation/stage0.md`):
`zwlr_layer_shell_v1` (shell surfaces), `wp_fractional_scale_manager_v1`,
`zwlr_output_manager_v1` (rotation/scale), `zwlr_foreign_toplevel_manager_v1` and
`ext_foreign_toplevel_list_v1` (task switching), `ext_idle_notifier_v1`,
`zwp_input_method_manager_v2` / `zwp_text_input_manager_v3` (on-screen keyboard),
`zwlr_screencopy_manager_v1`. Phoc also exposes `phosh_private`; Pocket does **not** use it
(private interface), unless a needed capability has no standard equivalent — such a case
requires an ADR.

## IPC direction

- Transport: **D-Bus session bus** (well-known name `org.pocketos.Core1`, *planned*).
  Reasons: already running per session, introspectable, first-class in Qt (QtDBus) and
  Rust (`zbus`), consistent with the services we integrate with.
- Contract: types in `pocket-ipc`; a `ProtocolVersion` is negotiated before use. Major
  bump on incompatible change. Interfaces are versioned in their name (`…Core1`).
- Pattern: clients send **commands**; the core emits **events/property changes**; clients
  mirror state and never assume a command succeeded until the event arrives.

## Platform adapter strategy

One adapter per external service, behind a Rust trait owned by the core, so the core can
be tested with fakes and so a service can be replaced (e.g. NetworkManager → iwd) without
touching UI. Adapters are added only when a feature needs them (none exist in Stage 0).

## Extension points (deliberately small)

- New UI features: a directory under `apps/shell/features/` using shared `components/`.
- New system integration: a new adapter implementing a core trait.
- New layout/mode: a surface composition in `apps/shell/surfaces/` selecting features.
No plugin loading, marketplace or public SDK is planned at this stage.

## Error handling and recovery

- Fail visible, not silent: diagnostics go to the session log with a stable prefix
  (`pocket.shell.diag: event=…`), retrievable over SSH (`scripts/logs.sh`).
- The graphical session is never in a boot runlevel during development. Recovery is
  always `rc-service pocket-session stop` (or `scripts/session.sh stop`), which terminates
  the logind session and returns to the tty1 console. SSH (USB network) is independent of
  the graphical stack.
- Core (planned): commands are validated and either applied atomically or rejected with
  an error reply; adapters report failures as events instead of panicking.

## Handheld and Dock modes

Both modes share Phoc, the core, the IPC contract, and all components/features. `Mode`
(`handheld` | `dock`) lives in the core (`pocket_ipc::Mode`, `pocket_core::CoreState`);
the shell selects a surface composition per mode. Orientation changes are an output
transform in Phoc (wlr-output-management); layouts respond to size, not to a hard-coded
orientation — the Stage 0 panel already re-flows from one column (portrait) to two
(landscape).
