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

## TASK-054 - drain tmux command output while waiting

State: COMPLETED

Goal:

- Make `Tmux::run` return a tmux command's complete output regardless of its
  size, so `-l/--log` works on sessions that have real scrollback (project
  review 2026-07-31, findings F1 and F14).

Dependencies:

- None.

Scope:

- `src/tmux.rs`: `wait_with_timeout` polls `child.try_wait()` and only calls
  `read_pipe` after the child has exited. A child that writes more than the OS
  pipe capacity (64 KiB on Linux) blocks in `write`, so it never exits, so the
  pipe is never drained; the command is killed at `COMMAND_TIMEOUT` and returns
  "tmux command timed out after 2 seconds; tmux may be unresponsive", which
  blames tmux for a deadlock the wrapper created. Read both pipes concurrently
  with the wait: take `stdout` and `stderr` before the wait loop, read each on
  its own thread, and join both after the child is reaped. Killing the child on
  timeout closes the pipes, so the readers always finish.
- Keep `COMMAND_TIMEOUT` at two seconds, keep the timeout path reaping the
  child, and keep `CommandOutput`'s fields and every existing error string
  unchanged.
- `src/tmux_version.rs`: `run_version_command` has the same wait-then-read
  shape. Give it the same treatment.
- The path that crosses the threshold in normal use is `capture-pane` in
  `src/logging.rs`, which requests the pane's entire retained history and is run
  through `Tmux::run`. 64 KiB is roughly 800 lines of 80-column output, and
  clean append mode raises `history-limit` to 50,000 lines, so the logging
  feature is designed to operate an order of magnitude past the current breaking
  point. A session with ~2000 lines of 79-column scrollback reproduces it every
  time.
- Because `on_attach_open` runs before the relay loop, this failure aborts the
  attach: the tmux attach child is spawned, but `on_attach_open` fails first, so
  `stay attach <name> -l <file>` returns the timeout error instead of handing
  the user an interactive session, in every `-l` mode.
- `src/logging.rs`: record in the module doc comment that clean mode re-captures
  the whole retained range on every tick and skips the already-captured prefix
  locally, that this is accepted for its atomicity (the alternative, a separate
  history-size query plus a relative `-N` offset, is racy against a growing
  pane), and that it is only affordable because the wrapper no longer deadlocks
  on large output. No behaviour change.

Acceptance criteria:

- A unit test using `Tmux::for_test_shell_script` proves a command emitting at
  least 1 MiB on stdout returns all of it well inside the timeout; a second
  proves the same for stderr; a third proves both at once.
- A real-tmux test captures a pane holding at least 2000 lines of 79 columns
  through `Tmux::run` and receives the whole capture.
- An integration test attaches with `-l` to a session whose retained history
  exceeds 64 KiB, and asserts the attach succeeds and the log contains the
  oldest retained line.
- `wrapper_timeout_reaps_the_child` and `timeout_terminates_a_wedged_command`
  pass unchanged.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-055 - report signal-killed panes instead of failing the attach

State: COMPLETED

Goal:

- A session command killed by a signal ends its attach cleanly, reports
  `128 + signal` as stay's exit status, and is described as signalled in
  listings rather than as `exit=0` (project review 2026-07-31, finding F2).

Dependencies:

- None.

Scope:

- tmux publishes `pane_dead_status` only when the pane's command exited
  normally. A signalled pane reports `pane_dead` `1`, a `pane_dead_time`, an
  empty `pane_dead_status`, and the signal number in `pane_dead_signal`
  (verified against tmux 3.6a).
- `src/relay.rs`: `parse_pane_state_row` parses the third field unconditionally,
  so a signalled pane yields `Err("invalid tmux pane dead status: ...")`. The
  relay's 500 ms pane poll propagates that with `?`, so any attached command
  that dies from a signal - a segfault, an OOM kill, `kill` from another
  terminal - ends the attach with a cryptic internal message, exit status 1, no
  `detach-client` cleanup, and no final log capture. Treat an empty
  `pane_dead_status` as `None`, matching `Tmux::pane_exit_status`, which already
  does this. Add `#{pane_dead_signal}` to the queried format and to `PaneState`,
  parsing an empty value as `None`.
- `src/relay.rs`: auto-detach must still fire for a signalled pane - it is dead,
  only its reported code changes. `exit_status_for_attach` returns
  `128 + signal` when the pane died from a signal during this attach, the
  existing `pane_dead_status` when it exited normally, and `0` otherwise.
- `src/tmux.rs`: add `#{pane_dead_signal}` to `list_sessions`' format and carry
  it through `PaneRecord`, `DeadPane`, and a new `SessionRecord::dead_signal`.
  `status_detail` must render a signalled row as exactly three suffix spans
  (`" [terminated signal="`, then the number with `emphasis: true`, then
  `" @<time>]"`), so that `picker::fitted_suffix`, which indexes `full[0]` and
  `full[1]` for terminated rows, keeps working unchanged.
