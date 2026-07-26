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

## TASK-023 - status markers as words: data and plain listing

State: NEW

Goal:

- Replace the single-letter status markers (`a`/`d`/`t`/`b`) with the words
  `attached`/`detached`/`terminated`/`broken`, flip the plain listing to
  name-first with the state as a bracketed suffix, and give the `terminated`
  marker the command's exit code + termination time (red for a non-zero exit).
  Implements the data layer + plain-listing half of TODO-003; the picker half is
  TASK-024. Signed off in `design_docs/mockups.html` §1, §5.

Dependencies:

- None new — builds on `list_sessions` / `render_session_inventory`
  (`src/tmux.rs`). Composes with TODO-004 (prior status before `-f`, shares the
  exit-code read) and TASK-022 (dead-pane auto-detach, reads the same
  `pane_dead*` fields on a different path); neither is required.

Scope:

- `src/tmux.rs`: extend `SessionRecord` (lines 23-27) with terminated state —
  add `terminated: bool`, `exit_code: Option<u8>`, `dead_time: Option<u64>`.
  Replace the `marker()` `char` (lines 32-38) with a method returning the status
  word, with priority terminated > attached > detached (a dead session shows
  `terminated` even while a client reviews it — the postmortem path from
  TASK-022): `terminated` → `"terminated"`, `attached` → `"attached"`, else
  `"detached"`. `broken` has no live row (a missing server is zero sessions, not
  a row) and needs no field.
- Data path: fold the pane state into ONE `list-panes -a` query that also
  carries the session fields, replacing the separate `list-sessions` call:
  `list-panes -a -F '#{session_name}:#{session_attached}:#{session_created}:#{pane_dead}:#{pane_dead_status}:#{pane_dead_time}'`
  (verified on tmux 3.6a: one row per pane; `session_attached`/`session_created`
  repeat per pane and match `list-sessions`; a live pane yields
  `name:<attached>:<created>:0::` with the last two fields EMPTY, a dead pane
  `name:<attached>:<created>:1:<status>:<epoch>` — the parser must treat an
  empty status/time field as `None`, not `0`). One query halves the picker's
  per-poll tmux cost (it polls `list_sessions` every 500 ms) and removes the
  cross-query race a second `list-panes` call would add. `list_sessions` parses
  each row into a `SessionRecord` and groups panes by `session_name`; the first
  pane's `session_attached`/`session_created` stand for the session (they are
  session-level, constant within a session).
- Multi-pane termination rule: a session is `terminated` only when it has NO
  live pane (every pane dead), NOT when any single pane is dead. A split session
  with one dead pane among live ones is still alive and must show
  `attached`/`detached`, not `terminated`. This composes with TASK-022:
  auto-detach fires when the active pane dies, but a multi-pane session with
  surviving panes is not terminated. For a terminated session the exit code and
  time come from the most-recently-dead pane (max `pane_dead_time`); stay's
  single-pane case is that one pane.
- Test-shim retarget (required by the one-query fold): the picker poll-failure
  test (`picker_retains_its_last_list_when_a_poll_fails`, TASK-014 R004) injects
  failure via the `attachment.rs` shim that intercepts the `list-sessions`
  command (keyed off `STAY_TEST_FAIL_LIST_FILE`). Once `list_sessions` no longer
  runs `list-sessions`, that shim stops triggering. Retarget the shim to
  intercept `list-panes` so the test still exercises a poll failure on the real
  code path, and keep its wait-for-initial-render discipline (R004).
- `render_session_inventory` (lines 43-51): rewrite the row from
  `<marker>\t<name>` to name-first — `<name> [<word>]`, the name padded by
  display width so the brackets line up, with a single space before the bracket.
  A `terminated` row carries ` [terminated exit=<code> @ <local time>]`;
  `attached`/`detached` rows show just ` [attached]` / ` [detached]`. Build each
  row from the `status_detail()` segments (below) so the suffix text is shared
  with the picker. The non-zero exit code is shown in ANSI red, but only when
  stdout is a TTY: `render_session_inventory` takes a `colour: bool` (or
  equivalent) and the `stay list` call site (`src/main.rs:78`) passes
  `std::io::stdout().is_terminal()`, so piped/redirected output stays
  monochrome. No in-repo v1 fixture pins the exact text, so under cleanroom this
  task defines it and captures the chosen form in a test.
