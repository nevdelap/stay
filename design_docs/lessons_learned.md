# Lessons Learned

This document is durable, in-tree guidance for the implementer and reviewer
agents (Igor and Rufus) working on `stay`. It distills mistakes actually made
during the build — preserved in task commits and review history — plus findings
from whole-application reviews, so future work does not repeat them.

It complements, and does not replace, `design_docs/agent_workflow.md` (the
process contract) and `docs/roles.md` (role definitions). Where this document
and those disagree, those win; open a task to reconcile them.

## Verification discipline

- Both gates are mandatory. A patch is not `IMPLEMENTED`, and cannot be marked
  `COMPLETED`, until the exact `just qcheck` and `just mac-qcheck` recipes both
  pass. "The macOS gate could not be run" is not a pass. Do not substitute an
  SSH wrapper, `ssh -F /dev/null`, an `XDG_RUNTIME_DIR` override, or a manual
  remote test command for the real recipe.
- The macOS gate catches what Linux cannot. It has repeatedly surfaced real
  portability bugs that the Linux gate passed clean: a tmux format string that
  produced literal `\t` instead of tabs, and the test process failing to find
  `/usr/local/bin/tmux` on the Mac. Treat a green Linux run as necessary, never
  sufficient.
- Two consecutive clean `just qcheck` runs, after the final amend, with no
  further file changes. If a quiet recipe rewrites files, inspect the diff,
  stage the good changes, and run again. A run counts only when it ends with no
  new changes.
- A transient failure in an unrelated, timing-sensitive test does not justify
  weakening a gate: inspect `check.log`, run the named test in isolation to
  distinguish a pre-existing flake from a regression, then rerun the exact quiet
  gate cleanly. This occurred during TASK-030 and TASK-035 review.
- Keep CI's platform and dependency checks representative of the real project:
  pin the Rust toolchain, run the full test suite on macOS with Homebrew's tmux
  and zsh, install CI-only tools from prebuilt actions, and run a dependency
  advisory scan. A manifest `rustc` warning policy may remain `warn`, but the
  normal Clippy gate must still compile with `-D warnings` (TASK-064 review).
- Changed-file quality gates must filter compiler diagnostics by source span,
  not merely by the command's exit status. A warm cache can make Clippy return
  non-zero for an unchanged warning; report changed-file diagnostics while
  preserving command failures that contain no compiler diagnostics. Keep changed
  and all-files dispatcher tests at the command boundary (TASK-065 review).
- Read `check.log` on failure. The quiet recipes write full output there; do not
  re-run the verbose recipe to see what happened.
- When a task changes the package version, bump it exactly one patch above the
  task baseline and update `Cargo.lock` plus every version assertion together. A
  `Cargo.toml`-only bump can pass an unlocked local build but fails the locked
  quality gate when package metadata disagrees (TASK-045/TASK-046 reviews).
- Commit-message list items must be part of one body, not separate
  `git commit -m` arguments: Git inserts a blank paragraph between separate
  message arguments, producing a visually broken message even when every line is
  short. Run `scripts/quality.py commit-message` and gitlint after every amend,
  then inspect the stored `%B` before handing off (TASK-065 housekeeping).
- Do not conflate "the file differs from the last commit" with "the formatter
  has more to do." Checking mdformat idempotency with `git diff --exit-code`
  reports dirty on a file you have just rewritten, because it differs from HEAD,
  not because mdformat changed it. Verify idempotency by running the formatter
  twice and comparing a checksum (`md5sum`) across the two passes — stable means
  the file is already in canonical form. `git diff` answers a different question
  than "is this file formatter-clean."

## Commit attribution

- Commit trailers must identify the actual model, version, and variant, not a
  role, provider, tool, or agent. Add exactly one `Co-Authored-By:` trailer per
  distinct model; if both roles use the same model, include it once. Never
  invent a variant to fill the template—look up the real identity. TASK-013 R001
  rejected role-qualified and fabricated model names.

## The tmux boundary

