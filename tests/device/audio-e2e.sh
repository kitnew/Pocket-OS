#!/bin/sh
# Stage 2.5 audio end-to-end test of a RUNNING Pocket session (no mocks): the session's
# PipeWire/WirePlumber/pipewire-pulse stack (`pocket-supervise audio`), the shell's
# volume adapter (Quickshell.Services.Pipewire) and the ALSA card. Host: scripts/e2e.sh audio.
#
# UI actions go through the shell's IPC handler (same Shell functions as the Quick
# Settings controls). Results are checked against independent sources: wpctl/pactl
# (PipeWire), the ALSA PCM status in /proc/asound, the process table and the session log.
# Audibility is NOT checked here (no microphone loopback); see docs/validation/stage2.5.md.
set -u
base="$HOME/pocket-os"
ctl="$base/bin/pocket-ctl"
run="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/pocket-os"
log="$HOME/.local/state/pocket-os/session.log"
[ -f "$run/env" ] || { echo "no Pocket session running" >&2; exit 1; }
. "$run/env"
# Client tools (wpctl, pactl) warn on stderr that RTKit is absent (no realtime scheduling
# for the session user); that is expected here and not part of what is tested.
wpctl() { command wpctl "$@" 2>/dev/null; }
pactl() { command pactl "$@" 2>/dev/null; }

pass=0
fail=0
ok() { pass=$((pass + 1)); echo "PASS  $*"; }
bad() { fail=$((fail + 1)); echo "FAIL  $*"; }
mark() { wc -l <"$log"; }
wait_log() { # wait_log MARK REGEX [SECONDS]
	i=0
	while [ $i -lt $((${3:-10} * 10)) ]; do
		tail -n +$(($1 + 1)) "$log" | grep -aEq "$2" && return 0
		sleep 0.1
		i=$((i + 1))
	done
	return 1
}
check() { if wait_log "$2" "$3" "${4:-10}"; then ok "$1"; else bad "$1 (no /$3/ in log)"; fi; }
ui() { "$ctl" ui "$@"; }
state() { ui state; }
wait_state() { # wait_state FRAGMENT [SECONDS]
	i=0
	while [ $i -lt $((${2:-10} * 10)) ]; do
		state | grep -q "$1" && return 0
		sleep 0.1
		i=$((i + 1))
	done
	return 1
}
expect() { if wait_state "$2" "${3:-10}"; then ok "$1"; else bad "$1 (state lacks $2: $(state | grep -o '"audio":{[^}]*}'))"; fi; }
pid() { cat "$run/$1.pid" 2>/dev/null; }
count() { pgrep -u "$(id -u)" -x "$1" | wc -l; }
wpvol() { wpctl get-volume @DEFAULT_AUDIO_SINK@ 2>/dev/null; } # "Volume: 0.40" [+ " [MUTED]"]
wait_wpvol() { # wait_wpvol REGEX [SECONDS]
	i=0
	while [ $i -lt $((${2:-5} * 10)) ]; do
		wpvol | grep -Eq "$1" && return 0
		sleep 0.1
		i=$((i + 1))
	done
	return 1
}
pcm_state() { sed -n 's/^state: //p' /proc/asound/card0/pcm0p/sub0/status 2>/dev/null || echo closed; }
wait_stack() { # all three members up and the shell reconnected
	i=0
	while [ $i -lt 150 ]; do
		[ "$(count pipewire)" = 1 ] && [ "$(count wireplumber)" = 1 ] && [ "$(count pipewire-pulse)" = 1 ] &&
			state | grep -q '"audio":{"connected":true,"available":true' && return 0
		sleep 0.1
		i=$((i + 1))
	done
	return 1
}

vol0=$(wpvol | awk '{print $2}')
muted0=$(wpvol | grep -q MUTED && echo true || echo false)
cleanup() {
	[ -n "$vol0" ] && wpctl set-volume @DEFAULT_AUDIO_SINK@ "$vol0" 2>/dev/null
	wpctl set-mute @DEFAULT_AUDIO_SINK@ "$([ "$muted0" = true ] && echo 1 || echo 0)" 2>/dev/null
	rm -f "$run/audio.hold"
}
trap cleanup EXIT
echo "start: $(wpvol)"

echo "== 1. audio stack in the session"
[ -n "$(pid audio)" ] && ok "audio component supervised (pid $(pid audio))" || bad "audio component not running"
for p in pipewire wireplumber pipewire-pulse; do
	n=$(count "$p")
	[ "$n" = 1 ] && ok "$p running once as $(id -un)" || bad "$p count=$n"
	r=$(pgrep -u 0 -x "$p" | wc -l)
	[ "$r" = 0 ] || bad "$p running as root"
done
[ "$(pgrep -x pulseaudio | wc -l)" = 0 ] && ok "no PulseAudio daemon" || bad "pulseaudio is running"
pactl info 2>/dev/null | grep -q "Server Name: PulseAudio (on PipeWire" &&
	ok "Pulse protocol served by PipeWire (pipewire-pulse)" || bad "pactl does not reach pipewire-pulse"

