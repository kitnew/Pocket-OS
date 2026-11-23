# Development guide

Host: Linux with `ssh`, `git`, `python3`, `curl`, `dbus-daemon`, Rust via rustup (the
`aarch64-unknown-linux-musl` target is added by `scripts/build.sh`). Target: xiaomi-lavender
reachable as SSH host `lavender` over USB networking (172.16.42.1).

Commands run **on the host** from the repository root unless prefixed with `device$`.
Scripts accept `POCKET_DEVICE=<ssh-alias>` to target another device.

## 0. Quick reference

```sh
scripts/check.sh                 # host: fmt, clippy, unit + D-Bus contract tests, shellcheck
scripts/build.sh                 # pocketd (host cross-build) + Pocket.Core QML bridge (on device)
scripts/deploy.sh                # copy shell, scripts, config, tests, pocketd to ~/pocket-os
scripts/session.sh start         # Phoc + pocketd + Pocket Shell on tty7
scripts/session.sh status        # session, components, core state
scripts/session.sh shell restart # restart only the shell   (also: stop | start)
scripts/session.sh core restart  # restart only pocketd     (also: stop | start)
scripts/session.sh osk restart   # restart only the on-screen keyboard
scripts/e2e.sh stage2            # Stage 2 shell e2e (apps, windows, panels, rotation, power)
scripts/logs.sh                  # fetch session log, print diagnostic events
scripts/e2e.sh                   # lifecycle e2e test against the running session
scripts/test-device.sh           # contract + bridge integration tests on the device
scripts/session.sh stop          # end the session, back to the tty1 console
```

## 1. One-time setup

### SSH
Key-based login is expected (`ssh lavender true` must not prompt). Install your key once
with `ssh-copy-id lavender`. Do not disable host-key checking.

### Device internet (only for installing packages / fetching sources)
The device has no internet over USB. Start the proxy in a host terminal and leave it
running while you install packages:
```sh
scripts/device-online.sh        # listens on 172.16.42.2:3128, serves only 172.16.42.1
device$ P=http://172.16.42.2:3128
device$ sudo env http_proxy=$P https_proxy=$P apk update
```

### Device clock
The RTC is read-only. `swclock-offset` keeps time across warm reboots, but after a cold
boot without network the clock may be in 1970. That breaks on-device CMake builds
(`scripts/build.sh bridge` refuses to build and prints this command) and TLS:
```sh
ssh -t lavender "sudo date -u -s @$(date +%s)"
```

### Device packages
Runtime (Stage 0):
```sh
device$ sudo env http_proxy=$P apk add phoc qt6-qtdeclarative qt6-qtwayland qt6-qtsvg \
          layer-shell-qt mesa-utils kmscube wayland-utils libinput-tools evtest grim wlr-randr
```
Build-only (Quickshell and the QML bridge), grouped so they can be removed together:
```sh
device$ sudo env http_proxy=$P apk add -t .pocket-build-deps cmake samurai g++ pkgconf \
          qt6-qtbase-dev qt6-qtbase-private-dev qt6-qtdeclarative-dev qt6-qtdeclarative-private-dev \
          qt6-qtwayland-dev qt6-qtwayland-private-dev qt6-qtshadertools-dev spirv-tools \
          wayland-dev wayland-protocols libdrm-dev jemalloc-dev
```
Stage 2 services, keyboard, test apps and icons:
```sh
device$ sudo env http_proxy=$P https_proxy=$P apk add upower iio-sensor-proxy squeekboard bluez \
          foot gtk+3.0-demo adwaita-icon-theme polkit-elogind-libs
device$ sudo rc-update add iio-sensor-proxy default && sudo rc-service iio-sensor-proxy start
device$ sudo rc-update add bluetooth default && sudo rc-service bluetooth start
```
`polkit-elogind-libs` replaces `polkit-noelogind-libs`. Without it polkit never sees the
seat session as active, so the accelerometer claim and the Wi-Fi toggle are refused.
These packages ship udev rules, so the install runs pmOS's `mkinitfs` trigger (initramfs
and `boot.img` are regenerated for the same kernel). Reboot once afterwards to confirm.

