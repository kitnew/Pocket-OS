# Stage 2 validation: Mobile Shell Foundation

- Date: 2026-11-23
- Device: Xiaomi Redmi Note 7 (`xiaomi-lavender`, Tianma), postmarketOS edge, kernel
  7.2.15-sdm660, Phoc 0.59.0, Qt 6.12.0, Quickshell 0.3.2 (rebuilt with Networking and
  Bluetooth modules), Mesa 26.3.1 freedreno (Adreno 512, hardware rendering confirmed by
  `/dev/dri/renderD128` mappings in the shell process).
- Baseline: Stage 1 `dbe3dac`.
- Evidence: `docs/validation/evidence/stage2/`.

**Verdict: COMPLETE**, with the explicit exceptions listed under "Not implemented /
blocked" (secure lock screen: BLOCKED by the compositor; volume control: not
implemented). Every other acceptance item passed on the device, including the physical
touchscreen test.

## What was tested

| Layer | How | Result |
|---|---|---|
| Rust (fmt, clippy `-D warnings`, unit tests: core 6, ipc 3, platform 8, pocketd 1) | `scripts/check.sh` | PASS |
| `pocketd` D-Bus contract, incl. new Apps object (catalog, launch, exit report, typed errors, inotify rescan) | `scripts/check.sh` (host) and `scripts/test-device.sh` (device) | PASS 2/2 on both |
| QML bridge (`CoreClient` lifecycle + new `AppCatalog`/`AppFilter`) against the real `pocketd` | `scripts/test-device.sh` | PASS 4/4 |
| QML lint (portable `state/ features/ components/ theme/`) | `scripts/check.sh --device` | PASS (only the documented Theme singleton false positive) |
| Shell scripts | `sh -n`, `shellcheck -S warning` | PASS |
| Stage 1 session lifecycle e2e (signals, Core/shell crash + restart, give-up, recovery) | `scripts/e2e.sh` | **PASS 29/29**, after a reboot. `evidence/stage2/session-e2e.txt` |
| Stage 2 functional e2e (below) | `scripts/e2e.sh stage2` | **PASS 56, FAIL 0, SKIP 1**. `evidence/stage2/stage2-e2e.txt` |
| Physical touchscreen test (below) | a person at the device | **PASS**. `evidence/stage2/touch-physical.log` |
| Reboot | `reboot` after the package changes; boot, no autostart, session start, both e2e suites | PASS |

### Stage 2 e2e (`tests/device/stage2-e2e.sh`)

It drives the UI over the shell's IPC handler (`pocket-ctl ui …`). The handler calls the
same `Shell` functions as the touch UI. Results are checked against independent
sources: Core over D-Bus, the compositor (`wlr-randr`, foreign toplevels), the process
table, sysfs, and the diagnostic log.

1. Adapters present: Core connected, power key grabbed, OSK service, UPower battery,
   NetworkManager Wi-Fi device, BlueZ adapter, iio-sensor-proxy accelerometer.
2. Catalog: Core lists installed apps. A new `.desktop` file is picked up live (inotify →
   `AppsChanged` → shell).
3. Launch `foot` through Core → the foreign toplevel appears → Home is hidden → the
   window is active.
4. Home over a running app. Opening the app again reactivates it (one process, no
   duplicate).
5. Second app (GTK3 widget factory, matched through `StartupWMClass`). The switcher lists
   both windows. **SKIP:** compositor activation driven purely over IPC. Quickshell's
   `Toplevel.activate()` uses the seat of the shell's last input event, and a shell that
   has received no touch since it started sends nothing. Activation with real touch
   passed in the physical test.
6. Launch failures are visible: an early non-zero exit (`status=3`) and an unknown app
   both produce error toasts.
7. Closing windows → Home returns on the home page, and the keyboard is hidden.
8. Quick Settings opens and closes. Brightness changes the sysfs backlight through logind.
9. Rotation: landscape → `wlr-randr` reports transform 90 → Home lays out in landscape;
   then back to portrait.
10. Display power off/on → `wlr-randr` reports the output disabled/enabled.
11. OSK show/hide over `sm.puri.OSK0` is mirrored by the shell. Restarting the keyboard
    component recovers it without a reboot.
12. A running app survives a shell restart (the restarted shell lists its window) and a
    Core restart.

## Physical verification on the Redmi's screen

The tester followed this script with touch only. No IPC or SSH input was sent during the
test, so every logged UI event below was caused by touch. The tester reported that all
steps worked. The log (`evidence/stage2/touch-physical.log`) confirms each step:

| Step | Logged evidence |
|---|---|
| All apps → tap search → type "fo" with the OSK → tap Foot | `nav.page page=apps`, `launcher.search focus=true`, `osk.visible visible=true`, `app.launch id=foot`, pocketd `launched id="foot"`, `window.opened appId=foot` |
| Type `echo hi` + Enter in Foot with the OSK | Reported by the tester (keystrokes are not logged by design) |
| Home → tap the "Foot" running chip | `nav.home windows=1`, `window.activate appId=foot`, then OSK shown for foot again |
| Home → GTK Widget Factory → switcher → Foot card → switcher → × on GTK card | `app.launch id=pocket-test-widget-factory`, `window.opened … windows=2`, `nav.page page=switcher`, `window.activate appId=foot`, `window.close appId=gtk3-widget-factory`, `window.closed … windows=0` |
| Status bar → Quick Settings → brightness drag → Rotation (landscape) | `panel open=quicksettings`, `rotation.mode mode=landscape`, `output.applied ok=true rotation=90` |
| In landscape: Home → All apps → tap an app | `layout.orientation … size=780x288 landscape=true`, `nav.page page=apps`, `app.reactivate id=foot`: correct touch mapping in landscape |
| Rotation Auto + tilting | `sensor: orientation right-up` → `rotation=270`, `left-up` → `rotation=90`, `normal` → `rotation=0` (`bottom-up` is deliberately ignored) |
| Power key short press, touch while dark, press again | `powerkey.pressed display=on`, `output DSI-1 power off`, `display.power on=false`, `powerkey.pressed display=off`, `request display on`. No touch events while dark (Phoc drops them) |

