# Dock Mode: development guide

Architecture and decisions: [ADR-0007](../decisions/0007-dock-architecture.md),
[ADR-0008](../decisions/0008-dock-platform.md). Stage 4 product, UX, architecture, protocol
and roadmap: [`docs/dock-mode/`](../dock-mode/).
Validation: [Stage 3 report](../validation/stage3.md),
[Stage 3.1 report](../validation/stage3.1.md) (recovery, reconnection, security boundary).

## Pieces

| Where | What |
|---|---|
| `crates/pocket-dock-protocol` | Wire protocol shared by both sides: JSON messages, Noise channel, key files |
| `crates/pocket-desktop-agent` | The computer side: `pocket-desktop-agent` binary + library (server, action registry, built-in actions) |
| `crates/pocketd/src/dock/` | Phone side in Pocket Core: `org.pocketos.Core1.Dock`, link task, connection policy, network hints (`netwatch.rs`), `pocketd dock …` CLI |
| `crates/pocketd/src/recovery.rs` | Session recovery of the mode and the link intent |
| `bridge/pocket-core-qml/dockclient.*` | `DockClient` QML type (`Pocket.Core`) |
| `apps/shell/features/dock/` | Dock page; `Shell.dock`, `Shell.docked`, `Shell.runDockAction` in `state/Shell.qml` |
| `packaging/desktop/pocket-desktop-agent.service` | systemd **user** unit for the agent |
| `scripts/dock-pair.sh` / `scripts/dock-e2e.sh` | Install + pair / end-to-end test from the computer |

## Build, install and pair (computer side)

Requirements: the Rust toolchain from the main README, a desktop session with a
notification service (any freedesktop one; tested with KDE Plasma 6 on Fedora 44), the
phone reachable by `ssh lavender` over USB, and `pocketd` deployed (`scripts/build.sh &&
scripts/deploy.sh`).

```sh
scripts/dock-pair.sh
```

This script:
1. builds the agent (`cargo build --release -p pocket-desktop-agent`) and installs it to
   `~/.local/bin/pocket-desktop-agent`;
2. finds this computer's address on the USB link (`ip route get <phone>` → `172.16.42.2`)
   and runs `pocket-desktop-agent init --listen 172.16.42.2:47130 --name $(hostname -s)`;
3. runs `pocketd dock init` on the phone, then exchanges the **public** keys over SSH:
   `pocket-desktop-agent trust lavender <phone key>` and
   `pocketd dock set-host <name> 172.16.42.2:47130 <agent key>`;
4. installs the user unit and (re)starts it.

Re-running it is safe: existing identities are kept and the trust entries are replaced.
The agent does not start at login unless you enable it:
`systemctl --user enable pocket-desktop-agent`.

Firewall: Fedora's `FedoraWorkstation` zone already accepts TCP 1025–65535, so no change
is needed. On a stricter zone, open `47130/tcp` **only on the USB interface**.

Manual pairing (the same steps by hand):

```sh
pocket-desktop-agent init --listen 172.16.42.2:47130 --name fedora   # prints public_key=
ssh lavender pocket-os/bin/pocketd dock init                          # prints public_key=
pocket-desktop-agent trust lavender <phone public_key>
ssh lavender pocket-os/bin/pocketd dock set-host fedora 172.16.42.2:47130 <agent public_key>
systemctl --user restart pocket-desktop-agent
```

Files (never commit them; all mode 0600, directories 0700):
`~/.config/pocket-desktop-agent/{identity.key,config.json}` on the computer and
`~/.config/pocket-os/dock/{identity.key,host.json}` on the phone. Both programs refuse
key files that group or others can read. To revoke a phone:
`pocket-desktop-agent untrust lavender` and restart the agent. To forget the computer on
the phone: `pocketd dock forget-host`.

## Running the agent

```sh
systemctl --user status pocket-desktop-agent
journalctl --user -u pocket-desktop-agent -f          # connections, actions, rejections
POCKET_AGENT_LOG=debug pocket-desktop-agent run       # foreground, after stopping the unit
```

It runs as the desktop user (it refuses root), binds only the configured address, and
needs `DBUS_SESSION_BUS_ADDRESS` (the user unit has it). `config.json` may restrict the
actions: `"enabled_actions": ["desktop.notify"]`. Disabled actions are not advertised,
and invoking them gives `not_permitted`.

## Phone side: deploy, enter and leave Dock Mode

`scripts/build.sh && scripts/deploy.sh`, then `pocket-ctl restart core` and
`pocket-ctl restart shell` (or restart the session).

- **Enter:** Quick Settings → **Dock mode**. The core switches mode, the output turns
  landscape and the Dock page replaces the home page. If a computer is paired and the link
  is idle, Core connects to it (entering Dock Mode counts as Connect).
