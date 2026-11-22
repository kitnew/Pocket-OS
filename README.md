# Pocket OS

Pocket OS is a modular, Linux-native mobile shell: its own home screen, launcher, status
bar, quick settings and notifications, plus an optional landscape **Dock Mode** that turns
the phone into a configurable desktop control surface. It runs on top of an ordinary
postmarketOS/Alpine system and standard Linux services. It is not a new distribution and
does not include a custom compositor.

**Status: Stage 0 (hardware & runtime validation) complete.** A minimal interactive
shell prototype runs on the physical device under Phoc. No user-facing features (home,
launcher, settings, notifications, Dock Mode) exist yet. See
[`docs/validation/stage0.md`](docs/validation/stage0.md) for the evidence.

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
Pocket Shell (QML, Quickshell)  ──D-Bus (planned)──  Pocket Core (Rust)
            │ Wayland (layer-shell)                         │ adapters (planned)
          Phoc (wlroots compositor)              NetworkManager, UPower, BlueZ, elogind…
            │ DRM/KMS, libinput via logind
   postmarketOS / Alpine, OpenRC, Linux
```

- **Pocket Core** (Rust) owns state and policy and never depends on Qt, QML, Quickshell
  or Phoc.
- **Pocket Shell** (Qt Quick) is only UI. It must not run shell commands for system
  control. System actions go through the core.
- **Phoc** does composition, input, output transforms and scaling through standard
  Wayland protocols.

Read [`docs/architecture/overview.md`](docs/architecture/overview.md) and
[`docs/decisions/0001-initial-technology-stack.md`](docs/decisions/0001-initial-technology-stack.md).

## Repository layout

| Path | Contents |
|---|---|
| `apps/shell/` | QML shell. `shell.qml` and `surfaces/` are specific to Quickshell. `components/`, `features/` and `theme/` are plain Qt Quick |
| `crates/pocket-ipc/` | Versioned IPC contract types (`Mode`, `ProtocolVersion`) |
| `crates/pocket-core/` | Framework-independent core state (mode state machine) |
| `config/phoc.ini` | Phoc configuration for the device (scale 3) |
| `packaging/openrc/` | `pocket-session` OpenRC service (started by hand, not enabled at boot) |
| `scripts/` | Host dev tools: deploy, session control, logs, inspection, checks, Quickshell build, device proxy |
| `scripts/device/` | Session launchers installed to `~/pocket-os/bin` on the device |
| `tests/device/` | On-device validation scenes |
| `docs/` | Architecture, decisions (ADRs), hardware, development, validation |

## Requirements

- Host: Linux with `ssh`, `git`, `python3`, `curl`, and Rust stable (`rustup`). Tested on Fedora.
- Device: postmarketOS edge on xiaomi-lavender, reachable by key-based `ssh lavender`
  over USB networking.
- Device packages and a one-time Quickshell build. See
  [`docs/development/README.md`](docs/development/README.md).

## Build and run the Stage 0 prototype

```sh
scripts/check.sh                 # host: Rust fmt/clippy/tests, script syntax
scripts/quickshell-build.sh      # once, and after every Qt upgrade on the device
scripts/deploy.sh --system       # first time: also installs the OpenRC service
scripts/session.sh start         # Phoc + Pocket Shell on tty7 (asks for sudo)
scripts/logs.sh                  # fetch logs and print shell diagnostic events
scripts/session.sh stop          # back to the tty1 console
```

## Known limitations

- Quickshell is not packaged for Alpine. It is built from source on the device and must
  be rebuilt after every Qt upgrade (it uses private Qt APIs).
- The device has no internet over USB. Package installs go through the host proxy
  (`scripts/device-online.sh`).
- A short press of the power key powers the phone off (elogind default). Pocket does not
  take a power-key inhibitor yet.
- Physical auto-rotation is not implemented. Landscape was tested only through a manual
  output transform.
- Nothing starts at boot. Session control needs `sudo` on the device.
- Pocket Core is a library skeleton. There is no daemon or IPC transport yet.

## License

MIT. See [LICENSE](LICENSE).
