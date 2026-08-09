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

The required Bats test names, command mappings, assertions, harness rules, and
CI constraints below are the complete implementation specification. No
implementation task may silently omit a listed scenario or requirement.

## TASK-095 - add the first Bats CLI acceptance scenario

State: COMPLETED

Goal:

- Establish one readable, black-box Bats acceptance file for the entire CLI
  acceptance suite and run it in a dedicated Linux/macOS acceptance matrix. This
  task adds only the first scenario to that file: the advertised human-readable
  session listing. Future acceptance scenarios must extend this file rather than
  create separate Bats files.

Dependencies:

- None. The current `main` baseline already provides the pinned tmux 3.6 CI
  setup needed by the scenario.

Scope:

- `tests/acceptance.bats`: create the single Bats file that will contain all
  future CLI acceptance scenarios, and add exactly one scenario for this task.
  The scenario must create an isolated temporary `TMUX_TMPDIR`, unset `TMUX`,
  use the production `stay` namespace `-L stay` within that temporary socket
  root, and clean up the server and temporary directory in all exit paths. It
  must create two named detached sessions with Stay through
  `cargo run --release --locked --quiet -- create`, invoke Stay again through
  `cargo run --release --locked --quiet -- list`, and assert success, both
  session names, the human-readable `[detached]` status, and the absence of ANSI
  escape sequences in captured non-terminal output. Keep the scenario focused on
  the public CLI; do not call Rust internals or add JSON, attach, picker, or
  error scenarios. Do not add another Bats file.
- `scripts/ci-install-bats.sh`: install the pinned Bats `1.14.0` release from
  its official source archive, verify its immutable checksum and reported
  version, and expose the `bats` command on `PATH`. Do not use an unpinned
  latest installer or a platform-specific package-manager version.
- `.github/workflows/ci.yml`: add a dedicated Linux/macOS acceptance matrix that
  verifies supported tmux, builds a binary, installs Bats, and runs the
  acceptance file with human-readable output. Preserve the existing job
  boundaries, timeouts, tool setup, macOS Rust job, and Rust test commands. The
  stable job does not need a duplicate acceptance run.
- `scripts/ci-install-tmux.sh` and `scripts/maccmd.sh`: rename the CI and macOS
  helper scripts to make their installation and shell-script roles explicit, and
  update all repository references.
- `design_docs/stay.html`: remove the obsolete, superseded design artifact; this
  deletion is explicitly authorized for this task.
- `Cargo.toml` and `Cargo.lock`: increment the patch version exactly once from
  the task baseline to `0.0.76` and keep the package metadata synchronized.

Acceptance criteria:

- `tests/acceptance.bats` is the only Bats acceptance file and contains exactly
  one executable scenario for this task. Its name and assertions make the
  human-readable `stay list` behavior clear without consulting the Rust tests;
  later acceptance work extends this same file.
- The scenario uses an isolated temporary tmux socket root, never touches a
  user's existing `stay` server, cleans up its two sessions/server, and passes
  on both Linux and macOS.
- The scenario proves both named sessions appear as detached human-readable
  rows, proves captured output is non-ANSI, and fails if `stay list` exits
  unsuccessfully or omits either row.
- CI installs and verifies the pinned Bats release before running the scenario
  in the dedicated Linux/macOS matrix, and the run visibly reports the scenario
  in Bats' pretty format.
- The exact `just qcheck` and `just mac-qcheck` recipes pass, and the package
  version advances exactly once with `Cargo.lock` synchronized.

## TASK-096 - harden the Bats harness and expand session inventory

State: NEW

Goal:

- Make the single `tests/acceptance.bats` file hermetic and reusable, then
  replace the first one-state listing scenario with human-readable and JSON
  inventory scenarios covering at least two detached, two attached, and two
  terminated sessions.

Dependencies:

- None beyond the completed TASK-095 harness.

Versioning:

- Increment the package patch version exactly once from the version present at
  task start; update the matching `stay` package entry in `Cargo.lock`, and
  leave all other dependency versions unchanged.

