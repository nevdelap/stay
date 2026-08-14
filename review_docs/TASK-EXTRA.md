# Review: TASK-EXTRA

## Findings

### R001

Status: ADDRESSED

`TASK-EXTRA` is not present in `design_docs/implementation_plan.md`. The
repository workflow requires the implementation plan to be the source of truth
for a task's goal, scope, acceptance criteria, dependencies, and state. This
commit adds `.github/dependabot.yml` without an approved task specification, so
Rufus cannot determine whether the requested update scope, omission of Python
and UV ecosystems, or required verification is complete. Add a self-contained
`TASK-EXTRA` plan entry and update the shared commit and this review against
that specification before approval.

The operator explicitly authorized this commit as an extra outside the
implementation plan and directed Rufus to call it `TASK-EXTRA`. That exception
resolves the scope concern for this review.

The configuration itself is structurally valid for the repository: it covers
the root Cargo manifest and root GitHub Actions workflows on weekly schedules.
No Python dependency manifest or UV lockfile is present, and no source or test
files changed.

### R002

Status: OPEN

This commit changes non-test application source in `src/picker/mod.rs`, but
leaves the package version at `0.0.86` and does not update the package version
metadata or version assertions. The mandatory versioning rules require a
patch increment exactly one above the task baseline, with the lockfile and
every version assertion updated together, whenever non-test application source
under `src/` changes. Because this is an extra commit, the operator must also
explicitly authorize that version change. The commit cannot be approved until
the shared commit is amended with the authorized versioning changes, or the
extra scope is changed so it contains no non-test application source change;
the applicable gates must then be rerun for the final snapshot.

## Review pass R002

- `just qcheck`: passed, including whole-tree Clippy, Linux tests, and the
  MSRV checks.
- `just mac-qcheck`: passed on the configured macOS host.
- The Clippy cleanup preserves the existing exit-code, signal, and unknown
  cause compact-suffix test behavior.

## Verification

- `just qlint`: passed.
- `uv run --script scripts/quality.py commit-message`: passed.
- `git diff --check HEAD^ HEAD`: passed.
- GitHub's Dependabot configuration requires `version`, `updates`, an
  ecosystem, a directory, and `schedule.interval`; this file supplies those
  fields for Cargo and GitHub Actions.

## Final decision

Status: REVIEWED_FOUND_ISSUES

R001 remains addressed by the operator's explicit out-of-plan authorization.
R002 is open because the source change lacks the mandatory authorized version
update.
