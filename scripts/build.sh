#!/bin/sh
# Build Pocket OS components for the device.
#   scripts/build.sh            both
#   scripts/build.sh rust       pocketd, cross-compiled on the host (static aarch64-musl,
#                               rust-lld; see .cargo/config.toml)
#   scripts/build.sh bridge     Pocket.Core QML module (C++/QtDBus), built natively on the
#                               device against the device's Qt, installed to ~/pocket-os/qml
# Then: scripts/deploy.sh
set -eu
HOST="${POCKET_DEVICE:-lavender}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
what="${1:-all}"
export PATH="$HOME/.cargo/bin:$PATH"

if [ "$what" = all ] || [ "$what" = rust ]; then
	echo "== pocketd (host cross-build)"
	rustup target list --installed | grep -qx aarch64-unknown-linux-musl ||
		rustup target add aarch64-unknown-linux-musl
	cargo build --release --target aarch64-unknown-linux-musl -p pocketd
fi

if [ "$what" = all ] || [ "$what" = bridge ]; then
	echo "== Pocket.Core QML bridge (native build on $HOST)"
	# No writable RTC on lavender: a clock far in the past makes CMake regenerate forever.
	# Sources are extracted with tar -m (mtime = device "now"), but warn loudly anyway.
	skew=$(($(ssh "$HOST" date +%s) - $(date +%s)))
	if [ "${skew#-}" -gt 3600 ]; then
		echo "WARNING: device clock is off by ${skew}s. Fix it first:" >&2
		echo "  ssh -t $HOST \"sudo date -u -s @\$(date +%s)\"" >&2
		[ "${POCKET_IGNORE_CLOCK:-}" = 1 ] || exit 1
	fi
	ssh "$HOST" 'rm -rf ~/pocket-os-build/pocket-core-qml-src && mkdir -p ~/pocket-os-build/pocket-core-qml-src'
	tar -C bridge/pocket-core-qml -c . | ssh "$HOST" 'tar -C ~/pocket-os-build/pocket-core-qml-src -xm'  # -m: clock skew
	ssh "$HOST" sh -eu <<'REMOTE'
b=~/pocket-os-build
cmake -S "$b/pocket-core-qml-src" -B "$b/pocket-core-qml-build" -G Ninja \
	-DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_INSTALL_PREFIX="$HOME/pocket-os/qml" >/dev/null
cmake --build "$b/pocket-core-qml-build"
rm -rf ~/pocket-os/qml/Pocket/Core
cmake --install "$b/pocket-core-qml-build" >/dev/null
ls ~/pocket-os/qml/Pocket/Core
REMOTE
fi
