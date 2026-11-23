#!/bin/sh
# Control the graphical session and its components on the device.
#   scripts/session.sh start|stop|restart     whole session (OpenRC service; Phoc, Core, Shell)
#   scripts/session.sh status                 session, components, core state
#   scripts/session.sh shell|core|osk|audio restart|stop|start
#                                             one component; the rest keeps running
# start/restart always stop a previous (possibly self-terminated) session first.
# Session start/stop uses sudo for `rc-service pocket-session` only; with the sudoers rule
# installed by `scripts/deploy.sh --system` it does not prompt.
set -eu
HOST="${POCKET_DEVICE:-lavender}"
rc='sudo rc-service pocket-session'
case "${1:-}" in
start | restart) exec ssh -t "$HOST" "$rc stop >/dev/null 2>&1; $rc start" ;;
stop) exec ssh -t "$HOST" "$rc stop" ;;
status)
	exec ssh "$HOST" 'loginctl list-sessions; echo "active VT: $(cat /sys/class/tty/tty0/active)"
		~/pocket-os/bin/pocket-ctl status || pgrep -l -x phoc || echo "phoc not running"' ;;
shell | core | osk | audio)
	case "${2:-}" in
	restart | stop | start) exec ssh "$HOST" "pocket-os/bin/pocket-ctl $2 $1" ;; # relative: ssh starts in $HOME
	esac ;;
esac
sed -n '2,6s/^# \{0,1\}//p' "$0" >&2
exit 2
