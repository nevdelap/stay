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

### R004

Status: ADDRESSED

This housekeeping commit changes the active TASK-108 specification from the
approved `v0.0.86` release data to `v0.0.88`, including all four URLs, hashes,
SRI values, and version assertions. The workflow permits housekeeping to
remove completed tasks and consume their review documents, but requires the
remaining plan to be preserved; a task-specification change belongs in a
separate `Planning:` commit with an independent planning review. The retained
`review_docs/TASK-PLANNING.md` still records approval of the `v0.0.86` scope,
so it does not review the current TASK-108 specification. Move this refresh
to a separate planning commit and update its planning review before
implementation. The housekeeping commit preserves the approved `v0.0.86`
plan, and the subsequent `Planning: refresh Nix release inputs` commit now
contains the `v0.0.88` refresh for independent review.

### R005

Status: ADDRESSED

The parent TASK-111 review still had R012 `OPEN` and the task state was
`REVIEWED_FOUND_ISSUES`. This commit deletes that review and removes the task
based only on the handoff's statement that the operator confirmed completion.
It does not preserve the exact release URL, four archive URLs and hashes,
archive content/mode checks, tap commit and pull-request identifiers, or
four-platform tap CI and `brew test` results required by TASK-111's approved
handoff. The housekeeping guidance requires that release-boundary evidence be
recorded before retirement. The operator confirmed TASK-111 is complete, and
this housekeeping commit now records the release URL, asset base and hashes,
archive contents and modes, tap commit and merge identifiers, and final
four-platform tap CI and `brew test` evidence.

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

R004 and R005 are addressed. The housekeeping commit preserves the approved
TASK-108 plan and records the operator-confirmed TASK-111 completion evidence.

## Current housekeeping handoff

### Completed review documents read

- `review_docs/TASK-111.md`: the pinned mandoc provenance, release-archive
  content and mode checks, same-archive Homebrew man-page installation, and
  the human-owned release boundary are captured in
  `design_docs/lessons_learned.md`. The operator confirmed that the release
  and tap deliverables completed after the historical review state was written;
  the task is therefore retired without amending its implementation commit.
- `review_docs/TASK-EXTRA.md`: the explicitly authorized extra-work boundary,
  version exception, and stable-toolchain verification history remain useful
  review history and are retained.
- `review_docs/TASK-PLANNING.md`: retained because it is the approved planning
  review for active TASK-108.

### Active-plan cleanup

- Retired TASK-111 from `design_docs/implementation_plan.md` based on the
  operator's completion confirmation.
- Preserved TASK-108 as the sole active `NEW` task, updating its pinned release
  data from `v0.0.86` to the current `v0.0.88` assets and checksums.
- Removed `review_docs/TASK-111.md` after its durable lessons were captured.
- Retained `review_docs/TASK-EXTRA.md` as authorized extra-work history and
  `review_docs/TASK-PLANNING.md` as active TASK-108 planning history.

### Known issues

- Retained both open entries in `design_docs/known_issues.md`; neither has
  been verified closed.

### Verification

- Documentation-only `just qformat` and `just qlint` are required.
- `git diff --check` and commit-message validation are required.
- No source, test, manifest, package-version, or release-tag changes are in
  scope.

### Removal suggestions

- `design_docs/external_review.md` — an old whole-project review whose findings
  are represented by completed task history and which has no in-tree
  references beyond housekeeping handoffs. Retain pending operator
  confirmation.
- `design_docs/external_review_2.md` — an old whole-project review whose
  findings are represented by completed task history and which has no in-tree
  references beyond housekeeping handoffs. Retain pending operator
  confirmation.

No other obsolete or unreferenced documentation artifacts were identified;
there are no stale screenshots or images in `design_docs/`.
