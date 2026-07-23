# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared
state transitions, commit contract, handoff procedures,
review-document format, and verification workflow are defined in
`design_docs/agent_workflow.md`; role responsibilities are defined in
`docs/roles.md`.

## Tasks

Completed task entries are removed from this active plan. Preserve their
history in git and in the corresponding review documents. Add new work as the
next stable task entry; do not reuse an identifier from a removed task.

### Task template

```markdown
## TASK-000 - short title

State: NEW

Goal:
- Describe the user-visible or maintainer-visible outcome.

Dependencies:
- List the tasks that must reach `COMPLETED` before this task may begin.

Scope:
- List files, modules, or documentation expected to change.

Acceptance criteria:
- State the behavior or documentation that must be true when complete.
- State the tests or quiet Just recipe that must pass.
```

Every task must be fully scoped before implementation begins. Its Goal, Scope,
Acceptance criteria, dependencies, and verification must completely describe
what done means; they must not be filled in incrementally during
implementation. Tasks should be small enough for one implementer/reviewer
conversation pair and must not leave investigation or design choices for the
implementer.

## Task state and sequencing

Every task has a stable ID and one valid state from the state rules in
`design_docs/agent_workflow.md`. Once published, a task ID must not be
renumbered, reused for different work, or rewritten because tasks were
reordered or removed. If the plan changes, move or delete the task entry while
preserving all surviving IDs.

Describe dependencies and ordering explicitly in the plan. State which task
must land first, which tasks may proceed independently, and which task should
land last because it documents or consolidates the completed shape. The next
task must not start until its dependencies have reached `COMPLETED`; an
earlier task that establishes a pattern must be correct before dependent work
reuses it.

For each active task, its `State:` field must match the state transition being
performed. A review document or informal conclusion does not advance a task;
the plan state must be updated explicitly, and the shared commit and review
record must reflect the transition as specified in
`design_docs/agent_workflow.md`.

## Task-specific documentation and verification

Each task's acceptance criteria must name the checks that establish completion
and any limitations on verification. Use the smallest relevant checks for
documentation-only work and the repository's full quiet check for normal code
changes, as specified in `design_docs/agent_workflow.md`.

Every task also updates the plan's own state and any pending-work or decision
register used to track rollout. Search for stale comments, documentation,
configuration descriptions, or references to files and behavior changed by the
task, and update them as part of the same task.

## Milestone 2 — core lifecycle

Milestone 2 is implemented in the order below. `TASK-005` establishes the
tmux command and parsing boundary; `TASK-006` adds deterministic creation and
startup validation; `TASK-007` adds plain listing and destructive lifecycle
actions; and `TASK-008` adds the temporary interactive attachment path.
`TASK-007` and `TASK-008` may proceed independently once TASK-006 is
`COMPLETED`, but both must complete before the relay begins. No task in this
milestone implements single-key interception, attach-mode flags, logging, or
the terminated-session `t` presentation; those belong to later milestones in
`design_docs/stay.md`.

The following decisions are fixed for these tasks so implementation does not
require an unresolved design choice:

- Every short-lived/control tmux invocation uses the dedicated `-L stay`
  server namespace and the shared bounded subprocess timeout. The one
  intentional exception is the long-lived interactive `attach-session` in
  TASK-008: it uses the same argv builder and namespace, but is `exec()`ed
  into the caller's controlling terminal and therefore has no deadline while
  the user is attached. A missing server while listing means an empty list;
  other command failures are reported with the tmux stderr.
- The tmux wrapper has an explicit namespace parameter for tests. Production
  construction is private/fixed to `stay`; integration tests construct an
  instance with a unique `stay-test-<unique>` namespace and always tear down
  that server. No production dispatch may accept a namespace from CLI,
  config, or environment.
- Session names are passed as separate argv values. They are already rejected
  by CLI parsing when they contain tmux-disallowed punctuation or control
  bytes; no silent mangling or shell interpolation is permitted.
- Creation uses `remain-on-exit on` and applies the configured
  `history_lines` as tmux's `history-limit`, so later post-mortem and logging
  milestones have the required session state from the beginning.
