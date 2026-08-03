# Review: TASK-EXTRA

This post-housekeeping change is explicitly recorded by the maintainer
outside `design_docs/implementation_plan.md`.
The earlier TASK-068 review history remains preserved in Git.

## Findings

### R005

Status: ADDRESSED

The GitHub Actions path resolves the PR head as the second parent of the
synthetic merge commit. The implementation now passes that SHA as
`--commit <sha>`, selecting only the PR-head commit. The focused GitHub
Actions and non-Actions tests pass with `uv run --offline python -m unittest`.

Reference: <https://jorisroovers.com/gitlint/latest/configuration/cli/>.

### R006

Status: ADDRESSED

The change was added after TASK-068 housekeeping, but the maintainer
explicitly authorized recording it as TASK-EXTRA outside the implementation
plan. No TASK-068 plan entry is recreated; this classification is the process
disposition for this CI-only continuation.

### R007

Status: ADDRESSED

The maintainer squashed the follow-up into the single `TASK-EXTRA` commit
`97d537f`, whose parent is the housekeeping baseline `3a7407c`. The task now
has exactly one commit above its baseline.

### R008

Status: ADDRESSED

All NEW tasks now require exactly one patch increment from their actual task
baseline. This remains valid when earlier tasks are skipped because they are
`BLOCKED`, while preserving the one-bump-per-task rule.

### R009

Status: ADDRESSED

TASK-072 now explicitly treats `#{pane_pipe}` as a boolean-only seam. It
requires one warning on every active-pipe raw attach, skips destructive
backfill, and redirects future output to the requested path for both
stay-created and externally-created pipes. The same-path and changed-path
cases are covered without requiring unavailable destination metadata.

## Final decision

Status: COMPLETED

TASK-EXTRA is approved. R005, R006, R007, R008, and R009 are addressed; the
NEW task specifications meet the planning quality bar.
