# Implementation Plan

This file is the task source of truth for planned project work.

Before starting a new change, add one `NEW` task under `Tasks`. The shared state
transitions, commit contract, handoff procedures, review-document format, and
verification workflow are defined in `design_docs/agent_workflow.md`; role
responsibilities are defined in `docs/roles.md`.

## Tasks

## TASK-114 - cache release Rust build artifacts

State: IMPLEMENTED

Goal:

- Reduce the time spent compiling retried tagged release builds, especially the
  expensive Frizbee-related `stay` code generation, by restoring Cargo and
  release target artifacts for a rerun of the same tag and commit.
- Keep the cache safe across the four native release targets without claiming
  that one tag's cache can be restored by a different tag.

Dependencies:

- TASK-113 must remain `COMPLETED`, because this task caches the release build
  introduced by the Frizbee picker implementation.

Design decision:

- Reuse `Swatinem/rust-cache@v2`, which is already used by the normal CI jobs,
  rather than introducing a second cache implementation. Select the installed
  Rust 1.89 toolchain before invoking the cache action so the toolchain is part
  of the cache environment, matching the action's documented usage.
- Enable `cache-workspace-crates: true`. The action's default dependency cache
  does not retain the workspace crate, but the long build is in `stay`'s
  Frizbee-instantiating release code generation. Dependency-only caching would
  not address the primary cost.
- Set `shared-key` to a release key containing both `${{ matrix.target }}` and
  `${{ github.sha }}`. The x86_64 and aarch64 targets, and the Linux and Darwin
  targets, must never restore one another's target directory. The commit
  component makes workspace-crate caching source-aware: a changed release commit
  cannot restore a prior commit's `stay` artifact, and each immutable cache key
  can save the artifacts built for that exact source revision. Keep the action's
  Rust-environment hash enabled so the Rust toolchain, `Cargo.toml`,
  `Cargo.lock`, toolchain files, and relevant Rust environment changes also
  invalidate the cache naturally.
- GitHub Actions cache access is scoped by ref. Since this workflow runs only
  for version-tag pushes, a cache from one release tag is not available to a
  different release tag. This task therefore promises reuse only for reruns of
  the same tag and commit. A trusted branch-scoped cache producer could provide
  cross-release reuse later, but is outside this task.
- Keep the existing 20-minute job timeout, target matrix, build command, binary
  smoke test, and archive contents unchanged. Caching cannot shorten the time
  spent waiting for a hosted runner or the first cold build, so those are
  explicitly outside this task.

Scope:

- Update only the `build-binaries` matrix job in
  `.github/workflows/release.yml`. Add the existing Rust cache action after
  `dtolnay/rust-toolchain@1.89.0` has selected the toolchain and before the
  release build. Configure the target-specific shared key and workspace-crate
  caching described above.
- Do not change normal CI cache configuration, the Nix jobs, the release
  timeout, the Rust toolchain, Cargo manifests, package version, source code,
  tests, or release archive packaging.
- Do not cache release archives or publish outputs. The cache is only for
  Cargo's registry, build dependencies, and target artifacts used to produce the
  current matrix job's binary.

Acceptance criteria:

- Every `build-binaries` matrix leg runs the cache action after Rust 1.89 is
  installed and before
  `cargo +1.89.0 build --release --locked --target "$TARGET"`.
- The cache configuration enables workspace-crate caching and includes both the
  matrix target and `github.sha` in its shared key. The four target legs
  therefore have independent, source-aware cache namespaces, and a rerun of the
  same tag and commit can restore the exact artifacts it previously saved.
- The task does not require cache reuse across different release tags. The plan
  documents that limitation rather than assuming that identical keys bypass
  GitHub Actions ref scoping. No cache setting permits one target architecture
  or source revision to consume another's workspace artifacts.
- The existing binary version check, tmux smoke test, archive packaging, and
  artifact upload continue to run unchanged for all four targets.
- A tagged release run or rerun provides evidence that each target can save or
  restore its cache, and a subsequent run of the same target reports an exact
  cache hit where the first run successfully saved one. The evidence records the
  target-specific cache keys and confirms that a cache hit does not skip the
  build, version check, smoke test, or packaging steps.
- The workflow passes the exact `just qlint` recipe on a clean final planning
  commit, including its actionlint, YAML, and Markdown checks. If the recipe
  formats the plan, the formatted result is committed and the recipe is run
  again until it makes no further changes. No Rust, acceptance, package-version,
  or release-content gate is required because this task changes workflow
  configuration only.

## TASK-115 - persist session definitions across reboots

State: COMPLETED

Goal:

- Keep a durable, per-user definition for every session created through stay so
  the session remains visible after the tmux server disappears during a reboot.
- Let the interactive picker recreate a saved session from its stored
  definition, without requiring the user to remember and retype its name,
  working directory, or command.

