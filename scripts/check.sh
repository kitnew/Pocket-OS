#!/bin/sh
# Local checks: Rust format/lint/tests (incl. the D-Bus contract test on a private bus),
# shell-script syntax/lint, Python, shell logic (node).
#   scripts/check.sh            host only
#   scripts/check.sh --device   + qmllint on the device and the device integration tests
#                               (deploys user files first; needs scripts/build.sh)
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
export PATH="$HOME/.cargo/bin:$PATH"
echo "== cargo fmt";    cargo fmt --all -- --check
echo "== cargo clippy"; cargo clippy --workspace --all-targets -- -D warnings
echo "== cargo test";   cargo test --workspace --quiet
echo "== sh -n"
for f in scripts/*.sh scripts/device/* tests/device/*.sh packaging/openrc/pocket-session.initd; do sh -n "$f"; done
if command -v shellcheck >/dev/null; then
	echo "== shellcheck"
	shellcheck -S warning scripts/*.sh scripts/device/* tests/device/*.sh
fi
echo "== python"; python3 -m py_compile scripts/host-proxy.py scripts/soak-report.py
if command -v node >/dev/null; then
	echo "== node (shell logic)"; node --test tests/shell/*.test.mjs
else
	echo "== node not installed: shell logic tests skipped" >&2
fi
if [ "${1:-}" = "--device" ]; then
	echo "== qmllint (device)"
	"$root/scripts/deploy.sh" >/dev/null
	# Theme.qml's "not declared as singleton" warning is a known qmllint false positive when
	# the directory singleton is linted standalone; the runtime resolves it.
	ssh "${POCKET_DEVICE:-lavender}" 'cd ~/pocket-os/shell && find components features theme state -name "*.qml" | xargs /usr/lib/qt6/bin/qmllint -I . -I ~/pocket-os/qml && echo "qmllint ok (Quickshell-specific surfaces/ and shell.qml are validated by running them)"'
	"$root/scripts/test-device.sh"
fi
echo "all checks passed"
