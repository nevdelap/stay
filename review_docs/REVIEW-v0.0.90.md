# Review: changes since v0.0.90

Scope: correctness, over-engineering, and complexity. Review documents were excluded when reviewing the source changes.

## Findings

### P2: Relay identity polling adds avoidable process and latency cost

The relay synchronously refreshes client identity about every 100 ms per attachment. Each refresh invokes tmux, and the relay does not poll the PTY while that query runs. This makes rename-safe pane polling and logging depend on repeated process launches and can add input/output latency when tmux is slow.

Change routine identity refreshes to run no more often than the existing 500 ms pane-control cadence, and make each routine refresh a single lookup rather than a retry loop. Keep bounded retries for startup/finalization if needed. A missed refresh must continue to suppress actions that need a session name until identity is resolved again; detach must remain PID-targeted and capture the session name atomically; rename must continue to update pane polling and logging to the new session.

Relevant code: `src/relay.rs`, `CLIENT_IDENTITY_REFRESH_INTERVAL`, `lookup_client_identity`, `require_client_identity`, and the relay loop (approximately lines 232–310 and 503–525).

## Resolved findings

- Detach now captures the session name in the same client-list query, preserving the final log capture across identity lookup misses.
- Rename now uses the pane's start command, and the query is session-scoped with `list-panes -s`. A multi-window regression test covers selecting the first pane across session windows.

## Verification

Tests were not run as part of this review.