- The termination time is shown as a human-readable LOCAL timestamp. The repo
  has no date crate today, so this adds a minimal time-formatting dependency
  (e.g. `time`); record it in `Cargo.lock` per the dependency rule. A no-dep
  fallback (UTC, or raw epoch) is acceptable only if chosen and documented.
- Share the suffix between the plain list and the picker (TASK-024) via one
  helper on `SessionRecord` (`status_detail()`) that returns structured segments
  — e.g. `Vec<SuffixSpan { text, emphasis: bool }>`, where `emphasis` marks the
  non-zero exit-code span. Each surface renders the segments with its own
  mechanism (ANSI for the list, ratatui `Style` for the picker), so the "what is
  red" decision lives in one place and neither surface re-parses the string to
  find the exit code.
- Tests: unit tests for the word priority (`terminated` over `attached` over
  `detached`) and for `render_session_inventory` rows (name-first, padded
  brackets, terminated exit+time, red only when `colour` is set); a tmux test
  driving `list_sessions` against a live and a dead-but-persisted session
  (`remain-on-exit`) asserting `terminated`/`exit_code`/`dead_time` populate and
  that both the `0::` and `1:<status>:<epoch>` pane rows parse. Update the
  existing assertions that break: the `marker()` checks at `src/tmux.rs:485`,
  `tests/tmux_inventory.rs:86` and `:144` (char → word), the `parse_session_row`
  3-field tests at `src/tmux.rs:476-491` (now 6-field), and the
  `render_session_inventory` callers at `src/main.rs:78` and
  `tests/tmux_inventory.rs:166`/`:174` (new `colour` argument). No real `$HOME`
  dependency.

Acceptance criteria:

- A session whose command has exited (under `remain-on-exit`) is reported
  `terminated` with its `pane_dead_status` as `exit_code` and `pane_dead_time`
  as `dead_time`; a running session reports not-terminated with `None` for both.
- The status word is `terminated` for a terminated session even when a client is
  attached to it (priority terminated > attached > detached); `attached` and
  `detached` otherwise. `broken` never appears as a row.
- The plain listing renders name-first `<name> [<word>]`; a `terminated` row
  carries `exit=<code> @ <local time>` inside the brackets, with a non-zero exit
  in ANSI red when stdout is a TTY (monochrome otherwise). `attached`/`detached`
  rows render as ` [attached]` / ` [detached]`.
- `status_detail()` returns structured segments shared by the plain list
  (TASK-023) and the picker (TASK-024), so the two surfaces render the same
  suffix.
- The termination time is shown as a local timestamp (or UTC/epoch if the no-dep
  fallback was chosen and documented).
- `list_sessions` merges `list-panes -a` pane state by session name; live-pane
  (`0::`) and dead-pane (`1:<status>:<epoch>`) rows both parse without error.
- A newly added time-formatting dependency (if any) is recorded in `Cargo.lock`.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-024 - status markers as words: picker rows

State: NEW

Goal:

- Show the word markers, name-first rows, and `terminated` exit code + time in
  the picker, unified with the plain listing (TASK-023); the focused row keeps
  its suffix instead of hiding it. Implements the picker half of TODO-003;
  builds on TASK-023's `SessionRecord` fields and `status_detail()`. Signed off
  in `design_docs/mockups.html` §2, §3, §4.

Dependencies:

- TASK-023 (needs the status-word method, `SessionRecord.terminated` /
  `exit_code` / `dead_time`, and the shared `status_detail()` formatter).

Scope:

