# Review: TASK-067

## Findings

### R001

Status: ADDRESSED

The release runbook can leave the operator using a release SHA captured
before TASK-067 added `.github/workflows/release.yml`. The resolved-release
section says to capture the commit after private preparation, and the
TASK-037 checklist says to capture the private-preparation commit, but the
TASK-067 and TASK-068 instructions do not explicitly recapture and verify
the final commit containing the workflow. If the operator uses the earlier
TASK-037 SHA for the first tag, that tag does not contain the release
workflow, so the verification-only bootstrap cannot start and the TASK-067
acceptance criterion for the first tagged workflow is not met.

Update `docs/release.md` so the final release SHA is captured only after
TASK-067 is complete and on `origin/main`, verify that the SHA contains
`.github/workflows/release.yml`, and use that SHA for the annotated tag.

Addressed in the current pass: the runbook now labels the TASK-037 SHA as
historical, captures the final SHA only after TASK-067, checks ancestry, and
requires `.github/workflows/release.yml` with `git cat-file` before tagging.

### R002

Status: ADDRESSED

The final release SHA verification snippet in `docs/release.md` runs
`git merge-base --is-ancestor` and `git cat-file -e` as standalone commands
without `set -e` or explicit status checks. If either check fails, a normal
shell continues to print the SHA, and the later instructions tell the
operator to record and use it. The verification must stop on failure, for
example with `set -euo pipefail` or explicit `|| exit 1` checks before the
SHA is printed.

Addressed in the current pass: the final-SHA snippet now starts with
`set -euo pipefail`, so failed version, ancestry, or workflow-file checks
stop before the SHA is printed.

### R003

Status: ADDRESSED

The first-bootstrap tag command uses `"v$version"` and `"Release $version"`,
but the runbook never assigns or validates the `version` shell variable. In
a normal shell this can create a tag named `v` instead of the resolved
stable version. Define `version` from the captured package metadata and
assert it before constructing the tag, or use the resolved version
explicitly.

Addressed in the current pass: the tag procedure derives and validates
`version` from `Cargo.toml` before constructing the annotated tag.

### R004

Status: ADDRESSED

The first-bootstrap tag block still lacks `set -euo pipefail`. A failed
version assertion, `git tag`, or tag-target verification can therefore be
ignored and followed by `git push`, so the public tag procedure is not
fail-closed. Add strict shell mode and verify that `release_commit` is set
before creating or pushing the tag.

Addressed in the current pass: the tag block now uses `set -euo pipefail`,
requires `release_commit`, validates the version, verifies the tag target,
and only then pushes the tag.

Verification evidence for this pass:

- The exact `just qcheck` recipe passed, including workflow YAML quality.
- The exact `just mac-qcheck` recipe passed.
- `scripts/quality.py commit-message` and gitlint passed.
- `design_docs/stay_planning.md` was untracked user work and was excluded
  from this review as instructed.

## Final decision

Status: COMPLETED
