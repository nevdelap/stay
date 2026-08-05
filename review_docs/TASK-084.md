# Review: TASK-084

## Findings

### R001

Status: ADDRESSED

The task acceptance criteria require the exact `just qcheck` recipe to pass.
The full local gate failed twice at the existing
`force_recreate_replaces_an_already_dead_session_with_a_new_command` test in
`tests/session_creation.rs:184`, reporting `timed out waiting for dead pane
swap`. The named test passes in isolation, and the exact `just mac-qcheck`
recipe passes, so this appears to be the known full-suite timing issue recorded
in `design_docs/known_issues.md`, not a TASK-084 regression. Nevertheless, the
required local gate is not green, so this task cannot be approved until a full
`just qcheck` run completes successfully.

The previously flaky full-suite run now completes successfully; the exact
local gate passes, and the named test continues to pass in isolation.

## Verification

- The new reaped-cleanup unit test passed.
- The new SIGINT and SIGHUP relay PTY tests passed in the local and macOS
  suites; existing SIGTERM and Ctrl-C-related coverage also passed.
- The named dead-pane swap test passed in isolation.
- The exact `just qcheck` recipe passed on the final review pass.
- The exact `just mac-qcheck` recipe passed on the final review pass.
- The patch version advances from 0.0.66 to 0.0.67, and `git diff --check`
  passes.

## Final decision

Status: COMPLETED
