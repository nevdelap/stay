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

State: COMPLETED

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
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` plus every version assertion. Run `just qcheck` twice after the
  final amend and run the exact `just mac-qcheck` recipe successfully.

## TASK-081 - raise the last fixed five-second real-tmux flood deadline

State: NEW

Goal:

- Finish the deadline half of external-review finding G22, which TASK-070 left
  incomplete: replace the one remaining fixed five-second real-tmux flood
  deadline with the ten-second real-tmux polling ceiling the rest of the suite
  already uses, so a loaded CI runner cannot spuriously time it out.

Dependencies:

- None. TASK-070 is `COMPLETED`; this corrects its residual in a fresh commit
  rather than amending an archived task.

Scope:

- `tests/attachment.rs`: in
  `attach_with_log_succeeds_when_retained_history_exceeds_the_os_pipe_capacity`,
  the wait that polls `capture-pane` until tmux has ingested the whole flood
  (`filler-1999`) still uses
  `let deadline = Instant::now() + Duration::from_secs(5);` (line 2895),
  unchanged since TASK-054. Raise it to `Duration::from_secs(10)`, matching the
  sibling eviction-flood wait in the same file (currently at line 3063) and
  TASK-070's stated ten-second ceiling. This is the only fixed five-second
  deadline left in `tests/`; the picker pre-input sleeps G22 also named were
  already converted to readiness polls in TASK-070, so they are out of scope
  here.
- Do not change the poll interval, the loop body, the timeout-panic message, or
  any other wait; this is a single-constant change.

Acceptance criteria:

- No `Duration::from_secs(5)` deadline remains anywhere under `tests/`, proved
  by a repository grep.
- `attach_with_log_succeeds_when_retained_history_exceeds_the_os_pipe_capacity`
  still passes.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-082 - capture the full session output in default log mode

State: NEW

Goal:

- Fix external-review finding H1 (High): default `-l` clean-append logging
  captures only tmux history (`-E -1`) on every tick, so a command whose output
  never scrolls off the visible screen is never logged; and resolve the related
  capture-range defects M7 (oversized-line eviction re-dump loop) and L10
  (module-doc inaccuracy), all in `src/logging.rs`.

Dependencies:

- None.

Scope:

- `src/logging.rs`: clean-append capture (`capture_once` with `truncate=false`,
  around line 282) requests `-E -1` on every tick, including the final
  `on_detach` capture, so up to a screenful of the newest output that never
  scrolled into history is lost. Make the `on_detach` capture - the single
  boundary tick that runs once the relay loop has exited and the pane is (or is
  about to become) frozen - capture through the visible screen (`-E -`) so the
  final screenful is recorded, while `on_tick` and `on_attach_open` keep using
  `-E -1`, because the visible screen is volatile during a live attach and must
  not be captured incrementally. All three currently funnel through the same
  private `tick`/`capture_once` (where `capture_once` already takes a
  `truncate: bool`, and `tick` reads `truncate` from the
  `Mode::Clean { truncate }` variant); none takes a per-call boundary flag. Add
  a per-invocation screen-inclusive boundary flag as an explicit argument
  threaded `on_detach` -> `tick` -> `capture_once`; it must be a per-call
  argument, not a field on `Mode::Clean`, because only the single detach tick
  sets it. Do not inspect `#{pane_dead}` per tick. The existing
  already-captured-prefix anchor logic must still apply, so the screen-inclusive
  boundary capture appends only genuinely-new lines and never duplicates.
  Rationale for scoping to `on_detach`: the headline data-loss case (a
  short-output command frozen under `remain-on-exit`) is captured at detach, so
  a per-tick dead-pane probe is unnecessary cost and is deliberately out of
  scope here.