Scope:

- `tests/acceptance.bats`: use the inherited wrapper-owned `TMUX_TMPDIR`, and
  isolate `HOME` and `XDG_CONFIG_HOME`; ensure the temporary home has no
  `.tmux.conf`; control `SHELL` and `PATH`; unset all `STAY_*` overrides before
  each test; generate unique names per test; and use bounded cleanup for every
  session and log. Keep direct tmux calls out of the Bats file.
- `tests/helpers/acceptance_pty.bash`: create the reusable PTY helper in this
  task and load it from `tests/acceptance.bats`. It must expose exactly
  `pty_start`, `pty_wait_until_attached`, `pty_send_input`, `pty_send_detach`,
  `pty_wait`, and `pty_force_cleanup`. `pty_start` must run the supplied Stay
  command through `script(1)`, create the FIFO or equivalent input handle, and
  set the documented shell variables `PTY_PID`, `PTY_TRANSCRIPT`, and
  `PTY_INPUT`. `pty_wait_until_attached SESSION` must poll
  `$STAY_BIN list --json` with a bounded timeout and diagnostic on timeout. The
  send, wait, and force-cleanup operations must use those handles; `pty_wait`
  must reap the child and return its status, and `pty_force_cleanup` must
  terminate, kill if necessary, reap, and remove the PTY handles. The helper
  owns all platform-specific `script(1)` details; no Bats scenario may invoke
  platform-specific PTY commands directly.
- `scripts/quality.py`: classify the `.bash` suffix in the existing `bash` group
  so `tests/helpers/acceptance_pty.bash` receives the same `shfmt` and
  ShellCheck formatting/linting as `.bats` and `.sh` files. Add a dispatcher
  regression assertion that a `.bash` path is selected for both operations.
- Unset exactly these inherited overrides in `setup()` before each test:
  `STAY_CMD`, `STAY_DETACH_KEY`, `STAY_COPY_MODE_KEY`, `STAY_HISTORY_LINES`,
  `STAY_LOG_CAPTURE_INTERVAL_SECONDS`, and `STAY_SESSION_NAME`. Set `SHELL` to
  the selected known shell and set `PATH` to the controlled tool directories
  plus `STAY_BIN`.
- Add one shared `setup_inventory_fixture()` helper that creates the same six
  uniquely named sessions for both inventory tests. It must create
  `inventory-${run}-detached-1` and `-2` with `stay create ... sleep 60`,
  `inventory-${run}-attached-1` and `-2` with the same long-lived command and
  two PTY `stay attach` clients, and `inventory-${run}-terminated-1` with
  `sh -c 'exit 7'` and `inventory-${run}-terminated-2` with
  `sh -c 'kill -TERM $$'`. The human-readable and JSON tests must call this
  helper rather than duplicate subtly different state setup.
- Attached fixtures must use a PTY and readiness polling; terminated fixtures
  must retain enough metadata for the public list output to report their causes.
- Before the fixture, assert the empty states: `stay list` has exactly empty
  stdout and `stay list --json` equals `{"sessions":[]}`. After the fixture,
  assert each inventory test's six names and statuses, then run the shared
  Stay-based cleanup.
- Add `scripts/ci-acceptance-cleanup.sh` with the exact interface
  `scripts/ci-acceptance-cleanup.sh STAY_BIN TMUX_TMPDIR [SESSION...]`. It must
  export the supplied `TMUX_TMPDIR`, unset `TMUX`, enumerate with the supplied
  Stay binary, kill every discovered session with `stay kill`, and also kill
  every supplied generated name as a fallback. It must refuse an empty or
  non-temporary `TMUX_TMPDIR` and must never target a user's default socket
  root. A valid directory is one created by `ci-run-acceptance.sh` with
  `mktemp -d "${TMPDIR:-/tmp}/stay-acceptance.XXXXXX"`, still present, and
  passed unchanged from the wrapper; the helper must validate that it is a
  directory beneath the real `${TMPDIR:-/tmp}` with the `stay-acceptance.`
  prefix. The Bats `teardown()` calls this helper with the test's known names.