- Explicit non-goal: `stay list --json` does not change. `exit_code` is already
  `null` for a signalled pane, and adding a `signal` field would change the
  stable schema documented in `design_docs/stay.html`, which is a separate
  decision. `stay.html` documents `exit_code: null` explicitly only for
  non-terminated rows; the terminated-but-signalled row is also `null` but left
  implicit there, so no doc change is required for the JSON to stay
  byte-identical.

Acceptance criteria:

- `parse_pane_state_row("1:1785465643:")` parses as a dead pane with no exit
  status rather than erroring, and a row carrying a signal parses its number.
- A real-tmux attach test whose session command is killed with `SIGKILL` while
  attached: stay detaches cleanly, restores the terminal, exits 137, and prints
  no error.
- A signalled row renders `[terminated signal=9 @<time>]` in both `stay list`
  and the picker, with the number emphasised the way a non-zero exit code is,
  and the narrow-row tests in `src/picker/mod.rs` still pass.
- `stay list --json` output is byte-identical to today for every existing case.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-056 - stop discarding tmux command failures

State: COMPLETED

Goal:

- No tmux control command's failure is silently ignored, and the public session
  API validates names the way `Tmux::rename_session` already does (project
  review 2026-07-31, findings F3 and F21).

Dependencies:

- None.

Scope:

- `Tmux::run` returns `Result<CommandOutput, String>`, where the `Err` means
  "could not spawn or timed out" and `CommandOutput.status` carries tmux's own
  exit status. Nine calls in `src/session.rs` use `?` and then drop the output,
  so a tmux command that ran and failed is indistinguishable from one that
  succeeded. The most serious is `set-option -g remain-on-exit on`: without it a
  finished command's pane disappears, and there is no terminated session to
  review, no exit code to report, and nothing for `-f` to warn about discarding.
  Its failure mode - "sessions vanish when the command finishes" - points
  nowhere near its cause.
- `src/tmux.rs`: mark `CommandOutput` `#[must_use]`, so the compiler finds every
  remaining unchecked call site and prevents the next one.
- `src/session.rs`: route every `tmux.run(...)` whose status is currently
  dropped through the existing `ensure_success` helper already defined in
  `src/session.rs` (which returns `Err` naming the tmux status and stderr) - the
  bootstrap `new-session`, both `set-option -g` calls, and all six calls in
  `apply_builtin_tmux_settings`, nine sites in total. Do not reference
  `tmux::ensure_command_success`: it is private to `src/tmux.rs` and not
  reachable from `src/session.rs`.
- `src/session.rs`: `create_session`, `create_session_with_shell`, and
  `kill_session` validate their session name with
  `crate::session_name::parse_session_name` before running tmux.
- Do not change error text that existing tests assert on. Add new text only
  where a previously silent failure now surfaces.

Acceptance criteria:

- Unit tests using `Tmux::for_test_shell_script` prove that a failing
  `set-option -g remain-on-exit`, a failing `set-option -g history-limit`, and a
  failing built-in-settings command each make session creation return an error
  naming the tmux failure.
- A unit test proves `create_session` and `kill_session` reject an invalid name
  such as `bad.name` without invoking tmux.
- `cargo clippy --all-targets` is clean with `CommandOutput` marked
  `#[must_use]`, which proves no unchecked call site remains.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-057 - replace the bootstrap session with a server-start config

State: COMPLETED

Goal:

- Creating a session can no longer leave an immortal `__stay-bootstrap-*`
  session behind, and the server's required options are still set before the
  real session's command can exit (project review 2026-07-31, finding F7).

Dependencies:

- TASK-056 must be `COMPLETED`.

Scope:

- Session creation starts a placeholder session running
  `/bin/sh -c "sleep 1000000"` purely to bring the server up so `set-option -g`
  has somewhere to land, then removes it with a `Drop` guard. `Drop` does not
  run for `SIGKILL` or an abort, and the window spans two `set-option` calls
  plus the real `new-session`, so a hard kill leaves a session named
  `__stay-bootstrap-<pid>-<nanos>` sleeping for eleven days - listed by
  `stay list`, shown in the picker, keeping the server alive, and cleaned up by
  nothing.
