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
