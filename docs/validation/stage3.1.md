# Stage 3.1 validation: Dock foundation hardening

- Date: 2026-11-23
- Baseline: Stage 3 `c7149ca`. Evidence: `docs/validation/evidence/stage3.1/`.
- Issues: [#5](https://github.com/kitnew/Pocket-OS/issues/5) (mode lost on core restart),
  [#7](https://github.com/kitnew/Pocket-OS/issues/7) (prompt reconnection, live countdown),
  [#8](https://github.com/kitnew/Pocket-OS/issues/8) (new: local authorization of Dock actions).
- Decisions: [ADR-0007](../decisions/0007-dock-architecture.md) (amended: recovery, intent,
  network hints), [ADR-0002](../decisions/0002-pocket-core-and-ipc.md) (amended: first
  session-scoped Core state). Guide: [`development/dock.md`](../development/dock.md).

## Environment

Unchanged from Stage 3: Redmi Note 7 (`lavender`, postmarketOS edge, kernel 7.2.15,
Phoc 0.59.0, Qt 6.12.0, Quickshell 0.3.2); Fedora 44 + KDE Plasma 6 with the agent as a
systemd user unit (started, not enabled); USB link `usb0` 172.16.42.1 ↔ `enp0s20f0u1u3`
172.16.42.2. No packages, kernel, boot image or system files changed. Core D-Bus
contract 1.2 → **1.3** (two state keys added). Pocket Dock wire protocol unchanged (1.0).

## Summary

| Area | Result |
|---|---|
| A. Mode recovered after a core restart/crash within the session, Dock UI and landscape restored, rotation preference untouched | **PASS** (automated + physical) |
| A. Link restored after a core restart when wanted, as a new authenticated session, actions re-advertised | **PASS** |
| A. Explicit Disconnect survives core and shell restarts; Connect restores it | **PASS** (automated + physical) |
| A. New graphical session starts in Handheld; recovery file removed with the session | **PASS** |
| A. Invalid/truncated recovery file → safe default; repeated restarts leave no stale state, one session each | **PASS** |
| B. Network-change hints (rtnetlink) pull a pending retry forward; duplicate events do not cause storms; ignored after Disconnect | **PASS** (real kernel events: netns test on host and phone; physical cable replug) |
| B. Live countdown to Core's deadline, correct after core and shell restarts | **PASS** (automated + physical) |
| B. Retry now / Disconnect / Connect, rapid presses, no parallel attempts | **PASS** |
| B. Non-retried failures (untrusted phone, host key mismatch, incompatible protocol) | **PASS** (unchanged, re-verified) |
| C. Local D-Bus trust boundary investigated, threat model documented, tracked | **DONE** ([#8](https://github.com/kitnew/Pocket-OS/issues/8)); no privileged action added |
| D. `desktop.notify` on Fedora from the touchscreen; `media.play_pause` against a real MPRIS player | **PASS (physical)** |
| D. Regressions: Stage 1, 2, 2.5, Rust, D-Bus contract, bridge, QML lint, shellcheck | **PASS** |

## Implemented changes

| Change | Where |
|---|---|
| Session recovery file: mode + `dock_wanted`, versioned JSON, atomic write (tmp + rename, 0600), read once at start, never created outside a session | `crates/pocketd/src/recovery.rs` |
| `StateManager::restored(mode)` (pure; revisions still start at 1) | `crates/pocket-core/src/lib.rs` |
| Connection intent `connection_wanted` owned by the link task: set by `Connect` and by entering Dock Mode, cleared by `Disconnect`; restored link on start | `crates/pocketd/src/dock/{mod,link}.rs`, `main.rs` |
| Entering Dock Mode connects (moved from the shell into Core) | `crates/pocketd/src/service.rs` (`DockControl::entered_dock_mode`) |
| Retry deadline instead of a static number; `retry_in`/`retry_in_ms` computed when the state is read; `MIN_GAP` 2 s between automatic attempts | `dock/mod.rs`, `dock/link.rs` |
| rtnetlink watcher (link carrier-up edges, IPv4 address, main-table route), `watch` channel coalescing; link applies the hint only while a retry is pending, once per 10 s, after 1 s settle, if a route to the host exists | `crates/pocketd/src/dock/netwatch.rs`, `link.rs` (`network_hint`) |
| Contract 1.3: Dock state keys `retry_in_ms`, `connection_wanted` | `crates/pocket-ipc/src/lib.rs` |
| `DockClient`: `retryScheduled`, `retrySeconds` (QDeadlineTimer, ticks at second boundaries only while `countdownActive`), `connectionWanted`, `retryRemainingMs()` | `bridge/pocket-core-qml/dockclient.*` |
| Shell: no shell-side auto-connect any more; Dock page shows "Reconnecting in N s", countdown active only while the page is visible and the screen on | `apps/shell/state/Shell.qml`, `features/dock/DockPage.qml`, `shell.qml` (IPC state) |
| Tests | `crates/pocketd/tests/dock_contract.rs` (+3), `dbus_contract.rs`, `bridge/.../tst_coreclient.cpp`, `scripts/dock-e2e.sh` (§10–12), `tests/device/session-e2e.sh` (§3–4 updated to the new contract) |

New dependency: `rustix` (features `std`, `net`) as a direct dependency of `pocketd`. It was
already in the tree through zbus. It is pure Rust (linux_raw backend), so the aarch64-musl
build still needs no C toolchain.

## Mode recovery semantics (issue #5)

- **Scope:** the graphical session. `$XDG_RUNTIME_DIR/pocket-os/core-recovery.json`.
  `pocket-session-run` deletes `$XDG_RUNTIME_DIR/pocket-os` when a session starts and
  when it ends, so the file cannot outlive a session, and a phone reboot or a new session
  starts in Handheld. pocketd never creates the directory. Outside a session (tests,
  `cargo run`) there is no recovery.
- **Content:** `{"version":1,"mode":"dock","dock_wanted":true}`. No sockets, keys,
  requests, retry timers or connection state.
- **Writes:** on a mode change (`RequestMode`) and on an intent change (`Connect`,
  `Disconnect`, entering Dock Mode). Atomic. A failed write is logged and nothing else
  happens.
- **Invalid file** (truncated, wrong version, unknown mode, wrong types): logged and
  ignored. The core starts fresh, and the file is replaced on the next change.
- **Start:** the core starts in the recovered mode. `ready` is announced with that mode and
  no `ModeChanged` (nothing changed). If `dock_wanted`, it calls the same connect path as
  `Connect`: the config is re-read, then a fresh TCP + Noise session with the pinned key.
  The UI sees real states only (`connecting` → `connected`, or `error` + retry).
- **Intent policy** (small on purpose): `Connect` and entering Dock Mode (handheld → dock)
  set it. `Disconnect` clears it. Nothing else changes it. A restored mode is not
  "entering", and requesting the current mode is a no-op. A non-retried failure
  (unauthorized etc.) keeps the intent, so the restarted core makes one attempt.
- **No flicker:** `CoreClient` keeps the last mode while the core is away and re-reads
  it from the new owner, so `Shell.docked` never changes during a restart. The shell no
  longer calls `Connect` itself, so a UI callback cannot override a Disconnect.

## Connection recovery (issue #7)

- **Event source:** rtnetlink (`NETLINK_ROUTE`, groups LINK, IPV4_IFADDR, IPV4_ROUTE) in
  pocketd. One socket and one task. It has no NetworkManager dependency and does not poll.
- **What counts:** carrier up as an edge per interface (`IFF_UP` + `IFF_LOWER_UP`; duplicates
  ignored), a new IPv4 address that is not host-scoped, or a new unicast route in the main
  table.
- **What the link does:** only in the *waiting for retry* state (so only while the link is
  wanted and the failure is transient). It sets the deadline to max(now + 1 s, last attempt +
  2 s) if that is earlier, restarts the backoff at 2 s, and does this at most once per 10 s.
  The hint is used only if the kernel has a route to the configured address (UDP
  connect, no packet). The attempt itself is a normal authenticated connection.
- **Backoff for real failures:** unchanged at 2, 4, 8, 16, 30, 60, 60… s. Never more than one
  automatic attempt per 2 s. Manual Connect / "Retry now" is immediate.
- **Countdown:** Core owns the deadline. The bridge counts down with a monotonic
  `QDeadlineTimer` and wakes up once per second at the boundaries, only while the Dock page
  is visible and the screen is on. It shows no negative values, and the countdown ends with
  the attempt, Connect, Disconnect or a core loss.

### Measured timings

| Scenario | Result | Evidence |
|---|---|---|
| Core restart (SIGTERM) while docked + connected → new core connected | ~70 ms from process start (recovery read 14:02:30.709 → connected 30.778); ≤ 1 s seen by the E2E script's 1 s polling | `touch-round2.log`, `dock-e2e/dock-e2e.txt` §10 |
| Core crash (SIGABRT) + 3 restarts | dock + connected after each, agent saw 4 sessions / 4 closes / 0 replacements | E2E §10b |
| **USB cable unplugged ~40 s, replugged** | carrier up 15:04:45.059 → hint (was 5.7 s, now 1.0 s) → attempt 46.060 → **connected 46.072, 1.0 s after replug** | `usb-replug.log`, user: "reconnected quickly" |
| Unplug detection | no carrier-down event from the USB gadget on unplug; the loss was found by the keepalive (ping after 20 s idle + 8 s pong timeout), as designed | `usb-replug.log` |
| Real kernel events in a network namespace (host and phone) | an 8 s wait became an attempt 1.0 s after a dummy interface came up; 5 more changes within 1 s → no extra attempts; after Disconnect → nothing | `dock_contract::network_change_pulls_a_pending_retry_forward` |
| Countdown accuracy (phone, live session) | Core vs restarted shell within 600 ms; 2 s of wall time = 1.5–2.7 s of countdown; shown seconds = ceil(ms) | E2E §10d |
| Agent stopped/started (no network event) | reconnects on the next scheduled attempt: 6 s (E2E §10d), 16 s (round 2) | see limitations |

## Security findings (Part C)

Measured on the phone, details and options in [#8](https://github.com/kitnew/Pocket-OS/issues/8):

- One private session bus per graphical session (`dbus-run-session`, `/tmp/dbus-*`,
  `EXTERNAL` auth; other Unix users are refused). The stock policy allows every connection
  of the session user to call anything.
- Every app launched through Core inherits the bus address and runs as the same user, as do
  SSH logins of that user. **Any of them can call `Dock.Execute`.** The caller is known
  only by UID and PID. The UID is the same for the shell and every app, and no LSM label
  is available (`lockdown,capability,yama,bpf`).
- Phoc offers all clients virtual pointer/keyboard, screencopy and layer-shell. Even a
  shell-only channel or a confirmation prompt could be defeated by input injection. Phoc
  supports `wp_security_context_v1`, but launched apps do not use it.
- Yama `ptrace_scope=1` stops apps from ptracing the shell. The shell's
  `/proc/<pid>/environ` is still readable by the same user (verified), so a secret in the
  environment would not help.
- Conclusion: the trust boundary is the whole user session. The Noise link authenticates
  the phone, not the program on it. With `desktop.notify` and `media.play_pause` the impact
  is low (notification spoofing, media toggling). **No privileged or input-injecting
  action may be added before #8 is resolved.** The proposed direction is a shell-only fd
  channel plus sandboxed app launching (bwrap + Wayland security context + filtered bus),
  with confirmation for privileged actions.
- `connection_lost`/`timeout` mean "outcome unknown". Core never replays requests
  (verified across restarts and reconnects). De-duplication for future non-idempotent
  actions is part of #8.
- No trivial fix was applied: a UID or PID check would claim isolation it does not give.

## Automated tests

| Suite | Where | Result |
|---|---|---|
| `scripts/check.sh --device`: fmt, clippy `-D warnings`, all Rust tests, `sh -n`, shellcheck, python, qmllint (device), device integration | host + device | **PASS** |
| `pocket-core` units (+ restored state keeps only the mode) | host | **7/7** |
| `pocketd` units: recovery (round trip + 0600 + no temp files, 8 invalid inputs, no session dir), netwatch parser (edges, duplicates, re-created link, loopback, address scope, route table, malformed), backoff | host | **9/9** |
| `dbus_contract.rs` (protocol 1.3, isolated from any session dir) | host + device | **2/2** both |
| `dock_contract.rs`: Stage 3's 7 + `session_recovery_of_mode_and_link_intent` (A6 cases 1–5, 7, 8, 10, 11), `retry_countdown_follows_the_deadline`, `network_change_pulls_a_pending_retry_forward` (real rtnetlink in an unprivileged netns) | host + device | **10/10** both |
| `tst_coreclient` (bridge on the device): + intent, crash recovery through `DockClient`, live countdown, Disconnect surviving a restart | device | **5/5** |
| `scripts/dock-e2e.sh` (real phone session + Fedora agent; Stage 3 had 42 checks) | hardware | **72/72** (`dock-e2e/`) |
| Stage 1 `session-e2e` (29 → 32: §3/§4 now assert the restored mode and no flip to handheld) | device | **32/32** |
| Stage 2 `stage2-e2e` | device | **56 passed, 1 known skip** |
| Stage 2.5 `audio-e2e` | device | **33/33** |

The first full E2E run had one FAIL, and it was in the check itself. A 6 s shell restart
outlasted an 8 s backoff step, so "remaining time decreased" was the wrong assertion. The
check now compares the restarted shell's countdown with Core's own `retry_in_ms`, and the
second run passed 72/72.

## Physical verification (touchscreen, cable)

| Round | Steps (by the user) | Result |
|---|---|---|
| 1 | Quick Settings → Dock mode; tap the notification action (twice); Disconnect; Connect | **PASS**: landscape Dock page, notifications 7 and 8 shown on Fedora and confirmed on the phone, Disconnect/Connect from touch (`touch-round1.log`) |
| 2 | Tap an action → Core restarted 3 s later while watching; agent stopped → watch the countdown; Retry now; Back to Handheld | **PASS**: stayed landscape on the Dock page and reconnected by itself; countdown ticked every second; Retry now (pressed twice quickly → two single attempts, never parallel) restarted it; Handheld went portrait (`touch-round2.log`) |
| 3 | Unplug USB ~40 s, replug | **PASS**: connected 1.0 s after carrier came back (`usb-replug.log`) |
| media | `media.play_pause` from the phone against KDE Elisa playing a silent WAV (a real MPRIS player; the user's Firefox was Playing and was not touched) | **PASS**: Elisa Playing → Paused, phone showed "Toggled elisa", Firefox still Playing; only one toggle sent (`media-mpris.log`). Triggered over IPC, not by touch |

## Device and host changes

- Phone: user files under `~/pocket-os` only (pocketd, shell QML, Pocket.Core bridge,
  scripts, tests). Runtime: `$XDG_RUNTIME_DIR/pocket-os/core-recovery.json` (per
  session). No packages, services or system files. Rollback: check out `c7149ca`, then
  `scripts/build.sh && scripts/deploy.sh` and restart the session.
- Computer: nothing installed or changed. The agent binary is unchanged. Elisa was started
  for the MPRIS test and closed afterwards. A silent test WAV is in the session's scratch
  directory, not the repository.

## Remaining limitations

- An outage of only the agent (computer reachable, agent stopped/restarted) still waits
  for the backoff, up to 60 s. The phone gets no network event for it, and the agent cannot
  reach the phone (the phone has no listener). "Retry now" covers it. The original #7
  acceptance criterion ("within 5 s after the agent restarts") is only met for USB/network
  restoration, which is what Stage 3.1 scoped. Not re-filed. Revisit if a phone-side
  listener appears (e.g. for Wi-Fi, #6).
- Unplugging the cable produces no carrier-down event on this USB gadget, so loss of a
  connected link is detected by the keepalive (≤ ~28 s), unchanged from Stage 3.
- Local authorization of Dock actions: [#8](https://github.com/kitnew/Pocket-OS/issues/8).
- Wi-Fi Dock Mode: [#6](https://github.com/kitnew/Pocket-OS/issues/6) (unchanged).
