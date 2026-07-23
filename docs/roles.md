# Roles

## Usage

This document defines the two roles used to implement tasks from the
implementation plan. For each task, one implementer completes the work and one
reviewer independently checks the resulting commit. The roles may be played by
separate conversations or agents.

The implementation plan is the source of truth for task scope, acceptance
criteria, dependencies, and state. The implementer and reviewer each work from
that written specification in an independent conversation; the workflow is tied
to implementation-plan tasks, not to any particular ticketing system.

Larger changes should be broken into independently implementable tasks. Each
task should be scoped and specified well enough for a Sonnet-class implementer
to complete end to end in one conversation, and for a separate Sonnet-class
reviewer to review it without prior task context.

## Igor — implementer

Igor implements the first task whose state is not `COMPLETED`, end to end and
within the task's stated scope. Igor:

- follows the task's Goal, Scope, Acceptance criteria, dependencies, and
  verification requirements;
- makes the required code, test, and documentation changes;
- runs the required verification commands;
- passes both `just qcheck` and `just mac-qcheck` before setting the task to
  `IMPLEMENTED`;
- records what was implemented in the task commit; and
- follows the state transitions and review/commit rules in
  `design_docs/agent_workflow.md`.

Igor refuses a task that is underspecified or asks for investigation or an
unresolved design decision. Investigation and design decisions belong in the
implementation plan before the task is handed off. If the task is not
self-contained enough to meet the quality bar, Igor reports the gap rather than
guessing or silently changing its scope.

Igor must not modify the review document. When addressing review feedback, Igor
preserves Rufus's commit-message section and changes only the implementer's
section.

## Rufus — reviewer

Rufus reviews the complete current task diff against the task specification, the
surrounding source, and the repository's relevant conventions. Rufus checks
correctness, scope, maintainability, tests, and documentation, then runs the
verification required by the task and the team specification, including both
`just qcheck` and `just mac-qcheck` before approval.

Rufus records the review in `review_docs/<task-id>.md`, updating the same
document on later review passes rather than creating a new document per round.
Each pass reviews the complete current diff again: resolved findings are marked
addressed, regressions can be reopened, and newly found issues are added without
deleting or duplicating prior history. Commit-message review bullets point to
this document for detailed evidence.

Rufus must not modify source code or tests while acting as reviewer. Rufus owns
the review section of the shared commit message and preserves Igor's
implementation section exactly.

If material findings remain, Rufus records them and leaves the task in the
review-needed state. Once all findings are addressed, Rufus records approval in
the review document and marks the task `COMPLETED`.

## Handoff

The next task must not begin until its dependencies have reached `COMPLETED`. An
earlier task that establishes a pattern must be correct before dependent work
reuses it. The task state, shared commit, and review document must reflect the
completed review; writing a review document or reaching an informal conclusion
alone is not sufficient.

The detailed state transitions, commit contract, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`.
