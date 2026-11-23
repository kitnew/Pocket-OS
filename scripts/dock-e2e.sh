#!/bin/bash
# Stage 3 Dock end-to-end test, driven from the development computer: the real phone
# session (scripts/session.sh start) and the real desktop agent user service on this
# computer (scripts/dock-pair.sh). It stops, restarts and freezes the agent and restarts
# the phone's shell and core, and checks the phone's state after each step.
#
#   scripts/dock-e2e.sh        ~2.5 min. Evidence: .cache/e2e/<timestamp>/dock-e2e.txt + PNGs
#
# The notification action really shows a notification on this desktop (twice).
# media.play_pause is NOT invoked (it would toggle the user's media player).
set -u
HOST="${POCKET_DEVICE:-lavender}"
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/.cache/e2e/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"
unit=pocket-desktop-agent.service
pass=0
fail=0
exec > >(tee "$out/dock-e2e.txt") 2>&1

ctl() { ssh "$HOST" "pocket-os/bin/pocket-ctl $*"; }
state() { ctl ui state; }
# q EXPR: evaluate a Python expression over the shell state `s` (dict).
q() { state | python3 -c "import sys,json; s=json.load(sys.stdin); print($1)"; }
ok() { pass=$((pass + 1)); echo "PASS  $*"; }
ko() { fail=$((fail + 1)); echo "FAIL  $*"; }
check() { # check DESCRIPTION EXPR (Python, true = pass)
	r="$(q "$2" 2>&1)"
	if [ "$r" = True ]; then ok "$1"; else ko "$1 [$2 → $r]"; fi
}
# wait_for DESCRIPTION EXPR SECONDS
wait_for() {
	i=0
	while [ "$i" -lt "$3" ]; do
		[ "$(q "$2" 2>/dev/null)" = True ] && { ok "$1 (${i}s)"; return 0; }
		sleep 1
		i=$((i + 1))
	done
	ko "$1 [not within $3 s: $(q 's["dock"]' 2>&1)]"
}
shot() { ssh "$HOST" ". \$XDG_RUNTIME_DIR/pocket-os/env; grim /tmp/$1.png" && scp -q "$HOST:/tmp/$1.png" "$out/" && echo "      screenshot $out/$1.png"; }
agent_log_since() { journalctl --user -u "$unit" --since "$1" --no-pager -o cat; }

echo "== 0. preconditions"
systemctl --user is-active --quiet "$unit" && ok "agent service active" || ko "agent service active"
ctl status >/dev/null && ok "phone session running" || { ko "phone session running"; exit 1; }
wait_for "shell answers over IPC" 's["core"]=="Connected"' 20
ctl ui mode handheld >/dev/null; ctl ui rotation portrait >/dev/null; ctl ui dock disconnect >/dev/null
wait_for "handheld, portrait, disconnected" 's["mode"]=="handheld" and s["rotation"]==0 and s["dock"]["state"]=="disconnected"' 10

echo "== 1. Dock Mode without a computer"
systemctl --user stop "$unit"
ctl ui mode dock >/dev/null
wait_for "Core reports dock; output landscape" 's["mode"]=="dock" and s["dock"]["docked"] and s["rotation"] in (90,270)' 10
check "rotation preference untouched" 's["rotationMode"]=="portrait"'
check "Dock page is the home page" 's["homeVisible"] and s["homePage"]=="home"'
wait_for "auto-connect fails visibly: error unreachable, retry scheduled" 's["dock"]["state"]=="error" and s["dock"]["error"]=="unreachable" and s["dock"]["retryIn"]>=2' 10
check "no actions without a computer" 'len(s["dock"]["actions"])==0'
[ "$(ctl ui dockAction desktop.notify)" = "not offered" ] && ok "IPC dockAction → not offered" || ko "IPC dockAction → not offered"
shot dock-no-computer

echo "== 2. leave Dock Mode without a connection"
ctl ui mode handheld >/dev/null
wait_for "handheld, portrait restored" 's["mode"]=="handheld" and s["rotation"]==0' 10
ctl ui dock disconnect >/dev/null
wait_for "Disconnect stops retrying" 's["dock"]["state"]=="disconnected" and s["dock"]["retryIn"]==0' 5

echo "== 3. agent available: enter Dock Mode, connect, discover"
systemctl --user start "$unit"; sleep 1
ctl ui rotation landscape >/dev/null
ctl ui mode dock >/dev/null
wait_for "connected after entering Dock Mode" 's["dock"]["state"]=="connected" and s["dock"]["host"]!=""' 10
check "capabilities: desktop.notify available" 'any(a["id"]=="desktop.notify" and a["available"] for a in s["dock"]["actions"])'
check "capabilities: media.play_pause listed" 'any(a["id"]=="media.play_pause" for a in s["dock"]["actions"])'
shot dock-connected

