# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

## TASK-105 - make five acceptance tests prove their claims

State: COMPLETED

Goal:

- Make these five acceptance tests fail when the behavior in their names is
  removed. Each test must observe the named behavior directly, not infer it from
  a weaker inventory, metadata, or successful-relay result.

Dependencies:

- None.

Scope:

- In `tests/acceptance.bats`, update only the test
  `stay create uses the configured default command` so its child command
  provides direct evidence that the configured default ran. Its existing create,
  detached-list, JSON-list, and cleanup flow remains in scope.
- In `tests/acceptance.bats`, update only the test
  `stay create starts the session in the requested directory` so its child
  process provides direct evidence of its working directory. Its existing
  canonicalization, create, and JSON inventory flow remains in scope.
- In `tests/acceptance.bats`, update only the test
  `stay create --force-recreate replaces an existing session` so the live
  replacement is process-observable. Its existing live collision and
  terminated-session warning/replacement branches remain in scope.
- In `tests/acceptance.bats`, update only the test
  `stay create --attach --low-priority attaches at low priority` so it observes
  the low-priority client state in addition to its existing PTY behavior.
- In `tests/acceptance.bats`, update only the test
  `stay attach --low-priority uses the low-priority client mode` so it observes
  the low-priority client state in addition to its existing PTY behavior, and
  remove any redundant attachment wait from that test.
- In `tests/helpers/acceptance_tmux.bash`, add only the bounded,
  session-specific tmux client-state polling needed by the two low-priority
  tests. Export that helper from the acceptance suite setup in
  `tests/acceptance.bats`.
- Do not modify `design_docs/acceptance_review.html`; it is the review input for
  this task and must remain uncommitted.
- Do not weaken existing lifecycle, metadata, relay, or detach assertions and do
  not change production behavior.

Acceptance criteria:

- For `stay create uses the configured default command`, set `STAY_CMD` to a
  shell command that writes the child PID to a unique marker file and then
  executes `sleep 60` (the command must use `exec` so the recorded PID is the
  live sleeping process). Wait until that file is present, create without
  trailing command words, and assert detached text and JSON inventory state plus
  the exact JSON field `"current_command":"sleep"`. A successful create or
  list-only observation is insufficient.
- For `stay create starts the session in the requested directory`, run a child
  command equivalent to `pwd > MARKER`, with `MARKER` outside the requested
  directory. Pass the canonical physical path through `--cwd`, wait for the
  marker, and assert its trimmed content equals that canonical path. Also retain
  the JSON `current_directory` assertion as a separate metadata check.
- For `stay create --force-recreate replaces an existing session`, cover both a
  live and an already-terminated session. In the live branch, make the original
  child execute `sleep 60` after writing its PID to an original marker file;
  force-recreate with a distinct command that executes `sleep 60` after writing
  a replacement PID to a different marker file; and wait with bounded polling
  until the replacement marker exists. Prove the recorded PIDs differ,
  `kill -0 OLD_PID` fails after the replacement, and `kill -0 NEW_PID` succeeds
  while the replacement is expected to remain alive. Assert the live
  replacement's JSON `current_command` is exactly `"sleep"`. In the terminated
  branch, retain the prior exit-code warning and prove the replacement is
  detached with JSON `"current_command":"sleep"`.
- For `stay create --attach --low-priority attaches at low priority`, retain the
  real PTY attach, input/output relay, and clean-detach assertions. While
  attached, observe this session's tmux client and require its supported
  `ignore-size` client flag, which is the flag stay uses for low priority.
  Normal attach success alone must not satisfy the test.
- For `stay attach --low-priority uses the low-priority client mode`, retain the
  existing attach, input/output, and clean-detach assertions. While attached,
  make the same direct `ignore-size` client-state assertion. Use supported tmux
  client metadata; a competing-client scenario is not required. There must be
  exactly one `pty_wait_until_attached` call; retain the separate child-output
  readiness wait only to synchronize the fixture, not as evidence of client
  priority.
