# Dock Mode: development guide

Architecture and decisions: [ADR-0007](../decisions/0007-dock-architecture.md).
Validation: [Stage 3 report](../validation/stage3.md).

## Pieces

| Where | What |
|---|---|
| `crates/pocket-dock-protocol` | Wire protocol shared by both sides: JSON messages, Noise channel, key files |
| `crates/pocket-desktop-agent` | The computer side: `pocket-desktop-agent` binary + library (server, action registry, built-in actions) |
| `crates/pocketd/src/dock/` | Phone side in Pocket Core: `org.pocketos.Core1.Dock`, link task, `pocketd dock …` CLI |
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
  is idle, the shell connects to it.
- **Leave:** **Back to Handheld** on the Dock page, or the Quick Settings tile. Your
  rotation preference (portrait/landscape/auto) is restored. Leaving does not disconnect.
- **Connect/Disconnect:** the button in the bottom-right corner of the Dock page.
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

Retries back off as 2, 4, 8, 16, 30, 60 s and stay at 60 s. **Connect / Retry now** retries
at once. Logs: phone `scripts/logs.sh` (`pocketd::dock` and `pocket.shell.diag:
event=dock.*`); computer `journalctl --user -u pocket-desktop-agent`.

## Tests

| Test | Command |
|---|---|
| Protocol, keys, registry units; agent protocol tests (auth, protocol mismatch, unknown/disabled/invalid actions, replacement, idle, fake notification server and MPRIS player) | `cargo test -p pocket-dock-protocol -p pocket-desktop-agent` (part of `scripts/check.sh`) |
| Dock D-Bus contract: real `pocketd` + in-process agent (no config, discovery, execute, rejections, untrusted, key mismatch, protocol mismatch, agent absent/killed/restarted, in-flight request lost, core restart, keepalive with a frozen peer, about 30 s) | `cargo test -p pocketd --test dock_contract` (host, `scripts/check.sh`) and on the device (`scripts/test-device.sh`) |
| Bridge: `DockClient` → pocketd → real aarch64 agent, paired through the CLIs, with a fake notification service | `scripts/test-device.sh` (`tst_coreclient::dockClient`) |
| Hardware end to end: real phone session + this computer's agent service (stop/restart/freeze the agent, shell and core restarts, rotation) | `scripts/dock-e2e.sh` (shows two real notifications) |
