#!/bin/bash
# Stage 3 / 3.1 Dock end-to-end test, driven from the development computer: the real phone
# session (scripts/session.sh start) and the real desktop agent user service on this
# computer (scripts/dock-pair.sh). It stops, restarts and freezes the agent and restarts
# the phone's shell and core, and checks the phone's state after each step.
#
#   scripts/dock-e2e.sh        ~5 min. Evidence: .cache/e2e/<timestamp>/dock-e2e.txt + PNGs
#
# Stage 3.1 (§10–12): mode and link-intent recovery across core crashes/restarts, Disconnect
# that sticks, live countdown, and a full session restart at the end (needs the sudoers
# rule for `rc-service pocket-session`; the session comes back in handheld).
#
# The notification action really shows a notification on this desktop (several times).
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
# Core's own view of the pending retry (ms), straight from org.pocketos.Core1.Dock.
core_retry_ms() { ctl dock GetState | tr -s ' \n' ' ' | sed -n 's/.*"retry_in_ms" variant uint32 \([0-9]*\).*/\1/p'; }
# Session log positions: log_mark, then log_since MARK / no_log DESC MARK REGEX.
log_mark() { ssh "$HOST" 'wc -l < ~/.local/state/pocket-os/session.log'; }
log_since() { ssh "$HOST" "tail -n +\$(($1 + 1)) ~/.local/state/pocket-os/session.log"; }
no_log() {
	m="$(log_since "$2" | grep -E "$3" | head -3)"
	if [ -z "$m" ]; then ok "$1"; else ko "$1 [$m]"; fi
}

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

