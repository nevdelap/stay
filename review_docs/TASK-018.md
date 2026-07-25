# Review: TASK-018

## Findings

### R001

Status: ADDRESSED

The fork/PTY picker helpers now immediately exec filtered helper tests in a
fresh process. This avoids inheriting libtest's process-global locks while
keeping the parent-side PTY emulator. The default-parallel `just qcheck` now
passes, including the picker and relay tests.

### R002

Status: ADDRESSED

The commit changes both `.codex/config.toml` and
`.codex/rules/default.rules`. These changes were explicitly requested by the
user, so the scope concern is resolved.

## Final decision

Status: COMPLETED

Verification:

- `just qcheck` passed with the fresh-process test helpers.
- The exact repository `just mac-qcheck` recipe passed with the configured
  macOS environment preserved.
- The working tree was clean after the latest commit.
