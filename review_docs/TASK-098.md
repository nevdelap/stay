# Review: TASK-098

## Findings

### R001

Status: ADDRESSED

The required `stay create --attach creates and attaches a session` scenario
never performs the specified later `stay attach --log`. It reattaches with
plain `stay attach` at `tests/acceptance.bats:489`, and no log path or logging
assertion is present in that scenario. This omits an explicit acceptance
requirement that creation has no logging flags while logging starts on the
later attach. Add the later `--log` attach using a hermetic log path and
assert the attach still completes while retaining the requested behavior.

Evidence: the scenario now reattaches with `--log`, waits for the requested
log content, and the focused acceptance suite passes on Linux and macOS.

### R002

Status: ADDRESSED

`pty_wait_until_detached` treats both `detached` and `terminated` as success
(`tests/helpers/acceptance_pty.bash:98-105`). The read-only and low-priority
scenarios call this helper after detaching without reattaching or otherwise
proving that the session remains live. A relay that incorrectly kills the
session during detach could therefore pass those scenarios. The helper or
each live-session assertion must require `status":"detached"`; terminated
postmortem cases should be checked separately.

Evidence: `pty_wait_until_detached` now requires the exact detached status;
terminated cases are checked through `wait_for_terminated` instead. The
read-only and low-priority scenarios pass on both platforms.

### R003

Status: ADDRESSED

The signal-derived attach case creates its fixture with `sleep 1; kill -TERM
$$` (`tests/acceptance.bats:616-624`) and only then starts the PTY client. On a
loaded runner the pane can terminate before the client becomes attached, so
`pty_wait_until_attached` can time out even though the relay is correct. This
also violates the task's prohibition on fixed sleeps to establish readiness.
Use a readiness-controlled long-lived fixture and trigger the signal after
attachment, with bounded polling for the resulting exit.

Evidence: the signal fixture now waits on a release marker, the test waits
until the client is attached, and only then creates the marker. The signal
status scenario passes on Linux and macOS.

## Verification

- Linux Bats 1.14.0 acceptance wrapper: passed (22 tests, 0 failures).
- macOS Bats 1.14.0 acceptance wrapper: passed (22 tests, 0 failures).
- `just qcheck`: passed.
- Exact `just mac-qcheck`: passed.

## Final decision

Status: COMPLETED
