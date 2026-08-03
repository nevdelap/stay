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

## TASK-070 - stabilize shared test state and readiness waits

State: NEW

Goal:

- Eliminate the process-global `TMUX_TMPDIR` race and remaining fixed-delay test
  flakes identified by external-review findings G6 and G22, while retaining
  real-tmux coverage and deterministic test isolation.

Dependencies:

- None. This task should complete before later tasks add or rely on concurrent
  real-tmux regressions.

Scope:

- Replace process-wide `TMUX_TMPDIR` mutation in production and test setup with
  an explicit test-owned value passed to every relevant `tmux` and spawned
  `stay` command. No concurrent test may read or write `TMUX_TMPDIR` through the
  process environment.
- Preserve production socket-root behavior and the existing test namespace,
  cleanup, and orphan-sweep guarantees. Do not serialize the entire test suite
  as a substitute for removing the environment race.
- Replace the remaining five-second logging-flood deadline with the established
  ten-second real-tmux polling ceiling. Replace picker pre-input sleeps with
  readiness polling that proves the intended picker state is visible before
  input is sent.
- Update the affected shared test helpers and their callers. Do not mutate the
  runner's real `HOME`, `PATH`, or other process-global environment to test the
  new behavior.

Acceptance criteria:

- Concurrent test execution has no unsynchronized
  `std::env::{var_os,set_var, remove_var}` access for `TMUX_TMPDIR`; all test
  tmux and child-process socket roots are explicit and isolated.
- Focused regressions prove separate namespaces work concurrently, the logging
  flood tolerates the ten-second ceiling, and picker input is sent only after
  its observable readiness condition.
- Existing real-tmux namespace cleanup and macOS socket-root behavior remain
  covered.
- Bump the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-071 - preserve clean logging bytes across write and eviction edges

State: NEW

Goal:

- Complete the clean-mode cursor model introduced by TASK-069 so partial writes
  and marked history transitions neither duplicate bytes nor silently lose the
  deterministically recoverable suffix, resolving G12 and G13.

Dependencies:

- TASK-069 must be `COMPLETED`.

Scope:

- Extend the anchor/cursor accounting so a partial append advances only through
  bytes that were safely persisted. A retry must neither duplicate a mid-line
  fragment nor skip bytes that were not written.
- When retained history changes and a valid unique overlap still exists, append
  only the genuinely uncaptured suffix. Emit the eviction marker and append a
  full retained dump only when the anchor is absent, ambiguous, corrupt, or
  otherwise cannot establish a safe suffix.
- Preserve TASK-069's explicit marked fallback, arbitrary-byte anchor encoding,
  cursor recovery, and no-silent-loss guarantee. Update every tracked logging
  design representation if the accounting semantics change.

Acceptance criteria:

- Deterministic fault-injection tests cover writes ending mid-line and at a
  newline boundary, then retry; the final log is byte-exact with no duplicated
  fragment and a valid cursor.
- Tests cover a shrink/eviction transition with a unique overlap and prove only
  the new suffix is appended, plus absent/ambiguous overlap cases that retain
  the marked full-dump fallback.
- A real-tmux logging regression continues to prove retained output and the
  eviction marker behavior at the history boundary.
- Bump the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-072 - harden log targets and disclose raw-history gaps

State: NEW

Goal:

- Give the primary log the same anti-symlink protection as its cursor sidecar
  and make a raw-mode destination change visible to the operator, resolving G10
  and G14.

Dependencies:

- TASK-071 must be `COMPLETED` so log-write correctness has one established
  cursor model before path-opening behavior changes.

Scope:

- Before every primary-log append, revalidate the canonical target and open it
  with no-follow semantics equivalent to the sidecar. Preserve owner-only
  creation, existing valid regular-file append behavior, and actionable errors
  for symlink, ownership, or permission violations.
- In `--raw` mode, when an existing pane pipe is redirected to a different
  requested path, do not backfill or truncate live history. Emit one clear
  warning that the new file begins with future pipe output only; suppress the
  warning when the active target already matches the requested path.
- Keep raw and clean modes distinct and do not expose a time-of-check/time-of-
  use window by validating only during initial logging setup.

