# Review: TASK-090

## Findings

### R001

Status: ADDRESSED

The regression test now captures picker stdout and waits for both initial
session rows before sending selection input. It waits for the selected-session
modifier state before killing the selected session, then waits for a redraw that
contains the surviving session and excludes the killed session before pressing
Enter. The existing no-attachment assertion remains, and fixed pre-input
sleeps are removed. The package version advances from 0.0.74 to 0.0.75 and the
lockfile matches.

## Verification

- Reviewed the complete TASK-090 diff against its parent.
- The modified regression passed in isolation.
- Two consecutive clean exact `just qcheck` runs passed after an earlier
  environment-level PTY-unit-test hang was stopped.
- The exact `just mac-qcheck` recipe passed.

## Final decision

Status: COMPLETED
