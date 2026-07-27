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

## TASK-027 - long forms; drop --alt-screen; swap -l/-L; flip -s/--ansi-stripped to --raw

State: NEW

Goal:

- Implement TODO-018: every short CLI option has a discoverable long spelling in
  `--help` and validation errors, so commands are self-documenting.
- Remove the redundant `--alt-screen` flag (the Auto probe already uses the
  alternate screen when the terminal supports it); keep `--no-alt-screen` as the
  one escape hatch.
- Swap `-l` and `-L` so `-l` means log and `-L` means low-priority (lowercase
  for the common log case).
- Flip the logging escape flag: the default log is clean text (tmux's default
  `capture-pane` output), and `-s`/`--ansi-stripped` (opt into clean) becomes
  the long-only `--raw` (opt into capturing ANSI escapes). stay requests clean
  output from tmux; it does not strip, so `--ansi-stripped` was a misnomer — and
  clean is tmux's default, not a special mode.
- Record the decision that recreate keeps `-f`/`--force-recreate` and `-r` stays
  `--read-only`.

Dependencies:

- None. The `-l`/`-L` swap changes the low-priority spelling that TODO-002's
  text references; TODO-002 is still open, so updating its text here (in
  `stay.html`) neither blocks on it nor is blocked by it.

Scope:

- `src/cli.rs`:
  - Add `long = "..."` to the short-only options: `cwd` -> `--cwd`, `truncate`
    -> `--truncate`, `kill` -> `--kill`, `read_only` -> `--read-only`,
    `force_recreate` -> `--force-recreate`, `pass_through` -> `--pass-through`.
  - Swap the log/low-priority shorts and add their long forms: `log_path` ->
    `short = 'l', long = "log"`; `low_priority` ->
    `short = 'L', long = "low-priority"`.
  - Flip the logging escape flag: rename the `ansi_stripped` field to `raw`
    (`true` = capture ANSI escapes; `false`/default = clean text from tmux),
    give it `long = "raw"`, and drop the `-s` short (long-only, like
    `--prompt-integration`/`--no-alt-screen`). Update its requires-`--log`
    validation message from `-s/--ansi-stripped requires ...` to
    `--raw requires -l/--log`.
  - Remove the `alt_screen` field, the `no_alt_screen && alt_screen` conflict,
    the `alt_screen` term in the picker-only guard (and fix its message to name
    only `--no-alt-screen`), and the `alt_screen` term in the
    `prompt_integration` exclusivity check.
  - Update validation strings for the swap: `-L/--log` -> `-l/--log`;
    `-l/--low-priority` -> `-L/--low-priority`.
  - Update tests: extend `help_lists_the_complete_flag_shape` to assert the long
    forms (and `--raw`, no longer `-s`); swap the `-L`/`-l` cases in
    `legal_combinations_parse` and change its `-s`/`--ansi-stripped` case to
    `--raw`; update `required_log_flag_is_named` to expect `-l/--log` and to
    drive the escape flag as `--raw`; delete
    `screen_mode_flags_are_mutually_exclusive`; drop `--alt-screen` from the
    picker-only test and fix its message assertion.
- `src/main.rs`: collapse the screen decision to
  `if cli.no_alt_screen { ForceMainScreen } else { Auto }`.
- `src/picker/mod.rs`: drop the `ForceAlternateScreen` variant from
  `ScreenPreference` and its arm in `resolve_screen_mode`; update the doc
  comments; convert or remove the force-alternate tests while keeping coverage
  that the picker enters the alternate screen via the Auto probe path.
- `design_docs/stay.html`: swap `-l`/`-L` meanings throughout (the stay log flag
  `-L` -> `-l`; low-priority `-l` -> `-L`), leaving `tmux -L stay` socket
  references untouched; update the Logging section heading/id and examples;
  document the logging escape flag as `--raw` (default log is clean text from
  tmux; `--raw` captures ANSI escapes), replacing `-s`/`--ansi-stripped`; remove
  the `--alt-screen` sentence from the screen-mode explanation; update
  TODO-002's `-l` low-priority references to `-L`; record the
  recreate-keeps-`-f` decision in the TODO-018 body.

Acceptance criteria:

- `stay --help` lists a long form beside each short option: `-c, --cwd`,
  `-l, --log`, `-t, --truncate`, `-k, --kill`, `-r, --read-only`,
  `-L, --low-priority`, `-f, --force-recreate`, `-p, --pass-through`. The
  logging escape flag is the long-only `--raw` (no `-s`).
