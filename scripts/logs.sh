#!/bin/sh
# Fetch session logs from the device into .cache/logs/<timestamp>/ and print the shell's
# diagnostic events. Root-only system logs are fetched with --system (asks for sudo).
set -eu
HOST="${POCKET_DEVICE:-lavender}"
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/.cache/logs/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"
scp -q "$HOST:.local/state/pocket-os/session.log*" "$out/" 2>/dev/null || echo "no session log yet"
ssh "$HOST" 'loginctl list-sessions; cat /sys/class/tty/tty0/active' > "$out/sessions.txt" 2>&1
if [ "${1:-}" = "--system" ]; then
	ssh -t "$HOST" 'sudo sh -c "logread | tail -n 500; echo; dmesg | tail -n 200"' > "$out/system.txt"
fi
echo "logs: $out"
grep -h "pocket.shell.diag" "$out"/session.log 2>/dev/null | tail -n 20 || true
