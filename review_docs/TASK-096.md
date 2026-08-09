# Review: TASK-096

## Findings

### R001

Status: ADDRESSED

The macOS acceptance job cannot run this suite under the repository's
supported shell. `tests/acceptance.bats:41` expands `BASHPID` while Bats runs
with `set -euo pipefail`; the macOS runner's `/bin/bash` is Bash 3.2.57 and
does not define `BASHPID`, so setup aborts with an unbound-variable error
before either fixture can run. The same unsupported variable is used for the
PTY paths in `tests/helpers/acceptance_pty.bash:12-13` and the cleanup probe in
`scripts/ci-acceptance-cleanup.sh:74`. Replace these identifiers with a
portable, collision-safe mechanism and verify the actual Bats acceptance
wrapper on macOS.

Evidence: the acceptance Bats file, PTY helper, and cleanup script now use
portable `mktemp` paths. The pinned Bats 1.14.0 wrapper passed on the
configured macOS host using Homebrew Bash 5.3.

### R002

Status: ADDRESSED

`assert_json_inventory` in `tests/acceptance.bats:302-333` does not enforce the
required per-session JSON contract. It only checks that the six names occur
somewhere and that each status, cause, timestamp, and null shape occurs
somewhere in the complete string. It does not bind detached/attached/
terminated status and causes to their expected names, assert `created_at` for
all six rows, assert the optional fields for each row, or compare the six
names against the required creation-order sequence. A response with the right
names and a mismatched status/cause assignment would pass. Parse or otherwise
match each object and assert every required field and the exact order.

Evidence: `assert_json_inventory` now parses six objects, compares their exact
name order, and matches each row's status, RFC 3339 timestamps, command,
directory, termination cause, and null fields. The JSON scenario passed on
Linux and macOS.

## Verification

- Local Bats 1.14.0 run through `STAY_BIN=... scripts/ci-run-acceptance.sh`:
  passed (2 tests, 0 failures).
- `just qcheck`: passed.
- Exact `just mac-qcheck`: passed.
- Mac shell check: the configured host reports Bash 3.2.57 and `BASHPID` is
  unset. The Bats launcher uses `set -euo pipefail`.

## Final decision

Status: COMPLETED
