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

## Third implementation review

### R003

Status: OPEN

The exact `just mac-qcheck` gate is still unresolved. It passed 305 macOS
unit tests and then hung in the attachment PTY suite at
`picker_attachment_status_covers_auto_and_forced_main_screen` and subsequent
picker tests; it was interrupted. The focused real-tmux rename test passes,
but that does not satisfy the exact full-gate requirement.

### R004

Status: ADDRESSED

Relay identity is now resolved once per loop iteration through a bounded
three-attempt lookup with a 10 ms retry delay. Pane polling, logging, pending
input, and explicit detach all receive that same identity. Finalization no
longer silently reuses a stale session name: it accepts a freshly resolved
identity, a confirmed detach identity, or returns no identity/error according
to the observed session state.

### R005

Status: ADDRESSED

The relay actions now consume an explicit per-iteration identity instead of
performing independent session refreshes inside each action. The finalization
and detach paths also carry identity state explicitly, and focused tests cover
transient lookup retry and rejection of an unconfirmed stale final identity.

### R006

Status: OPEN

The exact `just qcheck` gate is also unresolved for this changed Rust
snapshot. It passed 306 unit tests but hung in the same attachment PTY suite,
with `picker_attachment_status_covers_auto_and_forced_main_screen` and
subsequent picker tests running beyond 60 seconds; it was interrupted. The
rename regression test passes in isolation, but the full required gate must
complete before this task can be completed.

## Final decision

Status: CHANGES_REQUESTED

TASK-119 remains `IMPLEMENTED` pending R003 and R006; both exact Rust gates
must complete successfully.

## Fourth implementation review

### R003

Status: ADDRESSED

The exact `just mac-qcheck` gate now passes, including all 305 macOS unit
tests and all 51 attachment integration tests, including the rename relay
case.

### R006

Status: ADDRESSED

The prior attachment-suite hang no longer reproduces. The Linux Rust and
attachment integration tests all pass; the remaining Linux `qcheck` failure
is isolated to its MSRV toolchain setup and is recorded as R008.

### R007

Status: OPEN

When the fresh PID-based identity lookup misses, `update_relay_state` falls
back to `state.last_identity` for pane polling and pending-input delivery.
During a session rename, that value can still be the old session name. A
queued copy-mode action can therefore target the deleted name, and a queued
detach can successfully detach the renamed client by PID while recording the
old name in `detached_session_name`; final logging and pane-status handling
then use stale identity. The missing-pane counter can also stop the relay
after three polls against the old name, contrary to the acceptance criterion
that a transient lookup miss must not detach or restart the relay.

Only use a freshly resolved identity for name-sensitive operations, and keep
PID-based detach separate from the session name recorded for finalization.
Add a regression test covering a rename plus an identity miss while pending
input or detach is being processed.

### R008

Status: OPEN

The exact `just qcheck` recipe still exits non-zero during its MSRV step: all
Rust and integration tests pass, but rustup cannot create
`/usr/local/rustup/tmp/...` (`Permission denied`) while attempting to install
toolchain 1.89. The required Linux gate has therefore not completed
successfully in this environment.

## Final decision

Status: CHANGES_REQUESTED

TASK-119 remains `IMPLEMENTED` pending R007 and a successful exact
`just qcheck` run.

## Fifth implementation review

### R007

Status: PARTIALLY ADDRESSED

The relay no longer uses `last_identity` for pane polling or named pending
actions after a fresh lookup miss. Copy-mode is deferred, and PID-based
detach can proceed without inventing a session name. The new focused test
covers those stale-action protections.

However, a successful PID detach during that lookup-miss window returns a
`DetachedClient` with no session name. `finish_relay` then calls
`finalize_client_identity` with no confirmed name; if the old session still
exists, the retained `last_identity` causes finalization to return an error
even though the client was detached successfully. Preserve the confirmed
detach outcome through finalization (without reusing the old name), and add a
full relay regression covering detach during a transient identity miss.

### R008

Status: OPEN

The stable Linux Rust and integration tests pass, and the exact
`just mac-qcheck` gate passes. The exact `just qcheck` recipe still does not
complete: its 1.89 MSRV test run hangs in picker and relay unit tests after
the stable suite passes, so it was interrupted. The Linux gate remains
unresolved.

### R009

Status: OPEN

The current focused regression test verifies `drain_pending_input` in
isolation, but not the subsequent `finish_relay`/`finalize_client_identity`
path for a detach with no fresh identity. That missing end-to-end assertion
allowed the finalization error above to remain undetected.

## Final decision

Status: CHANGES_REQUESTED

TASK-119 remains `IMPLEMENTED` pending completion of the detach finalization
fix, its end-to-end regression test, and a successful exact `just qcheck`.

## Sixth implementation review

The outstanding exact Linux gate failure is not specific to TASK-119 and is
consolidated as G001 in `review_docs/GENERAL.md`. The task-specific relay
finalization finding remains open.

## Final decision

Status: CHANGES_REQUESTED

TASK-119 remains `IMPLEMENTED` pending its relay-finalization fix and
resolution of general finding G001.
