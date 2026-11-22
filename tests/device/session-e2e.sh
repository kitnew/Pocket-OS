#!/bin/sh
# End-to-end lifecycle test of a RUNNING Pocket session (no mocks): real pocketd, real
# Quickshell UI with the Pocket.Core bridge, real Phoc. Run on the device as the session
# user while the session is up (host: scripts/e2e.sh). Evidence: the UI's diagnostic
# events in the session log, process ids, and screenshots in $OUT.
#
# Covers: Core->UI signal path, invalid command, Core stop/start (UI Disconnected ->
# Connected), Core crash + automatic restart + resync, shell crash + restart with Phoc and
# Core untouched + state restore, requested shell restart, crash-loop give-up and manual
# recovery. Touch input is verified separately by hand (see docs/validation/stage1.md).
set -u
base="$HOME/pocket-os"
ctl="$base/bin/pocket-ctl"
run="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/pocket-os"
log="$HOME/.local/state/pocket-os/session.log"
OUT="${OUT:-/tmp/pocket-e2e}"
mkdir -p "$OUT"
[ -f "$run/env" ] || { echo "no Pocket session running" >&2; exit 1; }
. "$run/env"

pass=0
fail=0
ok() { pass=$((pass + 1)); echo "PASS  $*"; }
bad() { fail=$((fail + 1)); echo "FAIL  $*"; }
mark() { wc -l <"$log"; }
# wait_log MARK REGEX [SECONDS]: a session-log line after MARK matches REGEX.
wait_log() {
	i=0
	while [ $i -lt $((${3:-10} * 10)) ]; do
		tail -n +$(($1 + 1)) "$log" | grep -aEq "$2" && return 0
		sleep 0.1
		i=$((i + 1))
	done
	return 1
}
check() { # check DESCRIPTION MARK REGEX [SECONDS]
	if wait_log "$2" "$3" "${4:-10}"; then ok "$1"; else bad "$1 (no /$3/ in log)"; fi
}
pid() { cat "$run/$1.pid" 2>/dev/null; }
mode() { dbus-send --session --print-reply=literal --dest=org.pocketos.Core1 /org/pocketos/Core1 org.pocketos.Core1.GetMode 2>/dev/null | tr -d ' '; }
request() { dbus-send --session --print-reply --dest=org.pocketos.Core1 /org/pocketos/Core1 org.pocketos.Core1.RequestMode "string:$1" >/dev/null 2>&1; }
shot() { grim "$OUT/$1.png" 2>/dev/null && echo "      screenshot $OUT/$1.png"; }
wait_pid() { # wait_pid NAME OLD: a new pid for NAME
	i=0
	while [ $i -lt 150 ]; do
		p=$(pid "$1")
		[ -n "$p" ] && [ "$p" != "$2" ] && return 0
		sleep 0.1
		i=$((i + 1))
	done
	return 1
}

phoc0=$(pgrep -x phoc)
echo "session: phoc=$phoc0 core=$(pid core) shell=$(pid shell)"
request handheld

echo "== 1. Core -> UI signal path"
m=$(mark)
request dock
check "UI shows mode from StateChanged" "$m" "pocket.shell.diag.*event=core.mode mode=dock"
shot 1-dock

echo "== 2. invalid command"
if dbus-send --session --print-reply --dest=org.pocketos.Core1 /org/pocketos/Core1 \
	org.pocketos.Core1.RequestMode string:desktop 2>&1 | grep -q "org.pocketos.Core1.Error.InvalidMode"; then
	ok "invalid mode rejected with org.pocketos.Core1.Error.InvalidMode"
else bad "invalid mode not rejected"; fi
[ "$(mode)" = '"dock"' ] || [ "$(mode)" = dock ] && ok "state unchanged after invalid command" || bad "state changed: $(mode)"

echo "== 3. Core stopped -> UI disconnected; Core started -> UI resyncs"
shell0=$(pid shell)
m=$(mark)
"$ctl" stop core
check "UI shows Disconnected" "$m" "event=core.status status=disconnected"
sleep 0.5
shot 3-core-stopped
m=$(mark)
"$ctl" start core
check "UI reconnects to the new core" "$m" "event=core.status status=connected.*connections=[2-9]"
check "UI resynchronised to fresh core state (handheld)" "$m" "event=core.mode mode=handheld"
[ "$(pid shell)" = "$shell0" ] && ok "shell not restarted ($shell0)" || bad "shell restarted"
shot 3-core-restarted

