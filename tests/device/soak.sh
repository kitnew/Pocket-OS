#!/bin/sh
# Stage 2.5 memory/stability soak test of a RUNNING Pocket session (no mocks).
# Host: scripts/e2e.sh soak   (SOAK_MINUTES, SOAK_IDLE, SOAK_INTERVAL pass through)
#
# Phases:  baseline idle (SOAK_IDLE s) → activity (SOAK_MINUTES) → final idle (SOAK_IDLE s)
# Activity cycles drive the real UI over the shell's IPC handler (the same Shell functions
# as touch; no synthetic input events): Home ↔ launcher, app launch/close, switcher and
# activation, Quick Settings and developer panel, OSK show/hide (sm.puri.OSK0), portrait ↔
# landscape, a GTK app every third cycle, Core D-Bus calls, volume changes.
#
# A background sampler writes $OUT/soak.csv every SOAK_INTERVAL s from /proc only
# (smaps_rollup, stat, fdinfo; one awk per process): RSS, PSS, anonymous PSS, private,
# swap, anonymous huge pages, GPU memory (msm drm fdinfo, deduplicated per DRM client),
# CPU ticks and open fds for Phoc, the shell, squeekboard, pocketd and the audio stack,
# plus system used/available/swap. Analyse with scripts/soak-report.py.
set -u
base="$HOME/pocket-os"
ctl="$base/bin/pocket-ctl"
run="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/pocket-os"
log="$HOME/.local/state/pocket-os/session.log"
OUT="${OUT:-/tmp/pocket-soak}"
minutes="${SOAK_MINUTES:-40}"
idle="${SOAK_IDLE:-150}"
interval="${SOAK_INTERVAL:-30}"
mkdir -p "$OUT"
[ -f "$run/env" ] || { echo "no Pocket session running" >&2; exit 1; }
. "$run/env"
csv="$OUT/soak.csv"
events="$OUT/soak-events.txt"
procs="phoc quickshell squeekboard pocketd pipewire wireplumber pipewire-pulse"

now() { cut -d. -f1 /proc/uptime; }
t0=$(now)
phase=baseline
echo "$phase" >"$OUT/phase"

sample_proc() { # NAME PID
	awk -v name="$1" -v pid="$2" -v t="$(($(now) - t0))" -v phase="$(cat "$OUT/phase")" '
		FILENAME ~ /smaps_rollup/ {
			if ($1 == "Rss:") rss = $2; else if ($1 == "Pss:") pss = $2
			else if ($1 == "Pss_Anon:") anon = $2; else if ($1 == "Private_Clean:" || $1 == "Private_Dirty:") priv += $2
			else if ($1 == "Swap:") swap = $2; else if ($1 == "AnonHugePages:") thp = $2
			next
		}
		FILENAME ~ /\/stat$/ { cpu = $14 + $15; next }
		/^drm-client-id:/ { cid = $2; next }
		/^drm-total-memory:/ { if (!(cid in seen)) { seen[cid] = 1; gpu += $2 } }
		END { printf "%s,%s,%s,%s,%d,%d,%d,%d,%d,%d,%d,%d\n", t, phase, name, pid, rss, pss, anon, priv, swap, thp, gpu, cpu }
	' "/proc/$2/smaps_rollup" "/proc/$2/stat" /proc/"$2"/fdinfo/* 2>/dev/null
}
sampler() {
	echo "t,phase,proc,pid,rss_kb,pss_kb,pss_anon_kb,private_kb,swap_kb,thp_kb,gpu_kb,cpu_ticks" >"$csv"
	while :; do
		for p in $procs; do
			# The shell by its supervised pid: `pocket-ctl ui` runs short-lived `quickshell ipc`.
			if [ "$p" = quickshell ]; then pids=$(cat "$run/shell.pid" 2>/dev/null); else pids=$(pgrep -u "$(id -u)" -x "$p"); fi
			for pid in $pids; do sample_proc "$p" "$pid" >>"$csv"; done
		done
		awk -v t="$(($(now) - t0))" -v phase="$(cat "$OUT/phase")" '
			{ m[$1] = $2 }
			END { printf "%s,%s,system,0,%d,%d,0,0,%d,%d,0,0\n", t, phase, m["MemTotal:"] - m["MemAvailable:"],
				m["MemAvailable:"], m["SwapTotal:"] - m["SwapFree:"], m["AnonHugePages:"] }' /proc/meminfo >>"$csv"
		sleep "$interval"
	done
}

