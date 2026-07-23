# Review: TASK-EXTRA

## Findings

### R001

Status: ADDRESSED

The commit title is `TASK-005: add repository format and lint gates`, but
Nev-directed repository-tooling changes are outside the `TASK-005`
implementation-plan task. Rename the commit to a `TASK-EXTRA` summary that
describes the repository-tooling change. The commit is now titled
`TASK-EXTRA: add repository format and lint gates`.

### R002

Status: ADDRESSED

The new local format/review check is state-changing: `_format_commit` runs
`scripts/format_commit.py`, which invokes `git commit --amend` when it changes
the message. A local `just format` or quiet review gate can therefore rewrite
`HEAD` instead of reporting a formatting failure. Checks should be
non-mutating, or commit-message formatting should be separated into an
explicit local maintenance action and a pure validation recipe. The CI path is
not part of this finding. Nev has explicitly confirmed that this local amend
behavior is intentional and acceptable.

## Final decision

Status: COMPLETED