# SIGABRT, not SIGSEGV: Rust's runtime handles an externally sent SIGSEGV (stack-overflow
# guard) and keeps running, so SEGV is not a valid crash simulation for pocketd.
echo "== 4. Core crash (SIGABRT) -> supervisor restarts it -> UI resyncs"
request dock
core0=$(pid core)
m=$(mark)
"$ctl" kill core ABRT
check "supervisor saw the crash" "$m" "name=core event=exited pid=$core0 signal=ABRT reason=unexpected failures=[1-4]/"
wait_pid core "$core0" && ok "core restarted automatically (pid $(pid core))" || bad "core not restarted"
check "UI reconnected after crash" "$m" "event=core.status status=connected"
check "UI shows the restarted core's state" "$m" "event=core.mode mode=handheld"
[ "$(pgrep -x phoc)" = "$phoc0" ] && ok "Phoc untouched" || bad "Phoc changed"

echo "== 5. shell crash -> Phoc and Core keep running -> shell restored with Core state"
request dock
sleep 0.3
core1=$(pid core)
shell0=$(pid shell)
m=$(mark)
"$ctl" kill shell SEGV
check "supervisor saw the shell crash" "$m" "name=shell event=exited pid=$shell0 signal=SEGV reason=unexpected"
wait_pid shell "$shell0" && ok "shell restarted (pid $(pid shell))" || bad "shell not restarted"
check "restarted shell connected to Core" "$m" "event=core.status status=connected owner=.* connections=1"
check "restarted shell shows Core's current mode (dock)" "$m" "event=core.mode mode=dock"
[ "$(pgrep -x phoc)" = "$phoc0" ] && ok "Phoc untouched ($phoc0)" || bad "Phoc changed"
[ "$(pid core)" = "$core1" ] && ok "Core untouched ($core1)" || bad "Core changed"
sleep 1
shot 5-shell-restored

echo "== 6. requested shell restart is not counted as a failure"
shell0=$(pid shell)
m=$(mark)
"$ctl" restart shell
check "restart logged as requested" "$m" "name=shell event=exited pid=$shell0 .*reason=restart-requested"
wait_pid shell "$shell0" && ok "shell restarted on request" || bad "shell not restarted"
check "UI back with Core state" "$m" "event=core.mode mode=dock"

echo "== 7. crash loop -> supervisor gives up, session survives, manual recovery"
m=$(mark)
n=0
while [ $n -lt 10 ]; do # max is 5; more would mean the limit does not work
	p=$(pid shell)
	[ -e "$run/shell.failed" ] && break
	if [ -n "$p" ]; then
		kill -s SEGV "$p" 2>/dev/null
		n=$((n + 1))
	fi
	sleep 0.3
	# wait for the next instance before crashing it again
	i=0
	while [ -z "$(pid shell)" ] && [ ! -e "$run/shell.failed" ] && [ $i -lt 100 ]; do sleep 0.1; i=$((i + 1)); done
done
check "supervisor gave up after repeated crashes" "$m" "name=shell event=gave-up" 15
[ -e "$run/shell.failed" ] && ok "gave-up marker: $(cat "$run/shell.failed")" || bad "no gave-up marker"
sleep 3
[ -z "$(pid shell)" ] && ok "no further restarts after give-up" || bad "shell still restarting"
[ -f "$HOME/.local/state/pocket-os/shell-gave-up.log" ] && ok "diagnostic log kept (shell-gave-up.log)" || bad "no diagnostic log"
[ "$(pgrep -x phoc)" = "$phoc0" ] && ok "Phoc still running ($phoc0)" || bad "Phoc gone"
[ -n "$(pid core)" ] && ok "Core still running ($(pid core))" || bad "Core gone"
m=$(mark)
"$ctl" start shell
check "shell recovered with pocket-ctl start shell" "$m" "event=core.status status=connected" 15
check "recovered shell shows Core state (dock)" "$m" "event=core.mode mode=dock" 15
request handheld
sleep 1
shot 7-recovered

echo
echo "e2e: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
