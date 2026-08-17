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

### R003

Status: ADDRESSED

The current housekeeping pass incorrectly deleted the prior R001/R002 finding
history from this shared review document. That history is restored above. The
current diff was then reviewed in full: completed TASK-109 and TASK-110 review
documents are consumed into durable lessons, only completed tasks are removed
from the active plan, TASK-108 and unresolved known issues remain, and the
documentation audit includes explicit removal suggestions without deleting
uncertain artifacts.

For this review pass, `just qformat`, `just qlint`, commit-message validation,
and `git diff --check HEAD^ HEAD` all passed.

## Completed review documents read

- `review_docs/TASK-109.md`: the exact split Rust toolchains, separate locked
  MSRV target and documentation gates, and operator-owned GitHub ref check are
  captured in `design_docs/lessons_learned.md`.
- `review_docs/TASK-110.md`: stable-name picker restoration, inventory-poll
  ordering, stale-session fallback, real-PTY coverage, and bounded readiness
  evidence are captured in `design_docs/lessons_learned.md`.

## Active-plan cleanup

- Removed completed TASK-109 and TASK-110 from
  `design_docs/implementation_plan.md`.
- Preserved `TASK-108` as the sole active `NEW` task.
- Removed `review_docs/TASK-109.md` and `review_docs/TASK-110.md` after their
  useful lessons were captured.
- Retained `review_docs/TASK-PLANNING.md` because it contains the approved
  planning review for active TASK-108.
- Retained `review_docs/TASK-EXTRA.md` as the review history for explicitly
  authorized extra work rather than consuming it as a completed plan task.

## Known issues

- Retained the open picker terminal-state investigation in
  `design_docs/known_issues.md`.
- Retained the open large-input relay root-cause investigation in
  `design_docs/known_issues.md`.
- No known-issue entry is verified closed, so none was removed.

## Verification

- Documentation-only `just qlint` is required for this commit.
- `git diff --check` is required for the housekeeping diff.
- No source, test, manifest, package-version, or release-tag changes are in
  scope.

## Removal suggestions

- `design_docs/external_review.md` — an old whole-project review whose findings
  are represented by completed task history and which has no in-tree references
  beyond housekeeping handoffs. Retain pending operator confirmation.
- `design_docs/external_review_2.md` — an old whole-project review whose
  findings are represented by completed task history and which has no in-tree
  references beyond housekeeping handoffs. Retain pending operator
  confirmation.

No other obsolete or unreferenced documentation artifacts were identified;
there are no stale screenshots or images in `design_docs/`.

## Final decision

Status: COMPLETED

The completed TASK-109 and TASK-110 reviews were read, their durable lessons
were captured, and the active plan and handoff reflect their removal. R003 is
addressed and no material findings remain.
