# Review: TASK-082

## Findings

### R001

Status: ADDRESSED

`design_docs/stay.html` still documents the old default clean-capture range.
At lines 1020-1022 it says every clean capture uses
`capture-pane -p -S - -E -1`, and lines 1076-1085 describe the visible-screen
range as a behavior exclusive to `-t`. The implementation now deliberately
uses `-E -` for the final `on_detach` boundary capture
(`src/logging.rs:297-298`), and the updated module documentation says this is
required to retain short output. The tracked design document therefore
contradicts the shipped behavior and the task's documentation acceptance
criterion. The current commit updates its clean logging section to distinguish
incremental `on_attach_open`/`on_tick` history-only captures from the final
detach-boundary capture, while retaining the separate truncate and raw
descriptions.

### R002

Status: ADDRESSED

The current rewritten task commit had dropped the reviewer-owned `Reviewed:`
section from its commit message. The reviewer restored that section in the
review amendment and preserved the implementer's section unchanged.

## Verification

- The new real-tmux visible-output logging test passed as part of the
  attachment suite.
- The oversized-line deterministic logging regression and existing logging
  tests passed as part of the full test attempts.
- The focused logging unit tests and visible-output real-tmux test passed.
- The exact `just qcheck` recipe passed on the final review-amended commit.
- The exact `just mac-qcheck` recipe passed on the final review-amended
  commit.

## Final decision

Status: COMPLETED
