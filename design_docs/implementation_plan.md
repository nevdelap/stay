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

## TASK-066 - protect live recreation and improve picker navigation

State: COMPLETED

Goal:

- Make the session picker safe for live sessions and more usable for keyboard
  navigation: recreating a live session must require confirmation, standard
  navigation keys must move through the session list, and confirmation dialogs
  must accept direct `y`/`n` answers.

Context:

- Project review finding G9 identified that pressing `r` on a live session
  bypassed confirmation and immediately entered the existing destructive
  recreate path. This task closes that safety gap while adding the requested
  picker UI improvements.

Dependencies:

- None.

Scope:

- `src/picker/mod.rs`: route the live-session `r` action through the existing
  `RecreateConfirm` mode with the destructive default focused on `No`, just as
  terminated-session recreation is confirmed. `y` and `n` must directly confirm
  Yes or No in kill, kill-all, terminated-recreate, and live-recreate
  confirmations; Enter and Escape retain their existing focused-option and
  cancellation behavior. A cancelled live recreation must leave the running
  session untouched, while a confirmed one must use the existing recreate path
  exactly once and refresh the inventory.
- `src/picker/mod.rs`: add picker key variants and input decoding for standard
  PageUp (`CSI 5~`) and PageDown (`CSI 6~`) terminal sequences. In idle mode,
  Home selects the create row (the first logical row), End selects the last
  session, and PageUp/PageDown move by the current list viewport height, clamped
  to the first and last logical rows. Empty lists and a zero-height viewport
  remain safe no-ops; existing Home/End text-cursor behavior in the create and
  rename prompts remains unchanged. Selection movement continues to keep the
  selected row visible and clears pending attach modifiers.
- Picker unit and integration tests cover the Home/End/page movement and
  viewport clamping, decoding the standard page escape sequences, direct `y` and
  `n` answers in every confirmation mode, live-recreate cancellation, and one
  confirmed live recreation. Update picker status or prompt assertions only
  where the new behavior changes the rendered UI.

Acceptance criteria:

- Pressing `r` on a live session displays `Recreate session "…"? Yes No` with No
  focused and does not kill or recreate anything until confirmation.
- `y` confirms and `n` cancels every picker confirmation mode directly; `n` or
  Escape leaves a live session and its running command intact, while `y`
  performs exactly one existing action and refreshes the list.
- In the idle picker, Home selects the create row, End selects the last session,
  PageUp/PageDown move one viewport at a time, and all four operations clamp
  safely at the list boundaries and maintain visibility.
- The input reader recognizes the common `CSI 5~` and `CSI 6~` page sequences
  without disrupting existing arrow, Home, End, and text-edit decoding.
- Tests cover the new behavior on the picker state machine and through the
  relevant terminal/integration paths.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-037 - publish to crates.io as `stay`

State: BLOCKED

Goal:

- Implement TODO-011: publish `stay` to crates.io so it can be installed via
  `cargo install stay`.

Research already done:

- Checked crates.io directly (`GET https://crates.io/api/v1/crates/stay` →
  `404`): the name `stay` is currently unclaimed and available to register —
  this is worth confirming again immediately before actually publishing, since
  availability can change between now and whenever this task is picked up.
- `Cargo.toml` today has only `name`, `version`, `edition`, and `rust-version` —
  none of the metadata crates.io requires or strongly expects for publish
  (`description`, `license` or `license-file`, `repository`). `cargo publish`
  will refuse without at least `description` and a license field.
- No `LICENSE` file exists in the repo root yet. **Decided: MIT.** The exact
  license metadata and copyright line are specified in Scope below.
- `git remote -v` shows an existing GitHub remote
  (`git@github.com:nevdelap/stay.git`); the exact HTTPS repository URL is fixed
  by the Scope below.

Dependencies:

- The Issue 1 follow-up set (TASK-039 through TASK-048) must be `COMPLETED`. It
  already is - those tasks are done and removed from the active plan, preserved
  in git history - but this release must still include the complete set.
- The 2026-07-31 project-review fixes must all be `COMPLETED` before this task
  starts: TASK-054, TASK-055, TASK-056, TASK-057, TASK-058, TASK-059, TASK-060,
  TASK-061, TASK-062, TASK-063, and TASK-064. The first published release must
  not ship the F1-F27 defects, and two of these fixes are prerequisites for a
  clean publish specifically: TASK-062 removes dead public API before it becomes
  part of the crate's stable surface, and TASK-064 writes the README that
  crates.io renders as the crate's front page and adds the manifest-lint and
  audit gates.

Scope:

- `Cargo.toml`: set the following exact publish metadata:
  - `description = "A terminal session manager for persistent tmux sessions."`
  - `license = "MIT"`
  - `repository = "https://github.com/nevdelap/stay"`
  - `keywords = ["tmux", "terminal", "session-manager"]`
  - `categories = ["command-line-utilities"]`
- Add a new root `LICENSE` containing the standard MIT license text with
  `Copyright (c) 2026 Nev Delap`.
- Bump only the patch component of the package version exactly once from the
  version in `Cargo.toml` when this task starts; update `Cargo.lock` and the
  existing version assertion in `tests/cli_help.rs` to the same value.
- Add a manual `just publish` recipe. It must run
  `cargo publish --locked --dry-run` first and then `cargo publish --locked`; it
  must not run from CI or from another recipe. Document in `README.md` that
  publishing is manual, requires the operator's crates.io credentials, and is
  performed by explicitly invoking `just publish`.
- Before the real publish, query `https://crates.io/api/v1/crates/stay` and
  require HTTP 404; stop without changing release metadata for any other
  response. Run `just qcheck` and `just mac-qcheck` before invoking the real
  publish.
- After publishing, create a fresh temporary directory named `install_root`, run
  `CARGO_INSTALL_ROOT=install_root cargo install --locked --version <new-version> stay`,
  and verify `install_root/bin/stay --version` reports exactly
  `stay <new-version>`. The operator must have valid crates.io publish
  credentials; if credentials are unavailable, stop before the real publish.

Acceptance criteria:

- All dependency tasks listed above are `COMPLETED`: the Issue 1 follow-up set
  (TASK-039 through TASK-048) and the 2026-07-31 project-review fixes (TASK-054
  through TASK-064).
- The exact metadata, MIT license, single patch-version bump, lockfile, and
  version assertion are present and consistent.
- `just qcheck`, `just mac-qcheck`, and `cargo publish --locked --dry-run` all
  pass before publication.
- `just publish` performs the real publication only after its dry run passes; no
  CI workflow publishes the crate.
- A fresh temporary Cargo install using the published version succeeds, and the
  installed binary reports the same version.
- The manual publish workflow and credential precondition are documented in
  `README.md`.
