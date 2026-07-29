# Lessons Learned

This document is durable, in-tree guidance for the implementer and reviewer
agents (Igor and Rufus) working on `stay`. It distills mistakes actually made
during the build so far — preserved in the task commits in git history — plus
findings from the whole-application review, so the remaining work (polish,
terminated-session UX, the attach-mode and logging flags) does not repeat them.

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
- Read `check.log` on failure. The quiet recipes write full output there; do not
  re-run the verbose recipe to see what happened.
- Do not conflate "the file differs from the last commit" with "the formatter
  has more to do." Checking mdformat idempotency with `git diff --exit-code`
  reports dirty on a file you have just rewritten, because it differs from HEAD,
  not because mdformat changed it. Verify idempotency by running the formatter
  twice and comparing a checksum (`md5sum`) across the two passes — stable means
  the file is already in canonical form. `git diff` answers a different question
  than "is this file formatter-clean."

## Commit attribution

- A previous commit qualified the model name with the reviewer role, making a
  role label look like a model variant. Commit trailers must contain the actual
  model name, version, and variant; tool, provider, role, and agent names do not
  count as model attribution.
- Add exactly one `Co-Authored-By:` trailer per distinct model that performed
  work. If both roles use the same model, one trailer is valid and a duplicate
  trailer for that model is invalid.
- The model identity in trailers and doc examples must be the real name with
  version and variant, never an invented label. `Co-Authored-By: GPT-5 Standard`
  and the example `GPT-5.6 Standard` were both rejected because `Standard` is
  not a real model identifier; `gpt-5.6-luna` was. Do not fabricate a label to
  fill a template — look up the actual identity. This was TASK-013 R001.

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
  fields and made `parse_session_row` reject the whole row as malformed. Session
  names can be constrained to exclude the delimiter; arbitrary pane state
  (paths, foreground command names) cannot be, so it must never share a
  delimited row with the fixed fields at all. The fix: query the stable,
  delimiter-safe fields (session name, attachment, timestamps, `pane_id`) in the
  one batched `list-panes -F` call, then fetch each dynamic value separately per
  pane via `display-message -p -t <pane-id> <format>`. This was TASK-028 R002;
  cover a colon-containing working directory in the test the same way the R002
  fix did, so this class of collision cannot silently regress.
- "No server for this socket" means an empty inventory, not an error. Killing
  the last session lets the tmux server exit; listing and kill paths must treat
  a missing server identically to zero sessions.
- Set global options through a throwaway bootstrap session. `set-option -g`
  needs a running server, and options like `history-limit` are read when a
  session is created — so create a short-lived bootstrap session first, set the
  globals, then create the real session, then drop the bootstrap (guarded by
  `Drop` so it is cleaned up even on error). Do not assume options apply
  retroactively.
- Verify tmux feature and hook assumptions against the actual version's
  documented behavior before designing around them. In particular, do not invent
  a termination hook: check `show-hooks`/the shipped documentation and record
  any unattended-event gap explicitly when tmux offers no such hook (TASK-030).
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
- For persistent logging, test repeated attach/detach/reattach cycles against a
  live producer and assert that the file never shrinks and retains bytes
  captured during earlier cycles; a first-attach test cannot catch destructive
  reattachment behavior (TASK-030).
- Pass-through is a distinct workflow, not a special attach mode: validate the
  target, send bounded chunks through a dedicated tmux buffer, delete each
  buffer after pasting, and assert that `attach-session` is never invoked
  (TASK-032).
- Prefer a parameterized, pure check over mutating the process-global
  environment in a test. The nested-tmux guard takes `tmux: Option<&OsStr>` so
  tests pass the value directly and never touch the real `$TMUX` (TASK-020); the
  same shape avoids the `SHELL`-mutation races above. The same applies to
  `$HOME`-resolved paths: production resolves the tmux config path (classic vs
  XDG) from `$HOME`, but the create seam takes the config path as a parameter,
  so tests inject a temp path or `None` and never depend on the runner's real
  home (TASK-021 R001).

## Process discipline

- One commit per task; both roles amend it. The implementer owns the
  `Implemented:` section, the reviewer owns the `Reviewed:` section, and each
  preserves the other's exactly. Do not create follow-up review commits or
  squash task commits mid-task.

- Keep the review-doc format uniform. Use the `## Findings` → `### RNNN`
  (`Status: OPEN`/`ADDRESSED`) → `## Final decision` structure from
  `design_docs/agent_workflow.md`. Early docs drifted from this; new docs should
  not.

- Do not expand task scope mid-task by rewriting the governing process docs.
  TASK-012 R003 tried to retrofit a new commit-attribution contract by editing
  `agent_workflow.md`, `docs/roles.md`, and `lessons_learned.md` inside the task
  diff — files outside its pre-implementation scope. If a governing rule needs
  to change, open a separate `NEW` plan task for it; the rule cannot be
  rewritten to fit work already underway.

- The reviewer changes no source or tests. Findings go in the review doc and the
  commit's `Reviewed:` section; the implementer makes the code changes.
