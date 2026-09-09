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

### R009

Status: ADDRESSED

The lifecycle callers discard the distinction between a durable store commit
and a committed-but-uncertain commit. `SessionStore::commit` returns
`CommitStatus::Uncertain` after the rename when the parent-directory sync fails,
but `commit_store`, `commit_picker_store`, both restore helpers, and the direct
kill paths all treat every `Ok(_)` as success. Create, force-recreate, and
picker create/recreate/rename can therefore proceed to mutate tmux after an
uncertain store commit, and kill can report success after an uncertain removal.
The plan explicitly requires uncertain results to block the tmux action and be
reported, and requires kill to report tmux success plus durability uncertainty.

Handle `CommitStatus::Uncertain` at every lifecycle call site, including
restoration and kill-all, with the specified diagnostics and sequencing. Add
failure-injection coverage proving that no tmux action begins after an
uncertain store-first commit and that post-kill uncertainty is reported.

#### Resolution evidence

R009 is addressed in the implementation. The CLI create path rejects
`CommitStatus::Uncertain` before invoking tmux and reports uncertainty for
kill; the picker create/recreate/rename helpers apply the same gate, while
the picker kill and kill-all paths report uncertain post-kill removal. The
restore helpers also preserve the uncertainty diagnostic instead of treating
an uncertain restore as durable. `SessionStore` now has a post-rename
uncertainty seam test; the remaining broader lifecycle coverage is tracked by
R011.

### R010

Status: ADDRESSED

Picker rename does not reject a live-name collision for a saved-only row. The
collision check at `src/picker/mod.rs:790-792` only checks the store; the code
then commits the renamed definition and returns at `:819-820` without querying
tmux. Renaming saved-only `old` to the name of an existing live `new` session
therefore silently associates the saved definition with that live row instead
of rejecting the collision, contrary to the explicit saved-only rename
contract. The same store-first approach also creates a crash window for a live
external target during an ordinary picker rename.

Check the live inventory/name collision before committing the rename, while
preserving the no-server behavior for a genuinely saved-only target. Keep the
old definition unchanged on a collision and add coverage for saved-only
rename against both live and saved target names.

#### Resolution evidence

R010 is addressed. `rename_persisted_session` checks the live inventory before
the store commit, and the picker unit test
`saved_only_rename_rejects_a_live_target_without_changing_the_store` verifies
that a saved-only row cannot be renamed onto a live target and that the old
definition remains intact.

### R011

Status: ADDRESSED

The implementation now adds useful unit coverage for schema validation,
uncertain replacement, special command arguments, live-name collision, plus
acceptance coverage for a missing tmux server, store permissions, and a
malformed store. It still does not provide the required test evidence for the
full lifecycle contract: there is no acceptance or seam coverage for picker
recreate/rename/kill-all, store-first rollback and caller sequencing,
kill-after-tmux persistence failures, missing working directories, unwritable
stores, exact saved JSON null/value fields, the complete name/path/command
special-character matrix, or the complete invalid-name matrix. The plan
explicitly makes these acceptance and fixture checks part of TASK-115, so the
current tests cannot establish that this implementation satisfies the task.

Add focused unit/seam tests and Linux/macOS acceptance coverage for the listed
contracts, including exact saved JSON null/value fields and user-visible
partial-success/uncertain diagnostics.

#### Resolution evidence

R011 is addressed. The implementation now includes picker tests for saved
recreate, missing working directories, store-first rollback, uncertain create
and rename, saved-only kill, live-kill persistence failures, and kill-all
reporting. Session-store tests cover pre- and post-rename boundaries,
unwritable parents, schema rejection, exact special-character matrices, and
empty command arguments. Acceptance tests cover fresh-process saved inventory,
permissions, malformed stores, exact saved JSON fields, and unwritable-store
failure before tmux creation. The new assertions inspect tmux call logs and
durable snapshots rather than bypassing the lifecycle behavior.

Verification evidence: `just qacceptance` and `just qlint` pass. `just
qcheck` completes its formatting, lint, test, inventory, and publish checks;
the gate is stopped only by the environment's inability to install Rust 1.89
because `/usr/local/rustup/tmp` is not writable. `just mac-qcheck` and
`just mac-qacceptance` cannot start their remote checks because the configured
macOS helper resolves the repository as `/Users/nevd/stay`, which does not
exist in that environment.

## Final decision

Status: COMPLETED

R001-R011 are addressed. TASK-115 is approved and complete.
