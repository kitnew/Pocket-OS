# Audio integration (Stage 2.5)

PipeWire owns audio. Pocket adds only two things: a supervised session component that runs
the stack, and a shell adapter for volume and mute. Nothing audio-related is stored in
Pocket Core.

```
pocket-session-run
└─ pocket-supervise audio ── pocket-audio            (scripts/device/pocket-audio)
                              ├─ pipewire              graph; ALSA card via UCM (HiFi verb)
                              ├─ wireplumber           policy: profiles, routes, default sink,
                              │                        state in ~/.local/state/wireplumber
                              └─ pipewire-pulse        Pulse protocol for applications
Pocket Shell ── services/AudioService.qml ── Quickshell.Services.Pipewire (native client)
               └─ Shell.audio → Quick Settings: mute button + volume slider + status line
```

## Packages (postmarketOS-supported backend switch)

pmOS selects the audio server with a provider package. The device shipped with the
PulseAudio backend (PulseAudio installed but not running in the Pocket session). Stage 2.5
switched to the PipeWire backend:

```sh
device$ sudo sed -i 's/^postmarketos-base-ui-audio-backend-pulseaudio$/postmarketos-base-ui-audio-backend-pipewire/' /etc/apk/world
device$ sudo env http_proxy=$P https_proxy=$P apk add pipewire-tools alsa-utils   # resolves the new world
```

This removes `pulseaudio*` and `alsa-plugins-pulse`, and installs `pipewire-pulse`,
`pipewire-alsa`, `pipewire-tools`, `pipewire-spa-bluez`. PipeWire 1.6.11 and WirePlumber
0.5.19 were already installed. `alsa-utils` is kept explicitly for diagnostics. The
transaction removes `alsa-utils-udev`, so it ran the mkinitfs trigger, which **re-flashed
the boot partition**: see "Boot-image safety" in the development guide. Rollback: the same
`sed` in reverse, then `apk add alsa-utils`.

Quickshell is built with `-DSERVICE_PIPEWIRE=ON` (build dependency `pipewire-dev` in the
`.pocket-build-deps-audio` virtual package). `scripts/quickshell-build.sh` reuses the
existing build tree when the pinned source is unchanged, so an option change is an
incremental build (40 steps instead of a full ~50 min build).

## Lifecycle

- One component, `audio`, under the existing supervisor (no OpenRC user services: the
  user instance is login-scoped on this device, see ADR-0003). `pocket-audio` starts
  PipeWire, waits for `$XDG_RUNTIME_DIR/pipewire-0`, then starts WirePlumber and
  pipewire-pulse. If any member exits, it stops the others and exits non-zero, and the
  supervisor restarts the whole stack with the usual back-off and give-up rule.
  WirePlumber and pipewire-pulse cannot work without their daemon, so restarting them
  separately would only produce noisy failure loops.
- Every process runs as the session user. Access to `/dev/snd/*` comes from elogind's
  uaccess ACLs for the active seat session. The user is not in the `audio` group and
  needs no root.
- There is no RTKit on the device, so PipeWire falls back to normal priority (it logs a
  warning). Under heavy CPU load, audio can glitch. RTKit or rtprio limits are future work
  if that shows up.
- Control: `pocket-ctl restart|stop|start audio` (or `scripts/session.sh audio …`). The
  shell reconnects by itself: Quickshell watches the runtime dir for a new `pipewire-0`.
- Session stop: TERM goes to the supervisor, then to `pocket-audio`, then to each member.
  No audio process is left after `rc-service pocket-session stop` (checked in the
  validation report).

## Volume control

- `apps/shell/services/AudioService.qml` binds `Pipewire.defaultAudioSink` with a
  `PwObjectTracker` and exposes plain properties (`connected`, `available`, `volume`,
  `muted`, `outputName`) and `setVolume()`/`setMuted()`. Volume uses the same cubic scale
  as `wpctl` and pavucontrol. Range 0–100 %: there is no software boost.
- Every state change comes from PipeWire events. External changes (wpctl, pactl, apps, a
  default-sink change) update the slider. Nothing polls, and no command runs from QML.
- Status line: "No sound card detected" (`/proc/asound`, via `Pocket.Platform Audio`),
  "Audio service not running" (no PipeWire connection), "No audio output" (no default
  sink), otherwise "*output* · NN %" / "· muted".
- Diagnostics: `pocket.shell.diag: event=audio.state …` once per settled change.
  IPC: `pocket-ctl ui volume 40`, `pocket-ctl ui mute true`.
- WirePlumber persists route volumes (`~/.local/state/wireplumber/`), so a volume
  survives audio, shell and session restarts.

## Hardware state on lavender (what works, what does not)

| Item | Result |
|---|---|
| Card | `sdm660 - Xiaomi Redmi Note 7` (`snd-sm8250` machine driver, PM660L analog + digital codec, ADSP via APR). PCMs `hw:0,0` (MultiMedia1, playback + capture) and `hw:0,1` |
| UCM | `alsa-ucm-conf-qcom-sdm660` → `Xiaomi/lavender/HiFi.conf`: **Earpiece**, **Headphones**, **Mic** (built-in), **Headset Mic**. **No Speaker device** |
| PipeWire profiles | `HiFi (Headphones, Headset Mic)` (default, priority 8400), `HiFi (Earpiece, …)`, `HiFi (…, Mic)`, `pro-audio` |
| Jack detection | **Broken by a name mismatch.** UCM sets `JackControl "Headset Jack"`, but the kernel card exposes `Headphone Jack` and `Mic Jack`. Every route stays `available: unknown`, so WirePlumber always selects the Headphones profile and never switches to the earpiece or built-in mic. Fix belongs upstream (sdm660-mainline `alsa-ucm-conf`: `JackControl "Headphone Jack"` / `"Mic Jack"`) |
| Loudspeaker | Not supported by the current kernel/UCM: no Speaker UCM device, and no speaker amplifier on I²C (`/sys/bus/i2c/devices` lists only the touchscreen). The codec exposes `SPK DAC Switch`. Whether the speaker path needs an external amplifier enable (a device-tree change) was not resolved; see the validation report |
| Playback data path | Verified: a client stream reaches `hw:0,0` (PCM state `RUNNING`, `tests/device/audio-e2e.sh` §5) |
| Audibility | See `docs/validation/stage2.5.md` (physical test) |
