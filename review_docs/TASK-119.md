# Review: TASK-119

## Findings

### R001

Status: ADDRESSED

The task names relay bookkeeping and client identity as affected seams, but it
does not define the identity/rename protocol the implementation must use. The
current relay retains a session-name string and later resolves its client by
PID under that session name; after `rename-session`, the old target name is no
longer valid. The plan must specify whether the relay tracks a stable client
target, discovers the new session name, or uses another tmux identity, and how
polling, logging, auto-detach, and explicit detach behave during the rename
race. The real-tmux test must rename while the relay is actively attached and
prove the relay remains attached until its own detach action.

Addressed in the current planning pass: the plan requires stable attach-client
PID lookup without the original session-name scope, refreshes the session name
for polling/logging/detach paths, retries transient lookup misses, and requires
a real-tmux rename-while-attached test through the relay's own detach.

### R002

Status: ADDRESSED

Addressed in the current planning pass: successful renames must update the
saved name while preserving creation time, working directory, and effective
command for both persisted and runtime-reconstructed live definitions.

## Final decision

Status: PLANNING_APPROVED

R001 and R002 were addressed at planning time; TASK-119 was then `NEW` for
implementation.

## Implementation review

### Findings

No material implementation findings were identified. The relay's stable-PID
rename path and the picker definition preservation paths are covered by the
implementation and the Linux gate.

### R003

Status: OPEN

The exact `just mac-qcheck` gate did not complete. Two full attempts passed
all 303 unit tests but hung in the existing
`picker_attachment_status_covers_auto_and_forced_main_screen` integration
path; both were interrupted after the test exceeded 60 seconds. The isolated
TASK-119 macOS regression test passed, but the required full gate remains
unresolved.

## Final decision

Status: IMPLEMENTATION_REVIEW_BLOCKED

TASK-119 remains `IMPLEMENTED` pending a successful full `just mac-qcheck`.