- Explicit command words are passed as executable-plus-argv values. The exact
  creation argv is `tmux -L <namespace> new-session -d -s <name> [-c <cwd>]
  -e STAY_SESSION_NAME=<name> -- <executable> <argv...>`; `--` terminates
  tmux options so an executable or argument beginning with `-` is not
  consumed as a tmux option. Before creating a session, stay resolves the
  first explicit word using `PATH` and verifies it is a regular executable;
  failure is reported before tmux creation. When no explicit command words
  are supplied, the exact command tail is `-- <login-shell> -c
  <config.default_command>`, where `<login-shell>` is `$SHELL` or
  `/bin/sh`; the configured default is therefore one shell command string,
  with shell operators and quoting interpreted by that shell. An exit from
  either command form is a legitimate short-lived command result, not a
  startup error. No sleep, readiness probe, exit-code threshold, or
  post-creation “failed immediately” heuristic is used. Existing-session
  invocations with trailing command words are rejected at runtime as well as
  by the already-existing flag validation.
- Attachment temporarily replaces stay with `tmux attach-session`; tmux's
  normal prefix-based detach is accepted until the relay supplies the
  single-key UX. Milestone 2 does not claim pane exit-status propagation.

## TASK-005 - tmux command wrapper and session inventory

State: NEW

Dependencies:
- TASK-001, TASK-002, TASK-003, and TASK-004 must be `COMPLETED`.
- This task must complete before TASK-006, TASK-007, or TASK-008 starts.

Goal:
- Establish the single, testable boundary through which stay invokes tmux and
  obtain a deterministic inventory of stay-managed sessions for both plain
  listing and lifecycle decisions.

Scope:
- Add `src/tmux.rs` (or an equivalently named module) containing a typed
  wrapper with a private production constructor fixed to `stay`, an explicit
  test-only/custom constructor for isolated namespaces, argv construction,
  captured stdout/stderr,
  non-zero-status errors, and the same deadline-bounded wait/child cleanup
  behavior established by the version gate.
- Add the session-record types and parsing needed from
  `list-sessions -F` for session name, attachment count, and creation time.
  Treat the known “no server/no sessions yet” result as an empty inventory.
- Add deterministic sorting by session name with creation time as the
  tie-break, and derive only the Milestone 2 `a` (attached) and `d`
  (detached) markers. Do not add terminated-pane fields in this task.
- Keep all user-controlled values as separate `Command` arguments; no shell
  command string may contain a session name.

Acceptance criteria:
- Production wrapper calls target exactly the `stay` tmux namespace and return
  a clear timeout error instead of hanging when tmux does not respond. Test
  wrapper instances target only their explicitly injected unique
  `stay-test-<unique>` namespace.
- Production code cannot select a namespace; tests can select only through the
  explicit constructor, and wrapper tests prove both argv forms.
- Missing-server listing is an empty list, while malformed rows, tmux
  failures, and invalid UTF-8 produce actionable errors.
- Unit tests cover argv construction, row parsing, marker derivation,
  deterministic sorting, missing-server handling, and timeout cleanup.
- A real-tmux integration test uses a unique `-L` test socket, creates
  sessions with distinct names/times, verifies the parsed sorted inventory,
  and tears the server down even when the test body fails.
- `just qcheck` passes twice consecutively with no additional file changes.

## TASK-006 - create and validate startup

State: NEW

Dependencies:
- TASK-005 must be `COMPLETED`.

Goal:
- Make `stay <name> [command...]` create a persistent tmux-backed session with
  deterministic command validation before tmux is invoked.

Scope:
- Add creation orchestration in `src/session.rs` (and update `main.rs`
  dispatch as needed), using the TASK-005 wrapper.
- Create with `new-session -d -s <name>`, the configured `-c` directory when
  supplied, `-e STAY_SESSION_NAME=<name>`, `remain-on-exit on`, and the
  configured `history-limit`. Do not create a stay daemon.
- Resolve the configured default command as a shell command string through the
  configured user's shell; for explicit command words, resolve argv[0] via
  `PATH` and require a regular executable before calling `new-session`.
- If explicit-command preflight fails, return non-zero with the command and
  reason and do not create a tmux session. Once preflight succeeds, every
  command exit status, including 1 and 127 produced by the command itself, is
  treated as a legitimate short-lived command result; this task does not
  infer startup failure from timing or pane state.

Acceptance criteria:
- Creation of a missing/non-executable explicit command fails before
  `new-session`, with a non-zero status and a clear diagnostic; a command that
  exists but exits quickly is not misclassified.
- A real-tmux integration test verifies creation, configured working
  directory, `STAY_SESSION_NAME`, history-limit, and remain-on-exit.
