# Stage 2.5 validation: Stabilization & Hardware Integration

- Date: 2026-11-23
- Device: Xiaomi Redmi Note 7 (`xiaomi-lavender`), postmarketOS edge, kernel 7.2.15-sdm660
  (unchanged), Phoc 0.59.0, Qt 6.12.0, Quickshell 0.3.2 (rebuilt with PipeWire), PipeWire
  1.6.11, WirePlumber 0.5.19, squeekboard 1.43.1.
- Baseline: Stage 2 `c8bae14`. Evidence: `docs/validation/evidence/stage2.5/`.

## Summary

| Workstream | Result |
|---|---|
| A. Audio | **PASS (software); hardware partly BLOCKED.** PipeWire/WirePlumber/pipewire-pulse run supervised as the session user. Volume and mute work on the real sink, in both directions. The playback path reaches `hw:0,0`. Loudspeaker: **BLOCKED** (no Speaker in UCM/kernel). Jack detection: **BLOCKED** (UCM control-name mismatch, upstream). Audibility: see "Physical" |
| B. Memory | **PASS.** 36 min soak, 80+ interaction cycles, 0 errors, 0 restarts. No sustained growth beyond transparent-huge-page effects |
| C. Landscape OSK | **PASS.** Keyboard 180 → 144 px, app area 108 → 172 px (of 360). Deterministic keyboard/nav-bar stacking (a bug found and fixed) |
| D. Secure lock | **Investigated, deferred** ([ADR-0006](../decisions/0006-secure-lock-screen.md)). No insecure substitute shipped |
| E. Reliability | **PASS.** Boot-flash risk identified and documented. Host backup verified by hashes. Controlled reboot OK; all suites pass after it |

## Automated tests

| Test | Result |
|---|---|
| `scripts/check.sh` (fmt, clippy, Rust unit + D-Bus contract, `sh -n`, shellcheck, python) | PASS |
| Stage 1 lifecycle e2e (`scripts/e2e.sh`) | **29/29 PASS**, before and after the reboot |
| Stage 2 functional e2e (`scripts/e2e.sh stage2`) | **56 PASS, 0 FAIL, 1 SKIP** (the known IPC-only activation skip), before and after the reboot |
| New audio e2e (`scripts/e2e.sh audio`, `tests/device/audio-e2e.sh`) | **33/33 PASS**, before and after the reboot |
| Soak (`scripts/e2e.sh soak`, `tests/device/soak.sh`) | 0 errors, 0 component restarts during the run |

The audio e2e checks, against independent sources (wpctl, pactl, `/proc/asound`, process
table):
- the stack runs as the user, with no PulseAudio;
- the default sink is ALSA card 0, and the shell shows its name;
- UI volume and mute reach PipeWire; wpctl and pactl changes reach the shell;
- a 440 Hz test stream drives `hw:0,0` to `RUNNING`;
- volume persists across an audio restart (WirePlumber state);
- `SIGKILL` of PipeWire or WirePlumber restarts the whole stack, with exactly one of each
  process afterwards;
- stop/start of audio gives a clear "not connected" state, then reconnects;
- shell and Core restarts leave audio untouched.

Session stop left **0** Pocket/audio processes (checked before the reboot).

## A. Audio

| Check | Result |
|---|---|
| ALSA card | `sdm660 - Xiaomi Redmi Note 7`, PCM `hw:0,0`/`hw:0,1`, UCM verb `HiFi` |
| Devices (UCM) | Earpiece, Headphones, built-in Mic, Headset Mic. **No Speaker** |
| PipeWire sink | `Built-in Audio Headphones (3.5 mm jack)` (default, because jack state is `unknown`) |
| Jack detection | **BLOCKED**: UCM uses `Headset Jack`; the kernel exposes `Headphone Jack`/`Mic Jack` (both read `off` correctly). Fix upstream in sdm660 `alsa-ucm-conf` |
| Loudspeaker | **BLOCKED**: no UCM device, no amplifier on I²C; needs device-port work (kernel/DT/UCM), out of scope |
| Volume/mute control | PASS (e2e §3–4) |
| Playback reaches hardware | PASS (PCM `RUNNING`) |
| Audible output | **NOT TESTED** physically (no headphones attached during the session; the earpiece is not routed by default because of the jack bug) |
| Microphone | Devices present (Mic, Headset Mic; capture PCM `hw:0,1`). Recording **NOT TESTED** (not in scope) |
| Restarts (audio, shell, core), session stop, reboot | PASS |

