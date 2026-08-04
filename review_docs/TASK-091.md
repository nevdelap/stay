# Review: TASK-091

## Findings

### R001

Status: ADDRESSED

The pending-generation state exposes the previous result instead of the
required `Filtering...` placeholder. After a result has been published,
`filter_has_published_result` remains true. A query edit in
`apply_filter_edit` (around `src/picker/mod.rs:589-600`) or an inventory poll
in `apply_poll_result` (around `src/picker/mod.rs:1418-1423`) sets
`filter_pending` but does not clear that flag or hide `filter_matches`.
`status()` therefore returns the normal filter status at
`src/picker/mod.rs:1957-1964`, and `render_filter_list` deliberately renders
the old rows at `src/picker/mod.rs:2175-2215`.

This violates the task's pending-result acceptance criteria: while any current
query or inventory generation is pending, the input must remain visible, the
placeholder must read `Filtering...`, no old session row may be exposed as
selectable, and Enter must be a no-op. The fix must preserve the old snapshot
internally if desired, but hide it atomically until the result carrying the
current three generations is published. Add a regression that settles a
filter, edits the query (and separately refreshes inventory), and asserts the
pending status/placeholder and absence of old rows before publication.

Evidence: `status()` now returns `FILTER_PENDING_STATUS` for every pending
generation, `render_filter_list` hides all session rows while pending, and
`pending_filter_render_hides_the_previous_result_until_publication` verifies
the placeholder and stale-row absence. Inventory refreshes also requeue the
current generation and are covered by
`filter_poll_requeues_inventory_and_preserves_a_matching_selection`.

### R002

Status: ADDRESSED

The task's required focused coverage is incomplete. The new matcher test at
`src/picker/mod.rs:3877-3907` checks empty-query order and only membership for
the fuzzy query; it does not assert descending score order or deterministic
tie order. There is also no filter-specific unit coverage for the required
editing operations (cursor movement and deletion), Unicode matching, exact
and zero-match matcher queries, or inventory polling while filtering. The PTY
test covers the main attach/cancel path, but it cannot replace the explicitly
required unit coverage for these state transitions and matcher properties.

Add the missing focused tests, including an exact expected ranked sequence,
same-score tie ordering against inventory order, UTF-8 query/name matching,
all filter editing operations, and a poll-while-filtering case that verifies
selection preservation and generation invalidation.

Evidence: the current implementation hides all rows while
`filter_pending` is true, and the focused tests
`pending_filter_render_hides_the_previous_result_until_publication`,
`filter_editing_matches_name_prompt_cursor_and_deletion_semantics`,
`filter_poll_requeues_inventory_and_preserves_a_matching_selection`, and
the expanded `nucleo_matches_case_insensitively_and_keeps_inventory_ties_stable`
cover the required pending rendering, editing, inventory-generation, ranking,
tie-order, Unicode, exact, and zero-match cases.

### R003

Status: ADDRESSED

The commit introduces an authorized drive-by behavior change that is not
recorded in TASK-091's specification: the picker and README no longer
advertise the still-functional `c` create and `q` quit shortcuts. The commit
message's `Authorized drive-by` paragraph is not a substitute for updating the
implementation plan with the resulting scope and acceptance criteria. The
team process requires a user-authorized variation to be auditable in the
governing task before review, and the commit contract provides only the
implementer/reviewer sections for this shared state.

Either restore the existing advertisements, or update TASK-091's scope and
acceptance criteria to record the authorization and intended new status
surface, then preserve that decision in the commit's contract-compliant
`Implemented:` section while removing the extra message section.

Evidence: TASK-091's scope and acceptance criteria now record the authorized
status-surface variation, and the current commit uses only the contract's
`Implemented:`, `Reviewed:`, and trailer sections while retaining functional
`c` and `q` handling.

### R004

Status: ADDRESSED

The query-edit path still performs a complete synchronous inventory traversal
and deep clone on the picker input thread. Every printable filter edit calls
`apply_filter_edit` (`src/picker/mod.rs:589-600`), which calls
`queue_filter_request`; that method builds `FilterRequest.sessions` by cloning
every session name (`src/picker/mod.rs:1617-1626`). The matching scan itself is
on the worker, but the input thread still pays O(inventory size) work and
allocations for every keystroke. With a large inventory, rapid edits can also
enqueue many full inventory copies before the worker coalesces them. This
violates the task's requirement that query changes be accepted without
synchronously rescanning the complete inventory on the picker input thread and
undercuts the stated large-inventory responsiveness guarantee. Keep the
worker-owned inventory snapshot and send only the changed query/generations for
query edits; transfer a full inventory snapshot only when the inventory
generation changes.

Evidence: `queue_filter_request` now tracks
`filter_inventory_queued_generation` and sends the full inventory only for a
new inventory generation (`src/picker/mod.rs:1616-1644`). Query edits reuse the
worker-owned snapshot, while inventory polls still send the changed inventory.

### R005

Status: ADDRESSED

`enter_filter` resets `filter_query_generation` to zero
(`src/picker/mod.rs:1665-1670`). After a session that processed edits, the next
filter session therefore emits a smaller query generation than the previous
session. The session generation protects result validity, but it does not make
the query-generation field monotonically increasing as explicitly required by
TASK-091. Keep query generations monotonic for the lifetime of the picker and
use the current value when constructing the first request of each new filter
session.

Evidence: `enter_filter` no longer resets `filter_query_generation`, and the
new `filter_query_generation_stays_monotonic_across_reentry` test verifies that
the next session keeps the prior generation
(`src/picker/mod.rs:1678-1694`, `src/picker/mod.rs:3840-3875`).

### R006

Status: ADDRESSED

The README's Picker keys table was changed to remove `c`, `q`, and Escape while
the implementation still supports all three (`src/picker/mod.rs:395-446`) and
the task plan authorizes omitting `c` and `q` from status panels specifically,
not from the complete documentation table. The table is introduced as the
picker's key-binding reference, so it now omits functional create and quit
controls and does not accurately document the picker. Restore those entries in
`README.md`; keep the authorized omission limited to the rendered status
panels.

Evidence: the README now documents `c`, `q or Esc`, and the filter controls,
while the omission is explicitly limited to the compact rendered status panel
(`README.md:60-81`).

### R007

Status: ADDRESSED

The settled filtered render performs nested full-inventory lookups. Both
`picker_filter_area` (`src/picker/mod.rs:2378-2395`) and
`render_filter_list` (`src/picker/mod.rs:2183-2189`) iterate every filtered
name and call `state.sessions.iter().find(...)`. An empty query matches the
entire inventory, so each frame performs O(n²) session-name comparisons (and
the row-render loop performs additional linear lookups). A large inventory can
therefore block the picker input/render loop after the worker publishes, which
violates the task's large-inventory responsiveness requirement. Build a
name-to-record/index lookup once per inventory or otherwise retain match
indices so layout sizing and visible-row rendering are linear in the inventory
and viewport rather than nested scans.

Evidence: published results now resolve names to `filter_match_indices` once
with a single inventory map (`src/picker/mod.rs:1663-1676`). Filter sizing and
visible-row rendering consume those indices directly
(`src/picker/mod.rs:2205-2247`, `src/picker/mod.rs:2407-2424`), and focused
render coverage initializes and exercises the indexed path.

## Final decision

Status: COMPLETED
