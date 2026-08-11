# Review: HOUSEKEEPING

## Findings

### R001

Status: ADDRESSED

The handoff incorrectly described the retained TASK-107 planning review as
still containing an open finding. That review is completed; it is retained
because TASK-107 remains `NEW` and its planning history is still relevant to the
next implementation handoff. The handoff text is corrected below.

## Completed review documents read

- `review_docs/TASK-105.md`: direct behavioral evidence, bounded polling,
  client-state observation, and the rule that planning-only commits leave the
  planned task `NEW` are captured in `design_docs/lessons_learned.md`.
- `review_docs/TASK-106.md`: bounded read-only absence checks, socket-root-
  validated tmux side-effect snapshots, real JSON parsing, and exact fixture
  binding are captured in `design_docs/lessons_learned.md`.

## Active-plan cleanup

- Removed completed TASK-105 and TASK-106 entries from
  `design_docs/implementation_plan.md`.
- Retained TASK-107 because it is `NEW` and remains the next implementation
  task.
- Retained `review_docs/TASK-PLANNING.md` because TASK-107 remains `NEW` and its
  completed planning review is still relevant to the implementation handoff.

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
