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

## TASK-070 - stabilize shared test state and readiness waits

State: COMPLETED

Goal:

- Eliminate the process-global `TMUX_TMPDIR` race and remaining fixed-delay test
  flakes identified by external-review findings G6 and G22, while retaining
  real-tmux coverage and deterministic test isolation.

Dependencies:

- None. This task should complete before later tasks add or rely on concurrent
  real-tmux regressions.

Scope:

- Replace process-wide `TMUX_TMPDIR` mutation in production and test setup with
  an explicit test-owned value passed to every relevant `tmux` and spawned
  `stay` command. No concurrent test may read or write `TMUX_TMPDIR` through the
  process environment.
- Preserve production socket-root behavior and the existing test namespace,
  cleanup, and orphan-sweep guarantees. Do not serialize the entire test suite
  as a substitute for removing the environment race.
- Replace the remaining five-second logging-flood deadline with the established
  ten-second real-tmux polling ceiling. Replace picker pre-input sleeps with
  readiness polling that proves the intended picker state is visible before
  input is sent.
- Update the affected shared test helpers and their callers. Do not mutate the
  runner's real `HOME`, `PATH`, or other process-global environment to test the
  new behavior.

Acceptance criteria:

- Concurrent test execution has no unsynchronized
  `std::env::{var_os,set_var, remove_var}` access for `TMUX_TMPDIR`; all test
  tmux and child-process socket roots are explicit and isolated.
- Focused regressions prove separate namespaces work concurrently, the logging
  flood tolerates the ten-second ceiling, and picker input is sent only after
  its observable readiness condition.
- Existing real-tmux namespace cleanup and macOS socket-root behavior remain
  covered.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-081 - raise the last fixed five-second real-tmux flood deadline

State: NEW

Goal:

- Finish the deadline half of external-review finding G22, which TASK-070 left
  incomplete: replace the one remaining fixed five-second real-tmux flood
  deadline with the ten-second real-tmux polling ceiling the rest of the suite
  already uses, so a loaded CI runner cannot spuriously time it out.

Dependencies:

- None. TASK-070 is `COMPLETED`; this corrects its residual in a fresh commit
  rather than amending an archived task.

Scope:

- `tests/attachment.rs`: in
  `attach_with_log_succeeds_when_retained_history_exceeds_the_os_pipe_capacity`,
  the wait that polls `capture-pane` until tmux has ingested the whole flood
  (`filler-1999`) still uses
  `let deadline = Instant::now() + Duration::from_secs(5);` (line 2895),
  unchanged since TASK-054. Raise it to `Duration::from_secs(10)`, matching the
  sibling eviction-flood wait in the same file (currently at line 3063) and
  TASK-070's stated ten-second ceiling. This is the only fixed five-second
  deadline left in `tests/`; the picker pre-input sleeps G22 also named were
  already converted to readiness polls in TASK-070, so they are out of scope
  here.
- Do not change the poll interval, the loop body, the timeout-panic message, or
  any other wait; this is a single-constant change.

Acceptance criteria:

- No `Duration::from_secs(5)` deadline remains anywhere under `tests/`, proved
  by a repository grep.
- `attach_with_log_succeeds_when_retained_history_exceeds_the_os_pipe_capacity`
  still passes.
- `just qcheck` and `just mac-qcheck` both pass.
