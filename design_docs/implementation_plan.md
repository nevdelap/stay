# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

## TASK-114 - cache release Rust build artifacts

State: IMPLEMENTED

Goal:

- Reduce the time spent compiling retried tagged release builds, especially the
  expensive Frizbee-related `stay` code generation, by restoring Cargo and
  release target artifacts for a rerun of the same tag and commit.
- Keep the cache safe across the four native release targets without claiming
  that one tag's cache can be restored by a different tag.

Dependencies:

- TASK-113 must remain `COMPLETED`, because this task caches the release build
  introduced by the Frizbee picker implementation.

Design decision:

- Reuse `Swatinem/rust-cache@v2`, which is already used by the normal CI jobs,
  rather than introducing a second cache implementation. Select the installed
  Rust 1.89 toolchain before invoking the cache action so the toolchain is part
  of the cache environment, matching the action's documented usage.
- Enable `cache-workspace-crates: true`. The action's default dependency cache
  does not retain the workspace crate, but the long build is in `stay`'s
  Frizbee-instantiating release code generation. Dependency-only caching would
  not address the primary cost.
- Set `shared-key` to a release key containing both `${{ matrix.target }}` and
  `${{ github.sha }}`. The x86_64 and aarch64 targets, and the Linux and Darwin
  targets, must never restore one another's target directory. The commit
  component makes workspace-crate caching source-aware: a changed release commit
  cannot restore a prior commit's `stay` artifact, and each immutable cache key
  can save the artifacts built for that exact source revision. Keep the action's
  Rust-environment hash enabled so the Rust toolchain, `Cargo.toml`,
  `Cargo.lock`, toolchain files, and relevant Rust environment changes also
  invalidate the cache naturally.
- GitHub Actions cache access is scoped by ref. Since this workflow runs only
  for version-tag pushes, a cache from one release tag is not available to a
  different release tag. This task therefore promises reuse only for reruns of
  the same tag and commit. A trusted branch-scoped cache producer could provide
  cross-release reuse later, but is outside this task.
- Keep the existing 20-minute job timeout, target matrix, build command, binary
  smoke test, and archive contents unchanged. Caching cannot shorten the time
  spent waiting for a hosted runner or the first cold build, so those are
  explicitly outside this task.

Scope:

- Update only the `build-binaries` matrix job in
  `.github/workflows/release.yml`. Add the existing Rust cache action after
  `dtolnay/rust-toolchain@1.89.0` has selected the toolchain and before the
  release build. Configure the target-specific shared key and workspace-crate
  caching described above.
- Do not change normal CI cache configuration, the Nix jobs, the release
  timeout, the Rust toolchain, Cargo manifests, package version, source code,
  tests, or release archive packaging.
- Do not cache release archives or publish outputs. The cache is only for
  Cargo's registry, build dependencies, and target artifacts used to produce the
  current matrix job's binary.

Acceptance criteria:

- Every `build-binaries` matrix leg runs the cache action after Rust 1.89 is
  installed and before
  `cargo +1.89.0 build --release --locked --target "$TARGET"`.
- The cache configuration enables workspace-crate caching and includes both the
  matrix target and `github.sha` in its shared key. The four target legs
  therefore have independent, source-aware cache namespaces, and a rerun of the
  same tag and commit can restore the exact artifacts it previously saved.
- The task does not require cache reuse across different release tags. The plan
  documents that limitation rather than assuming that identical keys bypass
  GitHub Actions ref scoping. No cache setting permits one target architecture
  or source revision to consume another's workspace artifacts.
- The existing binary version check, tmux smoke test, archive packaging, and
  artifact upload continue to run unchanged for all four targets.
- A tagged release run or rerun provides evidence that each target can save or
  restore its cache, and a subsequent run of the same target reports an exact
  cache hit where the first run successfully saved one. The evidence records the
  target-specific cache keys and confirms that a cache hit does not skip the
  build, version check, smoke test, or packaging steps.
- The workflow passes the exact `just qlint` recipe on a clean final planning
  commit, including its actionlint, YAML, and Markdown checks. If the recipe
  formats the plan, the formatted result is committed and the recipe is run
  again until it makes no further changes. No Rust, acceptance, package-version,
  or release-content gate is required because this task changes workflow
  configuration only.
