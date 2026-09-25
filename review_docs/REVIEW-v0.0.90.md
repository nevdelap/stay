# Review: changes since v0.0.90

Scope: correctness, over-engineering, and complexity. Existing review documents were excluded from this review.

## Findings

### P2: A transient client lookup miss can skip the final log capture

When detach occurs while the current client identity is unavailable, the relay can record no detached session name. If the final lookup also misses, `finalize_client_identity` returns `None` for a completed detach, so `LogSession::on_detach` is skipped. Clean logging can miss its final capture during the transient lookup gap the relay explicitly handles.

Relevant code: `src/relay.rs` around `update_relay_state`, `finish_relay`, and `finalize_client_identity` (approximately lines 340–455).

Suggested direction: resolve the session name as part of detaching, or retry the final capture once the identity becomes available.

### P2: Renaming an unsaved session persists an incomplete launch command

When no saved definition is available, `rename_persisted_session` builds one from `pane_current_command` and stores that value as the complete command vector. That tmux field does not include the process arguments. Renaming a session running a command such as `python job.py` can therefore save only `python`; recreating the renamed session launches a different command.

Relevant code: `src/picker/mod.rs`, `rename_persisted_session` (approximately lines 846–897).

Suggested direction: preserve the original launch definition when available. If it cannot be recovered, avoid treating the inferred command as a faithful saved definition.

## Complexity and performance

The relay performs a synchronous `list-clients` lookup every 100 ms per attachment, with retries after misses. The lookup runs before the PTY poll, so a slow tmux query can delay input and output while also adding repeated process-spawn overhead. Session rename tracking may require refreshing identity, but the refresh frequency and retry behavior deserve a less costly strategy.

Relevant code: `src/relay.rs`, `CLIENT_IDENTITY_REFRESH_INTERVAL` and the relay loop (approximately lines 232–240 and 503–517).

## Verification

Tests were not run as part of this review.
