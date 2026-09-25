# Review: TASK-117

## Findings

### R001

Status: ADDRESSED

The task explicitly includes both Unix and non-Unix input paths, but its only
verification evidence is `just qcheck` and `just mac-qcheck`. Those gates run
Linux and macOS, so they do not compile or execute the `#[cfg(not(unix))]`
crossterm reader. The scope also requires platform-event decoding tests, but
does not name a non-Unix target or another command that would exercise that
path.

Addressed in the current planning pass: the task is now explicitly limited to
supported Unix platforms, Linux and macOS, and its scope and acceptance
criteria require matching Linux/macOS unit coverage and the exact qcheck and
mac-qcheck gates.

### R002

Status: ADDRESSED

The acceptance criteria define the desired control-key mappings but do not
define the Unix byte-level protocol that counts as a supported modified-arrow
sequence, the behavior for truncated/unknown sequences, or precedence when a
configured detach or copy-mode control overlaps a newly added alias. “Without
swallowing the following input” is an implementation constraint, not a
complete input contract. The rename-editor requirement that Ctrl+Up/Down “do
not corrupt or submit” also does not explicitly require a no-op preserving the
text and cursor.

Addressed in the current planning pass: the plan defines the supported CSI
forms, no-op behavior for unknown and truncated sequences, preservation of the
next ordinary byte, picker-only alias precedence, and the rename editor's
unchanged mode, text, and cursor for Ctrl+Up/Down.

## Final decision

Status: PLANNING_APPROVED

R001 and R002 are addressed. TASK-117 remains `NEW` for implementation.
