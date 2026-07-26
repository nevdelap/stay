# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

Completed task entries are removed from this active plan; their history is
preserved in git (the task commit and its `Reviewed:` section). Add new work as
the next stable task entry; do not reuse an identifier from a removed task.

## TASK-021 - built-in tmux settings when no user config

State: COMPLETED

Goal:

- Give stay a zero-config tmux experience. When there is no `~/.tmux.conf`, stay
  applies its own built-in tmux settings — the user's current `~/.tmux.conf`
  captured verbatim in TODO-015 (`design_docs/stay.html`) — so deleting the file
  changes nothing about how sessions look or behave. When `~/.tmux.conf` is
  present, stay applies none of the built-in cosmetic settings; tmux reads the
  user's file at server start, so in that case the user's file is authoritative
  for the cosmetic settings. Implements TODO-015.

Dependencies:

- None — builds on the completed session-creation path (TASK-005, TASK-006).

Scope:

- `src/session.rs`: in `create_session_with_shell`, immediately after the
  existing unconditional globals (`set-option -g remain-on-exit on`,
  `set-option -g history-limit`), apply the built-in cosmetic set only when
  `~/.tmux.conf` is absent. The existing functional globals stay unconditional
  and unchanged; this task adds only the gated cosmetic set, applied at session
  creation alongside them.
- The built-in set is the captured settings applied through the same `tmux.run`
  mechanism (each value a separate argument, never a shell): `status-style`,
  `status-left-length`, `status-left`, `status-right`, and the window globals
  `window-status-format` and `window-status-current-format`. The `status-right`
  value must embed stay's live version via `env!("CARGO_PKG_VERSION")` (e.g.
  `stay (wrapping tmux) v{version}`), not the literal `v0.0.3` captured in
  TODO-015; every other line is applied verbatim. The `r` reload keybind from
  the captured file is intentionally excluded: it is config-management tooling,
  not appearance, and stay applies built-ins only in absent-mode, where it would
  `source-file` a non-existent `~/.tmux.conf` and error.