Acceptance criteria:

- Security tests replace the primary target with a symlink after startup and
  prove capture refuses to follow it without writing through the link; valid
  regular targets still append and sidecar protections remain intact.
- Raw reattach tests cover same-path and changed-path requests against a live
  piped pane. The changed path receives only future output and exactly one
  warning; the same path produces no warning or destructive backfill.
- Documentation and errors explain the no-backfill behavior without revealing
  sensitive filesystem data beyond the requested path.
- Bump the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-073 - enforce compiler diagnostics in changed and CI scopes

State: NEW

Goal:

- Close the compiler-warning escape paths in local changed-file linting and CI,
  with tests that exercise real command behavior rather than only helper
  functions, resolving G4, G5, and G23.

Dependencies:

- None.

Scope:

- Add an authoritative whole-tree CI Clippy gate using `-D warnings`, separate
  from fast changed-file formatting/lint selection. Keep changed-scope feedback
  focused on changed spans without allowing unrelated diagnostics to mask a
  changed warning.
- Ensure changed-file Clippy performs a fresh analysis even with a warm target
  cache, without deleting broad user build artifacts. The selected strategy must
  be deterministic locally and in `rust-cache`-restored CI jobs.
- Expand `scripts/test_quality.py` fixtures to exercise copy detection, root-
  commit changed-path fallback, warm-cache changed-warning failure, and command
  failure propagation with and without compiler diagnostics.
- Keep ordinary changed-file checks fast enough for the local loop and preserve
  the existing all-files dispatcher behavior.

Acceptance criteria:

- CI contains a whole-tree
  `cargo clippy --locked --all-targets --all-features -- -D warnings` equivalent
  that runs on every push and pull request.
- A warm-cache regression proves a warning in a changed Rust source fails the
  changed-scope gate; unrelated diagnostics and non-diagnostic command failures
  retain their established reporting behavior.
- Quality-dispatcher tests prove actual `C` status handling and the initial
  commit fallback, not merely rename/add look-alikes.
- Bump the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-074 - complete the quality matrix and toolchain contract

State: NEW

Goal:

- Make local and CI toolchain expectations explicit and close remaining quality
  dispatcher blind spots, resolving G24 and G25.

Dependencies:

- TASK-073 must be `COMPLETED` so CI and dispatcher changes build on the
  authoritative compiler-gate design.

Scope:

- Add a tracked `rust-toolchain.toml` that selects the supported 1.88 toolchain
  and the repository-required components for local commands. Keep the existing
  explicit MSRV gate.
- Add a stable-toolchain CI build/test job alongside the 1.88 MSRV job, with the
  same locked dependency and feature coverage. Name jobs so release/workflow
  checks that require `check`, `msrv`, and `macos` remain valid.
- Classify Python files by suffix everywhere in the repository, excluding only
  generated or explicitly ignored paths through existing selection rules.
- Make the debugging-macro policy explicit and enforce it consistently for the
  intended Rust macros, including `println!` and `eprintln!`; preserve any
  documented intentional user-output exceptions through a narrowly scoped,
  reviewed mechanism rather than a broad regex exemption.

Acceptance criteria:

- Fresh local `cargo`, `just`, and formatter/linter invocations select the
  tracked 1.88 toolchain, and CI still runs the explicit MSRV job plus an
  independent stable build/test job.
- Dispatcher tests cover Python outside `scripts/` and every prohibited debug
  macro; any approved exception is explicit, local, and tested.
- Workflow YAML passes its linting and preserves the existing required CI job
  names and release preflight contract.
- Bump the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-075 - make picker recreation and empty-state behavior safe

State: NEW

Goal:

- Require confirmation before recreating any existing session and remove fragile
  empty-state/rendering assumptions, resolving G9, G26, and G27.

Dependencies:

- TASK-070 must be `COMPLETED` for reliable real-PTY picker interaction tests.

Scope:

- Route both live and terminated selected sessions through the existing
  `RecreateConfirm` mode with its No-focused selector, captured target name, and
  normal poll/feedback behavior. Do not change create-new-session behavior.
