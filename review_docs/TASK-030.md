# Review: TASK-030

## Findings

### R001

Status: ADDRESSED

`--raw` reattachment truncates the log and destroys everything the
already-running `pipe-pane` stream had captured beyond tmux's *current*
history retention — a real, reproducible data-loss bug, not a theoretical
edge case.

`LogSession::start` (`src/logging.rs:117-136`) runs its raw-mode backfill
unconditionally on every attach, not only the first:

```rust
if raw {
    let dump = run_capture_pane(tmux, session_name, "-", "-", true)?;
    if let Err(error) = write_full(&session.path, &dump) {
        session.warn_once(&write_failure_message(&session.path, &error));
    }
    start_pipe_pane(tmux, session_name, &session.path)?;
}
```

`write_full` (`src/logging.rs:317-324`) opens with `.truncate(true)`. tmux's
own `-o` flag on `pipe-pane` means a *second* `stay attach <name> -l <file>
--raw` to a session whose pipe is already open leaves that pipe running
unchanged (confirmed against the tmux 3.4/3.6 man page: "`-o` ... only opens
a new pipe if no previous pipe exists") — but the backfill's `write_full`
call runs regardless, truncating the file the still-active pipe is
concurrently appending to, and replacing its contents with nothing but a
fresh `capture-pane -S - -E -` snapshot of whatever tmux's own history
buffer *currently* retains. Raw mode never calls `raise_history_limit`
(that call is gated to the clean-mode-only `else if !truncate` branch), so
tmux's history buffer for a raw-logged session stays at its ordinary
(often much smaller) `history-limit` — well below what the log file itself
has accumulated over a longer detached period via the continuously-running
pipe.

Reproduced live end to end against a real tmux session (`-L stay`,
`history-limit` lowered to 50 so eviction is guaranteed, set via a global
option before the session was created per the project's own documented
bootstrap-session requirement):

1. `stay attach reprotest3 -l /tmp/repro.log --raw`, detach after a couple
   of seconds.
2. Left detached for 5s while the session kept producing output; log grew
   via the persistent pipe from 251 to 478 lines (`line-495` through
   `line-972`), exactly as designed.
3. `stay attach reprotest3 -l /tmp/repro.log --raw` a second time (same
   path, same flags — the ordinary "come back later and keep watching"
   workflow this feature exists for).
4. After this second attach, `/tmp/repro.log` had **69 lines**
   (`line-1130` through `line-1197`). `line-495`, the earliest line
   the first attach had safely logged, was gone — confirmed absent via
   direct `grep`. Roughly 700 lines of already-durably-logged output were
   silently destroyed.

This is squarely within scope: the task's acceptance criteria requires
`--raw` to "keep growing while detached" and requires back-filling "an
already-running ... session," and `design_docs/stay.html`'s own
"Back-filling a log added after session creation" section documents the
one-shot backfill-then-pipe sequence without ever describing what happens
on a *second* invocation against an already-piping session — this case
was never designed for, not merely under-tested.

Evidence of resolution: `src/logging.rs` now adds
`pane_has_active_pipe`, which queries tmux's `#{pane_pipe}` format via
`display-message`, and gates the raw-mode backfill/`write_full`/
`start_pipe_pane` sequence on it being false. A new regression test,
`raw_log_mode_reattach_does_not_truncate_the_still_piping_log`
(`tests/attachment.rs`), attaches `--raw` twice against the same session
and asserts the log neither shrinks nor loses its original backfilled
content across the reattach.

I independently re-verified the fix beyond reading the diff and the new
test, by re-running my own live reproduction from this finding against
the fixed binary (same technique: real `-L stay` session, `history-limit`
forced to 50 via a global option set before session creation):

1. First `--raw` attach, detach, left running detached for 5s: log grew
   from 247 to 474 lines (`line-155` through `line-627`) via the
   persistent pipe, same as before the fix.
2. Second `--raw` attach to the same session: log continued growing to
   975 lines; `line-155` (the earliest content from the first attach)
   was still present — no truncation.
3. A third attach/detach/reattach cycle (after another detached growth
   period) also preserved `line-155` while the file kept growing to 1587
   lines, confirming the fix holds across repeated reattaches, not just
   a single retry.
4. `design_docs/stay.html`'s `--raw` section is updated to correctly
   describe `-o`'s actual semantics (it was previously mis-described as
   "toggles the pipe off if already active," which is backwards from the
   tmux man page's "only opens a new pipe if no previous pipe exists")
   and to document the new `#{pane_pipe}` check.

## Final decision

Status: COMPLETED

Independent verification: two consecutive clean `just qcheck` runs (no
further file changes after either) and the exact `just mac-qcheck` recipe
both passed, including the new regression test
(`raw_log_mode_reattach_does_not_truncate_the_still_piping_log`, confirmed
present and passing in the mac gate's `check.log`). One `just qcheck` run
during this pass hit `picker::tests::panic_restores_the_picker_terminal_state`
failing under the parallel test runner; it passed in isolation and on a
clean rerun, it predates this task (present since TASK-014/`da2b2c3`), and
TASK-030's diff never touches the picker's panic/terminal-restore code —
a pre-existing flake of the kind `lessons_learned.md`'s "process-global
state" section already describes, not a regression from this task.

Everything else reviewed as correct and matching the task's Goal, Scope,
and Acceptance criteria:

- Default (clean) mode's incremental accounting is sound and well tested
  across two attach/detach cycles without duplication
  (`default_log_mode_appends_across_attach_detach_cycles_without_duplicating`),
  including truncate mode's overwrite semantics
  (`truncate_log_mode_overwrites_instead_of_appending`) and no-ANSI clean
  output (`default_log_mode_produces_a_clean_text_log_with_no_ansi`).
- The single-atomic-capture-plus-Rust-side-skip approach (rather than the
  plan's separate history-size-query-then-`-N`-offset design) is a
  deliberate, well-reasoned, and correct fix for a genuine race in the
  originally specified algorithm: a relative `-N` offset in a second,
  later `capture-pane` call is relative to the pane's history bottom *at
  that later moment*, which can have moved on from what an earlier,
  separate size query observed, silently dropping lines. This is a sound
  improvement over the letter of the Scope, not a regression.
- Log-target security (symlink/owner/permission checks before any tmux
  command), 0600 file creation, write-then-rename cursor persistence, and
  the one-time non-fatal write-failure warning are all implemented and
  tested as specified.
- The "no `pane-exited` hook" scope correction is accurate — independently
  reconfirmed here via `tmux show-hooks -g` against the actual tmux 3.4
  installed in this environment, which lists only
  `after-{capture,display,kill,list,pipe,resize,select}-pane`, no
  termination-triggered hook. Dropping the planned terminate-boundary
  capture and documenting the resulting unattended-termination gap (in
  both the `src/logging.rs` module doc and `design_docs/stay.html`) is a
  reasonable, transparently-disclosed response to a plan assumption that
  turned out to be false, not a silent scope cut.
- CLI/relay/session/main plumbing (`AttachOptions`, `dispatch_attach`,
  `reject_unimplemented_attach_options` losing `-l`/`-t`/`--raw`) is
  correct and consistent with the TASK-029 pattern.
- `design_docs/stay.html` TODO-006 section is struck through and
  documents the implemented behavior, including the unattended-gap
  trade-off.

R001 is addressed and no other issues were found on this pass. Approved.
