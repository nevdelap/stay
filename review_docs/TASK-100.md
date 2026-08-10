# Review: TASK-100

## Findings

### R001

Status: ADDRESSED

The required unreadable-startup-candidate case is not exercised. At
`tests/acceptance.bats:1273-1279`, the fixture creates `~/.profile` as a
directory. That follows the non-regular-file branch in
`find_rc_conflict()` and never attempts to read an unreadable regular file.
A regression in the permission/read-error path could therefore pass this
acceptance scenario. Create a regular startup candidate and remove its read
permission, then retain the warning and alias-omission assertions.

Evidence: the directory case remains at `tests/acceptance.bats:1273-1280`,
and a regular `~/.profile` with mode `000` is now exercised at
`tests/acceptance.bats:1282-1289`. The focused alias acceptance test passes.

## Verification

- `just qcheck`: passed on the amended commit.
- Exact `just mac-qcheck`: passed on the amended commit.
- Focused Linux Bats alias test: passed.
- The prompt-focused test could not run in this image because `zsh` is absent;
  the CI installer provisions `zsh` on both acceptance platforms.
- `just qlint`: passed after the review metadata amendment.

## Final decision

Status: COMPLETED
