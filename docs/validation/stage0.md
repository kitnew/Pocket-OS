# Stage 0 validation report: hardware & runtime

- Date: 2026-11-22
- Device: Xiaomi Redmi Note 7 `xiaomi-lavender` (Tianma), postmarketOS edge ("Nura edge"),
  kernel 7.2.15-sdm660
- Host: Fedora, connected over USB networking (`ssh lavender` → 172.16.42.1)
- Evidence: `docs/validation/evidence/` (paths below are relative to it)

## Summary

| # | Item | Result |
|---|---|---|
| 5.1 | Baseline inspection | **PASS** |
| 5.2 | GPU acceleration (hardware freedreno, not llvmpipe) | **PASS** |
| 5.2 | DRM/KMS on the physical panel (kmscube) | **PASS**, 59.98 fps vsynced |
| 5.3 | Phoc starts on DSI-1 in a real seat0 logind session | **PASS** |
| 5.3 | Wayland client connects; layer-shell available | **PASS** |
| 5.3 | Touch recognised by Phoc (`wl_seat` capability `touch`) | **PASS** |
| 5.3 | Clean termination, SSH stays up | **PASS** |
| 5.4 | Qt Quick: QML loads, appears on panel, GPU rendering, 60 fps | **PASS** |
| 5.4 | Qt Quick: physical touch, button visibly responds, logged | **PASS** (physical taps) |
| 5.5 | Quickshell: available as package | **FAIL** (not packaged in Alpine/pmOS) |
| 5.5 | Quickshell: source build on device | **PASS** (v0.3.2, Qt 6.12.0) |
| 5.5 | Quickshell: runtime on Phoc (layer-shell, touch, reload) | **PASS** |
| 5.6 | Pocket Shell prototype: portrait, touch, visible state change, diagnostic event | **PASS** (physical taps) |
| 5.6 | Landscape via **manual** output transform (wlr-output-management), touch in landscape | **PASS** (physical tap) |
| 5.6 | Physical auto-rotation (accelerometer) | **NOT TESTED** (out of Stage 0 scope) |
| 5.7 | Restart without reboot (≥3 consecutive stop/start cycles, plus every redeploy) | **PASS** |
| 5.7 | Stop returns to tty1 console; getty alive; SSH alive | **PASS** |
| 5.7 | Boot unchanged (nothing added to runlevels; kernel/boot untouched) | **PASS** (by construction, see below) |
| 5.7 | Reboot after Stage 0 changes | **NOT TESTED** separately. An unplanned power-off/boot cycle happened *before* the session service existed (see Incidents) |

## 5.1 Baseline inspection

Command: `scripts/inspect-device.sh` → `device-inspection.txt`. Highlights (full details in
`docs/hardware/lavender.md`):

- `Linux 7.2.15-sdm660 aarch64`, 8 CPUs, 3599 MiB RAM, zram swap, 50 GiB root (16% used after Stage 0).
- OpenRC default runlevel: sshd, dbus, networkmanager, chronyd, nftables, … (no display manager).
- `/dev/dri/card1` (msm_dpu, `DSI-1` connected, 1080×2340), `/dev/dri/renderD128`.
- Inputs: `nt36672a-ts` touchscreen (event3), power/volume keys, haptics, headset jack.
- elogind seat0; an SSH login is a session **without a seat** (`loginctl`: SEAT `-`).
- D-Bus system bus running. Device has **no internet** (wlan0 not connected).
- Package baseline: `apk-world-before.txt` (12 entries, console UI only).

## 5.2 Graphics

| Command (device) | Observed |
|---|---|
| `eglinfo -p surfaceless` | EGL 1.5 Mesa, driver `msm`, `freedreno` / `FD512` / `OpenGL ES 3.1 Mesa 26.3.1` |
| `sudo eglinfo -p gbm` | same renderer via GBM |
| `sudo dmesg` | `a530_pm4.fw` / `a530_pfp.fw` "loaded … from new location" on first GPU use. The boot-time `-2`/`-40` errors are deferred retries, not a failure |
| `sleep 70 \| sudo kmscube -D /dev/dri/card1 -c 600` | `Rendered 599 frames in 9.99 sec (59.98 fps)` (legacy KMS) |
| `… kmscube -A …` | atomic modesetting works, 57.2 fps average including warm-up, ~60 steady |

Conclusion: **hardware acceleration**. No llvmpipe/softpipe in use. Note: kmscube exits on
any stdin input, so stdin must be a blocking pipe (an early run "succeeded" instantly
for that reason and was discarded).

## 5.3 Phoc

Session plumbing (`packaging/openrc/pocket-session.initd`, `scripts/device/pocket-session`):
`openvt -c 7 -s` → `su -l nikita` (PAM → pam_elogind) → `dbus-run-session` → `phoc -E <client>`.

