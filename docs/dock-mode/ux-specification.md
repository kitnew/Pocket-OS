# Dock Mode: UX specification

Status: **Stage 4.5 redesign implemented** ([validation](../validation/stage4.5.md),
[ADR-0009](../decisions/0009-dock-ux-and-access.md)). Visual language:
[design system](design-system.md). Stage 4 version: git history of this file.

## Geometry and system chrome

The phone runs at scale 3: 1080×2340 px → **780×360 logical px** in landscape.

The Dock is **immersive**: while its home surface is in front, the status and navigation
bars are unmapped and the Dock uses the whole screen. They come back whenever something
else is in front (an app, the launcher, the window switcher) and in handheld mode.

```
┌──────┬───────────────────────────────────────────────────────────────────┐
│  ⌂   │                          ▬  ← swipe down: Controls                │
│  ▦   │                                                                   │
│  ▭•  │                 base view (Home | Deck | Laptop)                  │ 360
│      │                          712 × 360                                │
│  📌  │  (pin: only on Deck/Laptop)                                       │
│  ⚙   │  ← Controls (Quick Settings)                                      │
└rail──┴───────────────────────────────────────────────────────────────────┘
  68
```

What replaces the bars:

| Was in the bars | On the immersive Dock |
|---|---|
| Time | Ambient Home's clock; small time in the Deck and Laptop headers (never both) |
| Battery, Wi-Fi | Compact status at the top right of every view |
| Quick Settings (tap/drag status bar) | Rail **Controls** button, swipe down from the top edge |
| Home button | Rail Home; Quick Settings ▸ **Phone apps** opens the launcher (bars return) |
| Keyboard button | The keyboard appears with text fields (deck editor) |
| — | Quick Settings ▸ **Leave Dock**; Laptop view ▸ **Leave Dock** |

## Layers

| Layer | Contents | Blocks idle return? |
|---|---|---|
| Base View | Ambient Home, Control Deck, Laptop | — |
| Context Overlay | banner (non-modal); confirmation and deck editor (decision modals); tile details and notification history (informational modals) | decisions: yes; informational: no — idle return closes them |
| Global Interaction | reserved for voice (#18) | yes |

Modal sheets take every touch while open; nothing below reacts.

## Ambient Home

```
● nikitapc                                         🔔 2  ▭ 100%
 23:09                         ┌─────────────────────────────┐
 Monday, 23 November           │ • Now playing · YouTube Music│
                               │ Stuck in a Timeloop          │
                               │ Animadrop      ⏮  ⏯  ⏭      │
                               └─────────────────────────────┘
 ▭ 100%   CPU ▂▂▂▂ 39%                 MEMORY ▆▆▆▆ 72%
```

- Hero clock (120 px, tabular) and date; centred when no player exists, left when the
  media card shows.
- Status line: laptop link chip (green connected / amber reconnecting with countdown /
  red error / grey offline; tap → Laptop view), notifications (tap → history), phone
  battery.
- Media card only with a player; controls enabled only when the laptop says they can run.
- Vitals strip only while connected: laptop battery and lock, CPU/memory meters (dimmed
  when stale). Tap → Laptop view.
- Disconnected: remote content disappears (Core clears it), one calm line says what to do.

## Control Deck

- Header: page tabs (tap), ✎ edit, status with the time.
- Pages swipe sideways (snap per page) and show dots; 4 × 2 tiles fill a page (≈ 180×146
  px); more than 8 tiles scroll vertically within the page.
- Offline: a banner with the reason/countdown and Connect / Retry now; tiles dim.

### Tile kinds (`state/deckmodel.js`, in priority order)

| Kind | Look | Tap |
|---|---|---|
| running | accent outline + spinner, "Running…" or "Step 2/3 · …" | — |
| offline | dimmed | — |
| missing (laptop does not offer the action) | dimmed, "Not on nikitapc" | details |
| unauthorized | lock badge, "Needs permission" | details with **Ask nikitapc** |
| unconfigured (app/place/shortcut not offered) | info badge, "Not set up" | details (+ Edit tile) |
| unavailable | dimmed, human reason ("Nothing playing", "KDE shortcuts unavailable") | — |
| uncertain | amber outline, "Tap to retry" | Retry (same request key) |
| failed | red outline + badge, short reason | runs again |
| done | green tint + ✓ for 2.5 s | runs again |
| ready | plain | runs (confirmation first if the laptop asks for it) |

Long press on any tile opens its details: what it does, its state in words, and the way
forward (Run/Retry, Ask, Edit tile). Raw agent text is shown there, never as the primary
label.

### Editing (#17)

✎ enters edit mode (a decision modal: no idle return). Tiles show ✎ and "Tap to edit"; a
dashed **Add tile** cell ends each page; the current tab shows ✎ (tap → page sheet) and
＋ adds a page. Tile editor: label, icon, the laptop's actions and their choices (apps,
places, shortcuts, workflows; ungranted ones are marked "needs permission" and can still
be placed), move earlier/later or to another page, remove. Page sheet: title, move,
remove, reset the deck to the default (two taps). **Done** saves through Core (validated);
**Cancel** discards. Editing never runs or interrupts an action and never grants anything.

## Laptop view

Left: the laptop (name, state with countdown or error, address and key id), Connect /
Retry now / Disconnect, **Leave Dock**. Right: **Permissions** — every elevated action the
laptop offers, Allowed or Not allowed, with **Ask** (an Allow/Deny prompt on the laptop;
"Waiting for Allow on nikitapc" until answered).

## Quick Settings

Bounded by the free area (below/above mapped bars; the full screen on the Dock), scrolls
vertically. Landscape: two columns — brightness, volume + mute, output, Leave Dock / Phone
apps / Screen off / Developer | Wi-Fi, Bluetooth, Dock mode, Do not disturb, Rotation,
Keyboard, Reduce motion. Portrait: one column. Close with ✕ or a tap outside. Every tile
reflects real state and does what it says; unsupported ones are dimmed with the reason.

## Navigation

State: `{ view, page, pinned, lastInteraction, modal, global, idleTimeoutMs }`
(`state/navmodel.js`). Rules:

1. Ambient Home is the default (entering Dock Mode, shell restart).
2. Rail: Home, Deck, Laptop; the pin (shown on Deck/Laptop) keeps the view.
3. After the idle timeout (15 s default, 0 = never) a non-Home view returns Home, unless
   pinned or a decision modal (confirm, edit) or a Global Interaction is open.
   Informational modals (details, notification history) are closed by idle return.
4. Every touch resets the timer; banners do not.
5. Link changes never change the view; a confirmation closes if the link drops ("not sent").

## Notifications

Banner drops in from the top (6 s; critical stays); tap opens the history, swipe up or
sideways dismisses. History is a right-side panel: DND, clear all, swipe an entry to
remove it, Mute per app, Unmute all. Closing on the laptop removes it from the phone.

## Disconnect and reconnect

Unchanged from Stage 4: remote widgets and tile availability follow Core's session;
pending tiles resolve to "Tap to retry"; nothing stale is ever shown.
