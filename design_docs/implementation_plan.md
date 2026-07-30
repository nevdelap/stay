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

## TASK-042 - indicate view-only and low-priority attachments

State: COMPLETED

Goal:

- Give users a visible indication when the attached tmux client is view-only,
  low-priority, or both, while preserving user-owned tmux configuration.

Dependencies:

- TASK-029 (`COMPLETED`) — it supplies the working read-only and ignore-size
  client flags used by this task.

Scope:

- `src/session.rs`: extend the built-in status-left/status-right setup used when
  no user `~/.tmux.conf` exists so tmux's client formats expose the active
  attachment flags. Use tmux's documented `client_readonly` format for view-only
  and an exact `ignore-size` member match against `client_flags` for
  low-priority; do not infer either state from stay's process-local arguments
  after attach. The built-in status format must expand these conditions per
  attached client, not once when the session is created.
- Render `(view only)` for view-only alone, `(low priority)` for low-priority
  alone, and `(view only / low priority)` for both. A normal attachment must
  show neither label. Keep the existing session name, current directory, and
  stay version status content intact.
- Do not overwrite or alter status-bar settings when a user tmux config exists,
  matching the current built-in-settings contract. Update
  `design_docs/stay.html` to state that custom status configuration can
  therefore omit these indicators.
- Add live tmux/integration coverage for normal, view-only, low-priority, and
  combined clients, plus a regression proving user-configured status settings
  are not replaced. The coverage must exercise both alternate-screen and
  forced-main-screen attachment preferences.

Acceptance criteria:

- A built-in-config view-only client visibly contains `(view only)` in its
  status bar.
- A built-in-config low-priority client visibly contains `(low priority)`.
- A client with both flags visibly contains exactly
  `(view only / low priority)`; an ordinary client contains neither label.
- A user-provided tmux config is left untouched.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-050 - add explicit create-and-attach support

State: COMPLETED

Goal:

- Clarify and enforce that view-only and low-priority are tmux client modifiers
  available only when an attach operation creates or configures a client, never
  when creating a detached session, while adding an explicit create-and-attach
  path and locking down the existing per-client status behavior.

Dependencies:

- TASK-042 (`COMPLETED`) — it defines the per-client status indicators.
- TASK-043 (`COMPLETED`) — it supplies the picker modifier state and combined
  attach path.
- TASK-049 (`COMPLETED`) — it keeps pending picker detail attached to the
  selected existing-session row.

Scope:

- Keep `stay create NAME [COMMAND...]` detached and unchanged. Add `-a/--attach`
  to explicitly create the session and then attach a client. The attach path
  must reuse the existing attach mechanism and preserve the created command,
  cwd, and exit-status behavior. CLI create-and-attach has no picker
  residual-input handoff; it begins with the CLI's ordinary stdin.
- Make `-r/--read-only` and `-L/--low-priority` valid on `create` only when
  `-a/--attach` is present. Reject each modifier and their combination on a
  detached `stay create`, rather than interpreting them as properties of the
  session, pane, shell, or command. Cover the forms with and without a trailing
  command.
- Add CLI parser/help and command-dispatch coverage for plain create,
  create-and-attach, and create-and-attach with each modifier combination.
- Define and test option placement around trailing command words. Preserve the
  existing create parser behavior that accepts create options before or after
  those words, including `--attach`, `--read-only`, and `--low-priority`.
- Define `--` handling so option-like command arguments remain valid command
  words, for example `stay create NAME -- -r`; test that the command receives
  `-r` rather than the create parser treating it as an attachment modifier.
- Define and test `--force-recreate --attach`: recreate the session first, then
  attach using the requested client flags and return the attached command's exit
  status.
- Define the create-and-attach failure boundary: if creation succeeds but the
  subsequent attach fails, leave the created session in place and return the
  attach error or exit status without attempting an implicit rollback.
- In the picker, keep `v` and `l` inert when the synthetic create row is
  selected: they must not set pending state. After creating a session from that
  row, the subsequent picker attachment remains an ordinary attach with neither
  modifier.
- Preserve the existing four attachment outcomes for existing session rows,
  including the combined `read-only,ignore-size` path and the status indicators
  for attached clients. Add picker and integration regressions for the
  create-row behavior without introducing a second attach mechanism.
- Verify live status labels for existing-session attachment from both the CLI
  and picker where applicable: plain attach shows no modifier label, `read-only`
  shows `(view only)`, `ignore-size` shows `(low priority)`, and both flags show
  `(view only / low priority)`. Inspect tmux's per-client state, not stay's
  process-local arguments. If labels are absent during attach, diagnose and
  repair that TASK-042 status/lifecycle defect separately from create semantics;
  adding modifiers to `stay create` is not a fix.
- Ensure the built-in status settings are applied for an attachment to a
  pre-existing session when no user tmux configuration is present, while leaving
  user-configured status settings untouched. Cover this lifecycle path in the
  live tests.
