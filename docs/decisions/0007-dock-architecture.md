# ADR-0007: Dock architecture, desktop transport and agent

- Status: **Accepted** (implemented and validated in Stage 3; amended in Stage 3.1)
- Date: 2026-11-23
- Evidence: `docs/validation/stage3.md`, `docs/validation/stage3.1.md`

## Context

Dock Mode turns the phone into a landscape control surface for a computer. Stage 3 must
prove the whole path, from a touch on the phone to a confirmed action on the computer,
without defining the full Dock product (no Stream Deck clone, macros, widgets or remote
desktop). The architecture has to stay small, and it must not block those features later.

Constraints from earlier ADRs:
- Pocket Core owns authoritative state; the shell is only UI (ADR-0002, ADR-0004).
- `pocketd` is a static aarch64-musl binary built with Rust's bundled `rust-lld`. There
  is no C cross toolchain, so every Rust dependency must be pure Rust (ADR-0002).
- The phone reaches the development computer over USB networking (phone `172.16.42.1`,
  computer `172.16.42.2`).

## Decision

### Components and ownership

```
Phone (Pocket OS)                                         Computer (Fedora, KDE Plasma)
Pocket Shell (QML)                                        pocket-desktop-agent (user service)
  features/dock/DockPage ─ Shell.dock (DockClient) ─┐      ├ server: Noise handshake, trust, sessions
  Quick Settings "Dock mode" ─ Shell.requestMode    │      ├ actions::Registry (validation)
                                                    │ D-Bus│   ├ desktop.notify  → org.freedesktop.Notifications
pocketd (Rust)                                      ▼      │   └ media.play_pause → org.mpris.MediaPlayer2.*
  org.pocketos.Core1        mode (unchanged)               │
  org.pocketos.Core1.Dock   dock::DockService ─ cmds ─► dock::link ══ TCP + Noise_XX + JSON ══╛
                            (state, validation)          (connect, retry, keepalive, requests)
crates/pocket-dock-protocol: messages, framing, Noise channel, key files (shared by both sides)
```

| Concern | Owner |
|---|---|
| Operating mode (`handheld`/`dock`) | Pocket Core (`org.pocketos.Core1`, unchanged) |
| Desktop link state, trusted-host config, request tracking | Pocket Core, `pocketd/src/dock/` (`org.pocketos.Core1.Dock`) |
| Transport, encryption, peer authentication | `pocket-dock-protocol` (used by both sides) |
| Executing desktop actions, deciding what exists and what is available | the desktop agent |
| Presentation, mode toggle, dispatch on touch, last result per action | the shell (`Shell.dock`, `features/dock/`) |

**Mode and connection are separate.** `Mode` keeps exactly two values. The link has its
own `DockState`: `disconnected`, `connecting`, `connected`, `error`. Dock Mode renders and
can be left with no computer present. A link can stay up in Handheld Mode. The only
coupling is Core policy (Stage 3.1; in Stage 3 it was in `Shell.qml`): entering Dock Mode
(handheld → dock) acts as `Connect` when the link is `disconnected` and a host is
configured. Leaving Dock Mode does not disconnect. The shell never connects on its own.

**Intent is separate from state** (Stage 3.1). `connection_wanted` records what the user
asked for: `Connect` and entering Dock Mode set it, `Disconnect` clears it, and nothing
else changes it. The actual `DockState` is whatever the link task achieved. Automatic
reconnection (backoff, network hints, core restart) only happens while it is set.

**Processes.** No new phone process: the Dock module lives in `pocketd` as one D-Bus
object and one Tokio task. The computer runs one small agent as the desktop user
(systemd user unit, refuses to run as root). New crates exist only where they give a real
boundary: `pocket-dock-protocol` (shared by both programs) and `pocket-desktop-agent`.

### Transport: TCP + Noise XX + JSON (custom, narrow)

- **TCP** to one configured `address:port` (default port 47130), normally over the USB
  link. The agent binds only that address (`IP_FREEBIND`, so it can start before the cable
  is connected). It refuses `0.0.0.0`/`::`.