- `src/picker/mod.rs`: in `session_row` (lines 909-923), drop the
  `selected`-hides-suffix branch (lines 911-912) so the focused row renders the
  same `<name> [<word> ...]` suffix as every other row — including a
  `terminated` row's exit code + time, the postmortem data the user reads the
  row for. Build the suffix from TASK-023's `status_detail()` segments, mapping
  each emphasized segment to a red ratatui `Span` so the picker and plain list
  agree. The red emphasis is suppressed on the focused row: a selected
  (`REVERSED`, lines 875-879) row renders the exit code plain, so the highlight
  is uniform reverse video and red shows only on unfocused rows. This turns
  `session_row` from a `String` into a multi-span `Line`.
- Width-aware: account for the richer suffix in `suffix_width`
  (`UnicodeWidthStr::width`, line 916) and truncate the name to fit; on a
  terminal too narrow for the full suffix, degrade by the documented rule — drop
  the time first, then the exit code, keeping the marker word longest; never
  panic or overflow (mirrors the mockup §4 narrowing).
- The status line stays one line with the full keybind set (`IDLE_STATUS`, line
  27), adopting the `·` separator from the mockups in place of the current
  double-space (update the const and its assertion at line 1379); it flows
  (wraps) only when the terminal is narrower than that line. `IDLE_STATUS` keeps
  its `c create` entry (TASK-026 keeps `c`).
