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

## TASK-019 - yes/no selector for picker confirms

State: COMPLETED

Goal:

- Replace the picker's `y/N` text prompt with a reusable yes/no selector and
  migrate the kill confirmation to it. The selector renders two options (`Yes`,
  `No`) inline in the prompt row with the focused option highlighted in reversed
  video. `y`/`n` select directly, `Left`/`Right` move focus, `Enter` confirms
  the focused option, and `Esc` cancels. Default focus follows a rule: `No` for
  destructive actions, `Yes` for non-destructive ones — so kill (destructive)
  defaults to `No`.

Dependencies:

- None — builds on the completed picker (TASK-014 through TASK-018).

Scope:

- `src/picker/mod.rs`: add a reusable yes/no selector as a standalone component
  (option set, focus state, rendering, input handling) independent of
  `KillConfirm`, so other confirm sites (e.g. a later recreate confirm) can
  adopt it; migrate `KillConfirm` from the `y/N` text prompt to the new
  selector.
- Render the focused option in reversed video, matching the picker's existing
  row-highlight idiom, and expose the focused option as a unit-testable
  accessor.
- `src/picker/mod.rs` `InputReader`: parse Left/Right arrow sequences (`ESC[C` /
  `ESC[D`) for focus movement (Up/Down are not focus-movers in the selector),
  and update the `#[cfg(not(unix))]` stub to stay consistent.
- Tests: unit tests in `src/picker/mod.rs` for selector state and focus
  transitions via the accessor, plus a real-PTY picker test driving `k` →
  confirm → kill / not-killed.

Acceptance criteria:

- Kill confirm renders `Yes` and `No` inline in the prompt row; the focused
  option is reversed video, the other plain.
- On entering kill confirm, focus defaults to `No` (kill is destructive).
- `y` confirms `Yes` (kills the session); `n` confirms `No`; `Left`/`Right` move
  focus between the two options; `Enter` confirms the currently focused option;
  `Esc` cancels (returns to Idle without killing).
- Any key outside `{y, n, Left, Right, Enter, Esc}` — including `Up`/`Down`,
  `q`, and other characters — cancels to Idle (No-equivalent), matching the
  current behavior where any non-`y` key cancels a kill confirm.
- Confirming `No` or cancelling leaves the session untouched; confirming `Yes`
  kills it as before.
- The default-focus rule is testable: a destructive selector defaults focus to
  `No` and a non-destructive selector to `Yes` (kill is destructive, so `No`).
- Unit tests assert focus transitions via the accessor (default, `Left`/`Right`,
  `y`/`n`); a real-PTY test drives `k` → confirm and asserts the session is
  killed on `Yes` and untouched on `No`/cancel; `just qcheck` and
  `just mac-qcheck` both pass.

## TASK-020 - refuse to run inside tmux

State: NEW

Goal:

- When stay is invoked from inside an existing tmux session (`$TMUX` is set and
  non-empty), refuse to run: print a clear error and exit non-zero, rather than
  nesting or competing with the outer tmux for the terminal. `--help` and
  `--version` remain unaffected.

Dependencies:

- None.

Scope:

- Add a pure, unit-testable check parameterized by the TMUX value (e.g.
  `require_not_inside_tmux(tmux: Option<&OsStr>) -> Result<(), String>`) in the
  library, and call it at the top of `dispatch` in `src/main.rs`, before any
  other action. The caller passes `std::env::var_os("TMUX")`; tests pass the
  value directly so they never mutate the process-global environment.
- Place the guard in `dispatch`, not `main`: `--help`/`--version` exit during
  `Cli::parse_args` in `main`, so they never reach `dispatch` and are exempt by
  construction.
- Tests: unit tests for the check (present-and-non-empty → error; absent or
  empty → ok); extend `tests/cli_help.rs` to run `--help`/`--version` with
  `$TMUX` set and assert exit 0; and an integration test that runs the real
  binary with `$TMUX` set (non-empty) and a non-`--help` invocation, asserting
  exit 1 and stderr matches
  `stay: cannot run from inside tmux; detach or run it from a plain terminal`.
  The guard runs before `tmux_version::check_installed()` and any tmux call, so
  this test touches no tmux server and is deterministic.

Acceptance criteria:

- With `$TMUX` set and non-empty, any real invocation prints
  `stay: cannot run from inside tmux; detach or run it from a plain terminal` to
  stderr and exits 1, before starting tmux or taking over the terminal.
- With `$TMUX` absent or empty, stay behaves exactly as today.
- `--help` and `--version` still exit 0 with their usual output even when
  `$TMUX` is set.
- The guard is a single check at the top of `dispatch`, covering every real
  invocation (including `--prompt-integration`).
- The check is parameterized; no test mutates the process-global environment.
- The refusal path is covered end-to-end: an integration test runs the real
  binary with `$TMUX` set and a non-`--help` invocation and asserts exit 1 plus
  the exact pinned stderr line (this also pins the single `stay: ` prefix).
- `just qcheck` and `just mac-qcheck` both pass.
