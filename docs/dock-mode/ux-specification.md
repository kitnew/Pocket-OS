# Dock Mode: UX specification

Status: **implemented in Stage 4** (rendering and behaviour verified on the phone through
IPC; a physical touch pass is still open, see [validation](../validation/stage4.md)).
Per-item status: [roadmap](roadmap.md). Visual language: `apps/shell/theme/Theme.qml` (ADR-0004).

## Geometry

The phone runs at scale 3: 1080×2340 px → **780×360 logical px** in landscape. The status
bar (28 px, top) and navigation bar (44 px, bottom) stay visible in Dock Mode, so the
Dock surface has **780×288**. Every layout below is designed for that box.

```
┌──────────────────────────────────────────────── status bar 28 ┐
│ ┌────┐ ┌───────────────────────────────────────────────────┐ │
│ │ ⌂  │ │                                                   │ │
│ │ ▦  │ │            base view (Home | Deck | Link)          │ │ 288
│ │ ⇄  │ │                     724 × 288                      │ │
│ │    │ │                                                   │ │
│ │ 📌 │ └───────────────────────────────────────────────────┘ │
│ └rail┘                                                       │
└─────────────────────────────────────────────── nav bar 44 ───┘
```

The **rail** (56 px) holds the view buttons (Home, Deck, Link) and the pin toggle. The
current view's button is highlighted; the pin is accented while pinned.

## Layers

| Layer | Contents | Lifetime | May interrupt idle return? |
|---|---|---|---|
| **Base View** | Ambient Home, Control Deck, Link (connection details) | Persistent; one active | — |
| **Context Overlay** | Notification banner (non-modal), action confirmation sheet (modal), deck result toasts | Temporary, above the base view; never replaces it | Modal overlays block idle return; banners do not |
| **Global Interaction** | Reserved for voice sessions / agent tasks (future) | Session-long | Blocks idle return while active |

Overlays are siblings above the base view. Opening or closing one never resets the base
view, the deck page or the pin.

## Ambient Home

Left half: **large clock** (HH:mm, minute resolution), **date**, then a compact status
column: laptop link (`nikitapc · Connected` / `Reconnecting in 4 s` / `Not connected`) and
phone battery (percent, charging).

Right half: **widget column**, top to bottom, each shown only when its source exists:

| Widget | Source | Shown when | States |
|---|---|---|---|
| Media | laptop `media` topic | the laptop reports at least one player | playing/paused/stopped; title, artist, player; prev / play-pause / next |
| Laptop | laptop `host` + `system` topics | connected and the topics are available | battery %, charging, CPU %, memory %, locked |
| Notifications | notification bridge | at least one notification in history | count + latest app/summary; tap → history |
| Calendar | — (no integration) | never in Stage 4 | — |
| Running task | — (no integration; workflows show progress on their tile) | never in Stage 4 | — |
| Connection hint | Core link state | not connected | "Laptop disconnected" + reason, never stale data |

If no widget applies, the right half shows nothing but the link status (calm, not empty
placeholders). Remote widgets disappear immediately when the link is not `connected`
and come back after the fresh snapshot (no stale data).

## Control Deck

- **Pages** are tabs (chips) along the top: General, Development, Media by default.
  Tapping a chip switches the page; the selected page is remembered while the shell runs.
