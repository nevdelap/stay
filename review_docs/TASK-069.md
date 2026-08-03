# Review: TASK-069

This review covers the TASK-069 implementation commit against the task
specification and external-review finding G1. The earlier planning review is
retained below as R001.

## Findings

### R001

Status: ADDRESSED

The plan now specifies that the anchor contains only whole
newline-terminated lines, drops oldest lines to satisfy the 64-line and
8192-byte caps, and stores `anchor=none` when the newest complete line is too
large or no complete line exists. It defines that sentinel as unmatchable and
requires the marked full-dump fallback, with tests for empty dumps, empty
lines, oversized lines, and the 64-line boundary.

### R002

Status: ADDRESSED

`design_docs/stay.html` now describes the bounded hex overlap anchor,
`anchor=none` sentinel, exact unique matching, legacy/corrupt cursor fallback,
and the marked full-dump behavior. Its truncate and raw-mode descriptions are
unchanged.

## Verification

- Focused logging unit tests passed.
- The new real-tmux eviction integration test passed.
- The first full `just qcheck` run hit an unrelated scheduling timeout in
  `postmortem_attach_waits_for_manual_detach_and_exits_zero`; that test passed
  in isolation, and the exact `just qcheck` rerun passed.
- The exact `just mac-qcheck` recipe passed.

## Final decision

Status: COMPLETED

TASK-069 is approved. R001 and R002 are addressed, and the implementation,
documentation, tests, version bump, and required verification gates are
complete.
