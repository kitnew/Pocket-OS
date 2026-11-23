# ADR-0004: Shell UI architecture

- Status: **Accepted** (implemented in Stage 2)
- Date: 2026-11-23
- Evidence: `docs/validation/stage2.md`

## Context

Stage 1 had one full-screen bottom-layer surface whose development panel created its own
`CoreClient`. Stage 2 needs several surfaces (status bar, navigation, home/launcher,
quick settings, overlays) that share Core state, system state and navigation, plus
reusable components and one visual language. Quickshell must stay replaceable:
ADR-0001 keeps Quickshell-specific code out of portable components.

## Decision

### Directory boundaries

| Directory | Contents | May use Quickshell? |
|---|---|---|
| `apps/shell/shell.qml` | Entry: creates services and surfaces, injects adapters into `Shell` | yes |
| `apps/shell/surfaces/` | One `PanelWindow` (wlr-layer-shell surface) per system surface | yes |
| `apps/shell/services/` | Adapters over Quickshell integrations (`ToplevelManager`, UPower, Networking, Bluetooth) exposing plain properties/functions | yes |
| `apps/shell/state/` | `Shell` singleton: shared state and actions | **no** |
| `apps/shell/features/` | Feature views (home, launcher, switcher, status bar, nav bar, quick settings, toasts, dev panel) | **no** |
| `apps/shell/components/` | Reusable controls | **no** |
| `apps/shell/theme/` | `Theme` tokens and monochrome icons | **no** |

Portable code depends only on Qt Quick and the two C++ QML modules:

- `Pocket.Core` (`bridge/pocket-core-qml`): `CoreClient` (core state, mode) and
  `AppCatalog`/`AppFilter` (application catalog and launching, ADR-0005).
- `Pocket.Platform` (`bridge/pocket-platform-qml`): adapters that need the shell's own
  Wayland connection or a system D-Bus service and have no Quickshell integration:
  `PowerKey` (phosh_private accelerator grab), `DisplayPower`
  (wlr-output-power-management), `OutputConfig` (wlr-output-management transform),
  `Brightness` (logind `SetBrightness`), `OrientationSensor` (iio-sensor-proxy),
  `OnScreenKeyboard` (`sm.puri.OSK0`), `Audio` (`/proc/asound`).

### Shared state: one `Shell` singleton

`state/Shell.qml` is a plain QML singleton (`pragma Singleton`, `qmldir`). It owns
**one** `CoreClient` per shell process. `AppCatalog` has no D-Bus connection of its own:
it borrows the `CoreClient`'s connection and follows its status. It also owns the
platform adapters, the UI-owned navigation state and the shell's diagnostic log. The
Quickshell layer injects `Shell.windows` and `Shell.system` at startup. Portable code
reads them by duck typing: `toplevels[]`, `activate()`, `close()`,
`batteryPercent`, `wifiEnabled`, …

There is no event bus. Components bind to properties and call `Shell` functions.

State ownership:

| Owner | State | Where the shell reads it |
|---|---|---|
| Pocket Core | mode, app catalog, launch results | `Shell.core`, `Shell.apps` |
| Compositor (Phoc) | windows, activation, output power, output transform | `Shell.windows`, `Shell.display`, `Shell.output` |
| System services | battery, network, Bluetooth, backlight, accelerometer, OSK visibility | `Shell.system`, `Shell.brightness`, `Shell.sensor`, `Shell.osk` |
| UI | `homeRequested`/`homeVisible`, `homePage`, `panel`, `rotationMode`, toasts | `Shell` properties |

Nothing system- or compositor-owned is copied into Pocket Core. UI-owned state is
transient: a shell restart returns to Home and portrait mode, and the output keeps its
current transform.

### Surfaces and layers

| Surface | Layer | Anchors / exclusive zone | Keyboard | Mapped |
|---|---|---|---|---|
| Status bar | top | top edge, zone = 28 | none | always |
| Navigation bar | top | bottom edge, zone = 44 | none | always |
| Home (home/launcher/switcher) | top | fill, respects zones | on demand (search field) | `Shell.homeVisible` |
| Developer panel | overlay | fill, respects zones | none | `panel === "dev"` |
| Quick Settings | overlay | fill, ignores zones (scrim) | none | `panel === "quicksettings"` |
| Overlay host (toasts) | overlay | bottom edge, input mask = toasts | none | while toasts exist |
| On-screen keyboard (squeekboard, separate client) | top | bottom, own zone | — | when shown |

Surfaces that are not needed are **unmapped**, not hidden behind transparent pixels, so
they never take touches meant for applications. The bars' exclusive zones make Phoc lay
out (maximise) application windows between them.

### Navigation model

- The navigation bar has Keyboard (toggle the OSK), **Home**, and **Open windows**
  (switcher, with a window count).
- Home is shown when requested, and always when no application window exists
  (`homeVisible = homeRequested || windowCount === 0`).
- Opening an app from Home or the launcher activates its existing window if one matches
  (Wayland app_id = desktop ID or `StartupWMClass`). Otherwise it launches the app
  through Core. A new window hides Home.
- The switcher activates or closes windows through wlr-foreign-toplevel. Phoc cannot
  minimise (ADR-0005), so "Home" means showing the Home surface above the windows.
- Status bar tap or a short downward drag opens Quick Settings. A tap on the scrim
  closes it.
- No edge gestures: Phoc has no gesture protocol for clients, and the bars already
  provide reliable touch targets.

### Design system

`theme/Theme.qml` defines color tokens (one dark theme), the typography scale, the
spacing scale (4–32), radii, a 44 px minimum touch target, bar sizes and motion
durations. Icons are white SVGs on a 24-unit grid in `theme/icons/`, loaded with
`Theme.iconUrl()`. Components in `components/` are `PocketButton`, `IconButton`,
`AppIcon` (with a lettered fallback), `AppTile`, `PanelContainer`, `ToggleTile`
(on/off/busy/unsupported), `PocketSlider`, `StatusIndicator` and `BatteryIndicator`.
There is no component library dependency: Qt Quick Controls is not used.

### Responsive layout

All sizes are logical pixels. Phoc's output scale (3 on lavender) does the rest.
Layouts depend on `width > height`, never on the device resolution. Home uses one
column in portrait and two in landscape. Grids compute their column count from the
available width. The quick settings tile grid uses 2 columns in portrait and 4 in
landscape. Core's `dock` mode is shown (status bar indicator, home label) but does not
change the layout yet. The Dock dashboard is Stage 3.

## Consequences

- Adding a surface = one file in `surfaces/` + a feature view in `features/`
  (docs/development/README.md §9).
- A second QML host could reuse `state/`, `features/`, `components/` and `theme/` by
  providing its own `windows`/`system` adapters.
- UI state does not survive a shell restart. That is intended for Stage 2.
