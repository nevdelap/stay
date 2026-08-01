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

## TASK-FIXUPS - close TASK-063 acceptance gaps

State: COMPLETED

Goal:

- Close three deviations left by TASK-063 (`f418358`) that its review did not
  catch, so the suite honours the acceptance criteria it was marked `COMPLETED`
  against.

Dependencies:

- None. TASK-063 is already `COMPLETED`; this corrects it in a fresh commit
  rather than amending an archived task.

Scope:

- `tests/tmux_sweep.rs`: the sweep test killed its out-of-prefix control
  namespace with `kill-server` but never unlinked the socket file the scope of
  TASK-063 required. Unlink it via the test's own `socket_path` helper after
  `kill-server`, so the control socket no longer depends solely on the per-run
  `TMUX_TMPDIR` removal.
- `tests/tmux_sweep.rs`: `temporary_directory_is_removed_during_unwinding` only
  dropped the guard on a normal block-scope exit, so it never exercised the
  unwinding path its name and TASK-063's acceptance criterion promise. Force a
  panic inside the guard's scope with `catch_unwind` and assert the path was
  removed while the stack unwound.
- `tests/cli_surface.rs`, `tests/attachment.rs`: TASK-063 raised only the two
  CI-named real-tmux waits from a fixed five-second window to ten seconds and
  left seven structurally identical `0..250` (20 ms) waits at five seconds - the
  same loaded-runner flake class F25 targeted. Raise all seven to `0..500`. This
  only extends each ceiling; every loop still breaks on success.
- Deliberately deferred (not in this fixup, flagged for a separate decision):
  guarding the `<log>.offset` sidecars and the ~40 remaining
  `let _ = fs::remove_*` sites with `TempPath`, and rewording the "the same
  helper sets `TMUX_TMPDIR`" scope note (the helper forwards it; the per-run dir
  is created at the tmux seam). None changes behaviour or a stated acceptance
  criterion.

Acceptance criteria:

- The sweep test unlinks its control socket after `kill-server`.
- `temporary_directory_is_removed_during_unwinding` panics inside the guarded
  scope and asserts removal during unwinding.
- No `0..250` fixed real-tmux wait remains in `tests/`.
- `cargo build`, `cargo clippy --all-targets`, and the affected tests pass;
  `just qcheck` and `just mac-qcheck` both pass.

## TASK-065 - scope quality tooling to changed files

State: COMPLETED

Goal:

- Make the normal format and lint gates inspect only files changed by the
  current commit, while providing an explicit all-files mode for occasional
  formatter or linter upgrades. Keep the Justfile as a small, readable entry
  point rather than duplicating every language's path-selection rules there.

Dependencies:

- None.

Scope:

- Add one repository-owned quality dispatcher under `scripts/` that owns the
  changed-file/all-file scope, Git path selection, language classification, and
  tool invocation matrix. Do not scatter a second copy of the file globs and
  exclusions across Just recipes.
- Define changed-file selection precisely: when staged changes exist, use the
  staged diff against `HEAD`; in a clean commit or CI checkout, use the current
  commit's parent diff. Include the destination of added, copied, modified, and
  renamed paths; ignore deleted paths because there is no file to format or
  lint. The all-files scope uses tracked files, still excluding generated output
  and other explicitly unsupported paths.
- Make `format`, `lint`, and `check` use the changed-file scope by default, and
  add clearly named `format-all`, `lint-all`, and `check-all` entry points for
  deliberate repository-wide maintenance. Keep the quiet wrappers aligned with
  those scopes (`qformat`, `qlint`, `qcheck` and their all-files forms), without
  duplicating the tool lists in each recipe.
- Route every file-oriented formatter and linter through the selected path list:
  shfmt/shellcheck, Dockerfile formatting/linting, jq, mdformat/markdownlint,
  pyupgrade/Ruff/ty/Bandit, rustfmt, Taplo, yamlfmt/yamllint, actionlint, and
  the no-stray-debugging check. Empty per-tool file sets are successful no-ops.
  Preserve the existing Docker/UV/tool versions and security exclusions.
- Document and test the two non-file-granular checks rather than pretending they
  accept paths: commit-message formatting/linting always checks the current
  commit message, and Cargo/Clippy must continue compiling all affected targets
  while changed-file mode reports or fails only diagnostics whose source span is
  in the selected changed Rust files. The explicit all-files mode retains the
  existing whole-project warning policy.
- Preserve the existing final cleanliness check: a formatter run may update
  selected files, and the gate must make that change visible for staging rather
  than silently hiding it. Keep the current `justfile` recipes tidy enough that
  adding or changing a tool requires editing one dispatcher table and one small
  recipe list at most.
- Add dispatcher unit tests for staged versus commit-diff selection, renames,
  deletions, all-files mode, language classification, empty selections, and
  changed-file diagnostic filtering. Add an integration fixture proving an
  unchanged file's formatting/lint violation is ignored in default mode but is
  found by the corresponding all-files command.

Acceptance criteria:

- With a staged commit containing one Rust, one Markdown, and one shell file,
  the default format/lint commands invoke only those matching files and do not
  touch or report unrelated tracked files.
- A deleted path is never passed to a formatter or linter, and a rename uses the
  new path exactly once.
- `format-all` and `lint-all` exercise every eligible tracked file, including
  unchanged files, so a tool-version upgrade can intentionally refresh the
  repository; `check-all` composes those all-files gates with the existing tests
  and MSRV checks.
- The changed-file and all-files quiet recipes have one shared tool matrix, no
  duplicated language globs in the Justfile, and preserve current Docker, UV,
  Cargo, and commit-message checks.
- Unit and integration tests cover path selection, empty selections,
  formatter/linter dispatch, and the Rust diagnostic-scope behavior.
- `just qcheck` and `just mac-qcheck` pass, and the all-files quality command
  passes from a clean checkout.

## TASK-059 - make -l logging honest about its target and its cursor

State: COMPLETED

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

State: COMPLETED

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

State: COMPLETED

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

State: COMPLETED

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

State: COMPLETED

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

State: COMPLETED

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
