# Review: TASK-009

## Findings

### R001

Status: ADDRESSED

The input path now checks the attach-PTY HUP/error state before reading stdin
([src/relay.rs](/home/nevd/stay/stay/src/relay.rs:125)) and treats `EIO` and
`EPIPE` from a closed PTY as normal shutdown in `write_input`
([src/relay.rs](/home/nevd/stay/stay/src/relay.rs:189)). The focused
`closed_attach_pty_input_is_a_normal_shutdown` test covers the non-fatal write
path. This preserves cleanup and retained-pane status handling.

### R002

Status: ADDRESSED

The SIGTERM path now stops the attach child when `detach-client` fails
([src/relay.rs](/home/nevd/stay/stay/src/relay.rs:93)), and the child is reaped
through the normal cleanup path. The
`termination_fallback_stops_a_wedged_attach_child` unit test covers the
failure fallback, while
`sigterm_detaches_and_restores_cooked_terminal_settings` covers the normal
signal path.

### R003

Status: ADDRESSED

The added `forwards_attach_pty_output_to_stay_stdout` integration test verifies
attach-PTY output forwarding. The
`relay_resize_event_updates_the_attach_pty_size` unit test drives
`propagate_winsize`, and `signal_guard_ignores_and_restores_sigpipe` verifies
the SIGPIPE disposition during and after the relay guard lifetime.

## Final decision

Status: COMPLETED

Final approval: R001, R002, and R003 are addressed. The implementation meets
the TASK-009 scope and acceptance criteria.
