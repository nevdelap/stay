# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

Completed task entries are removed from this active plan; their history is
preserved in git (the task commit and its `Reviewed:` section). Add new work as
the next stable task entry; do not reuse an identifier from a removed task.

## TASK-093 - remove GitHub Actions warning noise

State: COMPLETED

Goal:

- Make the CI run's warnings actionable and make the default-parallel local
  verification reliable by removing the project-controlled GitHub Actions
  deprecation, cache-glob, Homebrew tap, and macOS compiler warnings identified
  in PR 31's CI run, then hardening the real-tmux and PTY lifecycle handling
  that makes the full gate flaky. Stay must require tmux 3.6 or newer because
  tmux 3.4 can permanently lose retained-pane death metadata during concurrent
  exits.

Dependencies:

- TASK-086.

Scope:

- `.github/workflows/ci.yml`: update `actions/checkout` to the Node 24 `v5`
  action and `astral-sh/setup-uv` to the Node 24 `v7` action in the CI jobs,
  eliminating the Node 20 deprecation annotations. Disable uv caching in the
  `check` and `lint-all` jobs because this repository has no Python dependency
  manifest for the action's default cache glob; do not change the Rust cache.
  Remove the macOS runner's untrusted `aws/tap` warning by best-effort untapping
  that runner-provided tap immediately before installing tmux and zsh; the
  command must remain safe when the tap is absent.
- `tests/session_creation.rs`: cfg-gate the `test_tmux_tmpdir` import to Linux,
  matching its only Linux-gated use and removing the macOS unused-import
  warning. Do not suppress compiler warnings globally.
- `src/tmux.rs`: keep real-tmux test namespaces isolated with the shared
  per-process socket root and unique namespaces; do not add a process-wide or
  test-thread serialization lock. The supported tmux floor removes the need for
  the former 3.4 metadata workaround while preserving normal parallelism.
- `src/tmux_version.rs`, `README.md`, and `design_docs/stay.html`: enforce and
  document tmux 3.6 as the minimum supported version, distinguishing the 3.3
  feature floor from the 3.6 reliability floor.
- `tests/tmux_inventory.rs`: use a renamed shell executable for dynamic-field
  fixtures so the test remains live on systems where `/bin/sleep` is a
  multi-call coreutils binary. Keep real-tmux coverage for both
  `pane_current_path` and `pane_current_command`; assert command presence rather
  than a platform-specific process basename, while parser tests retain exact
  colon/control-character decoding assertions.
- `tests/`, `src/`, and `design_docs/known_issues.md`: investigate the open
  flakiness records for the picker panic PTY test and real-tmux termination
  tests, fix confirmed isolation or synchronization defects without weakening
  assertions, and record only verified resolutions. The TASK-068 maintainer
  deferral is in scope for investigation now that the release has passed.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` and every version assertion.

Acceptance criteria:

- The CI workflow uses Node 24 action versions, does not request an unused uv
  dependency cache, and safely removes the untrusted runner tap before the macOS
  package install.
- `cargo test --locked --all-targets --all-features` produces no unused-import
  warning on macOS, and the existing test behavior is unchanged.
- The real-tmux dynamic-field fixtures use renamed shells and verify both
  current-directory values and non-empty current-command values on Linux and
  macOS; parser tests verify exact colon/control-character round trips.
- The PR CI run no longer emits the eight project-controlled warning annotations
  identified in run 31065888512, apart from warnings originating solely in
  GitHub's hosted runner or action internals.
- `just qcheck`, `just qcheck-all`, and the exact `just mac-qcheck` pass with
  the repository's normal parallel test settings.
- The default-parallel `just qcheck-all` passes five consecutive runs, and each
  named open or previously flaky test passes twenty consecutive targeted
  repetitions under its normal test runner; no test-thread serialization or
  assertion weakening is used as a workaround.
- `design_docs/known_issues.md` records evidence-backed resolutions for any
  issues fixed by this task and leaves any unreproduced issue explicitly open.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` and every version assertion.