- Add `scripts/ci-run-acceptance.sh` as the workflow entrypoint. Its exact
  interface is `STAY_BIN=/absolute/path/to/stay scripts/ci-run-acceptance.sh`;
  it creates `TMUX_TMPDIR` exactly as specified above, exports it, unsets
  `TMUX`, runs `bats --formatter pretty tests/acceptance.bats`, and traps
  `EXIT`, `INT`, and `TERM`. Bats `setup_file()` must require this inherited
  `TMUX_TMPDIR`, must not create a replacement, and `teardown_file()` must not
  remove it; the wrapper owns its lifetime. The trap must save Bats' exit
  status, call `scripts/ci-acceptance-cleanup.sh "$STAY_BIN" "$TMUX_TMPDIR"`,
  then run `tmux -L stay list-sessions` with that same `TMUX_TMPDIR` and
  `-f /dev/null`; only when that check finds a surviving server may it run
  `tmux -L stay -f /dev/null kill-server`. Finally it must remove its temporary
  directory and restore the saved Bats or signal status. The wrapper must never
  use the default socket.
- `.github/workflows/ci.yml`: set the acceptance job timeout to exactly 15
  minutes, retain the Linux/macOS matrix, and invoke
  `STAY_BIN="$GITHUB_WORKSPACE/target/release/stay" scripts/ci-run-acceptance.sh`
  in the Run Bats acceptance step after the release build and pinned Bats
  installation. Do not invoke Bats directly from the workflow or run Cargo for
  individual assertions.

Required Bats scenarios:

- `@test "stay list shows the session inventory as human-readable rows" {` — use
  `stay list` after the shared fixture creates two detached, two attached, and
  two terminated sessions. Assert all six names and states, terminated causes,
  no ANSI in non-terminal output, and the empty/no-server case.
- `@test "stay list --json emits a stable machine-readable inventory" {` — use
  `stay list --json` against the exact same fixture helper and assert all six
  rows, names, statuses, timestamps, optional fields, deterministic ordering,
  and no ANSI decoration.

Acceptance criteria:

- Every test runs with hermetic HOME/config, shell, PATH, tmux, and Stay
  environment inputs and does not read host configuration.
- Both inventory tests call the same fixture and assert six uniquely named
  sessions: two detached, two attached, and two terminated. Human output is
  non-ANSI; JSON fields, nulls, statuses, and creation-order sorting are checked
  exactly.
- The attached rows are observed only after both PTY clients report readiness;
  terminated rows are observed only after both commands report their expected
  causes.
- Empty-list assertions happen before fixture creation, and all six sessions are
  gone after teardown.
- Failed, timed-out, and interrupted scenarios cannot contaminate later tests or
  leave the isolated server running.
- Bats uses one release binary per CI job and the focused Bats run passes on
  Linux and macOS.

## TASK-097 - add deterministic lifecycle and CLI contract scenarios

State: NEW

Goal:

- Cover non-PTY session lifecycle, argument validation, help/version output, and
  explicit current contracts in the same Bats file.

Dependencies:

- TASK-096.

Versioning:

- Increment the package patch version exactly once from the version present at
  task start; update the matching `stay` package entry in `Cargo.lock`, and
  leave all other dependency versions unchanged.

Scope:

- Add scenarios for default-command creation, explicit command and argument
  preservation, `--cwd`, duplicate rejection, live and terminated
  `--force-recreate`, and `stay kill` for live, terminated, missing, and invalid
  sessions.
- Add exact help, subcommand-help, version, unknown-command, unknown-option,
  missing-value, invalid-name, and overlong-name assertions.
- Add the create and attach option-conflict matrix: create read-only and
  low-priority require `--attach`; attach `--truncate` and `--raw` require
  `--log`; and pass-through conflicts with read-only, low-priority, and logging.
- Add `stay attach --pass-through` coverage for incremental, partial, and large
  input. Assert the current CLI contract: successful forwarding returns zero; do
  not assert propagation of the session command's eventual exit status. Status
  propagation is out of scope and must not be implemented by TASK-097.

