# Known Issues

## CI run #55: picker panic terminal-state test

Status: OPEN — awaiting CI retrigger.

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

Next action: retrigger CI run #55's workflow. If the rerun passes, treat this as
transient and continue with the next `NEW` implementation-plan task. If it fails
again, investigate and harden the PTY terminal-state test or guard before
continuing.

## CI run #58: session creation dead-pane timeout

Status: OPEN — awaiting CI retrigger.

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

Next action: retrigger CI run #58's workflow. If the rerun passes, treat this as
transient. If it fails again, investigate the dead-pane polling window and CI
tmux scheduling before changing session-creation behavior.
