# ADR-0005: Applications, window management and compositor integration

- Status: **Accepted** (implemented in Stage 2)
- Date: 2026-11-23
- Evidence: `docs/validation/stage2.md`, `docs/validation/evidence/stage2/wayland-registry.txt`

## Context: what Phoc 0.59 actually offers

Verified two ways: the live Wayland registry, read by a client of the running session
(`wayland-info`), and the Phoc 0.59.0 sources (tag `v0.59.0`, `src/*.c`).

| Need | Interface on lavender | Verdict |
|---|---|---|
| Shell surfaces | `zwlr_layer_shell_v1` v3 (+ `zphoc_layer_shell_effects_v1` v4) | used (Quickshell `PanelWindow`) |
| App windows | `xdg_wm_base` v6. Phoc maximises toplevels to the usable area | used |
| Enumerate windows | `zwlr_foreign_toplevel_manager_v1` v3, `ext_foreign_toplevel_list_v1` v1 | used (wlr: Quickshell `ToplevelManager`) |
| Activate / close a window | wlr-foreign-toplevel `activate`, `close` | used |
| **Minimise** | the request exists, but **Phoc ignores it** ("We don't do window menus or minimize", `xdg-toplevel.c`) | **not available** |
| Match window ↔ process | `phoc_foreign_toplevel_pidfd_manager_v1` (not in Quickshell); app_id ↔ desktop ID works | app_id matching |
| Startup notification | `xdg_activation_v1` v1; phosh_private startup tracker | not used yet |
| Input method / OSK | `zwp_input_method_manager_v2` v1, `zwp_text_input_manager_v3` v2, `zwp_virtual_keyboard_manager_v1` v1 | used by squeekboard |
| Output power | `zwlr_output_power_manager_v1` v1. Phoc **drops touch input** while the built-in output is off (`seat.c` `handle_touch_down`) | used |
| Output transform/scale | `zwlr_output_manager_v1` v4 (phosh_private `rotate_display` is a stub that posts an error) | used |
| Power key | `phosh_private` v8 `get_keyboard_event` → `grab_accelerator_request("XF86PowerOff")`; XF86 keysyms are grabbable (`keysym_is_subscribeable`) | used |
| Idle | `ext_idle_notifier_v1` v2, `zwp_idle_inhibit_manager_v1` | not used yet |
| **Session lock** | **`ext_session_lock_manager_v1` is not offered** (no implementation in Phoc 0.59 sources) | **not available** |
| Screen capture (tests) | `zwlr_screencopy_manager_v1` v3, `ext_image_copy_capture_manager_v1` v1 | used by `grim` |

Privileged globals (output power, output management, phosh_private, foreign toplevel,
…) are hidden only from clients that run inside a `wp_security_context_v1`. The shell
and SSH tools have no security context, so they see them.

## Decision

### Applications: catalog and launching in the core (`pocket-platform`)

- `crates/pocket-platform` (new, Rust, no D-Bus/Qt) scans the XDG application
  directories in priority order. It uses `freedesktop-desktop-entry` for spec parsing,
  locales and `Exec` quoting/field codes, and `freedesktop-icons` for icon lookup
  (Adwaita → hicolor → pixmaps). It applies the visibility rules: `Type`, `Hidden`,
  `NoDisplay`, `OnlyShowIn`/`NotShowIn` against `XDG_CURRENT_DESKTOP=Pocket`,
  `TryExec`, `Terminal` (not supported yet → hidden), and an existing program.
  Commands are fixed at scan time and never pass through a shell. Unsupported or
  leftover field codes reject the entry instead of running a half-expanded command.
- `pocketd` serves it as `org.pocketos.Core1.Apps` on `/org/pocketos/Core1/Apps`
  (protocol **1.1**, additive): `ListApps`, `Launch(id) → pid`, `Reload`, signals
  `AppsChanged` (inotify on the application directories, debounced, no polling) and
  `AppExited(id, pid, success, detail)`.
  `DBusActivatable=true` apps are activated through `org.freedesktop.Application`
  first, with Exec as the fallback.
- Launched processes inherit the session environment. The session gives `pocketd` (and
  only `pocketd`, not Phoc) `WAYLAND_DISPLAY`. Processes run in their own process
  group, so restarting the core does not kill apps. They end with the logind session.
