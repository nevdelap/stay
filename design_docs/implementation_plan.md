# Implementation Plan

This file is the task source of truth for orchestrated `orc` work.

Before starting the orchestrator for a new change, add one `NEW` task
under `Tasks`. Task scoping, the task template, the commit contract,
and the task-state rules live in `design_docs/team_specification.md`.

## Tasks

The tasks below implement milestone 1 ("Skeleton") of the `stay` v2
rewrite plan in `design_docs/stay.md`: a buildable Cargo project with
its quiet Just recipes, a tmux presence/version gate, config loading,
the CLI shape and its flag-combination validation matrix, and session
name validation. Later milestones (2-9 in `stay.md`) are not yet
broken into tasks here.

## TASK-001 - repository and build skeleton, tmux version gate

State: NEW

Goal:
- Establish a buildable Rust CLI skeleton for `stay` v2, the quiet Just
  recipes the team specification's verification workflow depends on,
  and a startup check that fails clearly when `tmux` is missing or
  older than the minimum version this task determines is actually
  required.

Scope:
- New Cargo project at the repo root: `Cargo.toml` (binary crate
  `stay`), `src/main.rs`, `.gitignore` for `target/`.
- `rustfmt.toml` and a clippy lint gate sufficient to back
  `qformat`/`qlint` (project-level lint config, not source-level
  `#![deny]` attributes).
- `justfile` at the repo root with `qformat`, `qlint`, `qtest`,
  `qcheck` recipes (quiet variants that write full output to
  `check.log`, per `design_docs/team_specification.md`), plus their
  non-quiet counterparts (`format`, `lint`, `test`, `check`) for
  interactive use.
- A tmux version-gate module: runs `tmux -V`, parses the version
  string, and fails with a clear non-zero-exit stderr message if
  `tmux` is not on `PATH` or is older than the minimum version this
  task determines is required.
