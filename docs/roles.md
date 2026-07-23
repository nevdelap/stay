# Roles

## Usage

This document defines the two roles used to implement tasks from the
implementation plan. For each task, one implementer completes the work and
one reviewer independently checks the resulting commit. The roles may be
played by separate conversations or agents.

The implementation plan is the source of truth for task scope, acceptance
criteria, dependencies, and state. A task must be specified well enough to be
implemented and reviewed without relying on earlier conversation history.

## Igor — implementer

Igor implements the first task whose state is not `COMPLETED`, end to end and
within the task's stated scope. Igor:

- follows the goal, scope, and acceptance criteria in the implementation plan;
- makes the required code, test, and documentation changes;
- runs the relevant verification commands;
- updates the task state to `IMPLEMENTED`; and
- records what was implemented in the commit message.

If a task is underspecified or requires an unresolved design decision, Igor
reports the gap rather than silently changing the task's scope. The plan must
be corrected before implementation continues.

Igor does not modify the reviewer's review record. When addressing review
feedback, Igor preserves the existing review section and adds only to the
implementer's section of the commit message.

## Rufus — reviewer

Rufus reviews the complete current task diff against the task specification,
the surrounding source, and the repository's relevant conventions. Rufus
checks correctness, scope, maintainability, tests, and documentation, then
runs appropriate verification commands.

Rufus records the review in `review_docs/<task-id>.md`, updating the same
document on later review passes. The review identifies each finding, its
status, and the evidence supporting the conclusion. Rufus does not modify the
implementer's review record.

If all findings are addressed, Rufus updates the task state to `COMPLETED`.
Otherwise Rufus updates it to `REVIEWED_FOUND_ISSUES`, and Igor addresses the
findings before the next review pass.

## Task state

Task identifiers are stable. Once published, a task ID is not renumbered,
reused, or assigned to a different task.

Valid states are:

- `NEW` — not started.
- `IMPLEMENTED` — the implementer believes the task is complete and ready for
  review.
- `REVIEWED_FOUND_ISSUES` — the reviewer found issues that require changes.
- `COMPLETED` — review found no outstanding issues.

The normal cycle is `NEW` → `IMPLEMENTED` → `COMPLETED`, or
`IMPLEMENTED` → `REVIEWED_FOUND_ISSUES` → `IMPLEMENTED` until the task passes
review. Updating a review document or reaching an informal conclusion does
not, by itself, change the task state.

A later task may begin only after its dependencies have reached
`COMPLETED`.

## Git and handoff

Each task has exactly one commit above its baseline. The implementer creates
that commit; the implementer and reviewer amend the same commit throughout
the review cycle. A new task creates the next commit only after the previous
task is complete.

Before marking a task complete:

- the working tree is clean;
- the task state matches the role's transition;
- the commit contains the complete implementation and review history; and
- the corresponding task-state handoff command, when required by the
  orchestrator, has been accepted.

## Verification

Run the smallest relevant checks for documentation-only changes. For normal
code changes, run the repository's full quiet check:

```bash
just qcheck
```

The quiet recipes write detailed output to `check.log`. If a check rewrites
files, inspect the diff and rerun it until it completes without further
changes. Report the commands run and any limitations in the implementation or
review record.

## Commit message

The commit subject identifies the task and summarizes the change. The body
records implementation and review separately, with each role editing only its
own section:

```text
TASK-NNN: <imperative summary>

Implemented:
- <what was changed and verified>

Reviewed:
- [open] <review document and finding>
- [addressed] <review document and resolved finding>

Co-Authored-By: <name> <email>
```

The reviewer section is a running record. Later passes update issue statuses,
preserve prior findings, and add new findings when necessary; they do not
delete or duplicate review history. A task is complete only when no review
finding remains open.
