# Dock Mode: protocols

Two contracts carry Dock Mode:

1. **Pocket Dock** — phone (pocketd) ↔ laptop (agent), TCP + Noise + JSON. Transport,
   authentication and the 1.0 messages are specified in
   [ADR-0007](../decisions/0007-dock-architecture.md); code:
   `crates/pocket-dock-protocol/src/message.rs`. Stage 4 adds **version 1.1**.
2. **`org.pocketos.Core1.Dock`** — Core ↔ shell/tools on the phone's session bus. 1.2/1.3
   are in ADR-0007; code: `crates/pocket-ipc/src/lib.rs`. Stage 4 adds **contract 1.4**.

Both are additive: unknown message types, fields and dictionary keys are ignored, so a
1.0 agent works with a 1.1 phone and the other way round (with fewer features).
Status: **implemented** (Stage 4; contract tests in `crates/pocketd/tests/dock_contract.rs`
and `crates/pocket-desktop-agent/tests/protocol.rs`). `Execute` parameters may also be
wrapped once more in a variant (as `dbus-send` does); Core unwraps one level.

## Pocket Dock 1.1

### Commands vs events

| Kind | Direction | Messages | Delivery |
|---|---|---|---|
| Command | phone → agent | `invoke`, `subscribe`, `ping` | `invoke` gets exactly one `result`; `subscribe` gets one snapshot per topic |
| Reply | agent → phone | `result`, `pong`, `welcome` | correlated by `id` |
| Event | agent → phone | `actions`, `state`, `progress`, `notification`, `notification_closed` | unsolicited, latest-wins |
| Fatal | agent → phone | `error {code, message}` | then close |

### Additions to existing messages

`welcome` — `protocol: {major:1, minor:1}`; new optional `topics: ["host","media","system","notifications"]`
(topics this agent can publish).

`ActionInfo` (in `welcome.actions` and `actions`):

| Field | Type | Default (1.0 peer) | Meaning |
|---|---|---|---|
| `privilege` | `"basic"` \| `"elevated"` | `"basic"` | elevated actions need a grant on the laptop |
| `idempotent` | bool | `false` | running it twice has the same effect as once |
| `confirm` | bool | `false` | the UI asks before sending |
| `params[].choices` | `[{value, label}]` | none | the only accepted values (string params) |
| `params[].min` / `max` | integer | none | inclusive bounds (integer params) |

An `elevated` action that is not granted is advertised with `available: false` and
`reason: "not granted on <host> (pocket-desktop-agent grant <id>)"`.

`invoke` — new optional `key`: 1–64 chars `[A-Za-z0-9_-]`, chosen by the phone, unique per
user intent. Same key ⇒ same intent.

`result` — new optional `duplicate: bool`: the agent did not run the action again; `ok`,
`data` and `error` are those of the earlier execution.

### New messages

```jsonc
// phone → agent, once after welcome (re-sent never; a new session subscribes again)
{"type":"subscribe","topics":["host","media","system","notifications"]}

// agent → phone: full value of one topic; first one per topic is the snapshot
{"type":"state","topic":"media","rev":7,"available":true,"reason":"",
 "interval_ms":0,"data":{...}}

// agent → phone: workflow progress for an in-flight invoke
{"type":"progress","id":12,"step":2,"total":3,"label":"Open Konsole"}

// agent → phone
{"type":"notification","nid":"n-41","app":"Thunderbird","summary":"New mail",
 "body":"…","urgency":1,"redacted":false,"time_ms":1795428430310}
{"type":"notification_closed","nid":"n-41"}
```

`rev` strictly increases per topic within one agent process. `interval_ms > 0` means the
topic is sampled periodically; the phone treats it as stale after 3 × `interval_ms`
without an update. Unknown topics in `subscribe` are ignored; topics the agent cannot
serve are answered with `available:false` and a `reason`.

### Topic payloads

| Topic | `data` | Updates |
|---|---|---|
| `host` | `{locked: bool\|null, battery: {percent, charging}\|null}` | ScreenSaver `ActiveChanged`, UPower `PropertiesChanged` |
| `media` | `{active: "<player>"\|null, players: [{id, identity, status, title, artist, album, can_next, can_previous, can_play_pause}]}` | MPRIS `PropertiesChanged`, `NameOwnerChanged` (coalesced 250 ms) |
| `system` | `{cpu_percent, mem_percent}` | every 5 s while a phone is subscribed (`interval_ms: 5000`) |
| `notifications` | `{forwarding: bool, reason}` | when the policy changes; notifications themselves are separate messages |

### Error codes (`result.error.code`)

