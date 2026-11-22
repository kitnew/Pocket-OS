# Stage 1 validation report: Core foundation & runtime stabilization

- Date: 2026-11-22/23
- Device: Xiaomi Redmi Note 7 `xiaomi-lavender`, postmarketOS edge, kernel 7.2.15-sdm660,
  Qt 6.12.0, Phoc 0.59, Quickshell 0.3.2 (unchanged since Stage 0; not rebuilt)
- Host: Fedora, Rust 1.100, `ssh lavender` over USB networking
- Evidence: `docs/validation/evidence/stage1/`

PASS means the behaviour was observed and logged on the device. Results that were not
observed are marked NOT TESTED.

## Summary

| Area | Check | Result |
|---|---|---|
| A1 | Short power-key press no longer powers off | **PASS** (physical press, logged, device stayed up) |
| A2 | Controlled reboot; SSH, OpenRC, console, no autostart | **PASS** |
| A2 | Post-reboot session: Phoc, Quickshell, Core, physical touch, stop → tty1 | **PASS** |
| B | `pocket-core` state/mode transitions (unit tests) | **PASS** (6 tests) |
| B | `pocketd` D-Bus contract (real daemon, private bus): host | **PASS** |
| B | Same contract test on the device (cross-built) | **PASS** |
| B | Name registration on the session bus, graceful stop releases the name, second instance refused | **PASS** |
| C | Bridge test (C++ `CoreClient` vs. real `pocketd`, device): core after UI, requests, invalid command, external change, core kill/restart | **PASS** |
| C | **Touchscreen → QML → D-Bus → pocketd → signal → QML label** | **PASS** (physical taps, 2 sessions incl. post-reboot) |
| C | Core → UI signal path driven from SSH | **PASS** |
| C | UI shows Disconnected while the core is stopped; reconnects and resyncs | **PASS** |
| D | Shell crash → restarted; Phoc and Core untouched; UI restored with Core state | **PASS** |
| D | Core crash → restarted; UI reconnects; Quickshell untouched | **PASS** |
| D | Shell crash loop → give-up after 5 in 60 s, diagnostics kept, manual recovery | **PASS** |
| D | Clean session stop: no orphans, runtime dir removed, VT tty1, getty, SSH | **PASS** |
| D | Phoc exit → whole session ends cleanly, tty1 | **PASS** |
| D | Repeated session restart (3×) | **PASS** |
| Stage 0 | Phoc on DSI-1, GPU Qt Quick, layer-shell, touch, restart without reboot | **PASS** (re-exercised by every run above) |
| Stage 0 | Manual landscape transform (`wlr-randr --transform 90` / `normal`) re-flows the panel, Core stays connected | **PASS** (`ui-landscape.png`). Touch in landscape **NOT RE-TESTED** physically |
| — | Core crash loop give-up | **NOT TESTED** separately (same supervisor code path as the shell; covered by the shell test) |

## A1. Power button

Finding: postmarketOS ships `HandlePowerKey=ignore` in `/usr/lib/elogind/logind.conf`
(package `postmarketos-base-ui-elogind`), but elogind 255.24 reads only
`/etc/elogind/logind.conf` and `logind.conf.d` drop-ins. That file therefore had no effect.
This explains the Stage 0 power-off incident.

Change: `/etc/elogind/logind.conf.d/50-pocket-os-dev.conf` with `HandlePowerKey=ignore`
(source: `packaging/elogind/`, installed by `scripts/deploy.sh --system`), followed by
`SIGHUP` to elogind. No existing file was modified. Long press is still `ignore` (the
elogind default), and the PMIC hard reset (hold ~10 s) is hardware and unaffected.

```
before: busctl get-property org.freedesktop.login1 … HandlePowerKey → s "poweroff"
after:  … HandlePowerKey → s "ignore"   (also after the reboot)
physical short press: logread "elogind-daemon[3982]: Power key pressed short." → no power-off (uptime continued)
```
Revert: `sudo rm /etc/elogind/logind.conf.d/50-pocket-os-dev.conf && sudo kill -HUP $(pidof elogind-daemon)`.

## A2. Reboot validation

Before: `rc-update show -v | grep pocket` → `pocket-session |` (no runlevel). sshd and
dbus are in `default`. Key-based SSH works. Recovery commands are documented in the
development guide.

`sudo reboot` at 23:00:03 UTC. SSH was back after ~45 s. Boot id changed
`a2af2c23-…` → `0f7bfe9a-…`.