Details and rollback: [`../development/audio.md`](../development/audio.md).

## B. Memory and stability

Soak: fresh session, 150 s idle baseline, then IPC-driven cycles (Home ↔ launcher, Foot
launch/close, switcher + activate, Quick Settings, developer panel, OSK show/hide,
portrait ↔ landscape, GTK widget factory every third cycle, Core `GetState`/`ListApps`,
volume). Sampling every 30 s from `/proc/*/smaps_rollup`, `stat` and DRM `fdinfo`. The run
was stopped after 36 min of activity (planned: 40 min plus a final idle phase), so there
is no final-idle sample. Full table: `evidence/stage2.5/soak-report.md`. Raw data:
`soak.csv` (600 rows), `soak-events.txt`.

| Process | PSS baseline → after warm-up (9 min) → end (36 min) | Anonymous PSS | THP | Notes |
|---|---|---|---|---|
| Quickshell (shell) | 242 → 261 → 260 MiB (peak 275) | 140 → 160 → 168 | 128 → 118 → 130 | RSS 341 → 369 MiB |
| Phoc | 81 → 85 → 82 | 23.3 → 23.5 → 23.5 | 6–8 | GPU (imported client buffers) varies with open windows |
| squeekboard | 28 → 34 → 27 | 5 → 8 → 8 | 0 | |
| pocketd | 3.0 → 3.0 → 3.0 | 0.3 flat | 0 | |
| PipeWire / WirePlumber / pipewire-pulse | 3.4 / 64→54 / 1.8 | flat | 0 | WirePlumber PSS is mostly shared libraries (libcamera monitor) |
| System used (Total − Available) | 910 → 1010 → 1045 MiB of 3.6 GiB, peak 1138 MiB, swap 0 | | | |

CPU, average over the activity phase: shell 15 %, Phoc 10 %, squeekboard 5 % of one
core; pocketd and audio about 0. Idle: shell 0.1 %, others 0.

Assessment:
- Warm-up (first ~9 min): the shell grows by ~19 MiB PSS, as Stage 2 also saw. This is
  QML components, glyph and image caches, and JS heap being created for every page
  used.
- After warm-up, shell PSS is **flat** (261 → 260 MiB). Its anonymous PSS rises by
  ~8 MiB while AnonHugePages rises by ~12 MiB. The system runs THP `always` with
  khugepaged `max_ptes_none=511`, and Quickshell uses jemalloc: khugepaged collapses
  partly used 2 MiB regions into huge pages, which raises RSS without new allocations.
  The same effect showed up during the idle baseline (THP 96 → 128 MiB with no
  activity).
- Phoc, pocketd and the audio stack show no growth. No crashes and no restarts.
- **No memory leak is demonstrated, so no fix was made.** Remaining uncertainty: a run of
  several hours and a THP-disabled comparison (`MALLOC_CONF=thp:never`) were not done.
  GPU memory per client is sampled but fluctuates with the windows that are open.

One squeekboard exit (`status=1`) happened at session start, 1 s after Phoc started,
before the soak. The supervisor restarted it. This start-up race existed before and has
no effect on the user.

## C. Landscape OSK

Diagnosis. squeekboard sizes its panel from the output width and a fixed layout aspect
ratio, so in landscape it took 180 of 360 logical px. Status bar (28) and navigation bar
(44) left 108 px for the focused app. Phoc reserves the keyboard's exclusive zone
correctly: Foot was resized. The cause was keyboard sizing plus shell bars, not the
compositor.

Changes:
1. `scripts/device/pocket-osk`: squeekboard's supported GSettings key
   `scale-in-horizontal-screen-orientation=0.8` (override:
   `POCKET_OSK_LANDSCAPE_SCALE`). The keyboard is 144 px tall. Keys stay wider than in
   portrait (about 46 × 29 px in the 5-row terminal layout). 0.75 was tried and rejected:
   keys too small.
