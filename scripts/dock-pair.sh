#!/bin/sh
# Install the Pocket Desktop Agent on this computer and pair it with the phone.
#   scripts/dock-pair.sh            build + install the agent (user service), create both
#                                   identities if missing, exchange public keys, start
#   scripts/dock-pair.sh --no-start the same, without (re)starting the agent service
#
# Trust bootstrap: public keys are exchanged over the existing SSH connection to the
# phone (key-based `ssh lavender`), which is already authenticated and encrypted. No
# private key leaves its machine. The agent listens only on this computer's address on the
# USB link (found from the route to the phone), never on all interfaces.
# Re-running is safe: existing identities are kept, the trust entries are replaced.
set -eu
HOST="${POCKET_DEVICE:-lavender}"
PORT="${POCKET_DOCK_PORT:-47130}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
export PATH="$HOME/.cargo/bin:$PATH"

echo "== build the agent (host)"
cargo build --release -p pocket-desktop-agent
install -D -m 755 target/release/pocket-desktop-agent "$HOME/.local/bin/pocket-desktop-agent"
agent="$HOME/.local/bin/pocket-desktop-agent"

phone_ip="$(ssh -G "$HOST" | awk '$1 == "hostname" { print $2 }')"
host_ip="$(ip -4 route get "$phone_ip" | sed -n 's/.* src \([0-9.]*\).*/\1/p')"
[ -n "$host_ip" ] || { echo "no route to the phone ($phone_ip)" >&2; exit 1; }
name="$(hostname -s)"
echo "== phone $HOST ($phone_ip), this computer $name listens on $host_ip:$PORT"

"$agent" init --listen "$host_ip:$PORT" --name "$name" >/dev/null
agent_key="$("$agent" show | sed -n 's/^public_key=//p')"
phone_key="$(ssh "$HOST" 'pocket-os/bin/pocketd dock init' | sed -n 's/^public_key=//p')"
[ ${#agent_key} -eq 64 ] && [ ${#phone_key} -eq 64 ] || { echo "could not read the public keys" >&2; exit 1; }
"$agent" trust "$HOST" "$phone_key"
ssh "$HOST" "pocket-os/bin/pocketd dock set-host '$name' '$host_ip:$PORT' '$agent_key'"

install -D -m 644 packaging/desktop/pocket-desktop-agent.service \
	"$HOME/.config/systemd/user/pocket-desktop-agent.service"
systemctl --user daemon-reload
if [ "${1:-}" != "--no-start" ]; then
	systemctl --user restart pocket-desktop-agent.service
	sleep 1
	systemctl --user --no-pager --lines=3 status pocket-desktop-agent.service || true
fi
echo "paired: phone ${phone_key%"${phone_key#????????????}"}… ↔ $name ${agent_key%"${agent_key#????????????}"}…"
echo "start at login (optional): systemctl --user enable pocket-desktop-agent.service"
