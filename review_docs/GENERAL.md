# General Review: outstanding cross-task issues

This document records review issues that are not specific to one task and
therefore should not be duplicated in individual task review documents.

## Findings

### G001

Status: OPEN

The exact `just qcheck` gate does not complete on the current final snapshot.
The stable Rust and integration suites pass, but the 1.89 MSRV test run hangs
in picker and relay unit tests after the stable suite completes; the run was
interrupted after the tests exceeded 60 seconds. The exact `just mac-qcheck`
gate passes.

This is a shared verification-environment issue rather than a TASK-117 or
TASK-118 implementation defect. Re-run the exact Linux gate in an environment
where the MSRV test suite completes successfully. Until then, tasks requiring
that gate cannot be marked `COMPLETED`.

## Final decision

Status: IMPLEMENTATION_REVIEW_BLOCKED

G001 remains open and blocks completion of TASK-117 and TASK-118. TASK-119
also remains subject to its task-specific relay finding.
