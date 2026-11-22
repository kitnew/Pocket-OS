#!/bin/sh
# Build Quickshell natively on the device into a user prefix (~/pocket-os/opt/quickshell).
# Quickshell is not packaged for Alpine/postmarketOS; it uses private Qt APIs and MUST be
# rebuilt after every Qt upgrade on the device (re-run this script).
#
# Host: fetches pinned sources (needs internet), copies them to the device, builds there.
# Device prerequisites: the `.pocket-build-deps` packages (see docs/development/README.md).
set -eu
HOST="${POCKET_DEVICE:-lavender}"
QS_REPO="${QS_REPO:-https://git.outfoxxed.me/quickshell/quickshell.git}"
QS_REF="${QS_REF:-v0.3.2}"
CLI11_VER="${CLI11_VER:-2.5.0}"
JOBS="${JOBS:-4}"   # 4 GB RAM: more parallel C++ jobs swaps heavily

root="$(cd "$(dirname "$0")/.." && pwd)"
cache="$root/.cache/quickshell"
mkdir -p "$cache"

if [ ! -d "$cache/quickshell/.git" ]; then
	git clone --quiet "$QS_REPO" "$cache/quickshell"
fi
git -C "$cache/quickshell" fetch --quiet --tags
git -C "$cache/quickshell" -c advice.detachedHead=false checkout --quiet "$QS_REF"

if [ ! -f "$cache/CLI11-$CLI11_VER.tar.gz" ]; then
	curl -fsSL -o "$cache/CLI11-$CLI11_VER.tar.gz" \
		"https://github.com/CLIUtils/CLI11/archive/refs/tags/v$CLI11_VER.tar.gz"
fi

echo "== copying sources to $HOST"
ssh "$HOST" 'rm -rf ~/pocket-os-build/quickshell-src && mkdir -p ~/pocket-os-build'
git -C "$cache/quickshell" archive --format=tar --prefix=quickshell-src/ HEAD |
	ssh "$HOST" 'tar -C ~/pocket-os-build -xm'  # -m: tolerate device clock skew
scp -q "$cache/CLI11-$CLI11_VER.tar.gz" "$HOST:pocket-os-build/"

echo "== building on $HOST (Quickshell $QS_REF, -j$JOBS)"
ssh "$HOST" CLI11_VER="$CLI11_VER" JOBS="$JOBS" QS_REF="$QS_REF" sh -eu <<'EOF'
b=~/pocket-os-build
deps=$b/deps
prefix=~/pocket-os/opt/quickshell

# CLI11 is header-only; install it into a private prefix used only for this build.
rm -rf "$b/CLI11-$CLI11_VER" "$deps"
tar -C "$b" -xzf "$b/CLI11-$CLI11_VER.tar.gz"
cmake -S "$b/CLI11-$CLI11_VER" -B "$b/cli11-build" -G Ninja -DCMAKE_INSTALL_PREFIX="$deps" \
	-DCLI11_BUILD_TESTS=OFF -DCLI11_BUILD_EXAMPLES=OFF -DCLI11_BUILD_DOCS=OFF >/dev/null
cmake --install "$b/cli11-build" >/dev/null

cmake -S "$b/quickshell-src" -B "$b/quickshell-build" -G Ninja \
	-DCMAKE_BUILD_TYPE=RelWithDebInfo \
	-DCMAKE_INSTALL_PREFIX="$prefix" \
	-DCMAKE_PREFIX_PATH="$deps" \
	-DDISTRIBUTOR="Pocket OS dev build (source, $QS_REF)" \
	-DGIT_REVISION="$QS_REF" \
	-DCRASH_HANDLER=OFF -DX11=OFF -DI3=OFF -DHYPRLAND=OFF \
	-DSERVICE_PIPEWIRE=OFF -DSERVICE_PAM=OFF -DSERVICE_POLKIT=OFF -DSERVICE_GREETD=OFF \
	-DBLUETOOTH=OFF -DNETWORK=OFF
nice cmake --build "$b/quickshell-build" -j "$JOBS"
rm -rf "$prefix"
cmake --install "$b/quickshell-build" >/dev/null
"$prefix/bin/quickshell" --version
EOF