- Everything goes through `src/tmux.rs`. It is the single seam to the outside
  world. Production construction is fixed to the `stay` namespace and cannot be
  redirected; tests use `Tmux::for_test_namespace`, gated on a `stay-test-`
  prefix. Never add a code path that lets CLI, config, or environment choose the
  production namespace.
- Pass user values as separate arguments, never through a shell. Session names
  and command argv are distinct `Command`/`OsString` arguments. There is a test
  proving shell metacharacters in arguments survive verbatim; keep it true.
- Bound every short-lived tmux call, and reap the child on timeout. Use the
  shared `COMMAND_TIMEOUT` and the `wait_with_timeout` path. The one legitimate
  unbounded call is the long-lived interactive attach child while the user is
  attached — that exception is deliberate and documented; do not add others.
- tmux error classification is English-substring matching. The missing-server,
  missing-session, and last-session-shutdown checks all key off hardcoded
  English fragments. This works because tmux ships no translations, but it
  couples correctness to tmux's wording. If a future tmux release changes a
  message, this is the first place to look. Prefer structured signals (exit
  codes, format fields) over prose when tmux offers them.

## tmux behavior gotchas

- Use `:` as the list format delimiter, not `\t`. Some tmux builds emit the
  literal backslash-t rather than a tab in `-F` format strings. Session-name
  validation already rejects `:`, so a colon is an unambiguous, portable
  separator. This bit TASK-005 and was only caught on the Mac.
- A safe delimiter for fixed fields is not enough once a row carries dynamic,
  user-influenced fields. TASK-028 initially added `pane_current_path` and
  `pane_current_command` into the same colon-delimited `list-panes -F` row as
  the fixed fields; a working directory containing a literal `:` (unlike a
  session name, an ordinary filesystem path is not restricted) split into extra
  fields and made `parse_session_row` reject the whole row as malformed. TASK-
  058 folds the dynamic fields back into the atomic batched row behind the ASCII
  unit separator `0x1f`, which their ordinary values cannot contain, cutting
  refreshes from 2N+1 tmux processes to one. Emit the real byte in the format
  string, not the four-character `\\x1f` spelling; this was verified on macOS. A
  path containing `0x1f` remains an accepted, vanishingly rare residual that
  misparses. Keep the real-tmux regression for colon-containing working
  directories and commands so the original collision cannot silently regress.
- "No server for this socket" means an empty inventory, not an error. Killing
  the last session lets the tmux server exit; listing and kill paths must treat
  a missing server identically to zero sessions.
