# Review: TASK-045

## Findings

### R001

Status: ADDRESSED

The task commit does not bump the package patch version. `Cargo.toml` is
unchanged, and both `HEAD^` and `HEAD` contain version `0.0.31`. The repository
completion rules require the patch version to increase by exactly one for each
task commit. Update the package version (and any generated lockfile metadata if
required), preserve the resulting version consistently, and rerun both gates.

Addressed: `Cargo.toml` and `Cargo.lock` now contain package version `0.0.32`,
exactly one patch version above the task baseline, and the CLI version test no
longer hard-codes the old value.

### R002

Status: ADDRESSED

`src/session_name.rs:71` checks the length limit before the existing
disallowed-character scan. Consequently, an over-limit name containing `.`,
`:`, or an ASCII control character now reports only `TooLong` instead of the
existing disallowed-character error and position. This changes the behavior
covered by the acceptance criterion that existing disallowed-character
validation remain unchanged. Validate the existing disallowed characters with
their original precedence, or otherwise preserve that diagnostic behavior, and
add a regression test combining an over-limit name with a disallowed character.

Addressed: the validator scans for disallowed characters before applying the
length limit, and `disallowed_characters_keep_precedence_over_the_length_limit`
covers the combined over-limit/disallowed-character case.

## Final decision

Status: COMPLETED