- Integration tests verify that an explicit executable receives multiple
  arguments as separate argv values, including an argument containing spaces
  and shell metacharacters, without shell reinterpretation.
- Integration tests verify that omitting command words uses
  `config.default_command` as the command string, invokes the configured
  shell path with `-c`, and preserves shell operators/quoting according to
  that shell's semantics.
- Integration tests create commands that exit quickly with status 1 and
  status 127, and verify both sessions are retained and are not reported as
  preflight/startup failures; only the non-executable preflight case is
  rejected before creation.
- `just qcheck` passes twice consecutively with no additional file changes.

## TASK-007 - plain listing, kill, and force-recreate dispatch

State: NEW

Dependencies:
- TASK-005 and TASK-006 must be `COMPLETED`.

Goal:
- Complete the Milestone 2 command surface: plain non-TTY listing, explicit
  kill, and force-recreate, with safe behavior for missing and existing
  sessions.

Scope:
- Add lifecycle operations to `src/session.rs` and user-facing dispatch and
  rendering in `src/main.rs` (or the established command module).
- With no session name in a non-interactive invocation, print the sorted
  `a`/`d` inventory using this exact UTF-8 format and exit successfully,
  including when the tmux server has never started:
  `MARKER<TAB>SESSION_NAME<NEWLINE>` per session, one session per line. The
  marker is exactly `a` or `d`; the session name is emitted unchanged. Since
  CLI validation rejects tabs, newlines, and all control bytes in names, no
  additional escaping is used and the tab is an unambiguous separator. An
  empty inventory produces zero bytes on stdout (no header and no blank
  line).
- Implement `-k <name>` using `kill-session`, reporting a clear error for an
  unknown session.
- Implement `-f <name>` as kill-if-present followed by creation using the
  same cwd, command, environment, remain-on-exit, and history-limit rules as
  TASK-006. Do not prompt in this non-interactive core milestone; interactive
  picker confirmation belongs to the picker milestone.
- Ensure action dispatch never combines incompatible paths and that all
  errors reach stderr with non-zero exit status.

Acceptance criteria:
- Real-tmux integration tests cover plain empty listing, attached/detached
  markers, deterministic output, kill, force-recreate, and recreation after
  both a live and already-dead session.
- Listing tests assert the exact output bytes, including tab/newline
  separators, Unicode and space-containing names, and zero-byte empty output.
- Tests verify that kill does not accidentally create a session and that
  force-recreate leaves exactly one session with the requested command.
- `stay --help` remains valid and the existing CLI/config/name-validation
  tests continue to pass.
- `just qcheck` passes twice consecutively with no additional file changes.

## TASK-008 - temporary raw interactive attachment

State: NEW

Dependencies:
- TASK-005 and TASK-006 must be `COMPLETED`.
- TASK-007 may proceed independently, but TASK-007 must be `COMPLETED`
  before the next milestone begins.

Goal:
- Attach to an existing session through tmux's native interactive client until
  Milestone 3 replaces this path with the stay relay.

Scope:
- Add the existing-session attach path in `src/session.rs` using the
  TASK-005 namespace/argv builder, and wire it through `src/main.rs`.
- Validate that trailing command words are rejected before attachment, then
  replace the stay process with `tmux -L <namespace> attach-session -t
  <name>` using `exec()` so tmux owns the real controlling terminal.
- Document and test this as the deliberate exception to the bounded timeout:
  the attach call has no deadline while the user remains attached, but all
  pre-attach/control calls retain the wrapper timeout. Do not add stay-side
  raw-mode, PTY-relay, signal, or pane-exit-status behavior yet.

Acceptance criteria:
- A real PTY integration test launches stay with a controlling terminal,
  attaches to a test session, sends tmux's normal prefix detach, and verifies
  that stay returns cleanly. A pipe, `/dev/null`, or ordinary non-interactive
  test is not used to claim attach coverage; non-interactive coverage belongs
  to the wrapper and lifecycle tests.
- Tests verify that the attach argv uses the injected test namespace while
  production dispatch is fixed to `stay`, and that no bounded wait is applied
  to the long-lived attach process.
- Existing-session trailing command words fail without starting an attach.
- `just qcheck` passes twice consecutively with no additional file changes.

Milestone 2 is complete only when TASK-005, TASK-006, TASK-007, and TASK-008
each reach `COMPLETED` through the implementer/reviewer workflow. The next
eligible work is TASK-009 for the thin relay, which must not begin before all
four tasks are complete.
