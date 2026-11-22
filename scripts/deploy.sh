#!/bin/sh
# Deploy Pocket OS to the device (~/pocket-os). No root needed for the default mode.
#   scripts/deploy.sh            shell QML, session scripts, config, device tests, and the
#                                built artifacts (pocketd, QML bridge) if present
#   scripts/deploy.sh --system   additionally install the root-owned files (asks for sudo):
#                                OpenRC service, elogind power-key drop-in, sudoers rule
# Build first: scripts/build.sh. Quickshell itself: scripts/quickshell-build.sh
#
# POCKET_SUDO_PASSWORD_FILE=<file>  feed sudo from a file instead of an interactive prompt.
set -eu
HOST="${POCKET_DEVICE:-lavender}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
target=target/aarch64-unknown-linux-musl/release

ssh "$HOST" 'mkdir -p ~/pocket-os/bin ~/pocket-os/config ~/pocket-os/tests ~/pocket-os/qml && rm -rf ~/pocket-os/shell.new'
tar -C apps -c shell | ssh "$HOST" 'mkdir -p ~/pocket-os/shell.new && tar -C ~/pocket-os/shell.new --strip-components=1 -x && rm -rf ~/pocket-os/shell && mv ~/pocket-os/shell.new ~/pocket-os/shell'
tar -C scripts/device -c . | ssh "$HOST" 'tar -C ~/pocket-os/bin -x && chmod 755 ~/pocket-os/bin/*'
tar -C config -c . | ssh "$HOST" 'tar -C ~/pocket-os/config -x'
tar -C tests/device -c . | ssh "$HOST" 'tar -C ~/pocket-os/tests -x'
# Replace binaries by rename so a running pocketd keeps its (old) inode until restarted.
if [ -x "$target/pocketd" ]; then
	scp -q "$target/pocketd" "$HOST:pocket-os/bin/.pocketd.new"
	ssh "$HOST" 'mv -f ~/pocket-os/bin/.pocketd.new ~/pocket-os/bin/pocketd'
else
	echo "warning: $target/pocketd not built (scripts/build.sh); not deployed" >&2
fi
echo "deployed user files to $HOST:~/pocket-os"

if [ "${1:-}" = "--system" ]; then
	user="$(ssh "$HOST" id -un)"
	ssh "$HOST" 'rm -rf ~/pocket-os/system && mkdir -p ~/pocket-os/system'
	scp -q packaging/openrc/pocket-session.initd packaging/openrc/pocket-session.confd \
		packaging/elogind/50-pocket-os-dev.conf packaging/sudoers/pocket-os-dev "$HOST:pocket-os/system/"
	script="
		s=~$user/pocket-os/system
		install -m 755 \$s/pocket-session.initd /etc/init.d/pocket-session
		# An existing /etc/conf.d/pocket-session is kept (local edits win).
		[ -f /etc/conf.d/pocket-session ] ||
			sed s/^POCKET_USER=.*/POCKET_USER=$user/ \$s/pocket-session.confd > /etc/conf.d/pocket-session
		echo installed /etc/init.d/pocket-session, not added to any runlevel
		install -D -m 644 \$s/50-pocket-os-dev.conf /etc/elogind/logind.conf.d/50-pocket-os-dev.conf
		pid=\$(pidof elogind-daemon || true)
		[ -n \"\$pid\" ] && kill -HUP \$pid
		echo installed elogind drop-in, elogind reloaded
		sed s/^POCKET_USER/$user/ \$s/pocket-os-dev > \$s/pocket-os-dev.sudoers
		visudo -cqf \$s/pocket-os-dev.sudoers
		install -m 440 \$s/pocket-os-dev.sudoers /etc/sudoers.d/pocket-os-dev
		echo installed /etc/sudoers.d/pocket-os-dev
	"
	if [ -n "${POCKET_SUDO_PASSWORD_FILE:-}" ]; then
		ssh "$HOST" "sudo -S -p '' sh -euc '$script'" < "$POCKET_SUDO_PASSWORD_FILE"
	else
		ssh -t "$HOST" "sudo sh -euc '$script'"
	fi
fi
