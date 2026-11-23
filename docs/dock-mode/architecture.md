# Dock Mode: architecture (Stage 4, amended in Stage 4.5)

Stage 4.5 ([ADR-0009](../decisions/0009-dock-ux-and-access.md)): immersive Dock (system bars
follow the surface in front), Core 1.5 deck editing (`SetDeck`/`ResetDeck`) and action
parameters, live agent configuration (SIGHUP), `desktop.open` destinations,
laptop-approved `access.request`, and app launches as transient user services.

Status: **implemented (Stage 4)**, extending the Stage 3/3.1 foundation
([ADR-0007](../decisions/0007-dock-architecture.md)). Decisions and their tradeoffs:
[ADR-0008](../decisions/0008-dock-platform.md). Wire details: [protocol.md](protocol.md).
What is implemented: [roadmap.md](roadmap.md).

## Principle: extend, do not add processes

Stage 4 adds **no new process** on either side. Everything fits the three existing
components and the shared protocol crate:

```
Phone (Pocket OS)                                                 Laptop (Fedora, KDE Plasma)
Pocket Shell (QML)                                                pocket-desktop-agent (user service)
  features/dock/                                                    server: sessions, dedup cache, subscriptions
    DockView      rail + Base View + Context Overlay + Global slot   actions/: Registry (+ privilege, grants, workflows)
    AmbientHome   clock, status, widgets                                notify · media.* · volume.* · shortcut ·
    ControlDeck   pages + tiles (from Core's deck)                       app.launch · session.lock · workflow.run
    LinkView      Stage 3 page (connect, errors, leave)              live/: topic publishers
    navmodel.js   pure navigation state machine (tested with node)     mpris (players, metadata) · host (lock,
  Shell.dock ─ DockClient (Pocket.Core, QtDBus)                          battery) · system (cpu, mem)
        │ D-Bus org.pocketos.Core1.Dock (contract 1.4)                 notifications (session-bus monitor)
pocketd                                                                   │
  dock/mod.rs   D-Bus face: validation, deck, live, retry                 │
  dock/link.rs  connection, keepalive, requests, subscriptions ══ TCP + Noise_XX + JSON (Pocket Dock 1.1) ══╛
  dock/deck.rs  deck.json load + validation
  dock/prefs.rs notification preferences (DND, muted apps)
crates/pocket-dock-protocol: messages (1.1 additions), channel, keys
```

## Responsibility boundaries

| Concern | Owner | Why there |
|---|---|---|
| Which host actions exist, their parameters, privilege, availability | **Agent** | Only the laptop knows its desktop; a different companion client gets the same actions |
| Whether an action may run for this phone (grants) | **Agent** | Authorization is enforced where the effect happens; the phone cannot widen it |
| Executing actions, workflows, de-duplication by request key | **Agent** | Idempotency must be decided where execution happens |
| Observing media, lock state, vitals, notifications | **Agent** | Fedora-specific integrations stay on Fedora |
| Link state, request acceptance, one result per request, retry of unknown outcomes | **Core** | Unchanged Stage 3 rule: Core owns authoritative state; the shell never talks to the network |
| Deck configuration (pages, tiles, idle timeout): load, validate, serve | **Core** | Persistent, validated configuration with deterministic Rust tests; reusable by other clients (`pocket-ctl`, a future editor) |
| Notification preferences (DND, muted apps), filtering | **Core** | Persistent; filtering before any client sees content |
| Live topic cache and staleness (cleared when the session ends) | **Core** | One place decides what is current |
| Navigation (view, page, pin, idle return), overlays, tile result display, notification history | **Shell** | UI-owned, transient (ADR-0004) |
| Clock, phone battery | Shell (`Shell.now`, `Shell.system`) | System-owned state is read from its service, not Core |

## Communication flows

**Action (tile tap → laptop):**

```
Tile tap ─► Shell.runDeckTile ─► DockClient.execute(action, params)
  ─► Core Dock.Execute: connected? advertised? available? → request id + random key
  ─► invoke {id, key, action, params} ─► agent: id syntax → registered → enabled → granted
     → available → params (type, choices, limits) → dedup(key) → execute
  ◄─ [progress {id, step, total, label}]* ◄─ result {id, ok, data|error, duplicate}
  ◄─ Core ActionProgress / ActionFinished ◄─ DockClient ◄─ tile state
```

**Live state:** after `welcome` (agent ≥ 1.1) Core sends `subscribe {topics}`. The agent
answers each topic with a full snapshot (`state {topic, rev, available, data}`) and
sends a new full topic value on every change (coalesced). Core caches the latest value per
topic and emits `LiveChanged(topic, json)`; on session end it clears every topic and emits
`LiveChanged(topic, "")`. The shell renders only topics present and `available`.

**Notifications:** the agent observes `org.freedesktop.Notifications.Notify` calls and
their replies on the user's session bus (a read-only monitor connection; Plasma stays the
notification server), applies the laptop-side policy (opt-in, app filters, redaction
while locked) and sends `notification` / `notification_closed`. Core drops muted apps,
marks DND, emits `Notification(json)` / `NotificationClosed(id)`. The shell shows banners
and keeps a short history in memory.

## State ownership and lifetime

