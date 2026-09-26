# Review: changes since v0.0.90

Reviewed source range: `v0.0.90..a5d66ca` (HEAD). Review documents were excluded when assessing the source changes. Scope: correctness, over-engineering, and complexity.

## Findings

No open findings.


## Resolved findings

- Startup no longer retries client identity lookup up to 100 times. The relay starts with the attach command's session name and lets periodic refresh resolve delayed client publication or a rename.
- Routine identity refreshes now run at the 500 ms pane-control cadence and perform one lookup per refresh instead of retrying synchronously.
- Detach now captures the session name in the same client-list query, preserving the final log capture across identity lookup misses.
- Rename now uses the pane's start command, and the query is session-scoped with `list-panes -s`. A multi-window regression test covers selecting the first pane across session windows.

## Verification

Tests were not run as part of this review.