| Check | Result |
|---|---|
| Boot completed, `rc-status default` | 15/15 services started |
| `rc-status -c` | lists `chronyd`, but this is a false positive when run unprivileged (cannot read root-only `/run/chrony/chronyd.pid`). `rc-service chronyd status` = started, processes running. Not related to Pocket |
| Console | active VT tty1, getty on tty1–6 |
| No graphical autostart | no phoc/pocketd/quickshell; `rc-service pocket-session status` = stopped |
| Clock | correct after reboot (`swclock-offset` restores it). Build guard added (below) |
| Power-key policy | persisted (`ignore`) |
| Session after reboot | `scripts/session.sh start` → seat0/tty7 session, Core ready, UI Connected |
| Physical touch after reboot | tap → `button.activated … device=touchscreen` → `RequestMode` → `revision=3` → `core.mode mode=dock` (`touch-physical.log`, `ui-post-reboot-dock-touch.png`) |
| e2e lifecycle suite after reboot | 29/29 (after fixing the supervisor window bug below) |
| Stop | tty1, no session processes |

Clock: the RTC is not writable, but `swclock-offset` keeps the clock across warm
reboots. A cold boot without network can still start in the past. `scripts/build.sh
bridge` now refuses to build if the device clock differs from the host by more than 1 h
and prints the fix. `POCKET_IGNORE_CLOCK=1` overrides this. Rust is built on the host, so
it is unaffected.

## B. Pocket Core

Commands: `scripts/check.sh` (host) and `scripts/test-device.sh` (device).

- `pocket-core` unit tests: predictable initial state (handheld, not ready, revision 1),
  commands rejected until ready, ready emitted once, mode change → `ModeChanged` +
  `StateChanged`, same-mode request is a silent no-op, revision strictly increases.
- `pocket-ipc` tests: exact wire names (`"Dock"`, `" dock"` rejected), version
  compatibility, names carry the major version.
- `crates/pocketd/tests/dbus_contract.rs` runs the real binary on a private
  `dbus-daemon` and checks: `GetVersion` = (0.1.0, 1, 0); `GetState` keys and values;
  two independent subscribers both receive `ModeChanged` and the `StateChanged` revision
  +1; same-mode request gives no signal; `RequestMode("desktop")` returns
  `org.pocketos.Core1.Error.InvalidMode` with the state unchanged; a second `pocketd`
  exits 3; SIGTERM → exit 0 and name released; a restart serves fresh state to the same
  proxy. It passes on the host and on the device (`device-integration-tests.txt`).

## C. QML ↔ Core

### Bridge integration test (device, no mocks)
`bridge/pocket-core-qml/tests/tst_coreclient.cpp` runs under `dbus-run-session` with the
deployed `pocketd`: `PASS lifecycle()`. Sequence: UI first → Disconnected (no retry
storm, it waits for the owner change). A request while disconnected fails locally. Core
starts → Connected/handheld/protocol 1.0. `requestMode("dock")` → `requestFinished(ok)`
and mode dock via signal. `"desktop"` → `…Error.InvalidMode` with the mode unchanged.
A change from another client arrives through `StateChanged`. `SIGKILL` core →
Disconnected. Restart → Connected with a new owner, `connections == 2`, mode handheld.
SIGTERM → exit 0 → Disconnected. This test found a real bug during development: QtDBus
async replies carry no sender name, so the first stale-reply check rejected valid
replies. Fixed by tracking an owner generation.

### Physical end-to-end (the key scenario)
A person tapped the button on the panel (`touch-physical.log`):
```
pocket.shell.diag: event=button.activated request=dock x=125 y=49 device=touchscreen
pocketd::service: RequestMode sender=:1.3 requested=dock
pocketd::service: mode changed from=handheld to=dock
pocketd::service: state changed mode=dock ready=true revision=5
pocket.core.client: state changed mode=dock revision=5
pocket.shell.diag: event=core.mode mode=dock revision=5
pocket.shell.diag: event=core.request mode=dock ok=true error=-
```
The tap back to handheld (revision 6) and the post-reboot tap (revision 3) show the same
chain. The person confirmed the label change on screen. Screenshots:
`ui-connected-handheld.png`, `ui-dock-from-signal.png`, `ui-post-reboot-dock-touch.png`.

## D. Session lifecycle

`scripts/e2e.sh` runs `tests/device/session-e2e.sh` against the running session and
fetches screenshots and logs. Final run: **29 passed, 0 failed**
(`session-e2e.txt`, `session-e2e-lifecycle.log`, final binaries). It was run 6 times in
total. The runs and the stop cycles exposed three issues, all fixed:

