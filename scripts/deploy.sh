#!/bin/sh
# Deploy Pocket OS user-space files to the device (~/pocket-os). No root needed.
#   scripts/deploy.sh            shell QML, session scripts, config, device tests
#   scripts/deploy.sh --system   additionally (re)install the OpenRC service (asks for sudo)
# Quickshell itself is built separately: scripts/quickshell-build.sh
set -eu
HOST="${POCKET_DEVICE:-lavender}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

ssh "$HOST" 'mkdir -p ~/pocket-os/bin ~/pocket-os/config ~/pocket-os/tests && rm -rf ~/pocket-os/shell.new'
tar -C apps -c shell | ssh "$HOST" 'mkdir -p ~/pocket-os/shell.new && tar -C ~/pocket-os/shell.new --strip-components=1 -x && rm -rf ~/pocket-os/shell && mv ~/pocket-os/shell.new ~/pocket-os/shell'
tar -C scripts/device -c . | ssh "$HOST" 'tar -C ~/pocket-os/bin -x && chmod 755 ~/pocket-os/bin/*'
tar -C config -c . | ssh "$HOST" 'tar -C ~/pocket-os/config -x'
tar -C tests/device -c . | ssh "$HOST" 'tar -C ~/pocket-os/tests -x'
echo "deployed user files to $HOST:~/pocket-os"

if [ "${1:-}" = "--system" ]; then
	user="$(ssh "$HOST" id -un)"
	ssh "$HOST" 'mkdir -p ~/pocket-os/system'
	scp -q packaging/openrc/pocket-session.initd packaging/openrc/pocket-session.confd "$HOST:pocket-os/system/"
	# The existing /etc/conf.d/pocket-session is kept if present (local edits win).
	ssh -t "$HOST" "sudo sh -euc '
		install -m 755 ~$user/pocket-os/system/pocket-session.initd /etc/init.d/pocket-session
		[ -f /etc/conf.d/pocket-session ] ||
			sed s/^POCKET_USER=.*/POCKET_USER=$user/ ~$user/pocket-os/system/pocket-session.confd > /etc/conf.d/pocket-session
		echo installed /etc/init.d/pocket-session, not added to any runlevel
	'"
fi
