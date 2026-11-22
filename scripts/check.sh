#!/bin/sh
# Local checks: Rust format/lint/tests and shell-script syntax.
# QML is linted on the device (qmllint needs the target Qt): scripts/check.sh --device
set -eu
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
echo "== cargo fmt";    cargo fmt --all -- --check
echo "== cargo clippy"; cargo clippy --workspace --all-targets -- -D warnings
echo "== cargo test";   cargo test --workspace --quiet
echo "== sh -n"
for f in scripts/*.sh scripts/device/* packaging/openrc/pocket-session.initd; do sh -n "$f"; done
command -v shellcheck >/dev/null && shellcheck -S warning scripts/*.sh scripts/device/*
echo "== python"; python3 -m py_compile scripts/host-proxy.py
if [ "${1:-}" = "--device" ]; then
	echo "== qmllint (device)"
	"$root/scripts/deploy.sh" >/dev/null
	ssh "${POCKET_DEVICE:-lavender}" 'cd ~/pocket-os/shell && find components features theme -name "*.qml" | xargs /usr/lib/qt6/bin/qmllint -I . && echo "qmllint ok (Quickshell-specific surfaces/ and shell.qml are validated by running them)"'
fi
echo "all checks passed"