## TASK-094 - install the supported tmux version in CI

State: IMPLEMENTED

Goal:

- Make CI run its tmux-dependent tests with the supported tmux release instead
  of the runner image's unpinned package. This task must be independently
  implementable on `main`; it must not depend on TASK-093 being merged or on any
  TASK-093 source, test, or documentation change being present.

Dependencies:

- None. The workflow fix must apply cleanly to the current `main` baseline.

Scope:

- `.github/workflows/ci.yml` and `scripts/ci-tmux.sh`: update every job that
  runs tmux-dependent tests (`check`, `stable`, and `macos`) to install the
  official tmux 3.6 release explicitly. Ubuntu's `apt` package must not be
  trusted because the `ubuntu-latest` image's repository may provide tmux 3.4;
  the helper must build the pinned release, verify its SHA-256, put that binary
  first on `PATH`, and print/fail on the same 3.6 minimum check on Linux and
  macOS. Invoke its verification-only mode immediately before each test command.
  Keep the existing `ripgrep`, `zsh`, Rust, and cache setup. The `msrv` and
  `lint-all` jobs do not need tmux installation because they do not run
  tmux-dependent tests.
- Preserve the workflow's existing job boundaries, timeout settings, warning
  fixes, and test commands. Do not serialize tests or weaken test assertions to
  accommodate the installation change.
- Increment the patch version exactly once from this task's baseline and update
  `Cargo.lock` and every version assertion, even though the product behavior is
  unchanged.

Acceptance criteria:

- A clean checkout of the current `main` branch can apply and run this task
  without TASK-093 being merged first.
- `check`, `stable`, and `macos` each report tmux 3.6 or newer immediately
  before their tmux-dependent tests; no job uses Ubuntu's tmux 3.4 package or an
  unverified Homebrew version.
- The workflow continues to install all tools required by its existing steps,
  and the normal test parallelism and timeout settings are unchanged.
- Workflow YAML and shell checks pass, and the exact local `just qcheck` and
  `just mac-qcheck` recipes pass on the task baseline.
- The relevant CI jobs pass with the explicit tmux installation and version
  check, including the full test commands already used by each job.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion.

## TASK-085 - treat an empty file default_command as unset

State: COMPLETED

Goal:

- Fix external-review M5: a `default_command = ""` in the config file yields
  `Some("")`, so every `stay create <name>` without an explicit command runs
  `sh -c ""` and exits immediately, whereas the `STAY_CMD` env override already
  normalizes empty to unset.

Dependencies:

- None.

Scope:

- `src/config.rs` (around line 109): the file value is taken verbatim
  (`.or(file.default_command)`) with no empty filter, unlike every `STAY_*` env
  override, which passes through `non_empty_environment_value`. Filter an empty
  file `default_command` to `None`
  (`.or(file.default_command.filter(|value| !value.is_empty()))`) so an empty
  file value behaves as unset, matching the env path. Do not change the handling
  of a non-empty file value or of any other config key.

Acceptance criteria:

- A unit test proves `default_command = ""` in the file resolves to `None` (no
  default command), matching `STAY_CMD=""`.
