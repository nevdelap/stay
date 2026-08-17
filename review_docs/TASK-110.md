# Review: TASK-110

## Findings

### R001

Status: ADDRESSED

The TASK-110 planning commit is self-contained and reviewable. Its Goal,
Dependencies, Scope, and Acceptance criteria define the name-based selection
handoff, inventory-reordering and viewport behavior, stale-session fallback,
filter and attach-modifier coverage, PTY evidence, versioning, and required
Rust gates. The scope is limited to the picker implementation and its tests,
with no unrelated behavior or documentation changes. The task remains `NEW`
as required for a planning approval.

The committed plan is Markdown-format clean. The planning specification passes
the repository formatting and linting checks, and the commit-message contract
checks pass.

### R002

Status: ADDRESSED

The implementation diff preserves the successfully attached session name only
after a successful attach, restores it after the next inventory poll, follows
inventory reordering by name, and invokes the existing visibility handling.
Missing sessions fall back to the create row. The unit and real-PTY tests cover
the reorder/viewport case, both screen preferences, and immediate reattach in
the normal and fuzzy-filter paths without weakening assertions or adding fixed
sleeps. The package version and lockfile both advance from 0.0.86 to 0.0.87.

The exact required gates passed on the reviewed commit: `just qcheck`,
including the Rust 1.88 MSRV checks, and `just mac-qcheck`. The working tree was
clean and the commit-message contract check passed.

## Final decision

Status: COMPLETED
