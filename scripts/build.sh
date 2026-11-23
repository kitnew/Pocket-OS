#!/bin/sh
# Build Pocket OS components for the device.
#   scripts/build.sh            both
#   scripts/build.sh rust       pocketd, cross-compiled on the host (static aarch64-musl,
#                               rust-lld; see .cargo/config.toml)
#   scripts/build.sh bridge     Pocket.Core (QtDBus client of pocketd) and Pocket.Platform
#                               (Wayland/D-Bus system adapters) QML modules, built natively
#                               on the device against the device's Qt, into ~/pocket-os/qml
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
	for m in pocket-core-qml pocket-platform-qml; do
		ssh "$HOST" "rm -rf ~/pocket-os-build/$m-src && mkdir -p ~/pocket-os-build/$m-src"
		tar -C "bridge/$m" -c . | ssh "$HOST" "tar -C ~/pocket-os-build/$m-src -xm" # -m: clock skew
	done
	ssh "$HOST" sh -eu <<'REMOTE'
b=~/pocket-os-build
for m in pocket-core-qml pocket-platform-qml; do
	cmake -S "$b/$m-src" -B "$b/$m-build" -G Ninja \
		-DCMAKE_BUILD_TYPE=RelWithDebInfo -DCMAKE_INSTALL_PREFIX="$HOME/pocket-os/qml" >/dev/null
	cmake --build "$b/$m-build"
done
rm -rf ~/pocket-os/qml/Pocket/Core ~/pocket-os/qml/Pocket/Platform
for m in pocket-core-qml pocket-platform-qml; do
	cmake --install "$b/$m-build" >/dev/null
done
ls ~/pocket-os/qml/Pocket/*
REMOTE
fi