- Set global options through a server-start config file, not a bootstrap
  session. `set-option -g` needs a running server, and options like
  `history-limit` are read when a session is created, so they must be in force
  before the real session's command can run. Pass the required options in a
  temporary file via `tmux -f <file>` on the session-creating `new-session`:
  `-f` is loaded when the server starts, which is early enough (verified: the
  first session's dead pane is retained with its exit status). `-f` *replaces*
  `~/.tmux.conf` loading, so the generated file must begin with
  `source-file -q <user config>` to preserve the user's precedence. Do not use a
  throwaway bootstrap session guarded by `Drop`: `Drop` does not run on
  `SIGKILL`, which leaked immortal `__stay-bootstrap-*` sessions before TASK-057
  removed the pattern. Keep the explicit `set-option -g` calls after
  `new-session`, and check their status, for the already-running-server case. Do
  not assume options apply retroactively.
- Verify tmux feature and hook assumptions against the actual version's
  documented behavior before designing around them. In particular, do not invent
  a termination hook: check `show-hooks`/the shipped documentation and record
  any unattended-event gap explicitly when tmux offers no such hook (TASK-030).
- A format variable's rendered shape can differ across tmux versions even when
  its name and purpose stay the same. `#{pane_dead_signal}` returns the raw
  signal number on tmux 3.4 (Linux) but the platform's short signal name via
  `sig2name()` (e.g. `"kill"`, not `"9"`) on tmux 3.7b (macOS) - discovered only
  because `just mac-qcheck` runs against a real, differently-versioned tmux.
  Parse such a field defensively: try a numeric parse first, then fall back to
  resolving a name (upper-cased, `SIG`-prefixed) through `Signal::from_str`,
  which already maps each name to the current platform's own number, since Linux
  and BSD disagree on several (e.g. `SIGUSR1`) (TASK-055).
- A dead pane's metadata is not stamped atomically: tmux can briefly report
  `pane_dead=1` while `pane_dead_time`, `pane_dead_status`, and
  `pane_dead_signal` are all empty. Treat those fields as optional and let the
  next poll observe the completed state; do not turn this transient row into a
  hard attach failure. Exercise the shape under concurrent real-tmux load, since
  it appeared reliably enough to fail the signal-detach acceptance test on a
  busy runner (TASK-055 R001).
- A persistent `pipe-pane -o` stream must not be backfilled by truncating the
  same log on every reattach. Query the pane's active-pipe state
  (`#{pane_pipe}`) and only perform the initial capture/write/start sequence
  when no pipe exists; `-o` means "open only if none exists," not "toggle the
  existing pipe" (TASK-030).
- Backfill a log from one atomic pane capture and perform any cursor/history
  accounting against that captured data. A separate history-size query followed
  by a later relative capture races output arriving between the two commands and
  can silently drop lines (TASK-030).
- Treat cleanup and orphan-reaping probes as best-effort maintenance. Keep each
  command bounded; a probe or follow-up kill failure must leave the matching
  resource untouched and let the sweep continue rather than panic or abort the
  test process. Add a regression fixture for an unresponsive resource
  (TASK-035).
- When pinning a compatibility floor, verify the exact feature-introduction
  release against upstream release notes or history, and update the floor,
  evidence, and regression test together. Do not rely on a remembered or
  approximate version (TASK-033).
- Treat log paths as security-sensitive destinations: validate symlink,
  ownership, and permissions before invoking tmux, create new logs owner-only,
  persist cursors with write-then-rename, and make write failures visible once
  without turning an otherwise usable attach into a crash (TASK-030).
- A raw reattach must honor the newly requested log path even when the pane is
  already piped elsewhere: replace the pipe deliberately, and only backfill when
  no pipe is active. A capture cursor advances only by bytes successfully
  written and must be invalidated when the session or current log size no longer
  matches its sidecar metadata; validate both the sidecar and its temporary path
  on every write (TASK-059 review).
- `remain-on-exit on` keeps the pane and its exit status after the command
  exits. The relay polls `pane_dead` / `pane_dead_time` / `pane_dead_status`
  during attach and auto-detaches when the pane dies during the attach, exiting
  with `pane_dead_status`; attaching to an already-dead session is the
  postmortem path and does not auto-detach. (Before TODO-016 the relay left the
  user attached to the dead pane and read the status only after a manual
  detach.)
- The postmortem/auto-detach split turns on one anchor: `pane_dead` is true both
  for a pane dead before attach and one that dies during attach, so the relay
  records the attach-start time and compares the pane-death time against it —
  death before attach is postmortem, death after is auto-detach. Manual and
  signal detach reuse this same attach-time status rule; do not fork the status
  logic per detach path (TASK-022 R001).
- When formatting a past `pane_dead_time` as a local timestamp, compute the UTC
  offset AT that timestamp (`UtcOffset::local_offset_at`), not the current
  offset — DST means the offset then can differ from now, and the current offset
  mislabels the time. Fall back to UTC and document it when the local offset
  cannot be determined, and cover a DST-transition boundary in the test
  (TASK-023 R001).

## The PTY relay (highest-risk code)

- Give tmux a real controlling terminal unconditionally. Use `forkpty` (which
  does the `setsid`/`TIOCSCTTY` setup) so tmux attaches to a genuine PTY even
  when stay's own stdin/stdout are redirected or piped. This non-TTY path is
  heavily used and has a dedicated test; do not regress it.
- Restore the terminal on every exit path: normal return, signal, and panic.
  Termios is restored from a `Drop` guard and from a `std::panic::set_hook`
  hook, because a release build with `panic = "abort"` never runs `Drop` on
  panic. Keep both.
- SIGTERM must detach gracefully, with a hard fallback. On SIGTERM, run the same
  `detach-client` the detach key runs; if that fails, SIGTERM-then-SIGKILL the
  attach child and fall through to normal cleanup. SIGPIPE is ignored for the
  relay lifetime so a write to an already-exited attach child does not kill the
  relay. This exact fallback was a TASK-009 review finding — do not simplify it
  away.
- Every detach trigger, including a configured detach-key failure while the
  requesting client cannot be resolved, must use the same stop-and-reap path
  before returning an error. A helper that is correct for signal and pane-death
  paths is not enough if manual input bypasses it; test that the attach child is
  already reaped and that no detach command was issued (TASK-051 R001).
- Do not write a large input paste synchronously to a blocking attach PTY. Keep
  a bounded pending-input buffer, poll for writable capacity while continuing to
  drain child output, and stop reading stdin while the bound is reached;
  otherwise the relay can deadlock against a busy tmux pane (TASK-061 review).
- Check the attach-PTY HUP/error state before reading stdin, and treat `EIO`/
  `EPIPE` from a closed PTY as a normal shutdown, not an error. This was the
  TASK-009 R001 fix.
- WINCH is polled, not caught. The loop re-reads the terminal size on a short
  poll timeout rather than installing a SIGWINCH handler. That is an intentional
  trade-off; if you touch it, keep resize latency bounded and leave a comment.

## CLI and config

- Do not ship a flag that silently does nothing. If a flag is parsed but its
  behavior belongs to a later milestone, make it fail with an explicit "not yet
  implemented" message (as `--prompt-integration` does) rather than being
  accepted and ignored. A silently inert `-r/--read-only` is worse than an
  honest error, because the user believes they are safe. Wire the guard when you
  expose the flag; wire the behavior when its milestone lands.
- When independent CLI flags compose onto one underlying command, test every
  combination plus the no-flag byte-identical baseline, and verify the flags are
  threaded through every entry point (including picker actions) rather than only
  parsed at the top level (TASK-029).
- When a flag's polarity flips (TASK-027 renamed `-s/--ansi-stripped`, which
  opted into clean output, to `--raw`, which opts into ANSI capture — the
  opposite default), grep the whole design doc for every mechanism paragraph
  tied to the old flag, not just the flag name itself. TASK-027 R001 found that
  the doc's logging section still described the *old* default as the continuous
  ANSI `pipe-pane` stream and the opt-in mode as clean incremental
  `capture-pane` — exactly backwards for the new default/opt-in split. A rename
  that only updates the flag's spelling but leaves the surrounding "here's what
  each mode actually does" prose in its old arrangement produces a document that
  is internally self-contradictory, not just stale.
