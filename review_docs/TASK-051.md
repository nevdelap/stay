# Review: TASK-051

## Findings

### R001

Status: ADDRESSED

The manual detach-key path does not use the relay's stop-and-reap fallback
when the requesting client cannot be resolved. `handle_input` calls
`tmux.detach_client(...)?` directly, so a missing `#{client_pid}` returns out
of `relay_loop` before the attach child is stopped and reaped. The
`detach_client` helper contains the fallback for the SIGTERM and pane-death
paths, but it is not used for this configured detach-key error path. This can
leave the relay's tmux attach child and client running after `stay` reports an
error, contrary to the task's safe-failure requirement.

The revised commit routes manual detach errors through `handle_child_input`,
which stops and reaps the attach child before returning the actionable
resolution error. The new relay test drives the detach key with a missing PID,
verifies the error, and confirms the child has already been reaped without a
detach command.

## Final decision

Status: COMPLETED
