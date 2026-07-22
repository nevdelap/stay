# TASK-001 review

## Pass 1

Result: findings remain open.

### R001 — minimum tmux version is undocumented

- Status: open
- Severity: material acceptance-criteria failure
- Evidence: the task requires confirming the versions that introduced
  `ignore-size`, `pane_dead_status`/`pane_dead_time`, and `remain-on-exit`,
  choosing the highest version, and recording the evidence in a code comment
  or `design_docs/stay.md`. The commit hardcodes
  `MINIMUM_TMUX_VERSION` as `3.2` but adds no confirmation or rationale note.
- Required resolution: verify the three feature introduction versions from
  tmux's own release notes or `CHANGES`, retain the correct highest minimum,
  and record the result and reasoning in the source or design document.

### R001 resolution

- Status: addressed
- Evidence: the amended `src/tmux_version.rs` comment records the feature
  history and explains that `ignore-size` was added in tmux 3.2, making 3.2
  the highest required version. The commit message records this pass.

## Verification

- `just qcheck` — passed.
- A second consecutive `just qcheck` — passed.
- Real installed tmux (`3.6a`) — startup gate passed.
- The worktree also contains the pre-existing untracked user file
  `docs/roles.md`; it was not modified.
