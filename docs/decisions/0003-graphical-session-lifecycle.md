# ADR-0003: Graphical session lifecycle

- Status: **Accepted** (implemented and validated in Stage 1)
- Date: 2026-11-23
- Evidence: `docs/validation/stage1.md`

## Context

In Stage 0 the shell was Phoc's `-E` client: when Quickshell exited, Phoc exited and the
session ended. A mobile shell must survive a UI crash, the core must be restartable
without touching the UI, and a broken component must not cause a restart storm.

Findings on the device:
- Phoc 0.59 has no `--no-quit`-style option. It exits when its `-E` command exits.
- OpenRC 0.63 has per-user services (`openrc-user`, `/etc/user/init.d`), but on this
  device the user instance is started and torn down per login (it restarted with every
  SSH login) and does not get the Wayland environment. It cannot own a graphical
  session's processes yet.
- `supervise-daemon` is built for system services (root, `/run/openrc` state) and does
  not fit per-session, per-user children.
- elogind's `terminate-session` signals every process in the session, and
  `dbus-run-session` gives each session a private bus that ends with it.

## Decision

Keep the Stage 0 OpenRC service (`pocket-session`: `openvt` → `su -l` → PAM/elogind
seat0 session on tty7) and put a small POSIX-sh supervision layer inside the session:

```
rc-service pocket-session (root, manual)
└─ openvt -c 7 ─ su -l <user>                  logind session cN, seat0, tty7
   └─ pocket-session                           logging → ~/.local/state/pocket-os/session.log
      └─ dbus-run-session                      private session bus
         └─ pocket-session-run                 session root; lives as long as Phoc
            ├─ pocket-supervise core pocketd
            └─ phoc -E "pocket-supervise shell pocket-shell"
                         └─ quickshell (Pocket Shell + Pocket.Core bridge)
```

- **Stage 2 addition:** a third supervised component, `osk` (`pocket-supervise osk
  pocket-osk` → squeekboard), started by the session root once Phoc's socket exists and
  stopped before Phoc. `pocketd` (only it) also gets `WAYLAND_DISPLAY`, because it
  launches applications. Phoc must not inherit it, or wlroots would start nested.
- **Phoc** stays the `-E` parent of the shell, but its `-E` child is the shell
  *supervisor*, which never exits on a shell crash. Phoc therefore outlives the shell.
- **Core** is a sibling of Phoc under the session root, so it depends on neither Phoc
  nor the shell.
- **`pocket-supervise NAME CMD`** (`scripts/device/pocket-supervise`) restarts its child
  after unrequested exits with a growing delay (1 s, 2 s, …). It gives up after
  `POCKET_RESTART_MAX` (5) unrequested exits within a **sliding** `POCKET_RESTART_WINDOW`
  (60 s). It then writes `NAME.failed`, copies the session-log tail to
  `~/.local/state/pocket-os/NAME-gave-up.log`, and waits. Times come from `/proc/uptime`
  (this device has no writable RTC, so wall-clock jumps are normal).
- **Control** (`pocket-ctl`, as the user, no root) through marker files and signals in
  `$XDG_RUNTIME_DIR/pocket-os/`: `restart` (`NAME.restart`, not counted as a failure),
  `stop` (`NAME.hold`), `start` (removes hold/failed and sends SIGUSR1), `kill NAME SIG`
  (fault injection), `status`, `env`, `core METHOD`.
- **Shutdown**: `rc-service pocket-session stop` → `loginctl terminate-session` (SIGTERM
  to the session). The session root stops Phoc and the core supervisor, supervisors stop
  their children, `dbus-run-session` ends the bus, the runtime dir is removed, and the VT
  switches to tty1. If Phoc exits by itself, the session root stops the core and the
  session ends the same way.
- **No autostart.** The service stays out of every runlevel during development. A
  scoped sudoers rule (`/etc/sudoers.d/pocket-os-dev`) allows only `rc-service
  pocket-session start|stop|restart|status` without a password, so session control can
  be scripted.

## Recovery strategy

| Failure | Behaviour |
|---|---|
| Shell crash or exit | Supervisor restarts it after 1–4 s. Phoc and Core untouched. The new UI reads state from Core |
| Shell crash loop | Gives up after 5 exits within 60 s. Phoc and Core keep running. `pocket-ctl start shell` recovers |
| Core crash or exit | Supervisor restarts it. The UI shows Disconnected, then reconnects through the name-owner watch and re-reads state (fresh core = `handheld`) |
| Core crash loop | Same give-up rule. The UI stays up and shows Disconnected |
| Phoc exits | The session ends cleanly (no compositor means no UI); `scripts/session.sh start` |
| Session hung | `loginctl kill-session --signal=KILL <id>`, `chvt 1` (development guide) |

## Alternatives considered

| Alternative | Why not (now) |
|---|---|
| Keep `phoc -E pocket-shell` | Shell crash = session loss (Stage 0 problem) |
| Shell outside Phoc (session root starts Phoc, then the shell with `WAYLAND_DISPLAY`) | Needs a readiness wait for the Wayland socket. `-E` already gives a correct environment and start order |
| OpenRC user services for core/shell | User instance is login-scoped here and has no graphical environment. Revisit when it can be bound to the graphical session |
| `supervise-daemon` per component | System-service oriented (root state dir). Would need wrappers that are larger than the 100-line supervisor |
| systemd-style manager, s6/runit | Not on the system. Adding an init/supervision suite for two processes is out of scope |
| Restart the whole session on any failure | Loses core state and the compositor. This is what Stage 0 did |

## Consequences

- Two small shell scripts own the in-session lifecycle; their events are logged with
  stable prefixes (`pocket.session:`, `pocket.supervise:`) for diagnosis and tests.
- A Phoc crash still ends the session (by design for now; Phoc restart would also need
  the shell restarted).
- Core and shell share one log file (`session.log`, previous run in `session.log.1`).
- Autostart (runlevel, greeter, or OpenRC user session) is deferred until we need it.
  The session root does not need to change for it.