- **Tiles** come from the deck configuration (`deck.json`, see
  [architecture](architecture.md#deck-configuration)). Grid: 4 columns × 2 rows visible
  (~170×96 px), scrolls vertically if a page has more.
- A tile shows an icon, a label and a one-line status.

Tile states (all driven by Core/agent reports, never local guesses):

| State | Look | Trigger |
|---|---|---|
| ready | raised surface, label | action advertised and available |
| pressed | darker surface, 0.97 scale | finger down (immediate feedback) |
| confirming | confirmation sheet open | action has `confirm: true` |
| busy | dimmed + small spinner, not tappable | Core accepted the request |
| progress | "Step 2/3 · Konsole" | workflow progress |
| success | green check + text, fades after 3 s | laptop confirmed |
| error | red outline + message, stays until next tap | laptop or Core reported a failure |
| unknown outcome | amber "Outcome unknown" + **Retry** | `timeout` / `connection_lost` |
| unavailable | dimmed, reason (e.g. "Not granted on nikitapc") | not advertised, unavailable, not granted, or not connected |

Retry of an unknown outcome re-sends the **same request key**: the laptop returns the
stored result if it already ran it (no double execution), or runs it once if it never
arrived. Retry is always a user tap; nothing retries automatically.

Touch feedback is visual (press state within one frame). The device has no feedback
daemon for haptics; haptics are out of scope.

## Link view

The Stage 3 Dock page content: computer name, address and key id, state, error text,
retry countdown, **Connect / Retry now / Disconnect**, and **Back to Handheld**. It is
also where "not paired" guidance appears.

## Navigation state machine

State (UI-owned, `apps/shell/features/dock/navmodel.js`):
`{ view, page, pinned, lastInteraction, modal, global, idleTimeoutMs }`.

```
            navigate(v)                 idle ≥ timeout ∧ ¬pinned ∧ ¬modal ∧ ¬global
   ┌──────────────────────────┐   ┌──────────────────────────────────────────────┐
   ▼                          │   │                                              │
 HOME ──navigate(deck)──► DECK ──navigate(link)──► LINK ─────────────────────────┤
   ▲                                                                             │
   └─────────────────────────────────────────────────────────────────────────────┘
```

Rules:

1. Ambient Home is the default view (session start, entering Dock Mode, shell restart).
2. The user navigates with the rail or the nav bar's Home button (always → Home).
3. After `idle_timeout` (default **15 s**, configurable 5–600 s or 0 = never) without
   interaction, a non-Home view returns to Home.
4. Pin (rail) keeps the current view: no idle return while pinned. Navigating to another
   view keeps the pin on the new view; tapping the pin again unpins. Pinning Home is
   allowed (it is a no-op for idle return).
5. Every touch anywhere on the Dock surface resets the idle timer (passive handler; it
   does not steal the touch).
6. An open modal (confirmation sheet) or an active Global Interaction suspends idle
   return. When it closes, the idle timer starts again from that moment.
7. Banners do not count as interaction and do not suspend idle return.
8. Link state changes never change the view. If the link drops while a confirmation sheet
   is open, the sheet closes and the user is told the action was not sent.
9. Leaving Dock Mode keeps nothing; entering Dock Mode starts at Home, unpinned. A shell
   restart starts at Home, unpinned (UI-owned state is transient by ADR-0004).

Timers: one single-shot timer armed for the exact deadline (`nextDeadline()`), re-armed
on interaction. No periodic polling. Navigation inactivity is independent of display
power: the display's own idle/power-key behaviour is unchanged.

## Notifications

- **Banner** (Context Overlay): top of the Dock surface, app name, summary, one line of
  body (or "Hidden while the laptop is locked"), auto-hides after 6 s (critical: stays
  until tapped). Tap dismisses it on the phone only.
- **History**: the Notifications widget opens a list (newest first, max 50, kept in the
  shell's memory: cleared by a shell restart, kept across Core restarts). "Clear" empties
  it.
- **Do Not Disturb**: no banners; history still fills. Persisted.
- **Per-app mute**: from a history entry ("Mute <app>"); muted apps are dropped by Core.
  Persisted; unmute from the history header.
- Closing a notification on the laptop removes it from the phone. Dismissing on the
  phone does not close it on the laptop.
- Notification actions (buttons) are not offered: Plasma gives no reliable way for a
  third party to invoke them.
- Nothing is shown while disconnected; notifications sent while disconnected are not
  replayed.

## Disconnect and reconnect

| Situation | Home | Deck | Overlays |
|---|---|---|---|
| connected | all live widgets | tiles by availability | normal |
| connecting / error with retry | link line shows "Reconnecting in N s"; remote widgets hidden | tiles "Not connected"; busy tiles resolve to unknown outcome | confirmation sheet closes |
| disconnected (user) | "Not connected" | tiles "Not connected" | — |
| reconnected | fresh snapshot, widgets reappear | availability refreshed | — |
| Core restart | as reconnect (Core restores the link intent) | pending tiles → "Pocket Core restarted" (unknown) | sheet closes |
