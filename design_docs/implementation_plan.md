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

## TASK-047 - show the stay version in the picker title

State: NEW

Goal:

- Make the picker title identify the running stay version, matching the version
  already shown by the built-in tmux status bar.

Dependencies:

- None.

Scope:

- `src/picker/mod.rs`: derive the title from `env!("CARGO_PKG_VERSION")` and
  render exactly `stay v<CARGO_PKG_VERSION>` in the existing bordered title.
  Include the title width in the picker content-width calculation. If the
  physical frame is narrower than the complete title, truncate only the title
  text to the available inner width; preserve the border and never panic.
- Add render tests asserting exact title `stay v<package-version>` at a normal
  width and that a narrow frame preserves valid borders with title truncation.

Acceptance criteria:

- Every picker render includes `stay v<CARGO_PKG_VERSION>` when the frame can
  display the full title; an impossibly narrow frame truncates only the title.
- The displayed value follows the Cargo package version automatically; no
  duplicated hard-coded version is introduced.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-048 - return to the picker after detaching

State: NEW

Goal:

- When a user attaches from the picker and presses the configured detach key,
  return them to the picker instead of exiting stay immediately.

Dependencies:

- TASK-043 — the picker loop must preserve the attach-modifier state and
  behavior established by the combined-modifier task.

Scope:

- `src/picker/mod.rs` and the picker/relay boundary: keep the picker lifecycle
  alive across a picker-selected attach. When
  `session::attach_session_with_input` returns `Ok(exit_status)`, regardless of
  whether the relay returned because of the configured detach key or because the
  pane ended, close the current terminal guard and reopen the picker with a
  freshly polled inventory. When it returns `Err`, preserve the existing error
  return behavior and do not loop.
- Reuse the existing screen preference and terminal guards for every picker
  round; do not leave raw mode, alternate screen, or cursor visibility in a
  broken state between rounds.
- Preserve the current behavior for quitting the picker, explicit `stay attach`,
  and attach failures. A session that exits while attached must not cause a busy
  loop; refresh the inventory and allow the user to quit or choose another
  session normally.
- Preserve the selected attach modifiers and residual-input handoff for the
  attach that just began. Reset transient picker modes, pending modifiers, and
  feedback when a new picker round starts.
- Add PTY/integration coverage that selects a session, attaches, sends the
  configured detach key, observes the picker again, and then quits. Cover both
  alternate-screen and forced-main-screen preferences where the existing harness
  supports them.

Acceptance criteria:

- A picker-selected attach followed by detach returns to a working picker.
- The user can select another session or quit after returning.
- Terminal state is restored correctly across the attach/picker transition.
- `stay attach` outside the picker still exits according to its existing relay
  behavior.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-045 - bound session names and stabilize empty-picker sizing

State: NEW

Goal:

- Ensure the empty picker is not narrower than the normal shortcut panel, and
  prevent an arbitrarily long session name from making the picker unusably wide.

Dependencies:

- TASK-040 — the height calculation and viewport must use the same logical-row
  and visible-row model.

Scope:

- `src/session_name.rs`: add a documented maximum of 128 Unicode scalar
  characters to the shared session-name validator and a dedicated validation
  error. Apply it uniformly to CLI create/attach/kill parsing and picker create
  and rename input; do not silently truncate names passed to tmux.
- `src/picker/mod.rs`: compute content width as the maximum of the full idle
  shortcut-panel width, the create-row width, the wrapped prompt/status width,
  and each rendered session-row width including TASK-040's one-column gutter.
  Add the two border columns and clamp the resulting rectangle to the actual
  frame width. The width grows for longer prompts or names, but never shrinks to
  the empty-status text. Compute the requested height exactly from the create
  row, one logical row per session, separator, wrapped prompt/status lines, and
  border, then clamp that rectangle to the actual frame height for scrolling; do
  not add an arbitrary empty-list minimum.
- Add render/layout tests for no sessions, one session, a wrapped prompt, a
  maximum-length name, an over-limit name, and Unicode names. Add CLI/parser
  coverage proving the shared maximum applies to every session-name entry point.

Acceptance criteria:

- With no sessions, the picker width is at least the width required by the
  normal shortcut panel, while the height contains exactly its required rows and
  status area.
- A valid name of 128 characters is accepted; a 129-character name is rejected
  consistently by CLI and picker validation with a clear error.
- The picker cannot be widened without bound by a user-provided name.
- Existing disallowed-character validation remains unchanged.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-046 - kill all terminated sessions from the picker

State: NEW

Goal:

- Add a picker action to remove all currently terminated sessions in one
  confirmed operation.

Dependencies:

- None.

Scope:

- `src/picker/mod.rs`: bind uppercase `K` to a confirmation prompt for “kill all
  terminated sessions”. On `K`, snapshot the terminated session identifiers and
  show the snapshot count in the prompt; select No by default and leave all
  sessions unchanged on cancellation. Enter on Yes is the only destructive path;
  Enter on No, `n`, or Escape cancels. If there are no terminated sessions, do
  not enter a destructive confirmation; set the exact non-destructive feedback
  message `No terminated sessions to kill.` instead.
- `src/session.rs`: add a shared helper that snapshots terminated sessions and
  kills only the identifiers supplied by the picker, handling a session
  disappearing between the snapshot and each kill without aborting the remaining
  cleanup. A session that becomes terminated after the picker snapshot is not
  part of that operation. Surface an actionable error if a real tmux failure
  occurs, and refresh the picker after the operation.
- Update the shortcut text and add tests for no terminated rows, one and many
  terminated rows, mixed live/terminated inventories, safe cancellation,
  confirmation defaulting to No, the exact no-target feedback, and a race where
  one target disappears during cleanup.

Acceptance criteria:

- `K` never kills live sessions and never performs any kill before Yes.
- Yes removes every session identifier in the `K` snapshot, while cancellation
  removes none.
- No-target behavior is non-destructive and clearly reported.
- The picker inventory and selection are refreshed after cleanup.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-037 - publish to crates.io as `stay`

State: NEW

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

- TASK-039, TASK-040, TASK-041, TASK-042, TASK-043, TASK-044, TASK-045,
  TASK-046, TASK-047, and TASK-048 must all be `COMPLETED` before this task
  starts. This release must include the complete Issue 1 follow-up set.

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

- All ten Issue 1 follow-up tasks listed in Dependencies are `COMPLETED`.
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
