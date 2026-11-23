#!/bin/sh
# Stage 2 functional end-to-end test of a RUNNING Pocket session (no mocks): real
# pocketd, real shell (Quickshell + Pocket.Core + Pocket.Platform), real Phoc,
# squeekboard and test applications (foot, gtk3-widget-factory). Host: scripts/e2e.sh stage2.
#
# UI actions are driven through the shell's IPC handler (`pocket-ctl ui ...`), which
# calls the same Shell functions the touch UI calls. Results are checked against
# independent sources: Pocket Core (D-Bus), the compositor (wlr-randr, foreign
# toplevels), the process table, sysfs, and the shell's diagnostic log. Physical touch
# is verified separately (docs/validation/stage2.md).
set -u
base="$HOME/pocket-os"
ctl="$base/bin/pocket-ctl"
run="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/pocket-os"
log="$HOME/.local/state/pocket-os/session.log"
apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
OUT="${OUT:-/tmp/pocket-e2e2}"
mkdir -p "$OUT"
[ -f "$run/env" ] || { echo "no Pocket session running" >&2; exit 1; }
. "$run/env"

pass=0
fail=0
skip=0
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
has() { state | grep -q "$1"; } # has JSON-FRAGMENT
wait_state() { # wait_state FRAGMENT [SECONDS]
	i=0
	while [ $i -lt $((${2:-10} * 10)) ]; do
		has "$1" && return 0
		sleep 0.1
		i=$((i + 1))
	done
	return 1
}
expect() { if wait_state "$2" "${3:-10}"; then ok "$1"; else bad "$1 (state lacks $2: $(state | cut -c1-300))"; fi; }
shot() { grim "$OUT/$1.png" 2>/dev/null && echo "      screenshot $OUT/$1.png"; }
pid() { cat "$run/$1.pid" 2>/dev/null; }
transform() { wlr-randr | sed -n 's/^ *Transform: *//p' | head -n1; }
enabled() { wlr-randr | sed -n 's/^ *Enabled: *//p' | head -n1; }
nproc_of() { pgrep -x "$1" | wc -l; }

cleanup() {
	rm -f "$apps/pocket-test-fails.desktop"
	pkill -x foot 2>/dev/null
	pkill -x gtk3-widget-factory 2>/dev/null
	ui rotation portrait >/dev/null 2>&1
	ui display true >/dev/null 2>&1
	ui home >/dev/null 2>&1
}
trap cleanup EXIT
pkill -x foot 2>/dev/null
pkill -x gtk3-widget-factory 2>/dev/null
ui rotation portrait >/dev/null
ui home >/dev/null
sleep 1

echo "== 1. shell services and adapters"
expect "shell connected to Core" '"core":"Connected"'
expect "home visible with no windows" '"homeVisible":true'
expect "power key grabbed through phosh_private" '"powerKey":true'
expect "on-screen keyboard service present" '"osk":{"available":true'
expect "battery from UPower" '"battery":{"available":true'
expect "Wi-Fi device from NetworkManager" '"wifi":{"available":true'
expect "Bluetooth adapter from BlueZ" '"bluetooth":{"available":true'
expect "accelerometer from iio-sensor-proxy" '"sensor":{"available":true'
shot 1-home

echo "== 2. application catalog (Core) mirrored by the shell, live updates"
n0=$(state | sed -n 's/.*"apps":\([0-9]*\).*/\1/p')
[ "${n0:-0}" -ge 1 ] && ok "catalog has $n0 apps" || bad "empty catalog"
"$ctl" apps ListApps | grep -q '"foot"' && ok "foot in the Core catalog" || bad "foot missing from catalog"
m=$(mark)
cat >"$apps/pocket-test-fails.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=Pocket Test Fails
Exec=sh -c "exit 3"
EOF
check "new desktop file picked up (inotify -> AppsChanged -> shell)" "$m" "event=apps.catalog count=$((n0 + 1))" 10

echo "== 3. launch, window, home hidden"
m=$(mark)
ui open foot
check "launch requested through Core" "$m" "pocketd.*launched id=\"foot\""
check "foot window appeared (foreign toplevel)" "$m" "event=window.opened appId=foot" 15
expect "home hidden over the app" '"homeVisible":false'
expect "window listed and active" '"appId":"foot","title":"[^"]*","activated":true'
sleep 1
shot 3-foot

echo "== 4. home and reactivation without duplicates"
ui home
expect "home shown over the running app" '"homeVisible":true'
has '"appId":"foot"' && ok "app keeps running behind home" || bad "window lost"
shot 4-home-running
m=$(mark)
ui open foot
check "second open reactivates the window" "$m" "event=app.reactivate id=foot"
expect "home hidden again" '"homeVisible":false'
[ "$(nproc_of foot)" = 1 ] && ok "no duplicate process (1 foot)" || bad "foot processes: $(nproc_of foot)"

echo "== 5. switcher with two apps"
m=$(mark)
ui open pocket-test-widget-factory
check "second app window (matched via StartupWMClass)" "$m" "event=window.opened appId=gtk3-widget-factory app=pocket-test-widget-factory" 30
ui page switcher
expect "switcher page shown" '"homePage":"switcher"'
has '"appId":"foot"' && has '"appId":"gtk3-widget-factory"' && ok "both windows listed" || bad "window list incomplete"
sleep 0.5
shot 5-switcher
m=$(mark)
ui activate foot >/dev/null
check "activate request sent for foot" "$m" "event=window.activate appId=foot"
expect "home hidden after switching" '"homeVisible":false'
# Quickshell's Toplevel.activate() needs the seat of the shell's last input event; a shell
# that has only been driven over IPC since its start has none and sends nothing. Focus
# switching is therefore verified with real touch (docs/validation/stage2.md).
if wait_state '"appId":"foot","title":"[^"]*","activated":true' 3; then
	ok "compositor activated foot"
