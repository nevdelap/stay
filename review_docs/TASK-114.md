# Review: TASK-114

## Findings

### R001

Status: ADDRESSED

The cache design does not achieve its stated cross-release reuse goal. The
release workflow runs only from version tags, and omitting `github.sha` and the
tag from the cache key does not remove GitHub Actions' ref scoping. GitHub's
cache rules do not allow a run for one tag to restore a cache created for a
different tag, even when the cache keys are identical. Therefore a cache saved
by `v0.0.89` cannot be restored by `v0.0.90`; the proposed `shared-key` only
separates the matrix legs within an accessible cache scope.

The plan must either add a trusted branch-scoped cache producer that the
release-tag jobs can restore, or narrow the goal and acceptance language to
same-tag reruns and otherwise explain the release-to-release limitation. The
current claim that later releases can reuse the target artifacts is not
implementable with the scoped workflow described here.

#### Evidence

The release workflow is triggered by `push` to `v[0-9]+.[0-9]+.[0-9]+` tags
(.github/workflows/release.yml:3-7), while the plan changes only that release
job (implementation_plan.md:236-243). GitHub's dependency-cache reference
explicitly states that caches created for one tag cannot be restored by a run
triggered for another tag:
https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching#restrictions-for-accessing-a-cache.

#### Resolution evidence

R001 is addressed. The plan now limits the guarantee to reruns of the same tag
and commit, explicitly documents that different release tags cannot restore
one another's caches, and identifies a trusted branch-scoped producer as a
future option outside this task (implementation_plan.md:199-203,
:228-233, :261-264).

### R002

Status: ADDRESSED

The proposed `cache-workspace-crates: true` configuration has an unaddressed
source-invalidation problem. `Swatinem/rust-cache`'s automatic environment
hash covers manifests, lockfiles, toolchain files, and selected environment
variables, but not the workspace's Rust source contents. When a later release
changes `src/`, the same target cache key can restore the previous workspace
artifacts; Cargo may rebuild them, but the exact cache key is already occupied,
so the refreshed workspace artifacts are not saved for the next run. This can
make the expensive workspace code generation repeat on every release, which
undercuts the stated reason for enabling workspace-crate caching.

The plan must choose a source-aware target-cache strategy, such as a separate
source-keyed target cache with an appropriate restore/save policy, or explicitly
limit the benefit claim to unchanged-source reruns and document the repeated
rebuild behavior. It must also state how the four target namespaces and the
Rust/dependency cache interact under that strategy.

#### Evidence

The plan enables workspace-crate caching while excluding source or commit
identity from the cache key (implementation_plan.md:216-227). The upstream
rust-cache project documents this limitation in issue #348:
https://github.com/Swatinem/rust-cache/issues/348.

#### Resolution evidence

R002 is addressed. The plan now includes `${{ github.sha }}` and the matrix
target in `shared-key`, making workspace-crate caches source- and target-aware
for the promised same-commit reruns, and explicitly limits the benefit claim
to that scope (implementation_plan.md:220-227, :251-264).

### R003

Status: ADDRESSED

The committed planning document is not clean under the exact repository
quality gate. Running `just qlint` rewrites the TASK-114 section with the
repository's Markdown formatting and then fails because the worktree differs.
The planning commit therefore cannot be approved until the formatted plan is
committed and the exact planning quality checks pass on a clean tree.

#### Evidence

`just qlint` failed in the `format` recipe while processing
`design_docs/implementation_plan.md`; the failure output showed the TASK-114
section as formatter changes rather than reporting a clean result.

#### Resolution evidence

R003 is addressed. The formatted TASK-114 plan is committed, and the exact
`just qlint` recipe passes on the clean planning snapshot.

## Final decision

Status: PLANNING_APPROVED

R001-R003 are addressed. TASK-114 remains `NEW` and is approved for Igor to
implement from this planning baseline.