echo "== 10. core restart while docked and connected (Stage 3.1, issue #5)"
ctl ui rotation portrait >/dev/null
mark="$(log_mark)"; amark="$(date '+%Y-%m-%d %H:%M:%S')"
t0=$(date +%s.%N)
ctl restart core
wait_for "new core: still dock, landscape, link restored by Core" 's["core"]=="Connected" and s["mode"]=="dock" and s["rotation"] in (90,270) and s["dock"]["state"]=="connected" and s["dock"]["wanted"]' 20
echo "      core restart → connected: $(echo "$(date +%s.%N) - $t0" | bc) s (includes the 1 s polling of this script)"
check "rotation preference untouched (portrait)" 's["rotationMode"]=="portrait"'
check "actions re-advertised" 'any(a["id"]=="desktop.notify" and a["available"] for a in s["dock"]["actions"])'
no_log "no flicker: the shell never left Dock Mode, never rotated to portrait, never went home" "$mark" 'event=dock.mode docked=false|event=output.rotation rotation=0$|event=nav.home'
no_log "the shell sent no Connect of its own" "$mark" 'event=dock.connect|event=dock.autoconnect'
[ "$(log_since "$mark" | grep -c 'dock Connect why="restoring the link')" = 1 ] && ok "exactly one restore Connect in the new core" || ko "exactly one restore Connect"
[ "$(agent_log_since "$amark" | grep -c 'phone connected')" = 1 ] && ok "agent: one new authenticated session" || ko "agent: one new session [$(agent_log_since "$amark" | grep -c 'phone connected')]"
[ "$(ctl ui dockAction desktop.notify)" = ok ] && ok "action after core restart dispatched" || ko "action after core restart"
wait_for "…and confirmed by the computer" 's["dock"]["results"].get("desktop.notify",{}).get("ok")==True and not s["dock"]["pending"]' 10
shot dock-after-core-restart

echo "== 10b. core crash (SIGABRT) while docked, then repeated restarts"
amark="$(date '+%Y-%m-%d %H:%M:%S')"
ctl kill core ABRT >/dev/null
wait_for "after a crash: dock, landscape, reconnected" 's["core"]=="Connected" and s["mode"]=="dock" and s["rotation"] in (90,270) and s["dock"]["state"]=="connected"' 20
for i in 1 2 3; do
	ctl restart core
	wait_for "restart $i: dock and connected" 's["core"]=="Connected" and s["mode"]=="dock" and s["dock"]["state"]=="connected"' 20
done
sleep 1
c="$(agent_log_since "$amark" | grep -c 'phone connected')"; d="$(agent_log_since "$amark" | grep -c 'phone disconnected')"; r="$(agent_log_since "$amark" | grep -c 'replacing previous session')"
[ "$c" = 4 ] && [ "$d" = 4 ] && [ "$r" = 0 ] && ok "agent: 4 sessions, each old one closed by its core (connected $c, disconnected $d, replaced $r)" ||
	ko "agent sessions: connected $c, disconnected $d, replaced $r (want 4/4/0; the first disconnect is the crash)"
ssh "$HOST" 'ls $XDG_RUNTIME_DIR/pocket-os/ | grep -c core-recovery' | grep -qx 1 && ok "one recovery file, no leftovers" || ko "recovery file leftovers"

echo "== 10c. manual Disconnect survives core and shell restarts"
ctl ui dock disconnect >/dev/null
wait_for "disconnected, intent cleared" 's["dock"]["state"]=="disconnected" and not s["dock"]["wanted"]' 5
ctl kill core ABRT >/dev/null
wait_for "crashed core back in dock" 's["core"]=="Connected" and s["mode"]=="dock"' 20
sleep 5
check "still disconnected 5 s after the core restart" 's["dock"]["state"]=="disconnected" and not s["dock"]["wanted"]'
ctl restart shell
sleep 6
wait_for "shell back docked and landscape" 's["dock"]["docked"] and s["rotation"] in (90,270)' 15
sleep 4
check "still disconnected after the shell restart" 's["dock"]["state"]=="disconnected" and not s["dock"]["wanted"]'
ctl ui dock connect >/dev/null
wait_for "explicit Connect works" 's["dock"]["state"]=="connected" and s["dock"]["wanted"]' 10

echo "== 10d. computer away: Core and shell restarts during backoff, live countdown"
systemctl --user stop "$unit"
ctl ui display true >/dev/null
wait_for "backoff reached the 8 s wait" 's["dock"]["state"]=="error" and s["dock"]["retryMs"]>5000' 25
# Compare the countdown's decrease with the host time that really passed between the two
# samples (each sample is an SSH + IPC round trip of a few hundred ms).
t0=$(date +%s%3N); a="$(q 's["dock"]["retryMs"]')"; sleep 2; b="$(q 's["dock"]["retryMs"]')"; t1=$(date +%s%3N)
d=$(( (a - b) - (t1 - t0) ))
[ "${d#-}" -le 900 ] && ok "countdown decreases with the clock ($a → $b ms in $((t1 - t0)) ms)" || ko "countdown $a → $b ms in $((t1 - t0)) ms"
check "shown seconds match the deadline (page visible, screen on)" 's["dock"]["countdown"] and abs(s["dock"]["retryShown"] - (s["dock"]["retryMs"]+999)//1000) <= 1'
shot dock-countdown
ctl restart core
wait_for "core restart during backoff: dock, error, retry scheduled" 's["core"]=="Connected" and s["mode"]=="dock" and s["dock"]["state"]=="error" and s["dock"]["retryMs"]>0 and s["dock"]["wanted"]' 20
ctl restart shell
sleep 6
wait_for "shell restart during backoff: countdown running" 's["dock"]["state"]=="error" and s["dock"]["retryMs"]>1000' 15
b="$(q 's["dock"]["retryMs"]')"; c="$(core_retry_ms)"
[ $((b - c)) -ge -100 ] && [ $((b - c)) -le 600 ] && ok "restarted shell counts down to Core's deadline (shell $b ms, Core $c ms right after)" ||
	ko "restarted shell: shell $b ms vs Core $c ms"
systemctl --user start "$unit"
t0=$(date +%s)
wait_for "agent back: reconnects on the next scheduled attempt" 's["dock"]["state"]=="connected"' 70
echo "      agent start → connected: $(($(date +%s) - t0)) s (backoff, no network event)"

echo "== 11. handheld → core restart → handheld"
ctl ui mode handheld >/dev/null
wait_for "handheld, portrait" 's["mode"]=="handheld" and s["rotation"]==0' 10
ctl restart core
wait_for "new core: handheld, portrait, wanted link restored" 's["core"]=="Connected" and s["mode"]=="handheld" and s["rotation"]==0 and s["dock"]["state"]=="connected"' 20
ctl ui dock disconnect >/dev/null
wait_for "portrait, disconnected" 's["rotation"]==0 and s["dock"]["state"]=="disconnected"' 10

echo "== 12. a new graphical session starts in handheld"
ctl ui mode dock >/dev/null
wait_for "dock again (auto-connect on entry)" 's["mode"]=="dock" and s["dock"]["state"]=="connected"' 10
ssh "$HOST" 'sudo -n rc-service pocket-session stop >/dev/null 2>&1; ls $XDG_RUNTIME_DIR/pocket-os 2>/dev/null | wc -l' | grep -qx 0 &&
	ok "session stop removes the session runtime directory (recovery file gone)" || ko "runtime directory left after session stop"
ssh "$HOST" 'sudo -n rc-service pocket-session start >/dev/null 2>&1'
wait_for "new session: handheld, portrait, disconnected, nothing wanted" 's["core"]=="Connected" and s["mode"]=="handheld" and s["rotation"]==0 and s["dock"]["state"]=="disconnected" and not s["dock"]["wanted"]' 60
ctl status | head -5

scp -q "$HOST:.local/state/pocket-os/session.log" "$out/" 2>/dev/null || true
agent_log_since "-10min" > "$out/agent.log"
echo "RESULT: PASS $pass, FAIL $fail. Evidence: $out"
[ "$fail" -eq 0 ]
