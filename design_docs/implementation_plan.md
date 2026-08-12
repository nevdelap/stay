# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

## TASK-107 - publish prebuilt Stay binaries for Homebrew

State: REVIEWED_FOUND_ISSUES

Goal:

- Make users on macOS and Linux able to install Stay through the dedicated
  Homebrew tap without compiling Stay locally. A tagged Stay release must
  publish target-native binary archives to its GitHub Release, and the tap
  formula must select the matching archive for the user's operating system and
  CPU architecture while installing tmux as the runtime dependency.

Dependencies:

- Nev, acting as the human release owner, must have authority to merge or push
  the application release commit and create one new stable tag `v0.0.86`, whose
  version matches the package version in `Cargo.toml`. The current package
  version is `0.0.85`, and the existing released tag `v0.0.85` must remain
  untouched. The task must update `Cargo.toml` and the corresponding
  `Cargo.lock` package entry to `0.0.86`; the new tag is created only after that
  application commit is on `main` and its required CI passes. It is not a
  pre-existing dependency, and no existing Git tag may be moved or deleted.
- Write access to the already-created empty public GitHub repository
  `nevdelap/homebrew-stay`; the application repository `nevdelap/stay` is not
  itself the tap repository. Because the tap repository currently has no commit,
  its bootstrap must create an empty `main` commit and record that commit as
  `TAP_BASE_SHA`. The bootstrap commit contains no tap files and is setup
  history, not the tap deliverable.

Scope:

- Operator boundary for implementation and release: Igor must tell Nev to
  perform every Git operation that touches GitHub and every GitHub operation for
  this task. This includes inspecting or resolving GitHub refs, cloning or
  fetching from GitHub, creating or switching branches intended for GitHub,
  committing or amending deliverables intended for GitHub, pushing commits or
  tags, creating or updating pull requests or releases, and running GitHub
  Actions or API checks. Igor must not perform any of those operations. Nev must
  record the resulting repository refs, commit SHAs, pull-request URL, release
  URL, and gate results in the task handoff.
- Execute the application-to-tap delivery in this exact order: (1) Nev creates
  the empty tap `main` bootstrap commit and records `TAP_BASE_SHA`; (2) Nev
  updates the package version from `0.0.85` to `0.0.86` in `Cargo.toml` and
  `Cargo.lock`, and implements the release workflow and application README in
  one application commit, merges or pushes it to application `main`, and records
  its SHA after required application CI passes; (3) Nev creates the new stable
  tag `v0.0.86` on that exact application commit without changing `v0.0.85`; (4)
  the tag workflow publishes the `stay 0.0.86` crate and all four binary
  archives plus `SHA256SUMS` to the GitHub Release and records the release URL
  and checksums; (5) Nev creates `task-107-homebrew` from `TAP_BASE_SHA` and
  writes the tap formula with the already-published `0.0.86` release URLs and
  checksums in one tap commit; and (6) Nev opens the tap pull request and runs
  its audit, style, install, test, and checksum gates. The formula commit must
  never be created before the release assets and checksums exist.
- This task must be completed over multiple commits and must not be squashed
  into one commit. The required commit sequence is: the empty tap bootstrap
  commit; one application commit containing the release workflow, application
  README, and the `0.0.86` package-version update, followed by application CI
  and the `v0.0.86` tagged binary release; and one later `task-107-homebrew` tap
  commit containing the formula, tap README, and tap CI. The binary publication
  is the required boundary between the application commit and the subsequent
  Homebrew commit.
- In the application repository's `.github/workflows/release.yml`, extend the
  tag-triggered release workflow with a target matrix for exactly these four
  Rust targets and asset suffixes: `aarch64-apple-darwin`,
  `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`, and
  `x86_64-unknown-linux-gnu`. Build each target with the Rust toolchain selected
  by the workflow and `cargo build --release --locked --target`. The release
  must not depend on a developer's local Rust installation.
- The release matrix must use native GitHub-hosted runners with this exact
  mapping: `aarch64-apple-darwin` on `macos-14`, `x86_64-apple-darwin` on
  `macos-15-intel`, `aarch64-unknown-linux-gnu` on `ubuntu-24.04-arm`, and
  `x86_64-unknown-linux-gnu` on `ubuntu-24.04`. Each job must assert the runner
  architecture before building: `arm64`, `x86_64`, `aarch64`, and `x86_64`,
  respectively. Install the pinned `1.88.0` toolchain with
  `dtolnay/rust-toolchain@1.88.0`, add the matrix target with
  `rustup target add --toolchain 1.88.0`, and invoke
  `cargo +1.88.0 build --release --locked --target`. These native runners
  provide the linker and execution environment, so no cross-linker, QEMU, or
  other emulation is allowed or required.
