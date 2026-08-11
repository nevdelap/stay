# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

## TASK-105 - make five acceptance tests prove their claims

State: COMPLETED

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

## TASK-106 - strengthen nine covered acceptance tests

State: NEW

Goal:

- Make the nine acceptance tests currently marked `Covers claim; improve` in
  `design_docs/acceptance_review.html` prove their named behavior with complete
  and deterministic evidence. Preserve the existing end-to-end behavior checks
  while closing the specific evidence gaps identified by the review.

Dependencies:

- TASK-105 - make five acceptance tests prove their claims (must reach
  `COMPLETED` before this task begins, because this task builds on the current
  acceptance fixtures and helper conventions).

Scope:

- In `tests/acceptance.bats`, improve only these nine reviewed tests and the
  shared fixture/assertion code they directly require:
  `stay attach --log captures clean output across attaches`,
  `stay logging handles history and capture boundaries`,
  `stay create --attach --read-only prevents input changes`,
  `stay attach --read-only prevents mutating input`,
  `stay rejects invalid arguments and session names`,
  `stay rejects conflicting options`,
  `stay list shows the session inventory as human-readable rows`,
  `stay list --json emits a stable machine-readable inventory`, and
  `stay shell-integration prints the prompt snippet`.
- In `tests/helpers/acceptance_pty.bash`, extend the bounded absence-wait
  interface needed by the two read-only tests with the exact optional form
  `pty_wait --absent MARKER --attempts N`; each attempt polls once, then waits
  100 ms, and the helper must print the marker and PTY transcript on timeout. Do
  not add fixed sleeps or unbounded polling. Existing callers without
  `--attempts` may retain the current default.
- In `tests/acceptance.bats`, shared inventory-fixture or JSON-helper changes
  are in scope only when they support the two inventory tests above. The JSON
  helper must stop manually splitting serialized objects and must use `jq` (or
  an equivalently real JSON parser supplied by the repository).
- No production behavior changes are part of this task. Do not modify
  `design_docs/acceptance_review.html`; that review input is intentionally
  untracked and must remain uncommitted.
- Keep all existing lifecycle, relay, logging, error, inventory, startup-file,
  and cleanup assertions unless a criterion below explicitly strengthens that
  assertion. Do not weaken a check, replace an observable marker with a sleep,
  or use unquoted argument-string expansion.

Acceptance criteria:

- For `stay attach --log captures clean output across attaches`, retain the two
  real PTY attaches, detach boundary, clean-capture/no-ANSI assertion, and mode
  `0600` assertion. Assert the complete fixture marker set in the primary log:
  `retained-marker`, `ready`, `periodic-marker`, every `filler-00` through
  `filler-39`, and `visible-marker`. Each expected line occurs exactly once and
  the marker lines occur in fixture order; no unexpected nonempty marker line is
  accepted. Assert the `.offset` sidecar exists, has mode `0600`, has exactly
  the six cursor fields `session`, `log_size`, `line_count`, `partial`,
  `marker_bytes`, and `anchor`, and has values that are internally valid: the
  session is the target session, `log_size` equals the primary log byte size,
  `line_count` and `marker_bytes` are decimal, `partial` is `0` or `1`, and
  `anchor` is `none` or lowercase even-length hexadecimal. Assert the
  `.offset.tmp` path is absent after each completed capture.
- For `stay logging handles history and capture boundaries`, retain the
  more-than-64-KiB fixture and all three recovery cases: missing sidecar,
  malformed sidecar, and a sidecar whose session does not match. Count selected
  early and tail markers (at minimum `large-0000`, `large-0010`, `large-2990`,
  `large-2999`, and `visible-boundary`) in each capture result so a gap or
  duplicate cannot pass. For the initial capture, each selected marker is
  present exactly once and in order. Before each recovery attach, record the
  primary-log length; inspect only the newly appended suffix for that attach and
  require each selected marker exactly once and in order. The missing-sidecar
  suffix must contain no eviction marker, while malformed and mismatched-cursor
  suffixes must contain the documented `--- history evicted before capture ---`
  marker exactly once before their recovered selected-marker sequence. After
  every recovery attach, assert the sidecar is mode `0600`, contains the exact
  six-field cursor format (`session`, `log_size`, `line_count`, `partial`,
  `marker_bytes`, `anchor`), identifies the target session, and has a log size
  equal to the current log. Assert the expected warning or recovery marker for
  every corruption case, rather than checking only that a sidecar file exists.
- For `stay create --attach --read-only prevents input changes`, make the child
  print a `read-pending` marker immediately before it blocks in its `read` loop.
  Wait for that child-side marker before sending input. Send a nonempty
  distinguishable line, then invoke exactly
  `pty_wait --absent "received=" --attempts 50`. This is a five-second bounded
  observation (50 polls at 100 ms) and is the interval used to rule out delayed
  relay; the helper must fail with its marker and transcript diagnostics if the
  line appears. Then send the detach control input and require a clean wrapper
  exit and detached session. The test must prove both that ordinary input is not
  relayed and that detach remains the allowed control input.