- For both low-priority tests, the client-state observation must be bounded and
  session-specific. Use the acceptance server namespace (`tmux -L stay`),
  validate the controlled socket-root environment, and query exactly
  `tmux -L stay -f /dev/null list-clients -F '#{client_session}:#{client_flags}'`.
  Identify the row whose `#{client_session}` equals the test's target session
  and require that row's `#{client_flags}` contains the `ignore-size` token.
  Matching an unrelated client, merely seeing any client, or accepting a
  different flag is insufficient.
- Any new process/file/client polling must have bounded timeout diagnostics. Do
  not add arbitrary sleeps, broaden matches, suppress failures, or alter inputs
  merely to make a test pass. Preserve all existing assertions unless this
  specification explicitly adds a stronger replacement.
- `just qacceptance` and `just mac-qacceptance` pass for the final diff.

## TASK-106 - strengthen nine covered acceptance tests

State: COMPLETED

Goal:

- Make the nine acceptance tests currently marked `Covers claim; improve` in
  `design_docs/acceptance_review.html` prove their named behavior with complete
  and deterministic evidence. Preserve the existing end-to-end behavior checks
  while closing the specific evidence gaps identified by the review.

Dependencies:

- TASK-105 - make five acceptance tests prove their claims (must reach
  `COMPLETED` before this task begins, because this task builds on the current
  acceptance fixtures and helper conventions).

Scope:

- In `tests/acceptance.bats`, improve only these nine reviewed tests and the
  shared fixture/assertion code they directly require:
  `stay attach --log captures clean output across attaches`,
  `stay logging handles history and capture boundaries`,
  `stay create --attach --read-only prevents input changes`,
  `stay attach --read-only prevents mutating input`,
  `stay rejects invalid arguments and session names`,
  `stay rejects conflicting options`,
  `stay list shows the session inventory as human-readable rows`,
  `stay list --json emits a stable machine-readable inventory`, and
  `stay shell-integration prints the prompt snippet`.
- In `tests/helpers/acceptance_pty.bash`, extend the bounded absence-wait
  interface needed by the two read-only tests with the exact optional form
  `pty_wait --absent MARKER --attempts N`; each attempt polls once, then waits
  100 ms, and the helper must print the marker and PTY transcript on timeout. Do
  not add fixed sleeps or unbounded polling. Existing callers without
  `--attempts` may retain the current default.
- In `tests/helpers/acceptance_tmux.bash`, add only the bounded,
  socket-root-validated raw session/client snapshot and exact-session absence
  assertions needed by the invalid-name and conflicting-option tests. Keep all
  direct `tmux -L stay` inspection behind these helpers and include sessions,
  attachment state, and client rows in the snapshot.
- In `tests/acceptance.bats`, shared inventory-fixture or JSON-helper changes
  are in scope only when they support the two inventory tests above. The JSON
  helper must stop manually splitting serialized objects and must use `jq` (or
  an equivalently real JSON parser supplied by the repository).
- No production behavior changes are part of this task. Do not modify
  `design_docs/acceptance_review.html`; that review input is intentionally
  untracked and must remain uncommitted.
- Keep all existing lifecycle, relay, logging, error, inventory, startup-file,
  and cleanup assertions unless a criterion below explicitly strengthens that
  assertion. Do not weaken a check, replace an observable marker with a sleep,
  or use unquoted argument-string expansion.

Acceptance criteria:

- For `stay attach --log captures clean output across attaches`, retain the two
  real PTY attaches, detach boundary, clean-capture/no-ANSI assertion, and mode
  `0600` assertion. Assert the complete fixture marker set in the primary log:
  `retained-marker`, `ready`, `periodic-marker`, every `filler-00` through
  `filler-39`, and `visible-marker`. Each expected line occurs exactly once and
  the marker lines occur in fixture order; no unexpected nonempty marker line is
  accepted. Assert the `.offset` sidecar exists, has mode `0600`, has exactly
  the six cursor fields `session`, `log_size`, `line_count`, `partial`,
  `marker_bytes`, and `anchor`, and has values that are internally valid: the
  session is the target session, `log_size` equals the primary log byte size,
  `line_count` and `marker_bytes` are decimal, `partial` is `0` or `1`, and
  `anchor` is `none` or lowercase even-length hexadecimal. Assert the
  `.offset.tmp` path is absent after each completed capture.