- Make the zero-session status accurately advertise `c` create, `Enter` attach,
  and `q`/`Esc` quit, matching the implemented key handling.
- Remove positional indexing from `fitted_suffix`. Preserve compact terminated
  rendering when expected spans are available and use a safe, readable fallback
  for an empty or shorter-than-expected suffix.

Acceptance criteria:

- State-machine and real-PTY tests prove `r` on live and terminated sessions
  first shows a No-focused confirmation, `n`/cancel leaves the session intact,
  and only `y` performs recreation for the captured target.
- Empty-picker rendering and keyboard tests assert all advertised controls and
  their behavior.
- Unit tests pass empty, one-span, and normal terminated suffix inputs without
  panic and retain width-bounded output.
- Bump the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-076 - preserve tmux compatibility across versions and time zones

State: NEW

Goal:

- Accept supported tmux development version strings and render termination
  timestamps without a stale cross-DST local offset, resolving G17 and G18.

Dependencies:

- None.

Scope:

- Extend tmux version parsing to accept `next-<major>.<minor>` builds without
  weakening rejection of malformed or below-minimum versions. Preserve existing
  prerelease and platform-specific version handling.
- Render human-readable dead-pane timestamps in UTC (`Z`) rather than applying a
  process-start local offset to a past event. Remove the cached-local-offset
  path so a DST transition cannot mislabel either wall time or numeric offset.
- Keep JSON timestamps in UTC and update user-facing documentation/tests to
  describe the common UTC representation. Avoid broad changes to session
  ordering or dead-pane status parsing.

Acceptance criteria:

- Version-parser tests cover `next-3.4`, existing accepted prereleases,
  malformed strings, and below-floor versions.
- Timestamp tests cover an event on each side of a DST boundary and prove both
  render as the correct UTC instant with `Z`, independent of the process's
  current local offset.
- Linux and macOS compatibility tests continue to cover their existing tmux
  signal/version rendering differences.
- Bump the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-077 - make tmux text boundaries safe for arbitrary paths

State: NEW

Goal:

- Make tmux inventory and generated configuration handling safe when user
  controlled paths or commands contain control characters, resolving G2 and G8
  without sacrificing whole-inventory availability.

Dependencies:

- TASK-070 must be `COMPLETED` so the real-tmux regression fixtures are safe
  under concurrent test execution.

Scope:

- Replace the newline and unit-separator-delimited dynamic inventory framing
  with an injective record/value representation that round-trips arbitrary valid
  UTF-8 `pane_current_path` and `pane_current_command` values, including
  newline, carriage return, and `0x1f`. Keep the fixed session fields validated
  independently.
- A malformed or undecodable dynamic value must not make `stay list` or the
  picker fail wholesale: retain the valid session record and represent only the
  affected optional cwd/command as unavailable. Structural fixed-field errors
  still fail with an actionable tmux diagnostic.
- Correct the wrapper comment that currently claims separator-bearing paths are
  accepted. Preserve one batched `list-panes -a -F` inventory command; do not
  reintroduce per-pane tmux calls.
- Reject a user tmux-config path containing any control character before it is
  interpolated into the generated line-oriented `source-file` command. Keep the
  established escaping for safe paths and surface a clear error without starting
  a tmux server.

Acceptance criteria:

- Unit and real-tmux tests cover newline, carriage return, and unit-separator
  cwd/command values, proving the plain list and picker remain usable and
  preserve the other inventory fields.
- Tests cover malformed encoded data as a per-record degradation and preserve
  hard failure for malformed fixed fields.
- Session-creation tests prove newline/control-character config paths are
  rejected before tmux invocation, while ordinary paths containing existing
  escapable characters still produce the intended config.
- Bump the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-078 - bound tmux subprocess input and output cleanup

State: NEW

Goal:

- Ensure every short-lived tmux wrapper command drains input/output without a
  pipe deadlock and returns by its command deadline even when descendants retain
  output file descriptors, resolving G15 and G16.

Dependencies:

- TASK-070 must be `COMPLETED` for deterministic subprocess fixtures.

Scope:

