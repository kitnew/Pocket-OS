#!/bin/sh
# Control the graphical session on the device.
#   scripts/session.sh start|stop|restart|status
# start/restart always stop a previous (possibly self-terminated) session first.
# Uses sudo on the device (interactive password prompt).
set -eu
HOST="${POCKET_DEVICE:-lavender}"
case "${1:-}" in
	start | restart) cmd='rc-service pocket-session stop >/dev/null 2>&1; rc-service pocket-session start' ;;
	stop) cmd='rc-service pocket-session stop' ;;
	status) exec ssh "$HOST" 'loginctl list-sessions; echo "active VT: $(cat /sys/class/tty/tty0/active)"; pgrep -l -x phoc || echo "phoc not running"' ;;
	*) echo "usage: $0 start|stop|restart|status" >&2; exit 2 ;;
esac
exec ssh -t "$HOST" "sudo sh -c '$cmd'"