- For `stay logging handles history and capture boundaries`, retain the
  more-than-64-KiB fixture and all three recovery cases: missing sidecar,
  malformed sidecar, and a sidecar whose session does not match. Count selected
  early and tail markers (at minimum `large-0000`, `large-0010`, `large-2990`,
  `large-2999`, and `visible-boundary`) in each capture result so a gap or
  duplicate cannot pass. For the initial capture, each selected marker is
  present exactly once and in order. Before each recovery attach, record the
  primary-log length; inspect only the newly appended suffix for that attach and
  require each selected marker exactly once and in order. The missing-sidecar
  suffix must contain no eviction marker, while malformed and mismatched-cursor
  suffixes must contain the documented `--- history evicted before capture ---`
  marker exactly once before their recovered selected-marker sequence. After
  every recovery attach, assert the sidecar is mode `0600`, contains the exact
  six-field cursor format (`session`, `log_size`, `line_count`, `partial`,
  `marker_bytes`, `anchor`), identifies the target session, and has a log size
  equal to the current log. Assert the expected warning or recovery marker for
  every corruption case, rather than checking only that a sidecar file exists.
- For `stay create --attach --read-only prevents input changes`, make the child
  print a `read-pending` marker immediately before it blocks in its `read` loop.
  Wait for that child-side marker before sending input. Send a nonempty
  distinguishable line, then invoke exactly
  `pty_wait --absent "received=" --attempts 50`. This is a five-second bounded
  observation (50 polls at 100 ms) and is the interval used to rule out delayed
  relay; the helper must fail with its marker and transcript diagnostics if the
  line appears. Then send the detach control input and require a clean wrapper
  exit and detached session. The test must prove both that ordinary input is not
  relayed and that detach remains the allowed control input.
- For `stay attach --read-only prevents mutating input`, use the same
  `read-pending` synchronization and the exact same
  `pty_wait --absent "received=" --attempts 50` five-second negative assertion
  instead of a timing-only readiness assumption. After the read-only attach
  detaches, start a later normal writable attach to the same live session, wait
  for `read-pending`, send a different nonempty line, and require the child to
  emit the corresponding `received=` line. Detach cleanly and verify the session
  is detached. This must prove both non-mutation during the read-only attach and
  continued usability by a writable attach.
- For `stay rejects invalid arguments and session names`, invoke every argument
  case through Bash arrays and `run ... stay "${args[@]}"`; no unquoted `$args`
  expansion or shellcheck suppression for word splitting is allowed. Retain the
  unknown/missing command cases, dotted-name rejection, and 129-`界` rejection
  with their usage status and diagnostics. Add a 128-`界` name and require it to
  be accepted, listed, and cleaned up. Add a legal ordinary-space name such as
  `name with space` and require the same create/list/cleanup behavior. Add
  representative rejected names containing a tab, a newline, and a
  Unicode-invalid format/bidi character (U+2028 or U+202E), with usage status
  and the relevant validation diagnostic. After every rejected create/name case,
  assert the JSON inventory is empty and no tmux/session artifact for that
  candidate exists; accepted boundary cases are checked only after those
  empty-inventory assertions.
- For `stay rejects conflicting options`, table-drive the complete conflict
  matrix using argument arrays. It must include: create `--read-only`,
  `--low-priority`, and their combination without `--attach`; attach
  `--truncate`, `--raw`, and their combination without `--log`; `--pass-through`
  paired with `--read-only`, `--low-priority`, `--log`, `--log --raw`, and
  `--log --truncate`; the relevant pairings of `--pass-through` with both client
  modifiers; and the existing top-level `--no-alt-screen`/subcommand,
  `--prompt-integration`/subcommand, and
  `--prompt-integration`/`--no-alt-screen` conflicts. Include repeated forms for
  every boolean/log modifier exercised by the matrix (`--read-only`,
  `--low-priority`, `--truncate`, `--raw`, `--pass-through`, and `--log`),
  including repeated `--log` values. For every rejected matrix row, assert
  status `2`, empty stdout, the specific usage/conflict diagnostic, and no
  session, client, log, or other side effect; the pre-existing keeper session
  must remain unchanged. If a repeated flag is accepted by the parser rather
  than rejected as a conflict, assert that documented parser result explicitly
  and still require no unintended side effect. No case may depend on globbing or
  word splitting.