else
	skip=$((skip + 1)); echo "SKIP  compositor activation (shell has received no touch input since start)"
fi

echo "== 6. launch failures are visible"
m=$(mark)
ui open pocket-test-fails
check "early non-zero exit reported by Core" "$m" "event=app.exited id=pocket-test-fails pid=[0-9]+ success=false detail=status=3"
check "error toast shown" "$m" "event=toast kind=error text=\"Pocket Test Fails quit while starting"
m=$(mark)
ui open no.such.app
check "unknown app: typed error toast" "$m" "event=toast kind=error text=\"no.such.app could not be started"

echo "== 7. close windows -> back home"
m=$(mark)
ui close pocket-test-widget-factory >/dev/null
check "widget factory window closed" "$m" "event=window.closed appId=gtk3-widget-factory"
ui close foot >/dev/null
check "foot window closed" "$m" "event=window.closed appId=foot windows=0"
expect "home shown when no windows remain" '"homeVisible":true'
expect "home page (not an empty switcher) when no windows remain" '"homePage":"home"'
expect "keyboard hidden when no windows remain" '"osk":{"available":true,"visible":false}'
sleep 1
[ "$(nproc_of foot)" = 0 ] && ok "foot process ended" || bad "foot still running"

echo "== 8. quick settings, brightness"
ui panel quicksettings
expect "quick settings open" '"panel":"quicksettings"'
sleep 0.5
shot 8-quicksettings
b0=$(cat /sys/class/backlight/*/brightness | head -n1)
ui brightness 25
sleep 1
b1=$(cat /sys/class/backlight/*/brightness | head -n1)
[ "$b1" != "$b0" ] && ok "brightness changed through logind ($b0 -> $b1)" || bad "brightness unchanged ($b0)"
ui brightness $(( b0 * 100 / $(cat /sys/class/backlight/*/max_brightness | head -n1) ))
ui panel none
expect "quick settings closed" '"panel":""'

echo "== 9. rotation (wlr-output-management)"
m=$(mark)
ui rotation landscape
check "output transform applied" "$m" "event=output.applied ok=true rotation=90"
[ "$(transform)" = 90 ] && ok "compositor reports transform 90" || bad "transform $(transform)"
check "home relaid out in landscape" "$m" "event=layout.orientation .*landscape=true"
sleep 1
shot 9-landscape
m=$(mark)
ui rotation portrait
check "back to portrait" "$m" "event=output.applied ok=true rotation=0"
[ "$(transform)" = normal ] && ok "compositor reports transform normal" || bad "transform $(transform)"

echo "== 10. display power (wlr-output-power-management)"
m=$(mark)
ui display false
check "display off reported" "$m" "event=display.power on=false"
[ "$(enabled)" = no ] && ok "compositor output disabled" || bad "output still enabled"
ui display true
check "display on reported" "$m" "event=display.power on=true"
[ "$(enabled)" = yes ] && ok "compositor output enabled" || bad "output not enabled"

echo "== 11. on-screen keyboard control and recovery"
m=$(mark)
dbus-send --session --type=method_call --dest=sm.puri.OSK0 /sm/puri/OSK0 sm.puri.OSK0.SetVisible boolean:true
check "keyboard shown (shell follows sm.puri.OSK0)" "$m" "event=osk.visible visible=true"
dbus-send --session --type=method_call --dest=sm.puri.OSK0 /sm/puri/OSK0 sm.puri.OSK0.SetVisible boolean:false
check "keyboard hidden" "$m" "event=osk.visible visible=false"
osk0=$(pid osk)
"$ctl" restart osk
i=0
while { [ -z "$(pid osk)" ] || [ "$(pid osk)" = "$osk0" ]; } && [ $i -lt 100 ]; do sleep 0.1; i=$((i + 1)); done
[ -n "$(pid osk)" ] && [ "$(pid osk)" != "$osk0" ] && ok "keyboard restarted without reboot" || bad "keyboard not restarted"
expect "shell sees the restarted keyboard" '"osk":{"available":true' 15

echo "== 12. apps survive shell and core restarts"
m=$(mark)
ui open foot
check "foot running" "$m" "event=window.opened appId=foot" 15
fpid=$(pgrep -x foot)
"$ctl" restart shell
sleep 6
[ "$(pgrep -x foot)" = "$fpid" ] && ok "app survived shell restart" || bad "app lost on shell restart"
expect "restarted shell lists the existing window" '"appId":"foot"' 15
"$ctl" restart core
sleep 3
[ "$(pgrep -x foot)" = "$fpid" ] && ok "app survived core restart" || bad "app lost on core restart"
expect "shell reconnected to the new core" '"core":"Connected"' 15
m=$(mark)
ui close foot >/dev/null
check "app closed" "$m" "event=window.closed appId=foot"

echo
echo "stage2 e2e: $pass passed, $fail failed, $skip skipped"
[ "$fail" -eq 0 ]
