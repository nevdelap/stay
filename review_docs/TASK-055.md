# Review: TASK-055

## Findings

### R001

Status: ADDRESSED

`src/relay.rs` now treats an empty `dead_time` the same lenient way as
`dead_status`/`dead_signal` (parsed only `(!dead_time.is_empty()).then(||
dead_time.parse::<u64>()).transpose()`), so the transient
`dead=1, dead_time="", dead_status="", dead_signal=""` row parses to
`PaneState { dead: true, dead_time: None, ... }` instead of erroring.
`exit_status_for_attach`'s existing
`state.dead_time.is_some_and(|time| time >= attach_start)` filter then
naturally treats that row as "not detected as dead yet" and defers to the
next 500 ms poll, rather than aborting the attach. A new unit test,
`a_dead_pane_with_no_fields_stamped_yet_parses_instead_of_erroring`,
covers the parse directly, and `exit_status_for_attach`'s own test gained
a `not_yet_stamped` case.

Re-verified by re-running the exact reproduction from the original
finding against the fixed binary: 10-way, 20-way, and 30-way concurrent
runs of `a_signal_killed_pane_auto_detaches_and_reports_128_plus_the_signal`
(90 runs total) produced zero occurrences of `invalid tmux pane dead
time`/`status`/`signal`. A further 60-way run showed 11/60 failures, but
none referenced the parser; a control test unrelated to this task
(`auto_detaches_when_the_attached_command_ends_and_preserves_the_session`)
showed the same ~25% failure rate at 60-way concurrency, confirming that
level simply exceeds this sandbox's capacity for concurrent tmux servers
(generic `[server exited]` / early-exit failures) rather than reflecting
anything specific to this fix. `just qcheck` (twice, clean) and
`just mac-qcheck` both passed, with every TASK-055 test, including
`a_signal_killed_pane_auto_detaches_and_reports_128_plus_the_signal`,
green on the real Mac target.

`parse_pane_state_row` (`src/relay.rs:339-365`) still hard-errors when a
dead pane's `pane_dead_time` field is empty:

```rust
let dead_time = dead_time
    .parse::<u64>()
    .map_err(|_| format!("invalid tmux pane dead time: {row:?}"))?;
```

This task correctly stopped treating an empty `pane_dead_status` as an
error (tmux only publishes it on a normal exit) and added the same
empty-means-`None` treatment for `pane_dead_signal`, but tmux can *also*
transiently report `pane_dead=1` with `pane_dead_time`, `pane_dead_status`,
and `pane_dead_signal` **all** empty in the same `display-message` poll -
i.e. before it finishes stamping the death fields, not only before it
picks status vs. signal. The relay's 500 ms poll (`src/relay.rs:198`,
`pane_state(tmux, session_name)?`) has no retry and propagates this `Err`
straight out of the attach loop, which is exactly the failure class this
task's Scope paragraph describes ("ends the attach with a cryptic internal
message, exit status 1, no `detach-client` cleanup, and no final log
capture") - just triggered by one more field than the ones this task
covers.

This is not hypothetical: the task's own new real-tmux test,
`a_signal_killed_pane_auto_detaches_and_reports_128_plus_the_signal`
(`tests/attachment.rs`), fails intermittently with exactly this error when
run under concurrent load. Reproduction (compiled test binary run
directly, bypassing `cargo test`'s own build-lock contention):

- 25 sequential runs: 0 failures.
- 40 concurrent runs (same machine, `nproc`=16): 5 failures (12.5%), every
  one printing `stay: invalid tmux pane dead time: "1:::"` to stdout (i.e.
  the exact row `dead=1, dead_time="", dead_status="", dead_signal=""`)
  and exiting 1 instead of 137 - the acceptance criterion this task
  defines ("stay detaches cleanly, restores the terminal, exits 137, and
  prints no error") is violated in these runs.

Notably, the sibling parser for the same tmux fields,
`parse_session_row` in `src/tmux.rs` (used by `list_sessions`), already
treats `dead_time` as optional via `parse_optional_field` even when
`dead` is true - it was never given the stricter, hard-error treatment
`parse_pane_state_row` has. `parse_pane_state_row` should follow the same
pattern: treat an empty `dead_time` (and, for consistency, simultaneously
empty `dead_status`/`dead_signal`) while `dead` is `1` as "not yet fully
reported" rather than a parse error - either by making `PaneState`'s
`dead_time` optional the way `dead_status`/`dead_signal` already are, or
by having the poll loop retry once rather than failing the attach on this
specific transient shape.

Given the concurrent-load failure rate (12.5% in this run), this is a
real risk under CI load or the mac gate's SSH latency, not just a
theoretical edge case; a run of `just mac-qcheck` did not happen to hit it
this time, but a clean run does not rule it out.

## Final decision

Status: COMPLETED

Approved. R001 is addressed and re-verified; all acceptance criteria are
met; both gates pass.
