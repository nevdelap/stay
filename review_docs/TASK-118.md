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

R001 is addressed. TASK-118 remains `NEW` for implementation.
