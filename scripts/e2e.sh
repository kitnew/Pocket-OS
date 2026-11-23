#!/bin/sh
# Run an end-to-end test on the device against the running session (start it first:
# scripts/session.sh start) and fetch its screenshots + session log into
# .cache/e2e/<timestamp>/.
#   scripts/e2e.sh            Stage 1 session lifecycle (tests/device/session-e2e.sh)
#   scripts/e2e.sh stage2     Stage 2 shell functions (tests/device/stage2-e2e.sh)
#   scripts/e2e.sh all        both
set -eu
HOST="${POCKET_DEVICE:-lavender}"
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/.cache/e2e/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"
tar -C "$root/tests/device" -c . | ssh "$HOST" 'mkdir -p ~/pocket-os/tests && tar -C ~/pocket-os/tests -x'
case "${1:-stage1}" in
stage1) tests="session-e2e" ;;
stage2) tests="stage2-e2e" ;;
all) tests="session-e2e stage2-e2e" ;;
*) sed -n '2,8s/^# \{0,1\}//p' "$0" >&2; exit 2 ;;
esac
status=0
for t in $tests; do
	ssh "$HOST" "rm -rf /tmp/pocket-e2e; OUT=/tmp/pocket-e2e sh ~/pocket-os/tests/$t.sh" | tee "$out/$t.txt" || status=$?
	scp -q "$HOST:/tmp/pocket-e2e/*.png" "$out/" 2>/dev/null || true
done
scp -q "$HOST:.local/state/pocket-os/session.log" "$out/" 2>/dev/null || true
echo "evidence: $out"
exit "$status"
