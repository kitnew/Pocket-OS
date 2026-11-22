# Hardware compatibility: Xiaomi Redmi Note 7 (xiaomi-lavender)

All values below were **read from the device** on 2026-11-22 (commands in
`scripts/inspect-device.sh` and `docs/validation/stage0.md`), not taken from spec sheets.

## Identity and OS

| Item | Observed |
|---|---|
| Model (device tree) | `Xiaomi Redmi Note 7 (Tianma)` — Tianma panel variant, DTB `qcom/sdm660-xiaomi-lavender-tianma` |
| OS | `Nura edge` (`/etc/os-release`: `ID=nura`, `ID_LIKE=alpine`, bug tracker = postmarketOS pmaports). Repos: postmarketOS `main` + Alpine `edge` main/community/testing |
| Kernel | `7.2.15-sdm660` (`linux-postmarketos-qcom-sdm660-7.2.15-r2`), aarch64, SMP PREEMPT |
| Init | OpenRC 0.63.2; busybox init with getty on tty1–tty6 |
| Session/seat | elogind 255.24 (D-Bus activated; the OpenRC `elogind` service shows *stopped*, which is normal here), seat0 |
| Bus | dbus 1.16.4 system bus running |
| `su` | util-linux-login 2.42.4 (PAM-enabled); `sudo` available, password required; no `doas` |
| Network | USB gadget `usb0` 172.16.42.1/16 (host is 172.16.42.2). `wlan0` (ath10k) present but not connected → **no internet on the device** by default |
| sshd | `AllowTcpForwarding no` (distro policy, left unchanged) |

## CPU, memory, storage

| Item | Observed |
|---|---|
| CPU | 8 cores: 4× Kryo 260 "Falkor-V1/Kryo" + 4× "Kryo-V2" (SDM660), aarch64 |
| RAM | 3599 MiB total usable; ~3.1 GiB available at idle console |
| Swap | zram (`/dev/zram0`, ~5.3 GiB) + swapfile service |
| Root | `/dev/loop0p2` 50 GiB ext4 (image inside `mmcblk1p66` userdata); `/boot` 487 MiB |
| RTC | `/dev/rtc0` is **read-only** (`hwclock -w` → ENODEV). Without network the clock starts at 1970; pmOS `swclock-offset` keeps an offset. Set the clock before building (see dev guide) |

## Display and GPU

| Item | Observed |
|---|---|
| DRM driver | `msm_dpu` (card1, `c901000.display-controller`), `msm 1.13.0`; `simpledrm` used only during early boot |
| Nodes | `/dev/dri/card1` (root:video 0660), `/dev/dri/renderD128` (0666) |
| Connector | `DSI-1`, connected, single mode **1080×2340**, 67 mm × 145 mm |
| Panel | Novatek NT36672A (Tianma), video mode DSI |
| Backlight | `backlight` max 4095 |
| GPU | Adreno 512 (`adreno 5000000.gpu`, speedbin 0x87). Firmware `qcom/a530_pm4.fw`, `a530_pfp.fw` load **lazily on first GPU use** (the `-2`/`-40` errors at boot are the early attempts; the later "loaded … from new location" line confirms success) |
| Mesa | 26.3.1, Gallium **freedreno** (`msm_dri.so`), EGL 1.5 |
| Renderer | `OpenGL ES 3.1 Mesa 26.3.1`, renderer `FD512` — **hardware**, via GBM, surfaceless and Wayland EGL platforms. No llvmpipe fallback observed |
| KMS throughput | kmscube: **59.98 fps** (legacy, vsync-locked), atomic (`-A`) works (~57–60 fps) |

## Input and sensors

| Device | Node | Notes |
|---|---|---|
| `nt36672a-ts` touchscreen | event3 | Multitouch; exposed by Phoc as `wl_seat` capability `touch`; verified with physical taps |
| `pm8941_pwrkey` | event0 | **elogind default `HandlePowerKey=poweroff`: a short press powers the phone off** (observed once during Stage 0) |
| `pm8941_resin` (volume −), `gpio-keys` (volume +) | event1, event2 | |
| `spmi_haptics` | event4 | Vibrator |
| Headset jack | event5 | |
| IIO sensors | `qcom-smgr-accel/gyro/mag/prox` | Present via sensor manager; physical auto-rotation not attempted in Stage 0 |

## Known device limitations relevant to Pocket OS

1. No internet unless Wi-Fi is configured; Stage 0 used a host-side proxy over the USB link
   (`scripts/device-online.sh`).
2. Power key powers off immediately (elogind default). The shell must take a
   `handle-power-key` inhibitor before Stage 1 user testing.
3. RTC is not writable; clock is wrong after cold boot without network.
4. No permanent graphical session is configured; console getty on tty1 remains the default.
