# Pocket OS

Pocket OS is a modular, Linux-native mobile shell: its own home screen, launcher, status
bar, quick settings and notifications, plus an optional landscape **Dock Mode** that turns
the phone into a configurable desktop control surface. It runs on top of an ordinary
postmarketOS/Alpine system and standard Linux services. It is not a new distribution and
does not include a custom compositor.

**Status: Stage 2.5 (stabilization & hardware integration) complete.** On the phone,
Pocket OS is a usable touchscreen shell (the session is started manually). It has:
- a Home screen and a launcher with search;
- real application launching through Pocket Core;
- window switching and closing;
- a status bar;
- Quick Settings: brightness, **volume and mute**, Wi-Fi, Bluetooth, rotation,
  keyboard, screen off;
- the squeekboard on-screen keyboard, with a compact landscape mode;
- portrait/landscape/auto rotation;
- power-key display off/on.

Core, shell, keyboard and the PipeWire audio stack restart independently. A 36-minute
soak showed no memory leak. Package operations follow a verified boot-partition backup
procedure. There is **no lock screen**: no secure mechanism exists in Phoc yet (ADR-0006).
Evidence: [`docs/validation/stage2.5.md`](docs/validation/stage2.5.md)
([Stage 2](docs/validation/stage2.md), [Stage 1](docs/validation/stage1.md),
[Stage 0](docs/validation/stage0.md)).

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
Pocket Shell (QML on Quickshell) ── Pocket.Core (QtDBus) ──D-Bus session bus── pocketd (Rust, zbus)
  surfaces · services (Quickshell)                   org.pocketos.Core1      ├ pocket-core (state, modes)
  state · features · components · theme (Qt Quick)   org.pocketos.Core1.Apps └ pocket-platform (apps)
  Pocket.Platform ── Wayland: power key, output power, rotation; D-Bus: logind, sensors, OSK
            │ Wayland (layer-shell, foreign-toplevel, input-method)
          Phoc 0.59 (wlroots) ── squeekboard, apps
            │ DRM/KMS, libinput via logind
   postmarketOS / Alpine, OpenRC, elogind, UPower, NetworkManager, BlueZ, iio-sensor-proxy,
   PipeWire + WirePlumber (session; volume via Quickshell.Services.Pipewire)
```

- **Pocket Core** (Rust) owns Pocket state (mode) and authoritative actions (app catalog
  and launching). It never depends on Qt, QML, Quickshell or Phoc.
- **Pocket Shell** (Qt Quick) is only UI. One `Shell` singleton holds the single Core
  connection, the platform adapters and the UI-owned navigation state. QML never runs
  processes. System state comes from the owning service, not from Core.
- **Phoc** composes, routes input, stacks layer surfaces, activates and closes windows,
  and applies output power and transforms through standard protocols.
- **Session**: `pocket-session` (OpenRC, manual) → private bus → session root running
  `pocketd`, Phoc (→ shell), squeekboard and the audio stack under small supervisors with
  restart limits.

Read [`docs/architecture/overview.md`](docs/architecture/overview.md) and the decisions:
[ADR-0001 stack](docs/decisions/0001-initial-technology-stack.md),
[ADR-0002 core & IPC](docs/decisions/0002-pocket-core-and-ipc.md),
[ADR-0003 session lifecycle](docs/decisions/0003-graphical-session-lifecycle.md),
[ADR-0004 shell UI](docs/decisions/0004-shell-ui-architecture.md),
[ADR-0005 apps, windows & compositor](docs/decisions/0005-applications-and-window-management.md),
[ADR-0006 secure lock screen](docs/decisions/0006-secure-lock-screen.md).

## Repository layout

| Path | Contents |
|---|---|
| `apps/shell/` | QML shell. `shell.qml`, `surfaces/` (layer-shell windows) and `services/` (Quickshell service adapters) are specific to Quickshell. `state/` (`Shell` singleton), `features/`, `components/` and `theme/` are plain Qt Quick |
| `bridge/pocket-core-qml/` | `Pocket.Core` QML module: `CoreClient` and `AppCatalog`/`AppFilter`, QtDBus clients of the core (+ integration test) |
| `bridge/pocket-platform-qml/` | `Pocket.Platform` QML module: power key, display power, rotation (Wayland protocols); brightness, accelerometer, OSK (D-Bus) |
| `crates/pocket-ipc/` | IPC contract: D-Bus names, protocol version, `Mode`, state keys |
| `crates/pocket-core/` | Framework-independent state manager (modes, validation, events, revision) |
| `crates/pocket-platform/` | Desktop Entry catalog, visibility rules, safe Exec expansion, icon lookup |
| `crates/pocketd/` | Core daemon on the session bus (zbus) + black-box D-Bus contract test |
| `config/phoc.ini` | Phoc configuration for the device (scale 3) |
| `packaging/` | OpenRC `pocket-session` service (manual), elogind power-key drop-in, scoped sudoers rule |
| `scripts/` | Host dev tools: build, deploy, session control, logs, tests, checks, Quickshell build, device proxy |
| `scripts/device/` | Session root, supervisor, `pocket-ctl`, shell and keyboard launchers (installed to `~/pocket-os/bin`) |
| `tests/device/` | On-device tests: session lifecycle e2e, Stage 2 shell e2e, audio e2e, memory soak, test app entry, Stage 0 touch scene |
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
scripts/build.sh                 # pocketd (host cross-build) + QML modules (built on device)
scripts/deploy.sh --system       # first time: service, power-key policy, sudoers rule
scripts/session.sh start         # Phoc + pocketd + Pocket Shell on tty7
scripts/session.sh shell restart # restart only the shell (core: scripts/session.sh core restart)
scripts/e2e.sh all               # lifecycle + Stage 2 shell + audio e2e on the running session
scripts/boot-backup.sh backup    # before any apk operation (it may re-flash the boot partition)
ssh lavender pocket-os/bin/pocket-ctl ui state   # shell state (windows, panels, adapters)
scripts/logs.sh                  # fetch logs, print events
scripts/session.sh stop          # back to the tty1 console
```

## Known limitations

Highest impact first:

- **No lock screen.** Phoc has no `ext-session-lock-v1`, and Phosh's lock is not
  compositor-enforced. Power-key display-off is power management, not security. Plan:
  ADR-0006.
- **No loudspeaker; jack detection broken.** The kernel/UCM for lavender has no speaker
  route, and a UCM control-name mismatch makes headphones the permanent default output.
  Volume and mute work on the real PipeWire sink. Audible playback has not been
  physically confirmed. Details: [`docs/development/audio.md`](docs/development/audio.md).
- **Package installs can re-flash the boot partition** (pmOS boot-deploy with
  `flash_kernel_on_update`). Follow "Boot-image safety" in the development guide.
- Phoc cannot minimise windows. Home is shown above them instead.
- Quickshell is not packaged for Alpine. It is built from source on the device and must
  be rebuilt after every Qt upgrade (it uses private Qt APIs).
- The device has no internet over USB. Package installs go through the host proxy
  (`scripts/device-online.sh`).
- Core and UI state are in memory. A Core restart resets the mode to `handheld`; a shell
  restart returns to Home in portrait mode.
- A Phoc crash ends the session. Nothing starts at boot (by design during development).
- No realtime scheduling for PipeWire (no RTKit on the device).

## License

MIT. See [LICENSE](LICENSE).
