# Review: TASK-115

## Findings

### R001

Status: ADDRESSED

The planning change combines a planning commit with housekeeping work. In
addition to adding TASK-115, the change removes TASK-113 from
`design_docs/implementation_plan.md`, adds the TASK-113 lessons to
`design_docs/lessons_learned.md`, adds `review_docs/HOUSEKEEPING.md`, and
deletes `review_docs/TASK-113.md`. The commit subject is `Planning: persist
session definitions`, but the workflow requires housekeeping to be a distinct
`HOUSEKEEPING:` commit and says a planning commit may contain only the task
specification and planning guidance. This prevents the planning snapshot from
being an immutable, independently reviewable baseline for TASK-115.

#### Resolution evidence

R001 is addressed. The exact parent-to-current comparison uses
`jj diff --from '@-' --to '@'` and contains only
`design_docs/implementation_plan.md` and `review_docs/TASK-115.md`. The
TASK-113 retirement and `HOUSEKEEPING:` change are in the parent commit
`351eb0015fc6`, so the current planning change does not combine commit types.

### R002

Status: ADDRESSED

The design calls a write durable across a reboot but specifies only flushing a
temporary file and atomically renaming it. A file flush does not force the
file's contents to stable storage, and a rename is not durable until the
containing directory is synchronized. A crash after the rename can therefore
lose the session store or leave the old store in place, violating the goal and
the acceptance requirement for durable definitions.

#### Resolution evidence

R002 is addressed. The revised TASK-115 design requires `sync_all` on the
temporary file before rename and on the parent directory after rename, and the
acceptance criteria require a filesystem-seam test for that ordering and the
pre-rename failure case.

### R003

Status: ADDRESSED

The task spans two independent state systems—tmux and the session store—but
does not define failure ordering or compensation. For example, a successful
force-recreate followed by a store-write failure can leave the new live session
running with the old saved definition; a successful live kill followed by a
store-removal failure can make the saved-only definition reappear on the next
list. The wording that failed operations “leave the prior definition intact”
does not say whether the tmux operation is rolled back, whether the operation
is reported as partially successful, or which ordering is required for create,
force-recreate, rename, single kill, and kill-all.

#### Resolution evidence

R003 is addressed. The revised plan defines store-first create/recreate/rename
with snapshot restoration, tmux-first kill and kill-all with explicit partial
success reporting, saved-only kill without tmux, and failure-injection tests
for each path.

### R004

Status: ADDRESSED

The public JSON and persistence contracts are not fully specified. The plan
requires a “stable saved row” and a TOML store but does not state the exact
saved-row values for fields such as `created_at`, directory, command, and
termination fields, nor the TOML entry shape, missing-store behavior, duplicate
key policy, or validation/version policy. It also refers to names and command
arguments containing “supported special characters” without enumerating that
set. Implementers would have to infer these compatibility decisions from the
existing code or choose them ad hoc, making the acceptance criteria
non-deterministic.

#### Resolution evidence

R004 is addressed. The revised plan specifies the version-1 TOML shape,
required fields, duplicate/extra/missing-field rejection, absent-store
semantics, exact saved JSON values, unsupported-version fixtures, and the
complete name/path/command special-character matrix.

### R005

Status: ADDRESSED

The revised plan still contains a contradictory persistence contract. It says
that a failure syncing the renamed parent directory retains the newly renamed
store and reports uncertain durability. Elsewhere it says that a failed
persistence operation leaves the prior definition intact, and that a failed
initial store commit must not mutate tmux. A post-rename directory-sync error
is both a persistence failure and a case where the new snapshot already exists,
so the implementation cannot determine whether to roll back, avoid the tmux
operation, or proceed with the new snapshot. The same ambiguity affects the
store-first lifecycle error paths and the required user-visible diagnostics.

Choose one commit boundary and apply it consistently: define the result and
rollback behavior for post-rename directory-sync failure, then align the
design, acceptance criteria, and failure-injection tests with that choice.

#### Resolution evidence

R005 is addressed. The revised plan distinguishes pre-rename failure from a
committed-but-uncertain post-rename result, blocks the tmux action when the
store is not fully durable, and specifies the resulting diagnostics and tests.

### R006

Status: ADDRESSED

The documentation scope remains too vague for the planning contract. It says
to update “the user manual and any CLI/picker documentation,” but does not name
the repository files that define the public manual, generated help expectations,
picker display, and stable JSON contract. An implementer could update one
surface and omit another while still claiming the broad scope is complete.

Name the affected documentation file family explicitly (including the manual
page and README if both are public surfaces), and state the required saved-row
and picker text in each applicable document or test fixture.

#### Resolution evidence

R006 is addressed. The scope now names `README.md`, `docs/stay.1`,
`src/cli.rs`, `tests/cli_help.rs`, and `tests/acceptance.bats`, including the
saved-row text, JSON status, store path, and saved-only lifecycle behavior.

### R007

Status: ADDRESSED

The rename contract does not cover saved-only rows. The scope includes picker
rename and the task describes saved-only entries as recoverable for correction,
but the lifecycle design only specifies “store snapshot, then tmux rename.” A
saved-only row has no tmux session to rename. The plan must say whether picker
rename performs a store-only rename, is rejected with an actionable error, or
uses another recovery path, and must require coverage for that behavior.

Without this decision, an implementation can call `rename-session` and fail to
support correction of a saved-only definition while still claiming to satisfy
the broad rename scope.

#### Resolution evidence

R007 is addressed. The revised plan defines saved-only rename as a validated
store-only operation with collision checks, no tmux invocation, matching error
semantics, and dedicated tests.

### R008

Status: ADDRESSED

The task adds application code under `src/` but its scope and acceptance
criteria do not require the repository-mandated patch-version increment,
`Cargo.lock` update, and version assertions. The team rules require exactly
one patch bump for every task commit that changes non-test application source;
without stating that in TASK-115, an implementation can satisfy the listed
feature behavior and gates while still violating the task and commit contract.

Add the required versioning work to the scope and acceptance criteria, with the
baseline version and all affected assertions identified.

#### Resolution evidence

R008 is addressed. The revised plan identifies the verified `0.0.89` baseline,
requires exactly one bump to `0.0.90` in `Cargo.toml`, `Cargo.lock`, and the
manual header, and names the version assertion and metadata-derived checks.

## Final decision

Status: PLANNING_APPROVED

R001-R008 are addressed. TASK-115 remains `NEW` and is approved for Igor to
implement.
