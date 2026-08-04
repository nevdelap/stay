# Review: TASK-EXTRA

This is an advisory review of the TASK-091 specification. It does not change
the state of TASK-091 in `design_docs/implementation_plan.md`.

## Findings

### R001

Status: ADDRESSED

`design_docs/implementation_plan.md` is not formatter-clean in the planning
commit. Running `just qcheck` rewrites the TASK-091 prose with the repository's
markdown formatter and then fails the format check. Reformat the planning
document and verify the resulting commit is clean before handing the plan
forward.

Review pass 2: addressed. The plan is now canonically wrapped, and the current
tree passes `just qcheck` without producing further changes.

### R002

Status: ADDRESSED

The plan changes the picker keyboard contract by adding `/` and a filter-mode
status, but its scope only mentions source comments and picker tests. It omits
the tracked user and design documentation: `README.md`'s Picker keys table and
`design_docs/stay.html`'s picker key list/status-line sections currently do not
describe `/` or filter mode. Extend the scope and acceptance criteria to update
both documents accurately.

Review pass 2: addressed. The scope and acceptance criteria now explicitly
include `README.md` and `design_docs/stay.html`.

### R003

Status: ADDRESSED

The polling requirement says to “select the first remaining match while
filtering” after every successful inventory poll. The picker polls repeatedly;
resetting selection to the first match on each poll contradicts the requirement
that Up/Down and PageUp/PageDown navigate among multiple matches and makes the
multi-match attach flow unusable. Specify that polling preserves the selected
matching session, clamps the index, and selects the first match only when
entering or editing the query, or when the selected match disappears.

Review pass 2: addressed. The polling scope now preserves a selected matching
session by name and selects the first remaining match only when necessary.

### R004

Status: ADDRESSED

The plan targets a “large session list” but requires the low-level
`nucleo-matcher` to run directly in the picker path on every query edit. Its
own documentation warns that using `nucleo-matcher` directly in an interactive
UI loop can be very slow and recommends the higher-level `nucleo` crate for
large match sets: <https://docs.rs/nucleo-matcher>. Specify either a bounded
matching strategy, a worker/debounced matching path that keeps rendering and
input responsive, or a justified limit on inventory size. Add a responsiveness
regression or change the dependency choice accordingly.

Review pass 3: addressed. The plan now uses the managed `nucleo` worker,
forbids synchronous rescans on the input thread, and adds a responsiveness
regression without timing-sensitive sleeps.

### R005

Status: ADDRESSED

The existing picker uses `selected_name = None` for the synthetic create row,
while the new plan also says to clear selection when filtering yields no
matches. It does not define the distinct rendering, navigation, and scrolling
semantics for that state. In particular, the filter input row must remain
visible and editable but must not become a selectable create row; `Enter` must
remain a no-op; and `list_offset` must clearly apply to matching session rows
while the input row remains in its specified position. Add explicit no-match
and narrow/overflow acceptance criteria and render/state tests.

Review pass 3: addressed. The plan distinguishes filter no-match state from
idle create selection, fixes the input row at the top, defines row-only
scrolling, and adds explicit no-match and narrow/overflow criteria.

### R006

Status: ADDRESSED

The PTY requirement only says to wait for observed picker output before sending
input (`design_docs/implementation_plan.md:184-189`). That can still permit a
test to send `/`, the query, and Enter before the filter-mode redraw or ranked
rows have been observed. Require readiness checkpoints after entering filter
mode and after the query results are rendered, before sending the next input;
assert the filter row and intended ranked match in those observations.

Review pass 3: addressed. The PTY scope now requires separate observed renders
after `/` and after the complete query, before navigation, Enter, or Escape.

### R007

Status: ADDRESSED

The managed worker may publish the previous ranked snapshot while a new query
is being computed (`design_docs/implementation_plan.md:121-126`). The rest of
the plan still says the first ranked match is selected when the query is edited
and that Enter attaches the selected match. Without a query/generation check,
the picker can display rows ranked for the old query while showing the new
query, then let Enter attach a session that does not match what the user typed.
Inventory polling creates the same stale-snapshot risk. Define a result
generation or query identity, show an explicit pending state or disable
navigation/Enter until the current result is ready, and discard worker results
for obsolete queries or inventories. Add a deterministic test for editing and
pressing Enter while a worker result is pending.

Review pass 4: addressed. The plan now tags query and inventory generations,
discards obsolete results, hides stale rows, disables navigation and Enter
while pending, and tests Enter before publication.

### R008

Status: ADDRESSED

The responsiveness regression requires proving that input is accepted while a
large match scan runs, but it does not specify a deterministic synchronization
seam for that proof. “Large synthetic inventory” alone can pass on a fast host
and fail on a slow one, while a no-sleep rule excludes a timing-based assertion.
Specify a controllable worker/test seam (for example, a barrier around a scan)
and assert the ordered events: query enqueue, subsequent input handling, and
result publication.

Review pass 4: addressed. The plan adds a barrier/channel-backed matcher seam
and requires assertions for enqueue, input handling, and publication order.

### R009

Status: ADDRESSED

The generation rules cover query edits and inventory changes, but do not say
that cancelling filter mode invalidates the worker session. A late result from
the cancelled filter could therefore be delivered after Escape, and a rapid
filter re-entry could reuse the same query/inventory generations and accept a
result from the prior filter instance. Add a monotonically increasing filter
session generation (or explicit cancellation token) that is changed on entry
and Escape, and require late results from prior filter sessions to be ignored.
Test cancel-and-immediate-reenter with a delayed worker result.

Review pass 5: addressed. The plan adds a filter-session generation, invalidates
the worker on Escape and filter exit, rejects late results, and adds the
cancel-and-immediate-reenter delayed-result regression.

## Final decision

Status: COMPLETED