- Refactor `run_with_stdin` so stdout and stderr drains start before or
  concurrently with writing stdin. Preserve bounded complete stdin delivery and
  always reap the direct child after write, wait, or reader failure.
- Make reader completion bounded by `COMMAND_TIMEOUT` on both normal and
  timeout/error paths. If a descendant retains a pipe after the direct child
  exits, return an actionable bounded-output-drain error rather than blocking
  forever; retain output collected before the bound where it can aid diagnosis.
- Keep the existing command timeout, direct-child termination, non-shell argv,
  and normal tmux command semantics. Do not weaken timeouts or detach reader
  threads that can outlive test/process cleanup without ownership.

Acceptance criteria:

- A fixture that writes more than pipe capacity before consuming stdin completes
  through `run_with_stdin` and preserves its stdout/stderr result.
- A fixture whose descendant retains stdout or stderr proves success and timeout
  paths return within a bounded test deadline and leave the direct child reaped.
- Existing large-output `Tmux::run` behavior remains covered, and all error
  paths state whether the failure was write, wait, reader, or timeout related.
- Bump the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-079 - correct CLI and shell-integration boundary behavior

State: NEW

Goal:

- Make pure integration-output commands usable inside a stay pane, follow
  conventional usage exit codes, and keep optional shell alias generation
  conservative and non-disruptive, resolving G3, G19, G20, and G28.

Dependencies:

- None.

Scope:

- Dispatch `--prompt-integration` and `shell-integration` before the nested tmux
  guard because they emit only stdout and never attach to tmux. All other
  non-help operational commands remain guarded inside tmux.
- Return exit status 2 for clap usage/parse errors while preserving zero for
  help/version and one for runtime errors. Do not change stdout/stderr routing.
- When an rc candidate exists but cannot be read, treat it as a conservative
  alias conflict: omit `s`, emit an actionable warning, and still print the
  primary shell integration. Handle directories and permission failures without
  reading or modifying user files.
- Replace the stale `lessons_learned.md` prompt-integration example with a
  current implemented-feature example; review nearby CLI guidance for the same
  obsolete claim without broad unrelated documentation rewrites.

Acceptance criteria:

- CLI tests prove both integration snippets are byte-identical with and without
  `TMUX`, while `list`, attach, and other tmux-operating commands remain refused
  in a pane.
- Tests assert help/version exit 0, usage errors exit 2 on stderr, and runtime
  failures exit 1 without changing their diagnostic streams.
- Shell-integration tests cover unreadable and directory rc candidates, retain
  the primary snippet, omit the optional alias, and report the conservative
  conflict.
- Documentation contains no claim that prompt integration is unimplemented.
- Bump the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-080 - make relay nonblocking exits safe and ordered

State: NEW

Goal:

- Preserve a healthy relay through spurious nonblocking readiness, clean up the
  forkpty attach child on every error exit, and retain queued control actions
  after a closed input write, resolving G7, G11, and G21.

Dependencies:

- TASK-070 must be `COMPLETED` for stable PTY and timing fixtures.

Scope:

- Treat master-read `EAGAIN` and `EWOULDBLOCK` like an interrupt/no-data event
  and continue the poll loop. Keep real EOF, EIO, and other errors distinct.
- Route every post-fork, non-normal relay return through one stop-and-reap path
  that terminates/detaches as appropriate and reaps the attach child exactly
  once. Preserve the existing normal detach, pane-death, signal, and input-error
  semantics.
- When a byte write reports `WriteInput::Closed`, discard only bytes that can no
  longer be delivered; continue processing queued `Detach` and `CopyMode`
  actions in FIFO order through the normal cleanup path.
- Do not add blocking I/O, weaken signal handling, or alter user-configured
  detach/copy-mode keys.

Acceptance criteria:

- Deterministic relay tests inject `EAGAIN`/`EWOULDBLOCK` after readiness and
  prove the attach remains alive until a normal detach or pane exit.
- Each formerly direct error path has regression coverage showing the attach
  child is stopped and reaped, with no zombie left for a caller that catches the
  error and continues running.
- Queued byte-plus-control input tests prove `Detach` and `CopyMode` survive a
  closed byte write and execute in order.
- Bump the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.
