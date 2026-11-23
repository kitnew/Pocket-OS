# ADR-0008: Dock platform — host actions, authorization, live state, notifications

- Status: **Accepted**, implemented in Stage 4 ([validation](../validation/stage4.md),
  [roadmap](../dock-mode/roadmap.md))
- Date: 2026-11-23
- Amends: [ADR-0007](0007-dock-architecture.md) (action contract, protocol 1.1, Core 1.4)
- Related: [#8](https://github.com/kitnew/Pocket-OS/issues/8) (local caller authorization)
- Amended by: [ADR-0009](0009-dock-ux-and-access.md) (laptop-approved access requests, live
  configuration, app launch outside the agent sandbox)

## Context

Stage 4 turns Dock Mode from a two-button proof into a companion platform: Ambient Home,
a configurable Control Deck, real laptop actions (apps, shortcuts, media, lock,
workflows), live laptop state and notifications. ADR-0007's rule — "until #8 is
resolved, add only actions that are harmless if anyone in the phone session triggers
them" — blocks most of that. #8's real fix (sandboxed apps + a shell-only capability
channel + Wayland security contexts) is a launcher redesign (ADR-0005) and is not
achievable inside Stage 4.

## Decisions

### 1. No new processes; place features by ownership

Deck config and notification preferences live in **Core** (validated, persistent,
testable in Rust). Host integrations live in the **agent**. Navigation lives in the
**shell** as a pure, unit-tested state machine. See [architecture](../dock-mode/architecture.md).
Rejected: a phone-side "dock daemon" (a second process for state Core already owns);
deck config in QML files (no validation, no tests, not reusable by tools).

### 2. Authorization on the laptop: privilege classes + grants

Authentication (Noise) identifies the phone; it does not authorize actions. Every action
declares a privilege:

- **basic** — harmless if any program on the phone triggers it: `desktop.notify`,
  `media.*` transport, system volume. Enabled by default (unchanged behaviour for the
  Stage 3 actions).
- **elevated** — changes visible desktop state but is neither destructive nor
  data-exposing: `app.launch` (registered apps only), `desktop.shortcut` (approved KWin
  shortcuts only), `session.lock`, `workflow.run`. Runs only when the laptop user
  granted it: `pocket-desktop-agent grant <id>`. Not granted ⇒ advertised as unavailable
  with the reason, `invoke` ⇒ `not_permitted`.
- **never** — not implementable through the registry at all: arbitrary key/text/pointer
  input, commands, scripts, file/URL handling from phone-supplied values, shutdown,
  logout, clipboard.

Parameters that select *what* to act on are closed lists defined on the laptop
(`choices`): registered app IDs, approved shortcut names, configured workflow IDs. The
phone can only pick from them.

**Relation to #8.** This does **not** resolve #8: any phone-session process can still
trigger a granted action. It changes the rule from "nothing beyond basic" to "nothing
beyond basic *unless the laptop owner grants it, knowing #8's caveat*", and caps the
worst case at "an app on the phone opened a registered app, switched a workspace or
locked the laptop". The default configuration is as restrictive as Stage 3.1. #8 stays
open with this recorded.

Rejected: phone-side confirmation as authorization (defeated by Phoc's virtual pointer,
#8 option E); per-call PIN (no secure input path on the phone without #1/#8); raw
shortcut injection via uinput/`ydotool` (input injection, needs root/uinput, KDE-agnostic
only in the worst way). KDE's own `kglobalaccel.invokeShortcut` is used instead: named
compositor actions, no synthetic input.

### 3. Exactly-once-ish retries: request keys + agent dedup

Core attaches a random key to every request and keeps uncertain ones (`timeout`,
`connection_lost`) for 10 minutes. Only the user retries (`Retry`), with the same key.
The agent caches results per `(phone, key)` for 10 minutes and returns `duplicate`
instead of running twice; a retry racing the original waits for it. Rejected: automatic
replay after reconnect (non-idempotent actions would run twice); persisting the dedup
cache (adds disk state on the laptop for a 10-minute window; the residual risk after an
agent restart is shown in the UI).

### 4. Live state: subscribe + per-topic full values

After `welcome`, Core subscribes to topics; the agent sends a full snapshot per topic,
then a full new value per change (coalesced). Topic payloads are small (< 2 KB), so
per-topic replacement is simpler and safer than diffs: a lost or reordered message cannot
corrupt state, and reconnect resync is simply "subscribe again". Core clears all topics
when a session ends; the shell never sees stale remote data. Periodic topics declare
`interval_ms` so staleness inside a live session is detectable. Event sources: MPRIS and
UPower `PropertiesChanged`, ScreenSaver `ActiveChanged`; only CPU/memory are sampled
(5 s, only while subscribed).

### 5. Notifications: observe, do not replace

Plasma remains the notification server. The agent opens a separate session-bus
connection and calls `BecomeMonitor` with match rules for `Notify` calls, their method
returns and `NotificationClosed` signals (dbus-broker allows monitoring for the bus
owner's UID; verified in Stage 3 research). Forwarding is **opt-in on the laptop**
(`notifications.forward`), filtered by app allow/deny lists, the phone's own
`Pocket OS` notifications are excluded (no echo), and bodies are redacted while the
laptop is locked (default). Nothing is written to disk on either side.
Rejected: replacing the notification daemon (breaks Plasma's UI and history);
KDE Connect's notification sync (pulls in the whole KDE Connect stack on the phone,
ADR-0007); polling Plasma's history (no public API).

Notification actions are not supported: only the notification's owner receives
`ActionInvoked`, and Plasma exposes no API to trigger another client's action.

### 6. Shell: three layers, pure navigation model

`DockView` renders Base View / Context Overlay / Global Interaction as stacked siblings.
Navigation is a pure JavaScript reducer (`navmodel.js`) with an explicit
`nextDeadline()`, used from QML with a single-shot timer and tested with `node` on the
host (`scripts/check.sh`). Navigation idle is independent of display power.

## Consequences

- Protocol 1.1 and Core 1.4 are additive; Stage 3 clients and agents keep working.
- Laptop owners must run `pocket-desktop-agent grant …` for elevated tiles; until then
  those tiles say why they are unavailable.
- The phone's session bus now carries laptop notification content when forwarding is
  enabled. Any phone-session process can read it (#8). The default is off, and the
  agent's `show` command prints this caveat.
- Voice AI (future) reuses: the Global layer, the registry + grants for tools, live topics
  for task status. Audio needs its own channel; the JSON control channel is not for audio.
