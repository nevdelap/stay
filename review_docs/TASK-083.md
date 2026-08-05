# Review: TASK-083

## Findings

### R001

Status: ADDRESSED

`src/tmux.rs:1205` deliberately degrades an unknown
`#{pane_dead_signal}` value to `dead_signal = None`, while the pane remains
terminated. If that session is force-recreated, `src/session.rs:279-281`
interprets the missing signal and missing exit code as
`TerminationCause::ExitCode(0)` through `unwrap_or(0)`. The CLI and picker can
therefore report `exit code 0` for a signal-killed session whose signal name
could not be parsed. This violates the task scope's requirement to say
`exit code 0` only when the exit code is genuinely zero. Preserve the unknown
termination cause (or otherwise render an unknown-cause notice) so it is not
fabricated as a successful exit, and add a regression covering the
unknown-signal recreate path.

The implementation now introduces `TerminationCause::Unknown`, selects it
when both `exit_code` and `dead_signal` are absent, and renders an explicit
unknown-cause notice. Unit coverage verifies that this path no longer emits
`exit code 0`; the design document also records the unknown listing form.

### R002

Status: ADDRESSED

`src/tmux.rs:278-285` represents an unparseable signal with one
`SuffixSpan`, such as ` [terminated cause=unknown @<time>]`. However,
`src/picker/mod.rs:2668-2670` immediately replaces every terminated suffix
with fewer than two spans by the generic ` [terminated]` suffix. As a result,
the picker hides the unknown termination cause (and, when a recreate notice is
attached, the notice merged into that suffix) even though the plain listing
and the updated design document promise an explicit unknown-cause detail.
Adjust the picker fitting path to retain the unknown-cause detail when it fits
and add a render regression covering the normal and recreate-notice paths.

`fitted_suffix` now returns a non-empty suffix unchanged whenever it fits the
available width, before applying the terminated-row fallback. The picker tests
cover both the ordinary unknown-cause row and the row with a recreate notice,
including the narrow compact representation.

## Verification

- R001 and R002 focused unit and CLI integration coverage passed, including
  the unknown-cause recreate and picker paths.
- The exact `just qcheck` recipe passed.
- The exact `just mac-qcheck` recipe passed.

## Final decision

Status: COMPLETED