echo "== 2. real output device"
sink=$(wpctl inspect @DEFAULT_AUDIO_SINK@ 2>/dev/null)
echo "$sink" | grep -q 'api.alsa.pcm.card = "0"' && ok "default sink is ALSA card 0" || bad "default sink is not card 0"
desc=$(echo "$sink" | sed -n 's/.*node.description = "\(.*\)"/\1/p' | head -n1)
echo "      default sink: $desc"
expect "shell shows the default sink" "\"output\":\"$desc\""
expect "shell reports the sound card" '"cards":1'

echo "== 3. volume and mute from the shell reach PipeWire"
m=$(mark)
ui volume 25 >/dev/null
wait_wpvol '^Volume: 0\.2[45]' && ok "UI volume 25% -> PipeWire $(wpvol)" || bad "PipeWire volume after UI 25%: $(wpvol)"
check "volume change logged by the shell" "$m" "event=audio.state .*volume=25 muted=false"
ui mute true >/dev/null
wait_wpvol 'MUTED' && ok "UI mute -> PipeWire muted" || bad "not muted: $(wpvol)"
ui volume 30 >/dev/null
wait_wpvol '^Volume: 0\.(29|30)$' && ok "raising the volume unmutes" || bad "after volume 30: $(wpvol)"

echo "== 4. external changes reach the shell"
wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.6
expect "wpctl 60% -> shell 60%" '"volume":60,'
wpctl set-mute @DEFAULT_AUDIO_SINK@ 1
expect "wpctl mute -> shell muted" '"muted":true'
pactl set-sink-mute @DEFAULT_SINK@ 0
expect "pactl unmute (Pulse client) -> shell unmuted" '"muted":false'

echo "== 5. playback reaches the hardware PCM"
wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.15
speaker-test -D pipewire -c 2 -t sine -f 440 -l 1 >/dev/null 2>&1 &
st=$!
i=0
running=
while [ $i -lt 50 ] && kill -0 $st 2>/dev/null; do
	[ "$(pcm_state)" = RUNNING ] && running=1 && break
	sleep 0.1
	i=$((i + 1))
done
[ -n "$running" ] && ok "ALSA hw:0,0 RUNNING during a 440 Hz test tone (pipewire ALSA plugin -> PipeWire -> ALSA)" ||
	bad "hw:0,0 never RUNNING (state $(pcm_state))"
wait $st && ok "speaker-test finished without error" || bad "speaker-test failed"

echo "== 6. volume persists across an audio restart (WirePlumber state)"
wpctl set-volume @DEFAULT_AUDIO_SINK@ 0.33
sleep 1.5 # WirePlumber saves route volumes after a short delay
a0=$(pid audio)
m=$(mark)
"$ctl" restart audio
check "requested restart logged" "$m" "name=audio event=exited .*reason=restart-requested" 15
wait_stack && ok "stack back, shell reconnected" || bad "stack/shell not back: $(state | grep -o '"audio":{[^}]*}')"
expect "volume restored to 33%" '"volume":33,'
[ "$(pid audio)" != "$a0" ] && ok "new audio component pid $(pid audio)" || bad "same pid"

echo "== 7. PipeWire crash: the whole stack restarts, no orphans"
m=$(mark)
pkill -KILL -u "$(id -u)" -x pipewire
check "member exit detected" "$m" "pocket.audio: event=member.exited name=pipewire " 10
check "supervisor restarts the stack" "$m" "name=audio event=started" 15
wait_stack && ok "exactly one pipewire/wireplumber/pipewire-pulse; shell reconnected" ||
	bad "after crash: pw=$(count pipewire) wp=$(count wireplumber) pp=$(count pipewire-pulse)"
m=$(mark)
pkill -KILL -u "$(id -u)" -x wireplumber
check "WirePlumber exit detected" "$m" "pocket.audio: event=member.exited name=wireplumber " 10
check "WirePlumber crash also restarts the stack" "$m" "name=audio event=started" 15
wait_stack && ok "stack recovered after WirePlumber crash" || bad "not recovered after WirePlumber crash"

echo "== 8. audio stopped -> clear unavailable state; started -> back"
"$ctl" stop audio
expect "shell: audio not connected" '"audio":{"connected":false' 10
[ "$(count pipewire)" = 0 ] && [ "$(count wireplumber)" = 0 ] && ok "no audio processes while held" || bad "audio processes left"
"$ctl" start audio
wait_stack && ok "audio started again, shell reconnected" || bad "not reconnected after start"

echo "== 9. shell and core restarts do not touch audio"
s0=$(pid shell)
a0=$(pid audio)
pw0=$(pgrep -u "$(id -u)" -x pipewire)
"$ctl" restart shell
i=0
while [ $i -lt 150 ] && { [ -z "$(pid shell)" ] || [ "$(pid shell)" = "$s0" ]; }; do sleep 0.1; i=$((i + 1)); done
expect "restarted shell shows audio (33%)" '"volume":33,' 15
"$ctl" restart core
sleep 2
[ "$(pid audio)" = "$a0" ] && [ "$(pgrep -u "$(id -u)" -x pipewire)" = "$pw0" ] &&
	ok "audio untouched by shell and core restarts" || bad "audio restarted"

echo
echo "RESULT: $pass passed, $fail failed"
[ "$fail" = 0 ]