echo "== 4. real desktop action"
since="$(date '+%Y-%m-%d %H:%M:%S')"
[ "$(ctl ui dockAction desktop.notify)" = ok ] && ok "dispatch accepted" || ko "dispatch accepted"
wait_for "phone shows the computer's confirmation" 's["dock"]["results"].get("desktop.notify",{}).get("ok")==True and not s["dock"]["pending"]' 10
sleep 1
nid="$(agent_log_since "$since" | sed -n 's/.*desktop notification shown notification_id=\([0-9]*\).*/\1/p' | tail -1)"
[ -n "$nid" ] && ok "agent log: notification $nid shown by the desktop" || ko "agent log: notification shown"
ssh "$HOST" "grep 'dock.action.finished' ~/.local/state/pocket-os/session.log | tail -1" | grep -q "\"notification_id\":$nid" &&
	ok "same notification id confirmed on the phone" || ko "same notification id confirmed on the phone"
shot dock-action-done

echo "== 4b. leave and re-enter: the rotation preference is the user's"
ctl ui mode handheld >/dev/null
wait_for "handheld restores the landscape preference" 's["mode"]=="handheld" and s["rotationMode"]=="landscape" and s["rotation"] in (90,270)' 10
check "connection outlives Dock Mode" 's["dock"]["state"]=="connected"'
ctl ui rotation portrait >/dev/null
wait_for "portrait preference applied in handheld" 's["rotation"]==0' 10
ctl ui mode dock >/dev/null
wait_for "Dock Mode forces landscape again, preference stays portrait" 's["mode"]=="dock" and s["rotation"] in (90,270) and s["rotationMode"]=="portrait"' 10

echo "== 4c. Disconnect while docked sticks"
ctl ui dock disconnect >/dev/null
wait_for "disconnected in Dock Mode" 's["mode"]=="dock" and s["dock"]["state"]=="disconnected"' 5
sleep 3
check "no automatic reconnect after the user's Disconnect" 's["dock"]["state"]=="disconnected"'
ctl ui dock connect >/dev/null
wait_for "Connect button path reconnects" 's["dock"]["state"]=="connected"' 10

echo "== 5. unknown action rejected"
r="$(ctl dock Execute string:no.such.action dict:string:variant: 2>&1)"
echo "$r" | grep -q "Error.UnknownAction" && ok "Core rejects unknown action (UnknownAction)" || ko "Core rejects unknown action: $r"
r="$(ctl dock Execute string:../../bin/sh dict:string:variant: 2>&1)"
echo "$r" | grep -q "Error.UnknownAction" && ok "Core rejects shell-like action id" || ko "Core rejects shell-like action id: $r"

echo "== 6. agent restart"
systemctl --user restart "$unit"
wait_for "phone sees the disconnect" 's["dock"]["state"]!="connected"' 6
wait_for "reconnects by itself" 's["dock"]["state"]=="connected"' 15

echo "== 7. agent stopped (unexpected disconnection), then back"
systemctl --user stop "$unit"
wait_for "disconnection detected: error + retry scheduled" 's["dock"]["state"]=="error" and s["dock"]["retryIn"]>0 and not s["dock"]["actions"]' 6
check "still in Dock Mode, still landscape" 's["mode"]=="dock" and s["rotation"] in (90,270)'
shot dock-agent-down
systemctl --user start "$unit"
wait_for "connection restored" 's["dock"]["state"]=="connected"' 20

echo "== 8. frozen agent (no TCP reset): keepalive"
systemctl --user kill -s STOP "$unit"
wait_for "silent computer detected (timeout)" 's["dock"]["state"]=="error" and s["dock"]["error"]=="timeout"' 35
systemctl --user kill -s CONT "$unit"
wait_for "reconnects after the agent resumes" 's["dock"]["state"]=="connected"' 20

echo "== 9. shell restart while docked"
ctl restart shell
sleep 6
wait_for "shell comes back docked, landscape, connected, actions listed" 's["dock"]["docked"] and s["rotation"] in (90,270) and s["dock"]["state"]=="connected" and len(s["dock"]["actions"])>=1' 15
[ "$(ctl ui dockAction desktop.notify)" = ok ] && ok "action after shell restart dispatched" || ko "action after shell restart"
wait_for "…and confirmed" 's["dock"]["results"].get("desktop.notify",{}).get("ok")==True' 10

echo "== 10. core restart while docked (in-memory mode; issue #5)"
ctl restart core
sleep 4
wait_for "new core: handheld, portrait restored, link disconnected" 's["core"]=="Connected" and s["mode"]=="handheld" and s["rotation"]==0 and s["dock"]["state"]=="disconnected"' 15
ctl ui mode dock >/dev/null
wait_for "Dock Mode again reconnects" 's["dock"]["state"]=="connected"' 10

echo "== 11. back to handheld"
ctl ui mode handheld >/dev/null
wait_for "handheld, portrait" 's["mode"]=="handheld" and s["rotation"]==0' 10
ctl ui dock disconnect >/dev/null
wait_for "portrait, disconnected" 's["rotation"]==0 and s["dock"]["state"]=="disconnected"' 10
ctl status | head -5

scp -q "$HOST:.local/state/pocket-os/session.log" "$out/" 2>/dev/null || true
agent_log_since "-10min" > "$out/agent.log"
echo "RESULT: PASS $pass, FAIL $fail. Evidence: $out"
[ "$fail" -eq 0 ]
