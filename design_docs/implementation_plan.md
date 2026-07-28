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

## TASK-029 - attach-mode flags -r/-L, composable; wire picker v/l keys

State: COMPLETED

Goal:

- Implement TODO-002: `stay attach <name> -r/--read-only`, `-L/--low-priority`,
  and both together, mapped onto tmux's `attach-session -f <flags>`
  independently (`-f read-only` / `-f ignore-size` / `-f read-only,ignore-size`)
  rather than tmux's bundled `-r` shorthand (which always bundles
  `read-only,ignore-size` — confirmed against `man tmux`). Composability is the
  point: a low-priority read-write client and a full-priority read-only client
  are both real, distinct use cases.
- Wire the picker's existing `v` (view-only) and `l` (low-priority) keys to
  actually attach with these modifiers, replacing the current placeholder
  `action_error` ("v: not yet implemented" / "l: not yet implemented" in
  `src/picker/mod.rs`).

Dependencies:

- None. `stay attach <name> [-r/--read-only] [-L/--low-priority] ...` already
  parses; both flags currently route to the shared "not yet implemented" guard.
  This task replaces that guard's entries for these two flags with real
  behavior.

Scope:

- `src/tmux.rs`: extend `attach_program_and_arguments` to accept the two
  modifiers and append tmux's `-f <flags>` argument to the attach-session argv
  accordingly. Omit `-f` entirely when neither modifier is set, so today's
  plain-attach argv is byte-identical to what it is now (test this explicitly —
  it's the regression that would silently break every existing attach if gotten
  wrong).
- `src/relay.rs`: thread the modifiers through `attach`/`attach_with_input` down
  to `attach_program_and_arguments`. This is the actual production attach path
  (the `forkpty`/`execvp` child); as of this task's drafting,
  `Tmux::attach_command` in `src/tmux.rs` has no callers outside its own unit
  test (verified by grepping both `src/` and `tests/`), so it is not the thing
  to extend — but re-verify this with a fresh grep at implementation time rather
  than trusting this snapshot, since any merge landing between drafting and
  implementation could add a caller and invalidate the claim.
- `src/session.rs`: thread the same modifiers through
  `attach_session`/`attach_session_with_input`.
- The `attach` subcommand dispatch in `src/main.rs`: pass the CLI's
  `read_only`/`low_priority` fields into `session::attach_session`; remove them
  from the "not yet implemented" flag list
  (`reject_unimplemented_attach_options`).
- `src/picker/mod.rs`: give `PickerOutcome::Attach` two new fields
  (`read_only: bool`, `low_priority: bool`, both `false` for the plain-Enter
  path) and thread them into the `session::attach_session_with_input` call
  inside `picker::run`. Replace the `v`/`l` key handlers' inert `action_error`
  with the same residual-input-draining flow Enter already uses
  (`input.drain_available()`), setting `read_only: true` for `v` and
  `low_priority: true` for `l`.
- Tests: `tmux.rs` argv assertions for no-flags / read-only / low-priority /
  both; an attachment-level test (via the existing `script(1)`-based harness in
  `tests/attachment.rs`) proving a read-only attach's keystrokes don't reach the
  pane — `man tmux` confirms a read-only client only responds to keys bound to
  `detach-client`/`switch-client`; picker unit tests asserting `v`/`l` now
  produce the right `PickerOutcome::Attach` variant instead of an
  `action_error`.
- `design_docs/stay.html`: strike/update the TODO-002 index entry and body per
  the doc's existing "struck-through items are already implemented" convention.

Acceptance criteria:

- `stay attach <name> -r`, `-L`, and both together produce the correct tmux `-f`
  argument; plain `stay attach <name>` still produces no `-f` at all.
- A read-only attach behaves read-only (only detach/switch-client keys have
  effect); a low-priority attach does not resize the session to the attaching
  client's terminal.
- Picker `v` attaches read-only; `l` attaches low-priority; plain Enter is
  unaffected.
