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

### Task template

```markdown
## TASK-000 - short title

State: NEW

Goal:
- Describe the user-visible or maintainer-visible outcome.

Dependencies:
- List the tasks that must reach `COMPLETED` before this task may begin.

Scope:
- List files, modules, or documentation expected to change.

Acceptance criteria:
- State the behavior or documentation that must be true when complete.
- State the tests or quiet Just recipe that must pass.
```

Every task must be fully scoped before implementation begins. Its Goal, Scope,
Acceptance criteria, dependencies, and verification must completely describe
what done means; they must not be filled in incrementally during implementation.
Tasks should be small enough for one implementer/reviewer conversation pair and
must not leave investigation or design choices for the implementer.

## Task state and sequencing

Every task has a stable ID and one valid state from the state rules in
`design_docs/agent_workflow.md`. Once published, a task ID must not be
renumbered, reused for different work, or rewritten because tasks were reordered
or removed. If the plan changes, move or delete the task entry while preserving
all surviving IDs.

Describe dependencies and ordering explicitly in the plan. State which task must
land first, which tasks may proceed independently, and which task should land
last because it documents or consolidates the completed shape. The next task
must not start until its dependencies have reached `COMPLETED`; an earlier task
that establishes a pattern must be correct before dependent work reuses it.

For each active task, its `State:` field must match the state transition being
performed. A review document or informal conclusion does not advance a task; the
plan state must be updated explicitly, and the shared commit and review record
must reflect the transition as specified in `design_docs/agent_workflow.md`.

## Task-specific documentation and verification

Each task's acceptance criteria must name the checks that establish completion
and any limitations on verification. Every implementation patch must pass both
`just qcheck` and `just mac-qcheck`; these are the gates for marking the patch
`IMPLEMENTED` and for Rufus to mark it `COMPLETED`. A task may not claim
implementation or review completion when the macOS gate was not run or did not
pass. Use the smallest relevant checks for documentation-only work only when the
task explicitly scopes the change as documentation-only, as specified in
`design_docs/agent_workflow.md`.

Every task also updates the plan's own state and any pending-work or decision
register used to track rollout. Search for stale comments, documentation,
configuration descriptions, or references to files and behavior changed by the
task, and update them as part of the same task.

## Picker milestone

TASK-014, TASK-015, and TASK-016 deliver the ratatui interactive picker
described in `design_docs/stay.md`'s "Interactive picker" section and its
"Suggested milestones" entry 5. They must land in ID order: TASK-015's actions
run inside the loop TASK-014 builds, and TASK-016's inline edit-name reuses
TASK-015's text-entry widget. All three depend on TASK-013 being `COMPLETED`.

Scope note / deliberate deviation from `stay.md`: `stay.md` lists the picker's
data source as `list-sessions` + `list-panes -a` (for dead-session status) and
includes `v` (view-only, `-r`) and `l` (low-priority, `-l`) in the v1 keybinding
set. Milestone reordering put the picker ahead of "Terminated sessions" and
"Attach-mode flags", so TASK-014 scopes the listing to `list-sessions` data only
(name/attached, as `SessionRecord` already provides) and TASK-016 guards `v`/`l`
as "not yet implemented" rather than pulling the attach-mode-flag or
pane-dead-parsing milestones forward. Dead-session display and `v`/`l` behavior
land when those milestones are implemented; this note is the record that the
narrowing was deliberate, not an oversight.

Rendering spec: `stay.md` leaves "exact widget choice/styling" open as a looser
vision-doc statement, but this plan's own rules require every task to be fully
scoped with no design choice left to the implementer. The exact rendering is
therefore pinned here, illustrated in `design_docs/mockups.html`, and
TASK-014/015/016 below must match it exactly: a single outer `Block` with a
rounded border (`BorderType::Rounded`), titled `" stay "`; row order is exactly
`Tmux::list_sessions()`'s existing return order (already sorted by name, then
creation time) — no independent picker-side sort; each row shows the session
name left-aligned and its `SessionRecord::marker()` value right-aligned as
`[a]`/`[d]`; the selected row is rendered in reversed video
(foreground/background swapped) across the full row width, with no marker glyph;
↑/↓ clamp at the first/last row (no wrap-around); the list is polled every 500ms
unconditionally (including while a prompt is open); a one-line status/help area
sits below the list, inside the same outer border, separated by a horizontal
rule. Exact status-line text is given in each task below and in
`design_docs/mockups.html`; the mockups show the status line as it reads once
all three tasks are `COMPLETED` — each task's own scope states what its status
line reads before the next task lands, since each task reaches `COMPLETED` and
lands on `main` on its own (as TASK-010 through TASK-013 did) and must not
advertise a keybinding it does not yet implement (the same rule TASK-010 itself
established for CLI flags).

