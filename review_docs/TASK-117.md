# Review: TASK-117

## Findings

### R001

Status: OPEN

The task explicitly includes both Unix and non-Unix input paths, but its only
verification evidence is `just qcheck` and `just mac-qcheck`. Those gates run
Linux and macOS, so they do not compile or execute the `#[cfg(not(unix))]`
crossterm reader. The scope also requires platform-event decoding tests, but
does not name a non-Unix target or another command that would exercise that
path.

Narrow the scope to the supported Unix platforms, or name the supported
non-Unix target(s) and add the corresponding compile/test evidence to the
acceptance criteria.

### R002

Status: OPEN

The acceptance criteria define the desired control-key mappings but do not
define the Unix byte-level protocol that counts as a supported modified-arrow
sequence, the behavior for truncated/unknown sequences, or precedence when a
configured detach or copy-mode control overlaps a newly added alias. “Without
swallowing the following input” is an implementation constraint, not a
complete input contract. The rename-editor requirement that Ctrl+Up/Down “do
not corrupt or submit” also does not explicitly require a no-op preserving the
text and cursor.

Specify the supported CSI forms and malformed-sequence behavior, the control
precedence rule, and the exact rename-editor result for Ctrl+Up/Down before
implementation.

## Final decision

Status: REVIEWED_FOUND_ISSUES

R001 and R002 remain open. TASK-117 remains `NEW`.
