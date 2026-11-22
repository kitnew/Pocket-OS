# ADR-0001: Initial technology stack

- Status: **Accepted**. Stage 0 evidence; reconfirmed at the end of Stage 1. Follow-ups:
  IPC implemented in [ADR-0002](0002-pocket-core-and-ipc.md); `-E` coupling and the
  power key (risks 3 and 4) resolved in [ADR-0003](0003-graphical-session-lifecycle.md)
  and `docs/validation/stage1.md`
- Date: 2026-11-22
- Evidence: `docs/validation/stage0.md`

## Context

Pocket OS needs a mobile shell on xiaomi-lavender (SDM660, 4 GB RAM) that can grow into
handheld and Dock modes. We have to keep it maintainable by a small team, use the
existing Linux stack, and avoid writing a compositor or a distribution. Stage 0 tested
each layer on the real device before we commit to it.

## Decision

| Layer | Choice | Stage 0 evidence |
|---|---|---|
| OS | **postmarketOS edge / Alpine** (as installed: "Nura edge", kernel 7.2.15-sdm660) | Boots, mainline msm DRM, hardware freedreno GLES 3.1 |
| Init | **OpenRC** | Manual `pocket-session` service starts and stops the session cleanly |
| Session/seat | **elogind** (PAM, seat0 on a dedicated VT) | Phoc gets DRM and input through logind with no permission changes |
| Compositor | **Phoc 0.59** (packaged) | Runs on DSI-1 and exposes layer-shell, fractional-scale, output-management and touch |
| UI toolkit | **Qt 6.12 / Qt Quick / QML** (packaged) | RHI OpenGL ES on FD512, vsynced 60 fps, touch works |
| Shell runtime | **Quickshell 0.3.2, built from source on the device** | Layer-shell surface, touch, rotation re-layout and restart all work |
| Core language | **Rust** (stable; Cargo workspace) | `pocket-ipc` and `pocket-core` build, lint and test on the host |
| IPC | **D-Bus session bus** (implemented in Stage 1, ADR-0002) | A per-session bus is started by `dbus-run-session`, and Quickshell connects to it |

## Rationale

- **postmarketOS/Alpine + OpenRC**: already installed and working. Mainline graphics
  work. Small footprint. Changing distribution is out of scope.
- **Phoc**: built for phones (touch, scaling, panel cutouts, output transforms), packaged
  for this device, wlroots-based, and speaks standard protocols. We use it as a normal
  Wayland compositor and avoid its `phosh_private` protocol.
- **Qt Quick**: hardware-accelerated scene graph on this GPU, mature touch handling,
  declarative layouts that fit responsive handheld and Dock UIs.
- **Quickshell**: provides layer-shell surfaces, multi-surface shell structure, live
  reload, and ready integrations (UPower, MPRIS, notifications, toplevel management).
  That saves building our own C++ host for QML. We keep its footprint contained:
  Quickshell types appear only in `apps/shell/shell.qml` and `apps/shell/surfaces/`.
- **Rust for the core**: memory-safe, single static-ish binary, good D-Bus (`zbus`) and
  async ecosystem. Packaged on the device (rust 1.100).
- **D-Bus**: the transport every integrated service already uses. It is introspectable
  and native in both QtDBus and Rust.

## Alternatives considered

| Alternative | Why not (now) |
|---|---|
| Phosh as the shell | It is a complete GTK shell; Pocket's UI would be built around it instead of being Pocket's own |
| Sway / other wlroots compositors | Not phone-oriented (touch gestures, scaling and cutouts would be extra work). Kept only for diagnostics if Phoc misbehaves |
| Custom compositor | Out of scope and high risk |
| **Standalone Qt Quick app + layer-shell-qt 6.8.3** (packaged) | Viable fallback. It is packaged for Qt 6.12 and needs no private-API rebuilds, but needs our own C++ host and loses Quickshell's integrations and reload. It becomes the plan if Quickshell maintenance (below) turns out too costly. The runtime-agnostic QML layout keeps this switch cheap |
| GTK4 / Flutter / Slint UI | Weaker fit for a QML-style compositional shell, or a heavier toolchain on aarch64/musl |
| Varlink / custom socket IPC | Less tooling, and our system services all use D-Bus anyway |

## Consequences and unresolved risks

1. **Quickshell is not packaged for Alpine** and depends on **private Qt APIs**. Any
   Qt upgrade on the device (e.g. `qt6-qtdeclarative` 6.12.0-r2 → later) requires
   re-running `scripts/quickshell-build.sh`, or Quickshell may crash. A build takes
   tens of minutes on the device, and the build dependencies are ~2.5 GiB (removable
   with `apk del .pocket-build-deps`). Mitigations: pin and rebuild deliberately. In the
   longer term, package it as an APK (`packaging/alpine/`) or switch to the
   layer-shell-qt fallback.
2. **Quickshell is LGPL-3.0**; its terms apply when we redistribute it (e.g. in an APK). Pocket's own
   code is MIT and only *uses* Quickshell as a runtime.
3. **`-E` coupling**: if the shell exits, Phoc and the session exit with it. That is
   fine for development. A shell supervisor may be needed before daily use.
4. **Power key**: elogind powers off on a short press. The shell or core must take an
   inhibitor before any end-user testing.
5. **Edge rolling release**: package versions move. Every `apk add` must be simulated to
   avoid kernel or bootloader updates (this device flashes the kernel on update).
6. Physical auto-rotation (IIO accelerometer via `iio-sensor-proxy`) is untested.
