# Review: TASK-118

## Findings

### R001

Status: OPEN

The task specifies saved-row Enter behavior but does not say whether it applies
when the saved-only row is selected from idle mode, from filter mode, or both.
Those are separate dispatch paths in the current picker: idle Enter and filter
Enter currently both hand off directly to attach, and the latter is reached
through the asynchronous filter result path. An implementation that changes
only idle Enter can satisfy the literal wording while filter Enter still tries
to attach to the missing tmux session first.

State the required behavior for every entry path, including filter-mode
selection, pending-filter handling, pending attach modifiers, and typed-ahead
input. Also define how the picker is restored after an attach failure: the
scope says to show an actionable picker error, but the acceptance criteria do
not specify whether the attach handoff returns to the picker, how the selected
saved row is retained, or how a partially successful recreate is represented.

## Final decision

Status: REVIEWED_FOUND_ISSUES

R001 remains open. TASK-118 remains `NEW`.
