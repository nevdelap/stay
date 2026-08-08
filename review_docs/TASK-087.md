# Review: TASK-087

## Findings

### R001

Status: ADDRESSED

The documentation and version changes match the task scope and the current
implementation: the timestamp wording is UTC with a trailing `Z`, the README
documents the implemented create/attach flags and shell integration commands,
and the HTML picker status line matches `IDLE_STATUS`. The patch version moves
from 0.0.71 to 0.0.72 and the lockfile matches.

The operator explicitly authorized the documentation-only verification policy
added by this task. The TASK-087 specification now records that no code/test or
macOS gates are required for this documentation-only task.

The review therefore does not require the previously pending test-gate
evidence. No verification commands were run, per the operator's instruction.

### R002

Status: ADDRESSED

The commit changes `design_docs/agent_workflow.md` and `docs/roles.md` to
introduce a documentation-only verification exception. Those governing process
documents are outside TASK-087's declared scope, and
`design_docs/lessons_learned.md` explicitly says not to expand task scope by
rewriting them mid-task. A process-rule change must be specified as a separate
task; it cannot be used to alter this task's review requirements. Revert these
out-of-scope process edits or move the policy change to a separately scoped
task.

The operator explicitly authorized this guidance change. TASK-087's scope now
includes both governing documents and its acceptance criteria record the
documentation-only verification policy, so the change is auditable in the
task specification.

## Verification

- Reviewed the complete TASK-087 diff against its parent.
- Cross-checked the changed README claims against `src/cli.rs`, `src/main.rs`,
  `src/shell_integration.rs`, and `src/prompt_integration.rs`.
- No verification commands were run, per the operator's explicit instruction
  for this documentation-only task.

## Final decision

Status: COMPLETED