- For `stay list shows the session inventory as human-readable rows`, retain the
  six-state fixture and no-ANSI assertion. Split stdout into rows and assert
  exactly six rows, in the fixture's documented inventory order, with no extra
  or missing row. Require exact detached and attached row shapes, and require
  terminated rows to contain exit `7` or signal `15` as appropriate with a
  complete UTC timestamp matching exactly `YYYY-MM-DDTHH:MM:SSZ` (a four-digit
  year, two digits for every month/day/hour/minute/second component, and a
  literal trailing `Z`); a broad `.*Z` timestamp match is not sufficient.
- For `stay list --json emits a stable machine-readable inventory`, retain the
  same six-state fixture and replace delimiter splitting (`sed 's/},{/}\\n{/g'`
  or an equivalent approach) with parsing of the complete stdout through
  `jq -e`. Assert the root/object and `.sessions/array` types, exact array
  length `6`, fixture order, and exact type/value contracts for every lifecycle
  object: detached and attached rows have string `current_directory` and
  `current_command == "sleep"` with null termination fields; exit-7 and
  signal-15 rows have null `current_directory`, `current_command == "sh"`, a
  timestamp in the complete UTC shape, and only their corresponding exit/signal
  value. Assert all `created_at` and `terminated_at` timestamps against the same
  exact shape. Include one legal fixture `--cwd` path containing JSON-escaped
  characters (a quote and a backslash), and assert through `jq` that the decoded
  `current_directory` equals the original path; escaped content must not confuse
  the helper.
- For `stay shell-integration prints the prompt snippet`, retain the startup
  sentinel files and the assertion that the command does not edit them. Run
  `stay shell-integration` once with `TMUX` unset and once with
  `TMUX=simulated`; for both invocations require status `0`, empty stderr, and
  the exact same snippet as `stay --prompt-integration`. Write that returned
  snippet to a file and source it, without output or shell errors, in each
  supported shell: `sh`, `bash`, and `zsh`. In each shell call
  `stay_prompt_segment` with `STAY_SESSION_NAME` unset and with a nonempty name,
  and assert the documented empty and `[name] ` results. Recheck every startup
  sentinel after all invocations.
- Any new polling or absence observation is bounded and prints useful
  diagnostics on timeout; no arbitrary fixed sleep is introduced. The exact
  applicable gates for the final acceptance-layer diff, `just qacceptance` and
  `just mac-qacceptance`, pass.

## TASK-107 - publish prebuilt Stay binaries for Homebrew

State: NEW

Goal:

- Make users on macOS and Linux able to install Stay through the dedicated
  Homebrew tap without compiling Stay locally. A tagged Stay release must
  publish target-native binary archives to its GitHub Release, and the tap
  formula must select the matching archive for the user's operating system and
  CPU architecture while installing tmux as the runtime dependency.

Dependencies:

- Nev, acting as the human release owner, must have authority to merge or push
  the application workflow commit and create one stable tag whose name is
  `v<major>.<minor>.<patch>` and whose version matches the package version in
  `Cargo.toml`. The tag is created only after the application workflow commit is
  on `main` and its required CI passes; it is not a pre-existing dependency.
  This task must not bump the package version or move or delete a Git tag.
- Write access to the already-created empty public GitHub repository
  `nevdelap/homebrew-stay`; the application repository `nevdelap/stay` is not
  itself the tap repository. Because the tap repository currently has no commit,
  its bootstrap must create an empty `main` commit and record that commit as
  `TAP_BASE_SHA`. The bootstrap commit contains no tap files and is setup
  history, not the tap deliverable.