- `src/logging.rs`: the eviction/`None` fallback (around lines 345-359)
  re-appends the entire retained range with an eviction marker whenever
  `make_anchor` returns `None`, including when the newest complete line exceeds
  `MAX_ANCHOR_BYTES` (8192) - which recurs every tick, causing unbounded log
  growth, duplicated content, and a spurious "history evicted" marker. The
  specific `make_anchor` short-circuit is its oversized-line `return None`
  (around line 467, `newest_end - newest_start > MAX_ANCHOR_BYTES`), distinct
  from the empty/no-complete-line returns above it. When the newest line is too
  long to anchor, fall back to a bounded byte-suffix anchor - reuse the existing
  `make_partial_anchor` helper the partial-write path already uses (around line
  488\) - rather than abandoning anchoring; reserve the full re-dump plus marker
  for genuine front-eviction only.
- `src/logging.rs`: correct two stale doc comments (L10). The module doc comment
  (the `//!` block at the top of the file, whose capture-range claim currently
  says clean mode "restricts every capture to tmux's history range (`-E -1`),
  never the volatile visible screen") must state that only the incremental
  mid-attach append path (`on_tick`/`on_attach_open`) is restricted to `-E -1`,
  while truncate mode, raw backfill, and the final `on_detach` boundary capture
  intentionally include the visible screen. Separately, the `LogSession::start`
  method doc comment claims the log "reads as complete from session start";
  remove or qualify that claim to match the new boundary-capture behaviour. (The
  line 282 reference in the H1 bullet points at the clean-append capture call,
  not a doc comment.)

Acceptance criteria:

- A real-tmux test logs a session whose command prints fewer lines than the pane
  height and then exits under `remain-on-exit`, and asserts the log contains
  that output after detach (it does not today).
- A test proves a pane whose newest retained line exceeds `MAX_ANCHOR_BYTES`
  does not re-dump the whole retained range or emit a spurious eviction marker
  on successive ticks.
- Existing eviction, partial-write, and reattach-no-truncation tests still pass,
  and no capture path duplicates already-logged content.
- The module doc accurately describes each mode's capture range.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` and every version assertion; `just qcheck` and `just mac-qcheck`
  both pass.

## TASK-083 - represent signal termination consistently

State: NEW

Goal:

- Thread signal-termination information (`dead_signal`) through the surfaces
  that currently understand only exit codes, fixing external-review findings M1
  (the recreate notice mislabels a signal death as "exit code 0"), M2 (an
  unrecognized signal name aborts the whole inventory), M3 (`--json` cannot
  express a signal death), and L5 (the spec omits the signal row variant).

Dependencies:

- None.

Scope:

- `src/session.rs`: `TerminatedRecreateNotice` represents the death cause as
  only an `exit_code: u8` field (alongside its `session_name: String`) and is
  built with `session.exit_code.unwrap_or(0)` (around lines 226-264), so a
  signal-killed session renders `terminated with exit code 0 before recreate`.
  Carry the death cause (exit code vs signal number) on the notice. Keep the
  existing `exit code <n>` wording for an exit (both the `Display` string and
  `row_detail`), and for a signal render the `signal=<n>` token that
  `SessionRecord::status_detail` uses - for example
  `terminated with signal=<n> before recreate` and
  `[terminated signal=<n> before recreate]`; say "exit code 0" only when the
  exit code is genuinely 0. Do not change the exit-case wording to
  `status_detail`'s abbreviated `exit=<n>` form. Update the existing notice test
  that enshrines the `unwrap_or(0)` behaviour.
- `src/tmux.rs`: `parse_session_row` (around lines 1193-1198) maps a non-empty
  `#{pane_dead_signal}` through `parse_dead_signal(...).ok_or_else(...)?`, so a
  signal name `nix` cannot resolve (for example `SIGINFO` on macOS/BSD) makes
  `list_sessions` return `Err` and every session vanishes from `stay list`,
  `--json`, and the picker. Degrade an unparseable dead-signal to `None` while
  still reporting the pane as `terminated`, matching the graceful degradation
  the dynamic pane fields already use; do not abort the row or the inventory.
- `src/tmux.rs`: add a `signal` field to the `--json` output (`JsonSession` and
  `render_session_json`), populated from `dead_signal`, rendered as the number
  or `null`, placed and null-handled consistently with the existing `exit_code`
  field. This is an additive, backward-compatible extension (existing consumers
  ignore unknown fields) and resolves the decision TASK-055 deferred by
  extending the schema rather than breaking it. Update the JSON tests to the
  extended output.
- `design_docs/stay.html`: document the new `signal` JSON field and the
  `[terminated signal=<n> @<time>]` plain-listing row variant (L5) alongside the
  existing `exit=` form.

Acceptance criteria:

- A test proves force-recreating a signal-killed session renders `signal=<n>`
  (not `exit code 0`) in both the CLI notice and the picker row detail.
- `parse_session_row` with an unknown signal name parses as a terminated pane
  with `dead_signal = None` and does not error; a test proves one signal-killed
  session no longer hides the rest of the inventory.
- `stay list --json` emits a `signal` field (the number for a signal death,
  `null` otherwise), verified for a clean exit, a nonzero exit, and a signal
  death, and the schema documented in `stay.html` matches the output for each
  case.
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` and every version assertion; `just qcheck` and `just mac-qcheck`
  both pass.

## TASK-084 - harden relay abort ordering and external-signal cleanup

State: NEW

Goal:

- Prevent the relay from signalling an already-reaped (possibly recycled) PID
  (external-review M4), and ensure an externally delivered `SIGINT`/`SIGHUP`
  restores the terminal instead of leaving it in raw mode (L3).

Dependencies:

- None.

Scope:

- `src/relay.rs`: `AttachCleanup::abort` (around lines 756-765) calls
  `self.stop()` (SIGTERM then SIGKILL) before checking `if self.reaped`. On the
  normal exit path the child is reaped, then `attach_failure(...)?` and
  `pane_state(...)?` run after the reap; if either errors (for example an
  external `kill-session` makes `attach_failure` return `Err`), `abort` fires
  `kill()` at the reaped PID, which may have been recycled. Check
  `if self.reaped { return error; }` before calling `self.stop()`, so a reaped
  child is never signalled.
- `src/relay.rs`: `SignalGuard` installs a handler for `SIGTERM` only (and
  ignores `SIGPIPE`), so an external `SIGINT`/`SIGHUP` takes the default
  disposition and terminates `stay` without running `TerminalGuard`/
  `AttachCleanup`, leaving the terminal in raw mode. Route `SIGINT` and `SIGHUP`
  through the same termination mechanism `SIGTERM` uses - install
  `SigHandler::Handler(request_termination)` for them too, which sets the
  `TERMINATE_REQUESTED` flag the loop already drains - and restore their
  previous dispositions on drop as `SIGTERM` does (add the extra `previous_*`
  fields and `Drop` restores). This must not change the byte-forwarding of a
  terminal-generated Ctrl-C, which raw mode delivers as a byte, not a signal.
  Follow the existing `SignalGuard` install/rollback shape; do not add a second
  signal mechanism.

Acceptance criteria:

- A test proves that when the attach ends by an error on the post-reap path, the
  already-reaped child PID is not signalled. `stop_attach_child` is a free
  function with no spy seam, so assert this through observable `AttachCleanup`
  state rather than by intercepting the function: after `abort` runs on a reaped
  cleanup, `stopped()` must remain `false`, proving the reaped guard short-
  circuited before `self.stop()` (hence `stop_attach_child`) could run.
- A PTY test sends an external `SIGINT` and an external `SIGHUP` to a running
  relay and asserts the outer terminal is left in cooked mode. There is no
  existing external-signal terminal-restore test to copy: model the cooked-mode
  assertion on `panic_hook_restores_the_attach_terminal_state` (which builds a
  `forkpty` relay and asserts `tcgetattr` is equal before and after
  restoration), and deliver the real `SIGINT`/`SIGHUP` to that relay in the new
  test.
- The detach-key, copy-mode-key, and Ctrl-C-as-byte behaviours are unchanged
  (existing tests pass).
- Increment the patch version exactly once from the task baseline and update
  `Cargo.lock` and every version assertion; `just qcheck` and `just mac-qcheck`
  both pass.

## TASK-085 - treat an empty file default_command as unset

State: NEW

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

State: NEW

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