- The shell's `AppCatalog` mirrors the catalog over the shared Core connection. The
  launcher shows launch failures (D-Bus error, early non-zero exit, no window within
  20 s) as toasts.

Why the core and not QML/Quickshell (`DesktopEntries.execute()`): launching is an
authoritative system action that Dock Mode and the future desktop agent also need. It
must be unit-testable, must not depend on the UI runtime, and must outlive shell
restarts. `pocket-core`'s `StateManager` is unchanged: the catalog is platform data, not
Pocket state.

### Windows: wlr-foreign-toplevel through Quickshell

`services/WindowService.qml` wraps Quickshell's `ToplevelManager`. The shell activates
and closes windows and tracks the active one. Because Phoc cannot minimise, **Home is a
top-layer surface shown above the application windows**. It is unmapped when an app is
activated, so the app receives all input again. Re-opening an app from Home activates
its existing window (app_id = desktop ID or `StartupWMClass`, case-insensitive) instead
of starting a second instance. When the last window closes, Home is shown again.

### System overlays and the keyboard

Status and navigation bars are top-layer surfaces with exclusive zones. Quick Settings,
the developer panel and toasts use the overlay layer and are mapped only while needed
(toasts limit their input region with a mask). The on-screen keyboard is **squeekboard**
1.43 (Phosh's OSK; input-method-v2 + virtual-keyboard), run by the session as the
supervised component `osk`. It shows itself when a text-input-v3 field gains focus
(GSettings `screen-keyboard-enabled=true` through a private keyfile backend; the device
has no dconf). The navigation bar can also toggle it over `sm.puri.OSK0`.

### Display power, power key and rotation (Pocket.Platform, in the shell)

- Power key short press → grabbed through phosh_private → `DisplayPower` toggles
  wlr-output-power. Phoc ignores touches while the panel is off. elogind's
  `HandlePowerKey=ignore` stays, so a press never shuts the system down, even if the
  shell is not running. A long hardware press still resets the device.
- Rotation: manual Portrait/Landscape, or Auto from iio-sensor-proxy (accelerometer
  claimed only while Auto is on and the display is on), applied as the output transform
  through wlr-output-management. Phoc remaps touch input with the transform.
- Brightness: logind `Session.SetBrightness` (no root, active session only).

### Session lock: deferred

(Superseded in detail by [ADR-0006](0006-secure-lock-screen.md), which investigates the
Phoc/Phosh mechanisms and records the strategy.)

Phoc 0.59 has no `ext-session-lock-v1`. A lock implemented as an ordinary layer
surface would not be secure: if the shell crashes or is restarted, the compositor shows
the unlocked session, and other clients keep running and can map surfaces. Phosh's own
lock screen relies on the trusted shell process for exactly this reason. Stage 2 ships
**no lock screen**. Turning the display off is described as display power management
only. There is no PIN or password anywhere.

## Alternatives considered

- **Replacing or patching Phoc** (minimise, session lock): rejected. Every Stage 2
  requirement has a supported mechanism, and a fork is out of scope.
- **Home as the bottom-layer background**, with minimise to reveal it: impossible
  without minimise.
- **Launching from QML** (`Quickshell.execDetached`, `DesktopEntries.execute()`): ties a
  system action to the UI runtime; rejected (see above).
- **wvkbd** (virtual-keyboard only): it has no input-method support, so it cannot show
  itself on focus or commit text through text-input. Rejected in favour of squeekboard.
  phosh-osk-stevia is a drop-in alternative behind the same `sm.puri.OSK0` interface.
- **`wlr-randr` subprocess for rotation**: works, but is a shell-out from the UI.
  The protocol client is about 200 lines and gives success/failure events.

## Consequences / follow-ups

- Smallest viable fix for minimise: a Phoc change that maps foreign-toplevel `minimize`
  to hiding the view. It is not needed while Home overlays windows.
- Secure lock: needs `ext-session-lock-v1` in Phoc (upstream wlroots provides the
  helper), or an audited compositor-side lock. Revisit when Phoc ships it.
- A window ↔ process match for apps whose app_id differs from their desktop ID could use
  `phoc_foreign_toplevel_pidfd_manager_v1` with the pid that `Launch` returns.