- `tmux -f <file>` loads `<file>` when the server starts, which is early enough
  (verified: the first session's dead pane is retained with its exit status).
  Replace the placeholder with a temporary config file passed as `-f` on the
  `new-session` call.
- `tmux -f` *replaces* the default `~/.tmux.conf` loading rather than adding to
  it (verified: a server started with `-f` does not pick up the user's
  `status-right`). The generated file must therefore start with
  `source-file -q <user tmux config>` when `create_session_with_shell` was given
  one, so today's precedence is preserved exactly: the user's file first, then
  stay's `remain-on-exit` and `history-limit`.
- Write the file mode 0600 under the process's temporary directory, and remove
  it after the create attempt returns, on both the success and failure paths.
- Keep the explicit `set-option -g remain-on-exit`/`history-limit` calls after
  `new-session`, now checked per TASK-056: `-f` applies only when this call
  actually starts the server, and the explicit calls cover an already-running
  one.
- Delete `BootstrapGuard`, the bootstrap name construction, and
  `current_timestamp` if it becomes unused.
- `src/tmux.rs`: `list_sessions` ignores sessions whose name begins
  `__stay-bootstrap-`, with a comment recording that this exists only to hide
  placeholders leaked by versions before this task, and that stay no longer
  creates them. Killing an already-leaked placeholder stays a manual
  `tmux -L stay kill-session` for the user.
- `design_docs/lessons_learned.md`: the "Set global options through a throwaway
  bootstrap session" entry (under "tmux behavior gotchas") prescribes exactly
  the bootstrap-plus-`Drop` pattern this task removes and finding F7 identifies
  as the cause of the leaked immortal sessions; leaving it would make the lesson
  advocate the retired anti-pattern. Replace that entry with this text (adjust
  only if the implementation diverges): "Set global options through a
  server-start config file, not a bootstrap session. `set-option -g` needs a
  running server, and options like `history-limit` are read when a session is
  created, so they must be in force before the real session's command can run.
  Pass the required options in a temporary file via `tmux -f <file>` on the
  session-creating `new-session`: `-f` is loaded when the server starts, which
  is early enough (verified: the first session's dead pane is retained with its
  exit status). `-f` *replaces* `~/.tmux.conf` loading, so the generated file
  must begin with `source-file -q <user config>` to preserve the user's
  precedence. Do not use a throwaway bootstrap session guarded by `Drop`: `Drop`
  does not run on `SIGKILL`, which leaked immortal `__stay-bootstrap-*` sessions
  before TASK-057 removed the pattern. Keep the explicit `set-option -g` calls
  after `new-session`, and check their status, for the already-running-server
  case. Do not assume options apply retroactively."

Acceptance criteria:

- No code path creates a session named `__stay-bootstrap-*`, proved by a test
  that creates one session on a test namespace and asserts the server holds
  exactly one session.
- Creating a session whose command exits immediately still retains the dead pane
  with its exit status: `quick_exits_are_retained_and_report_their_statuses`
  passes unchanged.
- With a user tmux config present, its settings still apply and stay's built-in
  status settings still do not:
  `built_in_tmux_settings_do_not_apply_with_a_user_config` passes unchanged, and
  a new test proves an option set only in the user's config survives session
  creation.
- With no user tmux config, `remain-on-exit`, `history-limit`, and the built-in
  status settings are all applied as before.
- The temporary config file does not exist after `create_session` returns, on
  either path.
- `design_docs/lessons_learned.md`'s bootstrap-session entry is replaced with
  the server-start-config guidance, so the document no longer advocates the
  removed pattern.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-058 - collect the session inventory in one tmux call

State: COMPLETED

Goal:

- Refreshing the inventory costs one tmux process instead of 2N+1, so the
  picker's twice-a-second poll stops burning a fifth of a core, and each
  snapshot becomes atomic (project review 2026-07-31, findings F6, F19, and
  F23).

Dependencies:

- TASK-055 must be `COMPLETED`, because it changes the same format string.
- TASK-054 must be `COMPLETED`: it makes the wrapper spawn threads, which is
  what breaks `format_dead_time`'s per-call local-offset lookup; the
  offset-caching fix below depends on that being the established behaviour.

Scope:

- `Tmux::list_sessions` issues one `list-panes -a -F` and then, per pane, two
  more `display-message -p` calls through `enrich_pane` to fetch
  `pane_current_path` and `pane_current_command`. Measured with 8 sessions that
  is 17 tmux processes and 103 ms per refresh, twice a second for as long as the
  picker is open; one call carrying the same fields takes 12 ms. It also makes
  the poll non-atomic, because each pane's path and command are read after the
  snapshot, so rows can mix data from different instants.
- `src/tmux.rs`: fetch both fields in the single `list-panes -a -F` call. These
  two dynamic fields are deliberately not in the delimited row today (they are
  fetched per-pane, the TASK-028 R002 fix); folding them back reintroduces the
  delimiter-collision hazard, so it must be mitigated, not merely moved.
  Separate fields with the ASCII unit separator `0x1f` rather than `:`, because
  a path or a command may contain a colon and `parse_session_row` rejects a row
  with an extra field. Two constraints the implementer must honour:
  - Emit the real `0x1f` byte, not the four-character text `\x1f`. tmux's `-F`
    engine does not interpret backslash escapes - verified that both `\t` and
    `\x1f` are emitted as literal characters - so the format string must carry
    the byte itself (a Rust `"\u{1f}"` / `"\x1f"` string literal compiles to it;
    a raw string or a runtime-built escape does not). This is the TASK-005
    literal-escape failure class, which was first caught only on macOS, so the
    `just mac-qcheck` gate is load-bearing here, not incidental.
  - A path may legally contain a `0x1f` byte, so that residual (vanishingly
    rare) case still misparses. This is accepted; record it in a comment beside
    the delimiter rather than guarding it. Delete `enrich_pane` and
    `pane_value`.
- `design_docs/lessons_learned.md`: the TASK-028 R002 entry ("A safe delimiter
  for fixed fields is not enough once a row carries dynamic, user-influenced
  fields...") concludes that dynamic pane state "must never share a delimited
  row with the fixed fields at all" and prescribes the per-pane
  `display-message` design this task removes. Update that entry to record that
  dynamic fields may share the batched row behind a delimiter the value cannot
  contain (the `0x1f` unit separator), that this makes the snapshot atomic and
  cuts refresh from 2N+1 tmux processes to one, and to carry both constraints
  above (emit the real byte, verified on macOS; the `0x1f`-in-path residual is
  accepted). Keep the instruction to cover a colon-containing working directory
  and command in the test.
- Update `parse_session_row` and its unit tests to the new delimiter and field
  list, keeping every existing malformed-row rejection.
- `src/main.rs`: `require_existing_session` and `dispatch_create` call
  `list_sessions` only to answer "does this name exist?". Use
  `has-session -t <name>` instead, treating a missing server as "does not
  exist".
- `src/tmux.rs`: `paste_stdin_chunk` passes `-t <session>` to `load-buffer`,
  where `-t` is a target-*client*, not a target-pane, so the session name is
  meaningless there. tmux tolerates it today - verified: exit 0 even for a
  nonexistent client target - so this is latent rather than broken. Drop the
  flag; the buffer is server-global and named, and `paste-buffer` keeps its
  correct `-t`.
- `src/tmux.rs`: `format_dead_time` resolves the local UTC offset on every call
  and falls back to UTC when the `time` crate refuses to determine it, which it
  does in a multi-threaded process - and TASK-054 makes the wrapper spawn
  threads. Resolve the offset once, before any thread exists, and cache it for
  later calls. RFC 3339 renders the fallback as `Z` and a local offset as, for
  example, `+10:00`, so the output already says which it used; the point of this
  change is that the answer stops depending on when it is asked.
- `formats_termination_time_using_the_recorded_local_offset` computes its
  expected value with the same expression it is testing, so it passes either
  way. Replace it with a test that asserts an exact string for a fixed offset.
- This task also edits `list_sessions`, which TASK-057 (earlier in order)
  teaches to skip `__stay-bootstrap-` names. Preserve that filter; do not drop
  it while reworking the query, so a reorder or a merge cannot silently
  reintroduce the leaked-placeholder listing.

Acceptance criteria:

- `list_sessions` issues exactly one tmux command, proved by a shell-script shim
  test that counts invocations.
- A real-tmux test proves a session whose working directory and whose running
  command both contain a colon are reported correctly, and the same for a value
  containing the `0x1f` byte is documented as the accepted residual.
- `stay attach <missing>` and `stay create <existing>` still fail with today's
  exact error messages, now via `has-session`.
- `design_docs/lessons_learned.md`'s TASK-028 R002 delimiter entry is updated to
  the folded-row-with-`0x1f`-delimiter guidance, including the
  emit-the-real-byte and macOS-verification constraints.
- `pass_through_delivers_a_bounded_multiline_chunk_in_order_without_attaching`
  passes unchanged.
- A timestamp test asserts an exact formatted string for a fixed offset.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-059 - make -l logging honest about its target and its cursor

State: NEW

Goal:

- `--raw` logs to the path the user just asked for, the append cursor cannot
  silently skip content, and the offset sidecar is as hardened as the log itself
  (project review 2026-07-31, findings F9, F10, and F13).

Dependencies:

- None.

Scope:

- `LogSession::start` guards the whole `--raw` setup with
  `if !pane_has_active_pipe(..)`. The guard is meant to stop the backfill
  truncating away what a running pipe has appended, but it also skips
  `start_pipe_pane`. So attaching with `-l second.log --raw` to a session
  already piping to `first.log` keeps writing to `first.log`, writes nothing to
  `second.log`, and warns about nothing - while `Mode::Raw`'s tick opens the new
  path for append and reports success, so the only feedback the user gets is
  positive. tmux exposes only `#{pane_pipe}` (0 or 1), never the running pipe's
  command, so the existing target cannot be compared.
- `src/logging.rs`: always start the pipe for the requested path, dropping
  `pipe-pane`'s `-o` flag so the call replaces any existing pipe, and keep the
  backfill gated on `#{pane_pipe}` being 0 so a re-attach still never truncates.
  Replacing a pipe that already targets the same path is harmless because the
  command appends, and a different path now takes effect.
- `capture_once` calls `write_cursor(path, current_lines)` unconditionally,
  including on the branch where `append_bytes` has just failed and only produced
  a warning. Those lines are marked captured and never retried, so a transient
  write failure becomes permanent, unmarked loss - in the one mode that
  otherwise marks history eviction explicitly. Advance the cursor only by what
  was actually written.
- The cursor lives in `<log>.offset`, keyed on the path alone, so pointing two
  sessions at one log file, or rotating the log while leaving the sidecar,
  silently skips content: a cursor larger than the current capture is
  indistinguishable from a resumed session. Record the session name and the log
  file's current size alongside the line count, and fall back to a full capture
  when either disagrees with what is on disk.
- `validate_log_target` refuses a symlinked log, a non-regular file, a file
  owned by another user, and any group or other permission bit, but
  `write_cursor` calls `fs::write` on `<log>.offset.tmp` with no check at all,
  following a symlink planted at that path. Apply the same validation to the
  sidecar and its temporary file, or open both `O_NOFOLLOW`.

Acceptance criteria:

- An integration test attaches with `-l a.log --raw`, detaches, attaches with
  `-l b.log --raw`, and asserts new output reaches `b.log`.
- `raw_log_mode_reattach_does_not_truncate_the_still_piping_log` passes
  unchanged.
- A unit test proves a failed append leaves the cursor where it was, so the next
  capture re-emits those lines.
- A unit test proves a sidecar whose recorded session name or log size disagrees
  with the file on disk causes a full capture instead of a skip.
- A unit test proves a symlink at `<log>.offset` or `<log>.offset.tmp` is
  refused rather than written through.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-060 - keep the picker alive across attach failures and signals

State: NEW

Goal:

- A failed attach returns to the picker instead of exiting stay, a signal cannot
  leave the terminal in raw mode on the alternate screen, and the shortcut panel
  lists every key the picker implements (project review 2026-07-31, findings F5,
  F8, and F17).

Dependencies:

- None.

Scope:

- `picker::run` calls `session::attach_session_with_input(..)?`, so any attach
  failure exits stay. The list is a snapshot refreshed every 500 ms, so the
  ordinary race - a session killed or finished elsewhere between the last poll
  and pressing Enter - takes the user from "pick a session" to a bare shell with
  a one-line error. Every other picker action handles failure the opposite way:
  stored in `state.action_error`, shown in the status line, loop continues. Do
  the same here, which means passing an initial error into `run_picker` rather
  than starting from `PickerState::default()`, and re-polling so the stale row
  disappears.
- The picker holds raw mode, the alternate screen, and a hidden cursor, and has
  only a panic hook and a `Drop` guard, neither of which runs for a signal. A
  `SIGTERM` or `SIGHUP` while the picker is open leaves the user's shell in raw
  mode on the alternate buffer with no cursor - the exact state the relay goes
  to some length to prevent, reachable through the more commonly used of the two
  screens. Install handlers for `SIGTERM`, `SIGHUP`, and `SIGINT` that set a
  flag, check it in the `run_picker` loop, which already wakes every 50 ms, and
  exit through the normal `TerminalGuard` drop. Model this on
  `relay::unix::SignalGuard`'s shape - a guard restoring the previous
  dispositions on drop - rather than inventing a second mechanism.
  `relay::unix::SignalGuard` is a private struct in `relay.rs`'s private `unix`
  module and covers `SIGTERM`/`SIGPIPE`, so reuse its restore-on-drop shape as a
  template in the picker; do not import it, and extend it to the three signals
  named here.
- `IDLE_STATUS` advertises selection, both attach modifiers, Enter, `r`, `e`,
  `k`, `K`, and Esc, but `handle_idle_key` also implements `c` (open the create
  prompt) and `q` (quit), and the spec lists `c create` among the picker's keys.
  Add `c create`, and fold `q` into the quit entry. The panel's width feeds
  `picker_area`, so the exact-text and width tests must be updated with it.

Acceptance criteria:

- An integration test kills the selected session between the picker's poll and
  the Enter keystroke, then asserts stay stays in the picker, displays the
  failure, and no longer lists that session.
- A PTY test sends `SIGTERM` to a running picker and asserts the outer terminal
  is left in cooked mode with the alternate screen exited, mirroring
  `sigterm_detaches_and_restores_cooked_terminal_settings` for the relay.
- The shortcut panel contains `c create` and a quit entry naming both `q` and
  `Esc`, with `status_text_matches_this_milestone` and the picker width tests
  updated to the new text.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-061 - stop the relay blocking on its own attach PTY

State: NEW

Goal:

- A large paste into a busy pane cannot deadlock the relay against its own tmux
  child (project review 2026-07-31, finding F15).

Dependencies:

- None.

Scope:

- `write_input` loops until every byte reaches the attach PTY master, which is a
  blocking descriptor, while the tmux client on the other side writes output to
  the same PTY. If the output direction fills while stay is inside
  `write_input`, the child stops reading its input because it is blocked writing
  output, and neither side progresses: stay is not draining output because it is
  inside `write_input`, and the child is not draining input because it is inside
  its own write. It needs a multi-kilobyte paste concurrent with heavy pane
  output, so it is uncommon - but pastes are exactly where multi-kilobyte input
  happens, and the result is a hard hang of an interactive session.
- `src/relay.rs`: set the master non-blocking, hold whatever `write_input` could
  not write in a pending buffer owned by the relay loop, add `POLLOUT` to the
  existing `poll` set whenever that buffer is non-empty, and drain it as the
  child reads. Output must keep being forwarded while input is pending.
- Stop polling stdin while the pending buffer is above a fixed bound, so a fast
  producer cannot grow it without limit.
- Keep the current treatment of `EIO`/`EPIPE` as a normal shutdown and `EINTR`
  as a retry, and keep the detach-key and copy-mode-key scanning semantics
  exactly as they are, including that bytes preceding a control key are
  forwarded before the tmux-side call runs.

Acceptance criteria:

- A PTY test writes at least 1 MiB into a relay whose pane is simultaneously
  producing continuous output, and asserts every byte arrives at the pane and
  the relay neither hangs nor drops input.
- `closed_attach_pty_input_is_a_normal_shutdown`,
  `forwards_ordinary_input_bytes_verbatim`,
  `attaches_through_a_real_pty_and_detaches_with_stay_key`, and
  `copy_mode_key_enters_tmux_copy_mode_without_forwarding` pass unchanged.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-062 - CLI, config, and API-surface hygiene

State: NEW

Goal:

- Four small independent corrections, each too small to warrant its own task
  (project review 2026-07-31, findings F12, F16, F18, and F22).

Dependencies:

- None.

Scope:

- `src/main.rs` writes clap's output to stderr for every error kind, including
  `DisplayHelp` and `DisplayVersion`, so `stay --help | less` shows an empty
  page and `version=$(stay --version)` captures nothing. Write those two kinds
  to stdout and everything else to stderr, keeping today's exit codes.
  `tests/cli_help.rs`'s two assertions currently require stderr and must move to
  stdout.
- `src/config.rs`: `parse_key_spec` applies `to_ascii_uppercase() & 0x1f` to any
  single ASCII character, so specs that do not mean what they say are accepted
  silently - `Ctrl+2` becomes `0x12`, which is what `Ctrl+R` sends. Accept ASCII
  letters, the named `Space`, and the punctuation that genuinely produces a
  control byte (`@`, `[`, `\`, `]`, `^`, `_`), and reject anything else with a
  message naming what is allowed. This turns a previously silent
  misinterpretation into a startup error, which is the intent. Keep `Ctrl+?`
  mapping to `0x7f`, and document beside it that it collides with Backspace on
  most terminals.
- `src/config.rs`: environment overrides are taken verbatim, so an exported but
  empty `STAY_CMD` yields `Some("")` and every new session runs `sh -c ""`,
  exits immediately, and appears to die at birth. Treat an empty value for each
  `STAY_*` override as unset. Document that `history_lines = "unlimited"` means
  `UNLIMITED_HISTORY_LINES` (1,000,000) lines rather than no limit, wherever
  `history_lines` is described.
- Remove dead public API before it is published: `relay::attach`, which
  production never calls because everything goes through `attach_with_input`;
  and the `Serialize` derives on `JsonSession` and `JsonEnvelope`, whose JSON is
  written by hand in `render_session_json` and which cannot have been exercised
  because `serde_json` is not a dependency. Leave `Tmux::attach_command` `pub`:
  despite its doc comment it has a live caller in the integration-test crate
  (`tests/attachment.rs`,
  `production_wrapper_keeps_the_runtime_namespace_fixed_to_stay`), which links
  the non-test library build, so both `pub(crate)` and `#[cfg(test)]` would fail
  to compile under `--all-targets`.

Acceptance criteria:

- `stay --help` and `stay --version` write to stdout and exit 0, while every
  parse error still writes to stderr and exits 1.
- Unit tests prove `Ctrl+2` and `Ctrl+;` are rejected, and that `Ctrl+A`,
  `Ctrl+Space`, `Ctrl+\`, and `Ctrl+?` still resolve to the bytes they resolve
  to today.
- A unit test proves an empty `STAY_CMD` behaves as unset, leaving
  `default_command` as `None`.
- `cargo clippy --locked --all-targets --all-features` is clean after the API
  removals, and no test references a removed item.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-063 - make the test suite environment-independent and self-cleaning

State: NEW

Goal:

- The suite means the same thing wherever it runs, leaves nothing behind, and
  stops relying on fixed short deadlines that fail on loaded CI runners (project
  review 2026-07-31, findings F4, F25, F26, and F27).

Dependencies:

- None.

Scope:

- Integration tests spawn the real binary and inherit the caller's whole
  environment, and three inherited variables change what the binary does.
  `TMUX`: `dispatch` refuses to run inside tmux, so with it set 42 tests fail -
  34 of 36 in `tests/attachment.rs` and 8 of 11 in `tests/cli_surface.rs` - each
  panicking with "stay exited before attaching", a message that never names the
  cause. `HOME`: stay applies its built-in tmux settings only when there is no
  `~/.tmux.conf`, so the three tests asserting on the built-in status line fail
  on any machine that has one. `XDG_CONFIG_HOME` and the `STAY_*` overrides:
  `Config::load` reads them, so a host stay config or an exported `STAY_CMD`
  silently changes the program under test, including the detach key the tests
  type. Development happens inside a container that has none of these, which is
  why the suite has been legitimately green throughout.
- `tests/`: add one spawn helper that every real-binary spawn goes through. It
  clears `TMUX` and every `STAY_*` variable `Config` reads, and points `HOME`
  and `XDG_CONFIG_HOME` at a fresh empty temporary directory. Tests that want
  those values keep setting them explicitly, as `tests/cli_help.rs` already does
  for `TMUX` and one attach test does for `HOME`, so no coverage is lost.
- The same helper sets `TMUX_TMPDIR` to a short per-run temporary directory.
  tmux honours it, so every server a test starts - including the production
  `-L stay` namespace, which is what a spawn site that forgot the PATH shim
  would reach - lands in a disposable directory instead of the user's real
  socket directory. Keep that path short: a unix socket path is limited to about
  108 bytes.
- `tests/tmux_sweep.rs`: the sweep test creates a `stay-user-sweep-*` namespace
  as its control, correctly outside the sweeper's `stay-test-` prefix, and
  cleans up with `kill-server` - which stops the server but leaves the socket
  file (verified). Nothing unlinks a socket file whose name lacks that prefix,
  so every run leaks one; 127 had accumulated in the development container,
  oldest two days old, all dead. Unlink the socket after `kill-server`, using
  the test's own `socket_path` helper.
- Tests create files under `temp_dir()` and remove them with
  `let _ = fs::remove_file(..)` after their assertions, so a failing assertion
  panics first and every failed run leaves fixtures behind; one site
  (`tests/tmux_sweep.rs:113`, `remove_file(path).expect(..)`) forces the
  removal, which can turn a cleanup hiccup into a second, misleading failure.
  Add one `Drop`-based temporary-path guard and use it at every site, extending
  the pattern `ServerGuard` and `TestServerGuard` already use for tmux servers.
- The real-tmux termination waits use fixed five-second windows and time out on
  loaded runners. `design_docs/known_issues.md` records two such timing
  failures: CI run #58 (next action "retrigger") and CI build #81 (which passed
  only on its third retrigger, next action "investigate later"). Raise those
  waits to a deadline a loaded shared runner can meet, then close the two timing
  entries (CI run #58 and CI build #81) with a note recording what changed.
- Leave the third entry, `panic_restores_the_picker_terminal_state` (CI run
  #55), `OPEN`. Its cause is a fork interacting with crossterm's process-global
  raw-mode state, not a deadline, and it is out of scope here. Replace its
  "retrigger" next action with that description.

Acceptance criteria:

- `cargo test --locked --all-targets --all-features` passes with `TMUX` set to a
  non-empty value, with `HOME` pointing at a directory that contains a
  `.tmux.conf`, and with `STAY_CMD`, `STAY_DETACH_KEY`, and `STAY_HISTORY_LINES`
  all exported.
- No test creates anything under `/tmp/tmux-<uid>/`, proved by asserting the
  suite's tmux sockets appear under the per-run `TMUX_TMPDIR`.
- A full passing run leaves no socket, file, or directory behind, and a unit
  test proves the temporary-path guard removes its path while a panic unwinds
  through its scope.
- `design_docs/known_issues.md` has the two timing entries closed and the
  picker/PTY entry still `OPEN` with a concrete next action.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-064 - README, manifest lints, and CI coverage

State: NEW

Goal:

- Give stay a README that documents it, stop the manifest turning future
  compiler warnings into build failures for everyone building the crate, and
  close the CI gaps (project review 2026-07-31, findings F11, F20, and F24).

Dependencies:

- None.

Scope:

- `README.md` documents only "Recovering a deleted tmux socket". It is what
  crates.io will show as the crate's front page under TASK-037, so it must first
  cover: what stay is and why it exists, in a paragraph; installation; each of
  `stay`, `stay list [--json]`, `stay create`, `stay attach`, and `stay kill`
  with an example; the picker's keys; the config file's location and keys and
  the `STAY_*` environment overrides; the detach and copy-mode keys and how to
  change them; and that tmux 3.3 or newer is required, as `src/tmux_version.rs`
  enforces. Keep the existing socket-recovery section as a troubleshooting
  subsection.
- `Cargo.toml`: `[lints.rust] warnings = "deny"` makes any warning a future
  compiler introduces a hard build failure for anyone building the crate, not
  just in CI, where `just lint` already enforces the same standard with
  `cargo clippy ... -- -D warnings`. Remove the manifest-level deny or reduce it
  to `warn`, leaving the clippy lint table as it is.
- `.github/workflows/ci.yml`: add a macOS job that installs tmux and zsh with
  Homebrew and runs `cargo test --locked --all-targets --all-features`. It must
  not run the Docker-based format and lint recipes. The project maintains a
  whole macOS path - `just mac-check`, `scripts/maccmd`, and a `mac-qcheck`
  criterion on every task - that CI never exercises, on code full of `forkpty`,
  termios, and `poll` behaviour that differs between the platforms; the suite
  already carries one `#[cfg(not(target_os = "macos"))]` exclusion.
- `.github/workflows/ci.yml`: the workflow runs `just format` and then
  `just lint`, which itself depends on `format`, so the whole Dockerised
  formatter set runs twice per build. Drop the redundant step.
- `.github/workflows/ci.yml`: replace `cargo install just --locked`, which
  compiles just from source on every run, with a prebuilt install action, and
  pin `dtolnay/rust-toolchain` to a released tag instead of the mutable
  `@stable` ref, matching how the other actions are pinned.
- Add a dependency-advisory gate: a `just audit` recipe running
  `cargo audit --locked`, installed the same prebuilt way, plus a CI step that
  runs it. Nine dependencies including `nix`, `ratatui`, and `crossterm` are
  currently watched by nothing.

Acceptance criteria:

- `README.md` covers every item listed in Scope, and each example is accurate
  against the current CLI surface.
- `Cargo.toml` no longer denies all rustc warnings, and `just qlint` still fails
  on a warning, which proves the standard is still enforced.
- The CI workflow has a macOS job running the suite, runs the format set once,
  installs `just` from a prebuilt binary, pins the toolchain action, and runs
  `just audit`.
- `just audit` passes, or every finding it reports is recorded and addressed.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-037 - publish to crates.io as `stay`

State: BLOCKED

Goal:

- Implement TODO-011: publish `stay` to crates.io so it can be installed via
  `cargo install stay`.

Research already done:

- Checked crates.io directly (`GET https://crates.io/api/v1/crates/stay` →
  `404`): the name `stay` is currently unclaimed and available to register —
  this is worth confirming again immediately before actually publishing, since
  availability can change between now and whenever this task is picked up.
- `Cargo.toml` today has only `name`, `version`, `edition`, and `rust-version` —
  none of the metadata crates.io requires or strongly expects for publish
  (`description`, `license` or `license-file`, `repository`). `cargo publish`
  will refuse without at least `description` and a license field.
- No `LICENSE` file exists in the repo root yet. **Decided: MIT.** The exact
  license metadata and copyright line are specified in Scope below.
- `git remote -v` shows an existing GitHub remote
  (`git@github.com:nevdelap/stay.git`); the exact HTTPS repository URL is fixed
  by the Scope below.

Dependencies:

- The Issue 1 follow-up set (TASK-039 through TASK-048) must be `COMPLETED`. It
  already is - those tasks are done and removed from the active plan, preserved
  in git history - but this release must still include the complete set.
- The 2026-07-31 project-review fixes must all be `COMPLETED` before this task
  starts: TASK-054, TASK-055, TASK-056, TASK-057, TASK-058, TASK-059, TASK-060,
  TASK-061, TASK-062, TASK-063, and TASK-064. The first published release must
  not ship the F1-F27 defects, and two of these fixes are prerequisites for a
  clean publish specifically: TASK-062 removes dead public API before it becomes
  part of the crate's stable surface, and TASK-064 writes the README that
  crates.io renders as the crate's front page and adds the manifest-lint and
  audit gates.

Scope:

- `Cargo.toml`: set the following exact publish metadata:
  - `description = "A terminal session manager for persistent tmux sessions."`
  - `license = "MIT"`
  - `repository = "https://github.com/nevdelap/stay"`
  - `keywords = ["tmux", "terminal", "session-manager"]`
  - `categories = ["command-line-utilities"]`
- Add a new root `LICENSE` containing the standard MIT license text with
  `Copyright (c) 2026 Nev Delap`.
- Bump only the patch component of the package version exactly once from the
  version in `Cargo.toml` when this task starts; update `Cargo.lock` and the
  existing version assertion in `tests/cli_help.rs` to the same value.
- Add a manual `just publish` recipe. It must run
  `cargo publish --locked --dry-run` first and then `cargo publish --locked`; it
  must not run from CI or from another recipe. Document in `README.md` that
  publishing is manual, requires the operator's crates.io credentials, and is
  performed by explicitly invoking `just publish`.
- Before the real publish, query `https://crates.io/api/v1/crates/stay` and
  require HTTP 404; stop without changing release metadata for any other
  response. Run `just qcheck` and `just mac-qcheck` before invoking the real
  publish.
- After publishing, create a fresh temporary directory named `install_root`, run
  `CARGO_INSTALL_ROOT=install_root cargo install --locked --version <new-version> stay`,
  and verify `install_root/bin/stay --version` reports exactly
  `stay <new-version>`. The operator must have valid crates.io publish
  credentials; if credentials are unavailable, stop before the real publish.

Acceptance criteria:

- All dependency tasks listed above are `COMPLETED`: the Issue 1 follow-up set
  (TASK-039 through TASK-048) and the 2026-07-31 project-review fixes (TASK-054
  through TASK-064).
- The exact metadata, MIT license, single patch-version bump, lockfile, and
  version assertion are present and consistent.
- `just qcheck`, `just mac-qcheck`, and `cargo publish --locked --dry-run` all
  pass before publication.
- `just publish` performs the real publication only after its dry run passes; no
  CI workflow publishes the crate.
- A fresh temporary Cargo install using the published version succeeds, and the
  installed binary reports the same version.
- The manual publish workflow and credential precondition are documented in
  `README.md`.
