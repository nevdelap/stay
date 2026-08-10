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

## Full-suite large-input relay timeout

Status: OPEN — stabilized fixture, root cause unverified.

TASK-104 replaced the CPU-saturating busy-pane producer with a controlled-rate
numbered producer and added bounded readiness polling. Two consecutive full
Linux and macOS gate runs passed with the exact payload and busy-pane markers,
but those observations establish fixture stability only; they do not prove
whether the earlier timeout was caused by fixture contention or relay behavior.

Next action: investigate the original timeout if it recurs, and do not weaken
the large-input assertions or mark this issue closed without cause evidence.