- clap "errors" for `--help`/`--version` are successful exits. Map
  `ErrorKind::DisplayHelp` and `DisplayVersion` to a zero exit code; everything
  else from the parser is a real failure. This was TASK-007 R002.
- Config precedence is environment over file over built-in default, per key.
  Keep it explicit and tested; the collision check between the two configured
  keys must stay.
- Reject empty session names at parse time. clap runs the name through
  `parse_session_name` as the `session_name` value parser, so an empty name
  fails as `SessionNameError::Empty` ("invalid session name: must not be empty")
  during `Cli` parsing and never reaches tmux or the picker. Keep name
  validation in `src/session_name.rs` as the single source and route every name
  that enters the system (parse, picker rename) through it; do not re-add a path
  that accepts an empty name. This was resolved in TASK-010.
- Preserve the precedence of established validation diagnostics when adding a
  new constraint. Run the existing disallowed-character checks before a new
  length check, and test an over-limit name containing a disallowed character so
  the original character and position error remains visible (TASK-045 R002).
- The built-in tmux settings stay cosmetic-only — the handful applied in
  `apply_builtin_tmux_settings`. Never add a tmux key binding to them: it would
  collide with stay's own single-key UX or with the user's bindings. An `r`
  binding and its test assertion were removed for exactly this (TASK-021 R002).
