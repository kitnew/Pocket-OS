#!/bin/sh
# Temporary internet access for the (offline) device, for package installs only.
# Runs host-proxy.py in the foreground on the host's USB-link address; Ctrl-C stops it.
# On the device: export http_proxy=http://172.16.42.2:3128 https_proxy=$http_proxy
# (sudo resets the environment: pass it explicitly, e.g. sudo env http_proxy=... apk add ...)
set -eu
here="$(cd "$(dirname "$0")" && pwd)"
exec python3 "$here/host-proxy.py" "${POCKET_HOST_USB_ADDR:-172.16.42.2}" "${POCKET_PROXY_PORT:-3128}" "${POCKET_DEVICE_ADDR:-172.16.42.1}"
