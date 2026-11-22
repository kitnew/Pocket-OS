# Development guide

Host: any Linux with `ssh`, `git`, `python3`, Rust (via rustup). Target: xiaomi-lavender
reachable as SSH host `lavender` over USB networking (172.16.42.1).

All commands below run **on the host** from the repository root unless prefixed with
`device$`. Scripts accept `POCKET_DEVICE=<ssh-alias>` to target another device.

## 1. One-time setup

### SSH
Key-based login is expected (`ssh lavender true` must not prompt). If it prompts, install
your key once: `ssh-copy-id lavender`. Do not disable host-key checking.

### Device internet (only for installing packages / fetching sources)
The device has no internet over USB. Start the proxy in a host terminal and leave it
running while you install packages:

```sh
scripts/device-online.sh        # listens on 172.16.42.2:3128, serves only 172.16.42.1
```
On the device, `sudo` resets the environment, so pass the proxy explicitly:
```sh
device$ P=http://172.16.42.2:3128
device$ sudo env http_proxy=$P https_proxy=$P apk update
```
(postmarketOS's sshd has `AllowTcpForwarding no`, so an SSH tunnel is not used.)

### Device clock
The RTC is read-only; after a cold boot without network the clock may be in 1970, which
breaks builds (CMake regenerate loops) and TLS. Set it from the host:
```sh
ssh -t lavender "sudo date -u -s @$(date +%s)"
```

### Device packages
Runtime (Stage 0):
```sh
device$ sudo env http_proxy=$P apk add phoc qt6-qtdeclarative qt6-qtwayland qt6-qtsvg \
          layer-shell-qt mesa-utils kmscube wayland-utils libinput-tools evtest grim wlr-randr
```
Build-only, grouped so they can be removed with one command:
```sh
device$ sudo env http_proxy=$P apk add -t .pocket-build-deps cmake samurai g++ pkgconf \
          qt6-qtbase-dev qt6-qtbase-private-dev qt6-qtdeclarative-dev qt6-qtdeclarative-private-dev \
          qt6-qtwayland-dev qt6-qtwayland-private-dev qt6-qtshadertools-dev spirv-tools \
          wayland-dev wayland-protocols libdrm-dev jemalloc-dev
```
Always `apk add --simulate …` first and check that no `linux-*`/`postmarketos-mkinitfs`
packages are upgraded (this device flashes the kernel on update). Never run a blanket
`apk upgrade` as part of Pocket work.

### Quickshell (not packaged for Alpine)
```sh
scripts/quickshell-build.sh     # pinned tag, builds natively on the device (~15–25 min)
```
Installs to `~/pocket-os/opt/quickshell` on the device (nothing system-wide). Quickshell
uses private Qt APIs: **re-run after every Qt upgrade on the device**, otherwise it can
crash with ABI mismatches.

### Session service
```sh
scripts/deploy.sh --system      # installs /etc/init.d/pocket-session + /etc/conf.d (sudo prompt)
```
The service is **not** added to any runlevel; nothing starts at boot.

## 2. Edit → deploy → run loop

```sh
scripts/deploy.sh               # copy apps/shell, scripts/device, config, tests to ~/pocket-os
scripts/session.sh start        # (re)start Phoc + Pocket Shell on tty7 (sudo prompt)
scripts/logs.sh                 # fetch logs into .cache/logs/<ts>/, print diagnostic events
scripts/session.sh stop         # end the session, return to tty1 console
```
Quickshell live-reloads QML when files change, so for QML-only edits `scripts/deploy.sh`
while the session runs is usually enough (a restart is still the reliable path).

Run a different client in the session (e.g. the plain Qt Quick test) by writing
`~/pocket-os/session.env` on the device:
```sh
device$ echo 'POCKET_CLIENT="/usr/lib/qt6/bin/qmlscene $base/tests/qtquick-touch-test.qml"' > ~/pocket-os/session.env
device$ rm ~/pocket-os/session.env     # back to the shell
```

Screenshot of the running session (evidence): `device$ WAYLAND_DISPLAY=wayland-0 grim shot.png`.
Manual orientation test: `device$ WAYLAND_DISPLAY=wayland-0 wlr-randr --output DSI-1 --transform 90`
(`normal` to revert).

## 3. Logs

| What | Where |
|---|---|
| Phoc + shell stdout/stderr | device `~/.local/state/pocket-os/session.log` (previous run: `session.log.1`) |
| Shell diagnostic events | lines containing `pocket.shell.diag: event=` in the session log |
| Quickshell's own logs | device `$XDG_RUNTIME_DIR/quickshell/…` (per-instance) |
| System log | `device$ sudo logread`, `sudo dmesg` (or `scripts/logs.sh --system`) |

## 4. Recovery

The graphical session runs on **tty7**; the normal console (getty) stays on **tty1**; SSH
over USB is independent of both.

| Situation | Action |
|---|---|
| Stop the prototype, return to console | `scripts/session.sh stop` (= `sudo rc-service pocket-session stop`): terminates the logind session and switches to tty1 |
| Shell crashed / exited by itself | Phoc exits with it and the VT returns to tty1. OpenRC then reports the service as `crashed`; `scripts/session.sh start` handles that (stop, then start) |
| Session hung, stop does not return | `device$ sudo loginctl kill-session --signal=KILL <id>` (id from `loginctl list-sessions`, TTY `tty7`), then `sudo chvt 1` |
| Display stuck on tty7 | `device$ sudo chvt 1` |
| SSH lost | Re-plug USB. The USB gadget network is configured in the initramfs, independent of the graphical stack. Last resort: hold power ≥10 s to hard reboot; nothing graphical starts at boot |
| Power key pressed | elogind powers the phone off (default policy). Power it on again; nothing graphical autostarts |

## 5. After a reboot

Nothing Pocket-related starts automatically. To reproduce the Stage 0 tests:
```sh
ssh -t lavender "sudo date -u -s @$(date +%s)"   # if the clock is wrong
scripts/inspect-device.sh                         # baseline report
scripts/session.sh start                          # Phoc + Pocket Shell
scripts/logs.sh
scripts/session.sh stop
```
Graphics checks (as root, with the session stopped):
`device$ sleep 30 | sudo kmscube -D /dev/dri/card1 -c 600` (stdin must stay open — kmscube
exits on any input) and `device$ eglinfo -p surfaceless | grep -E "renderer|version"`.

## 6. Local checks

```sh
scripts/check.sh                # cargo fmt/clippy/test, script syntax, python compile
scripts/check.sh --device       # + qmllint of the runtime-agnostic QML on the device
```

## 7. Rust and cross-compilation

The Rust workspace builds and tests on the host (`cargo test`). Stage 0 ships no Rust
binary to the device, so cross-compilation is not set up. When `pocketd` arrives, options
are: build natively on the device (`apk add rust cargo`, rust 1.100 is packaged), or
cross-compile to `aarch64-unknown-linux-musl` from the host (needs an aarch64 musl
linker). Decision deferred to Stage 1.

## 8. Rollback of Stage 0 system changes

See `docs/validation/stage0.md#rollback`.
