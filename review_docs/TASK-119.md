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

## Second implementation review

### R004

Status: OPEN

The relay independently calls `refresh_session_name` for pane polling,
logging, pending-input handling, and the post-loop cleanup path. Each lookup
spawns another global `list-clients` command. A transient miss leaves the old
session name in `RelayLoopState`; the loop then retries without a backoff, and
the final log/detach bookkeeping ignores a failed refresh and can use that
stale name. This is not a clear implementation of the required transient
miss protocol and makes the rename race harder to reason about.

Resolve the current client/session identity once per relay iteration, make
the retry policy explicit and bounded, and do not silently use a stale name
when finalizing rename-sensitive operations.

### R005

Status: OPEN

`update_relay_state` now combines signal handling, client detachment, pane
death detection, log scheduling, terminal-size propagation, session refresh,
and input delivery. The behavior is spread across several independently
short-circuiting refresh calls, so it is difficult to prove that polling,
logging, copy-mode delivery, and explicit detach all observe the same session
identity. Split identity refresh from the relay actions, or otherwise make a
single refreshed identity an explicit input to those actions, and add focused
unit coverage for the transition behavior.

## Final decision

Status: CHANGES_REQUESTED

TASK-119 remains `IMPLEMENTED` pending R003, R004, and R005; the full macOS
gate must also complete successfully.
