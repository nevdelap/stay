# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

Completed task entries are removed from this active plan; their history is
preserved in git (the task commit and its `Reviewed:` section). Add new work as
the next stable task entry; do not reuse an identifier from a removed task.

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
what done means; they must not be filled in incrementally during implementation.
Tasks should be small enough for one implementer/reviewer conversation pair and
must not leave investigation or design choices for the implementer.

## Task state and sequencing

Every task has a stable ID and one valid state from the state rules in
`design_docs/agent_workflow.md`. Once published, a task ID must not be
renumbered, reused for different work, or rewritten because tasks were reordered
or removed. If the plan changes, move or delete the task entry while preserving
all surviving IDs.

Describe dependencies and ordering explicitly in the plan. State which task must
land first, which tasks may proceed independently, and which task should land
last because it documents or consolidates the completed shape. The next task
must not start until its dependencies have reached `COMPLETED`; an earlier task
that establishes a pattern must be correct before dependent work reuses it.

For each active task, its `State:` field must match the state transition being
performed. A review document or informal conclusion does not advance a task; the
plan state must be updated explicitly, and the shared commit and review record
must reflect the transition as specified in `design_docs/agent_workflow.md`.

## Task-specific documentation and verification

Each task's acceptance criteria must name the checks that establish completion
and any limitations on verification. Every implementation patch must pass both
`just qcheck` and `just mac-qcheck`; these are the gates for marking the patch
`IMPLEMENTED` and for Rufus to mark it `COMPLETED`. A task may not claim
implementation or review completion when the macOS gate was not run or did not
pass. Use the smallest relevant checks for documentation-only work only when the
task explicitly scopes the change as documentation-only, as specified in
`design_docs/agent_workflow.md`.

Every task also updates the plan's own state and any pending-work or decision
register used to track rollout. Search for stale comments, documentation,
configuration descriptions, or references to files and behavior changed by the
task, and update them as part of the same task.

## Milestone 2 review-finding fixes

TASK-010, TASK-011, and TASK-012 close the findings from the milestone 1-2
review. They are independent in subject but should land in ID order, since each
is one commit above the previous completed baseline. All three depend on the
milestone 2 core tasks being `COMPLETED`.

## TASK-010 - make the unimplemented CLI surface honest

State: COMPLETED

Goal:

- Stop stay from silently accepting flags it does not yet act on, and reject an
  empty session name at parse time, so the published CLI never lies about what
  it will do.

Dependencies:

- TASK-005, TASK-006, TASK-007, TASK-008, and TASK-009 must be `COMPLETED`.

Scope:

- Unimplemented-flag guard (`src/main.rs`): before any session work, and after
  the existing prompt-integration branch, detect the not-yet-implemented
  attach-mode flags `-r`/`-l`/`-p` and logging flags `-L`/`-t`/`-s`. If any are
  set, write `stay: <flag(s)> not yet implemented` to stderr, return non-zero,
  and do not touch tmux. Name offending flags with their documented spelling.
  Leave `--prompt-integration` as-is (it already reports "not yet implemented").
- Empty session name (`src/session_name.rs`): reject an empty name in
  `validate_session_name`/`parse_session_name` with a distinct diagnostic
  (`invalid session name: must not be empty`); update the unit test that
  currently asserts `""` is `Ok`.
- Do not implement any attach-mode or logging behavior; this task only guards
  the surface.

Acceptance criteria:

- `stay -r work`, `-l work`, `-p work`, `-L f work`, `-t -L f work`, and
  `-s -L f work` each exit non-zero, name the offending flag(s) with
  `not yet implemented` on stderr, and create or attach no session (asserted
  against an isolated test namespace).
- `stay ""` is rejected at parse time with the empty-name diagnostic and makes
  no tmux call.
- Existing create/attach/kill/force/list behavior is unchanged when none of the
  guarded flags are present.
- `just qcheck` and `just mac-qcheck` pass, and `just qcheck` passes twice
  consecutively after the final amend with no further file changes.

## TASK-011 - internal cleanup and error-coupling note

State: COMPLETED

Goal:

- Remove vestigial tmux-wrapper API left over from the pre-relay attach, correct
  a stale doc comment, make the relay unit tests robust against parallel
  execution, and document the tmux error-string coupling so a future tmux
  wording change is a known suspect.