- **Noise `Noise_XX_25519_ChaChaPoly_BLAKE2s`** via [`snow`](https://crates.io/crates/snow)
  (pure Rust; its `std` feature is disabled because it pulls `ring`, which needs a C
  compiler). This gives mutual authentication with static X25519 keys and an encrypted
  channel. There is no custom cryptography. Each side **pins** the other's public key:
  the phone stores exactly one host key, and the agent stores a list of trusted phones.
  The application layer is reached only after both checks pass.
- **Framing**: `u16` big-endian length + one Noise message (≤ 65535 bytes). Each direction
  uses its own nonce counter (snow's stateless transport), so sending and receiving run in
  separate tasks without locks.
- **Messages**: JSON objects tagged by `type` (`crates/pocket-dock-protocol/src/message.rs`).
  Unknown fields and types are ignored, so minor versions only add.

| Direction | Message | Meaning |
|---|---|---|
| phone → agent | `hello {protocol, client, device_name}` | first message; protocol majors must match |
| agent → phone | `welcome {protocol, agent, host_name, actions}` | capabilities |
| phone → agent | `invoke {id, action, params}` | run one registered action |
| agent → phone | `result {id, ok, data, error{code,message}}` | exactly one per invoke, after it ran |
| agent → phone | `actions {actions}` | availability changed (full list) |
| phone → agent / back | `ping {id}` / `pong {id}` | keepalive |
| agent → phone | `error {code, message}` | fatal (`unauthorized`, `incompatible_protocol`, `protocol_error`, `replaced`), then close |

Protocol version: **Pocket Dock 1.0** (`pocket_dock_protocol::PROTOCOL`), independent of
the Core D-Bus contract version.

### Authentication and pairing

Trust is bootstrapped over the existing key-based SSH link: `scripts/dock-pair.sh` creates
both identities if they are missing (`pocket-desktop-agent init`, `pocketd dock init`),
copies only the **public** keys, and records them (`pocket-desktop-agent trust`,
`pocketd dock set-host`). Private keys never leave their machine. Key and config files
are mode 0600 in 0700 directories. Files that group or others can access are refused.
Nothing is in Git or QML:

| Phone `~/.config/pocket-os/dock/` | Computer `~/.config/pocket-desktop-agent/` |
|---|---|
| `identity.key`, `host.json` (name, address, pinned key) | `identity.key`, `config.json` (listen address, host name, trusted phones, optional action allow-list) |

An unknown phone key gets `error unauthorized` (with the key to trust) and is closed. A
changed computer key fails on the phone with `host_key_mismatch`. Both need the user (to
re-pair), so neither is retried automatically.

**Trust assumptions.** Stage 3 runs over USB, a point-to-point link. Noise already
authenticates and encrypts every byte, so the same protocol is suitable for a LAN. Before
Wi-Fi is enabled, discovery, roaming addresses and the agent's listen policy (bind address
and firewall zone) still need a decision. This ADR does **not** claim that exposing the
agent on an untrusted network is reviewed. Internet access is out of scope.

### Desktop agent and the action contract

- `Action` trait: `info()` (stable id, label, category, `available` + `reason`, typed
  `params`) and async `execute(params) -> Result<data, {code, message}>`. Adding an action
  means one type plus one `register` call. Networking does not change.
- The registry validates before an action runs: id syntax (`[a-z][a-z0-9._-]{0,63}`) →
  registered (`unknown_action`) → enabled in config (`not_permitted`) → available
  (`unavailable`) → parameters by name and type (`invalid_params`). There is no
  shell-command or scripting action.
- Built-ins: `desktop.notify` (title/body, body escaped against notification markup)
  through `org.freedesktop.Notifications`, and `media.play_pause` through MPRIS. The
  available MPRIS players are tracked from `NameOwnerChanged`, without polling.
- One session per trusted phone. A newer authenticated session replaces the old one, so a
  phone recovering from a half-open connection can reconnect immediately.

### Core contract: `org.pocketos.Core1.Dock` (protocol 1.2, 1.3)

Since 1.3 (Stage 3.1) the state also has `retry_in_ms` (time left, computed when the state
is produced) and `connection_wanted`; `retry_in` is computed the same way.
`GetState → a{sv}`, `ListActions → aa{sv}`, `Connect()`, `Disconnect()`,
`Execute(s action, a{sv} params) → t request`; signals `StateChanged(a{sv})`,
`ActionsChanged(aa{sv})`, `ActionFinished(t request, s action, b ok, s error_code,
s message, s data_json)`. Keys: `pocket_ipc::dbus::dock`. New errors:
`…Error.{NotConfigured, NotConnected, UnknownAction, ActionUnavailable, InvalidArgument}`.
Existing interfaces are unchanged (1.2 only adds).

**Acceptance is not success.** `Execute` returns once Core has *accepted* the request
(connected, advertised, available). The UI shows a result only after `ActionFinished`,
which relays the computer's `result`. Every accepted request gets exactly one
`ActionFinished`: the agent's result, or `timeout` (15 s), `connection_lost` ("may or may
not have run") or `not_connected` from Core.

QML: `DockClient` in `Pocket.Core` shares the `CoreClient` connection (like
`AppCatalog`), mirrors the state by revision and re-reads everything after a core restart.

### Connection lifecycle and failure handling

```
disconnected ─Connect─► connecting ─welcome─► connected
     ▲ Disconnect (any state)   │ fail            │ closed / error / no pong
     └──────────────────────── error ◄────────────┘
          retry after 2, 4, 8, 16, 30, 60, 60… s (transient failures only)
```

- Transient failures (`unreachable`, `timeout`, `connection_closed`, `connection_error`,
  `handshake_failed`, `protocol_error`, `replaced`) are retried with that backoff. A
  manual `Connect` retries at once. Automatic attempts never start less than 2 s apart.
- Network hints (Stage 3.1, #7): `pocketd/src/dock/netwatch.rs` reads rtnetlink (carrier-up
  edges, new IPv4 addresses, new main-table routes). While a retry is pending, a hint moves
  it to 1 s from now (but ≥ 2 s after the previous attempt) and restarts the backoff, at
  most once per 10 s, and only if the kernel has a route to the host. NetworkManager's
  D-Bus API was not used: it is on the system bus, and pocketd needs only "the network
  changed". Polling was rejected.
- `unauthorized`, `host_key_mismatch` and `incompatible_protocol` stop and wait for the
  user.
- Keepalive: a `ping` after 20 s without traffic from the phone; no answer within 8 s
  ends the session. A frozen agent or a pulled cable is therefore noticed within about
  30 s. A killed agent is noticed at once (TCP FIN/RST). The agent closes sessions idle
  for 60 s.
- Connection state is transient and never persisted. **Session recovery** (Stage 3.1, #5):
  pocketd keeps `{version, mode, dock_wanted}` in `$XDG_RUNTIME_DIR/pocket-os/core-recovery.json`
  (atomic, 0600; the directory is created and deleted by `pocket-session-run`, so it never
  outlives the graphical session, and pocketd never creates it). A restarted core starts in
  that mode and, if the link was wanted, connects again with a new authenticated session.
  Sockets, keys, requests and timers are never saved. Invalid content means a fresh start.
  A new session starts in Handheld. A shell restart re-reads everything and keeps the link.
- Retry countdown: Core owns the deadline. `DockClient` counts down from `retry_in_ms` with a
  monotonic `QDeadlineTimer`, ticking once per second only while the Dock page is visible.
- Local callers are not authenticated: any process of the session user can call
  `Execute` (Stage 3.1 investigation, [#8](https://github.com/kitnew/Pocket-OS/issues/8)).
  The Noise link authenticates the phone, not the program on it. Privileged,
  destructive or input-injecting desktop actions must not be added before #8 is resolved.
- Orientation: while the core reports `dock`, the shell keeps the output landscape. The
  user's rotation preference (`rotationMode`) is not changed and applies again when
  Dock Mode ends.

## Alternatives considered

| Alternative | Why not (Stage 3) |
|---|---|
| **KDE Connect** (Fedora already runs `kdeconnectd`; Alpine has `kdeconnect` 26.08) | The phone side is the full KDE daemon: about 210 packages (KF6 KIO, Notifications, Qt Widgets/Multimedia/Bluetooth, Kirigami, sshfs). Its plugins are fire-and-forget, so there is no execution confirmation for ping or run-command. Run-command is a list of configured shell strings, which is exactly the generic endpoint we must avoid. Integration would be through `kdeconnectd`'s D-Bus API, with discovery broadcast on all interfaces. It does not fit a small, confirmed, typed action contract. Its MPRIS/notification sync could still be bridged later as an agent action |
| SSH with a forced command on the computer | Needs `sshd` enabled on the workstation and a system-wide config change. The agent would live only as long as a connection, and pocketd would spawn `ssh` |
| TLS (rustls) with pinned self-signed certificates | `ring`/`aws-lc-rs` need a C cross compiler for aarch64-musl. The pure-Rust provider is less mature than snow. Certificate handling buys nothing for one pinned peer |
| Pre-shared key + HMAC | A home-made authentication protocol without encryption. Noise is the standard construction for this |
| gRPC / HTTP / WebSocket | A larger dependency stack for 6 message types, and it still needs the same authentication layer |
| D-Bus over TCP | No modern authentication; unsafe beyond localhost |
| A separate Dock daemon on the phone | A second process for one task in `pocketd`; ADR-0002 keeps one core |

## Consequences

- One new trust root per pair (two key files), managed by `scripts/dock-pair.sh`.
- The contract is defined in four places: `pocket-dock-protocol` (wire),
  `pocket_ipc::dbus::dock` (D-Bus), `bridge/pocket-core-qml/dockclient.cpp` and this ADR.
  The tests catch drift: agent protocol tests, `crates/pocketd/tests/dock_contract.rs` and
  `tst_coreclient::dockClient`.
- Future work fits these extension points without changing the transport: new actions
  (agent registry), typed parameters (`params`), categories (already on the wire),
  status/telemetry (new agent → phone message types, ignored by older phones), several
  pages or profiles (UI only), Wi-Fi (another `address`, see the trust assumptions above),
  and automatic Dock activation (a shell or Core policy that calls `RequestMode`).
