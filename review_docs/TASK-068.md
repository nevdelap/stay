# Review: TASK-068

## Findings

### S001

Status: ADDRESSED

The TASK-068 specification is sufficiently complete for Igor to take
ownership of implementation. The pre-start handoff explicitly identifies
the provisional `justfile` and `docs/release.md` changes, assigns Igor
responsibility to review, revise, test, and carry them into the normal
implementation review, and states that the draft is not completed work.
The task also separates private implementation from the later authorized
public bootstrap and keeps the task `BLOCKED`.

The provisional publish recipe and runbook were syntax-checked but were not
treated as final implementation, and no publish, tag, repository setting,
Trusted Publisher, or automation action was run. `design_docs/stay_planning.md`
was excluded from this review as instructed.

### R001

Status: ADDRESSED

Section 4 makes the repository public before creating or applying the active
`main` ruleset. “Immediately” is not an atomic protection boundary: the public
repository can be briefly exposed without the required pull-request and direct
push controls. The runbook also does not preflight the GitHub plan. GitHub's
current documentation says branch and tag rulesets are available on public
repositories with GitHub Free, but on private repositories only with Pro,
Team, or Enterprise. Therefore the runbook must not assume that a ruleset can
be created privately and merely becomes enforced after visibility changes.

Revise the runbook and task evidence requirements to establish the plan
capability before the visibility checkpoint. Where the plan supports private
branch rulesets, create and verify the active `main` ruleset while the
repository is private, then change visibility and re-verify the effective
ruleset. Where it does not, stop before making the repository public and
require the documented plan upgrade or another approved control; do not rely
on the “immediately after” sequence. Keep the visibility change human-only.

Reference: <https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/managing-rulesets/about-rulesets>.

Evidence: the current runbook and implementation plan now preflight the
private-repository plan capability, require an effective private ruleset before
visibility changes, stop when the plan cannot provide it, and re-verify the
ruleset after making the repository public.

### R002

Status: ADDRESSED

The crates.io API requests in `just publish` and the release runbook do not
send a descriptive `User-Agent`. The package endpoint is public and does not
need a crates.io login, but the current request shape is rejected here: the
recipe's default `curl` request returns HTTP 403, while the same endpoint with
`stay-release-bootstrap/0.1 (https://github.com/nevdelap/stay)` returns HTTP
404 and the JSON response says that crate `stay` does not exist. Because the
recipe permits only 404, the current implementation refuses a valid unclaimed
package.

Add a stable, descriptive `User-Agent` to every crates.io API `curl` request
used by the recipe and runbook, and add hermetic assertions that the header is
present. Do not add authentication or a crates.io token. The relevant policy
is documented at <https://crates.io/data-access>.

Evidence: `just publish` now sends the descriptive header to the package
endpoint, the runbook sends it to both crates.io API checks, and
`just test-publish` passes all seven hermetic tests, including the header
assertion.

### R003

Status: OPEN

The exact `just qcheck` gate does not pass on the current commit. Both clean
runs failed in the existing full integration suite at
`force_recreate_replaces_an_already_dead_session_with_a_new_command`, timing
out while waiting for the dead pane swap. The same test passes in isolation,
and the exact `just mac-qcheck` gate passed, so this appears to be a test-suite
or tmux scheduling interaction rather than a TASK-068 code regression. It is
nevertheless an unmet required gate; capture a passing full `just qcheck` run
or resolve the existing failure before implementation approval. The maintainer
has explicitly deferred this pre-existing flaky-test investigation until after
the release; do not spend TASK-068 time investigating it or treat it as a
release-blocking implementation change. Retain it as a post-release follow-up,
and do not represent the exact `just qcheck` gate as passing.

## Final decision

Status: BLOCKED

Specification and pre-start handoff approved for Igor's implementation.
This is not final TASK-068 implementation approval; the task remains blocked
until the implementation phase is formally started.

## State transition

The maintainer authorized TASK-068 to start. The plan state was changed to
`NEW`,
with Igor's inherited draft work considered in progress under the explicit
ownership and review requirements in the task specification. This transition
does not approve the implementation or authorize any public release action.
