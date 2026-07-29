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

Next action: retrigger CI run #55's workflow. If the rerun passes, treat this as
transient and continue with the next `NEW` implementation-plan task. If it fails
again, investigate and harden the PTY terminal-state test or guard before
continuing.
