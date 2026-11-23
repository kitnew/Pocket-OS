# Dock Mode: product vision

Status: **Stage 4 product definition; Stage 4 is implemented** (see the
[roadmap](roadmap.md) for per-module status and [validation](../validation/stage4.md)). Architecture: [architecture.md](architecture.md).

## What Dock Mode is

A phone docked next to a laptop is idle most of the day. Dock Mode turns it into a
**companion device** for that laptop: a calm, always-visible display and a small touch
surface for the things that are awkward on the laptop itself.

The phone does **presentation, touch and device I/O**. The laptop owns
**integrations, application control, privileged operations and (later) AI
orchestration**. The phone never re-implements Fedora-specific logic; it asks the
laptop's agent to do things and shows what the laptop reports.

Dock Mode is **not** a second desktop, a remote desktop, a remote shell or a Stream Deck
clone. It does not mirror laptop windows and it never sends arbitrary keystrokes or
commands.

## Usage scenarios

| Scenario | What the user does | What Dock Mode provides |
|---|---|---|
| Desk at rest | Glances at the phone while working | Ambient Home: large clock, date, laptop link, phone battery, now-playing, laptop vitals |
| Media while coding | Skips a track or changes volume without leaving the editor | Media widget and Media deck page (MPRIS, system volume) |
| Context switching | Opens a terminal, switches workspace, shows the overview | Control Deck tiles mapped to registered laptop actions |
| Stepping away | Locks the laptop from the phone | `session.lock` (granted on the laptop, confirmed on the phone) |
| Interruptions | Sees laptop notifications without looking at the laptop | Notification overlay and history, per-app filters, Do Not Disturb |
| Multi-step routines | "Start work": open terminal + editor + go to workspace 2 | Workflows defined on the laptop, progress shown on the phone |
| Future: voice | Says the activation phrase and asks for something | Global Interaction layer over any view (Stage 5+, not in Stage 4) |

## Non-goals (Stage 4)

- Remote desktop, screen mirroring, window thumbnails.
- Text input, arbitrary keyboard or pointer injection, clipboard or file transfer.
- Shell or command execution of any kind (ADR-0007; still excluded).
- Shutdown, reboot, logout or anything destructive on the laptop.
- More than one paired laptop at a time; Wi-Fi docking (#6, separate).
- A phone-side editor for the deck (configuration is a validated JSON file in Stage 4).
- Voice, wake word, microphone streaming, AI agents (designed for, not built).

## Functional modules

| Module | Purpose | Owner of logic |
|---|---|---|
| **Ambient Home** | Default Dock screen: clock, date, link, phone battery, adaptive widgets | Shell (presentation); data from Core, system services and the laptop |
| **Adaptive widgets** | Contextual cards that appear only when a real data source exists | Shell; providers are Core live topics and phone services |
| **Control Deck** | Pages of configurable action tiles with clear feedback | Deck config: Core (validated file); execution: laptop agent |
| **Navigation** | View switching, idle return to Home, pinning, overlays | Shell (UI-owned state machine) |
| **Host Actions** | Typed, registered, authorized laptop operations | Laptop agent (`pocket-desktop-agent`) |
| **Live host state** | Media, laptop vitals, lock state: snapshot + updates | Laptop agent publishes; Core relays and marks stale |
| **Notification bridge** | Laptop notifications on the phone, with privacy controls | Laptop agent observes; Core filters; shell presents |
| **Global Interaction** (future) | Voice sessions and agent tasks above any view | Reserved layer in the shell; orchestration on the laptop |

## UX principles

1. **Calm by default.** Dark, high-contrast, large type, no idle animation. Nothing moves
   unless something changed.
2. **Truthful.** Show only real data. No demo values. A missing source is hidden or says
   "unavailable"; disconnected remote data is never shown as current.
3. **Confirmed, not assumed.** A tile shows success only after the laptop confirmed it.
   An unknown outcome is shown as unknown, never silently retried.
4. **Predictable navigation.** Home is the anchor. Leaving it is explicit; returning is
   automatic after inactivity unless the view is pinned. Modals are never interrupted.
5. **Glanceable and touchable.** 44 px minimum targets; key information readable from
   arm's length at the device's real 780×360 logical landscape resolution.
6. **Cheap to run.** Event-driven updates, minute-resolution clock, no polling on the
   phone, timers armed only when needed.
7. **Secure by construction.** Authentication is not authorization. The laptop decides
   which actions exist and which are granted. Nothing generic is ever exposed.

## Evolution

- **Stage 4** (this milestone): Ambient Home, Control Deck, navigation, host actions with
  authorization, media and laptop state, notification bridge.
- **Stage 5+ (planned, issues filed):** phone-side caller isolation (#8: sandboxed apps,
  shell-only capability channel), in-UI deck editing, contextual pages pushed by the
  laptop (e.g. per active app), voice AI (Global Interaction layer, bidirectional audio,
  tool execution with confirmation), background task widgets, Wi-Fi docking (#6).
- The protocol is client-agnostic: another companion client (a tablet, a web view) can
  speak Pocket Dock to the same agent without changing laptop integrations.
