# ADR-0006: Secure lock screen — investigation and decision

- Status: **Accepted: defer the lock screen** (Stage 2.5 investigation)
- Date: 2026-11-23
- Sources inspected: Phoc `v0.59.0` (installed `phoc-0.59.0-r0`) and Phoc `main`
  (`caebbe4`, 2026-11-15); Phosh `main` (`726bf75`, 2026-11-22); wlroots 0.20.2 (Phoc's
  subproject); Quickshell v0.3.2; the device's `/etc/pam.d`.
- Supersedes the short "Session lock: deferred" note in ADR-0005.

## Question

Can Pocket OS ship a lock screen now whose guarantees come from something other than
"the shell process happens to draw on top"? A lock must not show application content or
let anyone interact with applications. That has to hold when the lock UI crashes, when
the shell restarts, and when another client tries to draw over it or steal focus.

## What is actually available

| Mechanism | Present? | What it guarantees |
|---|---|---|
| `ext-session-lock-v1` | **No**, in Phoc 0.59 and in Phoc `main`. wlroots 0.20 has the protocol helper (`wlr_session_lock_manager_v1`), but Phoc does not use it | — |
| Phosh's lock screen | Phosh `src/lockscreen-manager.c`, `lockscreen.c`, `lockshield.c`: an ordinary **layer-shell OVERLAY** surface per output, keyboard-exclusive. Authentication runs **in the shell process** through PAM (`src/auth.c`, `pam_start("phosh", …)`) | Only that the trusted shell draws on top. The compositor enforces nothing lock-specific |
| Phosh's crash behaviour | `phosh-session` runs `phoc -S -E phosh`. When Phosh exits, Phoc's child watch quits the main loop (`server.c` `on_session_exit`), so **the whole session ends** and the greeter returns | Fail-closed by logging out. Running applications are lost |
| Phoc "shell mode" shield (`-S`) | When the `phosh_private` client disconnects, its shell state drops to `unknown`. Phoc then draws an opaque shield over every output (`on_shell_state_changed`, `output-shield.c`) until a shell reports `up` again | **Visual only.** `allow_input` is set to FALSE but is never read anywhere in `src/` (0.59 and `main`), so touch and keys still reach the applications underneath. Pocket does not run Phoc with `-S` |
| "Lockscreen" visibility check (Phoc `27ed972`) | Skips rendering views when an overlay surface covers the output | A rendering optimisation, not a security boundary |
| Privileged protocols (`phoc_desktop_is_privileged_protocol`) | layer-shell, virtual keyboard/pointer, input-method, screencopy, data-control, foreign-toplevel, output management, phosh_private, … | Hidden **only** from clients that connect inside a `wp_security_context_v1`. Pocket's apps (launched by `pocketd`) and Phosh's non-Flatpak apps have no security context. **Any app can therefore map an overlay surface above a lock, inject keys through virtual-keyboard, or capture the screen** |
| PAM | `linux-pam` is installed; there is no `/etc/pam.d/phosh` or Pocket service. Quickshell has `Quickshell.Services.Pam` (`SERVICE_PAM`, currently built OFF) and a session-lock client (`src/wayland/session_lock`) | Usable once a compositor-enforced lock exists |

## Options evaluated

| Option | Security | Crash / restart | Cost | Verdict |
|---|---|---|---|---|
| 1. `ext-session-lock-v1` in Phoc (upstream) + Quickshell `WlSessionLock` + PAM | Compositor shows only lock surfaces, sends input only to them, and **stays locked (blank) if the locker dies**. A new locker can take over. Other clients cannot draw above it | Survives shell crashes without logging out. Apps keep running | Phoc change of a few hundred lines (lock state, render/input/focus policy, per-output surfaces, takeover). wlroots provides the protocol side. Must go upstream, not into a Pocket fork | **Recommended** |
| 2. Phosh-equivalent: overlay surface + in-shell PAM, with fail-closed session end | Same as Phosh: relies on the shell being on top. Defeated by any unsandboxed client (overlay surface, virtual keyboard). No compositor guarantee | Pocket deliberately keeps Phoc alive across shell crashes (ADR-0003). Making it fail closed would mean ending the session (losing apps) when the shell dies while locked. Phoc's `-S` shield would hide the screen in between, but input still reaches apps (`allow_input` unused) | Small | Not shippable as *secure*. It would only reach Phosh parity, and with a weaker crash story |
| 3. Option 2 + launch apps inside `wp_security_context_v1` | Removes the "app draws over the lock / injects input" attacks for apps launched by Core | Still needs a fail-closed rule, and the `-S` shield still lets input through | `pocketd` needs its own Wayland connection and a listening socket per sandbox context. Useful hardening on its own | Worth doing **in addition to** option 1, not instead of it |
| 4. Defer | No false claims | — | None | **Chosen for now** |

What we will not do: a fullscreen overlay presented as authentication, a hard-coded or
plaintext PIN, our own authentication protocol, a Phoc fork, or replacing the compositor.

## Decision

1. **No lock screen in Pocket OS until option 1 exists.** The power key only switches the
   display off (display power management), as in Stage 2. The UI and documents do not
   call this "locking".
2. The recommended path is **`ext-session-lock-v1` in Phoc, contributed upstream**.
   Pocket then uses Quickshell's session-lock client plus `Quickshell.Services.Pam`
   (rebuild with `-DSERVICE_PAM=ON`) and a dedicated `/etc/pam.d/pocket-lock`
   (`auth include base-auth`). No credentials are stored by Pocket.
3. Independent hardening that a later lock relies on, and that is useful before it
   exists: launch applications inside `wp_security_context_v1` (option 3), so they
   cannot see layer-shell, virtual-keyboard, screencopy or foreign-toplevel.

## Follow-up work (exact)

1. Phoc: implement `ext-session-lock-v1` on wlroots' `wlr_session_lock_manager_v1`:
   - lock state in `PhocDesktop`;
   - render only lock surfaces (or solid black) on every output while locked, also when
     no lock client exists;
   - pointer, touch and keyboard focus go only to lock surfaces;
   - input to other clients is dropped;
   - an output without a lock surface stays black;
   - unlock only via `unlock_and_destroy` from the client that holds the lock;
   - a dead locker leaves the session locked, and a new client can obtain the lock.

   Also wire `allow_input` into the input paths, so that a `-S` shield really blocks
   input. Upstream first; carry no local patch.
2. Pocket: rebuild Quickshell with `SERVICE_PAM=ON`, add the PAM service file to
   `deploy.sh --system`, and add a `features/lock/` view plus a `surfaces/LockSurface.qml`
   using `WlSessionLock`. Lock on power key and on idle (`ext_idle_notifier_v1` is
   available).
3. Required tests before shipping:
   - authentication failure, with rate limiting through PAM;
   - shell `SIGSEGV`/`SIGKILL` while locked: the screen stays black, no app receives
     input, and the restarted shell takes over the lock;
   - Core restart while locked;
   - an application mapping an overlay surface or using virtual-keyboard while locked
     (must fail);
   - display off/on while locked;
   - rotation while locked;
   - Phoc's behaviour with no lock client;
   - session recovery after unlock (apps intact).
4. Optional before that: run Phoc with `-S` and have the shell report `up` through
   `phosh_private.set_shell_state`, so a shell crash hides the screen. This is privacy
   only, because input is not blocked, and it must not be described as a lock.

## Consequences

- Stage 3 (Dock Mode) is not blocked: it does not depend on locking.
- Anyone with physical access to an unlocked, display-off phone can use it. This is the
  documented state of Pocket OS until the follow-up lands.