errors=0
ev() { echo "$(($(now) - t0)) $*" >>"$events"; }
err() { errors=$((errors + 1)); ev "ERROR $*"; }
ui() { "$ctl" ui "$@" >/dev/null 2>&1 || err "ui $*"; }
osk() { dbus-send --session --print-reply --dest=sm.puri.OSK0 /sm/puri/OSK0 sm.puri.OSK0.SetVisible "boolean:$1" >/dev/null 2>&1 || err "osk $1"; }
has_window() { "$ctl" ui state 2>/dev/null | grep -q "\"appId\":\"$1\""; }
wait_window() { # APPID present|absent SECONDS
	i=0
	while [ $i -lt $(($3 * 5)) ]; do
		if has_window "$1"; then [ "$2" = present ] && return 0; else [ "$2" = absent ] && return 0; fi
		sleep 0.2
		i=$((i + 1))
	done
	return 1
}
starts() { grep -ac "pocket.supervise: name=$1 event=started" "$log"; }

cleanup() {
	[ -n "${sp:-}" ] && kill "$sp" 2>/dev/null
	pkill -x foot 2>/dev/null
	pkill -x gtk3-widget-factory 2>/dev/null
	"$ctl" ui rotation portrait >/dev/null 2>&1
	"$ctl" ui home >/dev/null 2>&1
}
trap cleanup EXIT INT TERM

for c in core shell osk audio; do eval "starts0_$c=$(starts $c)"; done
log0=$(wc -l <"$log")
: >"$events"
pkill -x foot 2>/dev/null
pkill -x gtk3-widget-factory 2>/dev/null
ui rotation portrait
ui home
sampler &
sp=$!
echo "soak: baseline idle ${idle}s, activity ${minutes} min, final idle ${idle}s, sample every ${interval}s"
ev "phase baseline"
sleep "$idle"

phase=activity
echo "$phase" >"$OUT/phase"
ev "phase activity"
end=$(($(now) + minutes * 60))
cycle=0
while [ "$(now)" -lt "$end" ]; do
	cycle=$((cycle + 1))
	ui home; ui page apps; sleep 0.5; ui page home; sleep 0.5
	ui page apps; osk true; sleep 0.7; osk false; ui page home
	ui open foot
	wait_window foot present 10 || err "foot did not appear (cycle $cycle)"
	sleep 1
	osk true; sleep 0.7; osk false
	ui page switcher; sleep 0.5; ui activate foot; sleep 0.5
	ui panel quicksettings; sleep 0.7; ui panel none
	ui panel dev; sleep 0.7; ui panel none
	ui rotation landscape; sleep 1.5
	osk true; sleep 0.7; ui page apps; sleep 0.5; osk false
	ui rotation portrait; sleep 1.5
	if [ $((cycle % 3)) = 0 ]; then
		ui open pocket-test-widget-factory
		wait_window gtk3-widget-factory present 20 || err "GTK app did not appear (cycle $cycle)"
		sleep 1
		ui close gtk3-widget-factory
		wait_window gtk3-widget-factory absent 10 || err "GTK app did not close (cycle $cycle)"
	fi
	ui home
	ui close foot
	wait_window foot absent 10 || err "foot did not close (cycle $cycle)"
	"$ctl" core GetState >/dev/null 2>&1 || err "core GetState"
	"$ctl" apps ListApps >/dev/null 2>&1 || err "apps ListApps"
	ui volume $((30 + cycle % 2))
	"$ctl" ui state >/dev/null 2>&1 || err "ui state"
	[ $((cycle % 10)) = 0 ] && ev "cycle $cycle errors=$errors"
	sleep 1
done
ev "activity done cycles=$cycle errors=$errors"

phase=final
echo "$phase" >"$OUT/phase"
ev "phase final"
sleep "$idle"
kill "$sp" 2>/dev/null
sp=

restarts=""
for c in core shell osk audio; do
	s0=0
	eval "s0=\$starts0_$c"
	restarts="$restarts $c=$(($(starts $c) - s0))"
done
crashes=$(tail -n +$((log0 + 1)) "$log" | grep -ac "reason=unexpected")
echo "cycles=$cycle errors=$errors restarts:$restarts unexpected-exits-in-log=$crashes duration=$(($(now) - t0))s" | tee "$OUT/soak-summary.txt"
ev "summary $(cat "$OUT/soak-summary.txt")"
[ "$errors" = 0 ] && [ "$crashes" = 0 ] && [ "$restarts" = " core=0 shell=0 osk=0 audio=0" ]
