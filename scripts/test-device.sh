#!/bin/sh
# Integration tests on the device, each on its own private session bus (dbus-run-session),
# so they never touch a running graphical session:
#   1. pocketd D-Bus contract tests (Rust, cross-built here, run the deployed pocketd):
#      core/apps (dbus_contract) and Dock (dock_contract, in-process agent on 127.0.0.1)
#   2. Pocket.Core bridge test (C++/QtTest, built on the device, runs the deployed pocketd
#      and a cross-built desktop agent on 127.0.0.1)
# Requires: scripts/build.sh && scripts/deploy.sh
set -eu
HOST="${POCKET_DEVICE:-lavender}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
export PATH="$HOME/.cargo/bin:$PATH"

echo "== pocketd contract tests (device)"
tests="$(cargo test --release --target aarch64-unknown-linux-musl -p pocketd --tests \
	--no-run --message-format=json 2>/dev/null |
	python3 -c 'import sys,json
for l in sys.stdin:
    m=json.loads(l)
    if m.get("reason")=="compiler-artifact" and m.get("executable") and m["target"]["name"] in ("dbus_contract","dock_contract"): print(m["target"]["name"], m["executable"])')"
[ "$(echo "$tests" | wc -l)" -eq 2 ] || { echo "could not build the contract tests" >&2; exit 1; }
cargo build --release --target aarch64-unknown-linux-musl -p pocket-desktop-agent
ssh "$HOST" 'mkdir -p ~/pocket-os/tests/bin'
echo "$tests" | while read -r name exe; do scp -q "$exe" "$HOST:pocket-os/tests/bin/$name"; done
scp -q target/aarch64-unknown-linux-musl/release/pocket-desktop-agent "$HOST:pocket-os/tests/bin/pocket-desktop-agent"
for t in dbus_contract dock_contract; do
	ssh "$HOST" "cd /tmp && dbus-run-session -- env POCKETD_BIN=\$HOME/pocket-os/bin/pocketd POCKETD_LOG=warn ~/pocket-os/tests/bin/$t --test-threads=1"
done

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
	POCKET_AGENT_BIN=$HOME/pocket-os/tests/bin/pocket-desktop-agent POCKET_AGENT_LOG=warn \
	"$b/pocket-core-qml-test/tst_coreclient"
REMOTE
echo "device integration tests passed"