Dependencies:

- None. This task uses the existing create, inventory, picker, and recreate
  flows and does not depend on the release-cache task.

Design decision:

- Store session definitions in a separate per-user TOML file at the existing
  stay configuration directory's `sessions.toml` path (the directory resolved by
  `dirs::config_dir()`, alongside `config.toml`). Do not put runtime session
  definitions in the user-edited configuration file.
- Each entry stores the validated session name, its Unix-seconds creation time,
  the resolved working directory, and the effective command argument vector used
  to create the session. The effective vector includes the configured default
  command and shell wrapper when the user did not provide explicit command
  words, so a later recreate does not depend on a changed configuration file.
- The exact version-1 TOML schema is `version = 1` followed by zero or more
  `[[sessions]]` tables with required `name`, `created`, `cwd`, and `command`
  fields, where `command` is a non-empty array of strings. Names are the unique
  identity key. An absent file or parent directory means an empty store;
  duplicate names, missing or extra fields, invalid session names, an empty
  command, malformed TOML, or an unsupported version reject the entire store
  with a visible error and never select an arbitrary duplicate or partial
  result.
- Writes are owner-only and crash-safe on Linux and macOS: create the parent
  directory with mode 0700 when needed, create a same-directory temporary file
  with mode 0600, write the complete snapshot, call `sync_all` on the file,
  atomically rename it into place, then open and `sync_all` the parent
  directory. A failure before rename is a failed store commit: the prior store
  remains intact and no store-first tmux action may begin. A rename followed by
  a parent-directory `sync_all` failure is a distinct committed-but-uncertain
  result: the new snapshot remains in place, no rollback is attempted, and the
  caller reports that durability is uncertain. A malformed or unreadable store
  is a visible error and must not be silently replaced or discarded.
- Merge saved definitions with the live tmux inventory by session name. A live
  row supplies its current status; a saved-only row is rendered as `saved` and
  remains selectable for recreate. Live sessions created outside stay remain
  visible but do not acquire a saved definition implicitly.
- A saved-only human-readable row uses the exact suffix ` [saved]`. Its JSON row
  has `status: "saved"`, `created_at` from the stored Unix timestamp in RFC 3339
  UTC form, `current_directory` equal to the stored `cwd`,
  `current_command: null`, `terminated_at: null`, `exit_code: null`, and
  `signal: null`. Live rows retain their existing JSON semantics.
- Mutating operations use explicit ordering to avoid an unreported cross-system
  write. Create, force-recreate, and rename first commit the desired store
  snapshot and require a fully durable store result; only then do they mutate
  tmux. A pre-rename store failure or a committed-but-uncertain directory-sync
  result prevents the tmux action and reports the corresponding diagnostic. If
  the tmux operation fails after a durable store commit, they restore the prior
  durable snapshot. If that restoration fails, they retain the desired snapshot
  and report both failures. A crash between the durable store commit and tmux
  action leaves a visible, retryable saved row.
- Live-session kill and kill-all first complete the tmux kill, then remove the
  saved definition. If removal fails after tmux succeeds, the saved definition
  remains visible as a saved-only row when the removal fails before rename and
  the command reports the partial success. If removal renames successfully but
  the directory sync is uncertain, the removed snapshot remains in place, no
  rollback is attempted, and the command reports both tmux success and store
  durability uncertainty. Killing a saved-only entry removes only its store
  record and needs no tmux server, using the same pre-rename and
  committed-but-uncertain outcomes. Each kill-all target follows this contract
  independently, preserving the existing race handling and reporting every
  persistence failure. No lifecycle path silently discards a durable definition.
- Picker rename of a saved-only row is a store-only rename: validate the new
  name, reject a live or saved name collision, and commit the renamed snapshot
  without invoking tmux. A failed or uncertain store commit leaves the old row
  or reports the committed-but-uncertain result using the same rules. Live
  persisted rows retain the store-first tmux rename contract above.

Scope:

- Add a persistence module and path seam under `src/`, using the existing
  serde/TOML dependencies and test-injected paths rather than mutating the
  process-global home or configuration environment in unit tests.
- Because this task changes non-test application source under `src/`, bump the
  package patch version exactly once from the baseline `0.0.89` to `0.0.90` in
  `Cargo.toml`, update the `stay` package entry to `0.0.90` in `Cargo.lock`, and
  update the `docs/stay.1` version header. Do not change dependency versions or
  perform a second version bump.
- Integrate persistence with explicit create, force-recreate, kill, and the
  interactive picker create, rename, recreate, and kill-all paths. Preserve the
  existing confirmation and destructive-action behavior.
- Extend the tmux inventory model and renderers so saved-only sessions appear in
  `stay list`, `stay list --json`, and the picker without duplicate live rows.
  Define and document the exact saved status, field values, timestamp, and
  human-readable/picker row text in the stable JSON inventory.
