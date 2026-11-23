#!/bin/bash
# Stage 4 Dock platform end-to-end test on real hardware: the phone's running Pocket
# session and this computer's desktop agent (scripts/dock-pair.sh, Stage 4 agent).
#
#   scripts/dock-platform-e2e.sh      ~3 min. Evidence: .cache/e2e/<timestamp>/platform-e2e.txt + PNGs
#
# It temporarily replaces the agent's config.json with a narrow TEST policy and always
# restores the original (trap), then restarts the agent:
#   grants: session.lock (only to open and CANCEL the confirmation: the screen is never
#           locked), app.launch with exactly one app (KCalc, started and closed again)
#   notifications: forwarded only from the app name "Pocket OS Test"
# Visible on this desktop: the volume OSD twice (up, then down: net zero), KCalc briefly,
# and two test notifications. Media playback is never touched.
set -u
HOST="${POCKET_DEVICE:-lavender}"
root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/.cache/e2e/$(date +%Y%m%d-%H%M%S)"
mkdir -p "$out"
unit=pocket-desktop-agent.service
cfg="${POCKET_AGENT_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/pocket-desktop-agent}/config.json"
pass=0
fail=0
exec > >(tee "$out/platform-e2e.txt") 2>&1

ctl() { ssh "$HOST" "pocket-os/bin/pocket-ctl $*"; }
q() { ctl ui state | python3 -c "import sys,json; s=json.load(sys.stdin); d=s['dock']; print($1)"; }
ok() { pass=$((pass + 1)); echo "PASS  $*"; }
ko() { fail=$((fail + 1)); echo "FAIL  $*"; }
check() { r="$(q "$2" 2>&1)"; if [ "$r" = True ]; then ok "$1"; else ko "$1 [$2 → $r]"; fi; }
wait_for() {
	i=0
	while [ "$i" -lt "$3" ]; do
		[ "$(q "$2" 2>/dev/null)" = True ] && { ok "$1 (${i}s)"; return 0; }
		sleep 1
		i=$((i + 1))
	done
	ko "$1 [not within $3 s: $(q 'd["nav"], d["state"]' 2>&1)]"
}
shot() { ssh "$HOST" ". \$XDG_RUNTIME_DIR/pocket-os/env; grim /tmp/$1.png" && scp -q "$HOST:/tmp/$1.png" "$out/" && echo "      screenshot $out/$1.png"; }
agent_log_since() { journalctl --user -u "$unit" --since "$1" --no-pager -o cat; }
restart_agent() { systemctl --user restart "$unit"; }
# notify APP SUMMARY BODY: a desktop notification from any client (notify-send may be broken).
notify() {
	gdbus call --session --dest org.freedesktop.Notifications --object-path /org/freedesktop/Notifications \
		--method org.freedesktop.Notifications.Notify "$1" 0 "" "$2" "$3" "[]" "{}" 5000 >/dev/null
}

# ── test policy (restored on exit) ──────────────────────────────────────────────────
cp -p "$cfg" "$out/config.json.orig"
restore() {
	cp -p "$out/config.json.orig" "$cfg" && chmod 600 "$cfg"
	restart_agent
	echo "restored the original agent configuration"
}
trap restore EXIT
apply_test_policy() {
	python3 - "$cfg" <<'PY'
import json, sys
p = sys.argv[1]
c = json.load(open(p))
c["grants"] = ["session.lock", "app.launch"]
c["apps"] = ["org.kde.kcalc"]
c["notifications"] = {"forward": True, "allow_apps": ["Pocket OS Test"], "deny_apps": [], "redact_when_locked": True}
open(p, "w").write(json.dumps(c, indent=2) + "\n")
PY
	chmod 600 "$cfg"
}

echo "== 0. preconditions"
systemctl --user is-active --quiet "$unit" && ok "agent service active" || { ko "agent service active"; exit 1; }
ctl status >/dev/null && ok "phone session running" || { ko "phone session running"; exit 1; }
ctl ui display true >/dev/null
ctl ui mode dock >/dev/null
ctl ui dock connect >/dev/null
wait_for "docked and connected, Core 1.4" 's["mode"]=="dock" and d["state"]=="connected" and d["platform"]' 15