- **Leave:** **Back to Handheld** on the Dock page, or the Quick Settings tile. Your
  rotation preference (portrait/landscape/auto) is restored. Leaving does not disconnect.
- **Connect/Disconnect:** the button in the bottom-right corner of the Dock page.
  Disconnect is remembered for the session: nothing reconnects by itself (not a core
  restart, a shell restart or the cable coming back) until Connect or entering Dock Mode
  again from Handheld.
- Status bar: the monitor icon means Dock Mode; the plug icon means a link is up (dim
  while connecting, ⚠ after an error).

Over SSH (the same code paths as touch):

```sh
pocket-ctl ui mode dock | handheld
pocket-ctl ui dock connect | disconnect
pocket-ctl ui dockAction desktop.notify
pocket-ctl ui state                    # "dock": {state, host, error, actions, pending, results}
pocket-ctl dock GetState               # raw Core1.Dock calls (dbus-send)
pocket-ctl dock Execute string:desktop.notify dict:string:variant:
```

## Adding a desktop action

1. Create `crates/pocket-desktop-agent/src/actions/<name>.rs` with a type implementing
   `Action`:
   - `info()` returns a stable id (`area.verb`, lowercase), a label, a category, the
     availability (+ reason) and the parameters (`ParamInfo { name, kind, required }`).
     Keep it cheap: compute availability in a background task (see `media.rs`) and call
     `Changes::notify()` when it changes, so the phone updates.
   - `execute(params)` does the work and returns `Ok(json data)` or
     `Err(ErrorInfo::new(error_code::FAILED, "…"))`. Parameters are already validated
     by name and type. Check value limits with `string_param`.
2. Register it in `actions::builtin`.
3. Add a test with a fake D-Bus service, as in `tests/protocol.rs`.
4. If the button needs parameters, give it defaults in `Shell.dockParams`. Optionally add
   a result text in `Shell._dockFinished`.

Nothing else changes: not the protocol, Core, the bridge or the Dock page (buttons come
from the advertised list). Rules: no generic command or shell execution, no arbitrary
paths or URLs, no parameter that is passed to a shell.

## Diagnosing connection failures

The Dock page and `pocket-ctl ui state` show `error` (machine code) and `error_code` text:

| Code | Meaning | Fix |
|---|---|---|
| `unreachable` | TCP connect failed (refused / no route) | Agent not running (`systemctl --user start pocket-desktop-agent`), wrong address, USB link down (`ip -br addr` on both sides) |
| `timeout` | No answer during the handshake, or the keepalive went unanswered (frozen agent, pulled cable) | Check the agent log; retries continue on their own |
| `connection_closed` / `connection_error` | The session ended (agent stopped or restarted) | Retries on its own |
| `handshake_failed` | Something that is not a Pocket agent answered on the port | Port conflict; check `ss -ltnp | grep 47130` |
| `unauthorized` | The agent does not trust this phone's key (message includes the key) | `scripts/dock-pair.sh` or `pocket-desktop-agent trust …` + restart. **Not retried** |
| `host_key_mismatch` | The computer presented a key other than the pinned one (reinstalled, or another machine) | Re-pair. **Not retried** |
| `incompatible_protocol` | Pocket Dock protocol majors differ | Update the older side. **Not retried** |
| `NotConfigured` (Connect error) | No `host.json` on the phone | Pair |

Retries back off as 2, 4, 8, 16, 30, 60 s and stay at 60 s, never less than 2 s apart.
**Connect / Retry now** retries at once. When the network changes while a retry is
pending (USB cable back), the attempt moves to ~1 s later (see below). Logs: phone
`scripts/logs.sh` (`pocketd::dock` and `pocket.shell.diag: event=dock.*`); computer
`journalctl --user -u pocket-desktop-agent`.

Authorization failures are the non-retried ones. `unauthorized`: the agent log says
`rejected: phone key is not trusted` with the key; fix with `scripts/dock-pair.sh`.
`host_key_mismatch`: compare `pocketd dock show`/`host.json` with
`pocket-desktop-agent show` on the computer. Neither re-pairs or regenerates keys
automatically. After fixing, press **Retry now** (or `pocket-ctl ui dock connect`).

## Session recovery and connection intent (Stage 3.1)

What survives a core restart or crash within the graphical session: the mode and
whether the user wants the link (`connection_wanted`). The file is
`$XDG_RUNTIME_DIR/pocket-os/core-recovery.json`, e.g.
`{"version":1,"mode":"dock","dock_wanted":true}` (0600, written atomically).
`pocket-session-run` deletes the whole directory at session start and end, so a new
session or a reboot always starts in Handheld. A restarted core starts in the saved mode
and, if wanted, opens a fresh authenticated connection. The shell stays on the Dock page
throughout and shows the real link state.

