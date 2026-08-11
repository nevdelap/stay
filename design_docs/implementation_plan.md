# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

## TASK-105 - make five acceptance tests prove their claims

State: IMPLEMENTED

Goal:

- Make these five acceptance tests fail when the behavior in their names is
  removed. Each test must observe the named behavior directly, not infer it from
  a weaker inventory, metadata, or successful-relay result.

Dependencies:

- None.

Scope:

- In `tests/acceptance.bats`, update only the test
  `stay create uses the configured default command` so its child command
  provides direct evidence that the configured default ran. Its existing create,
  detached-list, JSON-list, and cleanup flow remains in scope.
- In `tests/acceptance.bats`, update only the test
  `stay create starts the session in the requested directory` so its child
  process provides direct evidence of its working directory. Its existing
  canonicalization, create, and JSON inventory flow remains in scope.
- In `tests/acceptance.bats`, update only the test
  `stay create --force-recreate replaces an existing session` so the live
  replacement is process-observable. Its existing live collision and
  terminated-session warning/replacement branches remain in scope.
- In `tests/acceptance.bats`, update only the test
  `stay create --attach --low-priority attaches at low priority` so it observes
  the low-priority client state in addition to its existing PTY behavior.
- In `tests/acceptance.bats`, update only the test
  `stay attach --low-priority uses the low-priority client mode` so it observes
  the low-priority client state in addition to its existing PTY behavior, and
  remove any redundant attachment wait from that test.
- In `tests/helpers/acceptance_tmux.bash`, add only the bounded,
  session-specific tmux client-state polling needed by the two low-priority
  tests. Export that helper from the acceptance suite setup in
  `tests/acceptance.bats`.
- Do not modify `design_docs/acceptance_review.html`; it is the review input for
  this task and must remain uncommitted.
- Do not weaken existing lifecycle, metadata, relay, or detach assertions and do
  not change production behavior.

Acceptance criteria:

- For `stay create uses the configured default command`, set `STAY_CMD` to a
  shell command that writes the child PID to a unique marker file and then
  executes `sleep 60` (the command must use `exec` so the recorded PID is the
  live sleeping process). Wait until that file is present, create without
  trailing command words, and assert detached text and JSON inventory state plus
  the exact JSON field `"current_command":"sleep"`. A successful create or
  list-only observation is insufficient.
- For `stay create starts the session in the requested directory`, run a child
  command equivalent to `pwd > MARKER`, with `MARKER` outside the requested
  directory. Pass the canonical physical path through `--cwd`, wait for the
  marker, and assert its trimmed content equals that canonical path. Also retain
  the JSON `current_directory` assertion as a separate metadata check.
- For `stay create --force-recreate replaces an existing session`, cover both a
  live and an already-terminated session. In the live branch, make the original
  child execute `sleep 60` after writing its PID to an original marker file;
  force-recreate with a distinct command that executes `sleep 60` after writing
  a replacement PID to a different marker file; and wait with bounded polling
  until the replacement marker exists. Prove the recorded PIDs differ,
  `kill -0 OLD_PID` fails after the replacement, and `kill -0 NEW_PID` succeeds
  while the replacement is expected to remain alive. Assert the live
  replacement's JSON `current_command` is exactly `"sleep"`. In the terminated
  branch, retain the prior exit-code warning and prove the replacement is
  detached with JSON `"current_command":"sleep"`.
- For `stay create --attach --low-priority attaches at low priority`, retain the
  real PTY attach, input/output relay, and clean-detach assertions. While
  attached, observe this session's tmux client and require its supported
  `ignore-size` client flag, which is the flag stay uses for low priority.
  Normal attach success alone must not satisfy the test.
- For `stay attach --low-priority uses the low-priority client mode`, retain the
  existing attach, input/output, and clean-detach assertions. While attached,
  make the same direct `ignore-size` client-state assertion. Use supported tmux
  client metadata; a competing-client scenario is not required. There must be
  exactly one `pty_wait_until_attached` call; retain the separate child-output
  readiness wait only to synchronize the fixture, not as evidence of client
  priority.
- For both low-priority tests, the client-state observation must be bounded and
  session-specific. Use the acceptance server namespace (`tmux -L stay`),
  validate the controlled socket-root environment, and query exactly
  `tmux -L stay -f /dev/null list-clients -F '#{client_session}:#{client_flags}'`.
  Identify the row whose `#{client_session}` equals the test's target session
  and require that row's `#{client_flags}` contains the `ignore-size` token.
  Matching an unrelated client, merely seeing any client, or accepting a
  different flag is insufficient.
- Any new process/file/client polling must have bounded timeout diagnostics. Do
  not add arbitrary sleeps, broaden matches, suppress failures, or alter inputs
  merely to make a test pass. Preserve all existing assertions unless this
  specification explicitly adds a stronger replacement.
- `just qacceptance` and `just mac-qacceptance` pass for the final diff.
