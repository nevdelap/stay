# Review: TASK-012

## Findings

### R001

Status: ADDRESSED

The no-default-command implementation still exercises the nested-shell path
that this task is meant to remove. In
`tests/session_creation.rs::no_default_command_runs_one_interactive_shell_with_shell_set_and_unset`,
the wrapper records `2`, `-c`, and the wrapper path, and the test explicitly
expects that output. This is a `<login-shell> -c <login-shell>` invocation,
not one direct interactive shell with no `-c` argument as required by the
task acceptance criteria. The unset-`SHELL` half only waits for a dead pane and
does not establish that the fallback shell is interactive. Correct the session
creation path and tests so both set and unset `SHELL` cases verify the direct
interactive invocation.

Evidence: `create_session` now omits the command tail when no command is
configured, allowing tmux to launch its default shell directly. The set-
`SHELL` test asserts zero wrapper arguments, and the unset-
`SHELL` test asserts a live pane.

### R002

Status: ADDRESSED

The two new integration tests mutate the process-global `SHELL` environment
variable, but each declares its own function-local `static SHELL_LOCK`. Those
mutexes are distinct, so the tests can run concurrently and race while one
test changes or restores `SHELL` during the other test's session creation.
Move the mutex to one shared scope and use that same guard in every test that
mutates `SHELL`.

Evidence: `tests/session_creation.rs` now has one module-level `SHELL_LOCK`,
and both tests acquire it through `shell_lock()` before changing `SHELL`.

### R003

Status: ADDRESSED

This task commit changes `design_docs/agent_workflow.md`, `docs/roles.md`, and
`design_docs/lessons_learned.md` to alter the commit-attribution contract, and
retrofits those changes into TASK-012's scope and acceptance criteria. Those
files were not in the task's pre-implementation scope, and task scoping is
required to be complete before implementation; an implementer must not expand
it mid-task by rewriting the governing workflow. Remove these unrelated
process-document changes from TASK-012 and leave the existing attribution
rules for a separately scoped plan task if they are still desired.

Evidence: the current task diff no longer changes the workflow, roles, or
lessons documents. Attribution work is represented by separate `NEW`
TASK-013 in the implementation plan, dependent on TASK-012.

## Final decision

Status: COMPLETED

Verification:

- `just qcheck` passed independently.
- The exact `just mac-qcheck` recipe passed independently after rerunning it
  with escalation because the sandbox could not create Just's temporary files.
- The working tree was clean before adding this review document.