- For each target, package an archive with this exact name:
  `stay-${TAG_NAME}-${TARGET}.tar.gz`, where `TAG_NAME` includes the leading
  `v`. Each archive must contain an executable named `stay` at its top level, a
  copy of the MIT license, and the project README. Do not put a second
  architecture, a Cargo target directory, or an absolute filesystem path in an
  archive.
- Before publication, run the built binary's `--version` command and require
  exactly `stay ${VERSION}`, where `VERSION=${TAG_NAME#v}`. Run a noninteractive
  smoke test with `TMUX` unset and an isolated `TMUX_TMPDIR` that executes
  `stay list --json` and parses an empty `.sessions` array. The smoke test must
  use a real tmux 3.6-or-newer executable and must clean up any server, session,
  socket, and temporary directory it creates.
- On every release runner, install the repository's pinned tmux with
  `scripts/ci-install-tmux.sh`, then run `scripts/ci-install-tmux.sh --verify`
  before the version check and smoke test. The verified tmux executable must be
  first on `PATH` for those checks; a missing or unsupported tmux version fails
  that target job.
- Generate `SHA256SUMS` containing one SHA-256 line for each of the four
  archives, with the exact archive filenames and no unlisted binary. Publish all
  five assets to the GitHub Release for the triggering tag. The workflow must
  create the release if it does not exist, update only that same tag's release
  if it does exist, and fail rather than silently replacing an asset with a
  different checksum.
- Grant only the release job permissions required to create and upload assets,
  preserve the existing crates.io publication and verification gates, and make
  asset publication conditional on all four target builds, version checks, smoke
  tests, and checksum generation succeeding. A failed target must prevent a
  partial release.
- In the separate repository `nevdelap/homebrew-stay`, create a tap layout
  containing `Formula/stay.rb`, a tap README, and a CI workflow. The README must
  document the exact user commands `brew tap nevdelap/stay` and
  `brew install nevdelap/stay/stay`, and state that the formula downloads a
  release binary and installs tmux as a dependency.
- Treat the application and tap changes as two coordinated, separately committed
  deliverables. After the empty-repository bootstrap, create one branch named
  `task-107-homebrew` from the recorded `TAP_BASE_SHA` in
  `nevdelap/homebrew-stay`, put all tap files and tap CI changes in exactly one
  later tap commit, and open one pull request from that branch to `main`. The
  application repository's earlier task commit must contain only the release
  workflow, application README, and `0.0.86` package-version changes; it must
  record the tap pull-request URL and final tap commit SHA in its handoff. Rufus
  must review both repository diffs at those exact commits, and TASK-107 cannot
  reach `IMPLEMENTED` or `COMPLETED` until the tap pull request's audit, style,
  install, test, and checksum gates pass. Do not merge unrelated tap changes
  into that branch or squash the application and tap commits together.
- In `Formula/stay.rb`, define the formula named `stay` with a concise
  description, the HTTPS homepage `https://github.com/nevdelap/stay`, SPDX
  license `MIT`, and a formula version equal to the release version. Select the
  correct GitHub Release asset and its exact SHA-256 by platform and CPU: macOS
  ARM64 uses `aarch64-apple-darwin`, macOS Intel uses `x86_64-apple-darwin`,
  Linux ARM64 uses `aarch64-unknown-linux-gnu`, and Linux x86_64 uses
  `x86_64-unknown-linux-gnu`. Install the archive's `stay` executable into
  Homebrew's `bin` directory. Do not declare Rust or Cargo as a formula
  dependency and do not hard-code a Homebrew prefix.
- In `Formula/stay.rb`, declare `tmux` as the runtime dependency and enforce
  Stay's existing minimum of tmux 3.6 or newer. The formula test must first
  exercise the real Homebrew tmux dependency, then prepend a temporary
  executable named `tmux` that prints exactly `tmux 3.5` and require the
  installed Stay command to fail with a diagnostic containing
  `tmux 3.6 or newer`. No test may use a user's existing tmux server.
- Add the formula's `test do` block. It must unset `TMUX`, use an isolated
  temporary `TMUX_TMPDIR`, assert `stay --version` equals the formula version,
  parse `stay list --json` and require an empty `.sessions` array, create one
  short-lived named session, observe that session in the JSON inventory, kill it
  through Stay, and prove teardown leaves no session, server, socket, or
  temporary directory. The test must invoke the installed binary, not a source
  checkout.
- In the tap CI workflow, run on macOS Apple Silicon, macOS Intel, Linux ARM64,
  and Linux x86_64. Each job must run
  `brew audit --strict --new --formula nevdelap/stay/stay`,
  `brew style --formula Formula/stay.rb`, `brew install nevdelap/stay/stay`, and
  `brew test nevdelap/stay/stay`. The installation must consume the matching
  GitHub Release archive and must not install Rust, Cargo, or a compiler. The
  jobs must verify the installed asset's checksum against `SHA256SUMS` and fail
  if the formula selects the wrong platform archive.
