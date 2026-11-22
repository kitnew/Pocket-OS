#!/bin/sh
# Read-only inspection of the target device (no root needed). Prints a report to stdout.
HOST="${POCKET_DEVICE:-lavender}"
exec ssh "$HOST" sh -s <<'REMOTE'
section() { printf '\n## %s\n' "$1"; }
section "Device";   cat /proc/device-tree/model; echo; uname -a; . /etc/os-release; echo "$PRETTY_NAME (ID_LIKE=$ID_LIKE)"
section "CPU/RAM";  echo "cpus: $(nproc)"; free -m; df -h / /boot
section "DRM";      for c in /sys/class/drm/card*-*; do echo "$c: $(cat $c/status) $(head -1 $c/modes)"; done; ls -l /dev/dri
section "GPU/EGL";  EGL_PLATFORM=surfaceless eglinfo -p surfaceless 2>/dev/null | grep -E "EGL driver name|OpenGL ES profile (vendor|renderer|version)" || echo "eglinfo missing (apk add mesa-utils)"
section "Input";    grep -E "^N: Name" /proc/bus/input/devices
section "Seats";    loginctl list-seats; loginctl list-sessions; echo "active VT: $(cat /sys/class/tty/tty0/active)"
section "Services"; rc-status default | grep -E "\[" ; rc-service pocket-session status 2>&1 | head -1
section "Packages"; apk info -v 2>/dev/null | grep -E "^(phoc|qt6-qtbase|qt6-qtdeclarative|qt6-qtwayland|layer-shell-qt|mesa|linux-postmarketos)-[0-9]"
section "Quickshell"; ~/pocket-os/opt/quickshell/bin/quickshell --version 2>/dev/null || echo "not built"
REMOTE
