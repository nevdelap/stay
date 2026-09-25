# Review: TASK-118

## Findings

### R001

Status: ADDRESSED

The task specifies saved-row Enter behavior but does not say whether it applies
when the saved-only row is selected from idle mode, from filter mode, or both.
Those are separate dispatch paths in the current picker: idle Enter and filter
Enter currently both hand off directly to attach, and the latter is reached
through the asynchronous filter result path. An implementation that changes
only idle Enter can satisfy the literal wording while filter Enter still tries
to attach to the missing tmux session first.

Addressed in the current planning pass: the plan covers idle and published
filter entry, pending-filter no-op behavior, modifier preservation, typed-ahead
input, return to the originating picker mode, selected-row retention, and the
distinct picker state and error for recreation success followed by attach
failure versus recreation failure.

## Final decision

Status: PLANNING_APPROVED

R001 was addressed at planning time; TASK-118 was then `NEW` for
implementation.

## Implementation review

### Findings

No material implementation findings. Saved-only Enter is handled in both the
idle list and published filter results, while pending filter results remain a
no-op. The confirmation preserves the originating mode and attach modifiers,
refusal discards typed-ahead input, and successful recreation forwards the
remaining input through the normal attach handoff. Recreate and recreated-but-
unattached failures retain actionable feedback and the saved-row selection.

The exact `just qcheck` and `just mac-qcheck` gates passed on the final
implementation snapshot.

## Final decision

Status: COMPLETED

The implementation satisfies the approved TASK-118 scope and acceptance
criteria. No material review findings remain.

## Second implementation review

### R002

Status: OPEN

TASK-118 does not limit its typed-ahead and refusal behavior to Unix, but the
non-Unix `InputReader::discard_available` implementation only drains the
internal pending byte queue. It never polls or consumes already queued
crossterm events. Consequently, bytes typed while the saved-session
confirmation is displayed can remain in the console event stream and be
processed after No/Escape, violating the requirement that refusal leaves no
residual input; the successful attach handoff has the same limitation for
residual input capture.

Implement event-queue draining for the non-Unix reader, or explicitly narrow
the task's platform scope and acceptance criteria, then add coverage for the
chosen behavior.

## Final decision

Status: CHANGES_REQUESTED

TASK-118 remains `IMPLEMENTED` pending correction of R002 and rerunning the
required gates.

## Third implementation review

### R002

Status: ADDRESSED

The non-Unix reader now converts queued crossterm events into pending bytes
and drains both that pending queue and the crossterm event queue. The
successful handoff and refusal paths therefore capture or discard typed-ahead
events instead of leaving them in the console queue. The focused Linux picker
tests pass; the non-Unix helper is cfg-gated and will be exercised by the
macOS gate only if the full gate completes.

### R003

Status: OPEN

The exact required full gates remain unresolved for this changed Rust
snapshot. `just qcheck` passed the 306 unit tests but hung in the attachment
PTY suite, with `picker_attachment_status_covers_auto_and_forced_main_screen`
and subsequent picker tests running beyond 60 seconds; it was interrupted.
The exact `just mac-qcheck` likewise passed 305 macOS unit tests and hung in
the same attachment path before interruption. The focused tests pass, but the
task cannot be completed without the exact gates completing.

## Final decision

Status: CHANGES_REQUESTED

TASK-118 remains `IMPLEMENTED` pending R003 and successful reruns of both
required gates.

## Fourth implementation review

### R003

Status: ADDRESSED

The previous attachment-suite hang no longer reproduces on the updated
snapshot. The exact `just mac-qcheck` gate passes, and the Linux Rust and
attachment integration tests all pass.

### R004

Status: OPEN

The exact `just qcheck` recipe still exits non-zero during its MSRV step: the
required Rust tests pass, but rustup cannot create
`/usr/local/rustup/tmp/...` (`Permission denied`) while attempting to install
toolchain 1.89. The gate therefore has not completed successfully in this
environment.

## Final decision

Status: CHANGES_REQUESTED

TASK-118 remains `IMPLEMENTED` pending a successful exact `just qcheck` run.