- `-l` now takes the log FILE and `-L` is the low-priority switch: e.g.
  `stay -l out.log -t --raw work` parses, `stay -L work` parses, and the long
  forms behave the same (`--log`, `--low-priority`). The log-required errors
  name `-l/--log` (and `--raw requires -l/--log`); the action errors name
  `-L/--low-priority`.
- `--alt-screen` is gone (absent from help; rejected as unknown), while
  `--no-alt-screen` still works and is still picker-only.
- Recreate keeps `-f`/`--force-recreate`; `-r`/`--read-only` is unchanged.
- The picker still enters the alternate screen when the probe succeeds (Auto);
  that path stays tested.
- `just qcheck` and `just mac-qcheck` both pass.

## TASK-028 - subcommand CLI (list/create/attach/kill) and `stay list --json`

State: NEW

Goal:

- Implement TODO-017: an explicit, scriptable command surface. The picker (bare
  `stay` on a TTY) stays the primary interactive way to work; `stay list`,
  `stay create`, `stay attach`, and `stay kill` serve scripting and advanced
  use.
- `stay list` prints the same name-first, padded status rows as the picker —
  color when stdout is a terminal, plain when piped. `stay list --json` emits
  one stable JSON document for scripts and pipes.
- Move from the flat `stay <name> [flags]` form to subcommands; the bare
  `stay <name>` attach-or-create shorthand is removed (fully explicit).
  `stay attach` is strict: it errors if the named session does not exist.
- Replace the no-argument non-TTY inventory with an error pointing at
  `stay list`, so there is exactly one listing contract.

Dependencies:

- TASK-027. The subcommand refactor carries TASK-027's flag decisions (the long
  forms, the `-l`/`-L` swap, the dropped `--alt-screen`) into the per-subcommand
  modifiers, so it builds on the post-TASK-027 cli.rs.

Scope:

- `src/cli.rs`: replace the flat flag struct with a clap subcommand model.
  `stay` takes an optional subcommand plus the global flag
  `--prompt-integration`; bare `stay` (no subcommand) is the picker path.
  `--no-alt-screen` stays picker-only — accepted on the bare-`stay` picker path
  and rejected when a subcommand is present, preserving today's behavior and the
  no-inert-flags rule (it has no effect on `list`/`create`/`attach`/`kill`).

  - `stay list [--json]`.
  - `stay create <name> [command...] [-c/--cwd <dir>] [-f/--force-recreate]`.
    Creates a new session; errors if `<name>` already exists unless
    `-f/--force-recreate`, which kills the existing session and recreates it.
  - `stay attach <name> [-r/--read-only] [-L/--low-priority] [-p/--pass-through] [-l/--log <file>] [-t/--truncate] [--raw]`.
    Strict — errors if the session does not exist. The attach-mode and logging
    modifiers stay parsed but, like today, error "not yet implemented" until
    their TODOs land; plain `stay attach <name>` attaches.
  - `stay kill <name>`.
  - Keep `parse_session_name` as the value parser on every subcommand's `<name>`
    so empty/invalid names are still rejected at parse time.

- `src/main.rs`: dispatch on the subcommand — `list`, `create`, `attach`, `kill`
  route to their session operations; the existing "not yet implemented" guard
  moves onto the `attach` modifiers; bare `stay` runs the picker on a TTY and
  errors ("use `stay list`") on a non-TTY.

- Listing and JSON output:

  - `stay list` (human) reuses the picker's status-row rendering — the shared
    `status_detail()` segment helper from TASK-024 — mapping segments to ANSI
    color when stdout is a terminal and to plain text otherwise.
  - `stay list --json` serializes an envelope object `{"sessions": [...]}`. Each
    session object's fields appear in this order: `name` (string), `status` (one
    of `attached`, `detached`, `terminated` — the values `status_word()`
    produces today; `broken` is not yet derived), `created_at` (ISO-8601 UTC,
    e.g. `2026-07-27T10:30:00Z`), `current_directory` (absolute path string, or
    `null`), `current_command` (foreground program name string, or `null`),
    `terminated_at` (ISO-8601 UTC, or `null`), `exit_code` (integer, or `null`).
    `current_directory` comes from tmux's live `pane_current_path` and is `null`
    for a terminated session (tmux no longer reports its pane path); it may also
    differ from the directory the session was created in once the shell has
    changed directory, so it is named `current_directory`, not
    `start_directory`. `current_command` comes from tmux's live
    `pane_current_command` — the pane's foreground program name only (e.g.
    `vim`, not its arguments; the shell such as `bash` while idle at a prompt);
    for a terminated session it holds the last foreground program name.
    `terminated_at` and `exit_code` are `null` for non-terminated sessions. JSON
    output is never ANSI-decorated. The `sessions` array is sorted by
    `created_at` ascending, then by `name` ascending, so output is deterministic
    regardless of tmux's listing order (this JSON ordering is independent of the
    name-first sort used by the picker and the human `stay list`).
    `start_command` and `log_file` are deliberately omitted: stay is stateless
    and tmux exposes neither after creation (`current_command` is the live
    stand-in for the creation command).
  - Extend `SessionRecord` with `current_directory` and `current_command` fields
    and query tmux's `pane_current_path` and `pane_current_command` in
    `list_sessions` (add both to the `list-panes -a -F` format string and
    aggregate them per session like the existing fields; map an empty
    `pane_current_path` to `null`). Add a serde serialization path for the JSON
    output (a dedicated output type or `Serialize` on the record); `serde` is
    already a direct dependency, so no new crate or `Cargo.lock` change is
    expected.

