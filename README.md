# Pocket OS

Pocket OS is a modular, Linux-native mobile shell: its own home screen, launcher, status
bar, quick settings and notifications, plus an optional landscape **Dock Mode** that turns
the phone into a configurable desktop control surface. It runs on top of an ordinary
postmarketOS/Alpine system and standard Linux services. It is not a new distribution and
does not include a custom compositor.

**Status: Stage 1 (core foundation & runtime) complete.** A Rust core daemon (`pocketd`)
owns shell state on the D-Bus session bus (`org.pocketos.Core1`). The Quickshell UI
mirrors it through a small QtDBus QML module and switches the logical mode with a
touchscreen button. Shell, core and compositor restart independently under a supervised
session. No user-facing features (home, launcher, settings, notifications, Dock Mode UI)
exist yet. Evidence: [`docs/validation/stage1.md`](docs/validation/stage1.md)
([Stage 0](docs/validation/stage0.md)).

## Target device

| | |
|---|---|
| Device | Xiaomi Redmi Note 7 (`xiaomi-lavender`, Tianma panel), SDM660, 4 GB RAM |
| OS | postmarketOS edge ("Nura edge"), Alpine edge, aarch64, OpenRC, elogind |
| Kernel | 7.2.15-sdm660 (mainline-based) |
| Graphics | msm DPU KMS, Mesa 26.3.1 freedreno, Adreno 512, OpenGL ES 3.1 (hardware) |

Details: [`docs/hardware/lavender.md`](docs/hardware/lavender.md).

## Architecture

```
Pocket Shell (QML on Quickshell) ── Pocket.Core (QtDBus) ──D-Bus session bus──  pocketd (Rust, zbus)
            │ Wayland (layer-shell)                         org.pocketos.Core1   └ pocket-core (state, modes)
          Phoc (wlroots compositor)                                              adapters (planned)
            │ DRM/KMS, libinput via logind
   postmarketOS / Alpine, OpenRC, elogind, Linux
```

- **Pocket Core** (Rust) owns state and policy and never depends on Qt, QML, Quickshell
  or Phoc. Clients send requests and mirror state from `StateChanged` signals.
- **Pocket Shell** (Qt Quick) is only UI. It does not run shell commands for system
  control. System actions go through the core.
- **Phoc** does composition, input, output transforms and scaling through standard
  Wayland protocols.
- **Session**: `pocket-session` (OpenRC, manual) → private bus → session root running
  `pocketd` and Phoc under small supervisors with restart limits.

Read [`docs/architecture/overview.md`](docs/architecture/overview.md) and the decisions:
[ADR-0001 stack](docs/decisions/0001-initial-technology-stack.md),
[ADR-0002 core & IPC](docs/decisions/0002-pocket-core-and-ipc.md),
[ADR-0003 session lifecycle](docs/decisions/0003-graphical-session-lifecycle.md).

## Repository layout

| Path | Contents |
|---|---|
| `apps/shell/` | QML shell. `shell.qml` and `surfaces/` are specific to Quickshell. `components/`, `features/` and `theme/` are plain Qt Quick |
| `bridge/pocket-core-qml/` | `Pocket.Core` QML module: `CoreClient`, a QtDBus client of the core (+ integration test) |
| `crates/pocket-ipc/` | IPC contract: D-Bus names, protocol version, `Mode`, state keys |
| `crates/pocket-core/` | Framework-independent state manager (modes, validation, events, revision) |
| `crates/pocketd/` | Core daemon on the session bus (zbus) + black-box D-Bus contract test |
| `config/phoc.ini` | Phoc configuration for the device (scale 3) |
| `packaging/` | OpenRC `pocket-session` service (manual), elogind power-key drop-in, scoped sudoers rule |
| `scripts/` | Host dev tools: build, deploy, session control, logs, tests, checks, Quickshell build, device proxy |
| `scripts/device/` | Session root, supervisor, `pocket-ctl`, shell launcher (installed to `~/pocket-os/bin`) |
| `tests/device/` | On-device tests: session lifecycle e2e, Stage 0 touch scene |
| `docs/` | Architecture, decisions (ADRs), hardware, development, validation |

## Requirements

- Host: Linux with `ssh`, `git`, `python3`, `curl`, `dbus-daemon`, and Rust stable
  (`rustup`). Tested on Fedora.
- Device: postmarketOS edge on xiaomi-lavender, reachable by key-based `ssh lavender`
  over USB networking.
- Device packages and a one-time Quickshell build. See
  [`docs/development/README.md`](docs/development/README.md).

## Build and run

```sh
scripts/check.sh                 # host: fmt, clippy, unit + D-Bus contract tests, shellcheck
scripts/quickshell-build.sh      # once, and after every Qt upgrade on the device
scripts/build.sh                 # pocketd (host cross-build) + QML bridge (built on device)
scripts/deploy.sh --system       # first time: service, power-key policy, sudoers rule
scripts/session.sh start         # Phoc + pocketd + Pocket Shell on tty7
scripts/session.sh shell restart # restart only the shell (core: scripts/session.sh core restart)
scripts/e2e.sh                   # lifecycle e2e on the running session
scripts/logs.sh                  # fetch logs, print events
scripts/session.sh stop          # back to the tty1 console
```

## Known limitations

- Quickshell is not packaged for Alpine. It is built from source on the device and must
  be rebuilt after every Qt upgrade (it uses private Qt APIs).
- The device has no internet over USB. Package installs go through the host proxy
  (`scripts/device-online.sh`).
- Core state is in memory. A core restart resets the mode to `handheld`.
- A Phoc crash ends the session. Nothing starts at boot (by design during development).
- The power key short press is ignored (development policy). There is no lock screen.
- Physical auto-rotation is not implemented. Landscape works through a manual output
  transform.

## License

MIT. See [LICENSE](LICENSE).