- Shared user-visible behavior should have one implementation used by every
  entry point. For example, the terminated-session recreate notice is emitted by
  the shared session function used by both the CLI and picker, and a missing
  exit status is rendered as the documented default rather than handled
  separately in each caller (TASK-031).
- Treat public API and invariant comments as part of the contract: keep their
  grammar complete and state the exact baseline behavior, especially when
  describing argv compatibility or flag composition (TASK-029).
- A shell snippet advertised for zsh must account for zsh-specific prompt
  expansion: command substitutions in `PS1` require `setopt PROMPT_SUBST`.
  Document the required option and test both the default literal behavior and
  the enabled expansion against a real zsh (TASK-034).
- Shell setup helpers must preserve existing user names. For an optional alias,
  check supported rc files and the executable search path for an exact,
  case-sensitive conflict, warn and omit the alias when found, and always leave
  the primary integration output available. Inject paths in tests rather than
  mutating the real `HOME` or `PATH` (TASK-038).

## The picker

- Render and pad by terminal display width, not character or byte count. A wide
  name (CJK, emoji) occupies two columns, so padding rows by `char::len` or byte
  length misaligns them. Use `unicode-width`'s display width for both truncation
  and padding. This was TASK-014 R002.
- Keep the last known list when a poll fails; do not blank the screen. The
  picker re-reads sessions on a short poll, and a transient error should leave
  the previous list visible. A test that exercises this must first wait for the
  initial row to render before enabling the failure marker — otherwise it
  asserts against an empty list and passes for the wrong reason. This was
  TASK-014 R004.
- Capture the target name when the confirm is triggered, and act on that exact
  value. The kill path captures the session name up front so a list poll
  mid-confirm cannot retarget the kill toward a different session (TASK-015).
- The selector defaults to the safe option: destructive confirms (kill) focus
  `No`, non-destructive focus `Yes`, and any key outside the selector's accepted
  set cancels to the No-equivalent — matching the old "any non-`y` key cancels"
  behavior (TASK-019).
- When two surfaces render the same data (the plain `stay list` and the picker),
  share the formatting through one helper that returns structured segments, not
  a pre-styled string. Each surface maps the segments to its own styling (ANSI
  for the list, ratatui `Style` for the picker), so the text cannot drift
  between them and the "what is emphasized" decision lives in one place. This is
  the `status_detail()` design (TASK-024 R001).
- A synthetic list row that is not a real record (the "create new session" row)
  stays render-only: special-case it in the renderer and never let it enter the
  `SessionRecord` inventory or `list_sessions` output, or it pollutes the plain
  listing and the data model. Reuse the existing input path (the name prompt)
  for both `Enter` on the row and the `c` shortcut rather than duplicating
  create logic (TASK-026 R001).
- For a content-sized, centered box the height depends on how many lines the
  status wraps to, and wrapping depends on the width — so compute the capped
  width first (terminal width), then the wrapped-line count, then the height.
  Sizing the height from the unconstrained content width gives the wrong box
  when the terminal caps the width. Degrade to filling the frame when it is
  smaller than the content (TASK-025 R001).
- Treat every selectable picker entry, including a synthetic "create new
  session" row, as one logical list when calculating selection and scrolling.
  Keep the viewport offset in state, clamp it after polling, and ensure the
  selected logical row is visible after every movement. Render overflow markers
  in a reserved, non-selectable gutter so they cannot overwrite row text or
  reverse-video styling. Cover top, middle, and bottom positions with
  deterministic small-frame render tests (TASK-040).
- Keep transient action context in the existing row detail rather than in a
  separate prompt or bottom status message. For terminated-session recreate,
  share structured notice data between the session operation and picker, keep
  the interactive picker path silent on stderr, and preserve the current session
  state in the same bracketed detail (for example,
  `[detached - terminated with exit code 0 before recreate]`). The
  non-interactive CLI may retain its stderr notice semantics (TASK-041 R001).
- Width-aware rendering must preserve semantic tokens before applying generic
  truncation. If a full row detail does not fit, use a compact representation
  that keeps important multi-word values such as `exit code N` and `recreate`
  intact; add a narrow-width test that asserts the complete tokens rather than
  only the total row width (TASK-041 R002).