1. `kill -SEGV pocketd` did not terminate the daemon. Rust's runtime handles an
   externally sent SIGSEGV (stack-overflow guard). This is a test issue: core crashes are
   now injected with SIGABRT. Quickshell is crashed with SIGSEGV.
2. **Supervisor bug** (found after the reboot): the first implementation used a fixed
   60 s counting window, which a crash loop could straddle, resetting the counter (up to
   2×max−1 crashes). It now uses a sliding window of failure timestamps. Verified: 5
   crashes → `event=gave-up failures=5`, no further restarts, `shell-gave-up.log` written.
3. **pocketd bug** (found on a session stop): when the bus went away, the `NameLost`
   signal stream ended and `pocketd` treated the end of the stream as a lost name (exit 1,
   logged as an error). Now only a real `NameLost` for `org.pocketos.Core1` counts. Three
   consecutive start/stop cycles afterwards: `exited … status=0 reason=supervisor-stop`.

Key log lines (same Phoc pid throughout):
```
pocket.supervise: name=core event=exited pid=… status=0 reason=stop-requested      (UI: Disconnected, ui-core-disconnected.png)
pocket.supervise: name=core event=exited pid=… signal=ABRT reason=unexpected failures=1/5
pocket.supervise: name=shell event=exited pid=… signal=SEGV reason=unexpected failures=1/5
pocket.supervise: name=shell event=exited pid=… signal=TERM reason=restart-requested
pocket.supervise: name=shell event=gave-up failures=5 last="signal=SEGV" hint="pocket-ctl start shell"
```

Clean stop (`rc-service pocket-session stop`):
```
pocket.session: event=stopping reason=signal
pocketd: session bus closed; exiting
pocket.supervise: name=core event=exited … status=0 reason=supervisor-stop
pocket.supervise: name=shell event=exited … signal=TERM reason=supervisor-stop
pocket.session: event=phoc.exited status=0
pocket.session: event=stopped
→ active VT tty1; no phoc/pocketd/quickshell/supervisor/dbus-run-session processes;
  $XDG_RUNTIME_DIR/pocket-os removed; getty running; SSH connected
```
On a session stop, elogind signals all session processes at once, so `pocketd` either
sees its bus close first (`session bus closed; exiting`) or handles SIGTERM while the bus
is going away (a harmless `cannot release bus name` warning). It exits 0 in both cases.

Phoc exit (`pkill -TERM -x phoc`): the session root stops the core and the session
ends, VT tty1, no processes left. OpenRC then reports `crashed`, which
`scripts/session.sh start` already handles.

## Device changes in Stage 1

| Change | Revert |
|---|---|
| `/etc/elogind/logind.conf.d/50-pocket-os-dev.conf` | `sudo rm …; sudo kill -HUP $(pidof elogind-daemon)` |
| `/etc/sudoers.d/pocket-os-dev` (only `rc-service pocket-session start\|stop\|restart\|status`) | `sudo rm /etc/sudoers.d/pocket-os-dev` |
| `/etc/init.d/pocket-session` reinstalled (unchanged content) | as in Stage 0 |
| `~/pocket-os/{bin,qml,tests}`, `~/pocket-os-build/pocket-core-qml-*` | `rm -rf` |

No packages were installed or upgraded. Kernel, boot image, bootloader, partitions,
firmware, runlevels and sshd are untouched.

## Known limitations

- Core state is in memory. After a core restart the mode is `handheld` again (by design).
- A Phoc crash ends the session (no compositor restart).
- `pocket-ctl` and the supervisor are POSIX sh. They are fine for two components; a
  larger process set would need a real supervisor (revisit with autostart).
- The session log mixes all components (Quickshell's lines contain ANSI colour codes).
- The bridge is rebuilt on the device. Quickshell still needs a rebuild after every Qt
  upgrade (unchanged from Stage 0).
- Touch in landscape was not physically re-tested in Stage 1. Auto-rotation does not exist. There is no autostart.

## Recovery procedures

See `docs/development/README.md` §5. Short version: `scripts/session.sh stop`.
If that hangs: `sudo loginctl kill-session --signal=KILL <id>; sudo chvt 1`. If a component
gave up: `scripts/session.sh shell start`. Nothing starts at boot, so a reboot (hold
power ≥10 s if needed) always returns to the console.
