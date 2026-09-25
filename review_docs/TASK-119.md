# Review: TASK-119

## Findings

### R001

Status: OPEN

The task names relay bookkeeping and client identity as affected seams, but it
does not define the identity/rename protocol the implementation must use. The
current relay retains a session-name string and later resolves its client by
PID under that session name; after `rename-session`, the old target name is no
longer valid. The plan must specify whether the relay tracks a stable client
target, discovers the new session name, or uses another tmux identity, and how
polling, logging, auto-detach, and explicit detach behave during the rename
race. The real-tmux test must rename while the relay is actively attached and
prove the relay remains attached until its own detach action.

Without that contract, Igor must choose an unresolved design and the stated
“can still identify” criterion is not independently testable.

### R002

Status: OPEN

The acceptance criteria cover collision and failure consistency but do not
explicitly require that a successful rename update the saved definition's
name while preserving its creation time, working directory, and effective
command. The scope mentions picker/store rename coverage, and the existing
store-first lifecycle makes this a public durable-state contract, so the
successful live-persisted case and the external live-session case need exact
assertions alongside the saved-only case.

## Final decision

Status: REVIEWED_FOUND_ISSUES

R001 and R002 remain open. TASK-119 remains `NEW`.
