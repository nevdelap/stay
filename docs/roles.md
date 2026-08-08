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

Commit attribution identifies models, not participants. Each distinct model that
performs work gets one `Co-Authored-By:` trailer containing its actual name,
version, and variant. Tools, providers, roles, and agent names are not model
attribution values; for example, adding `-rufus` to a model name does not
identify a different model. When Igor and Rufus use the same model, the trailer
appears only once. The full commit contract and examples are in
`design_docs/agent_workflow.md`.

Larger changes should be broken into independently implementable tasks. Each
task should be scoped and specified well enough for a Sonnet-class implementer
to complete end to end in one conversation, and for a separate Sonnet-class
reviewer to review it without prior task context.

Before starting a task, both roles read `design_docs/lessons_learned.md`. It
records concrete mistakes made in earlier milestones — verification, tmux
behavior, the PTY relay, CLI/config, and test isolation — so they are not
repeated.

## Igor — implementer

Igor implements the first task whose state is neither `COMPLETED` nor `BLOCKED`,
end to end and within the task's stated scope. A `BLOCKED` task is skipped for
implementation as if it were not in the plan, until it is set back to `NEW`,
which Igor must not do unless a human directs it; its specification may still be
worked on as planning. Igor:

- follows the task's Goal, Scope, Acceptance criteria, dependencies, and
  verification requirements;
- makes the required code, test, and documentation changes;
- runs the required verification commands;
- for code or test changes, passes both `just qcheck` and `just mac-qcheck`
  before setting the task to `IMPLEMENTED`; for documentation-only changes, Igor
  runs only the relevant documentation formatting and linting checks;
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
verification required by the task and the team specification. Code and test
changes require both `just qcheck` and `just mac-qcheck` before approval; for
documentation-only changes, Rufus runs only the relevant documentation
formatting and linting checks instead.

For the macOS gate, Rufus runs the exact repository `just mac-qcheck` recipe.
When sandbox restrictions block Just or SSH setup, Rufus uses escalated or
unsandboxed execution while preserving the configured `MAC_*` environment. SSH
wrappers, altered runtime directories, and manually substituted remote commands
are diagnostics only and do not replace the gate.

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

## Housekeeping

Housekeeping reads every applicable completed-task review document in
`review_docs/` and includes its durable lessons in
`design_docs/lessons_learned.md` before removing any review document. It then
reviews `design_docs/known_issues.md`, removes issues that are verified closed,
and moves any durable lesson from them into `design_docs/lessons_learned.md`. It
removes completed tasks from the active implementation plan and deletes review
documents whose useful content is no longer needed. It also audits every file in
the documentation tree for obsolete or unreferenced artifacts and includes an
explicit `Removal suggestions` list in the housekeeping handoff. Each candidate
names its path and rationale; the handoff says explicitly when there are no
candidates. This includes stale screenshots or other images in `design_docs/`;
uncertain artifacts are recorded as suggestions rather than silently deleted. It
preserves the task commits and review history in Git, keeps active or referenced
design reviews, and is documentation-only unless the operator explicitly scopes
additional work.
