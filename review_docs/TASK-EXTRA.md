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

Status: ADDRESSED

The earlier review incorrectly applied the planned-task versioning rule to
this explicitly directed extra. The Extra commits section of
`agent_workflow.md` states that an extra commit does not bump the package
version unless the human operator explicitly directs that change. The
operator directed this bounded Clippy cleanup without a version bump, so
leaving the package metadata and version assertions unchanged is correct.

### R005

Status: ADDRESSED

This latest extra commit changes `rust-toolchain.toml` from the exact
`1.97.1` pin to floating `stable`, changes the `check`, `acceptance`,
`lint-all`, and `macos` CI jobs from `1.97.1` to `stable`, and changes the
release quality-tools action to `stable`. That reverses TASK-109's completed
Goal and Acceptance criteria, but the earlier review incorrectly treated
those planned-task requirements as binding on this explicitly directed
extra. The Extra commits section permits a bounded out-of-plan workflow
change when the operator directs it; this commit's stated scope is the
stable-toolchain switch while retaining 1.88 for MSRV and release builds.
The exact applicable gates pass, so R005 is addressed.

## Review pass R002

- `just qcheck`: passed, including whole-tree Clippy, Linux tests, and the
  MSRV checks.
- `just mac-qcheck`: passed on the configured macOS host.
- The Clippy cleanup preserves the existing exit-code, signal, and unknown
  cause compact-suffix test behavior.

## Review pass R005

- `just qcheck`: passed.
- `just mac-qcheck`: passed on the configured macOS host.
- `just qacceptance`: passed all 33 Linux acceptance tests.
- `just mac-qacceptance`: passed on the configured macOS host.
- The updated filter readiness helper preserves the original four-second
  bounded polling and now requires the latest matching render to be complete,
  rather than weakening the behavioral assertion.

## Verification

- `just qlint`: passed.
- `uv run --script scripts/quality.py commit-message`: passed.
- `git diff --check HEAD^ HEAD`: passed.
- GitHub's Dependabot configuration requires `version`, `updates`, an
  ecosystem, a directory, and `schedule.interval`; this file supplies those
  fields for Cargo and GitHub Actions.

## Final decision

Status: COMPLETED

R001 remains addressed by the operator's explicit out-of-plan authorization.
R002 and R005 are addressed by applying the Extra commits exception correctly
to the explicitly directed, bounded work.