Required Bats scenarios:

- `@test "stay create uses the configured default command" {` — set `STAY_CMD`
  to the fixture command, run `stay create SESSION`, inspect both list formats,
  and kill the session.
- `@test "stay create preserves the command and its arguments" {` —
  `stay create SESSION COMMAND...`; preserve every argument, including a command
  argument beginning with `-` after `--`.
- `@test "stay create starts the session in the requested directory" {` —
  `stay create SESSION --cwd DIR`; assert the directory in JSON inventory.
- `@test "stay create --force-recreate replaces an existing session" {` —
  `stay create SESSION --force-recreate`; cover both live and terminated
  sessions and assert one resulting session with the new command.
- `@test "stay create rejects duplicates and invalid session names" {` —
  exercise duplicate creation without `--force-recreate`, invalid punctuation,
  and overlong names; assert no unexpected partial session.
- `@test "stay kill removes live and terminated sessions" {` —
  `stay kill SESSION`; remove both live and retained terminated sessions and
  verify the empty-server/list state after the last kill.
- `@test "stay kill reports missing and invalid sessions" {` — use
  `stay kill SESSION` for missing and invalid names; assert useful stderr and
  nonzero status.
- `@test "stay help lists commands and options" {` — run `stay --help` and each
  of `stay list --help`, `stay create --help`, `stay attach --help`,
  `stay kill --help`, and `stay shell-integration --help`; assert stdout, exit
  zero, public command names, option shapes, and empty stderr.
- `@test "stay version prints the package version" {` — run `stay --version` and
  assert the exact `stay VERSION` line.
- `@test "stay rejects invalid arguments and session names" {` — use unknown
  commands/options, missing values, invalid punctuation, and names beyond 128
  Unicode characters; assert exit status 2 and usage on stderr.
- `@test "stay rejects conflicting options" {` — cover create read-only and
  low-priority without `--attach`, attach `--truncate`/`--raw` without `--log`,
  `stay attach SESSION --pass-through --read-only`,
  `stay attach SESSION --pass-through --low-priority`,
  `stay attach SESSION --pass-through --log FILE`, `stay list --no-alt-screen`,
  `stay --prompt-integration list`, and
  `stay --prompt-integration --no-alt-screen`.
- `@test "stay attach --pass-through forwards stdin without attaching" {` — use
  `stay attach SESSION --pass-through` for incremental, partial, and large
  input; assert forwarding succeeds and returns zero. Do not assert eventual
  session-command status because the current CLI does not propagate it.
- `@test "stay enforces its tmux environment boundary" {` — set a fake `TMUX`
  value and assert `stay list`, `stay create`, `stay attach`, and `stay kill`
  reject execution without changing the isolated server; unset `TMUX` and assert
  both integration commands still work in the same simulated environment. Also
  run against a deliberately unsupported tmux version and a missing tmux
  executable; assert the documented diagnostics and nonzero status without
  touching the default socket.

Acceptance criteria:

- These scenarios use exact stdout, stderr, and exit-status assertions and do
  not require a PTY.
- Every scenario uses unique names and the TASK-096 cleanup contract.
- The current pass-through behavior is documented by the test name and
  assertion, with no unsupported status-propagation requirement introduced.

## TASK-098 - add PTY attach and relay scenarios

State: NEW

Goal:

- Exercise public attach and create-and-attach relay behavior with a reusable
  PTY fixture instead of ordinary Bats pipes.

Dependencies:

- TASK-096 and TASK-097.

Versioning:

- Increment the package patch version exactly once from the version present at
  task start; update the matching `stay` package entry in `Cargo.lock`, and
  leave all other dependency versions unchanged.

Scope:

- Add PTY scenarios for `stay create --attach` and `stay attach`, including
  input/output forwarding, configured detach, terminal restoration, and
  re-attach after detachment.
- Cover attach `--read-only`, `--low-priority`, normal command exit status,
  signal-derived status, SIGHUP/SIGINT/SIGTERM cleanup, and failed attach
  cleanup.
