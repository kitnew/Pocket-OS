#!/bin/sh
# Deploy only the shell QML, after qmllint passes on the device, then restart the shell.
# A QML syntax error otherwise crash-loops the shell until its supervisor gives up.
#   scripts/shell-deploy.sh
set -eu
HOST="${POCKET_DEVICE:-lavender}"
root="$(cd "$(dirname "$0")/.." && pwd)"
ssh "$HOST" 'rm -rf ~/pocket-os-lint && mkdir -p ~/pocket-os-lint'
tar -C "$root/apps/shell" -c . | ssh "$HOST" 'tar -C ~/pocket-os-lint -x'
# Theme.qml's "not declared as singleton" warning is a known qmllint false positive.
if ! ssh "$HOST" 'cd ~/pocket-os-lint && find components features theme state -name "*.qml" | xargs /usr/lib/qt6/bin/qmllint -I . -I ~/pocket-os/qml 2>&1 | grep -v "singleton" | grep -E "^(Error|Warning)" -A2'; then
	echo "qmllint clean"
else
	echo "qmllint reported problems: not deployed" >&2
	exit 1
fi
ssh "$HOST" 'rm -rf ~/pocket-os/shell.new && mv ~/pocket-os-lint ~/pocket-os/shell.new && rm -rf ~/pocket-os/shell && mv ~/pocket-os/shell.new ~/pocket-os/shell'
ssh "$HOST" 'pocket-os/bin/pocket-ctl start shell >/dev/null 2>&1 || true; pocket-os/bin/pocket-ctl restart shell'
echo "shell deployed and restarted"
