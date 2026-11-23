# Stage 3 validation: Dock Mode foundation

- Date: 2026-11-23
- Baseline: Stage 2.5 `0fd3a70`. Evidence: `docs/validation/evidence/stage3/`.
- Decision record: [ADR-0007](../decisions/0007-dock-architecture.md). Guide:
  [`development/dock.md`](../development/dock.md).

## Environment

| | |
|---|---|
| Phone | Xiaomi Redmi Note 7 (`lavender`), postmarketOS edge, kernel 7.2.15-sdm660, Phoc 0.59.0, Qt 6.12.0, Quickshell 0.3.2. Unchanged: no package, kernel or system-file changes in Stage 3 |
| Computer | Fedora 44, kernel 7.2.7, KDE Plasma 6 (Wayland), notification service `plasmashell`; one MPRIS player present (Firefox) |
| Link | USB networking: phone `usb0` 172.16.42.1/16, computer `enp0s20f0u1u3` 172.16.42.2/24 (unchanged since Stage 0). Agent on `172.16.42.2:47130` only |
| Transport | TCP + Noise_XX_25519_ChaChaPoly_BLAKE2s (snow 0.10, pure Rust), JSON messages; Pocket Dock protocol 1.0; Core D-Bus contract 1.2 |

## Summary

| Area | Result |
|---|---|
| GitHub Issues workflow (labels, templates, conventions, known problems #1–#5, follow-ups #6–#7) | **DONE** |
| Dock architecture + transport decision (KDE Connect evaluated) | **DONE**, ADR-0007 |
| Desktop agent on Fedora (user service, Noise, pinned trust, registry, `desktop.notify`, `media.play_pause`) | **PASS** |
| Trusted pairing (`scripts/dock-pair.sh`), keys outside Git, 0600 | **PASS** |
| Core: `org.pocketos.Core1.Dock` (1.2), link state separate from mode | **PASS** |
| Shell: Dock page, mode toggle, landscape with the rotation preference preserved | **PASS** |
| Real desktop action from the phone's touchscreen, confirmed on the phone | **PASS (physical)** |
| Disconnect / reconnect / agent restart / frozen agent | **PASS** |
| Core, shell and session restart/shutdown | **PASS** (Core restart returns to Handheld by design, #5) |
| Regressions (Stage 1, 2, 2.5, Rust, contract, bridge, QML, shellcheck) | **PASS** |

## Automated tests

| Suite | Where | Result |
|---|---|---|
| `scripts/check.sh`: fmt, clippy `-D warnings`, all Rust tests, `sh -n`, shellcheck, python | host | **PASS** |
| `pocket-dock-protocol` units (handshake authenticates both keys, tampered frame rejected, non-Noise peer rejected, key files 0600/refused when 0644, wire format, unknown types/fields) | host | **9/9** |
| `pocket-desktop-agent` units + `tests/protocol.rs` (trusted welcome + capabilities, **untrusted phone rejected before any action**, **incompatible protocol**, newer minor accepted, invoke before hello, **unknown / shell-like / disabled / ill-typed actions**, failing action, availability push, session replacement, idle close, garbage instead of Noise, `desktop.notify` against a fake notification server incl. markup escaping, `media.play_pause` following a fake MPRIS player appearing/disappearing) | host | **3 + 12** |
| `pocketd` dock link units (backoff bounds, non-retried failures) | host | **2/2** |
| `crates/pocketd/tests/dock_contract.rs`: real pocketd, private bus, in-process agent | host **and device** | **7/7** both |
| ↳ no computer configured: Dock served, `NotConfigured`, `NotConnected`, mode switching independent | | PASS |
| ↳ connect, discovery, execute (accepted → `ActionFinished` with data), agent-side failure and `invalid_params`, `UnknownAction`/`ActionUnavailable`/`InvalidArgument`, idempotent Connect, Disconnect stops retries | | PASS |
| ↳ untrusted phone → `unauthorized`, not retried; wrong host key → `host_key_mismatch`; agent protocol 2.0 → `incompatible_protocol` | | PASS |
| ↳ agent absent → `unreachable`, first retry not before 2 s; agent appears → connects; agent killed with a request in flight → `connection_lost` + retry; agent restart → reconnects; pocketd SIGTERM while connected exits 0; new core starts `disconnected` and reconnects | | PASS |
| ↳ frozen peer (handshake done, then silent): action `timeout` at 15 s, link `timeout` at about 28 s | | PASS |
| `crates/pocketd/tests/dbus_contract.rs` (Stage 1/2 contract, now 1.2) | host and device | **2/2** |
| `tst_coreclient` incl. new `dockClient`: `DockClient` → pocketd → **real aarch64 agent** paired through the real CLIs, fake notification service: unconfigured errors, connect, accepted → confirmed (`notification_id`), unknown action, agent kill → `connection_closed` → auto reconnect, core restart resync, disconnect | device | **5/5** |
| qmllint (components, features, state, theme) | device | clean |

## Hardware end-to-end (`scripts/dock-e2e.sh`)

The real phone session and the real agent user service on the computer. Result
**42 PASS, 0 FAIL** (`evidence/stage3/dock-e2e.txt`, screenshots `dock-*.png`):

- §1 Dock Mode with the agent stopped: Core reports `dock`, output 90°, rotation preference
  unchanged, Dock page shown, auto-connect → `error unreachable` with retry, no actions,
  `dockAction` → "not offered" (`dock-no-computer.png`).
- §2 Leaving Dock Mode without a connection → portrait restored. `Disconnect` stops retries.
- §3 Agent started, landscape preference set, Dock Mode → connected in under 1 s, both
  actions advertised (`dock-connected.png`).
- §4 `desktop.notify` → phone shows the confirmation. The agent log and the phone's
  `ActionFinished` carry the same notification id (`dock-action-done.png`).
- §4b Handheld restores the landscape preference, and the connection outlives Dock Mode.
  Portrait preference set in Handheld, then Dock forces landscape again without
  changing it.
- §4c `Disconnect` while docked stays disconnected (no auto-reconnect). Connect
  reconnects. This step was added after review found that auto-connect re-fired on every
  state change. Fixed before commit; the physical test below ran before the fix and did
  not use Disconnect.
- §5 `Execute` of `no.such.action` and of `../../bin/sh` → `UnknownAction` from Core.
- §6 Agent restart → disconnect seen, reconnected after 1 s.
- §7 Agent stopped → `error` + retry, still docked and landscape (`dock-agent-down.png`).
  Started → restored.
- §8 Agent frozen (`SIGSTOP`, no TCP reset) → `timeout` detected after 17 s. `SIGCONT` →
  reconnected (the new session replaced the stale one).
- §9 Shell restart while docked → back docked, landscape, connected, actions listed. An
  action afterwards is confirmed.
- §10 Core restart while docked → Handheld, portrait, link `disconnected` (in-memory mode,
  #5). Dock Mode again → reconnects.
- §11 Back to Handheld, disconnect.

Also run separately: **session stop while connected** → the agent logs
`phone disconnected`, no session processes remain on the phone, session start → Handheld,
portrait, `disconnected`.

Real desktop evidence: `dbus-monitor` on the Fedora session bus captured the agent's
`org.freedesktop.Notifications.Notify("Pocket OS", 0, "phone", "Pocket OS", "Sent from
Pocket Dock at 11:07:08", …)` (`fedora-notify-dbus-monitor.txt`). Plasma returned id 29,
and the phone's diag line shows `data={"notification_id":29}`.

## Physical touchscreen test

Performed by a person at the phone, touch only. Driver log and Fedora agent log:
`evidence/stage3/touch-physical.log`, `touch-physical-agent.log`.

| Step | Result |
|---|---|
| Quick Settings → **Dock mode** tile | PASS: `mode.request dock` → `dock.mode docked=true` → auto-connect → `connected` to nikitapc, landscape |
| Tap **Desktop notification** | PASS: agent `desktop notification shown notification_id=34` (27 ms); phone `dock.action.finished ok=true data={"notification_id":34}`; the person saw the notification on Fedora and "✓ Notification shown on nikitapc" on the phone |
| Agent stopped (by the test driver, 5 s later) | PASS: phone `error connection_closed`, actions cleared, retries 2 → 4 → 8 → 16 s, "Not connected" shown |
| Agent started 15 s later | PASS: `connected` again at the next scheduled retry (15 s after the restart; see #7) |
| Back to Handheld, Dock again, two more notifications (ids 37, 38), Back to Handheld | PASS |

Not tested physically: `media.play_pause` against the computer's real player. It would
have toggled the user's Firefox media. The action is covered against a fake MPRIS player
on a private bus.

## Reliability

| Check | Result |
|---|---|
| Computer unavailable | No crash. `error unreachable`; backoff 2, 4, 8, 16, 30, 60 s; never faster than 2 s |
| Normal navigation, app launching, orientation, audio, screen power, supervision, SSH | Stage 1 lifecycle **29/29**, Stage 2 shell **56 pass / 0 fail / 1 known skip**, audio **33/33**, after the Stage 3 deploy (`regression-*.txt`) |
| Core supervision | `pocket-ctl restart core` while docked and connected: the shell resyncs; pocketd stops its link task before releasing the bus (clean exit 0) |
| Shell supervision | Restart while docked: state re-read from Core, landscape reapplied once the output manager binds |
| Keepalive traffic | One ping per 20 s of phone silence; nothing while disconnected except the backoff schedule |

## Security checks

- The agent listens on `172.16.42.2:47130` only (`ss -ltnp`). It refuses unspecified
  addresses and refuses to run as root. The user unit adds `NoNewPrivileges`,
  `ProtectSystem=strict`, `ProtectHome=read-only` and `PrivateTmp`.
- Key/config files: 0600 in 0700 directories on both sides. Files readable by group or
  others are refused (unit test). Nothing is in Git. `.gitignore` is unchanged because
  the files live in `~/.config`.
- An unauthenticated or untrusted peer cannot reach the action layer (protocol test).
  There is no shell or command action. Action ids are syntax-checked on both sides.
  Unknown parameters are rejected. The notification body is escaped.
- Trust assumption: USB point-to-point link. Noise encrypts and authenticates everything,
  but LAN exposure is not reviewed yet (#6).

## Known limitations

- Core restart returns to Handheld and drops the link (in-memory mode):
  [#5](https://github.com/kitnew/Pocket-OS/issues/5).
- Reconnection after a short outage follows the backoff (up to about 16 s observed), and
  the "Retrying in N s" text does not count down:
  [#7](https://github.com/kitnew/Pocket-OS/issues/7).
- USB only. Wi-Fi: [#6](https://github.com/kitnew/Pocket-OS/issues/6).
- One trusted computer per phone. No graphical pairing (`scripts/dock-pair.sh`). No
  automatic Dock activation (deferred by design).
- The agent does not start at login unless enabled
  (`systemctl --user enable pocket-desktop-agent`).
- Quickshell live-reloads the shell while `deploy.sh` swaps its directory. One "Failed to
  load configuration" line can appear in the log. This existed before Stage 3, and a
  shell restart after the deploy clears it.

## Recovery procedures

| Situation | Action |
|---|---|
| Phone shows `unauthorized` / `host_key_mismatch` | `scripts/dock-pair.sh` |
| Agent misbehaves | `systemctl --user restart pocket-desktop-agent`. The phone reconnects on its own (or tap **Retry now**) |
| Stuck in Dock Mode without touch | `ssh lavender pocket-os/bin/pocket-ctl ui mode handheld` |
| Remove Dock from the computer | `systemctl --user disable --now pocket-desktop-agent; rm ~/.config/systemd/user/pocket-desktop-agent.service ~/.local/bin/pocket-desktop-agent; rm -r ~/.config/pocket-desktop-agent` |
| Remove pairing from the phone | `ssh lavender 'rm -r ~/.config/pocket-os/dock'` |

## Device and host changes

- Phone: new user files only: `~/.config/pocket-os/dock/` and the updated
  `~/pocket-os/` deploy. No root changes.
- Computer: `~/.local/bin/pocket-desktop-agent`,
  `~/.config/systemd/user/pocket-desktop-agent.service` (started, not enabled),
  `~/.config/pocket-desktop-agent/`. No firewall or system changes.