- Reuse the PTY fixture for attached-inventory checks, keeping two attached
  clients alive while `stay list` runs.
- Require readiness polling for session creation, client attachment, output
  markers, and process exit. Register process cleanup and wait for every child
  on success and failure.
- Use the `tests/helpers/acceptance_pty.bash` helper specified and implemented
  by TASK-096 for every attached test. Do not add a second PTY helper or invoke
  platform-specific PTY commands directly from a scenario.

Required Bats scenarios:

- `@test "stay create --attach creates and attaches a session" {` — use
  `stay create SESSION --attach` through a PTY; forward input/output, detach,
  preserve the session, re-attach it, then allow the fixture command
  `sh -c 'printf ready; read value; printf "value=%s\\n" "$value"; exit 7'` to
  finish and assert status 7. Creation has no logging flags; logging begins with
  a later `stay attach --log`.
- `@test "stay create --attach --read-only prevents input changes" {` — use
  `stay create SESSION --attach --read-only` in a PTY with a fixture command
  that reads one line and prints it; send a mutating line and verify the read
  does not complete while output remains visible.
- `@test "stay create --attach --low-priority attaches at low priority" {` — use
  `stay create SESSION --attach --low-priority` with the relay fixture and
  verify the client still functions with the low-priority/ignore-size mode.
- `@test "stay attach relays input and output and detaches cleanly" {` — use
  `stay attach SESSION` in a PTY against a command that prints `ready`, reads a
  line, and prints `received`; assert forwarding of `input`, the configured
  detach key, terminal restoration, and re-attachability.
- `@test "stay attach --read-only prevents mutating input" {` — use
  `stay attach SESSION --read-only` in a PTY against the same read-and-echo
  command and assert output without input mutation.
- `@test "stay attach --low-priority uses the low-priority client mode" {` — use
  `stay attach SESSION --low-priority` against the relay fixture and assert
  normal input/output behavior with the low-priority client modifier.
- `@test "stay attach reports failures and preserves exit status" {` — use
  `stay attach SESSION` for a missing session (nonzero status and diagnostic),
  `sh -c 'exit 7'` (status 7), `sh -c 'kill -TERM $$'` (status 143), and
  external SIGHUP, SIGINT, and SIGTERM delivery to the attach process; assert
  the diagnostic, status, and absence of a remaining client for every subcase.
  Relay cleanup failure is covered by the lower-level test specified below, not
  by an artificial acceptance fixture.

Lower-level verification:

- Extend the `#[cfg(test)] mod tests` in `src/relay.rs` with the exact test
  `abort_reports_reap_failure_after_relay_error`. Construct `AttachCleanup` with
  a PID that is not a child of the test process, call
  `abort("relay input failed".to_owned())`, and assert that the returned error
  contains both the original relay error and `failed to reap tmux attach`. This
  deterministic unit test owns the cleanup-failure contract; no Bats test may
  attempt to manufacture a process-reaping race.

Acceptance criteria:

- Every PTY scenario uses the shared `script(1)`-based PTY helper from TASK-096,
  bounded polling, diagnostics on timeout, and deterministic child cleanup.
- No scenario relies on a fixed sleep to establish readiness or leaves a client
  attached after a failed assertion.
- The exact `just qcheck` and `just mac-qcheck` recipes pass.

## TASK-099 - add comprehensive logging scenarios

State: NEW

Goal:

- Demonstrate clean and raw logging end to end, including capture boundaries,
  target safety, and output volume beyond tmux history limits.

Dependencies:

- TASK-096 and TASK-098.

Versioning:

- Increment the package patch version exactly once from the version present at
  task start; update the matching `stay` package entry in `Cargo.lock`, and
  leave all other dependency versions unchanged.

Scope:

- Cover clean `--log`, repeated attach/detach append without duplication,
  visible-screen final capture, `--truncate`, raw ANSI backfill and streaming,
  detached raw growth, active-pipe replacement, and final output on pane exit.