1.0: `unknown_action`, `not_permitted`, `unavailable`, `invalid_params`, `failed`.
1.1 uses `not_permitted` for "not granted" and "value not in the laptop's list", and
adds `busy` (the same key is running in another session that ended without result — not
used yet; reserved).

### Idempotency and retry semantics

| Outcome on the phone | Meaning | What the UI offers |
|---|---|---|
| `ok` | the laptop ran it and confirmed | — |
| `ok` + `duplicate` | it had already run; this is the stored result | — |
| agent error (`failed`, `unavailable`, …) | it did not run (or failed while running) | tap again = a new intent, new key |
| `timeout`, `connection_lost` | **unknown**: it may or may not have run | **Retry** (same key) |
| `not_connected`, Core rejection | it was not sent | — |

- Core **never** re-sends on its own (not after reconnect, not after a Core restart).
- `Retry` re-sends the stored action, params and key. The agent's dedup cache
  (10 min, 512 entries, per phone key) turns it into "return the stored result" or "run it
  once". After an agent restart the cache is gone: a retry may run the action again. The
  UI therefore words Retry for non-idempotent actions as "Run again?" with confirmation.
- Two taps on the same tile are separate intents only after the first finished; while a
  request is pending, the tile is not tappable (shell) and Core rejects nothing extra.

### Versioning

`PROTOCOL = 1.1`. Majors must match (fatal `incompatible_protocol`). Core sends
`subscribe` and keys only to agents ≥ 1.1. The agent sends 1.1 events only after a 1.1
`subscribe` (events) or for 1.1 fields in messages a 1.0 phone ignores anyway.

## `org.pocketos.Core1.Dock` 1.4

Additions (existing members unchanged):

| Member | Signature | Notes |
|---|---|---|
| action dict keys | `privilege s`, `idempotent b`, `confirm b` | from the agent's advertisement |
| `Retry` | `(t request) → t new_request` | only for requests that ended `timeout`/`connection_lost` in the last 10 min; errors `…Error.UnknownRequest`, `NotConnected`, `ActionUnavailable` |
| `GetDeck` | `() → s json` | `{version, navigation, pages, source: "user"\|"default", error}` |
| `ReloadDeck` | `()` | re-reads `deck.json`; emits `DeckChanged` |
| `DeckChanged` | signal `()` | |
| `GetLive` | `() → s json` | `{topic: {rev, available, reason, interval_ms, data, age_ms}}`; `{}` unless connected |
| `LiveChanged` | signal `(s topic, s json)` | `json` empty = topic cleared (session ended) |
| `ActionProgress` | signal `(t request, u step, u total, s label)` | |
| `Notification` | signal `(s json)` | `{nid, app, summary, body, urgency, redacted, time_ms, silent}`; `silent` = DND |
| `NotificationClosed` | signal `(s nid)` | |
| `GetPreferences` | `() → a{sv}` | `dnd b`, `muted_apps as` |
| `SetPreference` | `(s key, v value)` | `dnd` (b), `mute_app` (s), `unmute_app` (s); persisted; error `InvalidArgument` |

`ActionFinished` keeps its 1.2 signature. A duplicate result is reported as `ok=true`
with `message = "duplicate"`.

`pocket_ipc::PROTOCOL_VERSION` becomes **1.4**.

## Stage 4.5 additions

### Agent actions (Pocket Dock 1.1, additive)

| Action | Privilege | Parameters | Notes |
|---|---|---|---|
| `desktop.open` | elevated | `destination` (choices: the laptop's `destinations` ids) | opens `{app, target}` through the app's `%f/%u` field code; target never comes from the phone (#16) |
| `access.request` | basic | `action` (choices: elevated actions that are enabled and not granted) | shows Allow/Deny on the laptop; result `{"status":"asked"}` at once; the outcome arrives as a new `actions` advertisement; `unavailable` while a prompt is open or within 60 s of a Deny |

The agent re-reads its policy on SIGHUP and pushes `actions` to connected phones.

### `org.pocketos.Core1.Dock` 1.5

| Member | Signature | Notes |
|---|---|---|
| action dict key | `params s` | JSON array `[{name, kind, required, choices:[{value,label}], min, max}]` |
| `SetDeck` | `(s json)` | validates (the `GetDeck` document round-trips: tile `id`, `source`, `error` ignored), stores the user deck atomically, emits `DeckChanged`; invalid → `InvalidArgument`, nothing changes |
| `ResetDeck` | `()` | removes the user deck; the built-in one applies; emits `DeckChanged` |
| `SetPreference` key | `reduce_motion` (b) | also returned by `GetPreferences` |

`pocket_ipc::PROTOCOL_VERSION` is **1.5**.