| Check | Evidence |
|---|---|
| Real seat session | `loginctl list-sessions`: `c39 10000 nikita seat0 tty7 active` |
| Phoc on panel | session log: `Output 'DSI-1' added … 67mm x 145mm`; no errors (only cosmetic `iso-codes` and `cutouts-overlay: No panel found` warnings) |
| Client connects | `wayland-info.txt`: 60 globals incl. `zwlr_layer_shell_v1`, `wp_fractional_scale_manager_v1`, `zwlr_output_manager_v1`, `zwlr_screencopy_manager_v1`, `ext_idle_notifier_v1`; `wl_output DSI-1 scale 3` |
| Touch | `wl_seat seat0 capabilities: touch`; physical taps delivered (5.4/5.6) |
| Termination | `rc-service pocket-session stop` → session gone, `active VT: tty1`, no phoc process. When the `-E` client exits, Phoc exits by itself and the VT returns to tty1 |
| SSH | stayed connected through every session start/stop |

No permissions were relaxed: the user is not in `video`/`input` groups. Device access
comes from logind via libseat.

## 5.4 Qt Quick

Scene: `tests/device/qtquick-touch-test.qml` run with `qmlscene` as the Phoc client.

- `QSG_INFO=1`: `threaded render loop`, `QRhi backend OpenGL`, `OpenGL VENDOR: freedreno RENDERER: FD512 VERSION: OpenGL ES 3.1 Mesa 26.3.1`.
- Fullscreen `360x780` logical @ DPR 3 (= 1080×2340). Screenshot `qtquick-before.png`.
- Frame counter (continuous `FrameAnimation`): `fps≈60.0` in every 5 s window (12 samples).
- Physical test (user tapped the button 3× and the screen elsewhere). Log excerpt:
  ```
  qml: [touch-test] touch press id=0 x=193 y=414
  qml: [touch-test] button tapped count=1 device=touchscreen
  qml: [touch-test] touch press id=0 x=183 y=410
  qml: [touch-test] button tapped count=2 device=touchscreen
  qml: [touch-test] touch press id=0 x=182 y=404
  qml: [touch-test] button tapped count=3 device=touchscreen
  qml: [touch-test] touch press id=0 x=169 y=201
  ```
  `qtquick-after.png` was captured afterwards. Exit was clean (the session stopped normally).

## 5.5 Quickshell

| Question | Finding |
|---|---|
| Package | none in Alpine edge main/community/testing or the pmOS repo (`apk search -x quickshell` → empty) |
| Build reqs | CMake, Qt ≥ 6.6 incl. **private** `qtdeclarative` headers, qtshadertools, spirv-tools, wayland-protocols, libdrm, CLI11 (header-only, **not packaged** in Alpine → fetched pinned v2.5.0 for the build), optional jemalloc |
| Qt compatibility | Built against Qt 6.12.0 (Alpine). Quickshell must be **rebuilt after every Qt upgrade** (private API/ABI) |
| Build | `scripts/quickshell-build.sh` (v0.3.2, native on device, `-j4`, 872 steps, warnings only). Disabled: crash handler, X11, i3, Hyprland, PipeWire, PAM, polkit, greetd, Bluetooth, NetworkManager integration |
| Footprint | install 141 MiB (RelWithDebInfo, user prefix `~/pocket-os/opt/quickshell`). Build deps ~2.5 GiB (`.pocket-build-deps`) |
| Runtime | Quickshell `PanelWindow` on `zwlr_layer_shell_v1` (Bottom layer, all edges anchored), renders through `libgallium` (freedreno). RSS ≈ 200 MiB, Phoc ≈ 87 MiB |
| Wrinkles found | Ninja is provided by `samurai` on Alpine (`ninja-build` installs to a private path). A device clock in 1970 caused an endless CMake regenerate loop, fixed by setting the clock and extracting sources with `tar -m`. Harmless warning: no `org.freedesktop.portal.Desktop` |

Decision: **Quickshell works and is used for the prototype.** layer-shell-qt (packaged)
remains the documented fallback (ADR-0001).

## 5.6 Pocket Shell prototype

`apps/shell/shell.qml` → `surfaces/HomeSurface.qml` (Quickshell layer surface) →
`features/stage0/Stage0Panel.qml` + `components/PocketButton.qml` + `theme/Theme.qml`.
Launch: `scripts/deploy.sh && scripts/session.sh start`.

Physical verification (user tapped and confirmed the label/colour change on screen).
`shell-session-touch-rotation.log`:
```
pocket.shell.diag: event=shell.started runtime=Quickshell size=360x780 landscape=false
pocket.shell.diag: event=button.activated count=1 x=121 y=41 device=touchscreen
pocket.shell.diag: event=button.activated count=2 x=120 y=46 device=touchscreen
pocket.shell.diag: event=layout.orientation landscape=true size=780x360
pocket.shell.diag: event=button.activated count=3 x=138 y=31 device=touchscreen
```
Screenshots: `shell-portrait.png` (initial), `shell-portrait-tapped.png`, `shell-landscape.png`.

