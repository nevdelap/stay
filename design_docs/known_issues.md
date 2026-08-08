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