Always run `apk add --simulate …` first and check that no `linux-*` or
`postmarketos-mkinitfs` package gets upgraded. Never run a blanket `apk upgrade` as part of
Pocket work. Stage 1 added no packages: Rust is built on the host, and QtDBus is part of
`qt6-qtbase`.

### Quickshell (not packaged for Alpine)
```sh
scripts/quickshell-build.sh     # pinned v0.3.2, native build on the device (~15–25 min)
```
Re-run after every Qt upgrade on the device (Quickshell uses private Qt APIs). The
Pocket.Core bridge uses only public Qt API, but rebuild it too (`scripts/build.sh bridge`).

### System files (root, once)
```sh
scripts/deploy.sh --system      # sudo prompt (or POCKET_SUDO_PASSWORD_FILE=<file>)
```
This installs:
- `/etc/init.d/pocket-session` + `/etc/conf.d/pocket-session`. The service is **not** in any
  runlevel, so nothing starts at boot.
- `/etc/elogind/logind.conf.d/50-pocket-os-dev.conf`: a short power-key press is ignored
  (development policy). pmOS's `/usr/lib/elogind/logind.conf` is not read by elogind 255.
  Check it with
  `busctl get-property org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager HandlePowerKey`.
- `/etc/sudoers.d/pocket-os-dev`: password-less `rc-service pocket-session start|stop|restart|status`
  for the session user, and nothing else.

## 2. Build

| Component | Where | Command | Output |
|---|---|---|---|
| `pocketd` | host, cross-compiled (static aarch64-musl, `rust-lld`, `.cargo/config.toml`) | `scripts/build.sh rust` | `target/aarch64-unknown-linux-musl/release/pocketd` |
| Pocket.Core + Pocket.Platform QML modules | device, native CMake/Ninja against the device's Qt | `scripts/build.sh bridge` | device `~/pocket-os/qml/Pocket/{Core,Platform}/` |
| Quickshell | device | `scripts/quickshell-build.sh` | device `~/pocket-os/opt/quickshell` |

Host-only Rust work: `cargo build`, `cargo test` (the contract test starts a private
`dbus-daemon`; nothing touches your desktop session bus).

## 3. Deploy and run

```sh
scripts/deploy.sh               # user files + pocketd (no root)
scripts/session.sh start        # (re)start the whole session
```
`deploy.sh` replaces `pocketd` by rename, so a running core keeps working until it is
restarted (`scripts/session.sh core restart`). QML changes: Quickshell live-reloads, or
use `scripts/session.sh shell restart`. Changes to the session scripts
(`scripts/device/pocket-session*`, `pocket-supervise`) need a session restart.