echo "== 1. Ambient Home with real laptop state"
wait_for "entering Dock Mode shows Home, unpinned" 'd["nav"]["view"]=="home" and not d["nav"]["pinned"]' 5
wait_for "live topics host, media, system, notifications" 'all(t in d["live"] and d["live"][t]["available"] for t in ["host","media","system","notifications"])' 10
check "laptop lock state known (unlocked)" 'd["live"]["host"]["data"]["locked"] is False'
check "system vitals are numbers" 'isinstance(d["live"]["system"]["data"]["cpu_percent"], (int,float))'
check "deck: built-in default with 3 pages" 'd["deck"]["source"]=="default" and d["deck"]["pages"]==["general","development","media"]'
shot platform-home

echo "== 2. navigation: idle return, touch resets, pin"
ctl ui nav deck >/dev/null
check "Deck view, idle return armed (~15 s)" 'd["nav"]["view"]=="deck" and 13000 < d["nav"]["dueInMs"] <= 15000'
shot platform-deck
sleep 10; ctl ui nav touch >/dev/null; sleep 8
check "a touch 10 s in kept the Deck (18 s > 15 s)" 'd["nav"]["view"]=="deck"'
wait_for "back to Home 15 s after the last touch" 'd["nav"]["view"]=="home"' 10
ctl ui nav deck >/dev/null; ctl ui nav pin >/dev/null
sleep 17
check "pinned Deck stays for 17 s" 'd["nav"]["view"]=="deck" and d["nav"]["pinned"] and d["nav"]["dueInMs"]==-1'
ctl ui nav unpin >/dev/null
wait_for "unpinned: back to Home after the timeout" 'd["nav"]["view"]=="home"' 18

echo "== 3. default authorization (nothing granted)"
check "elevated actions advertised as not granted" 'all(p.endswith("(off)") for p in d["privileges"] if ":elevated" in p)'
since="$(date '+%Y-%m-%d %H:%M:%S')"
ctl ui nav deck >/dev/null
ctl ui tile general/0 >/dev/null
sleep 1
check "ungranted Lock tile does nothing" 'd["confirm"]=="" and "session.lock" not in d["pending"]'
agent_log_since "$since" | grep -q "session locked" && ko "agent locked the screen" || ok "agent: no lock"

echo "== 4. a basic host action end to end (system volume via KDE, net zero)"
since="$(date '+%Y-%m-%d %H:%M:%S')"
[ "$(ctl ui dockAction volume.up)" = ok ] && ok "volume.up dispatched" || ko "volume.up dispatched"
wait_for "volume.up confirmed by the laptop" 'd["results"].get("volume.up",{}).get("ok") is True and not d["pending"]' 10
[ "$(ctl ui dockAction volume.down)" = ok ] && ok "volume.down dispatched" || ko "volume.down dispatched"
wait_for "volume.down confirmed" 'd["results"].get("volume.down",{}).get("ok") is True and not d["pending"]' 10
sleep 1
n="$(agent_log_since "$since" | grep -c 'shortcut invoked component="kmix"')"
[ "$n" = 2 ] && ok "agent invoked kmix increase/decrease_volume" || ko "agent kmix invocations: $n"

echo "== 5. test policy: grants, one registered app, notification allow-list"
apply_test_policy
restart_agent
wait_for "reconnected after the agent restart" 'd["state"]=="connected" and any(p=="session.lock:elevated" for p in d["privileges"])' 20
check "app.launch granted" 'any(p=="app.launch:elevated" for p in d["privileges"])'
check "desktop.shortcut still not granted" 'any(p=="desktop.shortcut:elevated(off)" for p in d["privileges"])'

