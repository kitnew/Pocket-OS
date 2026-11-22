#!/bin/sh
# Run the session lifecycle end-to-end test on the device against the running session
# (start it first: scripts/session.sh start) and fetch its screenshots + session log
# into .cache/e2e/<timestamp>/.
set -eu
HOST="${POCKET_DEVICE:-lavender}"
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/.cache/e2e/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"
tar -C "$root/tests/device" -c . | ssh "$HOST" 'mkdir -p ~/pocket-os/tests && tar -C ~/pocket-os/tests -x'
status=0
ssh "$HOST" 'rm -rf /tmp/pocket-e2e; OUT=/tmp/pocket-e2e sh ~/pocket-os/tests/session-e2e.sh' | tee "$out/e2e.txt" || status=$?
scp -q "$HOST:/tmp/pocket-e2e/*.png" "$out/" 2>/dev/null || true
scp -q "$HOST:.local/state/pocket-os/session.log" "$out/" 2>/dev/null || true
echo "evidence: $out"
exit "$status"
