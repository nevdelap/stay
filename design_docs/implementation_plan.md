# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

## TASK-116 - Wrap picker session-list navigation

State: NEW

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

State: NEW

Goal:

- Make common control-key navigation and editing combinations work reliably in
  the picker session list and the single-line rename editor on Unix and non-Unix
  input paths.

Dependencies:

- None.

Scope:

- `src/picker/mod.rs` input decoding, picker-mode dispatch, session-list
  navigation, and rename-name editing.
- Picker unit tests for raw control bytes, modified arrow escape sequences,
  platform event decoding, list navigation, and cursor/editing behavior.
- User-facing picker key-help text or documentation if the displayed controls
  change.

Acceptance criteria:

- In the session list, Ctrl+Left, Ctrl+Right, Ctrl+Up, and Ctrl+Down perform
  Home, End, Page Up, and Page Down respectively, including at list boundaries
  and with the Create New Session row.
- In the session list, Ctrl+A and Ctrl+E are aliases for Home and End, while
  Ctrl+P and Ctrl+N are aliases for Up and Down.
- In the rename editor, Ctrl+Left and Ctrl+Right move to the beginning and end
  of the name without inserting or deleting text. Ctrl+Up and Ctrl+Down do not
  corrupt or submit the single-line editor; page navigation remains a list
  operation rather than an invented text-field behavior.
- The conventional single-line editing controls remain available and are tested:
  Ctrl+A/E for Home/End, Ctrl+B/F for Left/Right, Ctrl+H/D for Backspace/Delete,
  Ctrl+K/U for delete-to-end/delete-to-start, and Ctrl+W for
  delete-previous-word.
- Modified arrow sequences are decoded without swallowing the following input,
  and the Unix byte-reader and non-Unix crossterm reader expose equivalent
  `PickerKey` behavior.
- Existing configured detach and copy-mode controls are not changed.
- Rust tests and the exact `just qcheck` and `just mac-qcheck` gates pass.

## TASK-118 - Confirm recreate-and-attach for saved sessions

State: NEW

Goal:

- Let Enter on a saved-only session explain that the live tmux session is gone
  and offer to recreate it and attach in one deliberate flow.

Dependencies:

- None.

Scope:

- `src/picker/mod.rs` saved-only Enter handling, confirmation state, recreate
  flow, attach handoff, status/error feedback, and pending attach modifiers.
- Picker and PTY attachment tests covering confirmation, refusal, successful
  recreation, attach failure, and saved-definition durability.
- Picker help/status text or `README.md`/`docs/stay.1` if the interaction is
  documented there.

Acceptance criteria:

- Enter on a `saved` row opens an explicit Yes/No confirmation explaining that
  the session is saved but not running and that Yes will recreate and attach; it
  does not attempt to attach to the missing tmux session first.
- No leaves the saved row intact, creates no tmux session, and returns to the
  picker without a spurious attach error.
- Yes recreates the session from its saved definition and then hands off to the
  normal attach relay automatically, carrying the pending read-only and
  low-priority modifiers if they were selected.
- Recreate or attach failure leaves an actionable error in the picker and never
  reports success or loses the saved definition; existing store rollback and
  durability semantics remain intact.
- The existing `r` action remains the explicit recreate-without-attach path.
- Rust tests and the exact `just qcheck` and `just mac-qcheck` gates pass.

## TASK-119 - Preserve attached clients across session rename

State: NEW

Goal:

- Rename a live session without disconnecting or otherwise booting clients that
  are already attached to it.

Dependencies:

- None.

Scope:

- `src/picker/mod.rs` rename action and error/rollback handling.
- `src/tmux.rs` session/client identity operations and `src/relay.rs` detach
  bookkeeping needed to survive a session-name change.
- Real-tmux tests in `tests/tmux_inventory.rs` and `tests/attachment.rs` with at
  least two attached clients, plus picker/store rename coverage.

Acceptance criteria:

- Renaming a live session leaves every pre-existing client attached to the same
  tmux session under the new name; client count, client attachment state, and
  pane process are unchanged.
- A Stay relay that was attached before the rename can still identify and detach
  only its own client after the rename; another client remains attached.
- A rename collision or tmux failure leaves both the live session name and the
  saved definition consistent, with the existing rollback/error visibility.
- Saved-only rename behavior remains supported and does not invoke a tmux client
  operation.
- Rust tests and the exact `just qcheck` and `just mac-qcheck` gates pass.
