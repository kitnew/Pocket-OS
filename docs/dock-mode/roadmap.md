# Dock Mode: Stage 4 roadmap

Tracking issue: [#9](https://github.com/kitnew/Pocket-OS/issues/9) · milestone
*Stage 4 — Dock Mode platform*. Status legend: **done** (implemented and verified as
stated), **partial**, **planned**. Evidence and the remaining manual checks:
[`docs/validation/stage4.md`](../validation/stage4.md).

## Order and dependencies

```
4.1 UI foundation (#10) ──► 4.3 live state (#14) ──► 4.4 notifications (#15)
       │                         ▲
       ▼                         │ (media topic reuses the agent's MPRIS tracker)
#11 authorization ─┐             │
#12 request keys ──┴──► 4.2 host actions (#13) ──► #16 destinations
4.1 ──► #17 deck editor        Stage 4 (#9) ──► #18 voice AI (future)
```

Deviation from the suggested order: the authorization model (#11) and request keys (#12)
are built **before** the new elevated host actions, because ADR-0007/#8 forbid adding
such actions without an explicit gate, and retry semantics change the wire format that
every action uses.

## Stage 4.1 — Dock UI foundation (#10)

| Deliverable | Status |
|---|---|
| Deck configuration in Core (default + user file, validation, `GetDeck`/`ReloadDeck`/`DeckChanged`) | **done** |
| Navigation model `navmodel.js` + node tests | **done** |
| DockView (rail, three layers), Ambient Home, Control Deck, Link view | **done** (rendering and IPC-driven behaviour verified on hardware; physical touch pass pending) |
| Confirmation sheet; tile states | **done** (same caveat) |

Acceptance: see #10. Tests: `cargo test -p pocketd` (deck), `node tests/shell/navmodel.test.mjs`,
`scripts/test-device.sh`, `scripts/dock-e2e.sh`.

## Stage 4.2 — Host actions (#11, #12, #13)

| Deliverable | Status |
|---|---|
| Protocol 1.1: privilege, idempotent, confirm, choices/bounds, keys, duplicate, progress | **done** |
| Agent grants + CLI (`grant`, `revoke`, `apps`, `notifications`) | **done** |
| Agent dedup cache; Core `Retry` | **done** |
| `media.next/previous`, `volume.up/down/mute` (basic) | **done** (volume verified on the real KDE; media transport against fakes and Stage 3's play/pause) |
| `app.launch`, `desktop.shortcut`, `session.lock`, `workflow.run` (elevated) | **done** (`app.launch` and the lock confirmation verified on hardware; shortcut/lock effects and workflows against fakes/tests) |
| Predefined destinations / projects | planned ([#16](https://github.com/kitnew/Pocket-OS/issues/16)) |

## Stage 4.3 — Media and live state (#14)

| Deliverable | Status |
|---|---|
| `subscribe` / `state` topics `host`, `media`, `system` | **done** (hardware) |
| Core live cache, `GetLive`/`LiveChanged`, clear on session end | **done** |
| Home widgets: Media (with controls), Laptop | **done** |
| Active application, running tasks | not available (no reliable source; widgets hidden) |

## Stage 4.4 — Notification bridge (#15)

| Deliverable | Status |
|---|---|
| Agent monitor + policy (opt-in, allow/deny, redact when locked, no echo) | **done** (hardware) |
| Core prefs (DND, muted apps), signals | **done** |
| Banner overlay, history, DND toggle, per-app mute | **done** (banner + DND verified on hardware; sheet buttons need the touch pass) |

## Testing strategy

| Layer | Tool | What |
|---|---|---|
| Pure logic | `cargo test`, `node --test` | deck validation, navigation transitions and deadlines, registry authorization, dedup cache |
| Agent ↔ fake desktop | `crates/pocket-desktop-agent/tests/` with private `dbus-daemon` | MPRIS, kglobalaccel, ScreenSaver, notifications, launching (fake launcher) |
| Core contract | `crates/pocketd/tests/dock_contract.rs` (real pocketd + in-process agent) | retry/dedup across connection loss, live topics cleared/resynced, deck and prefs persistence across Core restart |
| Bridge | `tst_coreclient` on the device | DockClient 1.4 members |
| Hardware | `scripts/dock-e2e.sh` + screenshots + manual touch checklist | real phone + real Fedora agent; non-disruptive actions only automatically |

Automated tests never touch the real desktop except `desktop.notify` (as in Stage 3).
Disruptive actions (lock, workspace switch, launching apps, media toggles) are verified
manually on hardware and recorded as such.

## Future: Voice AI (#18)

Not part of Stage 4. Stage 4 reserves: the Global Interaction layer (blocks idle
return), tool execution through the registry with grants and `confirm`, a `tasks` live
topic for long-running work. Voice needs a separate audio channel, a voice ADR, and the
phone's audio output issues (#2, #3) resolved.

## Definition of done

Stage 4 (#9) is done when: the docs describe the implemented system; issues reflect
reality; Home shows real information; the deck runs configurable actions; core Fedora
actions work end to end; media control and state sync work; supported notifications
reach the phone; idle return and pinning behave as specified; disconnects, reconnects and
service restarts are handled; implemented host operations satisfy ADR-0008; automated
tests pass; physical tests are done or listed as unverified; Stage 3.1 behaviour has not
regressed.