- For `stay attach --read-only prevents mutating input`, use the same
  `read-pending` synchronization and the exact same
  `pty_wait --absent "received=" --attempts 50` five-second negative assertion
  instead of a timing-only readiness assumption. After the read-only attach
  detaches, start a later normal writable attach to the same live session, wait
  for `read-pending`, send a different nonempty line, and require the child to
  emit the corresponding `received=` line. Detach cleanly and verify the session
  is detached. This must prove both non-mutation during the read-only attach and
  continued usability by a writable attach.
- For `stay rejects invalid arguments and session names`, invoke every argument
  case through Bash arrays and `run ... stay "${args[@]}"`; no unquoted `$args`
  expansion or shellcheck suppression for word splitting is allowed. Retain the
  unknown/missing command cases, dotted-name rejection, and 129-`界` rejection
  with their usage status and diagnostics. Add a 128-`界` name and require it to
  be accepted, listed, and cleaned up. Add a legal ordinary-space name such as
  `name with space` and require the same create/list/cleanup behavior. Add
  representative rejected names containing a tab, a newline, and a
  Unicode-invalid format/bidi character (U+2028 or U+202E), with usage status
  and the relevant validation diagnostic. After every rejected create/name case,
  assert the JSON inventory is empty and no tmux/session artifact for that
  candidate exists; accepted boundary cases are checked only after those
  empty-inventory assertions.
- For `stay rejects conflicting options`, table-drive the complete conflict
  matrix using argument arrays. It must include: create `--read-only`,
  `--low-priority`, and their combination without `--attach`; attach
  `--truncate`, `--raw`, and their combination without `--log`; `--pass-through`
  paired with `--read-only`, `--low-priority`, `--log`, `--log --raw`, and
  `--log --truncate`; the relevant pairings of `--pass-through` with both client
  modifiers; and the existing top-level `--no-alt-screen`/subcommand,
  `--prompt-integration`/subcommand, and
  `--prompt-integration`/`--no-alt-screen` conflicts. Include repeated forms for
  every boolean/log modifier exercised by the matrix (`--read-only`,
  `--low-priority`, `--truncate`, `--raw`, `--pass-through`, and `--log`),
  including repeated `--log` values. For every rejected matrix row, assert
  status `2`, empty stdout, the specific usage/conflict diagnostic, and no
  session, client, log, or other side effect; the pre-existing keeper session
  must remain unchanged. If a repeated flag is accepted by the parser rather
  than rejected as a conflict, assert that documented parser result explicitly
  and still require no unintended side effect. No case may depend on globbing or
  word splitting.
- For `stay list shows the session inventory as human-readable rows`, retain the
  six-state fixture and no-ANSI assertion. Split stdout into rows and assert
  exactly six rows, in the fixture's documented inventory order, with no extra
  or missing row. Require exact detached and attached row shapes, and require
  terminated rows to contain exit `7` or signal `15` as appropriate with a
  complete UTC timestamp matching exactly `YYYY-MM-DDTHH:MM:SSZ` (a four-digit
  year, two digits for every month/day/hour/minute/second component, and a
  literal trailing `Z`); a broad `.*Z` timestamp match is not sufficient.
- For `stay list --json emits a stable machine-readable inventory`, retain the
  same six-state fixture and replace delimiter splitting (`sed 's/},{/}\\n{/g'`
  or an equivalent approach) with parsing of the complete stdout through
  `jq -e`. Assert the root/object and `.sessions/array` types, exact array
  length `6`, fixture order, and exact type/value contracts for every lifecycle
  object: detached and attached rows have string `current_directory` and
  `current_command == "sleep"` with null termination fields; exit-7 and
  signal-15 rows have null `current_directory`, `current_command == "sh"`, a
  timestamp in the complete UTC shape, and only their corresponding exit/signal
  value. Assert all `created_at` and `terminated_at` timestamps against the same
  exact shape. Include one legal fixture `--cwd` path containing JSON-escaped
  characters (a quote and a backslash), and assert through `jq` that the decoded
  `current_directory` equals the original path; escaped content must not confuse
  the helper.
- For `stay shell-integration prints the prompt snippet`, retain the startup
  sentinel files and the assertion that the command does not edit them. Run
  `stay shell-integration` once with `TMUX` unset and once with
  `TMUX=simulated`; for both invocations require status `0`, empty stderr, and
  the exact same snippet as `stay --prompt-integration`. Write that returned
  snippet to a file and source it, without output or shell errors, in each
  supported shell: `sh`, `bash`, and `zsh`. In each shell call
  `stay_prompt_segment` with `STAY_SESSION_NAME` unset and with a nonempty name,
  and assert the documented empty and `[name] ` results. Recheck every startup
  sentinel after all invocations.
- Any new polling or absence observation is bounded and prints useful
  diagnostics on timeout; no arbitrary fixed sleep is introduced. The exact
  applicable gates for the final acceptance-layer diff, `just qacceptance` and
  `just mac-qacceptance`, pass.
