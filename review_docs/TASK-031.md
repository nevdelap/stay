# Review: TASK-031

## Findings

No material findings.

## Final decision

Status: COMPLETED

The implementation matches the TASK-031 goal, scope, and acceptance
criteria:

- `force_recreate_session` (`src/session.rs`) now looks up the target
  session via `tmux.list_sessions()` and, through the new
  `terminated_recreate_notice` helper, prints
  `session {name:?} terminated with exit code {code} before recreate` to
  stderr before calling `kill_session`, exactly once, in the one shared
  function both the CLI's `-f` path and the picker's `r` key
  (`PickerState::recreate`, confirmed it still calls
  `session::force_recreate_session` directly) already route through — no
  picker-side duplication needed, as the task anticipated.
- A live or nonexistent target session prints nothing extra
  (`terminated_recreate_notice` returns `None` in both cases), preserving
  today's non-terminated recreate UX unchanged.
- A missing `exit_code` defaults to `0` in the notice rather than
  panicking or omitting the code.
- Test coverage matches the task's Scope exactly: unit tests for the
  notice helper (named session/exit code, missing-exit-code default,
  none-for-live-or-missing), plus an end-to-end integration test
  (`force_recreate_reports_a_terminated_sessions_exit_code_only`) spawning
  the real binary and asserting all three cases (terminated, live,
  nonexistent) against real tmux.
- `design_docs/stay.html`'s TODO-004 body is struck through and now
  describes the implemented, shared-function behavior accurately.

I additionally exercised this by hand against a real tmux server (built
the binary, created a session that exits with a distinct code, confirmed
`stay list` reports it terminated, then ran `stay create <name> -f ...`)
and got exactly the specified message:
`session "died" terminated with exit code 3 before recreate`, followed by
a clean recreate.

Independent verification: two consecutive clean `just qcheck` runs (no
further file changes after either) and the exact `just mac-qcheck`
recipe, both passed, with all four new tests
(`terminated_recreate_notice_names_the_session_and_exit_code`,
`terminated_recreate_notice_defaults_a_missing_exit_code_to_zero`,
`terminated_recreate_notice_is_none_for_a_live_or_missing_session`,
`force_recreate_reports_a_terminated_sessions_exit_code_only`) confirmed
present and passing in the mac gate's `check.log`.

Approved on first pass; no findings to address.
