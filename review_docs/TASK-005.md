# Review: TASK-005

## Findings

### R001

Status: ADDRESSED

The required real-tmux integration coverage is now present in
`tests/tmux_inventory.rs`. It uses a unique `stay-test-<unique>` namespace,
asserts parsed ordering and creation times, and uses a `Drop` guard for
`kill-server` teardown. The in-memory unit test continues to cover the
name/creation-time comparator directly.

### R002

Status: ADDRESSED

Focused tests now cover non-missing failures, invalid UTF-8 on stdout and
stderr, wrapper timeout behavior, and direct child reaping after timeout.

### R003

Status: ADDRESSED

`Tmux::for_test_namespace` is now available to integration tests and enforces
the `stay-test-` namespace prefix; production construction remains fixed to
`stay`.

### R004

Status: ADDRESSED

The executable fixture now uses `OpenOptions::create_new` with a process-wide
counter, and the timeout fixture uses `exec sleep 3`. Two consecutive local
`just qcheck` runs passed after this change.

### R005

Status: ADDRESSED

The repository declares `just mac-qcheck` mandatory for implementation and
review. SSH escalation now reaches the Mac host and the installed tmux is
being exercised by the remote test suite.

### R006

Status: ADDRESSED

The Mac gate exposes a tmux-format portability bug. On tmux 3.7b, the exact
format passed by `list_sessions` (`#{session_name}\t#{session_attached}\t#{session_created}`)
produces literal `\\t` text rather than tab separators. The parser therefore
reports `tmux session row is missing attachment count` and the real-tmux Mac
tests fail. The implementation now uses a colon delimiter, which is excluded
by session-name validation; the remote Mac suite passes when `/usr/local/bin`
is explicitly added to `PATH`.

### R007

Status: ADDRESSED

The required `just mac-qcheck` still fails because the Cargo test process on
the Mac cannot find the installed `/usr/local/bin/tmux`, although the remote
shell can. Running the same remote test command with `/usr/local/bin` added
to `PATH` passes all 10 library tests, 33 binary tests, and 2 integration
tests. `scripts/maccmd` now exports `/usr/local/bin` and `/opt/homebrew/bin`;
the actual escalated `just mac-qcheck` passes.

### R008

Status: ADDRESSED

Local `just qcheck` remains flaky despite the fixture changes. Two of the
three latest runs failed in
`tmux::tests::rejects_invalid_utf8_from_tmux_stdout_and_stderr` with
`failed to start tmux: Text file busy (os error 26)` while executing the
generated script; intervening runs passed. The required two-consecutive-pass
gate is now reliable after replacing generated executable fixtures with
`/bin/sh -c` test commands. Two consecutive local `just qcheck` runs pass.

## Final decision

Status: COMPLETED