Scope:

- Operator boundary for implementation and release: Igor must tell Nev to
  perform every Git and GitHub operation for this task. This includes repository
  inspection, clone/fetch, branch creation, commit/amend, push, pull-request
  creation or update, tag/release handling, and GitHub Actions or API checks.
  Igor must not perform those operations. Nev must record the resulting
  repository refs, commit SHAs, pull-request URL, release URL, and gate results
  in the task handoff.
- Execute the application-to-tap delivery in this exact order: (1) Nev creates
  the empty tap `main` bootstrap commit and records `TAP_BASE_SHA`; (2) Nev
  implements the release workflow and application README in one application
  commit, merges or pushes it to application `main`, and records its SHA after
  required application CI passes; (3) Nev creates the stable tag on that exact
  application commit; (4) the tag workflow publishes all four archives and
  `SHA256SUMS` to the GitHub Release and records the release URL and checksums;
  (5) Nev creates `task-107-homebrew` from `TAP_BASE_SHA` and writes the tap
  formula with the already-published release version, URLs, and checksums in one
  tap commit; and (6) Nev opens the tap pull request and runs its audit, style,
  install, test, and checksum gates. The formula commit must never be created
  before the release assets and checksums exist.
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
- Treat the application and tap changes as two coordinated deliverables. After
  the empty-repository bootstrap, create one branch named `task-107-homebrew`
  from the recorded `TAP_BASE_SHA` in `nevdelap/homebrew-stay`, put all tap
  files and tap CI changes in exactly one tap commit, and open one pull request
  from that branch to `main`. The application repository's task commit must
  contain the release workflow and application README changes only; it must
  record the tap pull-request URL and final tap commit SHA in its handoff. Rufus
  must review both repository diffs at those exact commits, and TASK-107 cannot
  reach `IMPLEMENTED` or `COMPLETED` until the tap pull request's audit, style,
  install, test, and checksum gates pass. Do not merge unrelated tap changes
  into that branch.
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
- Do not change application source behavior or the package version, and do not
  move or delete the stable tag created on the application commit above. That
  one tag creation by Nev is the only tag operation in this task. Do not add a
  source-build fallback to the formula or require prebuilt Homebrew bottles; the
  required distribution artifact is the target-native binary archive attached to
  the Stay GitHub Release. Future releases must update the formula's version,
  asset URLs, and checksums together with the corresponding release assets.

Acceptance criteria:

- A tag-triggered release workflow run validates that the tag's version exactly
  matches `Cargo.toml`, uses the four exact target-to-runner mappings and native
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
- The application change is present in one application-repository commit and the
  tap change is present in one `task-107-homebrew` commit based on the recorded
  empty-bootstrap `TAP_BASE_SHA`; the handoff names that SHA, the tap pull
  request URL, and the final tap commit SHA. Rufus's review covers both exact
  diffs, and the task remains incomplete if the tap pull request or any of its
  required gates is missing.
- The handoff proves the required order: application commit SHA and successful
  CI precede the stable tag; the tagged release URL contains all four archives
  and `SHA256SUMS` before the tap commit; and the tap formula's four URLs and
  checksums equal those published release assets. A formula commit or tap pull
  request based on unpublished or later-replaced assets fails this criterion.
- The task handoff explicitly records Igor's instruction to Nev and shows that
  Nev performed every Git and GitHub operation, including the empty bootstrap,
  branch, commits, pull request, release assets, and verification checks. No Git
  or GitHub operation is attributed to Igor.
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
  `just qlint`, and every release target records successful
  `scripts/ci-install-tmux.sh` and `scripts/ci-install-tmux.sh --verify` gates
  before its binary smoke test.
- `Cargo.toml` and `Cargo.lock` remain byte-for-byte unchanged, no package
  version changes, no tag moves, and no application source behavior changes are
  included. The application repository's applicable workflow/documentation
  quality gates and the tap repository's Homebrew audit, style, release-asset
  checksum, and four-platform install/test gates all pass.
