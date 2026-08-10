# Review: HOUSEKEEPING

## Findings

### R001

Status: ADDRESSED

The housekeeping rules require an explicit `Removal suggestions` list covering
the documentation tree. The implementation commit did not include that audit
or list. This review records the audit below without deleting uncertain
artifacts.

## Verification

- Documentation-only `just qlint`: passed with no worktree changes.
- `git diff --check HEAD^ HEAD`: passed before review metadata changes.
- Completed plan entries and their review documents were removed while task
  history remains available in Git.
- The open picker terminal-state issue and large-input root-cause issue remain
  recorded in `design_docs/known_issues.md`.

## Removal suggestions

- `design_docs/external_review.md` — an old whole-project review whose findings
  are represented by completed task history and which has no in-tree references.
- `design_docs/external_review_2.md` — an old whole-project review whose
  findings are represented by completed task history and which has no in-tree
  references.

These are suggestions only; neither artifact was deleted in this housekeeping
pass.

## Final decision

Status: COMPLETED