- Resolve `stay.md`'s "Version gating" open risk before hardcoding a
  number: confirm, from tmux's own `CHANGES` file or release notes,
  which tmux version actually introduced the `ignore-size` client
  flag, the `pane_dead_status`/`pane_dead_time` format variables, and
  `remain-on-exit`, and set the check's minimum version to the highest
  of those (`stay.md`'s "≥ 3.2" is a placeholder needing confirmation,
  not a given). Record the confirmed minimum and the reasoning in a
  short note (a code comment at the check, or a short addition to
  `design_docs/stay.md`'s version-gating risk entry).
- Apply an explicit timeout to the `tmux -V` subprocess call, per
  `stay.md`'s "Robustness against a wedged tmux server" open risk
  (decide and implement a concrete mechanism, e.g. spawn plus a
  deadline-bounded wait that kills the child past it). This
  establishes the timeout mechanism later milestones' `tmux.rs`
  wrapper is expected to reuse, not re-decide.
- `main.rs` runs the version gate at startup before any other
  behavior; no CLI flags are parsed yet (that begins at TASK-003).

Acceptance criteria:
- `cargo build` and `cargo run` succeed from a clean checkout.
- `just qcheck` passes with no further file changes on a second run.
- Running the binary with `tmux` absent from `PATH` exits non-zero
  with a clear stderr message naming the missing dependency.
- Running the binary against a stubbed/faked older-tmux-version test
  double exits non-zero with a clear stderr message naming the
  required minimum version, without requiring an actual older tmux
  binary to be installed.
- Running the binary against the real installed tmux succeeds past
  the check.
- Unit tests cover version-string parsing (including malformed or
  unexpected `tmux -V` output) and the pass/fail threshold logic.

## TASK-002 - config loading

State: NEW

Goal:
- Implement `stay` v2's config loading exactly as specified in
  `design_docs/stay.md`'s "Config & CLI" section: TOML file plus env
  var overrides, same precedence as v1 (env > config file > default),
  same per-platform file location convention.

Scope:
- `src/config.rs`: a `Config` struct with fields `default_command`,
  `detach_key`, `copy_mode_key`, `history_lines`; TOML parsing via
  `serde` + `toml`.
- Env var overrides: `STAY_DETACH_KEY`, `STAY_COPY_MODE_KEY`,
  `STAY_CMD`, `STAY_HISTORY_LINES`, applied with precedence env >
  config file > default.
- Config file path resolution via the `dirs` crate: Linux
  `$XDG_CONFIG_HOME/stay/config.toml`, else
  `~/.config/stay/config.toml`; macOS
  `~/Library/Application Support/stay/config.toml` unconditionally
  (does not honor `XDG_CONFIG_HOME`).
- `default_command` fallback is `$SHELL`, falling back further to
  `/bin/sh -i` if `$SHELL` is unset — never a hardcoded `"bash"`.
- `history_lines` accepts the literal string `"unlimited"`, mapped to
  a documented large finite ceiling (e.g. `1_000_000`), as well as any
  concrete positive integer.
- Detach-key/copy-mode-key collision validation: reject at
  config-load time with a clear error naming both keys if
  `detach_key` and `copy_mode_key` resolve to the same control byte.
- Parsing for key-spec strings such as `'Ctrl+\'` / `'Ctrl+Space'`
  into the control byte(s) they represent.
- `Cargo.toml` gains `serde`, `toml`, `dirs` dependencies.

Acceptance criteria:
- Unit tests cover: defaults with no config file and no env vars
  (including `$SHELL` substitution and the `/bin/sh -i` fallback when
  `$SHELL` is unset); config-file values applied when present; env
  vars overriding config-file values; the documented precedence for
  every field; the `"unlimited"` mapping for `history_lines`; the
  detach/copy-mode key collision error; a malformed-TOML file
  surfaced as a clear error rather than a panic.
- `just qcheck` passes with no further file changes on a second run.
- This task's tests exercise `config.rs` directly; wiring config into
  a CLI is out of scope here (see TASK-003).

## TASK-003 - CLI shape and flag validation matrix

State: NEW

Goal:
- Finalize and implement `stay` v2's CLI flag set with `clap`, plus
  its flag-combination validation matrix, per `design_docs/stay.md`'s
  "CLI flag validation" section — turning "free to evolve" into a
  concrete, tested first shape.

Scope:
- `src/cli.rs`: clap definitions for every flag `stay.md`'s session
  lifecycle section names: session name (positional), trailing
  command words (positional, variadic), `-c <cwd>`, `-L <file>` (log
  path), `-t` (truncate, requires `-L`), `-s` (ANSI-stripped, requires
  `-L`), `-k` (kill), `-r` (read-only attach), `-l` (low-priority
  attach), `-f` (force recreate), `-p` (pass-through),
  `--prompt-integration`.
- Validation matrix, matching v1's posture as described in `stay.md`:
  - `-t` / `-s` require `-L`.
  - `-k` is mutually exclusive with the other action flags (`-r`,
    `-l`, `-f`, `-p`).
  - `-r` / `-p` are mutually exclusive.
  - Action flags require a session name to be given.
  - `--prompt-integration` is mutually exclusive with every other
    flag and with the session-name/command positionals.
  - Trailing command words are rejected when combined with any of
    `-k` / `-r` / `-l` / `-f` / `-p` (existing-session-only
    operations), matching `stay.md`'s "reject trailing command words
    on an existing session" (v1's "U1"). Only the flag-shape rule is
    in scope here; the "does this session already exist" runtime
    check is a later milestone.
- Clear, specific error messages naming the conflicting flags for
  every rejected combination.
- `main.rs` wires `cli.rs` parsing in after the TASK-001 version gate;
  no session/tmux orchestration logic yet (that begins at milestone
  2) — a parsed-result debug print or an explicit "not yet
  implemented" placeholder is sufficient dispatch for now.

Acceptance criteria:
- Tests (via clap's parse-from-args testing pattern; no real tmux
  needed) cover: every legal flag combination parses; every illegal
  combination listed above is rejected with a message naming the
  conflicting flags; `--help` lists all flags.
- `just qcheck` passes with no further file changes on a second run.

## TASK-004 - session name validation

State: NEW

Goal:
- Implement `stay` v2's session-name validation exactly as
  `design_docs/stay.md` specifies under "Create": reject (never
  silently map or mangle) names containing tmux's own disallowed
  characters, or any other control/ESC byte, per v1's broader posture
  carried forward for the ratatui picker's rendering safety.

Scope:
- A standalone, directly unit-testable validation function (e.g.
  `src/session_name.rs`, `validate_session_name(&str) -> Result<(),
  SessionNameError>`; exact name/location at implementer's
  discretion) — not embedded inline in CLI or session-creation code,
  since session creation itself is a later milestone.
- Rejects: `.`, `:`, a bare `\n` (tmux's own restriction), and any
  other ASCII control byte (`0x00`-`0x1F`, `0x7F`) including bare
  `ESC` (`0x1B`) — v1's broader control-byte rejection, carried
  forward here.
- A clear, specific error identifying which character and at what
  position triggered rejection.
- Wired into `cli.rs` (from TASK-003) so a session-name argument is
  validated at parse time, surfacing the error immediately rather
  than only once session creation exists.

Acceptance criteria:
- Unit tests cover: ordinary valid names accepted; each of `.`, `:`,
  bare `\n` rejected; representative control bytes (e.g. `\x01`, ESC
  `\x1B`, DEL `\x7F`) rejected; a valid name with a disallowed
  character at a boundary position (start/end) still rejected; error
  messages identify the offending character and position.
- `just qcheck` passes with no further file changes on a second run.
