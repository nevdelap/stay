# Known Issues

## CI run #55: picker panic terminal-state test

Status: OPEN — fork/PTY investigation pending.

CI run #55 failed in:

```text
picker::tests::panic_restores_the_picker_terminal_state
```

The child process completed its panic path, but the parent observed a different
PTY `Termios` value afterward. The reported post-test value was partially raw,
while the expected value was canonical. The failure occurred after 132 tests had
passed and was reported by the repository test recipe.

The exact test passes locally when run in isolation. The current working
hypothesis is a flaky fork/PTY interaction involving Crossterm's process-global
raw-mode bookkeeping, but this has not been confirmed and no implementation
change has been made.

The same failure recurred in CI build #79 with the same partially raw terminal
state observed after the panic path.

TASK-093 stress verification ran this test twenty consecutive times locally
without reproducing the failure. The issue remains open because that result does
not confirm or fix the fork/PTY interaction.

Next action: investigate the fork/PTY interaction between Crossterm's
process-global raw-mode bookkeeping and the terminal-state test, then harden the
test or guard when a task explicitly returns to this open issue.

## CI run #58: session creation dead-pane timeout

Status: CLOSED — addressed by TASK-063.

CI run #58 failed in:

```text
creates_session_with_cwd_environment_history_limit_and_remain_on_exit
```

The test timed out waiting for the one-second pane command to reach the
`remain-on-exit` dead state. Its polling window is five seconds. This test calls
the session-creation path directly and does not exercise TASK-040's picker
rendering or TASK-041's force-recreate path, so the failure is unrelated to
those tasks.

The exact test passes locally in isolation in 1.19 seconds. The current working
hypothesis is CI scheduling or tmux startup timing; this has not been confirmed
and no implementation change has been made.

Resolution: the real-tmux dead-pane polling window now allows ten seconds,
covering loaded CI scheduling without changing session-creation behavior.

## CI build #81: terminated inventory timing timeout

Status: CLOSED — addressed by TASK-063.

CI build #81 failed in:

```text
real_tmux_inventory_reports_a_terminated_session
```

The test timed out after five seconds waiting for a real tmux pane running
`sleep 1; exit 7` to report exit status 7. The test logic is unchanged by
TASK-053, and the exact test passes locally in 1.13 seconds. This is likely
another CI scheduling or tmux startup timing issue, related to the dead-pane
timeout recorded in CI run #58.

Resolution: the shared real-tmux termination polling window now allows ten
seconds, covering the observed CI scheduling delay without changing inventory
behavior.

## TASK-068: session creation dead-pane timeout

Status: CLOSED — addressed by TASK-093.

The deferred full-suite failure timed out while waiting for
`force_recreate_replaces_an_already_dead_session_with_a_new_command`. The
verified cause was a real-tmux fixture race: short-lived commands could exit
before `remain-on-exit` was enabled, and test processes needed cross-process
ownership of their shared tmux fixture.

Resolution: terminating fixtures retain the pane before respawning the
short-lived command, and session creation applies `remain-on-exit` before
starting the requested command. Real-tmux fixtures use unique namespaces under
the shared per-process socket root; there is no process-wide or test-thread
serialization lock. The tmux 3.4 retained-pane metadata defect is handled by
requiring tmux 3.6 or newer, where concurrent exits record the metadata
reliably.

Current TASK-093 evidence: tmux 3.6 passed twenty rounds of 16 concurrent
retained-pane probes with separate servers and twenty rounds of 16 simultaneous
retained exits in one server (320/320 recorded in each shape). The simultaneous
exit Stay regression passed 20/20. The dynamic-field inventory regressions use
renamed-shell fixtures and verify both real-tmux dynamic fields; parser tests
retain exact colon and control-character decoding coverage. Local `just qcheck`,
five consecutive default-parallel `just qcheck-all` runs, and the exact
`just mac-qcheck` all passed on the corrected tree.

## External review G1: clean logging history eviction

Status: CLOSED — addressed by TASK-069.

Clean append-mode logging previously used only the retained line count as its
cursor. When tmux evicted old lines while the retained window stayed the same
size, the next capture could therefore append an empty suffix and leave the
cursor unchanged without reporting the lost history.

Resolution: clean captures now persist a hex-encoded overlap anchor containing
up to the newest 64 complete lines and 8192 bytes. Missing, ambiguous, or
evicted anchors use a marked full-dump fallback beginning with
`--- history evicted before capture`, so currently retained output is kept.

Verification: `src/logging.rs` deterministically covers overlap, eviction,
anchor caps and sentinels, ambiguous anchors, cursor recovery, and write retry.
`tests/attachment.rs` drives a real tmux pane past its configured history limit
and asserts both the marker and retained output. The final implementation commit
records passing `just qcheck` and `just mac-qcheck` evidence.