- Pending picker modifiers are row-local state, not global prompt state. Render
  them only beside the selected record's existing status; keep unselected and
  synthetic create rows clean, leave the bottom controls stable, include the
  detail in width calculation, and test the exact labels at narrow widths
  (TASK-049 review).
- A successful picker-selected attach must return to a fresh picker round after
  either the configured detach key or pane termination. Drop and recreate the
  terminal guard, re-poll the inventory, reset transient picker state, and keep
  explicit non-picker attach behavior unchanged. Exercise both alternate-screen
  and forced-main-screen paths through a real PTY (TASK-048 review).
- An attach failure is an in-picker action error, not a reason to abandon the
  picker: show the error, refresh the inventory, and continue. Signal handlers
  for the picker must request normal loop shutdown so the existing terminal
  guard restores raw mode, the alternate screen, and the cursor. Keep the
  shortcut panel synchronized with every implemented key (TASK-060 review).
- Every destructive picker action needs the same safe default and confirmation
  semantics across all state variants. A live-session recreate is as dangerous
  as killing a session and must not bypass the `No`-focused confirmation path;
  capture the target when confirmation begins so a poll cannot retarget the
  action (project review finding G9).
- Picker keyboard changes need both state-machine coverage and a real PTY
  integration path: exercise standard `CSI 5~`/`CSI 6~` PageUp/PageDown input,
  Home/End selection and clamping, and direct `y`/`n` answers in every
  confirmation mode. This catches terminal decoding and rendered interaction
  regressions that unit tests alone can miss (TASK-066 review R002).

## Testing patterns

- Isolate every test server and tear it down. Each test that touches tmux uses a
  unique `stay-test-<unique>` namespace and a `Drop` guard that runs
  `kill-server`. A hard-killed test binary can still leak an orphaned server;
  keep teardown on the `Drop` path so normal panics are covered, and be aware a
  SIGKILLed run can still leave orphans behind.
- Exercise the relay through a real PTY, via `script(1)`. Do not claim attach
  coverage from a pipe or `/dev/null`; the PTY behavior is the point. The
  attachment suite launches the actual binary with a `tmux` shim that remaps the
  production `-L stay` socket onto the test namespace.
- Do not generate executable fixtures and immediately exec them. Writing a file
  and running it in the same test races the loader and fails intermittently with
  "Text file busy" (os error 26). Prefer `/bin/sh -c '<script>'` test commands
  over freshly-created executable files. This flakiness cost real time in
  TASK-005.
- Serialize tests that touch process-global state. The relay uses a global
  `TERMINATE_REQUESTED` atomic and the process-global panic/signal hooks; unit
  tests that mutate these can race each other under the default parallel test
  runner. The integration suite already guards PTY tests with a shared mutex —
  do the same for any unit test that installs a signal/panic handler or sets the
  terminate flag, or drive the behavior through a parameter instead of a global.
  Every test guarding the same global must hold the same shared mutex — a
  function-local `static` declared per test is a distinct lock and serializes
  nothing; TASK-012 R002 caught two tests each guarding `SHELL` with its own
  lock.
- On macOS, tmux may live in `/usr/local/bin` or `/opt/homebrew/bin` and not be
  on the test process's `PATH`. The Mac command wrapper exports those; if a
  real-tmux test fails only on the Mac with a "not found" shape, check `PATH`
  before suspecting logic.
- For real tmux socket discovery, follow tmux's socket-root rule: use
  `TMUX_TMPDIR` when it is non-empty, otherwise `/tmp`; do not substitute
  macOS's unrelated `TMPDIR` without verifying tmux uses it. Keep a real macOS
  sweep test in the exact `mac-qcheck` path (TASK-035).
- When a task removes or rewrites a code path, assert the new behavior, not the
  old one. TASK-012 R001 deleted the `<shell> -c <shell>` nested invocation but
  the test still expected the wrapper to record `-c` and a second shell —
  validating exactly the path the task removed. After a behavior change, grep
  the assertions for the old shape.