- Document that TASK-042's indicators describe attached tmux clients, that
  modifiers on `create` require `--attach`, and that custom tmux configuration
  may intentionally omit the indicators. Do not add a second tmux attach
  mechanism.

Acceptance criteria:

- Plain `stay create NAME` and `stay create NAME COMMAND...` behave as before
  and create detached sessions.
- `stay create NAME --attach` creates the session and attaches a normal client;
  the command, cwd, and exit-status behavior remain correct, with ordinary CLI
  stdin rather than picker residual input.
- `stay create NAME -r`, `stay create NAME -L`, and the combined forms are
  rejected without `--attach`, while `stay create NAME --attach -r`,
  `--attach -L`, and `--attach -r -L` attach with the corresponding client
  flags.
- Create options work in the defined positions before or after trailing command
  words; `stay create NAME -- -r` passes `-r` to the command, and
  `--force-recreate --attach` recreates before attaching and returns the
  attached command's exit status.
- If create-and-attach creation succeeds but attach fails, the session remains
  available and the command returns the attach error or exit status.
- `v` and `l` do nothing and leave no pending state on the synthetic create row;
  picker creation followed by attachment continues to use neither modifier.
- Existing-session picker attachment still supports plain, view-only,
  low-priority, and combined attachment, with the combined tmux flags intact and
  the exact live status labels verified for CLI and picker paths.
- Status labels reflect actual tmux client flags, and any missing-label defect
  is fixed without adding modifiers to detached creation or changing the
  custom-status configuration contract; attaching to a pre-existing session
  applies built-in settings when appropriate.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-051 - detach only the requesting tmux client

State: NEW

Goal:

- Ensure detaching one `stay` attachment leaves every other client attached to
  the same tmux session running normally.

Dependencies:

- TASK-050 (`COMPLETED`) — it supplies the current relay-owned attach child and
  the CLI and picker attachment paths that must share this behavior.

Scope:

- `src/tmux.rs`: replace the session-scoped detach operation used by the relay
  with a client-scoped operation. Resolve the relay's attach child PID against
  tmux's `#{client_pid}` for the target session, then invoke `detach-client -t`
  for that exact client. Use a stable, unambiguous format delimiter and pass the
  resolved client target as a separate argument. If the requesting client cannot
  be resolved, return an actionable error; never fall back to
  `detach-client -s`, which would detach unrelated clients.
- `src/relay.rs`: thread the attach child's identity through the configured
  detach-key path, SIGTERM path, and pane-death automatic-detach path. Preserve
  the existing SIGTERM fallback that stops the attach child when tmux cannot
  detach it, and preserve pane exit-status propagation and terminal cleanup.
- Add live PTY/integration coverage that starts two independent `stay attach`
  clients against one session, detaches only the first client with the
  configured detach key, and proves the second client remains attached and
  usable. Add wrapper/relay coverage where the attach PID has no matching
  `#{client_pid}`: return an actionable error and issue no `detach-client`
  command, including no session-scoped fallback. Keep existing single-client,
  SIGTERM, automatic-detach, and picker attachment coverage passing.
- Update any tmux-wrapper or relay comments and test helpers that currently
  describe detachment as session-scoped.

Acceptance criteria:

- Detaching one attached `stay` client leaves all other clients attached to the
  same session.
- Manual detach, SIGTERM detach, and automatic pane-death detach target only the
  relay that initiated the operation.
- A missing client identity fails safely with an actionable error and does not
  issue any detach command; a resolved identity never targets a different client
  or the whole session.
- The existing SIGTERM hard fallback, terminal restoration, pane exit-status
  handling, and picker/CLI attach behavior remain intact.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-044 - edit the existing picker name in place

State: NEW

Goal:

- Make picker `e` editing start with the selected session name instead of an
  empty input that forces the user to retype the whole name.

Dependencies:

- None.

Scope:

- `src/picker/mod.rs`: seed edit mode with the selected name and maintain an
  insertion cursor at the end of the name. The cursor is always between Unicode
  scalar values; Left and Right move it one scalar at a time and clamp at the
  beginning and end, Backspace deletes the scalar immediately before it, and a
  typed character is inserted at it. Keep Escape as cancel and Enter as the
  existing tmux rename operation. Render the edited value with a single `█`
  caret at the cursor, without making the original name look like a separate
  field that will accidentally be retained.
- Reuse `parse_session_name` and the shared name validation for the final value.
  Empty names, disallowed characters, and duplicate tmux names keep the existing
  error behavior and do not rename the session.
- Add unit tests for editing the end and middle of a name, cursor movement,
  beginning/end clamping, Unicode-scalar deletion, cancellation, successful
  rename, and validation failure.

Acceptance criteria:

- Pressing `e` on `my-session` opens an editor containing `my-session`, with the
  cursor at the end, and the user can change only the needed characters.
- Escape leaves the original session name unchanged.
- Enter renames to the edited value and refreshes/selects the renamed row.
- Invalid or conflicting edited names leave the original session intact and show
  an actionable error.
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