Dependencies:

- TASK-010 must be `COMPLETED`.

Scope:

- Vestigial API (`src/tmux.rs`): delete the unused `detach_command`. Keep
  `attach_command` (still used by tests to assert namespace/target wiring) but
  rewrite its doc comment to describe its current test-only role, dropping the
  obsolete "the caller must execute this command directly" text from the removed
  exec design.
- Relay test isolation (`src/relay.rs` tests): serialize the unit tests that
  mutate process-global state (`TERMINATE_REQUESTED`, the signal disposition,
  the panic hook) behind a shared test mutex, mirroring the integration suite's
  `pty_test_lock`, so `termination_fallback_stops_a_wedged_attach_child`,
  `signal_guard_ignores_and_restores_sigpipe`, and
  `panic_hook_restores_the_attach_terminal_state` cannot race under the default
  parallel runner.
- Error-coupling note (`src/tmux.rs`, `src/session.rs`): add a short comment at
  the missing-server/missing-session/last-session-shutdown matchers recording
  that they key off tmux's English message text, that this is safe today because
  tmux ships no translations, and that forcing a C locale on the wrapper is
  deliberately avoided because tmux copies its environment into created
  sessions. No behavior change.

Acceptance criteria:

- `detach_command` is gone with no remaining references; `attach_command`'s
  documentation matches its current test-only role and makes no claim about a
  caller exec-ing it.
- The named relay unit tests share a mutex and pass reliably; the relevant
  behavior is unchanged.
- The error-classification helpers carry the coupling note.
- `just qcheck` and `just mac-qcheck` pass, and `just qcheck` passes twice
  consecutively after the final amend with no further file changes.

## TASK-012 - default command runs the shell directly

State: COMPLETED

Goal:

- When no command is configured, launch the login shell as a direct interactive
  shell instead of wrapping it as `<login-shell> -c <login-shell>`, removing the
  surprising nested-shell (for example `zsh -c zsh`) default.

Dependencies:

- TASK-011 must be `COMPLETED`.

Scope:

- `src/config.rs`: represent "no default command configured" distinctly
  (`default_command` becomes `Option<String>`) rather than eagerly falling back
  to `$SHELL` as a command string.
- `src/session.rs` (`build_command_tail`/`default_command_tail`): when there are
  no command words and no configured default, return `[<login-shell>]`; when a
  default is configured, return `[<login-shell>, "-c", <default>]`; explicit
  command words are unchanged.
- Update the config unit tests that assert `default_command == "$SHELL"`, adjust
  every `Config { .. }` literal in unit and integration tests for the new
  `Option<String>` field, and add an integration test proving the no-command
  default yields a single interactive shell process (no `-c <shell>` nesting),
  with `SHELL` set and unset.

Acceptance criteria:

- With `SHELL` set and no configured command, a created session runs one shell
  process invoked as the login shell with no `-c` argument.
- With `SHELL` unset, the fallback shell still launches interactively.
- A configured `default_command` (file or `STAY_CMD`) is still run as
  `<login-shell> -c <default_command>`, preserving shell quoting/operators.
- `just qcheck` and `just mac-qcheck` pass, and `just qcheck` passes twice
  consecutively after the final amend with no further file changes.

## TASK-013 - clarify model commit attribution

State: COMPLETED

Goal:

- Make commit attribution unambiguous: trailers name the actual model, including
  its version and variant, and identical models are attributed once.

Dependencies:

- TASK-012 must be `COMPLETED`.

Scope:

- `design_docs/agent_workflow.md`: define model-only attribution and add
  examples for one shared model, distinct models, and duplicate trailers.
- `docs/roles.md`: state that tools, providers, roles, and agent names are not
  model attribution values.
- `design_docs/lessons_learned.md`: record the previous attribution mistake and
  the one-trailer-per-distinct-model rule.

Acceptance criteria:

- The commit contract keeps model attribution mandatory and says the value is
  the actual model name, version, and variant—not a tool or role name.
- Examples show one trailer when both roles use the same model, one trailer per
  model when they differ, and duplicate attribution as invalid.
- `just qformat` and `just qlint` pass with no formatter-generated changes.