- Example `stay list --json` document (a live session running `vim` and a
  terminated build that exited non-zero; `current_directory` is `null` for the
  terminated session because tmux no longer reports its pane path):

  ```json
  {
    "sessions": [
      {
        "name": "work",
        "status": "attached",
        "created_at": "2026-07-27T10:30:00Z",
        "current_directory": "/home/nevd/work",
        "current_command": "vim",
        "terminated_at": null,
        "exit_code": null
      },
      {
        "name": "build",
        "status": "terminated",
        "created_at": "2026-07-27T09:00:00Z",
        "current_directory": null,
        "current_command": "make",
        "terminated_at": "2026-07-27T10:15:30Z",
        "exit_code": 1
      }
    ]
  }
  ```

- tests:

  - Subcommand parse surface: `stay list`, `stay list --json`, `stay create`,
    `stay attach`, `stay kill` parse; the old flat forms (`stay <name>`,
    `stay -k <name>`, `stay -f <name>`, `stay <name> <cmd>`) are rejected as
    unknown.
  - `stay list --json`: assert the envelope shape, the field order (`name`,
    `status`, `created_at`, `current_directory`, `current_command`,
    `terminated_at`, `exit_code`), all three `status` values, a live session's
    `current_directory` and `current_command` echoing the pane (allowing the
    foreground process to settle after creation — querying instantly returns a
    transient `tmux`), `null` `current_directory`/`exit_code`/`terminated_at`
    for a terminated session, a terminated session carrying a non-zero
    `exit_code` and a UTC `terminated_at`, that the array is ordered by
    `created_at` then `name`, and no ANSI bytes in the output.
  - `stay attach` errors on a missing session; `stay create` errors on an
    existing session unless `-f/--force-recreate`; bare non-TTY `stay` errors
    pointing at `stay list`.

- `design_docs/stay.html`: replace the flat `stay <name>` / flag command
  description with the subcommand surface; document the `stay list --json`
  schema (envelope, field order, status enum, `current_directory` as the live
  pane path and `current_command` as the live foreground program name, UTC
  timestamps, no-ANSI, and the deliberate omission of `start_directory`,
  `start_command`, and `log_file` because stay is stateless); record that bare
  `stay` non-TTY now errors → `stay list`; update the examples and compatibility
  notes.

Acceptance criteria:

- `stay list` prints name-first padded status rows matching the picker (colored
  on a TTY, plain when piped). `stay list --json` prints the
  `{"sessions": [...]}` document with fields in order `name`, `status`,
  `created_at`, `current_directory`, `current_command`, `terminated_at`,
  `exit_code`; `status` ∈ {`attached`, `detached`, `terminated`};
  `current_directory` is the pane's live working directory and `current_command`
  its live foreground program name (each `null` when tmux reports none);
  `exit_code` and `terminated_at` are `null` for non-terminated sessions;
  timestamps are ISO-8601 UTC; the `sessions` array is ordered by `created_at`
  then `name`; the output contains no ANSI escapes.
- `stay create <name>` creates and errors if the session exists unless
  `-f/--force-recreate`; `stay attach <name>` attaches and errors if absent;
  `stay kill <name>` kills. The old `stay <name>`, `stay -k <name>`, and
  `stay -f <name>` flat forms are rejected.
- Bare `stay` opens the picker on a TTY and errors ("use `stay list`") on a
  non-TTY.
- `--prompt-integration` still works as a global flag and `--no-alt-screen`
  stays picker-only (rejected with a subcommand); TASK-027's flag decisions are
  preserved within the subcommand modifiers (long forms present; `-l`/`--log`,
  `-L`/`--low-priority`, and long-only `--raw` on `attach`; no `--alt-screen`).
- Empty/invalid session names are still rejected at parse time on every
  subcommand.
- `just qcheck` and `just mac-qcheck` both pass.