Screenshots (`grim`, downscaled): `1-home.png`, `3-foot.png` (app + OSK),
`4-home-running.png`, `5-switcher.png`, `8-quicksettings.png`, `9-landscape.png`.

## Performance (idle, Home shown, no apps)

| Process | CPU over 30 s idle | RSS |
|---|---|---|
| Quickshell (Pocket Shell) | 0 ticks | 327 MiB (161 MiB anonymous; 73 MiB is `libLLVM` mapped by Mesa gallium; the rest is Qt/Mesa libraries and GPU buffer mappings) |
| `pocketd` | 0 ticks | 3 MiB |
| squeekboard | 0 ticks | 43 MiB |
| Phoc | 0 ticks | 70 MiB |
| System | — | 439 MiB used of 3.6 GiB |

- No timers run while idle except the minute clock. The toast timer runs only while
  toasts exist. The accelerometer is claimed only in Auto rotation with the display on.
  No polling loops: Core, apps, windows, OSK and sensor all update from signals.
- Leak check: 20 launch/close cycles and 20 page switches took shell RSS from 294 to
  315 MiB, and the growth slowed (+13.5, then +6.7 MiB). This fits allocator, glyph-cache
  and JS-heap warm-up, but a longer soak test is still to do. Core RSS stayed at 3 MiB.
- Responsiveness: not measured with instrumentation. The physical tester reported every
  step working, without commenting on speed. Core launch to `launched` takes about 2 ms
  (pocketd log timestamps). GTK apps take about 6–8 s
  to show their first window on this device (GTK start-up, not the shell).

## Not implemented / blocked

| Item | Status | Reason |
|---|---|---|
| Secure lock screen | **BLOCKED** | Phoc 0.59 does not offer `ext-session-lock-v1` (registry and sources checked). An ordinary overlay would not be secure. Deliberately not built (ADR-0005). The power key only turns the display off |
| Volume control | **NOT IMPLEMENTED** | The sound card (`sdm660 - Xiaomi Redmi Note 7`) registers some time after boot, but the session runs no PipeWire/WirePlumber, and Quickshell was built without its PipeWire module. The tile is shown disabled with that reason |
| Minimise windows | **Not supported by Phoc** | Phoc ignores foreign-toplevel `minimize`. Home is shown above windows instead (works) |
| Activation without prior input (IPC/tests only) | Limitation | See e2e step 5. Not visible to a touch user |
| OSK in landscape | Limitation | squeekboard's landscape layout takes most of the 360 px logical height. Usable, but cramped |

## Device changes in Stage 2 (and rollback)

| Change | Why | Rollback |
|---|---|---|
| `apk add upower iio-sensor-proxy squeekboard bluez foot gtk+3.0-demo adwaita-icon-theme` (30 packages incl. deps; no kernel/firmware packages) | battery, accelerometer, OSK, Bluetooth, test apps, icons | `apk del upower iio-sensor-proxy squeekboard bluez foot gtk+3.0-demo adwaita-icon-theme` |
| The install ran pmOS's `mkinitfs`/`boot-deploy` trigger (udev rules from `iio-sensor-proxy-udev`, `feedbackd-udev`, `upower-udev`): `/boot/initramfs` and `/boot/boot.img` were regenerated for the **same** kernel | standard pmOS behaviour for udev-rule packages | Verified by a reboot (boot OK, same kernel). Removing the packages regenerates it again |
| `apk add polkit-elogind-libs` (replaces `polkit-noelogind-libs`) | without elogind support polkit never treats the seat session as active, so the sensor claim and the NetworkManager Wi-Fi toggle were refused | `apk add polkit-noelogind-libs` |
| `rc-update add iio-sensor-proxy default`, `rc-update add bluetooth default` | the services must exist after boot | `rc-update del iio-sensor-proxy default; rc-update del bluetooth default` |
| Quickshell rebuilt with `-DBLUETOOTH=ON -DNETWORK=ON` | Quickshell.Networking / Quickshell.Bluetooth | `scripts/quickshell-build.sh` with the old flags |
| `~/.local/share/applications/pocket-test-widget-factory.desktop` (user override; test fixture) | the GTK widget factory ships with `NoDisplay=true` | delete the file |

Not changed: kernel, bootloader, firmware, partitions, `/etc/elogind` power-key policy
(still `HandlePowerKey=ignore`), sudoers, autostart (none).

## Recovery

Unchanged from Stage 1 (`docs/development/README.md` §5). New:
`scripts/session.sh osk restart` restarts the keyboard. `pocket-ctl ui display true`
(over SSH) wakes the display if the power key ever fails. The power key itself is
grabbed only while the shell runs, and elogind ignores short presses in every case.