Run a different client instead of the shell (Phoc's supervised client) by writing
`~/pocket-os/session.env` on the device:
```sh
device$ echo 'POCKET_CLIENT="/usr/lib/qt6/bin/qmlscene $base/tests/qtquick-touch-test.qml"' > ~/pocket-os/session.env
device$ rm ~/pocket-os/session.env     # back to the shell
```
The same file can set `POCKET_RESTART_MAX`/`POCKET_RESTART_WINDOW` (supervisor limits),
`POCKET_PHOC_VERBOSE=1` and `POCKETD_LOG=debug`.

## 4. Inspect and debug

On the device, as the session user (SSH works; no root needed):
```sh
device$ ~/pocket-os/bin/pocket-ctl status             # components, pids, gave-up state, core state
device$ ~/pocket-os/bin/pocket-ctl core GetState      # call the core (dbus-send)
device$ ~/pocket-os/bin/pocket-ctl core RequestMode string:dock
device$ . $XDG_RUNTIME_DIR/pocket-os/env              # session bus + Wayland for other tools
device$ dbus-monitor --session "sender='org.pocketos.Core1'"
device$ grim /tmp/shot.png                            # screenshot (after sourcing env)
device$ wlr-randr --output DSI-1 --transform 90       # manual landscape (normal to revert)
device$ ~/pocket-os/bin/pocket-ctl kill shell SEGV    # fault injection (core: ABRT; Rust ignores an external SEGV)
device$ ~/pocket-os/bin/pocket-ctl ui state           # shell state as JSON (windows, panels, adapters)
device$ ~/pocket-os/bin/pocket-ctl ui open foot       # same action as tapping the app
device$ ~/pocket-os/bin/pocket-ctl apps ListApps      # Core's application catalog
```

| What | Where |
|---|---|
| All session output (Phoc, pocketd, shell, supervisors) | device `~/.local/state/pocket-os/session.log` (previous run: `session.log.1`) |
| Session / supervision events | lines `pocket.session:` / `pocket.supervise: name=…` |
| Core | lines `pocketd` (tracing; level `POCKETD_LOG`) |
| Bridge | lines `pocket.core.client` |
| UI diagnostic events | lines `pocket.shell.diag: event=…` |
| Gave-up diagnostics | `~/.local/state/pocket-os/<component>-gave-up.log` |
| Runtime control files | `$XDG_RUNTIME_DIR/pocket-os/` (exists only while a session runs) |
| System log | `device$ logread`, `sudo dmesg` (or `scripts/logs.sh --system`) |

## 5. Recovery

The graphical session runs on **tty7**, the console getty on **tty1**. SSH over USB is
independent of both.

| Situation | Action |
|---|---|
| Stop everything, back to console | `scripts/session.sh stop` |
| Shell crashed | Restarted automatically (Phoc and core keep running) |
| Shell or core "gave up" (5 crashes / 60 s) | Read `~/.local/state/pocket-os/<name>-gave-up.log`, fix, `scripts/session.sh shell start` (or `core start`) |
| UI shows "Core: Disconnected" | `scripts/session.sh status`. If the core is held or gave up: `scripts/session.sh core start` |
| Phoc exited | Session is over (VT back to tty1). `scripts/session.sh start` |
| Stop does not return | `device$ sudo loginctl kill-session --signal=KILL <id>` (TTY `tty7` in `loginctl list-sessions`), then `sudo chvt 1` |
| Display stuck on tty7 | `device$ sudo chvt 1` |
| SSH lost | Re-plug USB (the USB network comes from the initramfs). Last resort: hold power ≥10 s. Nothing graphical starts at boot |
| Power key pressed | Short press turns the display off/on (shell). Without a shell it does nothing. Long hold forces a hardware reset |
| Display stays dark | Press power (a restarted shell re-grabs the key and reads the real output state), or `device$ ~/pocket-os/bin/pocket-ctl ui display true` |
| Keyboard stuck or gone | `scripts/session.sh osk restart` |

## 6. After a reboot

Nothing Pocket-related starts automatically.
```sh
ssh lavender true && scripts/session.sh status   # expect: no session, tty1
scripts/session.sh start
scripts/e2e.sh                                   # optional: full lifecycle check
scripts/session.sh stop
```

## 7. Tests

| Test | Command | Needs |
|---|---|---|
| Rust unit tests + `pocketd` D-Bus contract test | `scripts/check.sh` (or `cargo test`) | host `dbus-daemon` |
| Same contract test on the device + bridge (C++/QtTest) integration test | `scripts/test-device.sh` | `scripts/build.sh && scripts/deploy.sh`. Runs on private buses, safe while a session runs |
| Session lifecycle e2e (signals, core/shell crash and restart, give-up, recovery) | `scripts/e2e.sh` | a running session. Evidence in `.cache/e2e/<ts>/` |
| Stage 2 shell e2e (catalog, launch, home/reactivate, switcher, failures, close, quick settings, brightness, rotation, display power, OSK, app survival) | `scripts/e2e.sh stage2` (`all` = both) | a running session; installs/removes test `.desktop` files in `~/.local/share/applications` |
| QML lint | `scripts/check.sh --device` (also runs `test-device.sh`) | device |
| Physical touch | the script in `docs/validation/stage2.md` (launcher search with OSK, Home/Running chip, switcher, Quick Settings, landscape taps, auto-rotation, power key); check the `pocket.shell.diag` events in `scripts/logs.sh` | a person |

## 8. Rollback of device changes

Stage 2: `docs/validation/stage2.md#device-changes-in-stage-2-and-rollback`.
Stage 1: `docs/validation/stage1.md#device-changes-in-stage-1`. Stage 0:
`docs/validation/stage0.md#rollback`.

## 9. Shell development (Stage 2)

Read [ADR-0004](../decisions/0004-shell-ui-architecture.md) first. The rule: Quickshell
types only in `shell.qml`, `surfaces/`, `services/`. Everything else is plain Qt Quick.
QML changes need only `scripts/deploy.sh` (then `scripts/session.sh shell restart`). C++
module changes need `scripts/build.sh bridge` first.

### Add a UI component

1. Create `apps/shell/components/MyThing.qml`. Use `Theme` tokens only (colors,
   `space1…6`, radii, `touchTarget` ≥ 44, `duration*`). No hard-coded pixel sizes of the
   device.
2. Accept data through properties and report actions through signals. Components do
   not import `state/` (features do).
3. Lint: `scripts/check.sh --device`.

### Add a shell surface

1. Put the content in a portable view in `features/<name>/`. It reads `Shell` and
   calls `Shell` functions.
2. Add `surfaces/<Name>Surface.qml`: a `PanelWindow` with explicit
   `WlrLayershell.layer`, `namespace` (`pocket-<name>`), `keyboardFocus` and
   `exclusionMode`, and `visible:` bound to `Shell` state. Unmap it when it isn't
   needed, rather than leaving a transparent surface that takes touches. Use `mask` if it
   only covers part of its area.
3. Instantiate it in `shell.qml`. If it is modal, add its name to `Shell.panel`.

### Integrate a new system action or state

- System-owned state with a Quickshell integration → expose plain properties from
  `services/SystemStatus.qml` (or a new service adapter).
- No Quickshell integration, and it needs the shell's Wayland connection or a
  session/system D-Bus service → add a `QML_ELEMENT` class to
  `bridge/pocket-platform-qml/` and instantiate it once in `state/Shell.qml`.
- An authoritative action other clients will need too (like launching) → a
  `pocket-platform` function served by `pocketd` on a new object/method (protocol minor
  bump, contract test in `crates/pocketd/tests/dbus_contract.rs`, bridge test in
  `bridge/pocket-core-qml/tests/`).
- Never run commands from QML.

### Launch and debug an application

```sh
device$ P=~/pocket-os/bin/pocket-ctl
device$ $P apps ListApps | less              # is it in the catalog? (hidden entries are not)
device$ echo 'export POCKETD_LOG=pocketd=debug' >> ~/pocket-os/session.env   # next session: logs why entries are skipped
device$ $P apps Launch string:foot           # launch through Core, prints the pid
device$ $P ui open foot                      # launch as the UI does (reactivates if running)
device$ $P ui state                          # windows (app_id, title, activated), home, panel
device$ grep -a "pocketd::apps\|event=app\.\|event=window\." ~/.local/state/pocket-os/session.log
```
App stdout/stderr go to `session.log`. An app shows a "running" dot and is reactivated
instead of relaunched when its Wayland app_id equals its desktop ID or its
`StartupWMClass`. To make a `NoDisplay` app visible, or to fix its `StartupWMClass`, put
an override with the same file name in `~/.local/share/applications/`. Core picks it up
immediately.

### Reproduce the Stage 2 tests

```sh
scripts/check.sh --device        # host checks + qmllint + device integration tests
scripts/session.sh start
scripts/e2e.sh all               # Stage 1 lifecycle (29) + Stage 2 functions (56, 1 skip)
```
Then run the physical script from `docs/validation/stage2.md`.
