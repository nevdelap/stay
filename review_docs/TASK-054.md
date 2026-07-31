# Review: TASK-054

## Findings

None. The diff was checked against the task's Goal, Scope, and Acceptance
criteria in `design_docs/implementation_plan.md`, and against
`design_docs/lessons_learned.md`.

Verification performed:

- Read `src/tmux.rs`, `src/tmux_version.rs`, `src/logging.rs`, and
  `tests/attachment.rs` in full against the task diff (`git show
  85cb4c2`).
- Confirmed `wait_with_timeout` and `run_version_command` now spawn both
  pipe readers before the wait loop and join them after, so a child
  writing past the OS pipe capacity can no longer deadlock the wrapper;
  `CommandOutput`'s fields and every existing error string are unchanged,
  and the timeout path still reaps the child via `terminate`.
- Confirmed the `src/logging.rs` module-doc addition is accurate: clean
  mode's non-truncate path (`capture_once`) does call `run_capture_pane`
  with the full retained range (`-S -`, `-E -1`) on every tick, matching
  the documented rationale for why per-tick full re-capture is only
  affordable now.
- Investigated a suspected race in the new
  `attach_with_log_succeeds_when_retained_history_exceeds_the_os_pipe_capacity`
  integration test: it raises `history-limit` to 6000 only *after* creating
  a session whose command immediately starts flooding output, unlike the
  sibling unit test in `src/tmux.rs`
  (`real_tmux_capture_pane_returns_history_larger_than_the_os_pipe_capacity`),
  which adds an explicit `sleep 1` before its flood for exactly this
  reason, and confirmed tmux's compiled-in default `history-limit` here is
  2000 (i.e. close enough to the flood size to matter in principle). Ran
  the compiled test binary directly 60 times (20 sequential, 10-way and
  30-way concurrent under real CPU contention) with zero failures; an
  earlier apparent flake was traced to concurrent `cargo test` invocations
  contending for the build lock, not the tmux race. Not a material
  finding, since it does not reproduce even under stress, but worth
  keeping in mind if this test ever becomes flaky in CI.
- Ran `just qcheck` twice consecutively (clean, no further file changes)
  and `just mac-qcheck` (real `scripts/maccmd` SSH recipe against the
  configured `MAC_HOST`/`MAC_PORT`/`MAC_DIR`, not a substitute), both
  green; `check.log` shows all five new/changed tests
  (`drains_stdout_larger_than_the_os_pipe_capacity`,
  `drains_stderr_larger_than_the_os_pipe_capacity`,
  `drains_stdout_and_stderr_concurrently_past_the_os_pipe_capacity`,
  `real_tmux_capture_pane_returns_history_larger_than_the_os_pipe_capacity`,
  `attach_with_log_succeeds_when_retained_history_exceeds_the_os_pipe_capacity`)
  passing on the Mac target, plus the two acceptance-criteria tests
  (`wrapper_timeout_reaps_the_child`, `timeout_terminates_a_wedged_command`)
  unchanged and passing.
- Confirmed `Cargo.toml`/`Cargo.lock` version bump (0.0.34 → 0.0.35) is
  paired, and the plan's `TASK-054` state transition is the only
  `implementation_plan.md` change in the diff.

## Final decision

Status: COMPLETED

Approved. All acceptance criteria are met; both gates pass; no material
issues found.
