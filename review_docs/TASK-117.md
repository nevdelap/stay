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

R001 and R002 were addressed at planning time; TASK-117 was then `NEW` for
implementation.

## Implementation review

### Findings

No material implementation findings. The Unix decoder covers the specified
control-byte aliases and modified CSI forms, preserves following input for
unknown or truncated sequences, and keeps the rename editor's vertical
controls as non-submitting no-ops. The picker and editor tests cover the
navigation mappings and boundary behavior on the supported Unix paths.

The exact `just qcheck` and `just mac-qcheck` gates passed on the final
implementation snapshot.

## Final decision

Status: COMPLETED

The implementation satisfies the approved TASK-117 scope and acceptance
criteria. No material review findings remain.

## Second implementation review

### R003

Status: OPEN

`InputReader::escape_or_quit` reads the first byte after `ESC [` or `ESC O`
into the sequence before validating it as a CSI parameter, intermediate, or
final byte. Therefore an input such as `ESC [` followed immediately by
Ctrl+P consumes the Ctrl+P byte and returns `Other`, instead of preserving it
for the next read. The acceptance criteria explicitly require unknown or
truncated candidates not to consume the following ordinary byte. The existing
test covers an invalid byte after an already-valid parameter prefix, but not
this first-candidate case.

Validate and push back an invalid first candidate in the same way as later
invalid bytes, and add a regression test for it.

## Final decision

Status: CHANGES_REQUESTED

TASK-117 remains `IMPLEMENTED` pending correction of R003 and rerunning the
required gates.