- Thread the config-existence decision as a parameter so no test touches the
  real `$HOME`. Add a pure predicate
  `user_tmux_config_exists(path: Option<&Path>) -> bool` (existing file -> true;
  `None` or a non-existent path -> false), and add a config-path parameter to
  `create_session_with_shell` (the existing testable seam) that gates the
  built-ins via that predicate. The public `create_session` resolves
  `dirs::home_dir().map(|h| h.join(".tmux.conf"))` and forwards it;
  `dirs::home_dir() == None` is treated as absent (apply built-ins). Only the
  classic `~/.tmux.conf` path is checked (matching the user's setup); the XDG
  `~/.config/tmux/tmux.conf` path is intentionally out of scope.
- Tests: a unit test for the predicate (an existing file -> true; `None` or a
  non-existent path -> false); a tmux test that drives
  `create_session_with_shell` with the config-path parameter — absent applies
  the built-ins (assert the globals land on the server via e.g.
  `show-options -g status-left`), present does not — without touching the real
  `$HOME`.

Acceptance criteria:

- With `~/.tmux.conf` absent, a new session's server has the captured settings
  applied: `status-style 'bg=darkblue,fg=white,bold'`, `status-left-length 200`,
  `status-left ' #{session_name}  #{pane_current_path}'`, `status-right`
  containing the live stay version (not a literal `v0.0.3`), and empty
  `window-status-format` and `window-status-current-format`.
- When the server starts with `~/.tmux.conf` present, stay applies none of the
  built-in cosmetic settings; the user's file is authoritative for them.
- `remain-on-exit on` and `history-limit` remain unconditional on both paths;
  precedence is `~/.tmux.conf` (when present, at server start) over stay's
  built-in cosmetic set (only when absent).
- Known limitation, out of scope for this task: stay never sources the user's
  `~/.tmux.conf` itself, and tmux auto-reads it only at server start, so adding
  or editing `~/.tmux.conf` after the stay server is already running does not
  take effect until the server restarts (or the user manually runs
  `source-file`). The precedence above is therefore stated at server start.
- The config-existence decision is a pure predicate parameterized by the config
  path and threaded through `create_session_with_shell`; no test (unit or
  integration) depends on the real `$HOME`.
- Each captured setting is applied through `tmux.run` as separate arguments (the
  empty `window-status-format` value included), never through a shell.
- The `r` reload keybind from the captured file is intentionally not bound: it
  is config-management tooling rather than appearance, and stay applies
  built-ins only in absent-mode, where the file to reload does not exist. (In
  present-mode the user's own reload binding, read by tmux from their file, is
  what runs.)
- A unit test asserts the predicate (existing -> true; `None`/non-existent ->
  false); a tmux test asserts the built-ins apply when absent and are not
  applied when a user config is present; `just qcheck` and `just mac-qcheck`
  both pass.

## TASK-022 - auto-detach when the attached command ends

State: NEW

Goal:

- When the command running in an attached stay session ends, stay detaches the
  user automatically and exits with that command's exit status, so the common
  case (running a command under stay) returns the user to their shell with the
  right code and no manual detach. The session is preserved by remain-on-exit
  for the uncommon postmortem: reconnecting to the terminated session shows the
  dead pane and exit status and does not auto-detach. Implements TODO-016.

Dependencies:

- None — builds on the completed relay (TASK-008, TASK-009). Composes with
  TODO-003 (terminated marker) and TODO-004 (prior status before -f), but does
  not require them.

Scope:

- `src/relay.rs`: while attached, poll the attached session's pane state on a
  throttled cadence inside the relay loop — the picker uses
  `POLL_INTERVAL = 500 ms` (`src/picker/mod.rs:25`) for the same tmux-state
  polling need; use a comparable coarser interval, not the 100 ms in-process I/O
  poll (`src/relay.rs:138`), so the relay does not spawn a tmux client ~10×/s.
  Query via
  `tmux list-panes -t <session> -F '#{pane_dead}:#{pane_dead_time}:#{pane_dead_status}'`
  (one line per pane; a live pane yields `0::` with empty time/status, so parse
  time/status only when `pane_dead` is set). Capture `attach_start` (epoch
  seconds) when the attach begins. When the attached pane is dead and
  `pane_dead_time >= attach_start`, run the same `detach-client` cleanup as the
  detach key, restore the terminal, and exit the process with
  `pane_dead_status`. `pane_dead_time` is an integer-second epoch (verified on
  tmux 3.6a; final version-floor confirmation is TODO-008's scope), so
  reconnecting in the same second as death reads
  `pane_dead_time == attach_start` and counts as died-during-attach —
  acceptable.
- If the pane was already dead at attach time (`pane_dead_time < attach_start`),
  do not arm auto-detach: this is the postmortem reconnect path; the user views
  the dead pane and detaches manually.
- Exit code depends on whether the pane died *during this attach*, not on which
  trigger detached: if the pane is dead at detach time and
  `pane_dead_time >= attach_start` -> exit `pane_dead_status`; otherwise
  (command still running, or a postmortem reconnect where
  `pane_dead_time < attach_start`) -> exit 0. The auto-detach, the manual detach
  key, and SIGTERM/logout are detach *triggers* and share this single rule, so
  the command's code still propagates if the user manually detaches in the
  window between the command dying and the poll detecting it. This requires
  changing the relay's final return
  (`Ok(tmux.pane_exit_status(session_name)?.unwrap_or(0))` at
  `src/relay.rs:181`) — today it returns the dead command's code on every loop
  exit, so a postmortem reconnect ended by a manual or signal detach would exit
  `pane_dead_status`; the new return must compute the code from
  pane-dead/`pane_dead_time`/`pane_dead_status` against `attach_start` so only a
  death during this attach propagates it.
- `remain-on-exit` stays on and unchanged; this task only adds a detach trigger
  to the relay. stay-created sessions are single-pane; auto-detach keys off the
  pane the attach client is on (manually multi-pane sessions where the active
  pane dies but others live are an edge — default: detach).
- Tests: no existing relay/attachment test asserts the old stay-attached
  behaviour — the existing dead-pane test
  (`returns_a_dead_panes_exit_status_after_detach`) already early-returns on the
  exit code before sending the detach key, so it tolerates the new behaviour and
  needs no rewrite. Add tests for (a) a command ending during attach -> stay
  exits with its code and the session remains terminated and re-listable; (b)
  attaching to an already-dead session -> no auto-detach, and a manual detach
  then exits 0 (not the stale command code); (c) the command's code still
  propagates when the user manually detaches immediately after the command ends
  (the race window the state-based exit-code rule covers). Relay tests run
  through a real PTY via `script(1)` as today.

Acceptance criteria:

- Running `stay <name> <cmd>` where the command exits with code N: stay detaches
  automatically when the command ends and its own process exits N; the session
  persists as terminated and re-listable.
- Manual detach (detach key) while the command still runs: stay exits 0 and the
  session keeps running.
- Attaching to an already-terminated session (postmortem): stay does not
  auto-detach; the user views the dead pane and detaches manually, and stay
  exits 0 (the stale command code is not propagated).
- The exit code is computed at detach time (any trigger) from
  pane-dead/`pane_dead_time`/`pane_dead_status` against `attach_start`: it is
  `pane_dead_status` only when the pane died during this attach, else 0 — so a
  postmortem reconnect detach exits 0, and a manual detach right after the
  command ends still propagates the code.
- The died-during-attach vs already-dead distinction uses `pane_dead_time`
  against `attach_start`; detection is polled on a bounded, throttled interval
  (comparable to the picker's 500 ms, not the 100 ms I/O poll; no busy loop).
- `remain-on-exit` remains on; the SIGTERM/SIGPIPE/panic/real-PTY relay
  guarantees from TASK-009 are preserved.
- `just qcheck` and `just mac-qcheck` both pass.