- Cover relative path resolution, private permissions, rejection of symlinks,
  non-regular files, unsafe permissions, cursor sidecars, and safe behavior when
  a log target disappears or becomes unwritable. Keep ownership checks and
  symlink-swap revalidation in the lower-level logging tests below so the
  acceptance suite has no privileged or timing-race fixture.
- Extend the `#[cfg(test)] mod tests` in `src/logging.rs` with the exact test
  `a_wrong_owner_log_target_is_rejected`. It must exercise ownership validation
  through the module's injected owner-id test seam, without `sudo` or a
  platform-specific user. Retain and verify the existing exact test
  `a_primary_log_symlink_swap_is_rejected_without_following_the_link`: it must
  resolve a secure temporary target, replace that path with a symlink to a
  separate sentinel file before the capture write, and assert the sentinel
  remains unchanged and the warning names the symlink. Both tests must run on
  Linux and macOS under the normal Rust test jobs.
- Add the long-output stress scenario: set a small `STAY_HISTORY_LINES`, a short
  `STAY_LOG_CAPTURE_INTERVAL_SECONDS`, and `--truncate` so the configured
  history limit remains active. Emit six batches of 80 uniquely numbered lines
  with a 1.5-second pause between batches, then emit a rapid 1,000-line flood.
  Assert that the six paced batches appear exactly once, that the flood forces
  the exact eviction marker, and that final flood markers and all content
  captured before eviction remain complete without duplicate old content.
- Keep logging validation for missing `--log`, create logging flags, and
  pass-through conflicts in the deterministic contract coverage.

Required Bats scenarios:

- `@test "stay attach --log captures clean output across attaches" {` — use
  `stay attach SESSION --log FILE`; assert retained history, periodic output,
  visible-screen final capture, append across attaches, no duplication, and
  private file permissions.
- `@test "stay attach --log --truncate overwrites the log" {` — use
  `stay attach SESSION --log FILE --truncate` repeatedly and assert replacement
  rather than append while retaining clean output.
- `@test "stay attach --log --raw preserves ANSI and streams output" {` — use
  `stay attach SESSION --log FILE --raw`; assert ANSI backfill, continuous
  detached growth, and that a second raw attach does not destroy active-pipe
  output.
- `@test "stay logging handles history and capture boundaries" {` — assert
  visible-screen preservation, large captures, the exact
  `--- history evicted before capture ---` marker, and recovery from missing,
  invalid, or mismatched cursor sidecars.
- `@test "stay logging preserves output across repeated history boundaries" {` —
  set small `STAY_HISTORY_LINES` and short `STAY_LOG_CAPTURE_INTERVAL_SECONDS`;
  use `stay attach SESSION --log FILE --truncate`; emit six batches of 80
  uniquely numbered lines with a 1.5-second pause between batches, then a rapid
  1,000-line flood; assert the paced batches appear exactly once, the eviction
  marker appears, and final markers plus all content captured before eviction
  remain complete.
- `@test "stay logging rejects unsafe log targets" {` — use
  `stay attach SESSION --log FILE` with relative paths, pre-existing symlinks,
  non-regular files, unsafe permissions, and unsafe cursor/temp symlinks; assert
  path safety and correct invoking-client working-directory resolution.
  Ownership and symlink-swap assertions belong to the named lower-level tests,
  not this Bats scenario.
- `@test "stay logging survives target failures safely" {` — remove or make the
  log target unwritable during `stay attach SESSION --log FILE`; assert a
  one-time warning without killing relay, safe cursor updates, and final output
  capture.
- `@test "stay logging validates its option combinations" {` — assert
  `--truncate` and `--raw` require `--log`, logging conflicts with
  `--pass-through`, and logging flags on `stay create` are rejected rather than
  silently ignored.

Acceptance criteria:

- Logging scenarios use PTY/readiness helpers and clean up primary logs, cursor
  sidecars, sessions, clients, and processes on every path.
- The stress scenario crosses capture boundaries repeatedly and demonstrates the
  documented behavior when old content leaves tmux history.
