# Pocket design system (Stage 4.5)

Source of truth: `apps/shell/theme/Theme.qml` (tokens) and `apps/shell/components/`
(primitives). Feature code uses tokens and components only — no literal colors, radii or
durations in `features/`.

## Principles

- **Glanceable first.** On the desk the phone is read from ~60 cm: one large thing per
  view (clock, tiles), secondary text muted, no debug detail as primary text.
- **Real state only.** Every state a control shows comes from its owner (Core, the
  laptop, PipeWire, NetworkManager…). Unknown is shown as unknown, never guessed.
- **Calm.** Dark neutral surfaces; color is information (accent = on/primary, category
  hues identify tiles, green/amber/red = state). Nothing animates while idle.
- **Touch first.** Targets ≥ 44 px (Dock primary controls 48–52 px); feedback within
  one frame (press scale + tint); destructive or disruptive actions need a deliberate
  second step.

## Tokens

| Group | Tokens |
|---|---|
| Surfaces | `background` #0b0e13 · `surface` #141922 · `surfaceRaised` #1c222d · `surfaceHigh` #262d3a · `surfacePressed` #303848 · `outline` · `outlineStrong` · `scrim` |
| Text | `text` · `textMuted` · `textDisabled` · `accentText` |
| State | `accent` #5b96ff (+`accentSoft`) · `success` · `warning` · `danger` (each with a `…Soft` 15 % tint) |
| Category | `categoryColor(c)`: apps violet, desktop blue, media pink, session amber, workflow teal |
| Type | `fontHero` 120 (Dock clock) · `fontDisplay` 56 · `fontHeadline` 24 · `fontTitle` 19 · `fontBody` 15 · `fontLabel` 13 · `fontCaption` 11; numbers use tabular figures |
| Space | 4 · 8 · 12 · 16 · 24 · 32; `gutter` 16 (screen edge) |
| Shape | `radiusSmall` 10 · `radius` 16 · `radiusLarge` 24; pills = height/2 |
| Size | `touchTarget` 44 · `touchLarge` 52 · `railWidth` 68 · icons 16/22/28 |
| Motion | `durationFast` 110 · `duration` 200 · `durationSlow` 280 ms; `easingEnter` OutQuart, `easingExit` InCubic; `travel` 28 px. All become 0 with **Reduce motion** |

Logical pixels only (Phoc scale 3 on lavender: 2340×1080 → 780×360 landscape).

## Components

| Component | Use |
|---|---|
| `Pressable` | Base of every tappable thing: press scale/tint, tap on release inside, optional long press (0.5 s) |
| `PocketButton` | Text (+icon) button; variants `tonal`, `primary`, `danger`, `ghost`; `compact` |
| `IconButton` | Round icon button (flat or filled, `checked`) |
| `Chip` | Pill with dot/icon + text (status, page move) |
| `ToggleTile` | Quick Settings switch/momentary tile with detail line; `dense` for landscape |
| `PocketSlider` | Filled pill slider with icon and value; horizontal drags only |
| `Sheet` | Modal: scrim + card from `center`, `right` or `bottom`; takes all input while open |
| `Spinner`, `Meter`, `TextBox`, `Icon`, `BatteryIndicator`, `AppIcon` | as named |

## Motion rules

1. Only transform and opacity animate (cheap on the Adreno 512); no layout animation of
   large trees, no shadows/blur.
2. View changes slide 28 px in the direction of travel; the outgoing view fades faster
   than the incoming one fades in.
3. Sheets rise/slide in with `easingEnter`; banners drop in from the top edge.
4. Animations never block input; a second tap during a transition acts on the new state.
5. Continuous animation only while something is actually busy and visible (spinners).
6. Reduce motion (Quick Settings, persisted by Core) sets every duration and travel to 0.

## Gestures

| Gesture | Where | Visible alternative |
|---|---|---|
| Swipe sideways | Deck pages | Page tabs |
| Swipe down from the top edge | Dock | Controls button on the rail |
| Long press | Deck tile → details | Tiles that cannot run open details on tap |
| Swipe banner up/sideways | Notification banner | Auto-hide, tap |
| Swipe sideways | Notification history entry → remove | Clear all |

Sliders take only horizontal drags; scroll areas only vertical ones; modal sheets take
every touch, so background gestures never fire under a dialog.