- `-r`/`-L` still require an existing session name and reject trailing command
  words (already enforced by cli.rs's existing `validate()`; carry the behavior
  over, don't re-derive it).
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-030 - logging: -l/--log, -t/--truncate, --raw

State: COMPLETED

Goal:

- Implement TODO-006:
  `stay attach <name> -l/--log <file> [-t/--truncate] [--raw]`.
- Default (no `--raw`): clean text, sourced from tmux's normal `capture-pane`
  output (no ANSI). Since tmux has no continuous clean-text pipe, this is
  captured incrementally at three boundaries — attach-open, detach, and session
  terminate — plus periodically while a client is attached (default ~5s,
  configurable). Because there's no stay daemon, an unattended session between
  those boundaries gets no interim capture; this is a documented trade-off, not
  a bug. Default mode is a genuine append-only incremental transcript (not a
  lighter boundary-only variant), matching `--raw`'s completeness guarantees
  modulo that unattended-gap trade-off — this scope decision is locked in; do
  not descope it.
- `--raw`: a continuous ANSI-preserving `pipe-pane` stream. This keeps logging
  while the session is detached, with no relay process needed to drive it.
- `-t/--truncate`: changes default-mode semantics from append-only-increment to
  overwrite-with-full-dump on every capture (no cursor tracking needed;
  trivially bounded by `history-limit`).

Dependencies:

- Sequencing after TASK-029 (TODO-002) is recommended: both tasks add new
  parameters threaded through the same
  `session::attach_session_with_input`/`relay::attach_with_input` call chain,
  and landing them back-to-back avoids two people extending the same function
  signatures at once.

Scope:

- New module `src/logging.rs` (already named in stay.html's architecture
  diagram: "pipe-pane / capture-pane wiring for -l/-t/--raw"), owning:
  - Log-target security (kept per explicit prior decision, not dropped for
    simplicity): before handing a log path to `pipe-pane`/`capture-pane`,
    `lstat` (not following symlinks) any pre-existing file at the path and
    reject unless it is a regular file, owned by the current user, with no
    group/other permission bits. `umask 077` inside the shell command only
    controls a file tmux *creates*; it says nothing about a pre-existing target,
    so this check has to run in stay itself before handing tmux the path, not
    inside the shell command.
  - Path resolution/de-dup: a relative `-l` path resolves against the invoking
    client's cwd, not stay's own; repeated or aliased `-l` paths in one
    invocation (identical path, `..`-relative alias, a symlinked directory alias
    resolving to the same canonical file) de-duplicate to a single log open.
  - Write-failure visibility: a failing log write (disk full, removed media,
    quota) must surface as a one-time non-fatal warning, not be silently
    swallowed — neither `pipe-pane`'s shell pipe nor a `capture-pane >> file`
    redirect reports this back to stay automatically, so this needs a concrete
    mechanism (e.g. periodically checking the log file is still
    writable/growing, or wrapping the shell command so a failing write reports
    back via a sentinel) decided at implementation time.
  - Back-fill: adding `-l` to an already-running or already-terminated session
    must back-fill the log with everything currently retained (one
    `capture-pane -p -S - -E -` dump) before starting the ongoing capture/pipe,
    so the log reads as complete from session start, bounded by whatever
    `history-limit` had already evicted.
- Default clean mode (no `--raw`):
  - Relay-owned boundary captures: one-shot capture on attach-open and on detach
    (the relay is already the live process for detach/copy-mode key interception
    during an attach); one-shot capture on terminate via a `pane-exited` hook;
    periodic capture on a timer while a client is attached.
  - Incremental accounting (verified end-to-end before this task was written —
    reconstructing a first snapshot plus a delta-only capture and diffing
    against a fresh full capture was byte-identical): restrict every capture to
    the history range only, excluding the volatile visible screen entirely, via
    `-E -1` (not `-E -`) — tmux's history addressing is stable and append-only
    in a way the visible screen never is, since already-scrolled lines never
    change retroactively. Track `history_size` at the time of the last capture
    as the cursor (`last_captured_size`, sidecar `<file>.offset` or relay
    memory) and on each capture compute
    `delta = history_size_now - last_captured_size`, then run
    `capture-pane -p -S -<delta> -E -1 -t <name> >> <file>`. Content that is
    still only on the visible screen (not yet scrolled into history) is
    necessarily not captured until it scrolls off — an inherent latency in this
    design worth documenting plainly, not merely an implementation detail.
  - Eviction/gap detection, using the cursor above: if a fresh `history_size` is
    less than the stored `last_captured_size`, tmux has evicted history that
    this session had already accounted for (the retained window shrank from
    underneath the cursor) — this is the detectable eviction signal, not a
    `delta > history_limit` comparison, since eviction shows up as the total
    count going backwards, not forwards. On detecting it, write an explicit
    `--- history evicted before capture, N lines possibly lost ---` marker (N is
    best-effort: `last_captured_size - history_size_now`), capture the entire
    currently retained history (`-S - -E -1`) rather than a delta (the safe
    choice, since the previous overlap point is no longer determinable once the
    reference frame has shifted), and reset
    `last_captured_size = history_size_now`. Raise `history-limit` on
    `-l`-logged sessions (configurable) to make this rare in practice.
  - `-t/--truncate`: every capture overwrites the file with a full `-S - -E -`
    dump instead of appending an increment — no cursor tracking needed for this
    mode.
- `--raw` mode: open with
  `tmux pipe-pane -o -t <name> 'umask 077; cat >> <file>'`; the `-o` toggle
  avoids stacking a duplicate pipe if logging is requested again for the same
  session. No relay polling needed — the pipe runs server-side and keeps
  producing output while the session is detached.
- Tests:
  - Log-target security: a symlink at the target path is rejected; a
    pre-existing wrong-owner/world-readable file is rejected; a fresh path is
    accepted.
  - Default mode: attaching with `-l` produces a clean-text log matching
    `capture-pane` output (no ANSI bytes); two consecutive attach/detach cycles
    append without duplicating content across the boundary; `-t` overwrites
    instead of appending.
  - `--raw` mode: attaching with `-l --raw` produces a log containing ANSI
    escape sequences; logging keeps growing after detach with no client attached
    — this is the test that most concretely proves the "doesn't need the relay"
    claim.
  - Path de-dup: two aliased `-l` paths in one invocation open the log exactly
    once.
- `design_docs/stay.html`: strike/update the TODO-006 index entry and body once
  implemented, per the doc's existing convention.

Acceptance criteria:

- `stay attach <name> -l <file>` produces a clean-text log with no ANSI;
  boundary and periodic captures are incremental (no duplicated content across
  repeated captures).
- `stay attach <name> -l <file> --raw` produces an ANSI-preserving log via a
  continuous `pipe-pane` stream that keeps growing while detached.
- `-t/--truncate` overwrites the log with the latest full state on every capture
  instead of appending.
- A pre-existing symlink or wrong-owner/world-readable file at the log path is
  rejected with a clear error before any tmux logging command runs.
- Adding `-l` to an already-running or already-terminated session back-fills the
  log with everything currently retained before starting ongoing capture.
- A failing log write surfaces a one-time non-fatal warning rather than being
  silently swallowed.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-031 - print prior exit status before force-recreate

State: COMPLETED

Goal:

- Implement TODO-004: when `stay create <name> -f/--force-recreate` (or the
  picker's `r` recreate key) targets a session that is currently `terminated`,
  print that session's exit status to stderr before killing and recreating it —
  so a terminated session's exit code is never silently discarded, matching v1's
  documented behavior.

Dependencies:

- None. `SessionRecord::exit_code`/`status_word()` already exist; this task only
  adds a print step ahead of an existing call.

Scope:

- `src/session.rs`: in `force_recreate_session`, before calling `kill_session`,
  look up the target session in `tmux.list_sessions()`; if found and
  `status_word() == "terminated"`, write a line to stderr with its exit code
  (e.g. `session {name:?} terminated with exit code {code} before recreate`)
  before proceeding. If the session doesn't exist or isn't terminated, print
  nothing — this is purely an information-preservation step for the case that
  would otherwise discard it, not a UX change to the non-terminated recreate
  path.
- This needs one extra `list_sessions()` call (or equivalent single-session
  lookup) inside `force_recreate_session`, which today only calls `kill_session`
  then `create_session`. `main.rs`'s `Command::Create` arm and
  `picker::PickerState::recreate` both call `session::force_recreate_session`
  directly, so putting the print inside that shared function (rather than
  duplicating it at both call sites) is the one-place fix.
- Tests: force-recreating a terminated session prints its exit code to stderr
  before recreating; force-recreating a live (attached/detached) session prints
  nothing extra; force-recreating a nonexistent session (today's "kill errors,
  ignored, then create" path) prints nothing extra either.
- `design_docs/stay.html`: strike/update the TODO-004 body per the doc's
  existing convention.

Acceptance criteria:

- `stay create <name> -f` against a terminated session writes its previous exit
  code to stderr before the session is killed and recreated.
- The picker's `r` key does the same (it already calls the same
  `force_recreate_session` function, so this falls out of the shared-function
  placement rather than needing separate picker-side work).
- Force-recreating a live or nonexistent session is behaviorally unchanged — no
  new output.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-032 - pass-through (-p): incremental stdin forwarding, no attach

State: COMPLETED

Goal:

- Implement TODO-005: `stay attach <name> -p/--pass-through` forwards stay's own
  stdin into the named session *without* attaching — no PTY relay, no
  `attach-session` call at all. Forwarding must be incremental (as data
  arrives), not buffered-to-EOF, so
  `tail -f data | stay attach session -p`-style continuous piping delivers input
  as it's produced. A single "read everything then send once" implementation
  would silently break that streaming use case (nothing sent until the source
  closes), which is why this is called out as a hard requirement rather than an
  implementation detail.

Dependencies:

- None. `-p/--pass-through` is already parsed on `stay attach`, routed to "not
  yet implemented" in `main.rs::reject_unimplemented_attach_options`. This task
  supplies the real behavior and removes it from that list.

Scope:

- `src/tmux.rs`: add wrapper methods for
  `load-buffer -b <buffer-name> -t <name> -` (reading the chunk from stdin via
  the child's own stdin pipe) and `paste-buffer -b <buffer-name> -t <name> -d`
  (paste then delete the named buffer immediately). Use a stay-specific buffer
  name (e.g. `stay-passthrough`) so this can never collide with a buffer the
  user's own tmux usage might create.
- New function (in `src/session.rs`, alongside the other session operations —
  there is no pass-through-specific module today, and this is a small enough
  operation not to warrant a new one): read stdin in ~8KB chunks in a loop; for
  each chunk, run `load-buffer` (piping the chunk in via the tmux child's stdin)
  then `paste-buffer -d` against the target session; stop at stdin EOF. This is
  a bounded, short-lived operation per chunk — not a long-lived relay — so it
  can reuse the same short-command timeout/reaping path `Tmux::run` already
  provides (`COMMAND_TIMEOUT`), called once per chunk rather than needing its
  own unbounded child process.
- The `attach` subcommand dispatch in `src/main.rs`: when `pass_through` is set,
  route to this new function instead of `session::attach_session`, and validate
  the target session exists first (same existence check the plain attach path
  already does) rather than silently creating one.
- `src/cli.rs`: `-r/--read-only` already conflicts with `-p/--pass-through` in
  `validate()` — no change needed there. Confirm (and keep, as a test) that
  `-L/--low-priority` and log flags being combined with `-p` are rejected too,
  since none of those apply to a mode that never calls `attach-session` — `-p`
  should be validated as exclusive of every other `attach` modifier, not just
  `-r`. This closes a gap in today's validation matrix, which only checks
  `-r`/`-p` against each other.
- Tests: piping a bounded input (e.g. a multi-line string) via `-p` lands in the
  target session's pane exactly once, in order, without an attach; piping a
  live/streaming producer (e.g. a background process writing chunks with delays)
  shows each chunk arriving in the session incrementally rather than only after
  the producer closes — this is the test that actually proves the "not
  buffered-to-EOF" requirement, so it needs a real timing assertion, not just an
  end-state check; pass-through against a nonexistent session errors without
  creating one; pass-through combined with any other attach modifier (`-r`,
  `-L`, `-l`) is rejected at parse time.
- `design_docs/stay.html`: strike/update the TODO-005 body per the doc's
  existing convention.

Acceptance criteria:

- `stay attach <name> -p` delivers stdin into the session's pane via
  `load-buffer`/`paste-buffer -d`, without ever calling `attach-session`.
- A streaming/long-lived stdin producer's output appears in the session
  incrementally, not only at EOF.
- `-p` against a nonexistent session errors (matching `attach`'s existing
  strictness) rather than creating one.
- `-p` combined with any other `attach` modifier is rejected at parse time, not
  silently ignored.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-033 - confirm the tmux 3.2 version floor against CHANGES

State: COMPLETED

Goal:

- Implement TODO-008: verify the exact minimum tmux version each feature stay
  depends on actually needs — the `ignore-size` client flag, the
  `pane_dead_status`/`pane_dead_time` format variables, and `remain-on-exit` —
  against tmux's own release history, rather than asserting "3.2" without
  evidence. This was flagged as open in `src/tmux_version.rs`'s own comment and
  in TASK-001's review (R001), which required this exact confirmation and was
  marked addressed on the strength of a comment that turns out to be only
  partially precise.

Research already done (against tmux's real upstream `CHANGES` file):

- `ignore-size` (client flag): confirmed at CHANGES FROM 3.1c TO 3.2: "Change
  the existing client flags for control mode to apply for any client... This
  separates the read-only flag from 'ignore size' behaviour (new ignore-size)
  flag." This matches the existing `src/tmux_version.rs` comment exactly,
  including its "3.1c TO 3.2" citation — that part of the existing comment was
  already correct.
- `remain-on-exit`: the option itself is far older than the comment states —
  introduced as "Zombie windows... may be set for a window with the new
  'remain-on-exit' option" at CHANGES FROM 0.8 TO 0.9 (2009), not "0.8-era"
  loosely speaking but literally the 0.9 release. One later, non-blocking
  refinement exists: CHANGES FROM 2.6 TO 2.7 added "Show exit status and time in
  the remain-on-exit pane text" (this is likely what the existing comment's
  "2.8" guess was trying to point at, but the actual release is 2.7, not 2.8).
  None of this affects the binding 3.2 floor, since `ignore-size` (3.2) is
  already higher than either of these.
- `pane_dead_status`/`pane_dead_time` (the actual format variable names stay
  queries via `list-panes -F`): not found by name in tmux's `CHANGES` file at
  all — that file documents user-facing behavior changes in prose, not every
  individual format-variable name as it's added, so their introduction version
  isn't determinable from `CHANGES` alone. They are confirmed to exist in
  `man tmux` (installed 3.6a) alongside `pane_dead` and `pane_dead_signal`, but
  that only proves "present by 3.6a," not a precise introduction version. This
  is the one piece of TODO-008 that needs a different verification method than
  the other two — see Scope.

Dependencies:

- None.

Scope:

- `src/tmux_version.rs`: rewrite the top-of-file comment to record the
  corrected, evidenced versions: `remain-on-exit` from 0.9 (not "0.8-era"), the
  exit-status-and-time text refinement from 2.7 (not "2.8"), `ignore-size` from
  3.2 (unchanged, now with the exact quoted evidence), and an explicit note that
  `pane_dead_status`/`pane_dead_time`'s introduction version could not be pinned
  from `CHANGES` and was instead confirmed present via a targeted method (see
  next bullet) rather than left as an unverified assumption.
- For `pane_dead_status`/`pane_dead_time` specifically: `CHANGES` doesn't name
  individual format variables, so pin this a different way — either (a) locate
  the tmux commit that added these two names in the upstream git history
  (searchable by variable name, unlike the prose changelog) and record its
  tagged release, or (b) if that's judged not worth the effort for two variables
  already known to work fine at today's 3.2 floor, explicitly downgrade this
  from "a confirmed version" to "known-present at 3.2, the current floor, with
  no evidence of being newer" and say so plainly in the comment — a stated,
  honest uncertainty, not a silently reused guess. Either resolution is
  acceptable; leaving the comment's current unsupported "2.8" claim in place is
  not.
- `MINIMUM_TMUX_VERSION` itself: the research above does not change the value
  (3.2 remains the correct floor — it's still the highest of the three
  requirements), so this task is a documentation/evidence correction, not a
  behavior change. If the format-variable research in the bullet above turns up
  a version higher than 3.2, update the constant and its test coverage
  accordingly — that would be a real, not merely cosmetic, finding.
- Tests: no new test behavior is expected unless the constant changes; if it
  does, extend the existing `tmux_version.rs` parse/floor tests the same way
  past version-floor changes have.
- `design_docs/stay.html`: TODO-008's body currently asks to "verify against
  tmux's CHANGES file per feature rather than assuming" — update it to record
  the corrected findings (or strike it entirely if you consider the
  format-variable question closed by option (b) above) per the doc's existing
  convention.

Acceptance criteria:

- `src/tmux_version.rs`'s comment cites accurate, quoted-or-paraphrased evidence
  for `remain-on-exit` (0.9) and `ignore-size` (3.2), replacing the current
  imprecise "0.8-era" and unevidenced "2.8" claims.
- The `pane_dead_status`/`pane_dead_time` provenance is either pinned with real
  evidence or explicitly and honestly documented as
  unpinned-but-known-present-at-the-3.2-floor — not left as a silent,
  unsupported assumption.
- `MINIMUM_TMUX_VERSION` is 3.2 unless the research above surfaces a higher
  genuine requirement, in which case it's updated with matching evidence and
  tests.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-034 - prompt integration (--prompt-integration)

State: COMPLETED

Goal:

- Implement TODO-007: `stay --prompt-integration` currently prints "prompt
  integration is not yet implemented" and exits 0 (`src/main.rs`). Replace that
  with printing an actual shell snippet to stdout, which a user sources or evals
  from their shell rc file, so their prompt can reflect the current
  `STAY_SESSION_NAME` when running inside a stay-created session's pane.

Cleanroom note — this is not a port of v1's snippet:

- `stay.html`'s TODO-007 entry says to port "v1's `STAY_SESSION_NAME`- driven
  shell snippet, ported as-is," but the actual snippet text lived in v1's
  deleted Rust source, not in any surviving doc or test in this tree (checked
  `design_docs/stay.html`, `tests/`, and `lessons_learned.md` — only the
  description "ported as-is" survives, never the literal snippet content). Per
  the cleanroom rule in `design_docs/lessons_learned.md` (v1 source must not be
  recovered from disk or git history; a plan gap is a plan bug to fix by
  expanding the plan, not a cue to go looking for the code), do not try to
  reconstruct or guess the original snippet text. Scope the observable behavior
  (a snippet that surfaces `STAY_SESSION_NAME` in the prompt) and treat the
  exact snippet syntax as a fresh implementation-time decision, same as the
  picker's "fresh design, not a v1 mimic" treatment elsewhere in this plan.

Dependencies:

- None. `STAY_SESSION_NAME` is already exported into every stay-created
  session's pane environment today (`-e STAY_SESSION_NAME=<name>` at
  `new-session` time, `src/session.rs`) — this task only adds the
  snippet-printing behavior; it depends on nothing unimplemented.

Scope:

- `src/main.rs`: replace the "prompt integration is not yet implemented" stdout
  line in `dispatch` with a real snippet, printed to stdout (not stderr — this
  output is meant to be `eval`'d or redirected into an rc file, matching the
  convention tools like `starship init`/`direnv hook` use).
- New function, e.g. `prompt_integration::snippet()` (a small new module, or
  inline in `main.rs` if kept short) returning the snippet text as a
  `&'static str` or similar. Content is an implementation-time decision per the
  cleanroom note above; a reasonable shape (not prescribed here) is a small
  POSIX-sh-compatible function that checks whether `$STAY_SESSION_NAME` is set
  and non-empty and, if so, emits a bracketed segment for inclusion in `$PS1` —
  but the exact syntax, shell-compatibility scope (bash/zsh only, vs. POSIX sh
  too), and installation instructions printed alongside the snippet are all open
  for the implementer to decide and document, not fixed by this plan.
- `src/cli.rs`: no change expected — `--prompt-integration`'s existing
  exclusivity-with-everything-else validation already covers this flag correctly
  for a print-and-exit behavior.
- Tests: `--prompt-integration` prints non-empty output to stdout and exits 0
  (extending the existing `tests/cli_help.rs` coverage, which today only checks
  the placeholder message and the inside-tmux refusal); the snippet's content is
  valid shell syntax for at least one shell it targets (e.g. shellcheck it in CI
  the same way `scripts/*.sh` already are, or a smoke test that sources it under
  `/bin/sh`/`bash` without error).
- `design_docs/stay.html`: strike/update the TODO-007 body, replacing "ported
  as-is" with an accurate description of the freshly-authored snippet and its
  exact syntax once decided.

Acceptance criteria:

- `stay --prompt-integration` prints a real, non-empty shell snippet to stdout
  and exits 0 (no longer the placeholder message).
- The snippet is valid shell syntax for whatever shell(s) it targets, and its
  README/help text says which ones.
- Sourcing the snippet inside a stay session's pane causes the prompt (once the
  user also adds the snippet's output to their own `$PS1`/prompt-building step,
  per its printed instructions) to reflect `$STAY_SESSION_NAME`; outside a stay
  session it degrades to showing nothing, not an error or a literal
  empty-variable artifact.
- `--prompt-integration` remains exclusive of every other flag and subcommand
  (already enforced; add a regression test if one doesn't already cover this
  combination).
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-038 - shell-integration subcommand with --s-alias

State: COMPLETED

Goal:

- Add `stay shell-integration` as a new subcommand alongside
  `list`/`create`/`attach`/`kill`, printing the same prompt-segment snippet
  TASK-034's `--prompt-integration` flag prints today (reusing
  `prompt_integration::snippet()` — no change to that function's content).
- Add a new `--s-alias` flag on this subcommand:
  `stay shell-integration --s-alias` additionally prints `alias s=stay` so a
  user can source the output and get a short `s` alias for `stay`.
- Before emitting the alias line, check for a conflict: an existing `alias s=`
  definition in the user's shell rc files, or an existing `s` executable already
  on `$PATH`. If either is found, warn to stderr and omit the alias line from
  stdout — never silently shadow something the user already has named `s`. The
  prompt-segment part of the output is unaffected either way.

Why a subcommand instead of leaving this on the existing flag:

- TASK-034's `--prompt-integration` is a global boolean flag with
  print-and-exit, exclusive-of-everything-else semantics — that shape doesn't
  extend cleanly to also carry a new option like `--s-alias` without stacking
  more global, mutually-exclusive-with-everything flags onto `Cli` (the existing
  pattern already has two such flags, `--prompt-integration` and
  `--no-alt-screen`, each with its own hand-written exclusivity checks in
  `Cli::validate`). A subcommand gets clap's normal per-subcommand flag scoping
  for free — this task is what actually needs that scoping, since `--s-alias`
  only makes sense in the shell-integration context. `--prompt-integration`
  itself is untouched by this task; it keeps behaving exactly as TASK-034 built
  it. Whether to eventually deprecate the flag in favor of the subcommand is
  future work, explicitly out of scope here.

Dependencies:

- TASK-034 (`COMPLETED`) — this task reuses `prompt_integration::snippet()` and
  its module as built there.

Scope:

- `src/cli.rs`: add a new `Command::ShellIntegration { s_alias: bool }` variant
  (`#[arg(long = "s-alias")]`), following the existing per-subcommand flag
  pattern (e.g. `Command::Create`'s `force_recreate`). `--prompt-integration`
  and its existing validation are untouched.
- `src/main.rs`: dispatch `Command::ShellIntegration` to a new function (see
  next bullet) instead of the `cli.prompt_integration` branch's inline write —
  that inline branch and its tests stay exactly as TASK-034 left them; this is a
  sibling code path, not a replacement.
- New module `src/shell_integration.rs` (or extend `src/prompt_integration.rs`
  if that reads more naturally once written — implementer's call, since both are
  small), owning:
  - Print `prompt_integration::snippet()`'s existing content unchanged.
  - When `--s-alias` is set: check for a conflict before appending the alias
    line.
    - PATH check: search `$PATH` for an executable literally named `s` — the
      same shape as `src/session.rs`'s existing `resolve_command_path` (split
      `$PATH`, check each candidate), but simpler: existence only, no
      executable-bit/regular-file validation needed since this is a conflict
      probe, not something stay is about to exec.
    - Rc-file check: grep `~/.bashrc`, `~/.zshrc`, and `~/.profile` (matching
      the three files TASK-034's existing usage instructions already name — fish
      is deliberately out of scope, since the prompt snippet itself is POSIX
      `sh` and doesn't target fish anyway) for a line matching `alias s=`
      (tolerate leading whitespace and the common
      `alias s="..."`/`alias s='...'`/`alias s=...` quoting variants). Files
      that don't exist are skipped, not an error — a user need not have all
      three. The check is case-sensitive (`s` only; a differently-cased `S`
      alias or command is not treated as a conflict).
    - If either check finds something: write a warning to stderr in this shape:
      `warning: an 's' <alias in ~/.bashrc | command on PATH> already exists; skipping 'alias s=stay' — add it yourself if you want to override it`
      (naming which file or PATH was the source), and print only the
      prompt-segment snippet to stdout, without the alias line.
    - If neither is found: print the prompt-segment snippet followed by
      `alias s=stay` on stdout.
- Tests:
  - `stay shell-integration` (no flag) prints exactly what
    `--prompt-integration` prints today (same content, byte for byte) — a
    regression test tying the two together so they can't silently drift apart
    while both exist.
  - `stay shell-integration --s-alias` with no conflict present appends
    `alias s=stay` to stdout.
  - `stay shell-integration --s-alias` with an `s` executable present on a
    test-controlled `$PATH` warns to stderr and omits the alias line from stdout
    (parameterize the PATH probe the same way `build_command_tail`'s tests
    already avoid depending on the real environment, per the existing
    testing-patterns lesson about not depending on process-global state).
  - `stay shell-integration --s-alias` with a fixture rc file (a temp file
    standing in for `~/.bashrc`, injected as a parameter rather than reading the
    real `$HOME` — same pattern as the existing tmux-config- path injection in
    `src/session.rs`) containing an `alias s=` line warns to stderr and omits
    the alias line.
  - A conflict on either check (PATH or any one rc file) is sufficient to warn
    and omit — test at least one case of each, and one case where both are
    absent (the clean case).
  - A differently-cased existing alias/command (e.g. `S`) does not trigger the
    warning — the check is exact-match `s` only.
  - `stay shell-integration` without `--s-alias` never touches PATH or rc files
    at all and never warns — the conflict-check code path must not run
    unconditionally.
- `design_docs/stay.html`: document the new `stay shell-integration [--s-alias]`
  subcommand alongside the existing TODO-007 section (which documents
  `--prompt-integration` and should note the subcommand as the newer, additional
  interface, not a replacement) and the subcommand list near TODO-017's body.

Acceptance criteria:

- `stay shell-integration` prints the same prompt-segment snippet as
  `stay --prompt-integration`, byte for byte.
- `stay shell-integration --s-alias` additionally prints `alias s=stay` when no
  `s` alias or PATH command conflict exists.
- When an `s` alias (in `~/.bashrc`, `~/.zshrc`, or `~/.profile`) or an `s`
  executable on `$PATH` already exists, `--s-alias` warns to stderr, naming what
  was found and where, and omits the alias line from stdout — the prompt-segment
  output is still printed either way.
- The conflict check is case-sensitive; a differently-cased match does not
  trigger the warning.
- `--prompt-integration` (the existing global flag from TASK-034) is completely
  unaffected: same behavior, same tests, same exclusivity rules.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-035 - sweep orphaned per-test tmux servers

State: NEW

Goal:

- Implement TODO-010: each test's own `ServerGuard`/`Drop` teardown (present in
  every test file already — `tests/attachment.rs`, `tests/cli_surface.rs`,
  `tests/session_creation.rs`, `tests/tmux_inventory.rs`, plus the in-module
  test guards in `src/tmux.rs` and `src/session.rs`) only runs on a normal test
  exit or panic; a hard-killed test binary (CI timeout, SIGKILL) skips `Drop`
  entirely and leaves its per-test `tmux -L stay-test-*` server running as an
  orphan. Add a sweep that reaps genuinely orphaned live server processes left
  by prior abnormal runs, mirroring v1's PID-liveness-gated harness sweep.

Evidence gathered during planning (concrete, not hypothetical):

- The problem is real: this development machine's `/tmp/tmux-$UID/` currently
  holds thousands of socket files matching `stay-test-*`, dating back across the
  whole multi-day test history. Every one checked returns tmux's own "no server
  running on `<path>`" — i.e. these are dead sockets with no server behind them,
  not live orphans.
- Important nuance: tmux never deletes its own socket file, even on a clean,
  successful `kill-server`. Verified directly: created a session, ran
  `kill-server` (which reported success), and the socket file was still present
  on disk afterward — only a subsequent connection attempt reveals it's dead.
  This means a large count of `stay-test-*` sockets is not evidence of leaked
  teardown — it is tmux's normal, expected behavior on every successful test
  run, guard or no guard. The actual thing TODO-010 needs to detect is narrower:
  sockets whose server process is still alive (a genuine orphan), not the far
  larger and totally benign pile of dead socket files a correctly-guarded test
  suite leaves behind by design.
- Confirmed a hard-killed test binary does leave a live orphan, not just a dead
  socket: spawned a tmux server from a subshell, then SIGKILLed the subshell
  (the process that ran the `new-session -d` command, standing in for a
  SIGKILLed test binary) — the tmux server process kept running afterward (`ps`
  still showed it, and its session was still listable). tmux servers detach and
  reparent; killing whatever spawned them does not kill them. This is the actual
  leak TODO-010 exists to catch.

Dependencies:

- None.

Scope:

- Decide and implement a sweep mechanism, run at the start of the test binary (a
  `#[ctor]`-style once-per-process hook is heavier than needed here; a plain
  function called from a `OnceLock`-guarded setup at the top of each integration
  test binary's first test, or a dedicated `just` recipe step run before
  `cargo test`, are both simpler fits for this codebase's existing patterns —
  pick whichever composes better with the five different test files' existing
  independent `unique_namespace()`/`ServerGuard` helpers rather than requiring
  them to share new infrastructure mid-task).
- The sweep must distinguish live orphans from ordinary dead sockets (per the
  evidence above — most `stay-test-*` sockets on disk at any time are expected
  and harmless): for each socket file matching `stay-test-*` under the tmux
  socket directory, attempt a bounded `tmux -L <name> list-sessions` (or
  equivalent) probe; if it fails with "no server running," the socket is dead —
  safe to unlink and not otherwise interesting. If it succeeds (or reports
  actual session data), the server is a live orphan — kill it via
  `tmux -L <name> kill-server`, the clean, in-band way; no PID lookup or SIGKILL
  is needed at all, since the socket connection itself already proves liveness
  and `kill-server` operates entirely through the socket, not a PID. (If a PID
  were ever needed for some other reason, tmux exposes its own server PID
  directly as the `pid` format variable — confirmed live via
  `tmux -L <name> display-message -p '#{pid}'`, which returned the exact value
  independently observed via `ps` — so no process-argv scanning would be needed
  for that either.)
- Age-gate the sweep so it never touches a socket from a test run that might
  still be in progress (e.g. only sweep sockets whose containing directory
  entry's mtime is older than some conservative threshold, or — simpler and more
  robust — only sweep at the very start of a fresh test invocation, before any
  of the current run's own tests have created their own namespaces, so there's
  no risk of a same-run collision). v1's "PID-liveness-gated" wording suggests
  it checked process liveness rather than age; the in-band `list-sessions` probe
  above is a tmux-native equivalent that doesn't need PID bookkeeping at all,
  since tmux's own refusal to connect to a dead socket *is* the liveness check —
  worth confirming this is an acceptable substitution for v1's exact mechanism,
  or matching v1's PID approach instead, before implementation.
- Prefix scope: sweep only `stay-test-*`-prefixed sockets (matching every
  existing test's own naming convention) — never touch `stay` (the real
  production namespace) or any non-stay tmux socket a user's own tmux usage
  might have created in the same directory.
- Tests: a sweep run against a directory containing (a) a dead socket file with
  no server, (b) a live orphaned server with no test currently watching it, and
  (c) a non-`stay-test-`-prefixed socket, reaps only (b) (or (a)+(b) if
  dead-socket cleanup is included — decide and document which) and never touches
  (c).

Acceptance criteria:

- Running the test suite after a simulated hard-kill (spawn a `stay-test-*`
  server, kill the spawning process without giving its `Drop` guard a chance to
  run, confirm the server is still alive, then run the sweep) leaves no live
  `stay-test-*` tmux server running afterward.
- The sweep never touches the production `stay` namespace or any socket outside
  the `stay-test-*` prefix.
- A normal, non-orphaned test run's own in-progress or just-finished namespaces
  are never mistakenly swept mid-run.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-036 - README note: recovering a manually deleted tmux socket via SIGUSR1

State: NEW

Goal:

- Implement a corrected TODO-009: document, in the eventual README, that if a
  user manually deletes tmux's own server socket file while stay-managed
  sessions are running, the fix is `kill -USR1 <tmux-server-pid>` (the server's
  PID is retrievable via `tmux -L stay display-message -p '#{pid}'` while the
  socket is still present, or — if the socket is already gone by the time the
  user notices — via `ps`/`pgrep` for the `tmux -L stay` process, since the
  running server process itself never went away, only its socket file did). This
  recreates the socket file in place; no session is destroyed, no data is lost,
  and no stay-side code is needed to implement the recovery itself — only to
  document it correctly, since it's easy to assume that "no stay daemon" implies
  "no recovery," when tmux already solves this on its own.

Correction — TODO-009's premise, as originally written, is factually wrong:

- TODO-009 as it currently reads in `design_docs/stay.html` states stay has "no
  equivalent to v1's SIGUSR1 recreate-socket recovery" and that the only remedy
  for a manually deleted tmux socket is restarting the server, destroying every
  session under it. This was checked and found wrong during planning: tmux
  itself already provides exactly this recovery, natively, with no stay-side
  code needed at all. `man tmux` states plainly: "If the socket is accidentally
  removed, the SIGUSR1 signal may be sent to the tmux server process to recreate
  it." Confirmed live end-to-end: created a session, deleted its socket file,
  confirmed reconnection failed, sent `SIGUSR1` to the server's PID, and the
  socket reappeared with the session immediately reconnectable and completely
  unharmed — no data loss, no restart, nothing destroyed. tmux's own `CHANGES`
  file dates this feature to the 0.8→0.9 release (2009): "Recreate the server
  socket on SIGUSR1" — it is not new, fragile, or version-gated near stay's 3.2
  floor. This task corrects the doc's claim, not merely strikes it.

Dependencies:

- None — this is documentation-only, no code change. It has no dependency on any
  other task, but practically speaking it makes most sense to land alongside
  whichever task first creates the actual README (there is no top-level
  `README.md` in the repo yet — this project's user-facing docs currently live
  entirely in `design_docs/stay.html`, which is itself framed throughout as a
  design/planning document, not end-user documentation). Whether to create a
  minimal README now just to hold this one note, or defer the whole README's
  creation to a dedicated later task and fold this note in then, is a sequencing
  call for the implementer to make explicitly rather than silently either
  blocking on or duplicating README-creation work.

Scope:

- A new top-level `README.md` (or, if a README-creation task is already planned
  elsewhere, that task's scope) containing a short note along these lines: "If
  you manually delete tmux's own server socket file while a stay session is
  running, the session is not lost — send `SIGUSR1` to the running tmux server
  process (`kill -USR1 <pid>`) and tmux will recreate the socket in place, with
  all sessions intact. The server's PID can be found via
  `tmux -L stay display-message -p '#{pid}'` before the socket is lost, or via
  `ps`/`pgrep` for a `tmux -L stay` process afterward, since the server process
  itself is unaffected by the socket file's deletion."
- `design_docs/stay.html`: correct the TODO-009 body itself (both the index-list
  summary and the full body entry) to state the actual, verified recovery
  mechanism instead of "no equivalent... no stay-side recovery mechanism" — then
  strike/update per the doc's existing convention once the README note exists.

Acceptance criteria:

- The README (wherever it lands) contains a clear, findable, and accurate note
  stating that a manually deleted tmux socket is recoverable via `SIGUSR1` with
  no data loss, and how to find the server's PID to send it.
- `design_docs/stay.html`'s TODO-009 text no longer claims "no stay-side
  recovery mechanism" exists.
- No code change; no test gate beyond whatever doc-lint gates already run
  (`just qcheck` already includes markdown lint/format if the note lands in a
  `.md` file).

## TASK-037 - publish to crates.io as `stay`

State: NEW

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
- No `LICENSE` file exists in the repo root yet. **Decided: MIT.** Use
  `license = "MIT"` in `Cargo.toml` and add a standard MIT `LICENSE` file at the
  repo root naming the copyright holder as Nev Delap and the current year.
- `git remote -v` shows an existing GitHub remote
  (`git@github.com:nevdelap/stay.git`) — the natural value for `Cargo.toml`'s
  `repository` field, once the repo's public visibility is confirmed (this is
  presumably a private/local repo today; confirm its visibility before pointing
  a published crate's metadata at it).

Dependencies:

- Sequencing note (a judgment call for the operator, not a hard blocker): the
  other tasks in this batch (TASK-029 through TASK-036) are all pre-1.0
  feature/fix work on an unreleased tool; TODO-011 is the first "ship it
  publicly" task in this batch. Publishing before the still-open
  TODO-002/004/005/006/007 land would mean crates.io users install a CLI whose
  surface is still actively changing (attach modifiers currently parse but error
  "not yet implemented" for several flags). Nothing technically blocks
  publishing early — crates.io has no concept of "pre-release feature
  completeness" — but publish timing relative to the rest of this backlog is
  worth an explicit decision rather than defaulting to "whenever this task's
  turn comes up."

Scope:

- `Cargo.toml`: add `description`, `license = "MIT"`, and `repository`. Consider
  `keywords` and `categories` (both optional, but they improve crates.io
  discoverability and cost nothing).
- A new `LICENSE` file at the repo root: standard MIT license text, copyright
  holder Nev Delap.
- A `justfile` recipe (e.g. `release` or `publish`), mirroring the existing
  `build-release`/`update-lock` pattern, wrapping `cargo publish --dry-run` for
  local verification before the real `cargo publish`, since there is no
  publish-shaped recipe today.
- Decide and record whether publishing is a manual, deliberate
  `just publish`-style step (matching this project's existing "no daemon, no
  hidden automation" philosophy) or something wired into CI on a tag push — the
  existing `.github/workflows/ci.yml` only runs tests today, with no
  release/publish job, and there's no existing precedent in this repo either
  way.

Acceptance criteria:

- `Cargo.toml` carries complete publish metadata (`description`,
  `license = "MIT"`, `repository`); a `LICENSE` file exists at the repo root
  with standard MIT text.
- `cargo publish --dry-run` succeeds locally.
- `stay` is published on crates.io and `cargo install stay` installs a working
  binary.
- The publish mechanism (manual vs. CI-tag-triggered) is decided and documented,
  not left implicit.
- `just qcheck` and `just mac-qcheck` both pass (unaffected by this task, but
  still the standing gate).
