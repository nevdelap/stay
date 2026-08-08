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