Orientation: **manual output transform only**
(`WAYLAND_DISPLAY=wayland-0 wlr-randr --output DSI-1 --transform 90`). Phoc rotated the
output and the surface got 780×360. The layout re-flowed to two columns, and a physical
tap in landscape hit the button (count=3), so Phoc maps touch input to the transform. A
sequence `90 → normal → 270 → normal` produced exactly 4 orientation events after the
debounce fix. Physical auto-rotation was **not** tested.

## 5.7 Operational reliability

| Check | Command | Result |
|---|---|---|
| SSH during session | any `ssh lavender …` while the session runs | works; `usb0` stays up |
| Remote logs | `scripts/logs.sh` | copies `session.log` to `.cache/logs/<ts>/`, prints diag events |
| Restart | `scripts/session.sh restart` (stop + start), repeated | new seat0 session each time (`c145`, `c149`, …), `shell.started` logged once per run |
| Stop | `scripts/session.sh stop` | `active VT: tty1`, no phoc/quickshell, no tty7 session, getty running |
| Boot impact | `rc-update show \| grep -c pocket` → `0` | nothing autostarts. Kernel, boot image, bootloader, partitions and firmware untouched |

### Incidents during Stage 0

1. **Unplanned power-off** (before any graphical work). `logread` showed
   `elogind: Power key pressed short. / Powering off...`. The power key was pressed, and
   the elogind default `HandlePowerKey=poweroff` applied. pstore confirms an orderly
   `reboot: Power down` (no panic). The phone came back on USB power. This has been
   documented as a usability risk.
2. **Device clock at 1970** after that boot (no NTP offline, RTC read-only). Fixed by
   setting time from the host. It is a documented prerequisite step.

## Stage 0 system changes on the device

| Change | Purpose |
|---|---|
| `~/.ssh/authorized_keys` += host's `id_ed25519.pub` | key-based SSH (password asked once, interactively; not stored) |
| `apk add phoc qt6-qtdeclarative qt6-qtwayland qt6-qtsvg layer-shell-qt mesa-utils kmscube wayland-utils libinput-tools evtest grim wlr-randr` | runtime + diagnostics (`apk-world-after.txt`) |
| `apk add -t .pocket-build-deps …` | Quickshell build only (upgraded `glib` 2.90.0→2.90.1 and `fontconfig` 2.18.1→2.18.4 as dependencies; no kernel/boot packages) |
| `/etc/init.d/pocket-session`, `/etc/conf.d/pocket-session` | manual session service (not in any runlevel) |
| `~/pocket-os/` (shell, bin, config, tests, `opt/quickshell`), `~/pocket-os-build/`, `~/pocket-os-stage0/`, `~/.local/state/pocket-os/` | user files |
| system clock set from host | builds/TLS |

Not changed: kernel, boot image, bootloader, partitions, firmware, sshd config, PAM,
elogind config, user groups, runlevels.

### Rollback

```sh
device$ sudo rc-service pocket-session stop
device$ sudo rm /etc/init.d/pocket-session /etc/conf.d/pocket-session
device$ sudo apk del .pocket-build-deps                     # build toolchain (~2.5 GiB)
device$ sudo apk del phoc qt6-qtdeclarative qt6-qtwayland qt6-qtsvg layer-shell-qt mesa-utils \
          kmscube wayland-utils libinput-tools evtest grim wlr-randr
device$ rm -rf ~/pocket-os ~/pocket-os-build ~/.local/state/pocket-os   # keep ~/pocket-os-stage0 for the baseline lists
# optional: remove the host key line from ~/.ssh/authorized_keys
```
`apk del` returns the world file to `apk-world-before.txt`. The dependency upgrades of
glib/fontconfig stay (edge versions, harmless). `apk` does not downgrade.

## Known limitations

- Quickshell is unpackaged and has to be rebuilt after every Qt upgrade.
- Session control needs sudo. There is no autostart, by design for Stage 0.
- If the shell exits, Phoc and the session end too (`phoc -E`).
- The power key powers off the device. There is no inhibitor yet.
- qmllint reports a false-positive warning for the `Theme` directory singleton when the
  file is linted standalone. The runtime resolves it correctly.
- Device internet relies on the host proxy (`scripts/device-online.sh`) unless Wi-Fi is set up.

## Next steps (Stage 1 — Pocket Core Foundation)

1. `pocketd`: a Rust daemon on the session bus (`org.pocketos.Core1`) built on
   `pocket-core`, using `zbus`. Decide between native and cross builds for aarch64-musl.
2. `pocket-platform` with the first adapters needed by Stage 1 features: elogind
   (power-key inhibitor, session) and UPower (battery).
3. A QML-side IPC client (QtDBus through a small Quickshell/QML-facing module, or
   Quickshell's existing D-Bus integration) that mirrors core state. It must not use shell
   commands.
4. Decouple shell restart from the Phoc lifetime (supervisor) and decide on autostart
   (an OpenRC runlevel or a greeter) once recovery has been exercised across reboots.
5. Package Quickshell (and later Pocket) as APKs under `packaging/alpine/`.