- When a test forks the test binary to run a filtered sub-suite (picker and
  relay helpers do this), `exec` a fresh process instead of continuing in the
  forked libtest process. libtest's process-global locks are inherited across
  `fork`, so the child can deadlock on a mutex the parent already holds and the
  default-parallel run fails; re-exec the binary with the test filter. This was
  TASK-018 R001.
- A newly added direct dependency must be recorded in `Cargo.lock` before the
  locked gates run. TASK-014 R003 added `unicode-width` to `Cargo.toml` while
  the lockfile lagged, so locked or offline verification failed to resolve it.
  After adding a dependency, let the build update the lock and commit it.
- Integration tests that create real Unix sockets or tmux servers must serialize
  tests sharing the same socket namespace and clean up fixtures on both success
  and failure paths. A fake unresponsive socket is useful for exercising
  bounded-failure handling, but it must not race a live-server sweep test
  (TASK-035).
- Remove test tmux servers before removing their socket root: deleting the
  socket directory does not terminate the server or its panes. Cleanup should
  enumerate only owned test namespaces, issue bounded `kill-server` commands,
  then remove the directory. Use a longer polling ceiling for real-tmux waits on
  shared runners, and isolate a named test before changing its timeout when a
  full run flakes (TASK-063 and TASK-FIXUPS reviews).
- A large-history capture test must establish its `history-limit` before the
  producer starts flooding the pane, or deliberately wait before the flood;
  raising the limit after output begins can evict the evidence the test needs.
  If a timing failure appears, run the compiled test directly and separate
  cargo-build contention from a real tmux race before changing the fixture
  (TASK-054 review).
- For persistent logging, test repeated attach/detach/reattach cycles against a
  live producer and assert that the file never shrinks and retains bytes
  captured during earlier cycles; a first-attach test cannot catch destructive
  reattachment behavior (TASK-030).
- When picker controls compose independent tmux client flags, unit-test all
  combinations but also drive the live picker and inspect tmux's rendered client
  state. Replace stale managed status settings before attachment, while
  preserving a user's explicit tmux status configuration. Exercise both the
  automatic alternate-screen path and the forced-main-screen path, and verify
  that the no-selection/create row keeps pending modifiers empty and shows no
  modifier labels (TASK-042/TASK-043 and TASK-050 reviews).
- Pass-through is a distinct workflow, not a special attach mode: validate the
  target, send bounded chunks through a dedicated tmux buffer, delete each
  buffer after pasting, and assert that `attach-session` is never invoked
  (TASK-032).
- Treat task artifacts referenced by a review as part of the task's tracked
  deliverable. Add screenshots and other baselines before review so a clean
  worktree and the review's artifact checks agree; do not leave an artifact as
  an untracked file and ask the reviewer to decide whether it is in scope
  (TASK-040 R002).
- Prefer a parameterized, pure check over mutating the process-global
  environment in a test. The nested-tmux guard takes `tmux: Option<&OsStr>` so
  tests pass the value directly and never touch the real `$TMUX` (TASK-020); the
  same shape avoids the `SHELL`-mutation races above. The same applies to
  `$HOME`-resolved paths: production resolves the tmux config path (classic vs
  XDG) from `$HOME`, but the create seam takes the config path as a parameter,
  so tests inject a temp path or `None` and never depend on the runner's real
  home (TASK-021 R001).

## Process discipline

- A user-authorized variation is a real scope change, not an implementation
  defect, but the governing task must be updated to the complete intended
  behavior before review. Keep the authorization and resulting acceptance
  criteria auditable in the task, and test every existing mode affected by a
  shared helper or decoder (TASK-052 R001/R002).

- Do not expand task scope mid-task by rewriting the governing process docs.
  TASK-012 R003 tried to retrofit a new commit-attribution contract by editing
  `agent_workflow.md`, `docs/roles.md`, and `lessons_learned.md` inside the task
  diff — files outside its pre-implementation scope. If a governing rule needs
  to change, open a separate `NEW` plan task for it; the rule cannot be
  rewritten to fit work already underway.
