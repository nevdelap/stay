# Review: TASK-097

## Findings

### R001

Status: ADDRESSED

`assert_usage_error` in `tests/acceptance.bats:223-227` checks only exit status
2, empty stdout, and nonempty stderr. The task's invalid-argument acceptance
contract explicitly requires usage on stderr, and every unknown-command,
unknown-option, missing-value, invalid-name, and overlong-name case delegates
that assertion to this helper. A regression that emits an arbitrary diagnostic
without usage would therefore pass the whole negative-argument scenario.
Assert the documented usage shape and the required diagnostic for each
subcase, rather than merely requiring nonempty stderr.

Evidence: `assert_usage_error` now requires Clap's help/usage footer for every
usage error, and the unknown-command, unknown-option, and missing-value cases
also require the explicit `Usage:` line. The full acceptance suite passed on
Linux and macOS.

### R002

Status: ADDRESSED

The partial and large branches of `stay attach --pass-through` in
`tests/acceptance.bats:485-493` assert only that the command exits zero. They
never wait for or inspect `partial_file` or `large_file`. An implementation
that accepts the input and forwards nothing would pass both required cases;
only the incremental branch proves that bytes reached the session command.
Wait for the partial payload and assert its exact content, and assert the
large payload's size/content markers after forwarding.

Evidence: the partial case waits for exact file content, while the large case
waits for exactly 20,000 bytes and checks both payload edges. The pass-through
scenario passed on Linux and macOS.

## Verification

- Linux Bats 1.14.0 acceptance wrapper: passed (15 tests, 0 failures).
- macOS Bats 1.14.0 acceptance wrapper: passed (15 tests, 0 failures).
- `just qcheck`: passed.
- Exact `just mac-qcheck`: passed.

## Final decision

Status: COMPLETED