echo "== 6. confirmation sheet: modal suspends idle return; Cancel sends nothing"
since="$(date '+%Y-%m-%d %H:%M:%S')"
ctl ui nav deck >/dev/null
[ "$(ctl ui tile general/0)" = confirm ] && ok "Lock tile asks for confirmation" || ko "Lock tile asks for confirmation"
check "modal open" 'd["nav"]["modal"]=="confirm" and d["confirm"]=="Lock laptop"'
shot platform-confirm
sleep 17
check "17 s later: still on the Deck with the sheet (modal suspends idle)" 'd["nav"]["view"]=="deck" and d["nav"]["modal"]=="confirm"'
ctl ui confirm false >/dev/null
check "Cancel closes the sheet" 'd["nav"]["modal"]=="" and d["confirm"]==""'
sleep 1
agent_log_since "$since" | grep -q "session locked" && ko "the screen was locked" || ok "nothing was sent to the laptop"
wait_for "idle return resumes after the sheet closed" 'd["nav"]["view"]=="home"' 18

echo "== 7. an elevated host action end to end (KCalc)"
since="$(date '+%Y-%m-%d %H:%M:%S')"
ctl dock Execute string:app.launch dict:string:variant:app,variant:string:org.kde.kcalc >/dev/null
sleep 3
line="$(agent_log_since "$since" | grep 'apps: launched' | tail -1)"
pid="$(echo "$line" | sed -n 's/.*pid=\([0-9]*\).*/\1/p')"
if [ -n "$pid" ]; then
	ok "agent launched org.kde.kcalc (pid $pid)"
	kill "$pid" 2>/dev/null && ok "closed KCalc again" || echo "      (KCalc already gone)"
else
	ko "agent launched org.kde.kcalc [$line]"
fi
ssh "$HOST" "grep 'dock.action.finished' ~/.local/state/pocket-os/session.log | tail -1" | grep -q 'id=app.launch ok=true' &&
	ok "phone received the confirmation" || ko "phone received the confirmation"
ctl dock Execute string:app.launch dict:string:variant:app,variant:string:org.kde.konsole >/dev/null
sleep 2
ssh "$HOST" "grep 'dock.action.finished' ~/.local/state/pocket-os/session.log | tail -1" | grep -q 'code=not_permitted' &&
	ok "an unregistered app is refused (not_permitted)" || ko "an unregistered app is refused"

echo "== 8. notifications: allow-list, banner, DND"
before="$(q 'd["notifications"]')"
notify "Some Other App" "Not forwarded" "filtered by the allow-list"
notify "Pocket OS Test" "Stage 4 notification test" "Body visible while the laptop is unlocked"
wait_for "the allowed notification reached the phone (banner)" "d['notifications']==$before+1 and d['banner']!=''" 10
shot platform-banner
ctl ui dnd true >/dev/null
sleep 1
check "DND stored in Core" 'd["prefs"]["dnd"] is True'
ctl ui nav home >/dev/null
sleep 7   # let the first banner expire
notify "Pocket OS Test" "Silent test" "DND: history only"
wait_for "DND: in the history, no banner" "d['notifications']==$before+2 and d['banner']==''" 10
ctl ui dnd false >/dev/null

echo "== 9. disconnect / reconnect: no stale remote state"
ctl ui dock disconnect >/dev/null
wait_for "disconnected: remote topics cleared" 'd["state"]=="disconnected" and d["live"]=={}' 10
shot platform-disconnected
ctl ui dock connect >/dev/null
wait_for "reconnected with a fresh snapshot" 'd["state"]=="connected" and "media" in d["live"] and "host" in d["live"]' 15

echo "== 10. Core restart keeps Dock Mode and the UI consistent"
ctl ui nav deck >/dev/null
ctl restart core
wait_for "core back, docked, connected, deck re-read" 's["mode"]=="dock" and d["state"]=="connected" and d["platform"] and len(d["deck"]["pages"])==3' 20
check "the shell kept its view across the core restart" 'd["nav"]["view"] in ("deck","home")'
wait_for "live state resynchronised" '"media" in d["live"]' 10

echo
echo "RESULT: $pass passed, $fail failed. Evidence: $out"
[ "$fail" -eq 0 ]
