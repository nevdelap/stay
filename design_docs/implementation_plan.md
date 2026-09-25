# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

## TASK-116 - Wrap picker session-list navigation

State: COMPLETED

Goal:

- Make Up and Down navigation in the idle picker continuous, so moving past
  either end wraps to the opposite end of the logical list.

Dependencies:

- None.

Scope:

- `src/picker/mod.rs` picker logical-row selection and scrolling.
- Picker unit tests covering empty lists, live/saved/terminated rows, and the
  Create New Session row.

Acceptance criteria:

- With no sessions, the Create New Session row remains selected and wrapping
  does not create an invalid session selection.
- With sessions present, the logical order is Create New Session, followed by
  every session row in display order; Up from Create New Session selects the
  last session and Down from the last session selects Create New Session.
- Wrapping keeps the selected row visible, preserves the existing viewport
  scrolling behavior, and clears pending attach modifiers in the same way as
  current navigation.
- Home, End, Page Up, Page Down, filtering, attach, recreate, kill, and rename
  behavior are unchanged except where they consume the wrapped selection.
- Rust tests and the exact `just qcheck` and `just mac-qcheck` gates pass.

## TASK-117 - Add ergonomic picker control-key navigation

State: COMPLETED

Goal:

- Make common control-key navigation and editing combinations work reliably in
  the picker session list and the single-line rename editor on the supported
  Unix platforms (Linux and macOS).

Dependencies:

- None.

Scope:

- `src/picker/mod.rs` Unix input decoding, picker-mode dispatch, session-list
  navigation, and rename-name editing. The existing non-Unix crossterm reader is
  outside this task's supported-platform scope.
- Picker unit tests for raw control bytes, supported modified-arrow escape
  sequences, list navigation, and cursor/editing behavior on Linux and macOS.
- User-facing picker key-help text or documentation if the displayed controls
  change.

Acceptance criteria:

- In the session list, Ctrl+Left, Ctrl+Right, Ctrl+Up, and Ctrl+Down perform
  Home, End, Page Up, and Page Down respectively, including at list boundaries
  and with the Create New Session row.
- In the session list, Ctrl+A and Ctrl+E are aliases for Home and End, while
  Ctrl+P and Ctrl+N are aliases for Up and Down.
- In the rename editor, Ctrl+Left and Ctrl+Right move to the beginning and end
  of the name without inserting or deleting text. Ctrl+Up and Ctrl+Down are
  explicit no-ops in the single-line editor: the mode, text, and cursor remain
  unchanged, and the key cannot submit the edit.
- The conventional single-line editing controls remain available and are tested:
  Ctrl+A/E for Home/End, Ctrl+B/F for Left/Right, Ctrl+H/D for Backspace/Delete,
  Ctrl+K/U for delete-to-end/delete-to-start, and Ctrl+W for
  delete-previous-word.
- The supported Unix modified-arrow protocol is CSI `1;5A`, `1;5B`, `1;5C`, and
  `1;5D` for Ctrl+Up, Ctrl+Down, Ctrl+Right, and Ctrl+Left; the equivalent CSI
  `5A`, `5B`, `5C`, and `5D` forms are accepted when a terminal omits the
  default cursor parameter. Unknown or truncated CSI sequences produce
  `PickerKey::Other`, do not submit or edit anything, and do not consume bytes
  after the sequence candidate; the next ordinary byte remains available to the
  next read.
- Raw control aliases take precedence only while the picker is active. The
  configured detach and copy-mode bytes retain their existing meaning after
  handoff to the attach relay and are not changed by picker aliases.
- Existing configured detach and copy-mode controls are not changed.
- Rust tests and the exact `just qcheck` and `just mac-qcheck` gates pass.

## TASK-118 - Confirm recreate-and-attach for saved sessions

State: COMPLETED

Goal:

- Let Enter on a saved-only session explain that the live tmux session is gone
  and offer to recreate it and attach in one deliberate flow.

Dependencies:

- None.

Scope:

- `src/picker/mod.rs` saved-only Enter handling, confirmation state, recreate
  flow, attach handoff, status/error feedback, and pending attach modifiers.
- Picker and PTY attachment tests covering idle and filter-mode entry,
  confirmation, refusal, successful recreation, attach failure, typed-ahead
  input, and saved-definition durability.
- Picker help/status text or `README.md`/`docs/stay.1` if the interaction is
  documented there.

Acceptance criteria:

- Enter on a `saved` row opens an explicit Yes/No confirmation explaining that
  the session is saved but not running and that Yes will recreate and attach; it
  does not attempt to attach to the missing tmux session first. This applies
  both to direct idle-list selection and to a published filter result; Enter
  while a filter result is still pending remains a no-op.
- No leaves the saved row intact, creates no tmux session, and returns to the
  originating picker mode with the saved row selected and without a spurious
  attach error. The pending read-only and low-priority choices remain available.
- Yes recreates the session from its saved definition and then hands off to the
  normal attach relay automatically, carrying the pending read-only and
  low-priority modifiers if they were selected.
- The confirmation consumes only its own control keys. Bytes typed ahead while
  the confirmation is displayed are not sent to a missing session; after a
  successful recreation, residual input is passed through the normal attach
  handoff, and refusing the action leaves no residual input to execute.
- Recreate or attach failure leaves an actionable error in the picker and never
  reports success or loses the saved definition; existing store rollback and
  durability semantics remain intact. If recreation succeeds but attach fails,
  the picker returns with the now-live row selected and explains that the
  session was recreated but not attached; if recreation fails, the saved-only
  row remains selected and no live row is claimed.
- The existing `r` action remains the explicit recreate-without-attach path.
- Rust tests and the exact `just qcheck` and `just mac-qcheck` gates pass.

## TASK-119 - Preserve attached clients across session rename

State: COMPLETED

Goal:

- Rename a live session without disconnecting or otherwise booting clients that
  are already attached to it.

Dependencies:

- None.

Scope:

- `src/picker/mod.rs` rename action and error/rollback handling.
- `src/tmux.rs` session/client identity operations and `src/relay.rs` detach,
  polling, and logging bookkeeping needed to survive a session-name change.
- Real-tmux tests in `tests/tmux_inventory.rs` and `tests/attachment.rs` with at
  least two attached clients and an active Stay relay, plus picker/store rename
  coverage.

Acceptance criteria:

- Renaming a live session leaves every pre-existing client attached to the same
  tmux session under the new name; client count, client attachment state, and
  pane process are unchanged.
- The relay uses its stable attach-client PID to resolve the current tmux client
  target and session name globally, rather than scoping lookup to the original
  session name. A rename refreshes that session name before pane polling,
  logging, automatic detach, and explicit detach; a transient lookup miss is
  retried without detaching or restarting the relay.
- A real-tmux test renames the session while the relay is actively attached and
  proves the relay remains attached until its own detach action, then detaches
  only that client while another client remains attached.
- A rename collision or tmux failure leaves both the live session name and the
  saved definition consistent, with the existing rollback/error visibility.
- A successful rename updates the saved definition's name while preserving its
  creation time, working directory, and effective command for both a live
  persisted session and a live session whose definition was reconstructed from
  runtime metadata.
- Saved-only rename behavior remains supported and does not invoke a tmux client
  operation.
- Rust tests and the exact `just qcheck` and `just mac-qcheck` gates pass.
