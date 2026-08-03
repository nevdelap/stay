# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

Completed task entries are removed from this active plan; their history is
preserved in git (the task commit and its `Reviewed:` section). Add new work as
the next stable task entry; do not reuse an identifier from a removed task.

## TASK-069 - preserve clean-mode logs across history eviction

State: NEW

Goal:

- Fix external review finding G1 so clean append-mode logging never silently
  discards output when tmux evicts old lines at `history-limit`.
- Detect when the previously captured content is no longer present in the
  retained capture, append an explicit eviction marker, and retain every
  currently available new line after the marker.
- Preserve the existing clean-mode behavior when the retained history still
  overlaps the previously captured content, including byte order and no
  duplicate output.

Dependencies:

- TASK-068 must be `COMPLETED`, including its post-release checkpoints, before
  implementation begins.
- The G1 finding in `design_docs/external_review.md` is the authoritative defect
  report for this task. The external review file is read-only input; do not
  modify it or broaden the task to G2 or later findings.

Scope:

- `src/logging.rs`: replace the line-count-only clean-mode cursor accounting
  with a persisted overlap anchor that survives detach and reattach. The anchor
  must contain the newest complete captured lines, up to 64 lines and 8192 raw
  bytes, encoded as hex in the cursor sidecar so arbitrary output bytes are
  safe. Select whole newline-terminated lines only, dropping the oldest lines
  until both caps fit. If the newest complete line alone exceeds 8192 bytes, or
  the dump has no complete line, persist the explicit `anchor=none` sentinel;
  this is not a matchable anchor and always uses the eviction fallback. Match a
  non-empty anchor only when the current dump contains exactly one occurrence;
  zero or multiple occurrences are ambiguous and must use the eviction fallback
  rather than silently choosing a match. The fallback must append a marker
  beginning `--- history evicted before capture` and then the full currently
  retained dump, accepting marked duplication when necessary. Preserve cursor
  recovery, write-failure retry, truncation, and raw-mode behavior.
- `src/logging.rs` tests and `tests/attachment.rs` real-tmux integration tests:
  cover normal overlapping append, history-window movement at the configured
  limit, output larger than the retained overlap, detach/reattach, legacy or
  corrupt cursor sidecars, ambiguous repeated anchors, and partial-write retry
  behavior. Tests must prove that an eviction cannot produce an empty append
  with an unchanged cursor.
- `design_docs/known_issues.md`: add a dedicated external-review G1 entry, mark
  it resolved by TASK-069, and record the deterministic and real-tmux
  verification evidence. Do not change the existing unrelated TASK-068 dead-pane
  timeout entry.
- `Cargo.toml`, `Cargo.lock`, and any version assertions: bump the package from
  `0.0.49` to `0.0.50` exactly once for this public post-release task.
- Do not change the CLI, release workflow, package publication behavior, or
  unrelated external-review findings.

Acceptance criteria:

- A clean append-mode capture with an intact overlap appends each new line
  exactly once and advances its cursor.
- When tmux history is full and the old overlap has been evicted, the next
  capture appends a marker beginning `--- history evicted before capture` and
  the full currently retained dump, and advances the cursor; it must not
  silently return an empty suffix while leaving the cursor unchanged.
- The persisted anchor is exactly the last 64 complete captured lines capped at
  8192 bytes and hex-encoded: only whole newline-terminated lines are kept,
  oldest first lines are dropped to satisfy the cap, and an oversized newest
  line or a dump with no complete line stores `anchor=none`. A legacy sidecar
  without an anchor, a corrupt anchor, a changed session identity, or a log-size
  mismatch takes the marked full-dump fallback and then writes the new schema
  atomically.
- Anchor matching is exact and unique. If the anchor is absent or occurs more
  than once, the implementation must use the marked full-dump fallback rather
  than guessing; marked duplication is preferred to silent loss.
- Cursor and anchor metadata advance only with the corresponding durable log
  progress. A failed or partial write leaves enough state for the next capture
  to retry without silently skipping bytes, while preserving the existing
  one-warning behavior.
- Truncate mode, raw `pipe-pane` mode, normal append mode, local write-failure
  retry, and existing security checks retain their current behavior.
- Tests deterministically exercise the eviction and overlap cases without
  relying only on a timing-sensitive large-output race, including empty dumps,
  empty lines, a newest line over 8192 bytes, and the 64-line boundary, plus at
  least one real-tmux integration path demonstrates the marker and retained
  output.
- The logging documentation and known-issue entry no longer claim that eviction
  is detected by a line-count decrease alone; the new G1 entry records the
  resolved defect and evidence.
- The package version is `0.0.50` in `Cargo.toml`, `Cargo.lock`, and every
  applicable version assertion, with no additional version bump.
- The exact `just qcheck` and `just mac-qcheck` recipes pass after the final
  amend, and the task commit records the test and gate evidence.