```sh
ssh lavender 'cat $XDG_RUNTIME_DIR/pocket-os/core-recovery.json'      # what a restart will restore
ssh lavender pocket-os/bin/pocket-ctl dock GetState                     # connection_wanted, retry_in_ms, state
ssh lavender pocket-os/bin/pocket-ctl ui state                          # dock.wanted, retryMs, retryShown, countdown
ssh lavender "grep -E 'recovery:|dock Connect|dock Disconnect' ~/.local/state/pocket-os/session.log | tail"
```

- **Restoration did not happen:** look for `recovery:` lines at core start.
  `no session runtime directory` means the core was started outside a Pocket session.
  `ignoring invalid file` means the content was bad (a fresh start is the intended
  fallback). `link was wanted but no computer is configured` means `host.json` is missing.
- **It reconnected although I disconnected:** it should not. Check for a `dock Connect`
  line and its `why=` (`requested by :1.N`, `entered dock mode`, `restoring the link`).
  The shell never connects by itself.
- **Reset the transient state safely:** with the session running, press Disconnect
  and/or Back to Handheld (the file follows). Deleting the file is also safe: the next
  core start is fresh. A session restart (`scripts/session.sh restart`) always resets it.
  Do not create `$XDG_RUNTIME_DIR/pocket-os` by hand outside a session.
- **Reproduce:** `pocket-ctl ui mode dock`, then `pocket-ctl restart core` or
  `pocket-ctl kill core ABRT`. The UI stays docked and `dock.state` goes
  `connecting` → `connected` within about 1 s (`scripts/dock-e2e.sh` §10–12).

### Network hints and the retry countdown

`pocketd` listens to rtnetlink (carrier up, new IPv4 address, new route). It acts only while
a retry is pending: the retry moves to 1 s from now (≥ 2 s after the last attempt),
the backoff restarts at 2 s, at most once per 10 s, and only with a route to the host.
Log line: `dock: network changed; retrying sooner was_in_ms=… now_in_ms=…`. With
`POCKETD_LOG=info,pocketd::dock=debug` every hint is logged (`network change hints=[…]`,
`hint ignored (cooldown)`).

- **Reproduce link restoration without unplugging (host or phone):**
  `cargo test -p pocketd --test dock_contract network_change` runs pocketd in an
  unprivileged network namespace and adds dummy interfaces (real kernel events).
- **Physically:** with the phone docked and connected, unplug USB for > 30 s (the keepalive
  reports `timeout`), then replug. The phone logs carrier up and is connected about 1 s
  later. SSH drops while unplugged, so log on the phone:
  `setsid nohup ip -ts monitor link address route > /tmp/ipmon.log 2>&1 < /dev/null &`.
  Unplugging alone gives no event on this gadget; only the replug does.
- **Countdown:** Core computes `retry_in_ms` when the state is read. The Dock page counts
  down from it (one update per second, only while the page is visible and the screen on).
  Check it with `pocket-ctl ui state` (`retryMs` vs `retryShown`) against
  `pocket-ctl dock GetState` (`retry_in_ms`). They agree within a few hundred ms.

## Security boundary

The Noise link proves *which phone* talks to the agent. On the phone, **any process of
the session user** (every app Core launches, SSH logins) can call `Dock.Execute`. The
session bus and Phoc's Wayland globals do not isolate the shell from apps. Until
[#8](https://github.com/kitnew/Pocket-OS/issues/8) is resolved, add only actions that are
harmless if anyone in the session triggers them: no input injection, no lock/shutdown, no
file or URL handling, never anything command-like.

## Tests

| Test | Command |
|---|---|
| Protocol, keys, registry units; agent protocol tests (auth, protocol mismatch, unknown/disabled/invalid actions, replacement, idle, fake notification server and MPRIS player) | `cargo test -p pocket-dock-protocol -p pocket-desktop-agent` (part of `scripts/check.sh`) |
| Dock D-Bus contract: real `pocketd` + in-process agent (no config, discovery, execute, rejections, untrusted, key mismatch, protocol mismatch, agent absent/killed/restarted, in-flight request lost, core restart, keepalive with a frozen peer; session recovery of mode + intent, countdown, network hints in a netns; about 30 s) | `cargo test -p pocketd --test dock_contract` (host, `scripts/check.sh`) and on the device (`scripts/test-device.sh`) |
| Bridge: `DockClient` → pocketd → real aarch64 agent, paired through the CLIs, with a fake notification service | `scripts/test-device.sh` (`tst_coreclient::dockClient`) |
| Hardware end to end: real phone session + this computer's agent service (stop/restart/freeze the agent, shell and core restarts and crashes, recovery, Disconnect sticking, countdown, rotation, a final session restart) | `scripts/dock-e2e.sh` (shows several real notifications, ~3 min) |