Selection identity: the picker's selection is tracked by **session name**, not
row index. Because the list re-polls on a fixed timer independently of user
input, an externally created/killed session can reorder or shrink the
name-sorted list between polls; tracking by index would silently move the
highlight (and any open confirm/edit-name prompt) onto a different session than
the one the user was looking at. If the selected name is absent from a freshly
polled list (killed elsewhere, or a first poll before any selection exists),
selection clears to none, with the same behavior as the zero-sessions case
below.

## TASK-014 - picker skeleton: listing, navigation, and attach handoff

State: COMPLETED

Goal:

- When `stay` is invoked with no session name and stdout is a terminal, show a
  full-screen ratatui session list instead of today's plain a/d listing.
  Selecting a session and pressing Enter attaches through the existing relay
  exactly as `stay <name>` does, with no typed-ahead input lost across the
  handoff. Non-terminal invocation is unchanged.

Dependencies:

- TASK-013 must be `COMPLETED`.

Scope:

- `Cargo.toml`: bump `rust-version` from `1.85` to `1.88` (verified via
  `cargo metadata`: today's `1.85` floor is incidental — it's `clap` 4.6.4's
  transitive MSRV, not a deliberate pin or a v1 holdover; every other dependency
  needs far less, and `1.88` is still strictly above `clap`'s floor). Add
  `ratatui = "0.30"` and `crossterm = "0.29"` as dependencies — the latest
  release of each, confirmed compatible with `rust-version = "1.88"` via
  `cargo add --dry-run`.
- `justfile`'s `msrv` recipe: change both the `rustup toolchain install` version
  check and the `cargo +1.85 check` invocation from `1.85` to `1.88` to match.
  This recipe is not part of `qcheck`/`mac-qcheck` (this task's own gates), but
  it is part of plain `just check` (what the pre-push hook runs) and CI's
  separate "Check MSRV" step; leaving it at `1.85` would fail both once
  `Cargo.lock` has a ratatui version the old 1.85 toolchain cannot compile.
- New `src/picker/` module (at minimum a `mod.rs`), wired into `lib.rs`.
- `src/main.rs`: when `cli.session_name` is `None`, check whether stdout is a
  terminal; if so, call into `picker::run` instead of the current
  `render_session_inventory` branch. Keep the existing plain-listing branch,
  unconditionally, for the non-terminal case — no change to that output.
- `src/picker/`: alternate-screen `ratatui::Terminal` setup/teardown on every
  exit path (attach, Esc/`q` cancel, error, panic — mirror `relay.rs`'s
  `Drop`-guard-plus-panic-hook pattern for terminal restoration, since a
  `panic = "abort"` release build never runs `Drop`); the rounded-border list
  and status area from the Picker milestone's rendering spec, built from
  `tmux.list_sessions()`, polled every 500ms unconditionally; ↑/↓ moves the
  selection and clamps at the first/last row; Enter on a valid selection tears
  down the alternate screen and calls `session::attach_session` for the selected
  name; Esc or `q` exits with no tmux call and exit code 0. Selection is tracked
  by session name per the milestone's "Selection identity" note, not row index.
  At this task, only navigation and quitting are wired: the idle status line
  reads exactly `↑/↓ select  Enter attach  Esc quit` — it must not mention
  `c`/`k`/`r`/`e`/`v`/`l`, since none of them do anything yet.
- Zero-sessions case: the list body renders exactly `(no sessions)`, centered;
  the status line reads exactly `Esc quit` (no `c` yet — TASK-015 adds it);
  ↑/↓/Enter are no-ops with no selection.
- Poll failure: if a poll of `tmux.list_sessions()` fails after the picker is
  already open, the status line shows the error string verbatim, the last-known
  list stays displayed unchanged, and polling continues on the same interval; a
  poll failure never exits the picker or panics.
- Typed-ahead handoff: any stdin bytes the picker's input loop reads at or after
  the Enter keypress that are not part of that keypress itself must be forwarded
  into the relay's input, not dropped.
- Reference: `design_docs/mockups.html`'s "Idle, sessions present" and "Zero
  sessions" frames show the border/list/highlight rendering and the
  *completed-milestone* status-line text; this task's own status line is the
  reduced text above until TASK-015/016 land.

Acceptance criteria:

- With one or more sessions present and stdout a tty, `stay` (no args) opens the
  picker; ↑/↓ moves the highlighted row; Enter on a highlighted session attaches
  (asserted the same way existing attach integration tests assert attach
  behavior); Esc and `q` exit 0 with the session list unchanged.
- With zero sessions, the picker opens with an empty list and no crash; Enter is
  a no-op.
- Pressing `c`, `k`, `r`, `e`, `v`, or `l` in this task's build has no
  observable effect (no tmux call, no status-line change beyond what an
  unrecognized key would do) and is not mentioned in the status line.
- A test proves that when the selected session is killed by a call outside the
  picker (simulating another terminal), the next poll clears the selection
  instead of silently highlighting whatever session now occupies that row index.
- A test proves that when `tmux.list_sessions()` fails (for example the test
  namespace's server is killed), the picker shows the error in the status line,
  keeps showing the last-known list, and keeps running rather than exiting or
  panicking.
- With stdout not a tty, `stay` (no args) output is byte-for-byte identical to
  the current plain a/d listing; no picker code path runs.
- A test proves stdin bytes typed immediately before the relay takes over
  (during the picker→attach transition) reach the attached session instead of
  being silently dropped.
- The outer terminal's mode/screen content after Esc/`q`/attach-then-detach is
  restored exactly as it was before the picker opened, including after a
  simulated panic inside the picker loop.
- `just qcheck` and `just mac-qcheck` pass, and `just qcheck` passes twice
  consecutively after the final amend with no further file changes.

## TASK-015 - picker actions: create, kill, recreate

State: COMPLETED

Goal:

- Wire the mutating v1 keybindings that don't need new tmux plumbing into the
  picker built in TASK-014: `c` create, `k` kill (with `y`/`N` confirm), `r`
  recreate (matching `-f`/`force_recreate` semantics).

Dependencies:

- TASK-014 must be `COMPLETED`.

Scope:

- `src/picker/`: an inline text-entry input mode for `c`, replacing the status
  line with `New session name: ` followed by the typed text and a trailing `█`
  cursor; on Enter, validates with the same `parse_session_name` rules as the
  CLI, then creates and attaches via `session::create_session` +
  `session::attach_session`; Esc cancels back to the idle list with no mutation.
  `c` works with no session selected (zero-sessions case included). A confirm
  sub-state for `k`, replacing the status line with exactly
  `Kill session "<name>"? y/N` (`<name>` is the selected session's name at the
  moment `k` was pressed); `y` confirms and calls `session::kill_session` on
  that captured name (not a re-read of "whatever is currently selected"), any
  other key or Esc cancels back to the list with no mutation; if that name has
  disappeared by the time `y` is pressed (killed elsewhere in the meantime), the
  resulting tmux error shows verbatim in the status line like any other failed
  action. `r` calls `session::force_recreate_session` on the selected session
  with no confirm (matching the CLI's own unconfirmed `-f`). `k` and `r` are
  no-ops with no session selected. The session list re-polls immediately after
  any mutating action. A failed action (for example a name collision on `c`)
  shows the underlying error string verbatim in the status line rather than
  exiting or panicking. The idle status line grows to
  `↑/↓ select  Enter attach  c create  k kill  r recreate  Esc quit`; the
  zero-sessions status line grows to `c create   Esc quit`.
- Reference: `design_docs/mockups.html`'s "Create (c)" and "Kill confirm (k)"
  frames show the border/list/prompt rendering and the *completed-milestone*
  status-line text; this task's own idle/zero-sessions status line is the text
  above until TASK-016 lands.

Acceptance criteria:

- `c` prompts for a name, validates it with the existing session-name rules, and
  creates+attaches on confirmation; an invalid or colliding name shows an error
  in the picker and returns to the entry prompt or list without mutating tmux;
  `c` works identically with zero sessions present.
- `k` requires `y` before killing; any other input cancels with no tmux call;
  after a confirmed kill the list no longer shows the session; with no
  selection, `k` is a no-op.
- A test proves that killing the selected session (outside the picker) while its
  `y/N` confirm is open, then pressing `y`, surfaces the resulting tmux error in
  the status line rather than killing a different, now-relocated session at the
  same row.
- `r` recreates the selected session (existing session gone, new one created)
  with the same startup-failure behavior as CLI `-f`; with no selection, `r` is
  a no-op.
- `just qcheck` and `just mac-qcheck` pass, and `just qcheck` passes twice
  consecutively after the final amend with no further file changes.

## TASK-016 - picker edit name, and honest guards for view-only/low-priority

State: COMPLETED

Goal:

- Add `e` edit-name-in-place via a new `tmux rename-session` wrapper, and make
  the `v` (view-only) and `l` (low-priority) picker keys report "not yet
  implemented" in the picker's status line instead of silently doing nothing or
  attaching normally, since the underlying attach-mode-flag behavior is not yet
  built (see the Picker milestone's scope note above).

Dependencies:

- TASK-015 must be `COMPLETED`.

Scope:

- `src/tmux.rs`: add
  `rename_session(&self, session_name: &str, new_name: &str) -> Result<(), String>`
  calling `tmux rename-session`, validated the same way `create_session`'s name
  is validated.
- `src/picker/`: `e` reuses TASK-015's text-entry widget, replacing the status
  line with `Edit name "<name>" to: ` followed by the typed text and a trailing
  `█` cursor; on Enter, calls `rename_session` and re-polls the list; an invalid
  or colliding new name shows the underlying error string verbatim in the status
  line without renaming; Esc cancels with no mutation. `v` and `l`, on a
  selected session, replace the status line with exactly
  `v: not yet implemented` or `l: not yet implemented` and make no tmux call,
  mirroring `main.rs`'s existing CLI guard for the same flags (TASK-010); the
  next keypress returns the status line to the idle text. `e`, `v`, and `l` are
  all no-ops with no session selected. The idle status line reaches its final,
  complete form:
  `↑/↓ select  Enter attach  c create  k kill r recreate  e edit name  v view-only  l low-priority  Esc quit`;
  the zero-sessions status line is unchanged from TASK-015 (`e`/`v`/`l` require
  a selection that cannot exist with zero sessions).
- Reference: `design_docs/mockups.html`'s "Edit name (e)" and "View-only /
  low-priority guard (v / l)" frames are the exact rendering to match, and its
  "Idle, sessions present" frame is now this task's own idle status line too.

Acceptance criteria:

- `e` edits the selected session's name to a valid new name; an invalid or
  colliding name leaves the original session untouched and shows an error; with
  no selection, `e` is a no-op.
- `v` and `l` make no tmux call and leave the list/selection unchanged; the
  status line names the key and says "not yet implemented"; with no selection,
  `v` and `l` are no-ops.
- The idle status line matches `design_docs/mockups.html`'s "Idle, sessions
  present" frame exactly, byte for byte.
- `just qcheck` and `just mac-qcheck` pass, and `just qcheck` passes twice
  consecutively after the final amend with no further file changes.

## TASK-017 - flush attach prompts and cover interactive output

State: COMPLETED

Goal:

- Make shell prompts and other partial-line output visible immediately after
  attach, including the picker-to-attach handoff, and add end-to-end coverage
  for the interactive behavior.

Dependencies:

- TASK-016 must be `COMPLETED`.

Scope:

- `src/relay.rs`: preserve the existing PTY relay and input-interception
  behavior while ensuring every output chunk read from the attach PTY is flushed
  to stdout before the relay continues; keep the forwarding helper small and
  independently testable.
- `tests/attachment.rs`: add a Unix real-PTY integration test that starts a
  session running a shell, asserts the initial prompt is observable before
  sending a command, asserts the command's output and following prompt are
  observable, then detaches and cleans up the isolated tmux server.
- `design_docs/implementation_plan.md`: track this task's state through the
  required implementer handoff transition.

Acceptance criteria:

- A prompt with no trailing newline is visible without waiting for Enter or a
  later newline.
- Output forwarding remains byte-preserving and the configured detach key still
  detaches normally.
- The real-PTY integration test fails against the pre-fix buffering behavior and
  passes with the fix; the unit test verifies a partial line is flushed before
  the writer is dropped.
- `just qcheck` and `just mac-qcheck` pass, and `just qcheck` passes twice
  consecutively after the final amend with no further file changes.
