# Review: HOUSEKEEPING

## Findings

### R001

Status: ADDRESSED

The handoff incorrectly described the retained TASK-107 planning review as
still containing an open finding. That review is completed; it is retained
because TASK-107 remains `NEW` and its planning history is still relevant to the
next implementation handoff. The handoff text is corrected below.

### R002

Status: ADDRESSED

The current housekeeping commit removes TASK-107 from the active plan and
deletes `review_docs/TASK-107.md` and `review_docs/TASK-PLANNING.md`, but this
handoff still says TASK-107 remains `NEW` and that the planning review is
retained. Update the housekeeping handoff to record TASK-107's completed-review
documents and their removal before this commit is approved. The handoff now
records both completed review documents and their removal.

## Completed review documents read

- `review_docs/TASK-105.md`: direct behavioral evidence, bounded polling,
  client-state observation, and the rule that planning-only commits leave the
  planned task `NEW` are captured in `design_docs/lessons_learned.md`.
- `review_docs/TASK-106.md`: bounded read-only absence checks, socket-root-
  validated tmux side-effect snapshots, real JSON parsing, and exact fixture
  binding are captured in `design_docs/lessons_learned.md`.
- `review_docs/TASK-107.md` and `review_docs/TASK-PLANNING.md`: release-order,
  target-native archive, Homebrew, checksum, tmux cleanup, and operator-boundary
  lessons are captured in `design_docs/lessons_learned.md`.

## Active-plan cleanup

- Removed completed TASK-105 and TASK-106 entries from
  `design_docs/implementation_plan.md`.
- Removed the completed TASK-107 entry from `design_docs/implementation_plan.md`.
- Removed `review_docs/TASK-107.md` and `review_docs/TASK-PLANNING.md` after
  their useful lessons were captured.

## Known issues

- Retained the open picker terminal-state investigation in
  `design_docs/known_issues.md`.
- Retained the open large-input relay root-cause investigation in
  `design_docs/known_issues.md`.
- No known-issue entry is verified closed, so none was removed.

## Verification

- Documentation-only `just qlint`: passed with no worktree changes.
- `git diff --check`: passed for the housekeeping diff.
- No source, test, manifest, package-version, or release-tag changes were made.

## Removal suggestions

- `design_docs/external_review.md` — an old whole-project review whose findings
  are represented by completed task history and which has no in-tree references
  beyond the prior housekeeping handoff. It is ignored and remains retained
  pending operator confirmation.
- `design_docs/external_review_2.md` — an old whole-project review whose
  findings are represented by completed task history and which has no in-tree
  references beyond the prior housekeeping handoff. It is ignored and remains
  retained pending operator confirmation.

No other obsolete or unreferenced documentation artifacts were identified;
there are no stale screenshots or images in `design_docs/`.

## Final decision

Status: COMPLETED

The completed TASK-107 review and planning documents were read, their durable
lessons were captured, and the active plan and handoff now reflect their
removal. No material findings remain.
