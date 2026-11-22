#!/bin/sh
# Integration tests on the device, each on its own private session bus (dbus-run-session),
# so they never touch a running graphical session:
#   1. pocketd D-Bus contract test (Rust, cross-built here, runs the deployed pocketd)
#   2. Pocket.Core bridge test (C++/QtTest, built on the device, runs the deployed pocketd)
# Requires: scripts/build.sh && scripts/deploy.sh
set -eu
HOST="${POCKET_DEVICE:-lavender}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
export PATH="$HOME/.cargo/bin:$PATH"

echo "== pocketd contract test (device)"
out="$(cargo test --release --target aarch64-unknown-linux-musl -p pocketd --test dbus_contract \
	--no-run --message-format=json 2>/dev/null |
	python3 -c 'import sys,json
for l in sys.stdin:
    m=json.loads(l)
    if m.get("reason")=="compiler-artifact" and m.get("executable") and m["target"]["name"]=="dbus_contract": print(m["executable"])' | tail -n1)"
[ -n "$out" ] || { echo "could not build the contract test" >&2; exit 1; }
ssh "$HOST" 'mkdir -p ~/pocket-os/tests/bin'
scp -q "$out" "$HOST:pocket-os/tests/bin/dbus_contract"
ssh "$HOST" 'cd /tmp && dbus-run-session -- env POCKETD_BIN=$HOME/pocket-os/bin/pocketd POCKETD_LOG=warn ~/pocket-os/tests/bin/dbus_contract --test-threads=1'

echo "== Pocket.Core bridge test (device)"
ssh "$HOST" 'rm -rf ~/pocket-os-build/pocket-core-qml-src && mkdir -p ~/pocket-os-build/pocket-core-qml-src'
tar -C bridge/pocket-core-qml -c . | ssh "$HOST" 'tar -C ~/pocket-os-build/pocket-core-qml-src -xm'
ssh "$HOST" sh -eu <<'REMOTE'
b=~/pocket-os-build
cmake -S "$b/pocket-core-qml-src" -B "$b/pocket-core-qml-test" -G Ninja -DPOCKET_BUILD_TESTS=ON \
	-DCMAKE_BUILD_TYPE=RelWithDebInfo >/dev/null
cmake --build "$b/pocket-core-qml-test" --target tst_coreclient >/dev/null
cd /tmp
dbus-run-session -- env POCKETD_BIN=$HOME/pocket-os/bin/pocketd POCKETD_LOG=warn \
	"$b/pocket-core-qml-test/tst_coreclient"
REMOTE
echo "device integration tests passed"
