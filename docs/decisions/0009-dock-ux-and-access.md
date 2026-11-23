# ADR-0009: Dock UX overhaul — immersive chrome, laptop-approved access, editable deck

- Status: **Accepted**, implemented in Stage 4.5 ([validation](../validation/stage4.5.md))
- Date: 2026-11-24
- Amends: [ADR-0008](0008-dock-platform.md) (grants can be approved from a phone request;
  live configuration; app launch outside the agent sandbox), [ADR-0004](0004-shell-ui-architecture.md)
  (mode-dependent system bars)
- Related: [#16](https://github.com/kitnew/Pocket-OS/issues/16), [#17](https://github.com/kitnew/Pocket-OS/issues/17), [#8](https://github.com/kitnew/Pocket-OS/issues/8)

## Context

The first touch pass on the real Redmi showed that Stage 4 worked but was not usable as a
desk companion: the status bar (28 px) and navigation bar (44 px) stayed on screen in Dock
Mode, leaving 780×288 of 780×360 and showing the time twice; Quick Settings clipped half
of its controls in landscape behind the navigation bar with no way to scroll; most Deck
tiles read "Not granted on nikitapc" because grants could only be set on the laptop's
command line and only applied after an agent restart; editing the deck meant editing
JSON on the phone. Testing the fixes also showed that apps the agent launched inherited
its systemd sandbox (read-only home).

## Decisions

### 1. Immersive Dock; the system bars follow the surface in front

`Shell.immersive = docked ∧ home surface visible ∧ home page`. While true, the status and
navigation bar surfaces are unmapped (their exclusive zones disappear, the Dock gets the
full 780×360). The Dock replaces what the bars gave: its own compact status (phone
battery, Wi-Fi, laptop notifications; the time only on views without the big clock), a
**Controls** rail button and a top-edge swipe that open Quick Settings, which holds
**Leave Dock** and **Phone apps**. As soon as anything else is in front (an app, the
launcher, the switcher) the bars come back, so handheld navigation never depends on Dock
UI. Rejected: hiding the bars unconditionally in Dock Mode (an app opened from the phone
would have no Home button); auto-hiding bars on a timer (Phoc has no edge gestures, so a
hidden bar would be unreachable); a second Dock-specific bar (two sources of truth).

### 2. Quick Settings is bounded by the free area and scrolls

The panel's height is the space between the mapped bars (or the screen edges when
immersive); content scrolls vertically only. Landscape uses two columns. Every control
must be reachable on the smallest layout; landscape on lavender shows all of them
without scrolling.

### 3. Access is requested by the phone and granted on the laptop

New basic action `access.request {action}` (closed list: elevated, enabled, not yet
granted actions). The agent shows an **Allow / Deny** notification through the laptop's
own notification server; only *Allow* clicked there adds the grant (saved to
config.json, applied immediately). This keeps ADR-0008's rule that authorization happens
on the laptop — the phone can only ask — while removing the need for a terminal. Limits:
one open prompt, 120 s timeout, 60 s cooldown after Deny. Editing a tile never grants
anything. #8's caveat is unchanged and shown in the prompt.
Rejected: granting from the phone UI (would make the phone its own authority); a PIN on
the phone (no secure input path, ADR-0008).

### 4. The agent applies its configuration live

Grants, enabled actions, workflows, registered apps and destinations live behind a lock in
the registry; SIGHUP re-reads config.json; every CLI edit sends it (and the unit has
`ExecReload`). Connected phones receive the new advertisement at once. Listen address,
identity, trusted phones and notification forwarding still need a restart (they change
the session or a privacy boundary).

### 5. Destinations (#16)

`desktop.open {destination}` (elevated) opens a laptop-configured `{id, label, app,
target}` through the app's desktop entry and its own `%f/%u` field code. Targets are
absolute/`~/` paths or http(s) URLs; nothing comes from the phone except the id.

### 6. Apps start outside the agent's sandbox

The agent runs with `ProtectHome=read-only`, `ProtectSystem=strict`, `PrivateTmp`.
Launched apps inherited that and its cgroup (could not save files; restarting the agent
killed them). They now start as transient user services (`systemd-run --user
--collect`), like any desktop launcher; the agent's own sandbox is unchanged except that
it may write its config directory (approved grants). Without a systemd user manager the
agent falls back to direct spawning.

### 7. Deck editing through Core (#17)

Core 1.5 `SetDeck`/`ResetDeck` validate with the same code as file loading and persist
atomically; `ListActions` carries each action's parameters (JSON, with `choices`), so the
phone can offer the laptop's real apps/places/shortcuts/workflows and can tell "not set
up" (tile names something the laptop does not offer) from "broken". The shell edits a
working copy; Done saves, Cancel discards; editing is a modal that suspends idle return.

### 8. Informational sheets do not pin a view

Navigation modals are split: decisions (`confirm`, `edit`) suspend idle return;
informational sheets (`detail`, `notifications`) do not, and idle return closes them. A
forgotten sheet left the Deck on screen forever in the first implementation.

## Consequences

- Core protocol 1.5 and the new agent actions are additive; Stage 4 clients and agents
  keep working (a 1.4 Core simply has no editor and no params; tiles fall back to the
  advertisement).
- Phone-launched apps no longer die with the agent and can write files.
- The laptop owner can approve access from the desk; revocation is still `pocket-desktop-agent
  revoke ID` (applies immediately).