- Tests: a unit test for `session_row` terminated formatting (name-first, suffix
  kept on the selected row, red exit span only when unfocused) and width
  truncation; a real-PTY picker test (via `script(1)` as today) that lists a
  live and a terminated session and asserts the terminated row shows the
  `terminated` word with the exit code, focused or not. Update the existing
  `session_row` test at `src/picker/mod.rs:1540-1548` (it asserts the selected
  row's suffix is dropped). Preserve the picker's keep-last-list-on-poll-failure
  and display-width padding guarantees (TASK-014). Note: TASK-026 also edits the
  list render (synthetic create row at index 0) — coordinate the
  `selected_index` offset if both land.

Acceptance criteria:

- The picker renders every row name-first as `<name> [<word> ...]`; a
  `terminated` row carries the exit code + time, with a non-zero exit in red on
  unfocused rows (the focused, reverse-video row renders it plain). A running
  session renders ` [attached]` / ` [detached]`.
- The focused (reverse-video) row shows its suffix like every other row — no gap
  under the highlight, including for `terminated` rows.
- The suffix width is accounted for so names pad/truncate correctly (no
  misalignment, mirroring the TASK-014 R002 display-width discipline).
- On a narrow terminal the row degrades by the documented rule (drop time, then
  exit code) and never panics or overflows.
- The picker still keeps the last list on a poll failure and pads by display
  width (TASK-014 guarantees preserved).
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-025 - content-sized, centered picker

State: NEW

Goal:

- On a full-screen terminal the picker no longer fills `frame.area()`; it
  shrink-wraps to its content and centers itself, leaving the rest of the
  terminal empty. The blue rounded border and grey interior are the actual
  ratatui rendering, not illustration. Signed off in `design_docs/mockups.html`
  §6.

Dependencies:

- None new — builds on the picker in `src/picker/mod.rs`. Independent of
  TASK-023/024 (can land before or after the word markers), though the mockup
  shows them together.

Scope:

- `src/picker/mod.rs`: in `render` (line 838), replace the full-screen
  `frame.area()` (line 839) with a content-sized `Rect` centered in the frame.
  Width = the widest line the box will hold (the longest session row, including
  TASK-024's suffix, or the status line — whichever is wider), capped at the
  terminal width; height = sessions + title border + status lines, capped at the
  terminal height — where "status lines" is the wrapped count, since the status
  flows (TASK-024) when the terminal is narrower than the one-line status.
  Center the `Rect` when the terminal is larger than the box.
- Style the existing `Block` (lines 840-843, already `Borders::ALL` +
  `BorderType::Rounded`) with a blue border `Style` and a background fill, and
  clear the frame to the terminal background colour before drawing so the area
  around the box is empty (the dark surround in the mockup). The `Layout`
  list/separator/status split (lines 851-861) is unchanged; it now splits the
  inner area of the smaller box.
- Edges: if sessions exceed terminal rows, height caps and overflow is hidden
  (today the picker already drops rows past the bottom, line 864); if the widest
  row exceeds terminal width, width caps and the TASK-024 narrowing rules apply.
- Tests: a render test asserting the box is content-sized and centered for a
  small session set on a large frame (the box is narrower/shorter than the frame
  and inset on both axes), and that it degrades to filling the frame when the
  frame is smaller than the content. Preserve existing render/selection
  behavior.

Acceptance criteria:

- On a frame larger than the content, the picker renders as a centered,
  content-sized box with empty space around it; on a frame smaller than the
  content, it fills the frame as today.
- The box shows the blue rounded border and grey interior as its actual styling;
  the surround is the terminal background.
- Existing picker behavior (selection, status, poll-failure keep-last-list,
  TASK-014) is preserved.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-026 - picker create row; `c` focuses and starts create

State: NEW

Goal:

- Add a permanent create row at the top of the session list, focused by default
  on open, and keep `c` as a create shortcut: `c` focuses the create row AND
  opens the inline name prompt immediately (one step, not focus-then-Enter).
  Creating a session always goes through the inline name prompt — there is never
  a default name. Signed off in `design_docs/mockups.html` §7.

Dependencies:

- None new — builds on the picker in `src/picker/mod.rs` (which already has a
  `Create` mode with an inline name prompt, lines 417 / 692-707). Composes with
  TASK-025 (the create row sits inside the content-sized box).

Scope:

- `src/picker/mod.rs`: add a synthetic create row as the first entry in the list
  (e.g. "create new session"), focused by default on open (`selected_index`
  starts at the create row). Arrow down past the create row to reach sessions.
- Two paths into create, both opening the existing `Create` inline name-prompt
  mode (lines 417 / 692-707), never a default name: (1) `Enter` on the focused
  create row opens the prompt; (2) `c` from anywhere in the idle list focuses
  the create row AND opens the prompt in the same keypress, so the user does not
  press Enter after `c`. On submit, an empty name is rejected by the existing
  session-name validation (`src/session_name.rs` rejects empty — keep it; stay
  never synthesizes a default name). `Enter` on a session row attaches as today.
- `c` in `handle_idle_key` (line 323) keeps entering `Create` mode (its original
  behavior, lines 358-364) but now also sets focus to the create row, so the
  highlighted create row is visible while the user types the name. `IDLE_STATUS`
  (line 27) and `EMPTY_STATUS` (line 28) keep their `c` entry; the empty-list
  case still opens with the create row focused.
- The create row is not a `SessionRecord`; it is a list entry the renderer
  special-cases, so it does not pollute `list_sessions` output or the plain
  listing. Note: TASK-024 also edits the list render — coordinate the
  `selected_index` offset (create row at 0 shifts session indices) if both land.
- Tests: the picker opens with the create row focused; `c` from a session row
  focuses the create row and enters the name-prompt mode in one step; `Enter` on
  the create row enters the name-prompt mode; an empty submitted name is
  rejected (no default name is ever created); the empty list still offers create
  via the focused create row. Preserve existing create/rename behavior.

Acceptance criteria:

- The picker opens with the create row at the top, focused.
- `c` from anywhere focuses the create row and opens the inline name prompt
  immediately (no second Enter); `Enter` on the create row also opens the
  prompt. Session rows are reached by arrowing down from the create row, and
  `Enter` on a session attaches.
- A session is never created with a default name: every create goes through the
  name prompt, and an empty name is rejected.
- The empty list still offers create (the focused create row).
- `IDLE_STATUS`/`EMPTY_STATUS` retain their `c` entry.
- `just qcheck` and `just mac-qcheck` both pass.
