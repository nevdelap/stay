# Review: TASK-FIXUPS

## Findings

No findings.

## Verification

- Reviewed the complete `TASK-FIXUPS` diff against `86c40c4`.
- The sweep test removes its out-of-prefix control socket after
  `kill-server`.
- The temporary-directory test now panics inside the guard and asserts the
  path is gone after `catch_unwind` returns.
- All seven remaining `0..250` polling windows were raised to `0..500`; no
  `0..250` wait remains in `tests/`.
- The package version advances from `0.0.46` to `0.0.47`.
- The exact `just qcheck` recipe passed.
- The exact `just mac-qcheck` recipe passed.

## Final decision

Status: COMPLETED

TASK-FIXUPS satisfies its stated acceptance criteria and is approved.