- Make picker recreate consume the stored working directory and effective
  command for saved-only and live persisted sessions. Keep a saved definition
  when its working directory no longer exists, show the recreate failure, and
  leave the row available for correction or removal.
- Add focused persistence/unit/inventory tests and Linux/macOS acceptance
  coverage using isolated configuration and tmux state. Cover reboot-shaped
  startup with no server, live/saved merging, create/recreate/rename/kill
  updates, malformed and unwritable stores, missing working directories, and
  names and command arguments containing the complete supported special-
  character matrix: valid names with hyphen, underscore, and non-ASCII Unicode;
  command and directory strings with spaces, quotes, backslashes, dollar signs,
  Unicode, and empty command arguments; plus rejection of names containing
  periods, colons, controls, and Unicode bidi/line-separator characters.
- Update the named public documentation surfaces: `README.md`'s Commands, JSON
  inventory, Picker keys, and configuration sections; `docs/stay.1`'s Picker,
  list, create, kill, and configuration sections; and the generated help text in
  `src/cli.rs` plus its expectations in `tests/cli_help.rs` when lifecycle
  wording changes. Update the matching inventory and picker fixtures in
  `tests/acceptance.bats` so each surface documents or asserts the exact
  ` [saved]` row suffix, `status: "saved"` JSON semantics, `sessions.toml` path,
  and saved-only recreate, rename, and kill behavior.

Acceptance criteria:

- A successful stay-created session leaves one durable definition containing its
  name, resolved working directory, creation time, and effective command vector.
  Create, force-recreate, and rename stage the desired definition before the
  tmux action; a successful action leaves it committed, while a failed action
  restores the prior snapshot or reports the restoration failure. Duplicate
  entries are never written.
- With no running tmux server, `stay list` and the picker show every saved-only
  session exactly once with the documented saved status. `stay list --json`
  emits the documented stable saved row, and the picker can select it and
  recreate it using the stored definition without retyping the session name,
  directory, or command.
- When a saved definition and a live session have the same name, the inventory
  has one row with live tmux status and retains the saved definition for
  recreate. Live sessions with no saved definition remain visible and are not
  persisted merely by listing them.
- Successful rename and kill paths update the store consistently. Renaming a
  saved-only row updates only the store and does not require a running tmux
  server; duplicate or invalid names leave the old row intact. Killing a
  saved-only row removes it without tmux. A pre-rename persistence failure
  leaves the prior definition intact; a post-rename directory-sync failure
  leaves the new snapshot in place and reports durability uncertainty. Tmux
  failures restore the prior snapshot or report the restoration failure.
- Store writes use the documented owner-only permissions and atomic replacement
  behavior, including file `sync_all` before rename and parent-directory
  `sync_all` afterward. Filesystem seam tests prove the pre-rename failure
  boundary, the committed-but-uncertain post-rename boundary, and the exact
  diagnostics and tmux sequencing for each. Malformed, unreadable, or unwritable
  store fixtures produce a visible failure without truncating or silently
  replacing existing data.
- Version-1 TOML fixtures prove that an absent store is empty, valid entries
  round-trip exactly, duplicate names and unsupported versions are rejected,
  extra or missing fields are rejected, and malformed entries cannot partially
  populate the inventory. Saved JSON fixtures assert every field and exact
  null/value semantics listed in the design decision.
- Failure-injection tests cover store-first create/force-recreate/rename,
  tmux-failure restoration, pre- and post-rename kill-after-tmux persistence
  failures, saved-only kill and rename without a server, rename collisions, and
  kill-all continuation/reporting. Each test checks the user-visible
  partial-success or durability-uncertain diagnostic, whether tmux was invoked,
  and the resulting live and durable inventories.
- Missing saved working directories and failed recreates remain visible and
  recoverable in the picker; no failed recreate deletes the saved definition.
- The manual, help text, human-readable inventory, picker display, and JSON
  documentation agree across `README.md`, `docs/stay.1`, `src/cli.rs`,
  `tests/cli_help.rs`, and `tests/acceptance.bats` on the saved status,
  persistence location, exact row text, saved-only recreate, rename, and kill
  behavior for Linux and macOS.
- The implementation version is exactly `0.0.90`: `Cargo.toml`, the `stay`
  package entry in `Cargo.lock`, and the `docs/stay.1` header agree. The
  existing `tests/cli_help.rs` version assertion continues to compare
  `stay --version` with `env!("CARGO_PKG_VERSION")`, and acceptance/release
  version checks remain metadata-derived rather than retaining a stale literal.
- The implementation passes the exact `just qcheck`, `just mac-qcheck`,
  `just qacceptance`, and `just mac-qacceptance` recipes on the final task
  commit, plus the relevant formatting and linting checks. The acceptance
  evidence includes a fresh-process, no-tmux-server restart-shaped scenario.