- In this application repository's `README.md`, add the Homebrew installation
  command and state that Homebrew supplies tmux but Stay requires tmux 3.6 or
  newer. Keep the existing Cargo installation instructions and all existing
  runtime, shell-integration, and platform documentation accurate.
- Do not change application source behavior. Update only the package-version
  metadata required for this task: `Cargo.toml` and the corresponding
  `Cargo.lock` package entry must change from `0.0.85` to `0.0.86`. Do not move
  or delete `v0.0.85` or any other existing tag; the one new tag `v0.0.86`
  created by Nev is the only tag creation in this task. Do not add a
  source-build fallback to the formula or require prebuilt Homebrew bottles; the
  required distribution artifact is the target-native binary archive attached to
  the Stay GitHub Release. Future releases must update the formula's version,
  asset URLs, and checksums together with the corresponding release assets.

Acceptance criteria:

- A `v0.0.86` tag-triggered release workflow run validates that the tag's
  version exactly matches `Cargo.toml`, publishes `stay 0.0.86` to crates.io
  exactly once, uses the four exact target-to-runner mappings and native
  architecture assertions, builds all four named targets with pinned locked
  Cargo, runs the pinned tmux install and verification followed by the version
  and isolated tmux smoke tests for each target, writes exactly four archive
  entries to `SHA256SUMS`, and publishes the four archives plus `SHA256SUMS` to
  the GitHub Release for that tag. No release is marked successful when any
  target or asset is missing.
- Each published archive has the exact `stay-v<version>-<target>.tar.gz` name,
  contains exactly one top-level executable named `stay`, includes the MIT
  license and README, and runs on its declared target. The executable reports
  exactly `stay <version>`.
- On clean Homebrew installations for macOS ARM64, macOS Intel, Linux ARM64, and
  Linux x86_64, `brew tap nevdelap/stay` followed by
  `brew install nevdelap/stay/stay` completes without a Rust toolchain,
  compiler, source checkout, manual copy, symlink, PATH edit, or custom
  post-install script. The formula selects the archive matching the host
  operating system and CPU.
- The task is represented by more than one commit: the empty tap bootstrap
  commit, one application-repository commit containing the release workflow,
  application README, and `0.0.86` package-version update, and one subsequent
  `task-107-homebrew` tap commit based on the recorded empty-bootstrap
  `TAP_BASE_SHA`. The application commit's successful CI and `v0.0.86` tagged
  binary and crate release occur before the tap commit, while `v0.0.85` remains
  unchanged. The handoff names every commit SHA, the tap pull-request URL, and
  the final tap commit SHA. Rufus's review covers both exact deliverable diffs,
  and the task remains incomplete if the tap pull request or any of its required
  gates is missing.
- The handoff proves the required order: application commit SHA and successful
  CI precede the stable tag; the tagged release URL contains all four archives
  and `SHA256SUMS` before the tap commit; and the tap formula's four URLs and
  checksums equal those published release assets. A formula commit or tap pull
  request based on unpublished or later-replaced assets fails this criterion.
- The task handoff explicitly records Igor's instruction to Nev and shows that
  Nev performed every Git operation that touches GitHub and every GitHub
  operation, including the empty bootstrap, branch, commits, pull request,
  release assets, and verification checks. No Git or GitHub operation is
  attributed to Igor.
- The formula passes `brew audit --strict --new --formula nevdelap/stay/stay`
  and `brew style --formula Formula/stay.rb` without warnings. Its four URLs and
  checksums match the four assets in the tagged Stay GitHub Release and the
  `SHA256SUMS` file; the formula version matches the release version.
- `brew deps --include-build nevdelap/stay/stay` reports `tmux` and no Rust or
  Cargo dependency. The installed formula passes its version, empty-list,
  create/list/kill, cleanup, and tmux 3.6 minimum tests. A controlled tmux 3.5
  probe exits nonzero with a diagnostic containing `tmux 3.6 or newer`.
- The application README documents the exact tap/install commands, the
  target-native GitHub Release asset distribution, and the tmux 3.6 minimum. The
  existing Cargo installation path remains present and correct.
- The application repository's workflow and documentation change passes
  `just qlint`, `just qcheck`, and `just mac-qcheck`; the package-version update
  must pass the repository's locked dependency, test, and macOS gates. Every
  release target records successful `scripts/ci-install-tmux.sh` and
  `scripts/ci-install-tmux.sh --verify` gates before its binary smoke test.
- `Cargo.toml` and the `stay` package entry in `Cargo.lock` change from `0.0.85`
  to `0.0.86`; no other manifest or lockfile changes, tag moves or deletions, or
  application source behavior changes are included. The existing `v0.0.85` tag
  remains unchanged. The application repository's applicable workflow,
  package-version, and documentation quality gates and the tap repository's
  Homebrew audit, style, release-asset checksum, and four-platform install/test
  gates all pass.