- Existing config precedence and empty-vs-unset tests still pass.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` and every version assertion; `just qcheck` and `just mac-qcheck`
  both pass.

## TASK-086 - close CI coverage gaps

State: COMPLETED

Goal:

- Close the CI enforcement holes identified by external-review M8-M12: the
  quality engine's own tests run in no gate, the release build/profile is never
  exercised, whole-tree non-Rust and debugging-macro enforcement is missing, the
  `stable` job lacks tmux/zsh, and the dispatcher mis-classifies future
  `scripts/` files.

Dependencies:

- None.

Scope:

- `.github/workflows/ci.yml`: add a step that runs the quality dispatcher's own
  test suite (`uv run --script scripts/test_quality.py`), so a regression in
  `quality.py`'s selection, classification, or diagnostic filtering is caught;
  it needs `cargo` and `git`, which the `check` job already provides (M8).
- `.github/workflows/ci.yml`: add a
  `cargo build --release --locked --all-features` step and a
  `cargo publish --locked --dry-run` step (both in the `check` job, which
  already has the full Rust toolchain, `cargo`, and `git`) so release-profile
  and packaging breakage is caught in CI rather than at manual publish time.
  These are raw `cargo` invocations that deliberately bypass the CI block on the
  operator-only `just publish` recipe (M9).
- `.github/workflows/ci.yml`: add a dedicated whole-tree lint job that runs
  `just lint all`, so the stray-debugging-macro guard and the
  yaml/markdown/toml/json/shell/docker linters - which run only in changed scope
  today - catch a violation in a file outside a pull request's diff (M10). Two
  behaviours of `just lint all` the job must account for: (1) it first runs
  `just format all` then `_assert-clean-worktree`, so this job is also a
  whole-tree *format* gate - any formatting drift anywhere in the tree fails it,
  which is intended; (2) it runs whole-tree clippy via
  `_lint_rust(all_files=True)`, overlapping the existing standalone whole-tree
  clippy step (`ci.yml:37`) - leave that step in place as fast Rust-only
  feedback and do not remove it; the overlap is acceptable and keeps overall
  coverage unchanged.
- `.github/workflows/ci.yml`: the `stable` job runs the tmux-dependent
  integration suite without installing tmux/zsh, relying on runner defaults; add
  the same `apt-get install --yes tmux zsh` step the `check` job uses (M11).
- `scripts/quality.py` (line 181): `classify()`'s bash-routing clause is
  currently
  `(suffix == ".sh" or path.startswith("scripts/")) and suffix != ".py"`, which
  routes any non-`.py` file under `scripts/` to bash tooling and would
  mis-handle a future `scripts/*.yaml`/`.toml`/`.json`/`.md`/`Dockerfile`.
  Restrict only the `scripts/` disjunct to extensionless-or-`.sh` files while
  preserving `.sh`-anywhere routing - for example
  `elif suffix == ".sh" or (path.startswith("scripts/") and suffix in {"", ".sh"}):`
  (`suffix` is already `Path(path).suffix.lower()`). Do not collapse this to
  `Path(path).suffix in {"", ".sh"}` alone: that would stop `.sh` files outside
  `scripts/` from reaching bash tooling (M12).
- Newly-enforced whole-tree gates may surface pre-existing violations in files
  outside any recent diff (a stray `dbg!`/`println!`, an unformatted or unlinted
  yaml/markdown/toml/json/shell/docker file, or release-only breakage). Bringing
  the current tree clean under the new gates is in scope for this task; do not
  disable, narrow, or `allow` a gate to get it green. The whole-tree lint job
  needs the same toolchain the changed-scope `check` job installs (`just`, `uv`,
  `ripgrep`) plus Docker with network egress for the images the non-Rust linters
  pull at runtime: `jq`, `shfmt`, and `shellcheck`, and also `hadolint`
  (Dockerfile), `actionlint` (workflow YAML), and `gitlint` (commit message).
  `ubuntu-latest` provides Docker; the job must install `just`, `uv`, and
  `ripgrep` exactly as the `check` job does.

Acceptance criteria:

- CI runs `test_quality.py`, builds the release profile and runs the publish dry
  run, enforces the debugging guard and non-Rust linters whole-tree via
  `just lint all`, and installs tmux/zsh in the `stable` job; each new step
  passes on the current tree.
- `scripts/quality.py` classifies a `scripts/*.toml` (or similar non-shell) file
  to its real linter rather than bash, covered by a new `test_quality.py` case.
- `just qcheck`, `just qcheck-all`, and the exact `just mac-qcheck` still pass.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` and every version assertion.

## TASK-087 - correct documentation drift

State: NEW

Goal:

- Fix documentation that misdescribes shipped behaviour: `stay.html` and
  `lessons_learned.md` still specify local-time termination timestamps though
  the code emits UTC (M6); the README omits pass-through and shell/prompt
  integration (L4); and `stay.html`'s picker status line is stale versus the
  implemented one (L6).

Dependencies:

- None. The signal-row spec text (L5) is handled by TASK-083; this task covers
  only the non-signal documentation drift.

Scope:

- `design_docs/stay.html` and `design_docs/lessons_learned.md`:
  `format_dead_time` now renders UTC (RFC 3339 `Z`) via `format_utc_timestamp`.
  Update the terminated-row spec (`@<local time>` to a UTC/`Z` example) and
  rewrite or remove the local-offset lesson (the `UtcOffset::local_offset_at`
  and DST-boundary guidance), which describes a mechanism no longer in the code
  (M6).
- `README.md`: add coverage for the attach `-p/--pass-through` flag, create's
  `-r/--read-only` and `-L/--low-priority`, and a "Shell integration" subsection
  documenting `stay shell-integration [--s-alias]` and
  `stay --prompt-integration` (L4). Note the README already documents attach's
  `--read-only`/`--low-priority` (long forms only, around README lines 57-58)
  but not their short forms and not `-p/--pass-through`; create's `-r/-L` are
  absent entirely. Add the missing flags and short forms without duplicating the
  existing attach descriptions.
- `design_docs/stay.html` (around lines 1291-1295): update the picker
  status-line text to match the implemented `IDLE_STATUS` (which lists
  `c create`, `K kill all terminated`, and `q/Esc`) and remove the stale "`c` is
  not listed" note (L6).

Acceptance criteria:

- No doc claims local-time termination timestamps; the terminated-row and JSON
  timestamp descriptions match the UTC output.
- The README documents pass-through, create `-r/-L`, and shell/prompt
  integration accurately against the current CLI surface.
- The `stay.html` picker status line matches the implemented `IDLE_STATUS`.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` and every version assertion; `just qcheck` and `just mac-qcheck`
  both pass.

## TASK-088 - session, name, and version-probe hardening

State: NEW

Goal:

- Three small correctness/robustness fixes: force-recreate validates the name
  only after invoking tmux (L1), session-name validation accepts Unicode
  line/bidi control characters (L2), and the version-probe timeout message
  hardcodes "2 seconds" independently of the actual timeout (T1).

Dependencies:

- None.

Scope:

- `src/session.rs` (`force_recreate_session_inner`, around lines 195-219):
  validate the name with `crate::session_name::parse_session_name` as the first
  line, before `list_sessions()` or any notice, so force-recreate matches the
  "reject an invalid name before running tmux" invariant the other three session
  APIs uphold (L1).
- `src/session_name.rs` (around lines 67-84): validation rejects only `.`/`:`
  and ASCII control bytes, so Unicode line/paragraph separators (U+2028/U+2029),
  NEL (U+0085), and bidi overrides (U+202A-202E, U+2066-2069) pass and can
  corrupt or spoof the picker/status-line display. Reject `char::is_control()`
  plus those line/paragraph/bidi format characters, and update the doc comment
  to describe the broadened rule (L2).
- `src/tmux_version.rs` (around line 80): the "timed out after 2 seconds"
  message hardcodes the duration though the timeout is a parameter; interpolate
  the actual timeout so it cannot go stale (T1). Do not use `timeout.as_secs()`:
  the test path uses a 20 ms timeout, which truncates to "0 seconds". Format the
  `Duration` so a sub-second timeout still reads correctly (for example the
  `Debug` form, "timed out after 20ms", or a millisecond value), and assert the
  configured duration on the short-timeout test path.

Acceptance criteria:

- A test proves `force_recreate_session`/`_for_picker` reject an invalid name
  before invoking tmux, extending the existing before-tmux-validation test to
  this path.
- A test proves a name containing U+2028 (or a bidi override) is rejected with
  the disallowed-character error, and the existing precedence (disallowed
  character before length) still holds.
- The version-probe timeout message reflects the configured timeout, asserted on
  the short-timeout test path.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` and every version assertion; `just qcheck` and `just mac-qcheck`
  both pass.

## TASK-089 - picker input bound, empty-state text, and render scan

State: NEW

Goal:

- Three low-risk picker fixes: bound the escape-sequence accumulator (L7),
  correct the empty-state status text that advertises "Enter attach" when Enter
  creates (L8), and remove the per-row O(n) `selected_index` recomputation in
  the render loop (L9).

Dependencies:

- None.

Scope:

- `src/picker/mod.rs` (`InputReader::escape_or_quit`, around lines 1933-1942):
  the CSI-collection loop has no length cap, unlike the module's other byte
  collectors (32-byte guards at lines 168 and 4683, which share only the 32-byte
  threshold - their actions differ: a `break` and an emulator resync
  respectively). Cap the collected sequence length at 32 bytes and return
  `PickerKey::Other` on overflow (a second `PickerKey::Other` path alongside the
  existing timeout/EOF one), so a long parameter run cannot grow the buffer
  unbounded (L7).
- `src/picker/mod.rs` (`EMPTY_STATUS`, line 31): with zero sessions there is
  nothing to attach and Enter opens the create prompt, so "Enter attach" is
  misleading. Change the empty-state hint to describe the actual empty-state
  keys (for example "c create" and "Enter create"), and update the exact-text
  status test (L8).
- `src/picker/mod.rs` (render loop, around lines 1418-1438): `selected_index()`
  (an O(n) name scan) is called once per visible row each frame. Compute it once
  before the loop and compare row indices against the cached value; no behaviour
  change (L9).

Acceptance criteria:

- A test proves an over-long escape sequence resolves to `PickerKey::Other`
  without unbounded accumulation.
- The empty-state status text matches the empty-state key behaviour, asserted by
  the status-text test.
- The render loop computes the selected index once per frame, with the existing
  render tests still passing.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` and every version assertion; `just qcheck` and `just mac-qcheck`
  both pass.

## TASK-090 - make the disappearing-selection picker test deterministic

State: NEW

Goal:

- Fix external-review L11:
  `picker_clears_selection_when_the_selected_session_disappears` uses fixed
  sleeps with a nulled stdout, so on a loaded runner its keystrokes can land
  before the picker is ready and its "nothing attached" assertion passes
  vacuously.

Dependencies:

- None.

Scope:

- `tests/attachment.rs` (around lines 2035-2067): the test sets the child's
  stdout to `Stdio::null()` and gates keystrokes on fixed `thread::sleep` delays
  rather than observed picker state. Capture the picker's stdout (into the
  shared `Arc<Mutex<Vec<u8>>>` reader the sibling picker tests use) and gate
  each keystroke on an observed readiness condition via
  `wait_for_output_contains`: wait for the initial picker render (both session
  names visible) before the first keystroke, and wait for the picker to redraw
  without the killed session's name before pressing Enter, so the keystrokes
  cannot land before the picker has processed the selection's disappearance.
  Keep the existing negative assertion (nothing is attached), and add a positive
  assertion that proves the picker actually re-rendered after the session
  vanished - for example that the observed output contains the surviving
  session's name but no longer contains the killed session's name at the point
  Enter is pressed. Do not weaken what the test verifies; the
  disappearing-selection behaviour must still be exercised end-to-end.

Acceptance criteria:

- The test drives the picker through observed readiness with no fixed pre-input
  sleeps, and fails if the picker never renders or the disappearing-selection
  handling regresses.
- The test passes reliably under the real-tmux/PTY harness.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` and every version assertion; `just qcheck` and `just mac-qcheck`
  both pass.