2. The status bar is unmapped while the OSK is visible in landscape. Navigation (Home,
   keyboard dismiss, switcher) stays.
3. Bug found while testing: after a shell restart the keyboard sat **below** the
   navigation bar. Phoc gives the edge to the oldest exclusive surface in a layer, and
   squeekboard keeps its surface. The navigation bar now uses the overlay layer, so the
   order is always bar at the edge and keyboard above it (ADR-0004).

Result (screenshots taken during the work): Foot in landscape has 172 px above the
keyboard (was 108). Portrait is unchanged. Rotation with the keyboard open re-lays out
correctly. The keyboard returns to normal after hiding.

| Physical check | Result |
|---|---|
| Portrait/landscape typing in Foot, GTK text input, launcher search, orientation change while typing, dismissal | **NOT TESTED by touch in Stage 2.5.** Verified over IPC and screenshots only. Physical typing was verified in Stage 2 (portrait/launcher/Foot); the Stage 2.5 changes are layout-only |

## D. Secure lock screen

[ADR-0006](../decisions/0006-secure-lock-screen.md):
- `ext-session-lock-v1` is absent in Phoc 0.59 and in Phoc `main`.
- Phosh's lock is an ordinary overlay surface with in-process PAM. It is safe only
  because Phoc exits when Phosh exits.
- Phoc's `-S` shield hides the screen when the shell disconnects but does not block input
  (`allow_input` is never read).
- Unsandboxed clients can use layer-shell and virtual-keyboard.

Decision: defer, and implement `ext-session-lock-v1` in Phoc upstream first. No lock
screen and no PIN exist in Pocket OS.

## E. Device reliability

- Finding: on lavender, every mkinitfs trigger regenerates `/boot/boot.img` **and `dd`s it
  to the `boot` partition** (`deviceinfo_flash_kernel_on_update="true"`). The Stage 2.5
  audio transaction triggered it, through the removal of `alsa-utils-udev`. The image
  came out byte-identical (SHA-256 `6fca4e0c…`), so the flash rewrote the same bytes.
- Backup on the host before any change: `~/pocket-os-device-backups/lavender/20261123-stage2.5-pre/`
  (64 MiB partition, `/boot` tar, versions). The SHA-256 of the partition matches on
  device and host. The partition starts with exactly `/boot/boot.img`, the image the
  device booted from.
- `scripts/boot-backup.sh backup|verify` makes this repeatable. After the reboot, `verify`
  reported: identical to the backup.
- Restore by `fastboot flash boot` is documented, but **not exercised**: no destructive
  restore was done, and fastboot mode was not entered.
- Controlled reboot after the package changes: boot OK, same kernel, SSH back, nothing
  started automatically, session started manually, all suites pass.

## Device changes and rollback

| Change | Rollback |
|---|---|
| `/etc/apk/world`: `postmarketos-base-ui-audio-backend-pulseaudio` → `…-pipewire`; `apk add pipewire-tools alsa-utils`. Removed: pulseaudio, pulseaudio-alsa/-bluez/-openrc/-wireplumber, alsa-plugins-pulse, gstreamer + gst-plugins-base (and libs), libcanberra-gstreamer, webrtc-audio-processing-1. Added: pipewire-pulse(+openrc), pipewire-alsa, pipewire-tools, pipewire-spa-bluez (+ codec libs). Log: `evidence/stage2.5/apk-audio-install.log`; world before: `apk-world-before.txt` | Reverse the `sed` in world, `apk add alsa-utils` (pulls the PulseAudio backend back) |
| `apk add -t .pocket-build-deps-audio pipewire-dev` (build only) | `apk del .pocket-build-deps-audio` |
| Boot artifacts regenerated + boot partition re-flashed (identical bytes) | `fastboot flash boot <backup>/boot-partition.img` |
| Quickshell rebuilt with `SERVICE_PIPEWIRE=ON` | `scripts/quickshell-build.sh` with the option OFF |
| User state: `~/.local/state/wireplumber/` (volumes) | delete the directory |

No services enabled or disabled. No changes to the kernel, bootloader, firmware,
partitions, elogind, sudoers or autostart.