| State | Owner | Persisted? | After Core restart | After shell restart | After reconnect |
|---|---|---|---|---|---|
| mode, link intent | Core | session recovery file (3.1) | restored | re-read | — |
| deck config | Core | `~/.config/pocket-os/dock/deck.json` (user) or built-in default | re-read | re-read | — |
| notification prefs | Core | `~/.config/pocket-os/dock/preferences.json` | re-read | re-read | — |
| advertised actions | agent → Core | no | after welcome | re-read | fresh `welcome` |
| live topics | agent → Core | no | empty until snapshot | re-read from Core | fresh snapshot |
| uncertain requests (for Retry) | Core | no (memory, 10 min) | lost | kept (Core) | kept |
| dedup cache | agent | no (memory, 10 min, 512 entries) | — | — | kept (agent) |
| grants, apps, workflows, notification policy | agent | `~/.config/pocket-desktop-agent/config.json` | — | — | — |
| view, page, pin, history, tile results | shell | no | kept | reset to Home | kept |

## Deck configuration

`deck.json` (schema v1). The built-in default is `config/dock/deck.json`, compiled into
pocketd; a user file at `~/.config/pocket-os/dock/deck.json` replaces it. Core validates
it completely; an invalid user file is reported (`GetDeck` → `error`) and the default is
used. `pocket-ctl dock ReloadDeck` re-reads it. Since Core 1.5 the phone's deck editor
saves through `SetDeck` (same validation; atomic write) and `ResetDeck` returns to the
default.

```json
{ "version": 1,
  "navigation": { "idle_timeout_s": 15 },
  "pages": [
    { "id": "general", "title": "General", "tiles": [
        { "action": "session.lock", "label": "Lock laptop", "icon": "lock" },
        { "action": "app.launch", "label": "Terminal", "icon": "terminal",
          "params": { "app": "org.kde.konsole" } } ] } ] }
```

Limits: ≤ 12 pages, ≤ 24 tiles per page, page id `[a-z][a-z0-9_-]{0,31}` unique, title
≤ 32 chars, label ≤ 40, valid action id, params only string (≤ 500 chars) / bool /
integer, icon `[a-z0-9-]{1,32}` (unknown icons fall back to a generic glyph). Tiles name
actions; whether the action exists, is granted and available is decided at run time by
the agent's advertisement, so one deck works with any laptop.

## Security boundaries

1. **Laptop ↔ phone** (unchanged): Noise XX with pinned keys on both sides; nothing
   reaches the application layer before both checks pass.
2. **Authorization on the laptop** (new, ADR-0008): every action has a privilege class.
   `basic` actions (harmless if any program on the phone triggers them: notification,
   media transport, system volume) are enabled by default. `elevated` actions (app
   launch, desktop shortcuts, lock, workflows containing elevated steps) run only if the
   laptop user **granted** them (`pocket-desktop-agent grant <id>`, or *Allow* on the
   laptop's prompt after the phone asked with `access.request`, ADR-0009). Parameters with
   fixed choices (registered apps, approved shortcuts, configured workflows) are checked
   against the laptop's lists, never interpreted.
3. **Never implemented:** arbitrary keys/text/pointer, commands or scripts, file or URL
   handling from phone-supplied values, shutdown/logout, clipboard.
4. **Phone-local callers** (#8, open): any process in the phone's session can call
   `Dock.Execute` and read Core's `Notification` signals. Stage 4 bounds the damage on
   the laptop side (grants, fixed choices, no destructive actions) and keeps notification
   forwarding **opt-in on the laptop** with redaction while locked. Real isolation
   (sandboxed apps + shell-only channel) remains #8. Confirmation sheets are a UX guard
   against accidental taps, **not** a security boundary (an app could tap them through
   Phoc's virtual pointer, see #8).

## Reliability rules

- Every accepted request ends with exactly one `ActionFinished` (unchanged).
- Core never replays a request automatically. `timeout` / `connection_lost` results are
  kept for 10 minutes as *uncertain*; `Retry(request)` re-sends them with the same key.
- The agent de-duplicates by `(phone, key)` for 10 minutes: a retry of something that
  already ran returns the stored result with `duplicate: true`; a retry that arrives while
  the original still runs waits for it. Lost on agent restart (documented: a retry after
  an agent restart may run the action again; non-idempotent actions say so).
- Live topics are cleared when the session ends; the next session starts from a fresh
  snapshot. A topic with `interval_ms` (system vitals) is stale on the phone when no
  update arrived within 3 × interval.
- Protocol compatibility: a 1.0 agent gets no `subscribe`/keys; a 1.0 phone ignores 1.1
  messages. Core D-Bus 1.4 only adds methods, signals and keys.

## Extension points (future, not implemented)

| Future feature | Where it plugs in |
|---|---|
| Voice session (Global Interaction) | Shell: `global` slot in the navigation model (blocks idle return) and the top overlay layer. Protocol: new message types `voice_*` and a separate low-latency audio channel negotiated in `welcome.features` (not the JSON control channel) |
| Agent tool execution | The same Registry: tools are actions; privilege + grants + `confirm` already model "explicitly authorized"; confirmation would come from the shell's sheet once #8 makes it a real boundary |
| Background task widgets | A `tasks` live topic (list of `{id, label, progress, state}`); the Home widget column already renders per-topic widgets |
| Contextual deck pages | A `context` live topic (e.g. active app) + deck pages with a `when` condition |
| Other companion clients | Speak Pocket Dock 1.x to the agent with their own Noise key and grants |