- Clean logs contain no ANSI bytes; raw logs preserve them; exact duplicate
  counts and boundary markers are asserted.
- The exact `just qcheck` and `just mac-qcheck` recipes pass.

## TASK-100 - add shell and prompt integration scenarios

State: NEW

Goal:

- Cover user-facing shell integration commands without modifying the developer's
  real home or startup files.

Dependencies:

- TASK-096 and TASK-097.

Versioning:

- Increment the package patch version exactly once from the version present at
  task start; update the matching `stay` package entry in `Cargo.lock`, and
  leave all other dependency versions unchanged.

Scope:

- Source the output of `stay --prompt-integration` under `sh`, Bash, and zsh;
  invoke `stay_prompt_segment` with and without `STAY_SESSION_NAME`; verify the
  zsh `PROMPT_SUBST` guidance.
- Assert that `stay shell-integration` emits the same prompt function and does
  not require tmux.
- Assert that `stay shell-integration --s-alias` emits `alias s=stay` in stdout
  only when safe, never edits startup files, and emits the documented warning
  for aliases in `.bashrc`, `.zshrc`, or `.profile`, an `s` executable on PATH,
  or an unreadable startup candidate.
- Use the hermetic HOME and controlled PATH from TASK-096 to create all conflict
  fixtures.

Required Bats scenarios:

- `@test "stay --prompt-integration prints a usable prompt function" {` — run
  `stay --prompt-integration`, source stdout under `sh`, Bash, and zsh, and
  invoke `stay_prompt_segment` with and without `STAY_SESSION_NAME`; assert the
  zsh `PROMPT_SUBST` guidance.
- `@test "stay shell-integration prints the prompt snippet" {` — run
  `stay shell-integration`; assert the same function as the global flag, no tmux
  requirement, and no startup-file changes.
- `@test "stay shell-integration --s-alias adds the safe alias" {` — run
  `stay shell-integration --s-alias`; assert `alias s=stay` is emitted in stdout
  only when safe, never written to startup files, and omitted with the
  documented warning for rc aliases, an `s` executable on PATH, or unreadable
  startup candidates.

Acceptance criteria:

- Assertions distinguish command output from the effect of invoking the emitted
  prompt function.
- No real startup file, PATH command, tmux server, or host shell configuration
  is read or modified.
- The exact `just qcheck` and `just mac-qcheck` recipes pass.

## TASK-101 - measure and finalize the acceptance CI budget

State: NEW

Goal:

- Make the complete single-file acceptance suite reliable and affordable in
  Linux and macOS CI after all scenarios are present.

Dependencies:

- TASK-096 through TASK-100.

Versioning:

- Increment the package patch version exactly once from the version present at
  task start; update the matching `stay` package entry in `Cargo.lock`, and
  leave all other dependency versions unchanged.

Scope:

- Measure the full Bats suite on both CI operating systems with one release
  binary, the pinned Bats installer, the supported tmux installer, and the
  PTY/logging fixtures enabled; record the worst-case runtime and the runtime of
  each scenario in CI artifacts.
- Keep one `tests/acceptance.bats` file and run the complete file in each
  Linux/macOS acceptance job; do not split or omit scenarios.
- Set the dedicated acceptance job's `timeout-minutes` to exactly `15` and
  retain the same Linux/macOS matrix and hermetic setup.
- Keep CI output human-readable, retain artifact diagnostics for failed PTY or
  logging tests, and ensure cleanup runs when a job is cancelled. Do not add a
  second acceptance file or omit scenarios to meet the timeout.

Acceptance criteria:

- The complete suite passes on Linux and macOS with no host configuration
  dependence, leaked sessions, leaked servers, or orphaned child processes.
- CI builds Stay once per job, installs and verifies pinned Bats once, and
  reports enough timing and failure context to diagnose a slow scenario.
- The exact 15-minute timeout is justified in the task's implementation notes by
  the measured worst-case runtime plus headroom. Reduce fixture overhead until
  the complete suite fits that fixed timeout while preserving every scenario; do
  not split or omit coverage.
